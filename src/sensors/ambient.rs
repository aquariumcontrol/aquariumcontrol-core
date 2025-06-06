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
use std::sync::mpsc::{Receiver, Sender};

use crate::sensors::dht::DhtResult;
use crate::simulator::get_resp_sim::GetResponseFromSimulatorTrait;
use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use std::time::Duration;

const LOG_TARGET_AMBIENT: &str = "ambient";

// time interval for repeating measurement
const MEASUREMENT_INTERVAL_SECS: i32 = 60;

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

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to acquire lock on mutex
    lock_error_dht_mutex: bool,
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
            lock_error_dht_mutex: false,
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
    /// * `mutex_dht` - An `Arc<Mutex<DhtResult>>` into which the latest recording
    ///   from Dht sensor is written. Ambient only reads it.
    ///
    /// # Panics
    /// This function will panic if:
    /// - It's configured to use the simulator but essential TCP communication channels are missing.
    /// - It's configured to use the simulator but DHT channels are unexpectedly present.
    /// - During hardware mode, a fatal error occurs while trying to receive a response from the DHT thread (e.g., the channel disconnected permanently).
    pub fn execute(
        &mut self,
        ambient_channels: AmbientChannels,
        mutex_ambient_temperature: Arc<Mutex<f32>>,
        mutex_ambient_humidity: Arc<Mutex<f32>>,
        mutex_dht: Arc<Mutex<DhtResult>>,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let sleep_duration_one_millis = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

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

        loop {
            let (
                quit_command_received, // the request to end the application has been received
                _,
                _,
            ) = self
                .process_external_request(&ambient_channels.rx_ambient_from_signal_handler, None);

            if quit_command_received {
                // quit command has been received
                break;
            }

            if measurement_countdown > 0 {
                measurement_countdown = measurement_countdown.saturating_sub(1);
            } else {
                if self.config.use_simulator {
                    self.communicate_with_simulator(
                        ambient_channels.tx_ambient_to_tcp_opt,
                        ambient_channels.rx_ambient_from_tcp_opt,
                    );
                } else {
                    {
                        match mutex_dht.lock() {
                            Ok(dht_result) => {
                                match *dht_result {
                                    Ok(tuple) => {
                                        self.ambient_temperature = tuple.0;
                                        self.ambient_humidity = tuple.1;
                                    }
                                    Err(_) => {
                                        // keep the values from previous measurement
                                    }
                                }
                            }
                            Err(e) => {
                                if !self.lock_error_dht_mutex {
                                    self.lock_error_dht_mutex = true;
                                    error!(target: LOG_TARGET_AMBIENT, "error when trying to lock mutex: {}", e);
                                }
                            }
                        }
                    }
                }
                measurement_countdown = MEASUREMENT_INTERVAL_SECS * 10;

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
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::Duration;
    use std::sync::mpsc::TryRecvError;

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
        let dht_result = Ok((1.0, 2.0));
        let mutex_dht = Arc::new(Mutex::new(dht_result));

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
                    tx_ambient_to_signal_handler,
                    rx_ambient_from_signal_handler,
                };

                // test object
                ambient.execute(
                    ambient_channels,
                    mutex_ambient_temperature,
                    mutex_ambient_humidity,
                    mutex_dht,
                );

                // signal end of test case execution to mock TCP communication thread
                let _ = tx_ambient_to_tcp.send(InternalCommand::Quit);
            });
        });
    }
}
