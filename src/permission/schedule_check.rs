/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Implements the central, schedule-based permission controller for the application.
//!
//! This module provides the `ScheduleCheck` struct, which runs as a dedicated, long-running
//! thread. Its primary responsibility is to act as a centralized "permission oracle" for
//! modules `Ventilation`, `Heating`, `Refill`, and `Balling`. It determines whether these
//! modules are allowed to perform their actions (e.g., turn on a pump or heater) based
//! on time-based schedules stored in the database.
//!
//! ## Key Components
//!
//! - **`ScheduleCheck` Struct**: The core state machine for the thread. It holds the
//!   configuration and a set of `ScheduleCheckLocks` for each client module. These locks are
//!   a crucial part of its design, used to prevent the same recurring error or warning
//!   from flooding the log files.
//!
//! - **`execute()` Method**: The main thread loop. It operates in two modes simultaneously:
//!   1.  **Proactive Caching**: It periodically re-reads all schedules from the
//!       database, caching the results in memory. This reduces database load, as it
//!       avoids querying on every single request.
//!   2.  **Reactive Service**: In each loop iteration, it polls the channels from each
//!       client module (`Heating`, `Refill`, etc.), checking for incoming requests.
//!
//! - **`check_pending_schedule_request()` Method**: The workhorse function that handles a
//!   single request. It performs a non-blocking `try_recv` on a client's channel. If a
//!   `ScheduleCheck` command is found, it consults the cached schedule. Based on the
//!   schedule entry (or a configured fallback strategy if none is found), it sends a
//!   simple `true` (allowed) or `false` (disallowed) reply to the client.
//!
//! ## Design and Architecture
//!
//! This module is a cornerstone of the application's concurrent design, promoting
//! decoupling and robustness.
//!
//! - **Centralized Permission Logic**: By centralizing all schedule-based checks, this
//!   module simplifies the logic in all other hardware-controlling modules. A module like
//!   `Heating` doesn't need to know anything about database queries or time calculations;
//!   it only needs to ask `ScheduleCheck` for permission.
//!
//! - **Robust Error Handling (Log Flood Prevention)**: The extensive use of `ScheduleCheckLocks`
//!   is a deliberate design choice for a resilient, long-running service. If a recurring
//!   problem occurs (e.g., a missing schedule in the database), the error is logged
//!   *once*. The lock is then engaged to suppress identical log messages until the
//!   underlying condition is resolved. This prevents a single, persistent issue from
//!   filling up disk space with repetitive logs.
//!
//! - **Decoupled Request/Reply Pattern**: The communication is based on a simple and
//!   effective request/reply pattern. A client sends a request and waits for a boolean
//!   response. This decouples the client modules from the implementation details of the
//!   permission logic.

#[cfg(feature = "debug_schedule_check")]
use log::debug;

#[cfg(all(not(test), target_os = "linux"))] // only required in real-life execution
use log::info;

#[cfg(all(target_os = "linux", not(test)))] // only required in real-life execution
use nix::unistd::gettid;

#[cfg(not(test))] // only required in real-life execution
use log::warn;

use crate::database::pingable::Pingable;
use crate::database::sql_interface_schedule::{ScheduleEntry, ScheduleType, SqlInterfaceSchedule};
use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::launch::execution_config::ExecutionConfig;
use crate::permission::schedule_check_channels::ScheduleCheckChannels;
use crate::permission::schedule_check_config::ScheduleCheckConfig;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::channel_content::InternalCommand;
use crate::utilities::database_ping_trait::DatabasePingTrait;
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use std::time::{Duration, Instant};

/// Contains all inhibition flags for a specific schedule communication
#[derive(Default)]
pub struct ScheduleCheckLocks {
    /// inhibition flag to avoid flooding the log file with repeated messages about not finding schedule entry
    find_schedule: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive information via the channel
    channel_receive: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send information via the channel
    channel_send: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command
    inapplicable_command: bool,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the schedule checker.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     schedule_check[Schedule check] --> ventilation[Ventilation control]
///     ventilation --> schedule_check
///     schedule_check --> heating[Heating control]
///     heating --> schedule_check
///     schedule_check --> refill[Refill control]
///     refill --> schedule_check
///     schedule_check --> balling[Balling dosing control]
///     balling --> schedule_check
///     schedule_check --> signal_handler[Signal handler]
///     signal_handler --> schedule_check
/// ```
pub struct ScheduleCheck {
    /// configuration data for schedule checker
    config: ScheduleCheckConfig,

