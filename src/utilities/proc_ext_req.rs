/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::{error, warn};

#[cfg(not(test))]
use log::info;

use std::sync::mpsc::{Receiver, TryRecvError};

use crate::food::{feed::Feed, food_injection::FoodInjection};
use crate::recorder::data_logger::DataLogger;
use crate::relays::relay_manager::RelayManager;
use crate::utilities::channel_content::InternalCommand;

#[cfg(target_os = "linux")]
use crate::dispatch::messaging::Messaging;

use crate::mineral::{balling::Balling, mineral_injection::MineralInjection};
use crate::permission::schedule_check::ScheduleCheck;
use crate::sensors::i2c_interface::I2cInterface;
use crate::simulator::tcp_communication::TcpCommunication;
use crate::watchmen::watchdog::Watchdog;
use crate::water::{refill::Refill, water_injection::WaterInjection};

const LOG_TARGET_PROC_EXT_REQ: &str = "proc_ext_req";

/// This trait defines a common interface for processing external requests, such as
/// commands received from a signal handler or a messaging system. It's designed
/// to be implemented by various control modules in the system.
///
/// The default implementation provided here handles basic "Quit," "Start," and "Stop"
/// commands, logging warnings for other inapplicable commands and errors for channel
/// disconnection. This default is used by modules like heating control, ventilation control,
/// and monitors.
///
/// More specialized modules, including Refill control, Data logger, Balling, Food injection,
/// Water injection, Mineral injection, Schedule check, Controllino, Gpio handler, Feed,
/// Messaging, and TCP communication, provide their own specific implementations of this trait.
pub trait ProcessExternalRequestTrait {
    /// Checks for and processes new commands received from the signal handler and an optional messaging channel.
    ///
    /// This function attempts to receive `InternalCommand` messages without blocking from two potential sources:
    /// a required channel from the signal handler and an optional channel from a messaging system.
    /// It specifically looks for `Quit` commands from the signal handler and `Start`/`Stop` commands
    /// from the messaging channel, ignoring other commands and logging warnings for them.
    /// Channel disconnection errors are also logged.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - A reference to the receiver end of the channel
    ///   for commands originating from the signal handler.
    /// * `rx_from_messaging_opt` - An `Option` containing a reference to the receiver end
    ///   of the channel for commands from the messaging system. This channel is optional.
    ///
    /// # Returns
    /// A tuple `(bool, bool, bool)` indicating the status of specific commands received:
    /// - The first `bool` is `true` if a `Quit` command was received from the signal handler; otherwise `false`.
    /// - The second `bool` is `true` if a `Start` command was received from messaging; otherwise `false`.
    /// - The third `bool` is `true` if a `Stop` command was received from messaging; otherwise `false`.
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        rx_from_messaging_opt: Option<&Receiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        let quit_command_received = match rx_from_signal_handler.try_recv() {
            Ok(c) => match c {
                InternalCommand::Quit => true,
                _ => {
                    warn!(
                        target: LOG_TARGET_PROC_EXT_REQ,
                        "ProcessExternalRequestTrait: ignoring inapplicable command from signal handler."
                    );
                    false
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        #[cfg(all(not(test), not(feature = "debug_signal_handler")))]
                        error!(
                            target: LOG_TARGET_PROC_EXT_REQ,
                            "receiving command from signal handler failed ({e:?})"
                        );
                    }
                }
                false
            }
        };

        let (start_command_received, stop_command_received) = match rx_from_messaging_opt {
            Some(rx_from_messaging) => {
                match rx_from_messaging.try_recv() {
                    Ok(c) => match c {
                        InternalCommand::Start => (true, false),
                        InternalCommand::Stop => (false, true),
                        _ => {
                            warn!(
                                target: LOG_TARGET_PROC_EXT_REQ,
                                "ProcessExternalRequestTrait: ignoring inapplicable command from messaging."
                            );
                            (false, false)
                        }
                    },
                    Err(e) => {
                        match e {
                            TryRecvError::Empty => { /* empty buffer - no action required */ }
                            TryRecvError::Disconnected => {
                                #[cfg(not(test))]
                                error!(
                                    target: LOG_TARGET_PROC_EXT_REQ,
                                    "(standard) receiving command from messaging failed ({e:?})"
                                );
                            }
                        }
                        (false, false)
                    }
                }
            }
            None => (false, false),
        };

        (
            quit_command_received,
            start_command_received,
            stop_command_received,
        )
    }
}

