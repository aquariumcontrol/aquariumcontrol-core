/* Copyright 2024 Uwe Martinuuu

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::{error, warn};
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::TryRecvError;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;
use crate::get_resp_sim::GetResponseFromSimulatorTrait;
use crate::proc_ext_req::ProcessExternalRequestTrait;
#[cfg(target_os = "linux")]
use rppal::gpio::InputPin;

/// Holds the configuration data for the tank level switch calculation.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Clone)]
pub struct TankLevelSwitchConfig {
    /// flag to determine if application shall communicate with hardware directly or via TCP with a simulator
    pub use_simulator: bool,

    /// number of seconds which need to indicate a low level before the refill control assumes that the water level is low
    tank_level_switch_position_high_stabilization_count: u32,
}

const LOG_TARGET_TANK_LEVEL_SWITCH: &str = "tank_level_switch";

/// Contains the measurement of tank level position and calculation of stabilized tank level position signal.
/// Tank level position is measured by accessing the state of a dedicated GPIO pin.
/// Alternatively, TCP communication is used when configured to run with simulator.
/// The struct holds attributes for the results and for error flags indicating unsuccessful communication.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     refill[Refill] --> tank_level_switch[TankLevelSwitch]
///     tank_level_switch --> refill
///     signal_handler[SignalHandler] --> tank_level_switch
///     tank_level_switch --> signal_handler
///     tank_level_switch --> tcp_communication[TcpCommunication]
///     tcp_communication --> tank_level_switch
///     heating[Heating] --> tank_level_switch
///     tank_level_switch --> heating
///     tank_level_switch --> data_logger[DataLogger]
///     data_logger --> tank_level_switch
/// ```

pub struct TankLevelSwitch {
    /// configuration of tank level switch calculation
    config: TankLevelSwitchConfig,

    /// flag indicating if the last measurement of tank level switch indicates high (true) or low (false)
    tank_level_switch_position: bool,

    /// flag indicating if the stabilized tank level switch indicates high (true) or low (false)
    tank_level_switch_position_stabilized: bool,

    /// flag indicating that the tank level switch position may be inaccurate
    tank_level_switch_invalid: bool,

    /// time counter to determine if positition is stabilized
    tank_level_switch_position_high_stabilization_counter: u32,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command from signal handler
    lock_warn_inapplicable_command_signal_handler: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via channel
    lock_error_channel_receive_termination: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received request from refill thread for signal which cannot be processed by this thread
    lock_error_invalid_signal_request_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received invalid command from refill thread
    lock_warn_inapplicable_command_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive request from refill thread
    lock_error_channel_receive_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send back value to refill thread
    lock_error_refill_response: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received request from heating thread for signal  which cannot be processed by this thread
    lock_error_invalid_signal_request_heating: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received invalid command from heating thread
    lock_warn_inapplicable_command_heating: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive request from heating thread
    lock_error_channel_receive_heating: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send back value to heating thread
    lock_error_heating_response: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received request from data logger thread for signal which cannot be processed by this thread
    lock_error_invalid_signal_request_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received invalid command from data logger thread
    lock_warn_inapplicable_command_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive request from data logger thread
    lock_error_channel_receive_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send back value to data logger thread
    lock_error_data_logger_response: bool,

    // interface to GPIO pin wrapped as optional
    #[cfg(target_os = "linux")]
    input_pin_opt: Option<InputPin>,
}

impl ProcessExternalRequestTrait for TankLevelSwitch {}

impl GetResponseFromSimulatorTrait for TankLevelSwitch {}

impl TankLevelSwitch {
    #[cfg(not(target_os = "linux"))]
    /// provides a struct of type TankLevelSwitchSignals for non-Linux platforms
    /// # Arguments
    /// * `config` - configuration data for TankLevelSwitch
    /// * `initial_position_stabilized` - assumed initial position of tank level before first measurement
    pub fn new(
        config: TankLevelSwitchConfig,
        initial_position_stabilized: bool,
    ) -> TankLevelSwitch {
        TankLevelSwitch {
            config: config,
            tank_level_switch_position: true, // initialization value assuming no error
            tank_level_switch_invalid: false, // initialization value assuming no error
            tank_level_switch_position_stabilized: initial_position_stabilized,
            tank_level_switch_position_high_stabilization_counter: 0,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
            lock_error_invalid_signal_request_refill: false,
            lock_warn_inapplicable_command_refill: false,
            lock_error_channel_receive_refill: false,
            lock_error_refill_response: false,
            lock_error_invalid_signal_request_heating: false,
            lock_warn_inapplicable_command_heating: false,
            lock_error_channel_receive_heating: false,
            lock_error_heating_response: false,
            lock_error_invalid_signal_request_data_logger: false,
            lock_warn_inapplicable_command_data_logger: false,
            lock_error_channel_receive_data_logger: false,
            lock_error_data_logger_response: false,
        }
    }

    #[cfg(target_os = "linux")]
    /// provides a struct of type TankLevelSwitchSignals for Linux platform.
    /// # Arguments
    /// * `config` - configuration data for TankLevelSwitch
    /// * `initial_position_stabilized` - assumed initial position of tank level before first measurement
    /// * `input_pin_opt` - interface to GPIO pin wrapped as optional
    pub fn new(
        config: TankLevelSwitchConfig,
        initial_position_stabilized: bool,
        input_pin_opt: Option<InputPin>,
    ) -> TankLevelSwitch {
        if !config.use_simulator && !input_pin_opt.is_some() {
            panic!("{}: application shall use real HW, but GPIO pin interface not provided.");
        }

        TankLevelSwitch {
            config: config,
            tank_level_switch_position: true, // initialization value assuming no error
            tank_level_switch_invalid: false, // initialization value assuming no error
            tank_level_switch_position_stabilized: initial_position_stabilized,
            tank_level_switch_position_high_stabilization_counter: 0,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
            lock_error_invalid_signal_request_refill: false,
            lock_warn_inapplicable_command_refill: false,
            lock_error_channel_receive_refill: false,
            lock_error_refill_response: false,
            lock_error_invalid_signal_request_heating: false,
            lock_warn_inapplicable_command_heating: false,
            lock_error_channel_receive_heating: false,
            lock_error_heating_response: false,
            lock_error_invalid_signal_request_data_logger: false,
            lock_warn_inapplicable_command_data_logger: false,
            lock_error_channel_receive_data_logger: false,
            lock_error_data_logger_response: false,
            input_pin_opt: input_pin_opt,
        }
    }

    /// Executes either
    /// - the measurement of GPIO pin position or
    /// - the channel communication to update the tank level switch position signal.
    /// # Arguments
    /// * `tx_to_tcp_opt` - sender part of the channel for communication via TCP wrapped as optional
    /// * `rx_from_tcp_opt` - receiver part of the channel for communication via TCP wrapped as optional
    pub fn update_tank_level_switch_signals(
        &mut self,
        tx_to_tcp_opt: &Option<Sender<InternalCommand>>,
        rx_from_tcp_opt: &Option<Receiver<f32>>,
    ) {
        if self.config.use_simulator {
            let tx_to_tcp = tx_to_tcp_opt.as_ref().unwrap();
            let rx_from_tcp = rx_from_tcp_opt.as_ref().unwrap();
            self.tank_level_switch_position = Self::get_response_from_simulator(
                &tx_to_tcp,
                &rx_from_tcp,
                InternalCommand::RequestSignal(AquariumSignal::TankLevelSwitchPosition),
            ) > 0.0;
        } else {
            #[cfg(target_os = "linux")]
            {
                // unwrap is safe, because tested in constructor
                let input_pin = self.input_pin_opt.unwrap();

                self.tank_level_switch_position = input_pin.is_high();
            }
        }
    }

    /// Calculates a stabilized tank level switch position signal considering:
    /// - if invalid bit of signal is true, then use the current value of the stabilized position signal.
    /// - if the new signal is true (indicating high water level), then set the stabilized signal immediately to true (high).
    /// - if the the new signal is false (indicating low water level), then set increase the stabilization counter.
    /// - if the stabilization counter exceeds the configured limit, then set the counter to zero and set the stabilized
    /// signal to true indicating high water level.
    fn calc_tank_level_switch_position_stabilized(&mut self) {
        if self.tank_level_switch_invalid {
            // goto safe position if input signal is invalid
            self.tank_level_switch_position_stabilized = true;
        } else {
            // consider asymmetric delay for calculation
            self.tank_level_switch_position_stabilized = match self.tank_level_switch_position {
                true => {
                    self.tank_level_switch_position_high_stabilization_counter = 0;
                    true // set stabilized to true without delay
                }
                false => {
                    if self.tank_level_switch_position_high_stabilization_counter
                        < self
                            .config
                            .tank_level_switch_position_high_stabilization_count
                            * 10
                    {
                        // do not change the signal if counter is not exceeded
                        self.tank_level_switch_position_high_stabilization_counter += 1;
                        //println!(
                        //    "calc_tank_level_switch_position_stabilized: tank_level_switch_position_high_stabilization_counter = {}, tank_level_switch_position_stabilized={}",
                        //     self.tank_level_switch_position_high_stabilization_counter,
                        //     self.tank_level_switch_position_stabilized
                        //);
                        self.tank_level_switch_position_stabilized
                    } else {
                        false
                    }
                }
            }
        }
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `tx_to_tcp_opt` - Sender part of channel for communication via TCP wrapped as optional
    /// * `rx_from_tcp_opt` - Receiver part of channel for communication via TCP wrapped as optional
    /// * `tx_to_data_logger` - Sender part of channel for communication to data logger
    /// * `rx_from_data_logger` - Receiver part of channel for communication from data logger
    /// * `tx_to_refill` - Sender part of channel for communication to refill control
    /// * `rx_from_refill` - Receiver part of channel for communication from refill control
    /// * `tx_to_heating` - Sender part of channel for communication to heating control
    /// * `rx_from_heating` - Receiver part of channel for communication from heating control
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    pub fn execute(
        &mut self,
        tx_to_tcp_opt: &Option<Sender<InternalCommand>>,
        rx_from_tcp_opt: &Option<Receiver<f32>>,
        tx_to_data_logger: &Sender<bool>,
        rx_from_data_logger: &Receiver<InternalCommand>,
        tx_to_refill: &Sender<bool>,
        rx_from_refill: &Receiver<InternalCommand>,
        tx_to_heating: &Sender<bool>,
        rx_from_heating: &Receiver<InternalCommand>,
        tx_to_signal_handler: &Sender<bool>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
    ) {
        let sleep_duration_hundred_millisec = Duration::from_millis(100);
        let sleep_duration_one_millisec = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        if (self.config.use_simulator) && (tx_to_tcp_opt.is_none() | rx_from_tcp_opt.is_none()) {
            panic!(
                "{}: configured to run with simulator, but no channel to TCP thread provided.",
                LOG_TARGET_TANK_LEVEL_SWITCH
            );
        }

        loop {
            let (
                quit_command_received, // request to end the application has been received
                _,
                _,
            ) = self.process_external_request(rx_from_signal_handler, None);
            if quit_command_received {
                break;
            }

            // either communicate with real HW or with simulator to get updated signal information
            self.update_tank_level_switch_signals(&tx_to_tcp_opt, &rx_from_tcp_opt);

            self.calc_tank_level_switch_position_stabilized();

            let response_to_refill = match rx_from_refill.try_recv() {
                Ok(c) => {
                    self.lock_error_channel_receive_refill = false;
                    match c {
                        InternalCommand::RequestSignal(d) => {
                            self.lock_warn_inapplicable_command_refill = false;
                            match d {
                                AquariumSignal::TankLevelSwitchPosition => {
                                    self.lock_error_invalid_signal_request_refill = false;
                                    Some(self.tank_level_switch_position)
                                }
                                AquariumSignal::TankLevelSwitchInvalid => {
                                    self.lock_error_invalid_signal_request_refill = false;
                                    Some(self.tank_level_switch_invalid)
                                }
                                AquariumSignal::TankLevelSwitchPositionStabilized => {
                                    self.lock_error_invalid_signal_request_refill = false;
                                    Some(self.tank_level_switch_position_stabilized)
                                }
                                _ => {
                                    if !self.lock_error_invalid_signal_request_refill {
                                        error!(
                                            target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                            "internal error: received request for invalid signal {}",
                                            d
                                        );
                                        self.lock_error_invalid_signal_request_refill = true;
                                    }
                                    None
                                }
                            }
                        }
                        _ => {
                            if !self.lock_warn_inapplicable_command_refill {
                                warn!(
                                    target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                    "ignoring inapplicable command from refill thread."
                                );
                                self.lock_warn_inapplicable_command_refill = true;
                            }
                            None
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => {
                            /* empty buffer - no action required */
                            self.lock_error_channel_receive_refill = false;
                        }
                        TryRecvError::Disconnected => {
                            if !self.lock_error_channel_receive_refill {
                                error!(
                                    target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                    "receiving command from refill thread failed ({e:?})"
                                );
                                self.lock_error_channel_receive_refill = true;
                            }
                        }
                    }
                    None
                }
            };

            match response_to_refill {
                Some(c) => {
                    _ = match tx_to_refill.send(c.into()) {
                        Ok(()) => {
                            self.lock_error_refill_response = false;
                        }
                        #[cfg(not(test))]
                        Err(e) => {
                            if !self.lock_error_refill_response {
                                error!(
                                    target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                    "responding back to refill thread failed ({e:?})"
                                );
                                self.lock_error_refill_response = true;
                            }
                        }
                        #[cfg(test)]
                        Err(_) => { /* do nothing */ }
                    };
                }
                None => {
                    // do nothing
                    self.lock_error_refill_response = false;
                }
            };

            let response_to_heating = match rx_from_heating.try_recv() {
                Ok(c) => {
                    self.lock_error_channel_receive_heating = false;
                    match c {
                        InternalCommand::RequestSignal(d) => {
                            self.lock_warn_inapplicable_command_heating = false;
                            match d {
                                AquariumSignal::TankLevelSwitchPosition => {
                                    self.lock_error_invalid_signal_request_heating = false;
                                    Some(self.tank_level_switch_position)
                                }
                                AquariumSignal::TankLevelSwitchInvalid => {
                                    self.lock_error_invalid_signal_request_heating = false;
                                    Some(self.tank_level_switch_invalid)
                                }
                                AquariumSignal::TankLevelSwitchPositionStabilized => {
                                    self.lock_error_invalid_signal_request_heating = false;
                                    Some(self.tank_level_switch_position_stabilized)
                                }
                                _ => {
                                    if !self.lock_error_invalid_signal_request_heating {
                                        error!(
                                            target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                            "internal error: received request for invalid signal {}",
                                            d
                                        );
                                        self.lock_error_invalid_signal_request_heating = true;
                                    }
                                    None
                                }
                            }
                        }
                        _ => {
                            if !self.lock_warn_inapplicable_command_heating {
                                warn!(
                                    target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                    "ignoring inapplicable command from heating thread."
                                );
                                self.lock_warn_inapplicable_command_heating = true;
                            }
                            None
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
                                    target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                    "receiving command from heating thread failed ({e:?})"
                                );
                                self.lock_error_channel_receive_heating = true;
                            }
                        }
                    }
                    None
                }
            };

            match response_to_heating {
                Some(c) => {
                    _ = match tx_to_heating.send(c.into()) {
                        Ok(()) => {
                            self.lock_error_heating_response = false;
                        }
                        #[cfg(not(test))]
                        Err(e) => {
                            if !self.lock_error_heating_response {
                                error!(
                                    target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                    "responding back to heating thread failed ({e:?})"
                                );
                                self.lock_error_heating_response = true;
                            }
                        }
                        #[cfg(test)]
                        Err(_) => { /* do nothing */ }
                    };
                }
                None => {
                    // do nothing
                    self.lock_error_heating_response = false;
                }
            };

            let response_to_data_logger = match rx_from_data_logger.try_recv() {
                Ok(c) => {
                    self.lock_error_channel_receive_data_logger = false;
                    match c {
                        InternalCommand::RequestSignal(d) => {
                            self.lock_warn_inapplicable_command_data_logger = false;
                            match d {
                                AquariumSignal::TankLevelSwitchPosition => {
                                    self.lock_error_invalid_signal_request_data_logger = false;
                                    Some(self.tank_level_switch_position)
                                }
                                AquariumSignal::TankLevelSwitchInvalid => {
                                    self.lock_error_invalid_signal_request_data_logger = false;
                                    Some(self.tank_level_switch_invalid)
                                }
                                AquariumSignal::TankLevelSwitchPositionStabilized => {
                                    self.lock_error_invalid_signal_request_data_logger = false;
                                    Some(self.tank_level_switch_position_stabilized)
                                }
                                _ => {
                                    if !self.lock_error_invalid_signal_request_data_logger {
                                        error!(
                                            target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                            "internal error: received request for invalid signal {} from data logger",
                                            d
                                        );
                                        self.lock_error_invalid_signal_request_data_logger = true;
                                    }
                                    None
                                }
                            }
                        }
                        _ => {
                            if !self.lock_warn_inapplicable_command_data_logger {
                                warn!(
                                    target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                    "ignoring inapplicable command from data logger thread."
                                );
                                self.lock_warn_inapplicable_command_data_logger = true;
                            }
                            None
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
                                    target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                    "receiving command from data logger thread failed ({e:?})"
                                );
                                self.lock_error_channel_receive_data_logger = true;
                            }
                        }
                    }
                    None
                }
            };

            match response_to_data_logger {
                Some(c) => {
                    _ = match tx_to_data_logger.send(c.into()) {
                        Ok(()) => {
                            self.lock_error_data_logger_response = false;
                        }
                        #[cfg(not(test))]
                        Err(e) => {
                            if !self.lock_error_data_logger_response {
                                error!(
                                    target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                    "responding back to data logger thread failed ({e:?})"
                                );
                                self.lock_error_data_logger_response = true;
                            }
                        }
                        #[cfg(test)]
                        Err(_) => { /* do nothing */ }
                    };
                }
                None => {
                    // do nothing
                    self.lock_error_data_logger_response = false;
                }
            };

            spin_sleeper.sleep(sleep_duration_hundred_millisec);
        }

        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_TANK_LEVEL_SWITCH,
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
                                target: LOG_TARGET_TANK_LEVEL_SWITCH,
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
                            target: LOG_TARGET_TANK_LEVEL_SWITCH,
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
    use all_asserts::{assert_ge, assert_le};
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::mock::tests::mock_data_logger;
    use crate::mock::tests::mock_heating;
    use crate::mock::tests::mock_refill;
    use crate::mock::tests::mock_tcp;
    use crate::mock::tests::MockSignal;
    use crate::mock::tests::TestCommand;
    use crate::tank_level_switch::TankLevelSwitch;

    #[test]
    pub fn test_refill_calc_tank_level_switch_position_stabilized_false_when_invalid() {
        // test if position stabilized signal are updating when invalid signal is true
        let (tx_tank_level_switch_to_tcp, rx_tcp_from_tank_level_switch) = mpsc::channel();
        let (tx_tcp_to_tank_level_switch, rx_tank_level_switch_from_tcp) = mpsc::channel();
        let (tx_tank_level_switch_to_refill, rx_refill_from_tank_level_switch) = mpsc::channel();
        let (tx_refill_to_tank_level_switch, rx_tank_level_switch_from_refill) = mpsc::channel();
        let (tx_tank_level_switch_to_heating, rx_heating_from_tank_level_switch) = mpsc::channel();
        let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating) = mpsc::channel();
        let (tx_tank_level_switch_to_data_logger, rx_data_logger_from_tank_level_switch) =
            mpsc::channel();
        let (tx_data_logger_to_tank_level_switch, rx_tank_level_switch_from_data_logger) =
            mpsc::channel();
        let (tx_tank_level_switch_to_signal_handler, rx_signal_handler_from_tank_level_switch) =
            mpsc::channel();
        let (tx_signal_handler_to_tank_level_switch, rx_tank_level_switch_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (_tx_test_environment_to_tcp, rx_tcp_from_test_environment) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_refill.toml".to_string());
        let duration_test_case_secs = config
            .tank_level_switch
            .tank_level_switch_position_high_stabilization_count
            + 1;
        let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true);

        // thread for mock tcp
        let join_handle_mock_tcp = thread::Builder::new()
            .name("mock_tcp".to_string())
            .spawn(move || {
                mock_tcp(
                    &tx_tcp_to_tank_level_switch,
                    &rx_tcp_from_tank_level_switch,
                    Some(&rx_tcp_from_test_environment),
                    true,
                    false,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_tank_level_switch,
                    &rx_data_logger_from_tank_level_switch,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        // thread for mock heating
        let join_handle_mock_heating = thread::Builder::new()
            .name("mock_heating".to_string())
            .spawn(move || {
                mock_heating(
                    Some(&tx_heating_to_tank_level_switch),
                    Some(&rx_heating_from_tank_level_switch),
                    &rx_heating_from_signal_handler,
                    None,
                    None,
                    None,
                    false,
                );
            })
            .unwrap();

        // thread for mock refill - includes assertions
        let join_handle_mock_refill = thread::Builder::new()
            .name("mock_refill".to_string())
            .spawn(move || {
                let results = mock_refill(
                    Some(&tx_refill_to_tank_level_switch),
                    Some(&rx_refill_from_tank_level_switch),
                    &rx_refill_from_signal_handler,
                    None,
                    None,
                    None,
                    false,
                );
                assert_ge!(results.len(), 50);
                assert_le!(results.len(), 60);
                // check if value of all elements of result vector
                assert_eq!(results.iter().all(|&item| item == true), true);
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration = Duration::from_secs(duration_test_case_secs.into());
                let sleep_duration_thread_processing = Duration::from_millis(50);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                spin_sleeper.sleep(sleep_duration_thread_processing);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                rx_signal_handler_from_tank_level_switch.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_tank_level_switch_to_tcp_for_test_case_finish =
                    tx_tank_level_switch_to_tcp.clone();
                let tx_tank_level_switch_to_tcp_opt = Some(tx_tank_level_switch_to_tcp);
                let rx_tank_level_switch_from_tcp_opt = Some(rx_tank_level_switch_from_tcp);

                tank_level_switch.execute(
                    &tx_tank_level_switch_to_tcp_opt,
                    &rx_tank_level_switch_from_tcp_opt,
                    &tx_tank_level_switch_to_data_logger,
                    &rx_tank_level_switch_from_data_logger,
                    &tx_tank_level_switch_to_refill,
                    &rx_tank_level_switch_from_refill,
                    &tx_tank_level_switch_to_heating,
                    &rx_tank_level_switch_from_heating,
                    &tx_tank_level_switch_to_signal_handler,
                    &rx_tank_level_switch_from_signal_handler,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_tank_level_switch_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
            })
            .unwrap();

        join_handle_mock_heating
            .join()
            .expect("Mock heating thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_mock_tcp
            .join()
            .expect("Mock TCP thread did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock data logger thread did not finish.");
        join_handle_mock_refill
            .join()
            .expect("Mock refill thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");

        println!("* [TankLevelSwitch] testing if stabilized tank level position is high when invalid bit is true succeeded.");
    }

    #[test]
    pub fn test_refill_calc_tank_level_switch_position_stabilized_false_true() {
        // test if position signal is updating without delay when invalid signal is false
        let (tx_tank_level_switch_to_tcp, rx_tcp_from_tank_level_switch) = mpsc::channel();
        let (tx_tcp_to_tank_level_switch, rx_tank_level_switch_from_tcp) = mpsc::channel();
        let (tx_tank_level_switch_to_refill, rx_refill_from_tank_level_switch) = mpsc::channel();
        let (tx_refill_to_tank_level_switch, rx_tank_level_switch_from_refill) = mpsc::channel();
        let (tx_tank_level_switch_to_heating, rx_heating_from_tank_level_switch) = mpsc::channel();
        let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating) = mpsc::channel();
        let (tx_tank_level_switch_to_data_logger, rx_data_logger_from_tank_level_switch) =
            mpsc::channel();
        let (tx_data_logger_to_tank_level_switch, rx_tank_level_switch_from_data_logger) =
            mpsc::channel();
        let (tx_tank_level_switch_to_signal_handler, rx_signal_handler_from_tank_level_switch) =
            mpsc::channel();
        let (tx_signal_handler_to_tank_level_switch, rx_tank_level_switch_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_test_environment_to_tcp, rx_tcp_from_test_environment) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_refill.toml".to_string());
        let duration_test_case_part_1: u64 = 3; // three seconds with level switch in low position
        let duration_test_case_part_2: u64 = 2; // two seconds with level switch in high position
        let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, false);

        // thread for mock tcp
        let join_handle_mock_tcp = thread::Builder::new()
            .name("mock_tcp".to_string())
            .spawn(move || {
                mock_tcp(
                    &tx_tcp_to_tank_level_switch,
                    &rx_tcp_from_tank_level_switch,
                    Some(&rx_tcp_from_test_environment),
                    false,
                    false,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_tank_level_switch,
                    &rx_data_logger_from_tank_level_switch,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        // thread for mock heating
        let join_handle_mock_heating = thread::Builder::new()
            .name("mock_heating".to_string())
            .spawn(move || {
                mock_heating(
                    Some(&tx_heating_to_tank_level_switch),
                    Some(&rx_heating_from_tank_level_switch),
                    &rx_heating_from_signal_handler,
                    None,
                    None,
                    None,
                    false,
                );
            })
            .unwrap();

        // thread for mock refill - includes assertions
        let join_handle_mock_refill = thread::Builder::new()
            .name("mock_refill".to_string())
            .spawn(move || {
                let results = mock_refill(
                    Some(&tx_refill_to_tank_level_switch),
                    Some(&rx_refill_from_tank_level_switch),
                    &rx_refill_from_signal_handler,
                    None,
                    None,
                    None,
                    false,
                );
                assert_ge!(results.len(), 40);
                assert_le!(results.len(), 50);
                // check if value of all elements of result vector
                for (idx, result) in results.iter().enumerate() {
                    println!("Result at position {}: {:?}", idx, result);
                    if idx <= 25 {
                        assert_eq!(*result, false);
                    }
                    if idx >= 30 {
                        assert_eq!(*result, true);
                    }
                }
            })
            .unwrap();

        let tx_test_environment_to_tcp_for_test_case_finish = tx_tank_level_switch_to_tcp.clone();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_part_1 = Duration::from_secs(duration_test_case_part_1.into());
                let sleep_duration_part_2 = Duration::from_secs(duration_test_case_part_2.into());
                let sleep_duration_thread_processing = Duration::from_millis(50);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_part_1);
                let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                    MockSignal::TankLevelSwitchPosition(true),
                ));
                spin_sleeper.sleep(sleep_duration_part_2);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                spin_sleeper.sleep(sleep_duration_thread_processing);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                rx_signal_handler_from_tank_level_switch.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);
                let _ = tx_test_environment_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_tank_level_switch_to_tcp_opt = Some(tx_tank_level_switch_to_tcp);
                let rx_tank_level_switch_from_tcp_opt = Some(rx_tank_level_switch_from_tcp);

                tank_level_switch.execute(
                    &tx_tank_level_switch_to_tcp_opt,
                    &rx_tank_level_switch_from_tcp_opt,
                    &tx_tank_level_switch_to_data_logger,
                    &rx_tank_level_switch_from_data_logger,
                    &tx_tank_level_switch_to_refill,
                    &rx_tank_level_switch_from_refill,
                    &tx_tank_level_switch_to_heating,
                    &rx_tank_level_switch_from_heating,
                    &tx_tank_level_switch_to_signal_handler,
                    &rx_tank_level_switch_from_signal_handler,
                );
            })
            .unwrap();

        join_handle_mock_heating
            .join()
            .expect("Mock heating thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_mock_tcp
            .join()
            .expect("Mock TCP thread did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock data logger thread did not finish.");
        join_handle_mock_refill
            .join()
            .expect("Mock refill thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");

        println!("* [TankLevelSwitch] testing immediate change false -> true for stabilized tank level position when invalid bit is false succeeded");
    }

    #[test]
    pub fn test_refill_calc_tank_level_switch_position_stabilized_true_false() {
        // test if position signal is updating with delay when invalid signal is false
        let (tx_tank_level_switch_to_tcp, rx_tcp_from_tank_level_switch) = mpsc::channel();
        let (tx_tcp_to_tank_level_switch, rx_tank_level_switch_from_tcp) = mpsc::channel();
        let (tx_tank_level_switch_to_refill, rx_refill_from_tank_level_switch) = mpsc::channel();
        let (tx_refill_to_tank_level_switch, rx_tank_level_switch_from_refill) = mpsc::channel();
        let (tx_tank_level_switch_to_heating, rx_heating_from_tank_level_switch) = mpsc::channel();
        let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating) = mpsc::channel();
        let (tx_tank_level_switch_to_data_logger, rx_data_logger_from_tank_level_switch) =
            mpsc::channel();
        let (tx_data_logger_to_tank_level_switch, rx_tank_level_switch_from_data_logger) =
            mpsc::channel();
        let (tx_tank_level_switch_to_signal_handler, rx_signal_handler_from_tank_level_switch) =
            mpsc::channel();
        let (tx_signal_handler_to_tank_level_switch, rx_tank_level_switch_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_test_environment_to_tcp, rx_tcp_from_test_environment) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_refill.toml".to_string());
        let duration_test_case_part_1: u64 = 3; // three seconds with level switch in low position
        let duration_test_case_part_2: u64 = 8; // two seconds with level switch in high position
        let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true);

        // thread for mock tcp
        let join_handle_mock_tcp = thread::Builder::new()
            .name("mock_tcp".to_string())
            .spawn(move || {
                mock_tcp(
                    &tx_tcp_to_tank_level_switch,
                    &rx_tcp_from_tank_level_switch,
                    Some(&rx_tcp_from_test_environment),
                    true,
                    false,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_tank_level_switch,
                    &rx_data_logger_from_tank_level_switch,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        // thread for mock heating
        let join_handle_mock_heating = thread::Builder::new()
            .name("mock_heating".to_string())
            .spawn(move || {
                mock_heating(
                    Some(&tx_heating_to_tank_level_switch),
                    Some(&rx_heating_from_tank_level_switch),
                    &rx_heating_from_signal_handler,
                    None,
                    None,
                    None,
                    false,
                );
            })
            .unwrap();

        // thread for mock refill - includes assertions
        let join_handle_mock_refill = thread::Builder::new()
            .name("mock_refill".to_string())
            .spawn(move || {
                let results = mock_refill(
                    Some(&tx_refill_to_tank_level_switch),
                    Some(&rx_refill_from_tank_level_switch),
                    &rx_refill_from_signal_handler,
                    None,
                    None,
                    None,
                    false,
                );
                assert_ge!(results.len(), 95);
                assert_le!(results.len(), 105);
                // check if value of all elements of result vector
                for (idx, result) in results.iter().enumerate() {
                    println!("Result at position {}: {:?}", idx, result);
                    if idx <= 75 {
                        assert_eq!(*result, true);
                    }
                    if idx >= 80 {
                        assert_eq!(*result, false);
                    }
                }
            })
            .unwrap();

        let tx_test_environment_to_tcp_for_test_case_finish = tx_tank_level_switch_to_tcp.clone();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_part_1 = Duration::from_secs(duration_test_case_part_1.into());
                let sleep_duration_part_2 = Duration::from_secs(duration_test_case_part_2.into());
                let sleep_duration_thread_processing = Duration::from_millis(50);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_part_1);
                let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                    MockSignal::TankLevelSwitchPosition(false),
                ));
                spin_sleeper.sleep(sleep_duration_part_2);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                spin_sleeper.sleep(sleep_duration_thread_processing);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                rx_signal_handler_from_tank_level_switch.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);
                let _ = tx_test_environment_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_tank_level_switch_to_tcp_opt = Some(tx_tank_level_switch_to_tcp);
                let rx_tank_level_switch_from_tcp_opt = Some(rx_tank_level_switch_from_tcp);

                tank_level_switch.execute(
                    &tx_tank_level_switch_to_tcp_opt,
                    &rx_tank_level_switch_from_tcp_opt,
                    &tx_tank_level_switch_to_data_logger,
                    &rx_tank_level_switch_from_data_logger,
                    &tx_tank_level_switch_to_refill,
                    &rx_tank_level_switch_from_refill,
                    &tx_tank_level_switch_to_heating,
                    &rx_tank_level_switch_from_heating,
                    &tx_tank_level_switch_to_signal_handler,
                    &rx_tank_level_switch_from_signal_handler,
                );
            })
            .unwrap();

        join_handle_mock_heating
            .join()
            .expect("Mock heating thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_mock_tcp
            .join()
            .expect("Mock TCP thread did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock data logger thread did not finish.");
        join_handle_mock_refill
            .join()
            .expect("Mock refill thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");

        println!("* [TankLevelSwitch] checking delayed change true -> false for stabilized tank level position when invalid bit is false succeeded.");
    }

    #[test]
    pub fn test_refill_calc_tank_level_switch_position_stabilized_is_true_with_toggling_input() {
        // test if position stabilized signal is not updating when raw input signal is toggling
        let (tx_tank_level_switch_to_tcp, rx_tcp_from_tank_level_switch) = mpsc::channel();
        let (tx_tcp_to_tank_level_switch, rx_tank_level_switch_from_tcp) = mpsc::channel();
        let (tx_tank_level_switch_to_refill, rx_refill_from_tank_level_switch) = mpsc::channel();
        let (tx_refill_to_tank_level_switch, rx_tank_level_switch_from_refill) = mpsc::channel();
        let (tx_tank_level_switch_to_heating, rx_heating_from_tank_level_switch) = mpsc::channel();
        let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating) = mpsc::channel();
        let (tx_tank_level_switch_to_data_logger, rx_data_logger_from_tank_level_switch) =
            mpsc::channel();
        let (tx_data_logger_to_tank_level_switch, rx_tank_level_switch_from_data_logger) =
            mpsc::channel();
        let (tx_tank_level_switch_to_signal_handler, rx_signal_handler_from_tank_level_switch) =
            mpsc::channel();
        let (tx_signal_handler_to_tank_level_switch, rx_tank_level_switch_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_test_environment_to_tcp, rx_tcp_from_test_environment) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_refill.toml".to_string());
        let duration_test_case_part: u64 = 2; // two seconds with level switch before toggling
        let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true);

        // thread for mock tcp
        let join_handle_mock_tcp = thread::Builder::new()
            .name("mock_tcp".to_string())
            .spawn(move || {
                mock_tcp(
                    &tx_tcp_to_tank_level_switch,
                    &rx_tcp_from_tank_level_switch,
                    Some(&rx_tcp_from_test_environment),
                    true,
                    false,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_tank_level_switch,
                    &rx_data_logger_from_tank_level_switch,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        // thread for mock heating
        let join_handle_mock_heating = thread::Builder::new()
            .name("mock_heating".to_string())
            .spawn(move || {
                mock_heating(
                    Some(&tx_heating_to_tank_level_switch),
                    Some(&rx_heating_from_tank_level_switch),
                    &rx_heating_from_signal_handler,
                    None,
                    None,
                    None,
                    false,
                );
            })
            .unwrap();

        // thread for mock refill - includes assertions
        let join_handle_mock_refill = thread::Builder::new()
            .name("mock_refill".to_string())
            .spawn(move || {
                let results = mock_refill(
                    Some(&tx_refill_to_tank_level_switch),
                    Some(&rx_refill_from_tank_level_switch),
                    &rx_refill_from_signal_handler,
                    None,
                    None,
                    None,
                    false,
                );
                assert_ge!(results.len(), 70);
                assert_le!(results.len(), 76);
                // check if value of all elements of result vector
                assert_eq!(results.iter().all(|&item| item == true), true);
            })
            .unwrap();

        let tx_test_environment_to_tcp_for_test_case_finish = tx_tank_level_switch_to_tcp.clone();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_part = Duration::from_secs(duration_test_case_part.into());
                let sleep_duration_thread_processing = Duration::from_millis(50);
                let spin_sleeper = SpinSleeper::default();
                for _ in 0..2 {
                    spin_sleeper.sleep(sleep_duration_part);
                    let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                        MockSignal::TankLevelSwitchPosition(false),
                    ));
                    spin_sleeper.sleep(sleep_duration_part);
                    let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                        MockSignal::TankLevelSwitchPosition(true),
                    ));
                }
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                spin_sleeper.sleep(sleep_duration_thread_processing);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                rx_signal_handler_from_tank_level_switch.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);
                let _ = tx_test_environment_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_tank_level_switch_to_tcp_opt = Some(tx_tank_level_switch_to_tcp);
                let rx_tank_level_switch_from_tcp_opt = Some(rx_tank_level_switch_from_tcp);

                tank_level_switch.execute(
                    &tx_tank_level_switch_to_tcp_opt,
                    &rx_tank_level_switch_from_tcp_opt,
                    &tx_tank_level_switch_to_data_logger,
                    &rx_tank_level_switch_from_data_logger,
                    &tx_tank_level_switch_to_refill,
                    &rx_tank_level_switch_from_refill,
                    &tx_tank_level_switch_to_heating,
                    &rx_tank_level_switch_from_heating,
                    &tx_tank_level_switch_to_signal_handler,
                    &rx_tank_level_switch_from_signal_handler,
                );
            })
            .unwrap();

        join_handle_mock_heating
            .join()
            .expect("Mock heating thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_mock_tcp
            .join()
            .expect("Mock TCP thread did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock data logger thread did not finish.");
        join_handle_mock_refill
            .join()
            .expect("Mock refill thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");

        println!("* [TankLevelSwitch] checking delayed change true -> false for stabilized tank level position when invalid bit is false succeeded.");
    }
}