    /// inhibition flags for Balling
    locks_balling: ScheduleCheckLocks,

    /// inhibition flags for ventilation
    locks_ventilation: ScheduleCheckLocks,

    /// inhibition flags for heating
    locks_heating: ScheduleCheckLocks,

    /// inhibition flags for refill
    locks_refill: ScheduleCheckLocks,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read from the database
    lock_error_database_read: bool,

    /// recording when the last database ping happened
    pub last_ping_instant: Instant,

    /// database ping interval
    pub database_ping_interval: Duration,
}

impl ScheduleCheck {
    /// Creates a new `ScheduleCheck` instance.
    ///
    /// This constructor initializes the schedule checking module with its specific
    /// configuration and a connection to the SQL database interface responsible
    /// for schedule data. It also sets up all internal lock flags to `false`,
    /// which are used to prevent log flooding during operation.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the schedule checker, dictating
    ///   behavior like check intervals and strategies for missing schedules.
    /// * `database_ping_interval` - A `Duration` instance, providing the interval to ping the database.
    ///
    /// # Returns
    /// A new `ScheduleCheck` struct, ready to monitor and evaluate schedules.
    pub fn new(config: ScheduleCheckConfig, database_ping_interval: Duration) -> ScheduleCheck {
        ScheduleCheck {
            config,
            locks_balling: Default::default(),
            locks_ventilation: Default::default(),
            locks_heating: Default::default(),
            locks_refill: Default::default(),
            lock_error_database_read: false,
            last_ping_instant: Instant::now(),
            database_ping_interval,
        }
    }

    /// Checks for and processes a pending `ScheduleCheck` request from another module.
    ///
    /// This private helper function is called repeatedly for each control module
    /// (Balling, Refill, Ventilation, Heating). It attempts to receive
    /// a `ScheduleCheck` command without blocking. If received, it evaluates whether actuation is
    /// currently allowed based on the provided `schedule_entry` and the configured
    /// `strategy_if_schedule_not_found`. The result (a boolean) is then returned
    /// to the calling module.
    ///
    /// The function uses several "lock" flags to prevent repetitive logging of the same error/warning.
    ///
    /// # Arguments
    /// * `config` - A reference to the `ScheduleCheckConfig` for general strategy settings.
    /// * `tx` - The sender channel to send the `bool` result (actuation allowed/disallowed) back to the requesting module.
    /// * `rx` - The receiver channel to receive `InternalCommand` (specifically `ScheduleCheck`) from the requesting module.
    /// * `schedule_entry` - An `Option` containing the `ScheduleEntry` relevant to the requesting module,
    ///   or `None` if no schedule was found for that module.
    /// * `lock_error_channel_receive` - A mutable reference to a boolean flag that, when `true`,
    ///   inhibits repeated error logging for channel receive failures.
    /// * `locks` - A mutable reference to the struct containing the lock flags.
    /// * `_module_name` - Name of the module for which the schedule shall be checked for logging purposes.
    ///
    fn check_pending_schedule_request(
        config: &ScheduleCheckConfig,
        tx: &mut AquaSender<bool>,
        rx: &mut AquaReceiver<InternalCommand>,
        schedule_entry: Option<&ScheduleEntry>,
        locks: &mut ScheduleCheckLocks,
        _module_name: &str,
    ) {
        // receive request
        let request = match rx.try_recv() {
            Ok(c) => {
                locks.channel_receive = false;
                c
            }
            Err(e) => {
                match e {
                    #[cfg(feature = "debug_channels")]
                    AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                    AquaChannelError::Empty => {
                        /* empty buffer - no action required */
                        locks.channel_receive = false;
                    }
                    AquaChannelError::Disconnected => {
                        if !locks.channel_receive {
                            #[cfg(not(test))] // only required in real-life execution
                            let context = format!("receiving command from {_module_name} failed.");
                            #[cfg(not(test))] // only required in real-life execution
                            log_error_chain(module_path!(), &context, e);
                            locks.channel_receive = true;
                        }
                    }
                }
                return;
            }
        };

        // check if schedule information is present
        let schedule_status = match request {
            InternalCommand::ScheduleCheck => {
                locks.inapplicable_command = false;
                match schedule_entry {
                    Some(c) => c.check_if_allowed(),
                    None => config.strategy_if_schedule_not_found,
                }
            }
            _c => {
                if !locks.inapplicable_command {
                    #[cfg(not(test))] // only required in real-life execution
                    #[cfg(not(tarpaulin_include))]
                    warn!(
                        target: module_path!(),
                        "ignoring inapplicable command ({_c}) from {_module_name}"
                    );
                    locks.inapplicable_command = true;
                }
                return;
            }
        };

        // send back status to requester
        match tx.send(schedule_status) {
            Ok(_) => {
                locks.channel_send = false;
            }
            Err(_e) => {
                if !locks.channel_send {
                    #[cfg(not(test))] // only required in real-life execution
                    #[cfg(not(tarpaulin_include))]
                    {
                        let context = format!("sending back message to {_module_name} failed");
                        log_error_chain(module_path!(), &context, _e);
                    }
                    locks.channel_send = true;
                }
            }
        };
    }

