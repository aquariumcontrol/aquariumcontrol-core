/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use std::sync::mpsc::{Receiver, TryRecvError};

use crate::channel_content::InternalCommand;
use crate::controllino::Controllino;
use crate::data_logger::DataLogger;
use crate::feed::Feed;
use crate::food_injection::FoodInjection;
use crate::gpio_handler::GpioHandler;

#[cfg(target_os = "linux")]
use crate::messaging::Messaging;

use crate::mineral_injection::MineralInjection;
use crate::refill::Refill;
use crate::schedule_check::ScheduleCheck;
use crate::tcp_communication::TcpCommunication;
use crate::water_injection::WaterInjection;

/// The trait implements processing of repeated patterns of channel communication from signal handler and from messaging.
/// Default implementation is used by Balling dosing control, heating control, ventilation control and monitors.
/// The following modules use specific implementations of this trait: Refill control, Data logger,
/// Food injection, Water injection, Mineral injection, Schedule check, Controllino, Gpio handler,
/// Feed, Messaging, TCP communication.
pub trait ProcessExternalRequestTrait {
    /// Checks if there are new messages in the channels.
    /// # Arguments
    /// * `rx_from_signal_handler` - reference to receiver part of channel for communication from signal handler
    /// * `rx_from_messaging` - reference to receiver part of channel for communication from messaging
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        rx_from_messaging: &Receiver<InternalCommand>,
    ) -> (bool, bool, bool) {
        let quit_command_received = match rx_from_signal_handler.try_recv() {
            Ok(c) => match c {
                InternalCommand::Quit => true,
                _ => {
                    println!("ProcessExternalRequestTrait: ignoring inapplicable command from signal handler.");
                    false
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        println!(
                        "ProcessExternalRequestTrait: Error in trying to receive command from signal handler: {e:?}"
                    );
                    }
                }
                false
            }
        };

        let (start_command_received, stop_command_received) = match rx_from_messaging.try_recv() {
            Ok(c) => match c {
                InternalCommand::Start => (true, false),
                InternalCommand::Stop => (false, true),
                _ => {
                    println!("ProcessExternalRequestTrait: ignoring inapplicable command from messaging.");
                    (false, false)
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        println!(
                        "ProcessExternalRequestTrait: Error in trying to receive command from signal handler: {e:?}"
                    );
                    }
                }
                (false, false)
            }
        };

        (
            quit_command_received,
            start_command_received,
            stop_command_received,
        )
    }
}

impl ProcessExternalRequestTrait for Refill {
    /// Checks if there are new messages in the channels.
    /// Implementation for Refill
    /// If the message contains the command to reset all errors, the function will reset all error bits.
    /// # Arguments
    /// * `rx_from_signal_handler` - reference to receiver part of channel for communication from signal handler
    /// * `rx_from_messaging` - reference to receiver part of channel for communication from messaging
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        rx_from_messaging: &Receiver<InternalCommand>,
    ) -> (bool, bool, bool) {
        let quit_command_received = match rx_from_signal_handler.try_recv() {
            Ok(c) => match c {
                InternalCommand::Quit => true,
                _ => {
                    println!("Refill: ignoring inapplicable command from signal handler.");
                    false
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        println!(
                            "Refill: Error in trying to receive command from signal handler: {e:?}"
                        );
                    }
                }
                false
            }
        };

        let (start_command_received, stop_command_received) = match rx_from_messaging.try_recv() {
            Ok(c) => match c {
                InternalCommand::Start => (true, false),
                InternalCommand::Stop => (false, true),
                InternalCommand::ResetAllErrors => {
                    self.refill_errors.reset_all_errors();
                    (false, false)
                }
                _ => {
                    println!("Refill: ignoring inapplicable command from messaging.");
                    (false, false)
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        println!(
                            "Refill: Error in trying to receive command from signal handler: {e:?}"
                        );
                    }
                }
                (false, false)
            }
        };

        (
            quit_command_received,
            start_command_received,
            stop_command_received,
        )
    }
}
/// Checks if there are new messages in the channel.
/// Associated function used by implementation for structs which only use signal handler and no messaging
/// # Arguments
/// * `rx_from_signal_handler` - reference to receiver part of channel for communication from signal handler
fn process_external_request_without_messaging(
    rx_from_signal_handler: &Receiver<InternalCommand>,
) -> (bool, bool, bool) {
    let quit_command_received = match rx_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Quit => true,
            _ => {
                println!("ProcessExternalRequestTrait: ignoring inapplicable command from signal handler.");
                false
            }
        },
        Err(e) => {
            match e {
                TryRecvError::Empty => { /* empty buffer - no action required */ }
                TryRecvError::Disconnected => {
                    println!(
                        "ProcessExternalRequestTrait: Error in trying to receive command from signal handler: {e:?}"
                    );
                }
            }
            false
        }
    };

    (quit_command_received, false, false)
}