impl ProcessExternalRequestTrait for Refill {
    /// Checks for and processes new commands relevant to the Refill control module from external channels.
    ///
    /// This is the specialized implementation of `ProcessExternalRequestTrait` for the `Refill` module.
    /// In addition to handling standard "Quit", "Start", and "Stop" commands, it specifically
    /// intercepts the `InternalCommand::ResetAllErrors` command from the messaging channel
    /// to reset the refill control's internal error states.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - A reference to the receiver end of the channel
    ///   for commands originating from the signal handler.
    /// * `rx_from_messaging_opt` - An `Option` containing a reference to the receiver end
    ///   of the channel for commands from the messaging system. This channel is optional.
    ///
    /// # Returns
    /// A tuple `(bool, bool, bool)` indicating the status of specific commands received:
    /// - The first `bool` is `true` if a `Quit` command was received from the signal handler; otherwise `false`.
    /// - The second `bool` is `true` if a `Start` command was received from messaging; otherwise `false`.
    /// - The third `bool` is `true` if a `Stop` command was received from messaging; otherwise `false`.
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        rx_from_messaging_opt: Option<&Receiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        let quit_command_received = match rx_from_signal_handler.try_recv() {
            Ok(c) => match c {
                InternalCommand::Quit => true,
                _ => {
                    warn!(
                        target: LOG_TARGET_PROC_EXT_REQ,
                        "ignoring inapplicable command from signal handler."
                    );
                    false
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        #[cfg(not(test))]
                        error!(
                            target: LOG_TARGET_PROC_EXT_REQ,
                            "receiving command from signal handler failed ({e:?})"
                        );
                    }
                }
                false
            }
        };

        let (start_command_received, stop_command_received) = match rx_from_messaging_opt {
            Some(rx_from_messaging) => {
                match rx_from_messaging.try_recv() {
                    Ok(c) => match c {
                        InternalCommand::Start => (true, false),
                        InternalCommand::Stop => (false, true),
                        InternalCommand::ResetAllErrors => {
                            #[cfg(not(test))]
                            info!(
                                target: LOG_TARGET_PROC_EXT_REQ,
                                "Received reset command for errors of refill control."
                            );
                            self.refill_errors.reset_all_errors();
                            (false, false)
                        }
                        _ => {
                            warn!(
                                target: LOG_TARGET_PROC_EXT_REQ,
                                "ignoring inapplicable command from messaging."
                            );
                            (false, false)
                        }
                    },
                    Err(e) => {
                        match e {
                            TryRecvError::Empty => { /* empty buffer - no action required */ }
                            TryRecvError::Disconnected => {
                                #[cfg(not(test))]
                                error!(
                                    target: LOG_TARGET_PROC_EXT_REQ,
                                    "(refill) receiving command from messaging failed ({e:?})"
                                );
                            }
                        }
                        (false, false)
                    }
                }
            }
            None => (false, false),
        };

        (
            quit_command_received,
            start_command_received,
            stop_command_received,
        )
    }
}

