/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#![allow(non_snake_case)]

use log::{error, warn};
use std::sync::{Arc, Mutex};

#[cfg(feature = "debug_ambient")]
use log::debug;

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use std::sync::mpsc::TryRecvError;
use std::sync::mpsc::{Receiver, Sender};

use std::time::Duration;

use crate::sensors::dht::DhtError;
use crate::simulator::get_resp_sim::GetResponseFromSimulatorTrait;
use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

const LOG_TARGET_AMBIENT: &str = "ambient";

// time interval for repeating measurement
const MEASUREMENT_INTERVAL_SECS: i32 = 60;

// max loops for receiving when the quit command has been received
#[cfg(all(target_os = "linux", feature = "target_hw"))]
const RECEIVE_COUNTER_MAX: u32 = 10;

#[derive(Deserialize, Debug)]
/// Holds the configuration data for the measurement of ambient signals from DHT sensor
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
pub struct AmbientConfig {
    /// flag to determine if the application shall communicate with hardware directly or via TCP with a simulator
    pub use_simulator: bool,

    /// replacement value for ambient temperature in case reading from DHT22 sensor fails
    pub replacement_value_ambient_temperature: f32,

    /// replacement value for ambient humidity in case reading from DHT22 sensor fails
    pub replacement_value_ambient_humidity: f32,
}

pub struct AmbientChannels<'a> {
    /// Sender part of the channel for communication via TCP wrapped as optional
    pub tx_ambient_to_tcp_opt: &'a Option<Sender<InternalCommand>>,

    /// Receiver part of the channel for communication via TCP wrapped as optional
    pub rx_ambient_from_tcp_opt: &'a Option<Receiver<f32>>,

    /// Sender part of the channel for communication to Dht
    pub tx_ambient_to_dht_opt: Option<Sender<InternalCommand>>,

    /// Receiver part of the channel for communication from Dht
    pub rx_ambient_from_dht_opt: Option<Receiver<Result<(f32, f32), DhtError>>>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_ambient_to_signal_handler: Sender<bool>,

    /// Receiver part of the channel for communication from the signal handler                
    pub rx_ambient_from_signal_handler: Receiver<InternalCommand>,
}

/// Contains the communication with DHT ambient temperature and humidity sensor.
/// Alternatively, TCP communication is used when configured to run with the simulator.
/// Sensor data is read periodically and stored.
/// Requests from data logger and heating control are answered with last measured data.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     ambient[Ambient] -.-> data_logger[DataLogger]
///     ambient --> tcp_communication[TcpCommunication]
///     tcp_communication --> ambient
///     ambient -.-> heating[Heating]
///     signal_handler[SignalHandler] --> ambient
///     ambient --> signal_handler
/// ```
pub struct Ambient {
    // configuration data for Ambient
    config: AmbientConfig,

    /// ambient temperature in °C
    ambient_temperature: f32,

    /// humidity in %
    ambient_humidity: f32,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    lock_error_channel_receive_termination: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via the channel from the signal handler thread
    lock_warn_inapplicable_command_signal_handler: bool,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send request to Dht thread via the channel
    lock_error_channel_send_dht: bool,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive from Dht thread via the channel
    lock_error_channel_receive_dht: bool,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read from Dht sensor
    lock_error_dht: bool,
}

impl ProcessExternalRequestTrait for Ambient {}

impl GetResponseFromSimulatorTrait for Ambient {}

impl Ambient {
    /// Creates a new `Ambient` control instance.
    ///
    /// This constructor initializes the ambient temperature and humidity measurement module.
    /// It sets the initial temperature and humidity values to configured replacement values
    /// and initializes all internal "lock" flags to `false`. These flags help prevent
    /// log flooding from recurring errors or warnings during operation.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the Ambient module, specifying behavior like
    ///   the use of a simulator versus hardware, and replacement values for sensor readings.
    ///
    /// # Returns
    /// A new `Ambient` struct, ready to collect and provide ambient sensor data.
    pub fn new(config: AmbientConfig) -> Ambient {
        let replacement_value_ambient_temperature = config.replacement_value_ambient_temperature;
        let replacement_value_ambient_humidity = config.replacement_value_ambient_humidity;
        Ambient {
            config,
            ambient_temperature: replacement_value_ambient_temperature,
            ambient_humidity: replacement_value_ambient_humidity,
            lock_error_channel_receive_termination: false,
            lock_warn_inapplicable_command_signal_handler: false,

            #[cfg(all(target_os = "linux", feature = "target_hw"))]
            lock_error_channel_send_dht: false,

            #[cfg(all(target_os = "linux", feature = "target_hw"))]
            lock_error_channel_receive_dht: false,

            #[cfg(all(target_os = "linux", feature = "target_hw"))]
            lock_error_dht: false,
        }
    }

