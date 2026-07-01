/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines a standardized trait for processing external commands across various modules.
//!
//! This module provides the `ProcessExternalRequestTrait`, a common interface that
//! allows different concurrent threads (like `Heating`, `Refill`, `DataLogger`, etc.)
//! to handle commands from central sources like the `SignalHandler` (for shutdown)
//! and `Messaging` (for runtime control).
//!
//! ## Purpose and Design
//!
//! The primary goal is to abstract and unify the non-blocking check for external
//! commands, which is a repetitive pattern in each thread's main loop. By using a
//! trait, the application achieves:
//!
//! - **Standardization**: Enforces a consistent method signature for command processing.
//! - **Code Reusability**: A default implementation handles the most common commands
//!   (`Quit`, `Start`, `Stop`), reducing boilerplate in many modules.
//! - **Specialization**: Allows modules with unique command requirements (e.g., `Refill`,
//!   `Feed`) to provide their own specific implementations while still conforming to
//!   the trait.
//!
//! ## Key Components
//!
//! - **`ProcessExternalRequestTrait`**: The core trait.
//!   - **`process_external_request()`**: The main method. It performs a non-blocking
//!     `try_recv()` on the `SignalHandler` channel and an optional `Messaging` channel.
//!
//! - **Default Implementation**: The trait provides a default implementation of
//!   `process_external_request()` that is used by most control threads (e.g., `Heating`,
//!   `Ventilation`). It handles:
//!   - `InternalCommand::Quit` from the signal handler.
//!   - `InternalCommand::Start` and `InternalCommand::Stop` from the messaging system.
//!
//! - **Specialized Implementations**: Several modules provide their own `impl` block
//!   to handle commands specific to their function:
//!   - **`Refill`**: Overrides the default to also handle `InternalCommand::ResetAllErrors`.
//!   - **`Feed` / `Balling`**: Override the default to handle `InternalCommand::Execute(id)`.
//!   - **Simpler Modules** (`DataLogger`, `RelayManager`, etc.): Use a helper function
//!     (`process_external_request_without_messaging`) that only listens for the `Quit`
//!     command, as they do not need runtime start/stop control.
//!
//! This pattern of a default implementation with targeted overrides is a powerful way
//! to share common code while still allowing for necessary specialization.

use log::{error, warn};

#[cfg(not(test))] // only required in real-life execution
use log::info;

use crate::food::{feed::Feed, food_injection::FoodInjection};
use crate::recorder::data_logger::DataLogger;
use crate::relays::relay_manager::RelayManager;
use crate::utilities::channel_content::InternalCommand;