/// Checks for new commands from the signal handler, specifically for modules that don't use messaging.
///
/// This associated function provides a streamlined way to process external requests
/// for components that only listen to the signal handler channel. It attempts to
/// receive without blocking `InternalCommand` messages and specifically looks for
/// a `Quit` command.
///
/// # Arguments
/// * `rx_from_signal_handler` - A reference to the receiver end of the channel
///   for commands originating from the signal handler.
///
/// # Returns
/// A tuple `(bool, bool, bool)` indicating the status of commands received:
/// - The first `bool` is `true` if a `Quit` command was received; otherwise `false`.
/// - The second `bool` is always `false` as "Start" commands are not processed by this function.
/// - The third `bool` is always `false` as "Stop" commands are not processed by this function.
fn process_external_request_without_messaging(
    rx_from_signal_handler: &Receiver<InternalCommand>,
    sender: String,
) -> (bool, bool, bool) {
    let quit_command_received = match rx_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Quit => true,
            _ => {
                warn!(
                    target: LOG_TARGET_PROC_EXT_REQ,
                    "ignoring inapplicable command from signal handler."
                );
                false
            }
        },
        Err(e) => {
            match e {
                TryRecvError::Empty => { /* empty buffer - no action required */ }
                TryRecvError::Disconnected => {
                    error!(
                        target: LOG_TARGET_PROC_EXT_REQ,
                        "({}) receiving command from signal handler failed ({e:?})",
                        sender
                    );
                }
            }
            false
        }
    };

    (quit_command_received, false, false)
}

impl ProcessExternalRequestTrait for DataLogger {
    /// Checks for and processes new commands relevant to the Data Logger module from external channels.
    ///
    /// This is the specialized implementation of `ProcessExternalRequestTrait` for the `DataLogger`.
    /// It delegates directly to `process_external_request_without_messaging`, indicating
    /// that the `DataLogger` only processes commands from the signal handler and
    /// ignores any input from a messaging channel.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - A reference to the receiver end of the channel
    ///   for commands originating from the signal handler.
    /// * `_` - This parameter is ignored, as the `DataLogger` does not process
    ///   messages from a messaging channel.
    ///
    /// # Returns
    /// A tuple `(bool, bool, bool)` indicating the status of commands received:
    /// - The first `bool` is `true` if a `Quit` command was received; otherwise `false`.
    /// - The second `bool` is always `false` (no "Start" commands processed).
    /// - The third `bool` is always `false` (no "Stop" commands processed).
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: Option<&Receiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(rx_from_signal_handler, "DataLogger".to_string())
    }
}

impl ProcessExternalRequestTrait for FoodInjection {
    /// Checks for and processes new commands relevant to the Food Injection module from external channels.
    ///
    /// This is the specialized implementation of `ProcessExternalRequestTrait` for `FoodInjection`.
    /// It delegates directly to `process_external_request_without_messaging`, indicating
    /// that the `FoodInjection` module only processes commands from the signal handler
    /// and ignores any input from a messaging channel.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - A reference to the receiver end of the channel
    ///   for commands originating from the signal handler.
    /// * `_` - This parameter is ignored, as the `FoodInjection` module does not process
    ///   messages from a messaging channel.
    ///
    /// # Returns
    /// A tuple `(bool, bool, bool)` indicating the status of commands received:
    /// - The first `bool` is `true` if a `Quit` command was received; otherwise `false`.
    /// - The second `bool` is always `false` (no "Start" commands processed).
    /// - The third `bool` is always `false` (no "Stop" commands processed).
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: Option<&Receiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(
            rx_from_signal_handler,
            "FoodInjection".to_string(),
        )
    }
}

impl ProcessExternalRequestTrait for MineralInjection {
    /// Checks for and processes new commands relevant to the Mineral Injection module from external channels.
    ///
    /// This is the specialized implementation of `ProcessExternalRequestTrait` for `MineralInjection`.
    /// It delegates directly to `process_external_request_without_messaging`, indicating
    /// that the `MineralInjection` module only processes commands from the signal handler
    /// and ignores any input from a messaging channel.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - A reference to the receiver end of the channel
    ///   for commands originating from the signal handler.
    /// * `_` - This parameter is ignored, as the `MineralInjection` module does not process
    ///   messages from a messaging channel.
    ///
    /// # Returns
    /// A tuple `(bool, bool, bool)` indicating the status of commands received:
    /// - The first `bool` is `true` if a `Quit` command was received; otherwise `false`.
    /// - The second `bool` is always `false` (no "Start" commands processed).
    /// - The third `bool` is always `false` (no "Stop" commands processed).
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: Option<&Receiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(
            rx_from_signal_handler,
            "MineralInjection".to_string(),
        )
    }
}

