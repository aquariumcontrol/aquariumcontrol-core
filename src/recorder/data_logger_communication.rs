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
    /// Wrapper function implemented individually for Refill and WaterInjection
    /// to allow access to the struct variables.
    /// # Arguments
    /// * `tx_to_data_logger` - The sender channel for sending the boolean status back to the data logger.
    /// * `rx_from_data_logger` - The receiver channel for getting `InternalCommand` requests from the data logger.
    /// * `refill_pump_status` - The current status of the refill pump (`true` for ON, `false` for OFF).
    fn communicate_with_data_logger(
        &mut self,
        tx_to_data_logger: &Sender<bool>,
        rx_from_data_logger: &Receiver<InternalCommand>,
        refill_pump_status: bool,
    );

    /// Checks for a `RequestSignal` command from the data logger and responds with the refill pump's status.
    ///
    /// This private helper function is a generic communication handler designed for modules
    /// (like Refill, Heating, Ventilation) that provide ON/OFF signal information to the data logger.
    /// It attempts to receive a command without blocking. If it's a `RequestSignal` for `RefillPumpStatus`,
    /// it sends the current `refill_pump_status` back to the data logger. Other commands or
    /// invalid signal requests are logged as warnings or errors.
    ///
    /// It uses "lock" variables (`lock_error_channel_receive_data_logger`, etc.) to prevent
    /// log flooding from repeated communication failures.
    ///
    /// # Arguments
    /// * `tx_to_data_logger` - The sender channel for sending the boolean status back to the data logger.
    /// * `rx_from_data_logger` - The receiver channel for getting `InternalCommand` requests from the data logger.
    /// * `refill_pump_status` - The current status of the refill pump (`true` for ON, `false` for OFF).
    /// * `lock_error_channel_receive_data_logger` - A mutable flag to inhibit repeated error logging for receiving failures.
    /// * `lock_warn_inapplicable_command_data_logger` - A mutable flag to inhibit repeated warning logging for inapplicable commands.
    /// * `lock_error_invalid_signal_request_data_logger` - A mutable flag to inhibit repeated error logging for invalid signal requests.
    /// * `lock_error_send_to_data_logger` - A mutable flag to inhibit repeated error logging for sending failures.
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
    /// Communicates the refill pump's status to the data logger upon request.
    ///
    /// This implementation of `communicate_with_data_logger` for the `Refill` module
    /// delegates the actual communication handling to a generic helper function.
    /// It provides the `refill_pump_status` and the necessary lock variables
    /// to manage error logging for channel communication.
    ///
    /// # Arguments
    /// * `tx_to_data_logger` - The sender channel for sending the boolean status to the data logger.
    /// * `rx_from_data_logger` - The receiver channel for getting `InternalCommand` requests from the data logger.
    /// * `refill_pump_status` - The current status of the refill pump (`true` for ON, `false` for OFF).
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
    /// Communicates the water injection pump's status to the data logger upon request.
    ///
    /// This implementation of `communicate_with_data_logger` for the `WaterInjection` module
    /// delegates the actual communication handling to a generic helper function.
    /// It provides the `refill_pump_status` (which acts as the water injection pump's status in this context)
    /// and the necessary lock variables to manage error logging for channel communication.
    ///
    /// # Arguments
    /// * `tx_to_data_logger` - The sender channel for sending the boolean status to the data logger.
    /// * `rx_from_data_logger` - The receiver channel for getting `InternalCommand` requests from the data logger.
    /// * `refill_pump_status` - The current status of the refill pump (`true` for ON, `false` for OFF).
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