    /// Finds the relevant schedule for a module and processes any pending requests.
    ///
    /// This is a helper function designed to reduce repetitive logic in the main `execute`
    /// loop. It encapsulates the full sequence of actions for a single client module
    /// (e.g., "ventilation", "heating").
    ///
    /// The function first checks if the target module's thread is running. If so, it
    /// attempts to find the corresponding schedule entry from the in-memory cache
    /// (if `config.active` is true). It handles any errors during this lookup, using
    /// the provided `locks` to prevent log flooding on repeated failures.
    ///
    /// Finally, it delegates the task of channel communication (receiving a request
    /// and sending a reply) to the `check_pending_schedule_request` function.
    ///
    /// # Arguments
    /// * `config` - A reference to the main `ScheduleCheckConfig`.
    /// * `thread_is_executed` - A boolean indicating if the target client module's thread is active.
    /// * `sql_interface_schedule` - The database interface used to find the schedule entry.
    /// * `locks` - A mutable reference to the log-suppression flags for the specific client module.
    /// * `action` - The name of the client module (e.g., "ventilation"), used for logging.
    /// * `schedule_check_channels` - A mutable reference to the struct containing all communication channels.
    fn find_schedule_check_request(
        schedule_type: ScheduleType,
        config: &ScheduleCheckConfig,
        thread_is_executed: bool,
        sql_interface_schedule: &SqlInterfaceSchedule,
        locks: &mut ScheduleCheckLocks,
        module_name: &str,
        channel_pair: (&mut AquaSender<bool>, &mut AquaReceiver<InternalCommand>),
    ) {
        let (tx_schedule_check_to_requester, rx_schedule_check_from_requester) = channel_pair;
        if thread_is_executed {
            let schedule_entry_opt: Option<&ScheduleEntry> = if config.active {
                match sql_interface_schedule.find_schedule(schedule_type) {
                    Ok(c) => {
                        locks.find_schedule = false;
                        Some(c)
                    }
                    Err(e) => {
                        if !locks.find_schedule {
                            let context = format!("could not find schedule for {module_name}");
                            log_error_chain(module_path!(), &context, e);
                            locks.find_schedule = true;
                        }
                        None
                    }
                }
            } else {
                None
            };
            Self::check_pending_schedule_request(
                config,
                tx_schedule_check_to_requester,
                rx_schedule_check_from_requester,
                schedule_entry_opt,
                locks,
                module_name,
            );
        }
    }