impl ProcessExternalRequestTrait for WaterInjection {
    /// Checks for and processes new commands relevant to the Water Injection module from external channels.
    ///
    /// This is the specialized implementation of `ProcessExternalRequestTrait` for `WaterInjection`.
    /// It delegates directly to `process_external_request_without_messaging`, indicating
    /// that the `WaterInjection` module only processes commands from the signal handler
    /// and ignores any input from a messaging channel.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - A reference to the receiver end of the channel
    ///   for commands originating from the signal handler.
    /// * `_` - This parameter is ignored, as the `WaterInjection` module does not process
    ///   messages from a messaging channel.
    ///
    /// # Returns
    /// A tuple `(bool, bool, bool)` indicating the status of commands received:
    /// - The first `bool` is `true` if a `Quit` command was received; otherwise `false`.
    /// - The second `bool` is always `false` (no "Start" commands processed).
    /// - The third `bool` is always `false` (no "Stop" commands processed).
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: Option<&Receiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(
            rx_from_signal_handler,
            "WaterInjection".to_string(),
        )
    }
}

impl ProcessExternalRequestTrait for I2cInterface {
    /// Checks for and processes new commands relevant to the I2cInterface module from external channels.
    ///
    /// This is the specialized implementation of `ProcessExternalRequestTrait` for `I2cInterface`.
    /// It delegates directly to `process_external_request_without_messaging`, indicating
    /// that the `I2cInterface` module only processes commands from the signal handler
    /// and ignores any input from a messaging channel.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - A reference to the receiver end of the channel
    ///   for commands originating from the signal handler.
    /// * `_` - This parameter is ignored, as the `I2cInterface` module does not process
    ///   messages from a messaging channel.
    ///
    /// # Returns
    /// A tuple `(bool, bool, bool)` indicating the status of commands received:
    /// - The first `bool` is `true` if a `Quit` command was received; otherwise `false`.
    /// - The second `bool` is always `false` (no "Start" commands processed).
    /// - The third `bool` is always `false` (no "Stop" commands processed).
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: Option<&Receiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(
            rx_from_signal_handler,
            "I2cInterface".to_string(),
        )
    }
}

impl ProcessExternalRequestTrait for ScheduleCheck {
    /// Checks for and processes new commands relevant to the Schedule Check module from external channels.
    ///
    /// This is the specialized implementation of `ProcessExternalRequestTrait` for `ScheduleCheck`.
    /// It delegates directly to `process_external_request_without_messaging`, indicating
    /// that the `ScheduleCheck` module only processes commands from the signal handler
    /// and ignores any input from a messaging channel.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - A reference to the receiver end of the channel
    ///   for commands originating from the signal handler.
    /// * `_` - This parameter is ignored, as the `ScheduleCheck` module does not process
    ///   messages from a messaging channel.
    ///
    /// # Returns
    /// A tuple `(bool, bool, bool)` indicating the status of commands received:
    /// - The first `bool` is `true` if a `Quit` command was received; otherwise `false`.
    /// - The second `bool` is always `false` (no "Start" commands processed).
    /// - The third `bool` is always `false` (no "Stop" commands processed).
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: Option<&Receiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(
            rx_from_signal_handler,
            "ScheduleCheck".to_string(),
        )
    }
}

