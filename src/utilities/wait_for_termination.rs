/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::dispatch::messaging::Messaging;
use crate::food::feed::Feed;
use crate::launch::channels::{AquaChannelError, AquaReceiver};
use crate::mineral::balling::Balling;
use crate::recorder::data_logger::DataLogger;
use crate::sensors::atlas_scientific::AtlasScientific;
use crate::sensors::sensor_manager::SensorManager;
use crate::sensors::tank_level_switch::TankLevelSwitch;
use crate::thermal::heating::Heating;
use crate::thermal::ventilation::Ventilation;
use crate::utilities::channel_content::InternalCommand;
use crate::water::refill::Refill;
use log::{error, warn};
use spin_sleep::SpinSleeper;
use std::time::Duration;

/// Defines a standard behavior for components that need to wait for a termination signal.
///
/// This trait is implemented by long-running components (often those running in their own thread)
/// that need to perform a graceful shutdown. After completing their primary tasks, they can call
/// `wait_for_termination` to pause execution efficiently until the central signal handler
/// sends a final `InternalCommand::Terminate` command.
pub trait WaitForTerminationTrait {
    /// Provides a mutable reference to the component's warning lock flag.
    /// This flag is used to prevent log-flooding with inapplicable command warnings.
    fn get_warn_lock_mut(&mut self) -> &mut bool;

    /// Provides a mutable reference to the component's error lock flag.
    /// This flag is used to prevent log-flooding with the channel receive errors.
    fn get_error_lock_mut(&mut self) -> &mut bool;