#[cfg(target_os = "linux")]
use crate::dispatch::messaging::Messaging;
use crate::launch::channels::{AquaChannelError, AquaReceiver};
use crate::mineral::{balling::Balling, mineral_injection::MineralInjection};
use crate::permission::schedule_check::ScheduleCheck;
use crate::sensors::dht::Dht;
use crate::sensors::i2c_interface::I2cInterface;
use crate::simulator::tcp_communication::TcpCommunication;
use crate::thermal::heating::Heating;
use crate::thermal::ventilation::Ventilation;
use crate::watchmen::watchdog::Watchdog;
use crate::water::{refill::Refill, water_injection::WaterInjection};

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
        rx_from_signal_handler: &mut AquaReceiver<InternalCommand>,
        rx_from_messaging_opt: Option<&mut AquaReceiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        let quit_command_received = match rx_from_signal_handler.try_recv() {
            Ok(c) => match c {
                InternalCommand::Quit => true,
                _ => {
                    #[cfg(any(not(test), not(feature = "debug_signal_handler")))] // only required in real-life execution
                    #[cfg(not(tarpaulin_include))]
                    warn!(
                        target: module_path!(),
                        "ProcessExternalRequestTrait: ignoring inapplicable command from signal handler."
                    );
                    false
                }
            },
            Err(e) => {
                match e {
                    #[cfg(feature = "debug_channels")]
                    AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                    AquaChannelError::Empty => { /* empty buffer - no action required */ }
                    AquaChannelError::Disconnected => {
                        #[cfg(all(not(test), not(feature = "debug_signal_handler")))] // only required in real-life execution
                        #[cfg(not(tarpaulin_include))]
                        error!(
                            target: module_path!(),
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
                            #[cfg(not(test))] // only required in real-life execution
                            #[cfg(not(tarpaulin_include))]
                            warn!(
                                target: module_path!(),
                                "ProcessExternalRequestTrait: ignoring inapplicable command from messaging."
                            );
                            (false, false)
                        }
                    },
                    Err(e) => {
                        match e {
                            #[cfg(feature = "debug_channels")]
                            AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                            AquaChannelError::Empty => { /* empty buffer - no action required */ }
                            AquaChannelError::Disconnected => {
                                #[cfg(not(test))] // only required in real-life execution
                                #[cfg(not(tarpaulin_include))]
                                error!(
                                    target: module_path!(),
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
        rx_from_signal_handler: &mut AquaReceiver<InternalCommand>,
        rx_from_messaging_opt: Option<&mut AquaReceiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        let quit_command_received = match rx_from_signal_handler.try_recv() {
            Ok(c) => match c {
                InternalCommand::Quit => true,
                _ => {
                    #[cfg(any(not(test), feature = "debug_signal_handler"))] // only required in real-life execution
                    #[cfg(not(tarpaulin_include))]
                    warn!(
                        target: module_path!(),
                        "ignoring inapplicable command from signal handler."
                    );
                    false
                }
            },
            Err(e) => {
                match e {
                    #[cfg(feature = "debug_channels")]
                    AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                    AquaChannelError::Empty => { /* empty buffer - no action required */ }
                    AquaChannelError::Disconnected => {
                        #[cfg(not(test))] // only required in real-life execution
                        #[cfg(not(tarpaulin_include))]
                        error!(
                            target: module_path!(),
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
                            #[cfg(not(test))] // only required in real-life execution
                            #[cfg(not(tarpaulin_include))]
                            info!(
                                target: module_path!(),
                                "Received reset command for errors of refill control."
                            );
                            self.refill_errors.reset_all_errors();
                            (false, false)
                        }
                        _ => {
                            #[cfg(not(test))] // only required in real-life execution
                            #[cfg(not(tarpaulin_include))]
                            warn!(
                                target: module_path!(),
                                "ignoring inapplicable command from messaging."
                            );
                            (false, false)
                        }
                    },
                    Err(e) => {
                        match e {
                            #[cfg(feature = "debug_channels")]
                            AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                            AquaChannelError::Empty => { /* empty buffer - no action required */ }
                            AquaChannelError::Disconnected => {
                                #[cfg(not(test))] // only required in real-life execution
                                #[cfg(not(tarpaulin_include))]
                                error!(
                                    target: module_path!(),
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
/// * `sender` - Sender channel for IPC commands.
fn process_external_request_without_messaging(
    rx_from_signal_handler: &mut AquaReceiver<InternalCommand>,
    sender: String,
) -> (bool, bool, bool) {
    let quit_command_received = match rx_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Quit => true,
            _ => {
                #[cfg(any(not(test), feature = "debug_signal_handler"))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                warn!(
                    target: module_path!(),
                    "ignoring inapplicable command from signal handler."
                );
                false
            }
        },
        Err(e) => {
            match e {
                #[cfg(feature = "debug_channels")]
                AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                AquaChannelError::Empty => { /* empty buffer - no action required */ }
                AquaChannelError::Disconnected => {
                    error!(
                        target: module_path!(),
                        "({sender}) receiving command from signal handler failed ({e:?})"
                    );
                }
            }
            false
        }
    };

    (quit_command_received, false, false)
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
fn process_standard_request(rx_from_signal_handler: &mut AquaReceiver<InternalCommand>) -> bool {
    match rx_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Terminate => true,
            InternalCommand::Quit => true,
            _ => {
                #[cfg(any(not(test), feature = "debug_signal_handler"))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                warn!(
                    target: module_path!(),
                    "ignoring inapplicable command {c} from signal handler."
                );
                false
            }
        },
        Err(e) => {
            match e {
                #[cfg(feature = "debug_channels")]
                AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                AquaChannelError::Empty => { /* empty buffer - no action required */ }
                AquaChannelError::Disconnected => {
                    #[cfg(not(test))] // only required in real-life execution
                    #[cfg(not(tarpaulin_include))]
                    error!(
                        target: module_path!(),
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
        rx_from_signal_handler: &mut AquaReceiver<InternalCommand>,
        rx_from_messaging_opt: Option<&mut AquaReceiver<InternalCommand>>,
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
                                #[cfg(not(test))] // only required in real-life execution
                                #[cfg(not(tarpaulin_include))]
                                info!(
                                    target: module_path!(),
                                    "Received reset command for errors of feed control."
                                );
                                self.execution_blocked = false;
                                (false, false, false, 0)
                            }
                            InternalCommand::Execute(c) => (false, false, true, c),
                            _ => {
                                #[cfg(not(test))] // only required in real-life execution
                                #[cfg(not(tarpaulin_include))]
                                warn!(
                                    target: module_path!(),
                                    "ignoring inapplicable command {c} from messaging."
                                );
                                (false, false, false, 0)
                            }
                        },
                        Err(e) => {
                            match e {
                                #[cfg(feature = "debug_channels")]
                                AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                                AquaChannelError::Empty => { /* empty buffer - no action required */
                                }
                                AquaChannelError::Disconnected => {
                                    #[cfg(not(test))] // only required in real-life execution
                                    #[cfg(not(tarpaulin_include))]
                                    error!(
                                        target: module_path!(),
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
        rx_from_signal_handler: &mut AquaReceiver<InternalCommand>,
        rx_from_messaging_opt: Option<&mut AquaReceiver<InternalCommand>>,
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
                                #[cfg(not(test))] // only required in real-life execution
                                #[cfg(not(tarpaulin_include))]
                                warn!(
                                    target: module_path!(),
                                    "ignoring inapplicable command {c} from messaging."
                                );
                                (false, false, false, 0)
                            }
                        },
                        Err(e) => {
                            match e {
                                #[cfg(feature = "debug_channels")]
                                AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                                AquaChannelError::Empty => { /* empty buffer - no action required */
                                }
                                AquaChannelError::Disconnected => {
                                    #[cfg(not(test))] // only required in real-life execution
                                    #[cfg(not(tarpaulin_include))]
                                    error!(
                                        target: module_path!(),
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
        rx_from_signal_handler: &mut AquaReceiver<InternalCommand>,
        rx_from_messaging_opt: Option<&mut AquaReceiver<InternalCommand>>,
    ) -> (bool, bool, bool) {
        let quit_command_received = process_standard_request(rx_from_signal_handler);

        let (start_command_received, stop_command_received) = match rx_from_messaging_opt {
            Some(rx_from_messaging) => {
                match rx_from_messaging.try_recv() {
                    Ok(c) => match c {
                        InternalCommand::Start => (true, false),
                        InternalCommand::Stop => (false, true),
                        _ => {
                            #[cfg(not(test))] // only required in real-life execution
                            #[cfg(not(tarpaulin_include))]
                            warn!(
                                target: module_path!(),
                                "ignoring inapplicable command {c} from messaging."
                            );
                            (false, false)
                        }
                    },
                    Err(e) => {
                        match e {
                            #[cfg(feature = "debug_channels")]
                            AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                            AquaChannelError::Empty => { /* empty buffer - no action required */ }
                            AquaChannelError::Disconnected => {
                                #[cfg(not(test))] // only required in real-life execution
                                #[cfg(not(tarpaulin_include))]
                                error!(
                                    target: module_path!(),
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

/// A macro to generate the `ProcessExternalRequestTrait` implementation for structs
/// that only need to listen for commands from the signal handler (i.e., no messaging).
macro_rules! impl_process_request_no_messaging {
    // The `$(...)+` syntax allows the macro to accept one or more struct types,
    // separated by commas.
    ($($struct_name:ty),+) => {
        $(
            impl ProcessExternalRequestTrait for $struct_name {
/// Performs the `process_external_request` operation.
///
/// # Arguments
/// * `rx_from_signal_handler` - Receiver channel for signals from the signal handler.
                fn process_external_request(
                    &mut self,
                    rx_from_signal_handler: &mut AquaReceiver<InternalCommand>,
                    _: Option<&mut AquaReceiver<InternalCommand>>,
                ) -> (bool, bool, bool) {
                    // Use stringify! to get the name of the struct for logging.
                    process_external_request_without_messaging(
                        rx_from_signal_handler,
                        stringify!($struct_name).to_string(),
                    )
                }
            }
        )+
    };
}

/// A macro to generate the `ProcessExternalRequestTrait` implementation for structs
/// that need to listen for commands from the signal handler and messaging, including
/// the `ResetAllErrors` command which resets `actuator_disabled`.
/// This macro is used for the trait implementation of Heating and Ventilation.
///
/// # Arguments
/// * `rx_from_signal_handler` - Receiver channel for signals from the signal handler.
/// * `rx_from_messaging_opt` - Optional receiver channel for messages from messaging.
macro_rules! impl_process_request_with_reset_errors {
    ($($struct_name:ty),+) => {
        $(
            impl ProcessExternalRequestTrait for $struct_name {
                fn process_external_request(
                    &mut self,
                    rx_from_signal_handler: &mut AquaReceiver<InternalCommand>,
                    rx_from_messaging_opt: Option<&mut AquaReceiver<InternalCommand>>,
                ) -> (bool, bool, bool) {
                    let quit_command_received = match rx_from_signal_handler.try_recv() {
                        Ok(c) => match c {
                            InternalCommand::Quit => true,
                            _ => {
                                #[cfg(any(not(test), feature = "debug_signal_handler"))] // only required in real-life execution
                                #[cfg(not(tarpaulin_include))]
                                warn!(
                                    target: module_path!(),
                                    "ignoring inapplicable command from signal handler."
                                );
                                false
                            }
                        },
                        Err(e) => {
                            match e {
                                #[cfg(feature = "debug_channels")]
                                AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                                AquaChannelError::Empty => { /* empty buffer - no action required */ }
                                AquaChannelError::Disconnected => {
                                    #[cfg(not(test))] // only required in real-life execution
                                    #[cfg(not(tarpaulin_include))]
                                    error!(
                                        target: module_path!(),
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
                                        #[cfg(not(test))] // only required in real-life execution
                                        #[cfg(not(tarpaulin_include))]
                                        info!(
                                            target: module_path!(),
                                            "Received reset command for errors of {} control.",
                                            stringify!($struct_name).to_lowercase()
                                        );
                                        self.actuator_disabled = false;
                                        (false, false)
                                    }
                                    _ => {
                                        #[cfg(not(test))] // only required in real-life execution
                                        #[cfg(not(tarpaulin_include))]
                                        warn!(
                                            target: module_path!(),
                                            "ignoring inapplicable command from messaging."
                                        );
                                        (false, false)
                                    }
                                },
                                Err(e) => {
                                    match e {
                                        #[cfg(feature = "debug_channels")]
                                        AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                                        AquaChannelError::Empty => { /* empty buffer - no action required */ }
                                        AquaChannelError::Disconnected => {
                                            #[cfg(not(test))] // only required in real-life execution
                                            #[cfg(not(tarpaulin_include))]
                                            error!(
                                                target: module_path!(),
                                                "({}) receiving command from messaging failed ({e:?})",
                                                stringify!($struct_name).to_lowercase()
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
        )+
    };
}

impl_process_request_no_messaging!(
    DataLogger,
    Dht,
    FoodInjection,
    MineralInjection,
    WaterInjection,
    I2cInterface,
    ScheduleCheck,
    RelayManager,
    TcpCommunication
);

impl_process_request_with_reset_errors!(Heating, Ventilation);

// The impl for Messaging is conditional, so it gets its own call.
#[cfg(target_os = "linux")]
impl_process_request_no_messaging!(Messaging);