impl ProcessExternalRequestTrait for RelayManager {
    /// Checks for and processes new commands relevant to the Relay Manager module from external channels.
    ///
    /// This is the specialized implementation of `ProcessExternalRequestTrait` for `RelayManager`.
    /// It delegates directly to `process_external_request_without_messaging`, indicating
    /// that the `RelayManager` (which often interacts with hardware like Controllino)
    /// only processes commands from the signal handler and ignores any input from a messaging channel.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - A reference to the receiver end of the channel
    ///   for commands originating from the signal handler.
    /// * `_` - This parameter is ignored, as the `RelayManager` does not process
    ///   messages from a messaging channel.
    ///
    /// # Returns
    /// A tuple `(bool, bool, bool)` indicating the status of commands received:
    /// - The first `bool` is `true` if a `Quit` command was received; otherwise `false`.
    /// - The second `bool` is always `false` (no "Start" commands processed).
    /// - The third `bool` is always `false` (no "Stop" commands processed).
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: Option<&Receiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(
            rx_from_signal_handler,
            "RelayManager".to_string(),
        )
    }
}

#[cfg(target_os = "linux")]
impl ProcessExternalRequestTrait for Messaging {
    /// Checks for and processes new commands relevant to the Messaging module from external channels.
    ///
    /// This is the specialized implementation of `ProcessExternalRequestTrait` for `Messaging`.
    /// It delegates directly to `process_external_request_without_messaging`, indicating
    /// that the `Messaging` module only processes commands from the signal handler
    /// and ignores any input from a separate messaging channel.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - A reference to the receiver end of the channel
    ///   for commands originating from the signal handler.
    /// * `_` - This parameter is ignored, as the `Messaging` module does not process
    ///   messages from a secondary messaging channel.
    ///
    /// # Returns
    /// A tuple `(bool, bool, bool)` indicating the status of commands received:
    /// - The first `bool` is `true` if a `Quit` command was received; otherwise `false`.
    /// - The second `bool` is always `false` (no "Start" commands processed).
    /// - The third `bool` is always `false` (no "Stop" commands processed).
    ///
    /// This code is specific for a Linux operating system because the messaging system
    /// is only implemented for that platform.
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: Option<&Receiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(rx_from_signal_handler, "Messaging".to_string())
    }
}

impl ProcessExternalRequestTrait for TcpCommunication {
    /// Checks for and processes new commands relevant to the TCP Communication module from external channels.
    ///
    /// This is the specialized implementation of `ProcessExternalRequestTrait` for `TcpCommunication`.
    /// It delegates directly to `process_external_request_without_messaging`, indicating
    /// that the `TcpCommunication` module primarily processes commands from the signal handler
    /// and does not use a separate messaging channel for external requests.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - A reference to the receiver end of the channel
    ///   for commands originating from the signal handler.
    /// * `_` - This parameter is ignored, as the `TcpCommunication` module does not process
    ///   messages from a messaging channel in this context.
    ///
    /// # Returns
    /// A tuple `(bool, bool, bool)` indicating the status of commands received:
    /// - The first `bool` is `true` if a `Quit` command was received; otherwise `false`.
    /// - The second `bool` is always `false` (no "Start" commands processed).
    /// - The third `bool` is always `false` (no "Stop" commands processed).
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _: Option<&Receiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        process_external_request_without_messaging(
            rx_from_signal_handler,
            "TcpCommunication".to_string(),
        )
    }
}

/// Checks for specific standard commands (Terminate or Quit) from the signal handler channel.
///
/// This associated function is a streamlined helper for implementations that
/// primarily need to react to global termination signals. It attempts
/// to receive without blocking `InternalCommand` messages and returns `true` if either
/// a `Terminate` or `Quit` command is found. Other commands are logged as warnings.
///
/// # Arguments
/// * `rx_from_signal_handler` - A reference to the receiver end of the channel
///   for commands originating from the signal handler.
///
/// # Returns
/// `true` if either a `Terminate` or `Quit` command was received; otherwise `false`.
fn process_standard_request(rx_from_signal_handler: &Receiver<InternalCommand>) -> bool {
    match rx_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Terminate => true,
            InternalCommand::Quit => true,
            _ => {
                warn!(
                    target: LOG_TARGET_PROC_EXT_REQ,
                    "ignoring inapplicable command {} from signal handler.",
                    c
                );
                false
            }
        },
        Err(e) => {
            match e {
                TryRecvError::Empty => { /* empty buffer - no action required */ }
                TryRecvError::Disconnected => {
                    #[cfg(not(test))]
                    error!(
                        target: LOG_TARGET_PROC_EXT_REQ,
                        "receiving command from signal handler failed ({e:?})"
                    );
                }
            }
            false
        }
    }
}

