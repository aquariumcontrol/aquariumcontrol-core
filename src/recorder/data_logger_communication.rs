/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::{error, warn};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
use crate::water::{refill::Refill, water_injection::WaterInjection};

const LOG_TARGET_DATA_LOGGER_COMMUNICATION: &str = "data_logger_communication";

/// Trait for the communication to data logger used by Refill and WaterInjection
pub trait DataLoggerCommunicationTrait {
    /// wrapper function implemented individually for Refill and WaterInjection
    /// to allow access to the struct variables.
    fn communicate_with_data_logger(
        &mut self,
        tx_to_data_logger: &Sender<bool>,
        rx_from_data_logger: &Receiver<InternalCommand>,
        refill_pump_status: bool,
    );

    /// Checks request from data logger and responds via channel
    /// # Arguments
    /// * `tx_to_data_logger` - Reference to sender part of channel for communication to data logger
    /// * `rx_from_data_logger` - Reference to Receiver part of channel for communication from Controllino
    /// * `refill_pump_status` - information if refill pump status is active (true) or inactive (false)
    /// ...
    fn communicate_with_data_logger_generic(
        tx_to_data_logger: &Sender<bool>,
        rx_from_data_logger: &Receiver<InternalCommand>,
        refill_pump_status: bool,
        lock_error_channel_receive_data_logger: &mut bool,
        lock_warn_inapplicable_command_data_logger: &mut bool,
        lock_error_invalid_signal_request_data_logger: &mut bool,
        lock_error_send_to_data_logger: &mut bool,
    ) {
        match rx_from_data_logger.try_recv() {
            Ok(c) => {
                *lock_error_channel_receive_data_logger = false;
                match c {
                    InternalCommand::RequestSignal(d) => {
                        *lock_warn_inapplicable_command_data_logger = false;
                        match d {
                            AquariumSignal::RefillPumpStatus => {
                                *lock_error_invalid_signal_request_data_logger = false;
                                match tx_to_data_logger.send(refill_pump_status) {
                                    Ok(_) => {
                                        *lock_error_send_to_data_logger = false;
                                    }
                                    Err(e) => {
                                        if !*lock_error_send_to_data_logger {
                                            error!(
                                                target: LOG_TARGET_DATA_LOGGER_COMMUNICATION,
                                                "channel communication to data logger failed ({e:?})"
                                            );
                                            *lock_error_send_to_data_logger = true;
                                        }
                                    }
                                };
                            }
                            _ => {
                                if !*lock_error_invalid_signal_request_data_logger {
                                    error!(
                                        target: LOG_TARGET_DATA_LOGGER_COMMUNICATION,
                                        "internal error: received request for invalid signal {} from data logger",
                                        d
                                    );
                                    *lock_error_invalid_signal_request_data_logger = true;
                                }
                            }
                        }
                    }
                    _ => {
                        if !*lock_warn_inapplicable_command_data_logger {
                            warn!(
                                target: LOG_TARGET_DATA_LOGGER_COMMUNICATION,
                                "ignoring inapplicable command from data logger thread."
                            );
                            *lock_warn_inapplicable_command_data_logger = true;
                        }
                    }
                }
            }
            Err(e) => {
                match e {
                    TryRecvError::Empty => {
                        /* empty buffer - no action required */
                        *lock_error_channel_receive_data_logger = false;
                    }
                    TryRecvError::Disconnected => {
                        if !*lock_error_channel_receive_data_logger {
                            error!(
                                target: LOG_TARGET_DATA_LOGGER_COMMUNICATION,
                                "receiving command from data logger thread failed ({e:?})"
                            );
                            *lock_error_channel_receive_data_logger = true;
                        }
                    }
                }
            }
        };
    }
}

impl DataLoggerCommunicationTrait for Refill {
    fn communicate_with_data_logger(
        &mut self,
        tx_to_data_logger: &Sender<bool>,
        rx_from_data_logger: &Receiver<InternalCommand>,
        refill_pump_status: bool,
    ) {
        Self::communicate_with_data_logger_generic(
            tx_to_data_logger,
            rx_from_data_logger,
            refill_pump_status,
            &mut self.lock_error_channel_receive_data_logger,
            &mut self.lock_warn_inapplicable_command_data_logger,
            &mut self.lock_error_invalid_signal_request_data_logger,
            &mut self.lock_error_send_to_data_logger,
        );
    }
}

impl DataLoggerCommunicationTrait for WaterInjection {
    fn communicate_with_data_logger(
        &mut self,
        tx_to_data_logger: &Sender<bool>,
        rx_from_data_logger: &Receiver<InternalCommand>,
        refill_pump_status: bool,
    ) {
        Self::communicate_with_data_logger_generic(
            tx_to_data_logger,
            rx_from_data_logger,
            refill_pump_status,
            &mut self.lock_error_channel_receive_data_logger,
            &mut self.lock_warn_inapplicable_command_data_logger,
            &mut self.lock_error_invalid_signal_request_data_logger,
            &mut self.lock_error_send_to_data_logger,
        );
    }
}
