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
use crate::proc_ext_req::ProcessExternalRequestTrait;

/// Holds the configuration data for the tank level switch calculation.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Clone)]
pub struct TankLevelSwitchConfig {
    /// number of seconds which need to indicate a low level before the refill control assumes that the water level is low
    tank_level_switch_position_high_stabilization_count: u32,
}

const LOG_TARGET_TANK_LEVEL_SWITCH: &str = "tank_level_switch";

/// Contains the communication between Refill thread and GpioHandler thread via channels.
/// The struct holds attributes for the results and for error flags indicating unsuccessful communication.
pub struct TankLevelSwitch {
    /// configuration of tank level switch calculation
    config: TankLevelSwitchConfig,

    /// flag indicating if the last measurement of tank level switch indicates high (true) or low (false)
    pub tank_level_switch_position: bool,

    /// flag indicating if the stabilized tank level switch indicates high (true) or low (false)
    pub tank_level_switch_position_stabilized: bool,

    /// flag indicating that the tank level switch position may be inaccurate
    pub tank_level_switch_invalid: bool,

    /// time counter to determine if positition is stabilized
    tank_level_switch_position_high_stabilization_counter: u32,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure of sending request for tank level switch position via channel
    lock_error_channel_send_tank_level_switch_position: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure of receiving tank level switch position via channel
    lock_error_channel_receive_tank_level_switch_position: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure of sending request for tank level switch invalid via channel
    lock_error_channel_send_tank_level_switch_invalid: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure of receiving tank level switch invalid via channel
    lock_error_channel_receive_tank_level_switch_invalid: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command from signal handler
    lock_warn_inapplicable_command_signal_handler: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via channel
    lock_error_channel_receive_termination: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received request for signal which cannot be processed by this thread
    lock_error_invalid_signal_request: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received invalid command from refill thread
    lock_warn_inapplicable_command_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive request from refill thread
    lock_error_channel_receive_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send back value to refill thread
    lock_error_refill_response: bool,
}

impl ProcessExternalRequestTrait for TankLevelSwitch {}

impl TankLevelSwitch {
    /// provides a struct of type TankLevelSwitchSignals.
    pub fn new(config: TankLevelSwitchConfig) -> TankLevelSwitch {
        TankLevelSwitch {
            config: config,
            tank_level_switch_position: true, // initialization value assuming no error
            tank_level_switch_invalid: false, // initialization value assuming no error
            tank_level_switch_position_stabilized: true, // initialization value assuming no error
            tank_level_switch_position_high_stabilization_counter: 0,
            lock_error_channel_send_tank_level_switch_position: false,
            lock_error_channel_receive_tank_level_switch_position: false,
            lock_error_channel_send_tank_level_switch_invalid: false,
            lock_error_channel_receive_tank_level_switch_invalid: false,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
            lock_error_invalid_signal_request: false,
            lock_warn_inapplicable_command_refill: false,
            lock_error_channel_receive_refill: false,
            lock_error_refill_response: false,
        }
    }

    /// Executes the channel communication to update the tank level switch position signal
    /// as well as the tank level switch invalid signal.
    /// # Arguments
    /// * `tx_to_gpio` - sender part of the channel for communication to GPIO handler
    /// * `rx_from_gpio` - receiver part of the channel for communication from GPIO handler
    pub fn update_tank_level_switch_signals(
        &mut self,
        tx_to_gpio: &Sender<InternalCommand>,
        rx_from_gpio: &Receiver<f32>,
    ) {
        // send request for tank level switch position
        match tx_to_gpio.send(InternalCommand::RequestSignal(
            AquariumSignal::TankLevelSwitchPosition,
        )) {
            Ok(_) => {
                // Sent request for tank level switch position.
                // now listen for response
                self.tank_level_switch_position = match rx_from_gpio.recv() {
                    Ok(c) => {
                        self.lock_error_channel_receive_tank_level_switch_position = false;
                        c > 0.0
                    }
                    Err(e) => {
                        if !self.lock_error_channel_receive_tank_level_switch_position {
                            error!(
                                target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                "receiving tank level switch position from GPIO handler failed ({e:?})"
                            );
                            self.lock_error_channel_receive_tank_level_switch_position = true;
                        }
                        false
                    }
                };
                self.lock_error_channel_send_tank_level_switch_position = false;
            }
            Err(e) => {
                if !self.lock_error_channel_send_tank_level_switch_position {
                    error!(
                        target: LOG_TARGET_TANK_LEVEL_SWITCH,
                        "channel communication for GPIO handler for tank level switch position failed ({e:?})"
                    );
                    self.lock_error_channel_send_tank_level_switch_position = true;
                }
            }
        }

        // send request for tank level switch invalid
        match tx_to_gpio.send(InternalCommand::RequestSignal(
            AquariumSignal::TankLevelSwitchInvalid,
        )) {
            Ok(_) => {
                // now listen for response
                self.tank_level_switch_invalid = match rx_from_gpio.recv() {
                    Ok(c) => {
                        self.lock_error_channel_receive_tank_level_switch_invalid = false;
                        c > 0.0
                    }
                    Err(e) => {
                        if !self.lock_error_channel_receive_tank_level_switch_invalid {
                            error!(
                                target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                "receiving tank level switch invalid from GPIO handler failed ({e:?})"
                            );
                            self.lock_error_channel_receive_tank_level_switch_invalid = true;
                        }
                        true
                    }
                };
                self.lock_error_channel_send_tank_level_switch_invalid = false;
            }
            Err(e) => {
                if !self.lock_error_channel_send_tank_level_switch_invalid {
                    error!(
                        target: LOG_TARGET_TANK_LEVEL_SWITCH,
                        "channel communication for GPIO handler for tank level switch invalid failed ({e:?})"
                    );
                    self.lock_error_channel_send_tank_level_switch_invalid = true;
                }
            }
        }
    }