impl ProcessExternalRequestTrait for Feed {
    /// Checks for and processes new commands relevant to the Feed control module from external channels.
    ///
    /// This is the specialized implementation of `ProcessExternalRequestTrait` for the `Feed` module.
    /// It handles standard "Terminate" or "Quit" commands from the signal handler.
    /// From the messaging channel, it specifically processes "Start," "Stop," "ResetAllErrors,"
    /// and "Execute" commands, updating internal state variables based on these commands.
    /// Inapplicable commands are ignored with warnings.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - A reference to the receiver end of the channel
    ///   for commands originating from the signal handler.
    /// * `rx_from_messaging_opt` - An `Option` containing a reference to the receiver end
    ///   of the channel for commands from the messaging system. This channel is optional.
    ///
    /// # Returns
    /// A tuple `(bool, bool, bool)` indicating the status of specific commands received:
    /// - The first `bool` is `true` if a `Quit` or `Terminate` command was received from the signal handler; otherwise `false`.
    /// - The second `bool` is `true` if a `Start` command was received from messaging; otherwise `false`.
    /// - The third `bool` is `true` if a `Stop` command was received from messaging; otherwise `false`.
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        rx_from_messaging_opt: Option<&Receiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        let quit_command_received = process_standard_request(rx_from_signal_handler);

        self.execute_command_received = false;
        self.profile_id_requested = -1;
        let (start_command_received, stop_command_received, execute_command_received, profile_id) =
            match rx_from_messaging_opt {
                Some(rx_from_messaging) => {
                    match rx_from_messaging.try_recv() {
                        Ok(c) => match c {
                            InternalCommand::Start => (true, false, false, 0),
                            InternalCommand::Stop => (false, true, false, 0),
                            InternalCommand::ResetAllErrors => {
                                #[cfg(not(test))]
                                info!(
                                    target: LOG_TARGET_PROC_EXT_REQ,
                                    "Received reset command for errors of feed control."
                                );
                                self.execution_blocked = false;
                                (false, false, false, 0)
                            }
                            InternalCommand::Execute(c) => (false, false, true, c),
                            _ => {
                                warn!(
                                    target: LOG_TARGET_PROC_EXT_REQ,
                                    "ignoring inapplicable command {} from messaging.",
                                    c
                                );
                                (false, false, false, 0)
                            }
                        },
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    #[cfg(not(test))]
                                    error!(
                                        target: LOG_TARGET_PROC_EXT_REQ,
                                        "(feed) - receiving command from messaging failed ({e:?})"
                                    );
                                }
                            }
                            (false, false, false, 0)
                        }
                    }
                }
                None => (false, false, false, 0),
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

impl ProcessExternalRequestTrait for Balling {
    /// Checks for and processes new commands relevant to the Balling control module from external channels.
    ///
    /// This is the specialized implementation of `ProcessExternalRequestTrait` for the `Balling` module.
    /// It handles standard "Terminate" or "Quit" commands from the signal handler.
    /// From the messaging channel, it specifically processes "Start," "Stop," and "Execute" commands,
    /// updating internal state variables (`execute_command_received`, `pump_id_requested`) based on these commands.
    /// Inapplicable commands are ignored with warnings.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - A reference to the receiver end of the channel
    ///   for commands originating from the signal handler.
    /// * `rx_from_messaging_opt` - An `Option` containing a reference to the receiver end
    ///   of the channel for commands from the messaging system. This channel is optional.
    ///
    /// # Returns
    /// A tuple `(bool, bool, bool)` indicating the status of specific commands received:
    /// - The first `bool` is `true` if a `Quit` or `Terminate` command was received from the signal handler; otherwise `false`.
    /// - The second `bool` is `true` if a `Start` command was received from messaging; otherwise `false`.
    /// - The third `bool` is `true` if a `Stop` command was received from messaging; otherwise `false`.
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        rx_from_messaging_opt: Option<&Receiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        let quit_command_received = process_standard_request(rx_from_signal_handler);

