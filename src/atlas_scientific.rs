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
use crate::get_resp_sim::GetResponseFromSimulatorTrait;
#[cfg(target_os = "linux")]
use crate::i2c_interface::I2cInterface;
use crate::proc_ext_req::ProcessExternalRequestTrait;

const LOG_TARGET_ATLAS_SCIENTIFIC: &str = "atlas_scientific";

// time interval for repeating measurement
const MEASUREMENT_INTERVAL_SECS: i32 = 60;

/// Holds the configuration data for the Atlas Scientific sensor communication.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct AtlasScientificConfig {
    /// flag to determine if application shall communicate with hardware directly or via TCP with a simulator
    pub use_simulator: bool,

    /// replacement value for water temperature in case reading from I2C fails
    pub replacement_value_water_temperature: f32,

    /// replacement value for pH in case reading from I2C fails
    pub replacement_value_ph: f32,

    /// replacement value for conductivity in case reading from I2C fails
    pub replacement_value_conductivity: f32,
}

/// Contains the communication Atlas Scientific sensors via I2C.
/// Alternatively, TCP communication is used when configured to run with simulator.
/// The struct holds attributes for the results and for error flags indicating unsuccessful communication.
/// Sensor data is read periodically and stored.
/// Requests from data logger, ventilation control and heating control are answered with last measured data.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// creates the channels and sets up threads.
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     atlas_scientific[AtlasScientific] --> tcp_communication[TcpCommunication]
///     tcp_communication --> atlas_scientific
///     heating[Heating] --> atlas_scientific
///     atlas_scientific --> heating
///     ventilation[Ventilation] --> atlas_scientific
///     atlas_scientific --> ventilation
///     atlas_scientific --> signal_handler[SignalHandler]
///     signal_handler --> atlas_scientific
///     atlas_scientific --> data_logger[DataLogger]
///     data_logger --> atlas_scientific
/// ```

pub struct AtlasScientific {
    // configuration data for AtlasScientific
    config: AtlasScientificConfig,

    /// water temperature in °C
    water_temperature: f32,

    /// flag indicating that the water temperature may be inaccurate
    #[allow(dead_code)]
    water_temperature_invalid: bool,

    /// conductivity in mS
    conductivity: f32,

    /// flag indicating that the conductivity may be inaccurate
    #[allow(dead_code)]
    conductivity_invalid: bool,

    /// pH value
    pH: f32,

    /// flag indicating that the pH value may be inaccurate
    #[allow(dead_code)]
    pH_invalid: bool,

    // I2C interface
    #[cfg(target_os = "linux")]
    i2c_interface_opt: Option<I2cInterface>,

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

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive signal from ventilation control thread via channel
    lock_error_channel_receive_ventilation: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send response to ventilation control thread via channel
    lock_error_ventilation_response: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via channel from ventilation control thread        
    lock_warn_inapplicable_command_ventilation: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received invalid signal request via channel from ventilation control thread    
    lock_error_invalid_signal_request_ventilation: bool,
}

impl ProcessExternalRequestTrait for AtlasScientific {}

impl GetResponseFromSimulatorTrait for AtlasScientific {}

impl AtlasScientific {
    #[cfg(target_os = "linux")]
    /// provides a struct of type AtlasScientific for Linux platforms
    /// # Arguments
    /// * `config` - configuration data for AtlasScientific
    /// * `i2c_interface_opt` - struct of type I2cInterface wrapped as optional          
    pub fn new(
        config: AtlasScientificConfig,
        i2c_interface_opt: Option<I2cInterface>,
    ) -> AtlasScientific {
        if !config.use_simulator && !i2c_interface_opt.is_some() {
            panic!("{}: application shall use real HW, but I2C interface not provided.");
        }
        AtlasScientific {
            config: config,
            water_temperature: 0.0,
            water_temperature_invalid: true,
            conductivity: 0.0,
            conductivity_invalid: true,
            pH: 0.0,
            pH_invalid: true,
            i2c_interface_opt: i2c_interface_opt,
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
            lock_error_channel_receive_ventilation: false,
            lock_error_ventilation_response: false,
            lock_warn_inapplicable_command_ventilation: false,
            lock_error_invalid_signal_request_ventilation: false,
        }
    }

    #[cfg(not(target_os = "linux"))]
    /// provides a struct of type TankLevelSwitchSignals for non-Linux platforms
    /// # Arguments
    /// * `config` - configuration data for AtlasScientific
    pub fn new(config: AtlasScientificConfig) -> AtlasScientific {
        AtlasScientific {
            config: config,
            water_temperature: 0.0,
            water_temperature_invalid: true,
            conductivity: 0.0,
            conductivity_invalid: true,
            pH: 0.0,
            pH_invalid: true,
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
            lock_error_channel_receive_ventilation: false,
            lock_error_ventilation_response: false,
            lock_warn_inapplicable_command_ventilation: false,
            lock_error_invalid_signal_request_ventilation: false,
        }
    }

