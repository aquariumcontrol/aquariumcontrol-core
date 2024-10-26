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
use crate::dht::Dht;
use crate::get_resp_sim::GetResponseFromSimulatorTrait;
#[cfg(target_os = "linux")]
use crate::i2c_interface::I2cInterface;
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
///     ambient[Ambient] --> datalogger[DataLogger]
///     datalogger --> ambient
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

    /// struct of type Dht wrapped as optional
    #[allow(dead_code)]
    dht_opt: Option<Dht>,
}

impl ProcessExternalRequestTrait for Ambient {}

impl GetResponseFromSimulatorTrait for Ambient {}

impl Ambient {
    #[cfg(target_os = "linux")]
    /// provides a struct of type Ambient for Linux platform
    /// # Arguments
    /// * `config` - configuration data for Ambient
    /// * `dht_opt` - struct of type Dht wrapped as optional          
    pub fn new(config: AmbientConfig, dht_opt: Dht) -> Ambient {
        if !config.use_simulator && !dht_opt.is_some() {
            panic!("{}: application shall use real HW, but DHT interface not provided.");
        }
        Ambient {
            config: config,
            ambient_temperature: 0.0,
            ambient_humidity: 0.0,
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
            dht_opt: dht_opt,
        }
    }

    #[cfg(not(target_os = "linux"))]
    /// provides a struct of type Ambient for non-Linux platforms
    /// # Arguments
    /// * `config` - configuration data for Ambient
    pub fn new(config: AmbientConfig) -> Ambient {
        Ambient {
            config: config,
            ambient_temperature: 0.0,
            ambient_humidity: 0.0,
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
            dht_opt: None,
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
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        let sleep_duration_hundred_millisec = Duration::from_millis(100);
        let sleep_duration_one_millisec = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        // initialize signals with replacement values
        self.ambient_temperature = self.config.replacement_value_ambient_temperature;
        self.ambient_humidity = self.config.replacement_value_ambient_humidity;

        // initialize countdown
        let mut measurement_countdown: i32 = MEASUREMENT_INTERVAL_SECS * 100;

        if (self.config.use_simulator) && (tx_to_tcp_opt.is_none() | rx_from_tcp_opt.is_none()) {
            panic!(
                "{}: configured to run with simulator, but no channel to TCP thread provided.",
                LOG_TARGET_AMBIENT
            );
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
                        // unwrap is safe, because tested in constructor
                        let dht = self.dht_opt.unwrap();

                        // scope for platform-specific code
                        (self.ambient_temperature, self.ambient_humidity) =
                            match self.dht.get_data() {
                                Ok(c) => c,
                                Err(_) => {
                                    // in case of error, stay with previous values
                                    (self.ambient_temperature, self.ambient_humidity)
                                }
                            };
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
                measurement_countdown = MEASUREMENT_INTERVAL_SECS * 100;
            }

            //*** communication to heating thread ****
            let response_to_heating = match rx_from_heating.try_recv() {
                Ok(c) => {
                    self.lock_error_channel_receive_heating = false;
                    match c {
                        InternalCommand::RequestSignal(d) => {
                            self.lock_warn_inapplicable_command_heating = false;
                            match d {
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
                            }
                        }
                        _ => {
                            if !self.lock_warn_inapplicable_command_heating {
                                warn!(
                                    target: LOG_TARGET_AMBIENT,
                                    "ignoring inapplicable command from heating thread."
                                );
                                self.lock_warn_inapplicable_command_heating = true;
                            }
                            0.0 // default response to unknown request instead of panicking
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
                    0.0
                }
            };

            _ = match tx_to_heating.send(response_to_heating.into()) {
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

            //*** communication to data logger thread ***
            let response_to_data_logger = match rx_from_data_logger.try_recv() {
                Ok(c) => {
                    self.lock_error_channel_receive_data_logger = false;
                    match c {
                        InternalCommand::RequestSignal(d) => {
                            self.lock_warn_inapplicable_command_data_logger = false;
                            match d {
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
                            0.0 // default response to unknown command instead of panicking
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
                    0.0
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
    use chrono::DateTime;
    use chrono::Local;
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::ambient::Ambient;
    use crate::ambient::LOG_TARGET_AMBIENT;
    use crate::channel_content::AquariumSignal;
    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::proc_ext_req::ProcessExternalRequestTrait;
}