    /// Executes the main control loop for the schedule checker.
    ///
    /// This function runs continuously, managing the validation of various device schedules
    /// against the current time. It periodically reads the latest schedule entries from
    /// the SQL database and, for each relevant module (Ventilation, Heating, Refill, Balling),
    /// it checks if that module is currently allowed to actuate its devices based on its schedule.
    /// The result of this check is communicated back to the respective module via channels.
    ///
    /// The loop continues until a `Quit` command is received from the signal handler.
    /// Upon receiving `Quit`, it sends a confirmation back to the signal handler and
    /// then waits for a `Terminate` command to complete its shutdown.
    ///
    /// # Arguments
    /// * `schedule_check_channels` - A `ScheduleCheckChannels` struct containing all
    ///   necessary `mpsc` sender and receiver channels for communication with
    ///   other application modules and the signal handler.
    /// * `execution_config` - An `ExecutionConfig` struct containing information about which threads
    ///   have been started.
    /// * `sql_interface_schedule` - A `SqlInterfaceSchedule` instance providing
    ///   the interface to the SQL database for retrieving schedule entries.
    pub async fn execute(
        &mut self,
        schedule_check_channels: &mut ScheduleCheckChannels,
        execution_config: ExecutionConfig,
        mut sql_interface_schedule: SqlInterfaceSchedule,
    ) {
        #[cfg(all(target_os = "linux", not(test)))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let sleep_duration_hundred_millis = Duration::from_millis(100);

        let duration_schedule_check_interval =
            Duration::from_secs(self.config.schedule_check_interval.into());
        let mut last_check_instant = Instant::now();
        let mut first_iteration = true;

        loop {
            if self.config.active
                && (last_check_instant.elapsed() > duration_schedule_check_interval
                    || first_iteration)
            {
                first_iteration = false;
                last_check_instant = Instant::now();
                let (res, sql_interface) = tokio::task::spawn_blocking(move || {
                    let res = sql_interface_schedule.read_schedule();
                    (res, sql_interface_schedule)
                })
                .await
                .unwrap();
                sql_interface_schedule = sql_interface;
                if let Err(errors) = res {
                    if !self.lock_error_database_read {
                        for e in errors {
                            log_error_chain(module_path!(), "Reading schedule failed", e);
                        }
                        self.lock_error_database_read = true;
                    }
                } else {
                    self.lock_error_database_read = false;
                }
            }

            let (quit_command_received, _, _) = self.process_external_request(
                &mut schedule_check_channels.rx_schedule_check_from_signal_handler,
                None,
            );
            if quit_command_received {
                #[cfg(feature = "debug_schedule_check")]
                debug!(
                    target: module_path!(),
                    "received QUIT command"
                );

                break;
            }

            //*********** ventilation ***********
            Self::find_schedule_check_request(
                ScheduleType::Ventilation,
                &self.config,
                execution_config.ventilation,
                &sql_interface_schedule,
                &mut self.locks_ventilation,
                "ventilation",
                (
                    &mut schedule_check_channels.tx_schedule_check_to_ventilation,
                    &mut schedule_check_channels.rx_schedule_check_from_ventilation,
                ),
            );

            //*********** heating ***********
            Self::find_schedule_check_request(
                ScheduleType::Heating,
                &self.config,
                execution_config.heating,
                &sql_interface_schedule,
                &mut self.locks_heating,
                "heating",
                (
                    &mut schedule_check_channels.tx_schedule_check_to_heating,
                    &mut schedule_check_channels.rx_schedule_check_from_heating,
                ),
            );

            //*********** refill ***********
            Self::find_schedule_check_request(
                ScheduleType::Refill,
                &self.config,
                execution_config.refill,
                &sql_interface_schedule,
                &mut self.locks_refill,
                "refill",
                (
                    &mut schedule_check_channels.tx_schedule_check_to_refill,
                    &mut schedule_check_channels.rx_schedule_check_from_refill,
                ),
            );

            //*********** Balling ***********
            Self::find_schedule_check_request(
                ScheduleType::Balling,
                &self.config,
                execution_config.balling,
                &sql_interface_schedule,
                &mut self.locks_balling,
                "balling",
                (
                    &mut schedule_check_channels.tx_schedule_check_to_balling,
                    &mut schedule_check_channels.rx_schedule_check_from_balling,
                ),
            );

            tokio::time::sleep(sleep_duration_hundred_millis).await;

            if self.get_last_ping_instant().elapsed() > self.get_ping_interval() {
                sql_interface_schedule = tokio::task::spawn_blocking(move || {
                    sql_interface_schedule.ping();
                    sql_interface_schedule
                })
                .await
                .unwrap();
                self.update_last_ping_instant();
            }
        }

        schedule_check_channels.acknowledge_signal_handler();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::channels::Channels;
    use crate::utilities::channel_content::InternalCommand;
    use chrono::NaiveTime;

    // Helper function to create a default config for tests.
    fn default_test_config(strategy_if_not_found: bool) -> ScheduleCheckConfig {
        ScheduleCheckConfig {
            active: true,
            execute: true,
            schedule_check_interval: 60,
            strategy_if_schedule_not_found: strategy_if_not_found,
        }
    }

    // Helper to create a schedule entry that is always active and covers the entire day.
    fn always_allowed_schedule() -> ScheduleEntry {
        ScheduleEntry::new_for_test(
            ScheduleType::Heating,
            NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
            NaiveTime::from_hms_opt(23, 59, 59).unwrap(),
            true,
        )
    }

    #[test]
    fn test_check_request_allowed_by_schedule() {
        // Arrange
        let config = default_test_config(false);
        let mut locks = ScheduleCheckLocks::default();
        let mut channels = Channels::new_for_test();
        let schedule = always_allowed_schedule();

        channels
            .heating
            .tx_heating_to_schedule_check
            .send(InternalCommand::ScheduleCheck)
            .unwrap();

        // Act
        ScheduleCheck::check_pending_schedule_request(
            &config,
            &mut channels.schedule_check.tx_schedule_check_to_heating,
            &mut channels.schedule_check.rx_schedule_check_from_heating,
            Some(&schedule),
            &mut locks,
            "mock_heating",
        );

        // Assert
        let result = channels.heating.receive_from_schedule_check().unwrap();
        assert_eq!(result, true, "Should be allowed by the active schedule");
        assert!(!locks.channel_receive, "Receive lock should not be set");
        assert!(!locks.channel_send, "Send lock should not be set");
        assert!(
            !locks.inapplicable_command,
            "Command lock should not be set"
        );
    }

    #[test]
    fn test_check_request_allowed_by_inactive_schedule() {
        // Arrange (fallback strategy is irrelevant)
        let config = default_test_config(true);
        let mut locks = ScheduleCheckLocks::default();
        let mut channels = Channels::new_for_test();

        // An inactive schedule is always disallowed.
        let mut schedule = always_allowed_schedule();
        schedule.deactivate();

        channels
            .heating
            .tx_heating_to_schedule_check
            .send(InternalCommand::ScheduleCheck)
            .unwrap();

        // Act
        ScheduleCheck::check_pending_schedule_request(
            &config,
            &mut channels.schedule_check.tx_schedule_check_to_heating,
            &mut channels.schedule_check.rx_schedule_check_from_heating,
            Some(&schedule),
            &mut locks,
            "mock_heating",
        );

        // Assert
        let result = channels.heating.receive_from_schedule_check().unwrap();
        assert_eq!(
            result, true,
            "Should be disallowed by the inactive schedule"
        );
    }

    #[test]
    fn test_check_request_no_schedule_fallback_allow() {
        // Arrange with fallback strategy to allow
        let config = default_test_config(true);
        let mut locks = ScheduleCheckLocks::default();
        let mut channels = Channels::new_for_test();

        channels
            .heating
            .tx_heating_to_schedule_check
            .send(InternalCommand::ScheduleCheck)
            .unwrap();

        // Act
        ScheduleCheck::check_pending_schedule_request(
            &config,
            &mut channels.schedule_check.tx_schedule_check_to_heating,
            &mut channels.schedule_check.rx_schedule_check_from_heating,
            None, // No schedule entry provided
            &mut locks,
            "test_module",
        );

        // Assert
        let result = channels.heating.receive_from_schedule_check().unwrap();
        assert_eq!(result, true, "Should be allowed by the fallback strategy");
    }

    #[test]
    fn test_check_request_no_schedule_fallback_disallow() {
        // Arrange with fallback strategy to disallow
        let config = default_test_config(false);
        let mut locks = ScheduleCheckLocks::default();
        let mut channels = Channels::new_for_test();

        channels
            .heating
            .tx_heating_to_schedule_check
            .send(InternalCommand::ScheduleCheck)
            .unwrap();

        // Act
        ScheduleCheck::check_pending_schedule_request(
            &config,
            &mut channels.schedule_check.tx_schedule_check_to_heating,
            &mut channels.schedule_check.rx_schedule_check_from_heating,
            None, // No schedule entry provided
            &mut locks,
            "test_module",
        );

        // Assert
        let result = channels.heating.receive_from_schedule_check().unwrap();
        assert_eq!(
            result, false,
            "Should be disallowed by the fallback strategy"
        );
    }

    #[test]
    fn test_check_request_inapplicable_command() {
        // Arrange
        let config = default_test_config(false);
        let mut locks = ScheduleCheckLocks::default();
        let mut channels = Channels::new_for_test();
        let schedule = always_allowed_schedule();

        channels
            .heating
            .tx_heating_to_schedule_check
            .send(InternalCommand::ResetAllErrors)
            .unwrap();

        // Act
        ScheduleCheck::check_pending_schedule_request(
            &config,
            &mut channels.schedule_check.tx_schedule_check_to_heating,
            &mut channels.schedule_check.rx_schedule_check_from_heating,
            Some(&schedule),
            &mut locks,
            "test_module",
        );

        assert!(
            channels
                .heating
                .rx_heating_from_schedule_check
                .try_recv()
                .is_err(),
            "Should not send a reply for an inapplicable command"
        );
        assert!(
            locks.inapplicable_command,
            "The inapplicable_command lock should be engaged"
        );
    }

    #[test]
    fn test_check_request_no_request_on_channel() {
        // Arrange
        let config = default_test_config(false);
        let mut locks = ScheduleCheckLocks::default();
        let mut channels = Channels::new_for_test();
        let schedule = always_allowed_schedule();

        // Act
        ScheduleCheck::check_pending_schedule_request(
            &config,
            &mut channels.schedule_check.tx_schedule_check_to_heating,
            &mut channels.schedule_check.rx_schedule_check_from_heating,
            Some(&schedule),
            &mut locks,
            "test_module",
        );

        // Assert
        assert!(
            channels
                .heating
                .rx_heating_from_schedule_check
                .try_recv()
                .is_err(),
            "Should not send a reply when no request is received"
        );
        assert!(
            !locks.channel_receive,
            "Receive lock should not be set on empty channel"
        );
    }

    #[test]
    fn test_check_request_receive_disconnected() {
        // Arrange
        let config = default_test_config(false);
        let mut locks = ScheduleCheckLocks::default();
        let mut channels = Channels::new_for_test();
        let schedule = always_allowed_schedule();

        drop(channels.heating.tx_heating_to_schedule_check); // Drop the sender to simulate a disconnected channel

        // Act
        ScheduleCheck::check_pending_schedule_request(
            &config,
            &mut channels.schedule_check.tx_schedule_check_to_heating,
            &mut channels.schedule_check.rx_schedule_check_from_heating,
            Some(&schedule),
            &mut locks,
            "test_module",
        );

        // Assert
        assert!(
            locks.channel_receive,
            "The channel_receive lock should be engaged on disconnect"
        );
    }

    #[test]
    fn test_check_request_send_disconnected() {
        // Arrange
        let config = default_test_config(true);
        let mut locks = ScheduleCheckLocks::default();
        let mut channels = Channels::new_for_test();
        let schedule = always_allowed_schedule();

        channels
            .heating
            .tx_heating_to_schedule_check
            .send(InternalCommand::ScheduleCheck)
            .unwrap();
        drop(channels.heating.rx_heating_from_schedule_check); // Drop the receiver to simulate a disconnected channel

        // Act
        ScheduleCheck::check_pending_schedule_request(
            &config,
            &mut channels.schedule_check.tx_schedule_check_to_heating,
            &mut channels.schedule_check.rx_schedule_check_from_heating,
            Some(&schedule),
            &mut locks,
            "test_module",
        );

        // Assert
        assert!(
            locks.channel_send,
            "The channel_send lock should be engaged on disconnect"
        );
    }
}