    /// check if data logger sent command and respond
    fn communicate_with_data_logger(
        &mut self,
        tx_to_data_logger: &Sender<f32>,
        rx_from_data_logger: &Receiver<InternalCommand>,
    ) {
        match rx_from_data_logger.try_recv() {
            Ok(c) => {
                self.lock_error_channel_receive_data_logger = false;
                match c {
                    InternalCommand::RequestSignal(d) => {
                        self.lock_warn_inapplicable_command_data_logger = false;
                        let response_to_data_logger = match d {
                            AquariumSignal::WaterTemperature => {
                                self.lock_error_invalid_signal_request_data_logger = false;
                                self.water_temperature
                            }
                            AquariumSignal::Conductivity => {
                                self.lock_error_invalid_signal_request_data_logger = false;
                                self.conductivity
                            }
                            AquariumSignal::pH => {
                                self.lock_error_invalid_signal_request_data_logger = false;
                                self.pH
                            }
                            _ => {
                                if !self.lock_error_invalid_signal_request_data_logger {
                                    error!(
                                        target: LOG_TARGET_ATLAS_SCIENTIFIC,
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
                                        target: LOG_TARGET_ATLAS_SCIENTIFIC,
                                        "responding back to data logger thread failed ({e:?})"
                                    );
                                    self.lock_error_data_logger_response = true;
                                }
                            }
                        };
                    }
                    _ => {
                        if !self.lock_warn_inapplicable_command_data_logger {
                            warn!(
                                target: LOG_TARGET_ATLAS_SCIENTIFIC,
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
                                target: LOG_TARGET_ATLAS_SCIENTIFIC,
                                "receiving command from data logger thread failed ({e:?})"
                            );
                            self.lock_error_channel_receive_data_logger = true;
                        }
                    }
                }
            }
        };
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `tx_to_tcp_opt` - Sender part of channel for communication via TCP wrapped as optional
    /// * `rx_from_tcp_opt` - Receiver part of channel for communication via TCP wrapped as optional
    /// * `tx_to_heating` - Sender part of channel for communication to heating control
    /// * `rx_from_heating` - Receiver part of channel for communication from heating control
    /// * `tx_to_ventilation` - Sender part of channel for communication to ventilation control
    /// * `rx_from_ventilation` - Receiver part of channel for communication from ventilation control
    /// * `tx_to_data_logger` - Sender part of channel for communication to data logger
    /// * `rx_from_data_logger` - Receiver part of channel for communication from data logger
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    pub fn execute(
        &mut self,
        tx_to_tcp_opt: Option<Sender<InternalCommand>>,
        rx_from_tcp_opt: Option<Receiver<f32>>,
        tx_to_heating: Sender<f32>,
        rx_from_heating: Receiver<InternalCommand>,
        tx_to_ventilation: Sender<f32>,
        rx_from_ventilation: Receiver<InternalCommand>,
        tx_to_data_logger: Sender<f32>,
        rx_from_data_logger: Receiver<InternalCommand>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        let sleep_duration_hundred_millisec = Duration::from_millis(100);
        let sleep_duration_one_millisec = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        // initialize signals with replacement values
        self.water_temperature = self.config.replacement_value_water_temperature;
        self.pH = self.config.replacement_value_ph;
        self.conductivity = self.config.replacement_value_conductivity;

        // initialize countdown
        let mut measurement_countdown: i32 = MEASUREMENT_INTERVAL_SECS * 100;

        if (self.config.use_simulator) && (tx_to_tcp_opt.is_none() | rx_from_tcp_opt.is_none()) {
            panic!(
                "{}: configured to run with simulator, but no channel to TCP thread provided.",
                LOG_TARGET_ATLAS_SCIENTIFIC
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
                        // unwrapping is safe, because tested in constructur
                        let i2c_interface = self.i2c_interface_opt.unwrap();

                        // scope for platform-specific code
                        self.water_temperature = match self
                            .i2c_interface
                            .get_response_from_i2c(AquariumSignal::WaterTemperature)
                        {
                            Ok(c) => {
                                self.water_temperature_invalid = false;
                                c
                            }
                            Err(_) => {
                                self.water_temperature_invalid = true;
                                self.water_temperature // use previously measured value
                            }
                        };
                        self.conductivity = match self
                            .i2c_interface
                            .get_response_from_i2c(AquariumSignal::Conductivity)
                        {
                            Ok(c) => {
                                self.conductivity_invalid = false;
                                c
                            }
                            Err(_) => {
                                self.conductivity_invalid = true;
                                self.conductivity // use previously measured value
                            }
                        };
                        self.pH = match self.i2c_interface.get_response_from_i2c(AquariumSignal::pH)
                        {
                            Ok(c) => {
                                self.pH_invalid = false;
                                c
                            }
                            Err(_) => {
                                self.pH_invalid = true;
                                self.pH // use previously measured value
                            }
                        };
                    }
                } else {
                    // unwrap is safe, because tested above
                    let tx_to_simulator = tx_to_tcp_opt.as_ref().unwrap();
                    let rx_from_simulator = rx_from_tcp_opt.as_ref().unwrap();

                    self.water_temperature = Self::get_response_from_simulator(
                        &tx_to_simulator,
                        &rx_from_simulator,
                        InternalCommand::RequestSignal(AquariumSignal::WaterTemperature),
                    );
                    self.conductivity = Self::get_response_from_simulator(
                        &tx_to_simulator,
                        &rx_from_simulator,
                        InternalCommand::RequestSignal(AquariumSignal::Conductivity),
                    );
                    self.conductivity = Self::get_response_from_simulator(
                        &tx_to_simulator,
                        &rx_from_simulator,
                        InternalCommand::RequestSignal(AquariumSignal::pH),
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
                                AquariumSignal::WaterTemperature => {
                                    self.lock_error_invalid_signal_request_heating = false;
                                    self.water_temperature
                                }
                                _ => {
                                    if !self.lock_error_invalid_signal_request_heating {
                                        error!(
                                            target: LOG_TARGET_ATLAS_SCIENTIFIC,
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
                                    target: LOG_TARGET_ATLAS_SCIENTIFIC,
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
                                    target: LOG_TARGET_ATLAS_SCIENTIFIC,
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
                            target: LOG_TARGET_ATLAS_SCIENTIFIC,
                            "responding back to heating thread failed ({e:?})"
                        );
                        self.lock_error_heating_response = true;
                    }
                }
            };

            //*** communication to ventilation thread ****
            let response_to_ventilation = match rx_from_ventilation.try_recv() {
                Ok(c) => {
                    self.lock_error_channel_receive_ventilation = false;
                    match c {
                        InternalCommand::RequestSignal(d) => {
                            self.lock_warn_inapplicable_command_ventilation = false;
                            match d {
                                AquariumSignal::WaterTemperature => {
                                    self.lock_error_invalid_signal_request_ventilation = false;
                                    self.water_temperature
                                }
                                _ => {
                                    if !self.lock_error_invalid_signal_request_ventilation {
                                        error!(
                                            target: LOG_TARGET_ATLAS_SCIENTIFIC,
                                            "internal error: received request for invalid signal {}",
                                            d
                                        );
                                        self.lock_error_invalid_signal_request_ventilation = true;
                                    }
                                    0.0 // default response to unknown signal request instead of panicking
                                }
                            }
                        }
                        _ => {
                            if !self.lock_warn_inapplicable_command_ventilation {
                                warn!(
                                    target: LOG_TARGET_ATLAS_SCIENTIFIC,
                                    "ignoring inapplicable command from ventilation thread."
                                );
                                self.lock_warn_inapplicable_command_ventilation = true;
                            }
                            0.0 // default response to unknown request instead of panicking
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => {
                            /* empty buffer - no action required */
                            self.lock_error_channel_receive_ventilation = false;
                        }
                        TryRecvError::Disconnected => {
                            if !self.lock_error_channel_receive_ventilation {
                                error!(
                                    target: LOG_TARGET_ATLAS_SCIENTIFIC,
                                    "receiving command from ventilation thread failed ({e:?})"
                                );
                                self.lock_error_channel_receive_ventilation = true;
                            }
                        }
                    }
                    0.0
                }
            };

            _ = match tx_to_ventilation.send(response_to_ventilation.into()) {
                Ok(()) => {
                    self.lock_error_ventilation_response = false;
                }
                Err(e) => {
                    if !self.lock_error_ventilation_response {
                        error!(
                            target: LOG_TARGET_ATLAS_SCIENTIFIC,
                            "responding back to ventilation thread failed ({e:?})"
                        );
                        self.lock_error_ventilation_response = true;
                    }
                }
            };

            //*** communication to data logger thread ***
            self.communicate_with_data_logger(&tx_to_data_logger, &rx_from_data_logger);

            spin_sleeper.sleep(sleep_duration_hundred_millisec);
        }

        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_ATLAS_SCIENTIFIC,
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
                                target: LOG_TARGET_ATLAS_SCIENTIFIC,
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
                            target: LOG_TARGET_ATLAS_SCIENTIFIC,
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
    /*
    use chrono::DateTime;
    use chrono::Local;
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::atlas_scientific::AtlasScientific;
    use crate::atlas_scientific::LOG_TARGET_ATLAS_SCIENTIFIC;
    use crate::channel_content::AquariumSignal;
    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::proc_ext_req::ProcessExternalRequestTrait;
    */
}
