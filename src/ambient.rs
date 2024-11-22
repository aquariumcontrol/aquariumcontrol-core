/* Copyright 2024 Uwe Martinuuu

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#![allow(non_snake_case)]
use crate::channel_content::AquariumSignal;
use log::{error, warn};
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::TryRecvError;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::channel_content::InternalCommand;
use crate::dht::DhtError;
use crate::get_resp_sim::GetResponseFromSimulatorTrait;
use crate::proc_ext_req::ProcessExternalRequestTrait;

const LOG_TARGET_AMBIENT: &str = "ambient";

// time interval for repeating measurement
const MEASUREMENT_INTERVAL_SECS: i32 = 60;

/// Holds the configuration data for the measurement of ambient signals from DHT sensor
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.

#[derive(Deserialize, Debug)]
pub struct AmbientConfig {
    /// flag to determine if application shall communicate with hardware directly or via TCP with a simulator
    pub use_simulator: bool,

    /// replacement value for ambient temperature in case reading from DHT22 sensor fails
    replacement_value_ambient_temperature: f32,

    /// replacement value for ambient humidity in case reading from DHT22 sensor fails
    replacement_value_ambient_humidity: f32,
}

/// Contains the communication with DHT ambient temperature and humidity sensor.
/// Alternatively, TCP communication is used when configured to run with simulator.
/// Sensor data is read periodically and stored.
/// Requests from data logger and heating control are answered with last measured data.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     ambient[Ambient] --> data_logger[DataLogger]
///     data_logger --> ambient
///     ambient --> tcp_communication[TcpCommunication]
///     tcp_communication --> ambient
///     heating[Heating] --> ambient
///     ambient --> heating
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

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via channel
    lock_error_channel_receive_termination: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via channel from signal handler thread
    lock_warn_inapplicable_command_signal_handler: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive signal from data logger thread via channel
    lock_error_channel_receive_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send response to data logger thread via channel
    lock_error_data_logger_response: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via channel from data logger thread
    lock_warn_inapplicable_command_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received invalid signal request via channel from data logger thread
    lock_error_invalid_signal_request_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive signal from heating control  thread via channel
    lock_error_channel_receive_heating: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send response to heating control thread via channel
    lock_error_heating_response: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via channel from heating control thread    
    lock_warn_inapplicable_command_heating: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received invalid signal request via channel from heating control thread    
    lock_error_invalid_signal_request_heating: bool,

    #[cfg(target_os = "linux")]
    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send request to Dht thread via channel
    lock_error_channel_send_dht: bool,

    #[cfg(target_os = "linux")]
    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive from Dht thread via channel
    lock_error_channel_receive_dht: bool,

    #[cfg(target_os = "linux")]
    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read from Dht sensor
    lock_error_dht: bool,
}

impl ProcessExternalRequestTrait for Ambient {}

impl GetResponseFromSimulatorTrait for Ambient {}

impl Ambient {
    /// provides a struct of type Ambient
    /// # Arguments
    /// * `config` - configuration data for Ambient
    pub fn new(config: AmbientConfig) -> Ambient {
        let replacement_value_ambient_temperature = config.replacement_value_ambient_temperature;
        let replacement_value_ambient_humidity = config.replacement_value_ambient_humidity;
        Ambient {
            config: config,
            ambient_temperature: replacement_value_ambient_temperature,
            ambient_humidity: replacement_value_ambient_humidity,
            lock_error_channel_receive_termination: false,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_data_logger: false,
            lock_error_data_logger_response: false,
            lock_warn_inapplicable_command_data_logger: false,
            lock_error_invalid_signal_request_data_logger: false,
            lock_error_channel_receive_heating: false,
            lock_error_heating_response: false,
            lock_warn_inapplicable_command_heating: false,
            lock_error_invalid_signal_request_heating: false,
            #[cfg(target_os = "linux")]
            lock_error_channel_send_dht: false,

            #[cfg(target_os = "linux")]
            lock_error_channel_receive_dht: false,

            #[cfg(target_os = "linux")]
            lock_error_dht: false,
        }
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `tx_to_tcp_opt` - Sender part of channel for communication via TCP wrapped as optional
    /// * `rx_from_tcp_opt` - Receiver part of channel for communication via TCP wrapped as optional
    /// * `tx_to_heating` - Sender part of channel for communication to heating control
    /// * `rx_from_heating` - Receiver part of channel for communication from heating control
    /// * `tx_to_data_logger` - Sender part of channel for communication to data logger
    /// * `rx_from_data_logger` - Receiver part of channel for communication from data logger
    /// * `tx_to_dht_opt` - Sender part of channel for communication to Dht
    /// * `rx_from_dht_opt` - Receiver part of channel for communication from Dht
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    pub fn execute(
        &mut self,
        tx_to_tcp_opt: &Option<Sender<InternalCommand>>,
        rx_from_tcp_opt: &Option<Receiver<f32>>,
        tx_to_heating: Sender<f32>,
        rx_from_heating: Receiver<InternalCommand>,
        tx_to_data_logger: Sender<f32>,
        rx_from_data_logger: Receiver<InternalCommand>,
        tx_to_dht_opt: Option<Sender<InternalCommand>>,
        rx_from_dht_opt: Option<Receiver<Result<(f32, f32), DhtError>>>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        let sleep_duration_hundred_millisec = Duration::from_millis(100);
        let sleep_duration_one_millisec = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        #[cfg(target_os = "linux")]
        let mut request_sent_to_dht: bool = false;

        // initialize signals with replacement values
        self.ambient_temperature = self.config.replacement_value_ambient_temperature;
        self.ambient_humidity = self.config.replacement_value_ambient_humidity;

        // initialize countdown
        let mut measurement_countdown: i32 = MEASUREMENT_INTERVAL_SECS * 10;

        if (self.config.use_simulator) && (tx_to_tcp_opt.is_none() | rx_from_tcp_opt.is_none()) {
            panic!(
                "{}: configured to run with simulator, but no channel to TCP thread provided.",
                LOG_TARGET_AMBIENT
            );
        }

        #[cfg(not(target_os = "linux"))]
        if tx_to_dht_opt.is_some() || rx_from_dht_opt.is_some() {
            warn!(target: LOG_TARGET_AMBIENT,
                "Sender/Receiver to Dht are not 'None'");
        }

        loop {
            let (
                quit_command_received, // request to end the application has been received
                _,
                _,
            ) = self.process_external_request(&rx_from_signal_handler, None);
            if quit_command_received {
                break;
            }

            if measurement_countdown > 0 {
                measurement_countdown -= 1;
            } else {
                if !self.config.use_simulator {
                    #[cfg(target_os = "linux")]
                    {
                        if tx_to_dht_opt.is_some() && rx_from_dht_opt.is_some() {
                            if !request_sent_to_dht {
                                let tx_to_dht = tx_to_dht_opt.as_ref().unwrap();

                                // respond only after having received request:
                                match tx_to_dht.send(InternalCommand::RequestSignal(
                                    AquariumSignal::AmbientTemperature,
                                )) {
                                    Ok(()) => {
                                        self.lock_error_channel_send_dht = false;
                                        request_sent_to_dht = true;
                                    }
                                    Err(e) => {
                                        if !self.lock_error_channel_send_dht {
                                            error!(
                                                target: LOG_TARGET_AMBIENT,
                                                "sending request to Dht thread failed. ({e:?})"
                                            );
                                            self.lock_error_channel_send_dht = true;
                                        }
                                    }
                                };
                            } else {
                                // read from channel if measurement result is available from Dht thread
                                let rx_from_dht = rx_from_dht_opt.as_ref().unwrap();

                                match rx_from_dht.try_recv() {
                                    Ok(response) => match response {
                                        Ok((ambient_temperature, ambient_humidity)) => {
                                            self.ambient_temperature = ambient_temperature;
                                            self.ambient_humidity = ambient_humidity;
                                            self.lock_error_dht = false;
                                        }
                                        Err(e) => {
                                            if !self.lock_error_dht {
                                                error!(
                                                    target: LOG_TARGET_AMBIENT,
                                                    "error when reading from DHT sensor ({e:?})"
                                                );
                                                self.lock_error_dht = true;
                                            }
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
                                    }
                                }
                                request_sent_to_dht = false; // allow for next request to be sent
                            }
                        }
                    }
                } else {
                    // unwrap is safe, because tested above
                    let tx_to_simulator = tx_to_tcp_opt.as_ref().unwrap();
                    let rx_from_simulator = rx_from_tcp_opt.as_ref().unwrap();

                    self.ambient_temperature = Self::get_response_from_simulator(
                        &tx_to_simulator,
                        &rx_from_simulator,
                        InternalCommand::RequestSignal(AquariumSignal::AmbientTemperature),
                    );
                    self.ambient_humidity = Self::get_response_from_simulator(
                        &tx_to_simulator,
                        &rx_from_simulator,
                        InternalCommand::RequestSignal(AquariumSignal::AmbientHumidity),
                    );
                }
                measurement_countdown = MEASUREMENT_INTERVAL_SECS * 1000;
            }

            //*** communication to heating thread ****
            match rx_from_heating.try_recv() {
                Ok(c) => {
                    self.lock_error_channel_receive_heating = false;
                    match c {
                        InternalCommand::RequestSignal(d) => {
                            self.lock_warn_inapplicable_command_heating = false;
                            let response_to_heating = match d {
                                AquariumSignal::AmbientTemperature => {
                                    self.lock_error_invalid_signal_request_heating = false;
                                    self.ambient_temperature
                                }
                                _ => {
                                    if !self.lock_error_invalid_signal_request_heating {
                                        error!(
                                            target: LOG_TARGET_AMBIENT,
                                            "internal error: received request for invalid signal {}",
                                            d
                                        );
                                        self.lock_error_invalid_signal_request_heating = true;
                                    }
                                    0.0 // default response to unknown signal request instead of panicking
                                }
                            };
                            // respond only after having received request:
                            match tx_to_heating.send(response_to_heating.into()) {
                                Ok(()) => {
                                    self.lock_error_heating_response = false;
                                }
                                Err(e) => {
                                    if !self.lock_error_heating_response {
                                        error!(
                                            target: LOG_TARGET_AMBIENT,
                                            "responding back to heating thread failed ({e:?})"
                                        );
                                        self.lock_error_heating_response = true;
                                    }
                                }
                            };
                        }
                        _ => {
                            if !self.lock_warn_inapplicable_command_heating {
                                warn!(
                                    target: LOG_TARGET_AMBIENT,
                                    "ignoring inapplicable command from heating thread."
                                );
                                self.lock_warn_inapplicable_command_heating = true;
                            }
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => {
                            /* empty buffer - no action required */
                            self.lock_error_channel_receive_heating = false;
                        }
                        TryRecvError::Disconnected => {
                            if !self.lock_error_channel_receive_heating {
                                error!(
                                    target: LOG_TARGET_AMBIENT,
                                    "receiving command from heating thread failed ({e:?})"
                                );
                                self.lock_error_channel_receive_heating = true;
                            }
                        }
                    }
                }
            };

            //*** communication to data logger thread ***
            match rx_from_data_logger.try_recv() {
                Ok(c) => {
                    self.lock_error_channel_receive_data_logger = false;
                    match c {
                        InternalCommand::RequestSignal(d) => {
                            self.lock_warn_inapplicable_command_data_logger = false;
                            let response_to_data_logger = match d {
                                AquariumSignal::AmbientTemperature => {
                                    self.lock_error_invalid_signal_request_data_logger = false;
                                    self.ambient_temperature
                                }
                                AquariumSignal::AmbientHumidity => {
                                    self.lock_error_invalid_signal_request_data_logger = false;
                                    self.ambient_humidity
                                }
                                _ => {
                                    if !self.lock_error_invalid_signal_request_data_logger {
                                        error!(
                                            target: LOG_TARGET_AMBIENT,
                                            "internal error: received request for invalid signal {} from data logger",
                                            d
                                        );
                                        self.lock_error_invalid_signal_request_data_logger = true;
                                    }
                                    0.0 // default response to unknown signal request instead of panicking
                                }
                            };
                            _ = match tx_to_data_logger.send(response_to_data_logger.into()) {
                                Ok(()) => {
                                    self.lock_error_data_logger_response = false;
                                }
                                Err(e) => {
                                    if !self.lock_error_data_logger_response {
                                        error!(
                                            target: LOG_TARGET_AMBIENT,
                                            "responding back to data logger thread failed ({e:?})"
                                        );
                                        self.lock_error_data_logger_response = true;
                                    }
                                }
                            }
                        }
                        _ => {
                            if !self.lock_warn_inapplicable_command_data_logger {
                                warn!(
                                    target: LOG_TARGET_AMBIENT,
                                    "ignoring inapplicable command from data logger thread."
                                );
                                self.lock_warn_inapplicable_command_data_logger = true;
                            }
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => {
                            /* empty buffer - no action required */
                            self.lock_error_channel_receive_data_logger = false;
                        }
                        TryRecvError::Disconnected => {
                            if !self.lock_error_channel_receive_data_logger {
                                error!(
                                    target: LOG_TARGET_AMBIENT,
                                    "receiving command from data logger thread failed ({e:?})"
                                );
                                self.lock_error_channel_receive_data_logger = true;
                            }
                        }
                    }
                }
            };

            spin_sleeper.sleep(sleep_duration_hundred_millisec);
        }

        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_AMBIENT,
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // this thread has channel connection to underlying threads (GpioHandler).
        // These threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by signal_handler crate.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match rx_from_signal_handler.recv() {
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
            spin_sleeper.sleep(sleep_duration_one_millisec);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::sync::mpsc::TryRecvError;
    use std::thread::scope;
    use std::time::Duration;

    use crate::ambient::Ambient;
    use crate::channel_content::AquariumSignal;
    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;

    #[test]
    // test case verifies if the test object is processing the interfaces correctly
    pub fn ambient_interfaces() {
        // channels for communication with test environment
        let (tx_data_logger_to_ambient, rx_ambient_from_data_logger) = mpsc::channel();
        let (tx_ambient_to_data_logger, rx_data_logger_from_ambient) = mpsc::channel();
        let (tx_tcp_to_ambient, rx_ambient_from_tcp) = mpsc::channel();
        let (tx_ambient_to_tcp, rx_tcp_from_ambient) = mpsc::channel();
        let tx_ambient_to_tcp_opt = Some(tx_ambient_to_tcp.clone());
        let rx_ambient_from_tcp_opt = Some(rx_ambient_from_tcp);
        let (tx_heating_to_ambient, rx_ambient_from_heating) = mpsc::channel();
        let (tx_ambient_to_heating, rx_heating_from_ambient) = mpsc::channel();
        let (tx_signal_handler_to_ambient, rx_ambient_from_signal_handler) = mpsc::channel();
        let (tx_ambient_to_signal_handler, rx_signal_handler_from_ambient) = mpsc::channel();

        // re-use configuration previously create for different component
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_data_logger.toml".to_string());

        #[cfg(not(target_os = "linux"))]
        let mut ambient = Ambient::new(config.ambient);
        #[cfg(target_os = "linux")]
        let mut ambient = Ambient::new(config.ambient, None);

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
                // mock Heating thread
                let result_temperature = match tx_heating_to_ambient.send(
                        InternalCommand::RequestSignal(AquariumSignal::AmbientTemperature)
                    ) {
                    Ok(()) => {
                        match rx_heating_from_ambient.recv() {
                            Ok(c) => { c },
                            Err(e) => {
                                panic!("Error occurred when mock heating thread tried to receive from test object: {e:?}");
                            }
                        }
                    }
                    Err(e) => {
                        panic!("Error occurred when mock heating thread tried to send to test object: {e:?}");         
                    }
                };
                assert_eq!(result_temperature, 24.0);
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
                // mock Data logger thread
                let result_temperature = match tx_data_logger_to_ambient.send(
                        InternalCommand::RequestSignal(AquariumSignal::AmbientTemperature)
                    ) {
                    Ok(()) => {
                        match rx_data_logger_from_ambient.recv() {
                            Ok(c) => { c },
                            Err(e) => {
                                panic!("Error occurred when mock data logger thread tried to receive from test object: {e:?}");
                            }
                        }
                    }
                    Err(e) => {
                        panic!("Error occurred when mock data logger thread tried to send to test object: {e:?}");         
                    }
                };
                assert_eq!(result_temperature, 24.0);
                let result_humidity = match tx_data_logger_to_ambient.send(
                        InternalCommand::RequestSignal(AquariumSignal::AmbientHumidity)
                    ) {
                    Ok(()) => {
                        match rx_data_logger_from_ambient.recv() {
                            Ok(c) => { c },
                            Err(e) => {
                                panic!("Error occurred when mock heating thread tried to receive from test object: {e:?}");
                            }
                        }
                    }
                    Err(e) => {
                        panic!("Error occurred when mock heating thread tried to send to test object: {e:?}");         
                    }
                };
                assert_eq!(result_humidity, 60.0);
            });

            scope.spawn(move || {
                // test object
                ambient.execute(
                    &tx_ambient_to_tcp_opt,
                    &rx_ambient_from_tcp_opt,
                    tx_ambient_to_heating,
                    rx_ambient_from_heating,
                    tx_ambient_to_data_logger,
                    rx_ambient_from_data_logger,
                    None,
                    None,
                    tx_ambient_to_signal_handler,
                    rx_ambient_from_signal_handler,
                );

                // signal end of test case execution to mock TCP communication thread
                let _ = tx_ambient_to_tcp.send(InternalCommand::Quit);
            });
        });
    }
}