    /// Communicates with a simulator thread via channels to retrieve ambient temperature and humidity.
    ///
    /// This private helper function is used when the `Ambient` module is configured
    /// to run in simulation mode. It sends `RequestSignal` commands for ambient temperature
    /// and humidity to the simulator and updates the module's internal `ambient_temperature`
    /// and `ambient_humidity` fields with the responses.
    ///
    /// # Arguments
    /// * `tx_ambient_to_tcp_opt` - A reference to an `Option<Sender<InternalCommand>>` used
    ///   for sending commands to the `TcpCommunication` thread (which acts as the simulator interface).
    /// * `rx_ambient_from_tcp_opt` - A reference to an `Option<Receiver<f32>>` used for
    ///   receiving simulated sensor data (temperature and humidity) from the `TcpCommunication` thread.
    ///
    /// # Panics
    /// This function will **panic** if either `tx_ambient_to_tcp_opt` or `rx_ambient_from_tcp_opt`
    /// are `None`. This is because the `Ambient` module's `execute` function is expected to
    /// perform checks beforehand to ensure these channels are `Some` when running in simulator mode.
    fn communicate_with_simulator(
        &mut self,
        tx_ambient_to_tcp_opt: &Option<Sender<InternalCommand>>,
        rx_ambient_from_tcp_opt: &Option<Receiver<f32>>,
    ) {
        // unwrap is safe because tested beforehand
        let tx_ambient_to_simulator = tx_ambient_to_tcp_opt.as_ref().unwrap();
        let rx_ambient_from_simulator = rx_ambient_from_tcp_opt.as_ref().unwrap();

        self.ambient_temperature = Self::get_response_from_simulator(
            tx_ambient_to_simulator,
            rx_ambient_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::AmbientTemperature),
        );
        self.ambient_humidity = Self::get_response_from_simulator(
            tx_ambient_to_simulator,
            rx_ambient_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::AmbientHumidity),
        );
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Sends a request to the DHT sensor thread to perform a new ambient temperature and humidity measurement.
    ///
    /// This private helper function is used when the `Ambient` module is configured
    /// to interact with physical hardware. It sends an `InternalCommand::RequestSignal`
    /// for `AquariumSignal::AmbientTemperature` (implying both temp and humidity will be read)
    /// to the DHT sensor thread.
    ///
    /// It updates an internal lock flag (`lock_error_channel_send_dht`) to prevent
    /// repeated error logging if the channel send operation fails.
    ///
    /// # Arguments
    /// * `tx_ambient_to_dht` - A reference to the sender part of the channel, used to
    ///   send commands to the DHT sensor thread.
    ///
    /// # Returns
    /// `true` if the request was successfully sent; `false` if the channel send operation failed.
    fn send_request_to_dht(&mut self, tx_ambient_to_dht: &Sender<InternalCommand>) -> bool {
        match tx_ambient_to_dht.send(InternalCommand::RequestSignal(
            AquariumSignal::AmbientTemperature,
        )) {
            Ok(()) => {
                self.lock_error_channel_send_dht = false;

                #[cfg(feature = "debug_ambient")]
                debug!(
                    target: LOG_TARGET_AMBIENT,
                    "sent request to Dht"
                );

                true
            }
            Err(e) => {
                if !self.lock_error_channel_send_dht {
                    error!(
                        target: LOG_TARGET_AMBIENT,
                        "sending request to Dht thread failed. ({e:?})"
                    );
                    self.lock_error_channel_send_dht = true;
                }
                false
            }
        }
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Receives and processes measurement responses from the DHT sensor thread.
    ///
    /// This private helper function is used when the `Ambient` module is configured
    /// for hardware interaction. It attempts to receive a `Result` without blocking
    /// containing ambient temperature and humidity data from the DHT thread.
    ///
    /// If successful, it updates the module's internal `ambient_temperature` and
    /// `ambient_humidity` fields. It manages various lock flags (`lock_error_dht`,
    /// `lock_error_channel_receive_dht`) to prevent log flooding from sensor
    /// reading errors or channel disconnections.
    ///
    /// # Arguments
    /// * `rx_ambient_from_dht` - A reference to the receiver part of the channel, used to
    ///   get measurement results from the DHT sensor thread.
    ///
    /// # Returns
    /// `true` if a response was expected but not yet received (meaning `receive_response_from_dht`
    /// should be called again soon, e.g., on the next loop iteration); `false` if a response
    /// was successfully received and processed, or if a non-empty error occurred.
    fn receive_response_from_dht(
        &mut self,
        rx_ambient_from_dht: &Receiver<Result<(f32, f32), DhtError>>,
    ) -> bool {
        #[cfg(feature = "debug_ambient")]
        debug!(
            target: LOG_TARGET_AMBIENT,
            "checking if response from dht is available",
        );

        match rx_ambient_from_dht.try_recv() {
            Ok(response) => match response {
                Ok((ambient_temperature, ambient_humidity)) => {
                    #[cfg(feature = "debug_ambient")]
                    debug!(
                        target: LOG_TARGET_AMBIENT,
                        "received response from dht: temperature={}, humidity={}",
                        ambient_temperature,
                        ambient_humidity
                    );

                    self.ambient_temperature = ambient_temperature;
                    self.ambient_humidity = ambient_humidity;
                    self.lock_error_dht = false;
                    false // allow for the next request to be sent
                }
                Err(e) => {
                    if !self.lock_error_dht {
                        error!(
                            target: LOG_TARGET_AMBIENT,
                            "error when reading from DHT sensor ({e:?})"
                        );
                        self.lock_error_dht = true;
                    }
                    false // allow for the next request to be sent
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => {
                        /* empty buffer - no action required */
                        self.lock_error_channel_receive_dht = false;
                    }
                    TryRecvError::Disconnected => {
                        if !self.lock_error_channel_receive_dht {
                            error!(
                                target: LOG_TARGET_AMBIENT,
                                "error occurred when trying to receive response from Dht ({e:?})"
                            );
                            self.lock_error_channel_receive_dht = true;
                        }
                    }
                }
                true // repeat receiving
            }
        }
    }

    /// Executes the main control loop for the ambient temperature and humidity sensor module.
    ///
    /// This function runs continuously, managing the periodic measurement and provision of
    /// ambient temperature and humidity data. It operates in two main modes:
    /// - **Simulator Mode**: Fetches simulated sensor data via a TCP communication channel.
    /// - **Hardware Mode (Linux with `target_hw` feature)**: Interacts directly with a DHT sensor
    ///   thread, sending requests and receiving actual sensor readings.
    ///
    /// The collected data is then written to shared mutexes, making it available to other
    /// application threads. The loop maintains a measurement countdown and remains
    /// responsive to `Quit` and `Terminate` commands from the signal handler for graceful shutdown.
    ///
    /// # Arguments
    /// * `ambient_channels` - A struct containing all `mpsc` channels necessary for inter-thread
    ///   communication with the `TcpCommunication` (if in simulator mode), `Dht` (if in hardware mode),
    ///   and the `SignalHandler`.
    /// * `mutex_ambient_temperature` - An `Arc<Mutex<f32>>` into which the latest ambient
    ///   temperature reading will be written, ensuring thread-safe access for other modules.
    /// * `mutex_ambient_humidity` - An `Arc<Mutex<f32>>` into which the latest ambient
    ///   humidity reading will be written, ensuring thread-safe access for other modules.
    ///
    /// # Panics
    /// This function will panic if:
    /// - It's configured to use the simulator but essential TCP communication channels are missing.
    /// - It's configured to use the simulator but DHT channels are unexpectedly present.
    /// - During hardware mode, a fatal error occurs while trying to receive a response from the DHT thread (e.g., channel disconnected permanently).
    pub fn execute(
        &mut self,
        ambient_channels: AmbientChannels,
        mutex_ambient_temperature: Arc<Mutex<f32>>,
        mutex_ambient_humidity: Arc<Mutex<f32>>,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let sleep_duration_one_millis = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        cfg_if::cfg_if! {
            if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
                let mut state_send_receive: bool = false;
                let mut receive_counter: u32 = 0;
            }
        }

        // initialize signals with replacement values
        self.ambient_temperature = self.config.replacement_value_ambient_temperature;
        self.ambient_humidity = self.config.replacement_value_ambient_humidity;

        // initialize countdown
        let mut measurement_countdown: i32 = 0;

        if (self.config.use_simulator)
            && (ambient_channels.tx_ambient_to_tcp_opt.is_none()
                | ambient_channels.rx_ambient_from_tcp_opt.is_none())
        {
            panic!(
                "{}: configured to run with simulator, but no channel to TCP thread provided.",
                LOG_TARGET_AMBIENT
            );
        }

        if (self.config.use_simulator)
            && ((ambient_channels.tx_ambient_to_dht_opt.is_some())
                | (ambient_channels.rx_ambient_from_dht_opt.is_some()))
        {
            panic!(
                "{}: configured to run with simulator, but channels to Dht thread provided",
                LOG_TARGET_AMBIENT
            );
        }

        #[cfg(not(target_os = "linux"))]
        if ambient_channels.tx_ambient_to_dht_opt.is_some()
            || ambient_channels.rx_ambient_from_dht_opt.is_some()
        {
            warn!(target: LOG_TARGET_AMBIENT,
                "Sender/Receiver to Dht are not 'None'");
        }

        loop {
            let (
                quit_command_received, // the request to end the application has been received
                _,
                _,
            ) = self
                .process_external_request(&ambient_channels.rx_ambient_from_signal_handler, None);

            cfg_if::cfg_if! {
                if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
                    if quit_command_received && (!state_send_receive || receive_counter > RECEIVE_COUNTER_MAX) {
                        // quit command has been received, and there is no pending measurement
                        // no further waiting for Dht is required
                        break;
                    }
                }
                else {
                    if quit_command_received {
                        break;
                    }
                }
            }

            if measurement_countdown > 0 {
                measurement_countdown = measurement_countdown.saturating_sub(1);
            } else {
                if !self.config.use_simulator {
                    #[cfg(all(target_os = "linux", feature = "target_hw"))]
                    {
                        if ambient_channels.tx_ambient_to_dht_opt.is_some()
                            && ambient_channels.rx_ambient_from_dht_opt.is_some()
                        {
                            if !state_send_receive {
                                let tx_ambient_to_dht =
                                    ambient_channels.tx_ambient_to_dht_opt.as_ref().unwrap();

                                state_send_receive = self.send_request_to_dht(tx_ambient_to_dht);

                                #[cfg(feature = "debug_ambient")]
                                {
                                    if state_send_receive {
                                        debug!(
                                            target: LOG_TARGET_AMBIENT,
                                            "switching to receiving mode"
                                        );
                                    }
                                }
                            } else {
                                // read from the channel if a measurement result is available from Dht thread
                                let rx_ambient_from_dht =
                                    ambient_channels.rx_ambient_from_dht_opt.as_ref().unwrap();

                                state_send_receive =
                                    self.receive_response_from_dht(rx_ambient_from_dht);

                                if !state_send_receive {
                                    // data could be received successfully
                                    measurement_countdown = MEASUREMENT_INTERVAL_SECS * 10;
                                    receive_counter = 0;
                                } else {
                                    receive_counter = receive_counter.saturating_add(1);
                                }

                                #[cfg(feature = "debug_ambient")]
                                {
                                    if (receive_counter % 10 == 0) && state_send_receive {
                                        debug!(
                                            target: LOG_TARGET_AMBIENT,
                                            "tried to receive from Dht (counter={})",
                                            receive_counter
                                        );
                                    }
                                }
                            }
                        }
                    }
                } else {
                    self.communicate_with_simulator(
                        ambient_channels.tx_ambient_to_tcp_opt,
                        ambient_channels.rx_ambient_from_tcp_opt,
                    );
                    measurement_countdown = MEASUREMENT_INTERVAL_SECS * 10;
                }

                // write the measurement result to Mutex
                {
                    match mutex_ambient_temperature.lock() {
                        Ok(mut c) => *c = self.ambient_temperature,
                        Err(_) => {
                            // do nothing
                        }
                    }
                    match mutex_ambient_humidity.lock() {
                        Ok(mut c) => *c = self.ambient_humidity,
                        Err(_) => {
                            // do nothing
                        }
                    }
                }
            }

            #[cfg(feature = "debug_ambient")]
            {
                if measurement_countdown % 100 == 0 {
                    debug!(
                        target: LOG_TARGET_AMBIENT,
                        "measurement_countdown = {}",
                        measurement_countdown
                    );
                }
            }

            spin_sleeper.sleep(sleep_duration_hundred_millis);
        }

        match ambient_channels.tx_ambient_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_AMBIENT,
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // This thread has channel connections to underlying threads (GpioHandler).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received =
                match ambient_channels.rx_ambient_from_signal_handler.recv() {
                    Ok(c) => match c {
                        InternalCommand::Terminate => {
                            self.lock_warn_inapplicable_command_signal_handler = false;
                            true
                        }
                        _ => {
                            if !self.lock_warn_inapplicable_command_signal_handler {
                                warn!(
                                    target: LOG_TARGET_AMBIENT,
                                    "ignoring inapplicable command from signal handler ({})",
                                    c
                                );
                                self.lock_warn_inapplicable_command_signal_handler = true;
                            }
                            false
                        }
                    },
                    Err(e) => {
                        if !self.lock_error_channel_receive_termination {
                            error!(
                                target: LOG_TARGET_AMBIENT,
                                "receiving termination signal from signal handler failed ({e:?})"
                            );
                            self.lock_error_channel_receive_termination = true;
                        }
                        false
                    }
                };
            spin_sleeper.sleep(sleep_duration_one_millis);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{self, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::Duration;

    use crate::sensors::ambient::{Ambient, AmbientChannels};
    use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
    use crate::utilities::config::{read_config_file, ConfigData};

    #[test]
    // Test case verifies if the test object is processing the interfaces correctly.
    pub fn ambient_interfaces() {
        // channels for communication with the test environment
        let (tx_tcp_to_ambient, rx_ambient_from_tcp) = mpsc::channel();
        let (tx_ambient_to_tcp, rx_tcp_from_ambient) = mpsc::channel();
        let tx_ambient_to_tcp_opt = Some(tx_ambient_to_tcp.clone());
        let rx_ambient_from_tcp_opt = Some(rx_ambient_from_tcp);
        let (tx_signal_handler_to_ambient, rx_ambient_from_signal_handler) = mpsc::channel();
        let (tx_ambient_to_signal_handler, rx_signal_handler_from_ambient) = mpsc::channel();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());

        let initial_value_ambient_temperature =
            config.ambient.replacement_value_ambient_temperature;
        let initial_value_ambient_humidity = config.ambient.replacement_value_ambient_humidity;
        let mut ambient = Ambient::new(config.ambient);

        let mutex_ambient_temperature = Arc::new(Mutex::new(initial_value_ambient_temperature));
        let mutex_ambient_humidity = Arc::new(Mutex::new(initial_value_ambient_humidity));

        scope(|scope| {
            scope.spawn(move || {
                // mock signal handler
                let sleep_duration_8_seconds = Duration::from_millis(8000);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_8_seconds);
                let _ = tx_signal_handler_to_ambient.send(InternalCommand::Quit);
                rx_signal_handler_from_ambient.recv().unwrap();
                let _ = tx_signal_handler_to_ambient.send(InternalCommand::Terminate);
            });

            scope.spawn(move || {
                // mock TCP communication thread
                let sleep_duration_one_millisecond = Duration::from_millis(1);
                let spin_sleeper = SpinSleeper::default();
                loop {
                    match rx_tcp_from_ambient.try_recv() {
                        Ok(c) => {
                            let response_value = match c {
                                InternalCommand::RequestSignal(d) => {
                                    match d {
                                        AquariumSignal::AmbientTemperature => 20.0,
                                        AquariumSignal::AmbientHumidity => 60.0,
                                        _ => {
                                            panic!("Mock TCP communication: Received request for inapplicable signal  {}.", d);
                                        }
                                    }
                                },
                                InternalCommand::Quit => {
                                    break;
                                }
                                _ => {
                                    panic!("Mock TCP communication: Received invalid command.");
                                },
                            };
                            match tx_tcp_to_ambient.send(response_value) {
                                Ok(()) => {}
                                Err(e) => {
                                    panic!(
                                        "Error in trying to send mock TCP communication response back to test object: {e:?}"
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!("Mock TCP communication: Error in trying to receive command from data logger: {e:?}");
                                }
                            }
                        }
                    }
                    spin_sleeper.sleep(sleep_duration_one_millisecond);
                }
            });

            scope.spawn(move || {
                let ambient_channels = AmbientChannels {
                    tx_ambient_to_tcp_opt: &tx_ambient_to_tcp_opt,
                    rx_ambient_from_tcp_opt: &rx_ambient_from_tcp_opt,
                    tx_ambient_to_dht_opt: None,
                    rx_ambient_from_dht_opt: None,
                    tx_ambient_to_signal_handler,
                    rx_ambient_from_signal_handler,
                };

                // test object
                ambient.execute(
                    ambient_channels,
                    mutex_ambient_temperature,
                    mutex_ambient_humidity,
                );

                // signal end of test case execution to mock TCP communication thread
                let _ = tx_ambient_to_tcp.send(InternalCommand::Quit);
            });
        });
    }
}