impl ProcessExternalRequestTrait for DataLogger {
    /// Checks if there are new messages in the channel.
    /// Implementation for DataLogger
    /// # Arguments
    /// * `rx_from_signal_handler` - reference to receiver part of channel for communication from signal handler
    /// * `_` - ignored
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: &Receiver<InternalCommand>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(rx_from_signal_handler)
    }
}

impl ProcessExternalRequestTrait for FoodInjection {
    /// Checks if there are new messages in the channel.
    /// Implementation for FoodInjection
    /// # Arguments
    /// * `rx_from_signal_handler` - reference to receiver part of channel for communication from signal handler
    /// * `_` - ignored
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: &Receiver<InternalCommand>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(rx_from_signal_handler)
    }
}

impl ProcessExternalRequestTrait for MineralInjection {
    /// Checks if there are new messages in the channel.
    /// Implementation for MineralInjection
    /// # Arguments
    /// * `rx_from_signal_handler` - reference to receiver part of channel for communication from signal handler
    /// * `_` - ignored
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: &Receiver<InternalCommand>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(rx_from_signal_handler)
    }
}

impl ProcessExternalRequestTrait for WaterInjection {
    /// Checks if there are new messages in the channel.
    /// Implementation for WaterInjection
    /// # Arguments
    /// * `rx_from_signal_handler` - reference to receiver part of channel for communication from signal handler
    /// * `_` - ignored
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: &Receiver<InternalCommand>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(rx_from_signal_handler)
    }
}

impl ProcessExternalRequestTrait for ScheduleCheck {
    /// Checks if there are new messages in the channel.
    /// Implementation for ScheduleCheck
    /// # Arguments
    /// * `rx_from_signal_handler` - reference to receiver part of channel for communication from signal handler
    /// * `_` - ignored
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: &Receiver<InternalCommand>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(rx_from_signal_handler)
    }
}

impl ProcessExternalRequestTrait for Controllino {
    /// Checks if there are new messages in the channel.
    /// Implementation for Controllino
    /// # Arguments
    /// * `rx_from_signal_handler` - reference to receiver part of channel for communication from signal handler
    /// * `_` - ignored
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: &Receiver<InternalCommand>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(rx_from_signal_handler)
    }
}

impl ProcessExternalRequestTrait for GpioHandler {
    /// Checks if there are new messages in the channel.
    /// Implementation for GpioHandler
    /// # Arguments
    /// * `rx_from_signal_handler` - reference to receiver part of channel for communication from signal handler
    /// * `_` - ignored
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: &Receiver<InternalCommand>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(rx_from_signal_handler)
    }
}

#[cfg(target_os = "linux")]
impl ProcessExternalRequestTrait for Messaging {
    /// Checks if there are new messages in the channel.
    /// Implementation for Messaging
    /// # Arguments
    /// * `rx_from_signal_handler` - reference to receiver part of channel for communication from signal handler
    /// * `_` - ignored
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: &Receiver<InternalCommand>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(rx_from_signal_handler)
    }
}

impl ProcessExternalRequestTrait for TcpCommunication {
    /// Checks if there are new messages in the channel.
    /// Implementation for TcpCommunication
    /// # Arguments
    /// * `rx_from_signal_handler` - reference to receiver part of channel for communication from signal handler
    /// * `_` - ignored
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: &Receiver<InternalCommand>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(rx_from_signal_handler)
    }
}

impl ProcessExternalRequestTrait for Feed {
    /// Checks if there are new messages in the channels.
    /// Implementation for Feed
    /// # Arguments
    /// * `rx_from_signal_handler` - receiver part of channel    
    /// * `rx_from_messaging` - receiver part of channel    
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        rx_from_messaging: &Receiver<InternalCommand>,
    ) -> (bool, bool, bool) {
        let quit_command_received = match rx_from_signal_handler.try_recv() {
            Ok(c) => match c {
                InternalCommand::Quit => true,
                _ => {
                    println!("Feed: ignoring inapplicable command from signal handler.");
                    false
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        println!(
                            "Feed: Error in trying to receive command from signal handler: {e:?}"
                        );
                    }
                }
                false
            }
        };

        self.execute_command_received = false;
        self.profile_id_requested = -1;
        let (start_command_received, stop_command_received, execute_command_received, profile_id) =
            match rx_from_messaging.try_recv() {
                Ok(c) => match c {
                    InternalCommand::Start => (true, false, false, 0),
                    InternalCommand::Stop => (false, true, false, 0),
                    InternalCommand::Execute(c) => (false, false, true, c),
                    _ => {
                        println!("Feed: ignoring inapplicable command from messaging.");
                        (false, false, false, 0)
                    }
                },
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            println!(
                        "Heating: Error in trying to receive command from signal handler: {e:?}"
                    );
                        }
                    }
                    (false, false, false, 0)
                }
            };

        self.execute_command_received = execute_command_received;
        self.profile_id_requested = profile_id;

        (
            quit_command_received,
            start_command_received,
            stop_command_received,
        )
    }
}