    /// Calculates a stabilized tank level switch position signal considering:
    /// - if invalid bit of signal is true, then use the current value of the stabilized position signal.
    /// - if the new signal is true (indicating high water level), then set the stabilized signal immediately to true (high).
    /// - if the the new signal is false (indicating low water level), then set increase the stabilization counter.
    /// - if the stabiliyation counter exceeds the configured limit, then set the counter to zero and set the stabilized
    /// signal to true indicating high water level.
    /// # Arguments
    /// * `tank_level_switch_signals` - struct containing the refill-relevant signal data
    fn calc_tank_level_switch_position_stabilized(&mut self) {
        if !self.tank_level_switch_invalid {
            // update stabilized signal only when new signal is not invalid
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
                    {
                        self.tank_level_switch_position_high_stabilization_counter += 1;
                        true
                    } else {
                        false
                    }
                }
            }
        }
    }

    pub fn execute(
        &mut self,
        tx_to_gpio: Sender<InternalCommand>,
        rx_from_gpio: Receiver<f32>,
        tx_to_refill: Sender<f32>,
        rx_from_refill: Receiver<InternalCommand>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        let sleep_duration_one_second = Duration::from_secs(1);
        let sleep_duration_one_millisec = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        loop {
            let (
                quit_command_received, // request to end the application has been received
                _,
                _,
            ) = self.process_external_request(&rx_from_signal_handler, None);

            if quit_command_received {
                break;
            }

            // either communicate with real HW or with simulator to get updated signal information
            self.update_tank_level_switch_signals(&tx_to_gpio, &rx_from_gpio);

            self.calc_tank_level_switch_position_stabilized();

            let response_to_refill = match rx_from_refill.try_recv() {
                Ok(c) => {
                    self.lock_error_channel_receive_refill = false;
                    match c {
                        InternalCommand::RequestSignal(d) => {
                            self.lock_warn_inapplicable_command_refill = false;
                            match d {
                                AquariumSignal::TankLevelSwitchPosition => {
                                    self.lock_error_invalid_signal_request = false;
                                    Some(self.tank_level_switch_position)
                                }
                                AquariumSignal::TankLevelSwitchInvalid => {
                                    self.lock_error_invalid_signal_request = false;
                                    Some(self.tank_level_switch_invalid)
                                }
                                AquariumSignal::TankLevelSwitchPositionStabilized => {
                                    self.lock_error_invalid_signal_request = false;
                                    Some(self.tank_level_switch_position_stabilized)
                                }
                                _ => {
                                    if !self.lock_error_invalid_signal_request {
                                        error!(
                                            target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                            "internal error: received request for invalid signal {}",
                                            d
                                        );
                                        self.lock_error_invalid_signal_request = true;
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
                        Err(e) => {
                            if !self.lock_error_refill_response {
                                error!(
                                    target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                    "responding back to refill thread failed ({e:?})"
                                );
                                self.lock_error_refill_response = true;
                            }
                        }
                    };
                }
                None => {
                    // do nothing
                    self.lock_error_refill_response = false;
                }
            };

            spin_sleeper.sleep(sleep_duration_one_second);
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