    /// Implements the graceful shutdown wait loop for several threads (default implementation).
    ///
    /// This function enters a loop that continuously checks for a termination command from the
    /// signal handler. It uses `try_recv` for non-blocking message checking.
    ///
    /// - If `InternalCommand::Terminate` is received, the loop exits.
    /// - If any other `InternalCommand` is received, a warning is logged. A lock flag
    ///   (`lock_warn_inapplicable_command_signal_handler`) prevents spamming the log with
    ///   repeated warnings.
    /// - If the channel receive operation fails (e.g., the sender is dropped), an error is
    ///   logged. A lock flag (`lock_error_channel_receive_termination`) prevents repeated
    ///   error logs.
    ///
    /// The thread sleeps for the specified `sleep_duration` between each check
    /// to minimize CPU consumption.
    ///
    /// # Arguments
    /// * `rx_waiting_thread_from_signal_handler` - The channel receiver to listen for commands.
    /// * `sleep_duration` - The duration to pause between checks.
    /// * `origin` - module name which called this function.
    fn wait_for_termination(
        &mut self,
        rx_waiting_thread_from_signal_handler: &mut AquaReceiver<InternalCommand>,
        sleep_duration: Duration,
        origin: &str,
    ) {
        let spin_sleeper = SpinSleeper::default();
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match rx_waiting_thread_from_signal_handler.try_recv() {
                Ok(c) => match c {
                    InternalCommand::Terminate => {
                        *self.get_warn_lock_mut() = false;
                        true
                    }
                    _ => {
                        // [impl->swreq~water_log_flood_protection~1]
                        // [impl->swreq~food_log_flood_protection~1]
                        // [impl->swreq~dispatch_log_flood_protection~1]
                        // [impl->swreq~mineral_log_flood_protection~1]
                        // [impl->swreq~recorder_log_flood_protection~1]
                        // [impl->swreq~relays_log_flood_protection~1]
                        // [impl->swreq~schedule_log_flood_protection~1]
                        // [impl->swreq~sensors_log_flood_protection~1]
                        // [impl->swreq~thermal_log_flood_protection~1]
                        // [impl->swreq~watchmen_log_flood_protection~1]
                        if !*self.get_warn_lock_mut() {
                            warn!(
                                target: origin,
                                "ignoring inapplicable command from signal handler ({c})"
                            );
                            *self.get_warn_lock_mut() = true;
                        }
                        false
                    }
                },
                Err(e) => {
                    match e {
                        #[cfg(feature = "debug_channels")]
                        AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                        AquaChannelError::Empty => { /* do nothing */ }
                        AquaChannelError::Disconnected => {
                            // [impl->swreq~water_log_flood_protection~1]
                            // [impl->swreq~food_log_flood_protection~1]
                            // [impl->swreq~dispatch_log_flood_protection~1]
                            // [impl->swreq~mineral_log_flood_protection~1]
                            // [impl->swreq~recorder_log_flood_protection~1]
                            // [impl->swreq~relays_log_flood_protection~1]
                            // [impl->swreq~schedule_log_flood_protection~1]
                            // [impl->swreq~sensors_log_flood_protection~1]
                            // [impl->swreq~thermal_log_flood_protection~1]
                            // [impl->swreq~watchmen_log_flood_protection~1]
                            if !*self.get_error_lock_mut() {
                                error!(
                                    target: origin,
                                    "receiving termination signal from signal handler failed ({e:?})"
                                );
                                *self.get_error_lock_mut() = true;
                            }
                        }
                    }
                    false
                }
            };
            spin_sleeper.sleep(sleep_duration);
        }
    }

    /// Implements the graceful shutdown wait loop for tokio tasks (default implementation).
    ///
    /// This function enters a loop that continuously checks for a termination command from the
    /// signal handler. It uses `try_recv` for non-blocking message checking.
    ///
    /// - If `InternalCommand::Terminate` is received, the loop exits.
    /// - If any other `InternalCommand` is received, a warning is logged. A lock flag
    ///   (`lock_warn_inapplicable_command_signal_handler`) prevents spamming the log with
    ///   repeated warnings.
    /// - If the channel receive operation fails (e.g., the sender is dropped), an error is
    ///   logged. A lock flag (`lock_error_channel_receive_termination`) prevents repeated
    ///   error logs.
    ///
    /// The tokio task yields/sleeps asynchronously for the specified `sleep_duration` between
    /// each check to avoid blocking the executor thread.
    ///
    /// # Arguments
    /// * `rx_waiting_thread_from_signal_handler` - The channel receiver to listen for commands.
    /// * `sleep_duration` - The duration to pause between checks.
    /// * `origin` - module name which called this function.
    #[allow(async_fn_in_trait)]
    async fn wait_for_termination_async(
        &mut self,
        rx_waiting_thread_from_signal_handler: &mut AquaReceiver<InternalCommand>,
        sleep_duration: Duration,
        origin: &str,
    ) {
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match rx_waiting_thread_from_signal_handler.try_recv() {
                Ok(c) => match c {
                    InternalCommand::Terminate => {
                        *self.get_warn_lock_mut() = false;
                        true
                    }
                    _ => {
                        // [impl->swreq~water_log_flood_protection~1]
                        // [impl->swreq~food_log_flood_protection~1]
                        // [impl->swreq~dispatch_log_flood_protection~1]
                        // [impl->swreq~mineral_log_flood_protection~1]
                        // [impl->swreq~recorder_log_flood_protection~1]
                        // [impl->swreq~relays_log_flood_protection~1]
                        // [impl->swreq~schedule_log_flood_protection~1]
                        // [impl->swreq~sensors_log_flood_protection~1]
                        // [impl->swreq~thermal_log_flood_protection~1]
                        // [impl->swreq~watchmen_log_flood_protection~1]
                        if !*self.get_warn_lock_mut() {
                            warn!(
                                target: origin,
                                "ignoring inapplicable command from signal handler ({c})"
                            );
                            *self.get_warn_lock_mut() = true;
                        }
                        false
                    }
                },
                Err(e) => {
                    match e {
                        #[cfg(feature = "debug_channels")]
                        AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                        AquaChannelError::Empty => { /* do nothing */ }
                        AquaChannelError::Disconnected => {
                            // [impl->swreq~water_log_flood_protection~1]
                            // [impl->swreq~food_log_flood_protection~1]
                            // [impl->swreq~dispatch_log_flood_protection~1]
                            // [impl->swreq~mineral_log_flood_protection~1]
                            // [impl->swreq~recorder_log_flood_protection~1]
                            // [impl->swreq~relays_log_flood_protection~1]
                            // [impl->swreq~schedule_log_flood_protection~1]
                            // [impl->swreq~sensors_log_flood_protection~1]
                            // [impl->swreq~thermal_log_flood_protection~1]
                            // [impl->swreq~watchmen_log_flood_protection~1]
                            if !*self.get_error_lock_mut() {
                                error!(
                                    target: origin,
                                    "receiving termination signal from signal handler failed ({e:?})"
                                );
                                *self.get_error_lock_mut() = true;
                            }
                        }
                    }
                    false
                }
            };
            if !termination_command_received {
                tokio::time::sleep(sleep_duration).await;
            }
        }
    }
}