        self.execute_command_received = false;
        self.pump_id_requested = -1;
        let (start_command_received, stop_command_received, execute_command_received, pump_id) =
            match rx_from_messaging_opt {
                Some(rx_from_messaging) => {
                    match rx_from_messaging.try_recv() {
                        Ok(c) => match c {
                            InternalCommand::Start => (true, false, false, 0),
                            InternalCommand::Stop => (false, true, false, 0),
                            InternalCommand::Execute(c) => (false, false, true, c),
                            _ => {
                                warn!(
                                    target: LOG_TARGET_PROC_EXT_REQ,
                                    "ignoring inapplicable command {} from messaging.",
                                    c
                                );
                                (false, false, false, 0)
                            }
                        },
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    #[cfg(not(test))]
                                    error!(
                                        target: LOG_TARGET_PROC_EXT_REQ,
                                        "(balling) receiving command from messaging failed ({e:?})"
                                    );
                                }
                            }
                            (false, false, false, 0)
                        }
                    }
                }
                None => (false, false, false, 0),
            };

        self.execute_command_received = execute_command_received;
        self.pump_id_requested = pump_id;

        (
            quit_command_received,
            start_command_received,
            stop_command_received,
        )
    }
}

impl ProcessExternalRequestTrait for Watchdog {
    /// Checks for and processes new commands relevant to the Watchdog module from external channels.
    ///
    /// This is the specialized implementation of `ProcessExternalRequestTrait` for the `Watchdog`.
    /// It handles standard "Terminate" or "Quit" commands from the signal handler.
    /// From the messaging channel, it specifically processes "Start" and "Stop" commands,
    /// ignoring other inapplicable commands with warnings.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - A reference to the receiver end of the channel
    ///   for commands originating from the signal handler.
    /// * `rx_from_messaging_opt` - An `Option` containing a reference to the receiver end
    ///   of the channel for commands from the messaging system. This channel is optional.
    ///
    /// # Returns
    /// A tuple `(bool, bool, bool)` indicating the status of specific commands received:
    /// - The first `bool` is `true` if a `Quit` or `Terminate` command was received from the signal handler; otherwise `false`.
    /// - The second `bool` is `true` if a `Start` command was received from messaging; otherwise `false`.
    /// - The third `bool` is `true` if a `Stop` command was received from messaging; otherwise `false`.
    fn process_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        rx_from_messaging_opt: Option<&Receiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        let quit_command_received = process_standard_request(rx_from_signal_handler);

        let (start_command_received, stop_command_received) = match rx_from_messaging_opt {
            Some(rx_from_messaging) => {
                match rx_from_messaging.try_recv() {
                    Ok(c) => match c {
                        InternalCommand::Start => (true, false),
                        InternalCommand::Stop => (false, true),
                        _ => {
                            warn!(
                                target: LOG_TARGET_PROC_EXT_REQ,
                                "ignoring inapplicable command {} from messaging.",
                                c
                            );
                            (false, false)
                        }
                    },
                    Err(e) => {
                        match e {
                            TryRecvError::Empty => { /* empty buffer - no action required */ }
                            TryRecvError::Disconnected => {
                                #[cfg(not(test))]
                                error!(
                                    target: LOG_TARGET_PROC_EXT_REQ,
                                    "(watchdog) receiving command from messaging failed ({e:?})"
                                );
                            }
                        }
                        (false, false)
                    }
                }
            }
            None => (false, false),
        };

        (
            quit_command_received,
            start_command_received,
            stop_command_received,
        )
    }
}