/// A macro to generate the `WaitForTerminationTrait` implementation for structs
/// that follow the standard pattern of having `lock_warn...` and `lock_error...` fields.
macro_rules! impl_wait_for_termination {
    // The `$(...)+` syntax allows the macro to accept one or more struct types,
    // separated by commas.
    ($($struct_name:ty),+) => {
        $(
            impl WaitForTerminationTrait for $struct_name {
                /// Method connects the default trait implementation with the specific implementation
                /// accessing the warn-lock.
                fn get_warn_lock_mut(&mut self) -> &mut bool {
                    &mut self.lock_warn_inapplicable_command_signal_handler
                }

                /// Method connects the default trait implementation with the specific implementation
                /// for accessing the error-lock.
                fn get_error_lock_mut(&mut self) -> &mut bool {
                    &mut self.lock_error_channel_receive_termination
                }
            }
        )+
    };
}

impl_wait_for_termination!(
    TankLevelSwitch,
    Ventilation,
    Refill,
    Heating,
    Feed,
    Balling,
    SensorManager,
    AtlasScientific,
    DataLogger,
    Messaging
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::channels::channel;
    use crate::launch::execution_config::ExecutionConfig;
    use std::time::Duration;

    // [utest->swreq~water_log_flood_protection~1]
    // [utest->swreq~food_log_flood_protection~1]
    // [utest->swreq~dispatch_log_flood_protection~1]
    // [utest->swreq~mineral_log_flood_protection~1]
    // [utest->swreq~recorder_log_flood_protection~1]
    // [utest->swreq~relays_log_flood_protection~1]
    // [utest->swreq~schedule_log_flood_protection~1]
    // [utest->swreq~sensors_log_flood_protection~1]
    // [utest->swreq~thermal_log_flood_protection~1]
    // [utest->swreq~watchmen_log_flood_protection~1]
    #[tokio::test]
    async fn test_messaging_wait_for_termination_inhibition_flags() {
        #[cfg(not(target_os = "linux"))]
        let mut messaging = Messaging::new(MessagingConfig {}, ExecutionConfig::default())
            .expect("Initialization of Messaging failed");

        #[cfg(target_os = "linux")]
        let mut messaging = Messaging {
            mq: None,
            mq_max_msg_len: 0,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
            execution_config: ExecutionConfig::default(),
        };

        // Verify initial state of inhibition flags on Messaging struct
        assert!(!*messaging.get_warn_lock_mut());
        assert!(!*messaging.get_error_lock_mut());

        // 1. Verify warning lock flag usage on receiving inapplicable command
        let (mut tx, mut rx) = channel::<InternalCommand>(10, false);
        tx.send(InternalCommand::Quit).unwrap();
        tx.send(InternalCommand::Terminate).unwrap();

        messaging
            .wait_for_termination_async(&mut rx, Duration::from_millis(1), "test_messaging")
            .await;

        // After Terminate signal, warn lock flag is reset to false
        assert!(!messaging.lock_warn_inapplicable_command_signal_handler);

        // 2. Verify error lock flag state manipulation via get_error_lock_mut
        assert!(!messaging.lock_error_channel_receive_termination);
        *messaging.get_error_lock_mut() = true;
        assert!(messaging.lock_error_channel_receive_termination);
        *messaging.get_error_lock_mut() = false;
        assert!(!messaging.lock_error_channel_receive_termination);
    }
}
