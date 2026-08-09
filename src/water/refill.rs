/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages the automated freshwater refill system for the aquarium.
//!
//! This module provides the `Refill` struct, which runs as a dedicated thread to
//! intelligently control the water refill process. It acts as a central coordinator,
//! integrating sensor data, historical usage from the database, and external commands
//! to ensure the aquarium's water level is maintained safely and efficiently.
//!
//! ## Core Functionality
//!
//! The `Refill` thread operates in a continuous loop, performing a sophisticated
//! sequence of checks and actions:
//!
//! 1.  **Sensor-Driven Activation**: The primary trigger for a refill is the `TankLevelSwitch`
//!     indicating a low water level.
//!
//! 2.  **Historical Data Analysis**: Before every potential refill, it queries the
//!     database for critical statistics:
//!     - Time elapsed since the last refill.
//!     - Total volume and number of refills in the last hour.
//!     - Total volume and number of refills in the last 24 hours.
//!
//! 3.  **Safety Limit Enforcement**: It strictly enforces configured safety limits. A refill
//!     is **inhibited** if any of the following conditions are met:
//!     - The maximum refill volume or count per hour/24h has been exceeded.
//!     - The minimum time interval between refills has not yet passed.
//!
//! 4.  **Error Detection & Handling**: It includes logic to detect and react to
//!     potential hardware or system failures:
//!     - **Switch Stuck High**: If an unusually long time passes without a refill, it flags a
//!       `error_switch_stuck_high` condition.
//!     - **Switch Stuck Low**: The underlying `WaterInjectionTrait` implementation detects if the
//!       level switch fails to go high during a refill, flagging `error_switch_stuck_low`.
//!     - **Database Failures**: Errors during database reads or writes will inhibit refills.
//!     - All errors can be reset via a `ResetAllErrors` command.
//!
//! 5.  **Schedule Adherence**: It communicates with the `ScheduleCheck` module to ensure
//!     refills only occur during permitted time windows.
//!
//! 6.  **External Control**: It responds to `Start` and `Stop` commands from the `Messaging`
//!     system, allowing for dynamic runtime control without restarting the application.
//!
//! 7.  **Coordinated Actuation**: It uses a shared `device_scheduler` mutex to acquire an
//!     exclusive lock before activating the pump, preventing conflicts with other hardware.
//!
//! 8.  **Graceful Shutdown**: It handles `Quit` and `Terminate` signals for a clean shutdown,
//!     acknowledging the signal handler and waiting for final termination.
//!
//! ## Key Components
//!
//! - **`Refill`**: The main struct holding configuration, state, and many log-suppression flags.
//! - **`RefillStatus`**: A simple struct shared via a `Mutex` to provide a live status
//!   (`refill_in_progress_live`) to other modules, such as the `DataLogger`.
//! - **`execute()`**: The main entry point and control loop for the thread.
//! - **`WaterInjectionTrait`**: An abstraction for the physical act of adding water. This is
//!   the component that directly controls the pump relay and monitors the level switch during injection.
//! - **`DatabaseInterfaceRefillTrait`**: An abstraction for all database interactions.

#[cfg(feature = "debug_refill")]
use log::debug;

use log::info;
use log::warn;

#[cfg(all(not(test), target_os = "linux", not(feature = "test-utils")))]
// only required in real-life execution
use nix::unistd::gettid;

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_error::SqlInterfaceError;
use crate::database::sql_interface_refill::{DatabaseInterfaceRefillTrait, SqlInterfaceRefill};
use crate::perform_schedule_check;
use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::config::{read_config_file, read_config_file_with_test_database, ConfigData};
use crate::utilities::database_ping_trait::DatabasePingTrait;
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;
use crate::water::refill_channels::RefillChannels;
use crate::water::refill_config::RefillConfig;
pub use crate::water::refill_error_states::RefillErrorStates;
use crate::water::refill_monitor_view::RefillMonitorView;
use crate::water::refill_stats_data::RefillStatsData;
use crate::water::refill_status::RefillStatus;
use crate::water::water_injection::WaterInjectionTrait;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MILLIS_PER_SEC: u64 = 1000;

#[derive(Default)]
pub struct RefillLocks {
    /// inhibition flag to avoid flooding the log file with repeated messages about refill volume of last 24h
    volume_last_24h: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about refill count of last 24h
    count_last_24h: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about refill volume of last hour
    volume_last_hour: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about refill count of last hour
    count_last_hour: bool,
}

/// Contains the configuration and the implementation for the refill control.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
/// refill[Refill control] --> water_injection[Water Injection]
/// tank_level_switch[Tank Level Switch] -.-> refill
/// tank_level_switch -.-> water_injection
/// water_injection --> relay_manager[Relay Manager]
/// relay_manager --> water_injection
/// refill --> signal_handler[Signal Handler]
/// signal_handler --> refill
/// signal_handler --> water_injection
/// refill --> schedule_check[Schedule Check]
/// schedule_check --> refill
/// refill -.-> data_logger[Data Logger]
/// messaging[Messaging] --> refill
/// ```
/// Communication channel to and from the relay manager is forwarded to implementation of WaterInjectionTrait.
/// Communication channel to and from data logger is forwarded to implementation of WaterInjectionTrait.
/// Signal handler is communicating both directly with refill as well with implementation of WaterInjectionTrait.
/// Tank level switch calculation communicates both directly with refill control and with implementation of WaterInjectionTrait.
///
/// Architecture of the component is as follows:
/// ```mermaid
/// graph LR
/// execute --> calc_tank_level_switch_position_stabilized;
/// execute --> get_sql_refill_data;
/// execute_refill --> process_external_request;
/// execute --> check_error_condition_switch_stuck_high;
/// execute --> process_external_request;
/// execute --> update_tank_level_switch_signals;
/// execute_refill --> update_tank_level_switch_signals;
/// execute --> check_volume_count_time_limits;
/// execute --> execute_refill;
/// ```
pub struct Refill {
    /// configuration of refill control
    /// Attribute is public because access from within test cases.
    pub config: RefillConfig,

    /// struct of type RefillErrors which includes flags for different kind of errors
    pub refill_errors: RefillErrorStates,

    /// inhibition flag to avoid flooding the log file with repeated messages about stop command being received
    lock_info_stop_command_received: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about start command being received
    lock_info_start_command_received: bool,

    /// inhibition flags to avoid flooding the log file with repeated messages about refill count or volume
    locks_info_check: RefillLocks,

    /// inhibition flag to avoid flooding the log file with repeated messages about refill interval check
    lock_info_refill_check_interval_last_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read duration since the last refill
    lock_error_read_duration_since_last_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to check if the refill log table is empty
    lock_error_check_empty: bool,

    /// inhibition flags to avoid flooding the log file with repeated messages about failure to read refill volume or count
    locks_error_read: RefillLocks,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to request to schedule check via the channel
    lock_error_channel_send_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive from schedule check via the channel
    lock_error_channel_receive_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send view to monitor
    lock_error_channel_send_monitors: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received not applicable command from the signal handler
    pub lock_warn_inapplicable_command_signal_handler: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive from the signal handler via the channel
    pub lock_error_channel_receive_termination: bool,

    /// recording when the last database ping happened
    pub last_ping_instant: Instant,

    /// database ping interval
    pub database_ping_interval: Duration,

    /// The last monitor view sent, to avoid sending redundant data.
    last_sent_monitor_view: Option<RefillMonitorView>,
}

impl Refill {
    /// Creates a new `Refill` control instance.
    ///
    /// This constructor initializes the freshwater refill control module with its specified
    /// configuration and a dedicated SQL database interface. It sets up various internal
    /// error flags to the default (inactive) state and initializes many "lock" flags
    /// to `false`. These lock flags are crucial during operation to prevent log files from
    /// being flooded with repetitive error or warning messages.
    ///
    /// # Arguments
    /// * `config` - **Configuration data** for the refill control, loaded from a TOML file.
    ///   This includes parameters such as refill limits, intervals, and pump flow rates.
    /// * `database_ping_interval` - A `Duration` instance, providing the interval to ping the database.
    ///
    /// # Returns
    /// A new **`Refill` struct**, ready to manage the aquarium's freshwater refill system.
    ///
    /// [impl->swreq~water_calibration_parameters~1]
    pub fn new(config: RefillConfig, database_ping_interval: Duration) -> Refill {
        Self {
            config,
            refill_errors: RefillErrorStates {
                error_switch_stuck_high: false,
                error_switch_stuck_low: false,
                error_receive_tank_level_switch_position: false,
                error_sql_update_failed: false,
                error_sql_get_historic_data_failed: false,
                error_join_failed: false,
            },
            lock_info_stop_command_received: false,
            lock_info_start_command_received: false,
            lock_info_refill_check_interval_last_refill: false,
            lock_error_read_duration_since_last_refill: false,
            lock_error_check_empty: false,
            lock_error_channel_send_schedule_check: false,
            lock_error_channel_receive_schedule_check: false,
            lock_error_channel_send_monitors: false,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
            last_ping_instant: Instant::now(),
            database_ping_interval,
            last_sent_monitor_view: None,
            locks_info_check: Default::default(),
            locks_error_read: Default::default(),
        }
    }

    /// Creates a new `Refill` struct, allowing for explicit control over initial error states.
    ///
    /// This constructor is exclusively used in test environments. It initializes the
    /// refill control module with a provided configuration, a specific SQL interface,
    /// and a predefined `RefillErrors` struct. This allows tests to simulate scenarios
    /// where certain error conditions are already active at startup, enabling more
    /// granular and precise testing of error-handling logic.
    ///
    /// # Arguments
    /// * `config` - **Configuration data** for the refill control, loaded from a TOML file.
    /// * `refill_errors` - A **`RefillErrors` struct** containing the initial states of various
    ///   error flags, allowing tests to set up specific error conditions.
    ///
    /// # Returns
    /// A new **`Refill` struct**, with its `refill_errors` field initialized to the provided value.
    ///
    /// [impl->swreq~water_startup_data_recovery~1]
    pub fn new_with_errors(config: RefillConfig, refill_errors: RefillErrorStates) -> Refill {
        Self {
            config,
            refill_errors,
            lock_info_stop_command_received: false,
            lock_info_start_command_received: false,
            lock_info_refill_check_interval_last_refill: false,
            lock_error_read_duration_since_last_refill: false,
            lock_error_check_empty: false,
            lock_error_channel_send_schedule_check: false,
            lock_error_channel_receive_schedule_check: false,
            lock_error_channel_send_monitors: false,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
            last_ping_instant: Instant::now(),
            database_ping_interval: Duration::from_secs(1000),
            last_sent_monitor_view: None,
            locks_info_check: Default::default(),
            locks_error_read: Default::default(),
        }
    }

    /// Processes the result of a database query for a single refill statistic.
    ///
    /// This generic helper function is designed to handle the outcome of a database
    /// call that fetches a specific piece of historical refill data (e.g., total volume,
    /// total count). It encapsulates the logic for both success and failure scenarios.
    ///
    /// On success (`Ok`), it updates the target `stats_value` with the new data and resets
    /// the associated `lock_flag`, indicating the operation was successful.
    ///
    /// On failure (`Err`), it logs the error using `log_error_chain` and sets the
    /// `lock_flag` to `true`. This built-in log suppression prevents the log file from
    /// being flooded with identical error messages on further failed attempts. The
    /// error is only logged the first time it occurs after a successful read.
    ///
    /// # Type Parameters
    /// * `T`: The type of the statistic being read from the database.
    ///
    /// # Arguments
    /// * `database_result`: The `Result` returned from the database interface method.
    /// * `stats_value`: A mutable reference to the field where the successful result will be stored.
    /// * `lock_flag`: A mutable reference to a boolean flag used for log suppression.
    /// * `check_type`: A string slice describing the type of check (e.g., "volume", "count"), used for logging.
    /// * `duration`: A string slice describing the time window (e.g., "24h", "hour"), used for logging.
    ///
    /// # Returns
    /// * `false` if the database query was successful.
    /// * `true` if the database query failed, indicating an error occurred.
    fn read_refill_stats<T>(
        database_result: Result<T, SqlInterfaceError>,
        stats_value: &mut T,
        lock_flag: &mut bool,
        check_type: &str,
        duration: &str,
    ) -> bool {
        match database_result {
            Ok(value) => {
                *lock_flag = false;
                *stats_value = value;
                false
            }
            Err(_e) => {
                if !*lock_flag {
                    let context =
                        format!("Could not get refill {} of last {}.", check_type, duration);
                    log_error_chain(module_path!(), &context, _e);
                    *lock_flag = true;
                }
                true
            }
        }
    }

    /// Retrieves comprehensive historic refill data from the SQL database and updates the provided container.
    /// This helper function queries the `sql_interface_refill` for various statistics
    /// crucial for refill control:
    /// - Duration since the last refill operation.
    /// - Total volume and count of refills in the last 24 hours.
    /// - Total volume and count of refills in the last hour.
    ///
    /// It populates the `refill_stats_data` struct with these values. Errors encountered during
    /// database communication are logged and cause the `error_sql_get_historic_data_failed` flag
    /// in `refill_errors` to be set, potentially inhibiting further refills. Internal lock flags
    /// prevent log flooding.
    ///
    /// # Arguments
    /// * `refill_stats_data` - A mutable reference to a `RefillStatsData` struct, which will be
    ///   populated with the retrieved historical data.
    /// * `is_multi_thread` - A boolean flag indicating if the Tokio runtime is running in multithreaded mode.
    fn get_refill_stats_data(
        &mut self,
        refill_stats_data: &mut RefillStatsData,
        sql_interface_refill: &mut dyn DatabaseInterfaceRefillTrait,
        is_multi_thread: bool,
    ) {
        let mut action = || {
            match sql_interface_refill.check_empty() {
                Ok(0) => {
                    self.lock_error_check_empty = false;
                    refill_stats_data.duration_since_last_refill =
                        self.config.min_interval_last_refill.saturating_add(1);
                }
                Ok(_) => {
                    self.lock_error_check_empty = false;
                    match sql_interface_refill.get_duration_since_last_refill() {
                        Ok(duration) => {
                            self.lock_error_read_duration_since_last_refill = false;
                            refill_stats_data.duration_since_last_refill = duration;
                        }
                        Err(_e) => {
                            // [impl->swreq~water_db_failure_inhibition~1]
                            self.refill_errors.error_sql_get_historic_data_failed = true;
                            if !self.lock_error_read_duration_since_last_refill {
                                log_error_chain(
                                    // no required during testing
                                    module_path!(),
                                    "could not get duration since last refill event",
                                    _e,
                                );
                                self.lock_error_read_duration_since_last_refill = true;
                            }
                        }
                    }
                }
                Err(_e) => {
                    self.refill_errors.error_sql_get_historic_data_failed = true;
                    if !self.lock_error_check_empty {
                        log_error_chain(
                            module_path!(),
                            "could not check if refill table is empty or not",
                            _e,
                        );
                        self.lock_error_check_empty = true;
                    }
                }
            };

            self.refill_errors.error_sql_get_historic_data_failed |= Self::read_refill_stats(
                sql_interface_refill.get_refill_volume_of_last_24h(),
                &mut refill_stats_data.refill_volume_last_24h,
                &mut self.locks_error_read.volume_last_24h,
                "volume",
                "24h",
            );

            self.refill_errors.error_sql_get_historic_data_failed |= Self::read_refill_stats(
                sql_interface_refill.get_refill_count_of_last_24h(),
                &mut refill_stats_data.refill_count_last_24h,
                &mut self.locks_error_read.count_last_24h,
                "count",
                "24h",
            );

            self.refill_errors.error_sql_get_historic_data_failed |= Self::read_refill_stats(
                sql_interface_refill.get_refill_volume_of_last_hour(),
                &mut refill_stats_data.refill_volume_last_hour,
                &mut self.locks_error_read.volume_last_hour,
                "volume",
                "hour",
            );

            self.refill_errors.error_sql_get_historic_data_failed |= Self::read_refill_stats(
                sql_interface_refill.get_refill_count_of_last_hour(),
                &mut refill_stats_data.refill_count_last_hour,
                &mut self.locks_error_read.count_last_hour,
                "count",
                "hour",
            );
        };

        if is_multi_thread {
            tokio::task::block_in_place(action);
        } else {
            action();
        }
    }

    /// Checks if the duration since the last refill operation exceeds a configurable safety limit, indicating a potential "switch stuck high" error.
    ///
    /// This private helper function compares the `duration_since_last_refill` with `max_duration_since_last_refill` from the configuration.
    /// If the elapsed time is longer than the configured maximum, it implies that the water level switch might be stuck in a "high"
    /// position, preventing detection of low water levels and thus inhibiting refills. The corresponding error flag in `refill_errors` is set if this condition is met.
    /// This helps to detect and log a critical fault condition.
    ///
    /// # Arguments
    /// * `duration_since_last_refill` - The duration (in seconds) that has passed since the last refill operation.
    ///
    /// [impl->swreq~water_error_detection~1]
    pub fn check_error_condition_switch_stuck_high(&mut self, duration_since_last_refill: u64) {
        self.refill_errors.error_switch_stuck_high =
            duration_since_last_refill > self.config.max_duration_since_last_refill;
    }

    /// Calculates the **inhibition flag** for the refill control based on received start and stop commands.
    ///
    /// This private helper function determines the current operational state (inhibited or active)
    /// of the refill control. A `Stop` command will inhibit refilling, while a `Start` command
    /// will activate it. The `Start` command takes precedence over `Stop` if both are received
    /// simultaneously. It uses internal lock flags to prevent log flooding from repeated info messages.
    ///
    /// # Arguments
    /// * `current_state` - The current boolean value of the inhibition flag (`true` if inhibited, `false` if active) before processing new commands.
    /// * `start_command_received` - A flag indicating whether a `Start` command was received from an external source in the current cycle.
    /// * `stop_command_received` - A flag indicating whether a `Stop` command was received from an external source in the current cycle.
    ///
    /// # Returns
    /// A `bool` representing the **new state of the inhibition flag** after processing the commands.
    ///
    /// [impl->swreq~water_determine_refill~1]
    fn calc_inhibition_flag(
        &mut self,
        current_state: bool,
        start_command_received: bool,
        stop_command_received: bool,
    ) -> bool {
        // initialize return value with current state
        let mut new_state: bool = current_state;

        if stop_command_received {
            // [impl->swreq~water_log_flood_protection~1]
            if !self.lock_info_stop_command_received {
                #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                info!(
                    target: module_path!(),
                    "received Stop command. Inhibiting refill control."
                );

                self.lock_info_stop_command_received = true;
            }
            new_state = true;
        } else {
            self.lock_info_stop_command_received = false;
        }
        // The start command has a higher priority than the stop command.
        // Therefore, it is evaluated after the stop command.
        if start_command_received {
            // [impl->swreq~water_log_flood_protection~1]
            if !self.lock_info_start_command_received {
                #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                info!(
                    target: module_path!(),
                    "received Start command. Restarting refill control."
                );

                self.lock_info_start_command_received = true;
            }
            new_state = false;
        } else {
            self.lock_info_start_command_received = false;
        }
        new_state
    }

    /// Checks a given value against a maximum limit and logs a warning if exceeded.
    ///
    /// This generic helper function is used to check various refill statistics (like
    /// volume or count) against their configured maximums. It returns `true` if the
    /// limit is exceeded, indicating a potential problem.
    ///
    /// A key feature is its built-in log suppression. It uses a mutable `lock_flag`
    /// to ensure that a warning message for a specific transgression is logged only
    /// once. The lock is automatically reset when the value returns to a normal range,
    /// allowing future warnings if the limit is exceeded again.
    ///
    /// # Type Parameters
    /// * `T`: A generic type that must support comparison (`PartialOrd`) and display
    ///   formatting (`Display`). This allows the function to work with both integers
    ///   (for counts) and floating-point numbers (for volumes).
    ///
    /// # Arguments
    /// * `actual_value`: The current measured value (e.g., `150.0` liters).
    /// * `max_value`: The configured maximum permissible value (e.g., `100.0` liters).
    /// * `lock_flag`: A mutable reference to a boolean flag used for log suppression.
    /// * `check_type`: A string slice describing the type of check (e.g., "volume", "count").
    /// * `duration`: A string slice describing the time window (e.g., "24h", "hour").
    ///
    /// # Returns
    /// * `true` if `actual_value` is greater than `max_value`.
    /// * `false` if `actual_value` is within the limit.
    ///
    /// [impl->swreq~water_faulty_switch_protection~1]
    fn check_volume_count_time_limit<T: std::fmt::Display + PartialOrd>(
        actual_value: T,
        max_value: T,
        lock_flag: &mut bool,
        check_type: &str,
        duration: &str,
    ) -> bool {
        if actual_value > max_value {
            if !*lock_flag {
                info!(
                    target: module_path!(),
                    "refill {} of last {} exceeds permissible {} per {} of {} L",
                    check_type, duration, check_type, duration, max_value
                );

                *lock_flag = true;
            }
            true
        } else {
            *lock_flag = false;
            false
        }
    }

    /// Checks if historical refill data (volume, count, and time interval) **exceeds configured limitations**.
    ///
    /// This private helper function performs a series of checks against the `RefillConfig`'s
    /// maximum permitted values for:
    /// - Total refill volume within the last 24 hours.
    /// - Total refill count within the last 24 hours.
    /// - Total refill volume within the last hour.
    /// - Total refill count within the last hour.
    /// - Minimum time interval since the last refill operation.
    ///
    /// If any limit is exceeded, an informational message is logged (once per transgression
    /// thanks to internal lock flags), indicating a potential issue that might inhibit further refills.
    ///
    /// # Arguments
    /// * `sql_refill_data` - A reference to a `RefillStatsData` struct containing the historic
    ///   refill data retrieved from the SQL database.
    /// * `tx_refill_to_monitors` - The sender part of the channel for communication to the monitors.
    ///
    /// # Returns
    /// `true` if **any** of the configured volume, count, or time limits are exceeded; `false` otherwise.
    /// * `refill_channels` - A mutable reference to the `RefillChannels` communication interface.
    pub fn check_volume_count_time_limits(
        &mut self,
        sql_refill_data: &RefillStatsData,
        refill_channels: &mut RefillChannels,
    ) -> bool {
        #[cfg(feature = "debug_refill")]
        debug!(
            target: module_path!(),
            "check_volume_count_time_limits called.",
        );

        let check_result_volume_last_24h = Self::check_volume_count_time_limit(
            sql_refill_data.refill_volume_last_24h,
            self.config.max_refill_volume_24h,
            &mut self.locks_info_check.volume_last_24h,
            "volume",
            "24h",
        );

        let check_result_count_last_24h = Self::check_volume_count_time_limit(
            sql_refill_data.refill_count_last_24h,
            self.config.max_refill_count_24h,
            &mut self.locks_info_check.count_last_24h,
            "count",
            "24h",
        );

        let check_result_volume_last_hour = Self::check_volume_count_time_limit(
            sql_refill_data.refill_volume_last_hour,
            self.config.max_refill_volume_hour,
            &mut self.locks_info_check.volume_last_hour,
            "volume",
            "hour",
        );

        let check_result_count_last_hour = Self::check_volume_count_time_limit(
            sql_refill_data.refill_count_last_hour,
            self.config.max_refill_count_hour,
            &mut self.locks_info_check.count_last_hour,
            "count",
            "hour",
        );

        let refill_check_interval_last_refill =
            sql_refill_data.duration_since_last_refill < self.config.min_interval_last_refill;
        if refill_check_interval_last_refill {
            // [impl->swreq~water_log_flood_protection~1]
            if !self.lock_info_refill_check_interval_last_refill {
                #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                info!(
                    target: module_path!(),
                    "time interval since last refill {} is too small (limit={}).",
                    sql_refill_data.duration_since_last_refill, self.config.min_interval_last_refill
                );

                self.lock_info_refill_check_interval_last_refill = true;
            }
        } else {
            self.lock_info_refill_check_interval_last_refill = false;
        }

        let refill_monitor_view = RefillMonitorView {
            refill_check_volume_last_24h: check_result_volume_last_24h,
            refill_check_count_last_24h: check_result_count_last_24h,
            refill_check_volume_last_hour: check_result_volume_last_hour,
            refill_check_count_last_hour: check_result_count_last_hour,
            refill_check_interval_last_refill,
        };

        let has_issue = refill_monitor_view.has_any_issue();
        self.send_monitor_view_log_flood(refill_channels, refill_monitor_view);
        has_issue
    }

    /// Executes the **main control loop for the refill module**.
    ///
    /// This function runs continuously, managing the aquarium's freshwater refill system
    /// until a termination signal is received. It periodically checks the tank water
    /// level, assesses historical refill data against configured limits, and initiates
    /// refill operations when necessary and permitted by schedules.
    ///
    /// **Key Operations:**
    /// - **Initial Wait**: Observes an initial waiting period after application startup before active control begins.
    /// - **Sensor Data Acquisition**: Reads raw and stabilized tank level switch signals from a shared mutex.
    /// - **SQL Data Retrieval**: Fetches historical refill data (counts, volumes, last refill time) from the database.
    /// - **Safety Checks**: Verifies if refill operations are within configured limits (max count/volume per hour/24h) and checks for a "tank level switch stuck high" error.
    /// - **Schedule & External Control**: Communicates with the `ScheduleCheck` module and responds to `Start`/`Stop` commands to inhibit or enable refilling.
    /// - **Water Injection**: If all conditions are met (low water, within limits, allowed by schedule, not inhibited), it triggers water injection via the `WaterInjectionTrait`.
    /// - **Data Logging Communication**: Periodically communicates the refill pump status to the `DataLogger`.
    /// - **Cycle Management**: Maintains a fixed cycle time, sleeping as necessary, and warning if execution duration exceeds the cycle limit.
    /// - **Graceful Shutdown**: Responds to `Quit` and `Terminate` commands from the signal handler, ensuring a safe shutdown.
    ///
    /// # Arguments
    /// * `mutex_device_scheduler_refill` - A clone of the `Arc<Mutex<i32>>` used to coordinate device
    ///   actuation across the application, preventing conflicts and tracking activity counts.
    /// * `refill_channels` - A struct containing all sender and receiver channels for communication
    ///   with modules like `RelayManager`, `SignalHandler`, `DataLogger`, `ScheduleCheck`, and `Messaging`.
    /// * `water_injection` - A mutable reference to an object implementing `WaterInjectionTrait`,
    ///   responsible for the physical control of the refill pump.
    /// * `mutex_tank_level_switch_signals_clone_for_refill` - The `Arc<Mutex<TankLevelSwitchSignals>>`
    ///   containing the latest tank level switch readings (raw and stabilized).
    /// * `sql_interface_refill` - The **specific SQL interface** for refill-related database operations.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute(
        &mut self,
        mutex_device_scheduler_refill: Arc<tokio::sync::Mutex<i32>>,
        refill_channels: &mut RefillChannels,
        water_injection: &mut (impl WaterInjectionTrait + Send),
        mutex_tank_level_switch_signals_clone_for_refill: Arc<Mutex<TankLevelSwitchSignals>>,
        mutex_refill_status: Arc<Mutex<RefillStatus>>,
        mut sql_interface_refill: Box<dyn DatabaseInterfaceRefillTrait + Sync + Send>,
        is_multi_thread: bool,
    ) {
        #[cfg(all(target_os = "linux", not(test), not(feature = "test-utils")))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let cycle_time_duration =
            Duration::from_millis(self.config.check_interval * MILLIS_PER_SEC);
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let mut i = 0;

        let mut schedule_check_result: bool;
        #[cfg(feature = "debug_refill")]
        {
            schedule_check_result = false;
        }

        let mut tank_level_switch_invalid: bool = false;
        let mut tank_level_switch_position_stabilized: bool = true;

        #[cfg(feature = "debug_refill")]
        {
            tank_level_switch_invalid = false;
            tank_level_switch_position_stabilized = true;
        }

        let mut lock_warn_cycle_time_exceeded: bool = false;
        let mut cycle_time_exceeded: bool; // ensures that signal handler/messaging are polled at least once even if cycle time is exceeded

        // retrieving data from SQL database
        let mut refill_stats_data = RefillStatsData::default();
        self.get_refill_stats_data(
            &mut refill_stats_data,
            &mut *sql_interface_refill,
            is_multi_thread,
        );

        let mut refill_inhibited = false; // a flag indicating the external request to inhibit refill from injecting water
        let mut quit_command_received = false; // the request to end the application has been received
        let mut start_command_received; // the request to (re-)start refill control has been received
        let mut stop_command_received; // the request to (temporarily) stop the refill control has been received
        let mut water_injected = false; // inhibit cycle time monitoring when water injection happens

        // initial waiting time
        if self.config.initial_wait_interval > 0 {
            while i < self.config.initial_wait_interval * 10 {
                match i.checked_add(1) {
                    Some(c) => i = c,
                    None => {
                        // overflow
                        break;
                    }
                }

                (
                    quit_command_received,
                    start_command_received,
                    stop_command_received,
                ) = self.process_external_request(
                    &mut refill_channels.rx_refill_from_signal_handler,
                    Some(&mut refill_channels.rx_refill_from_messaging),
                );
                refill_inhibited = self.calc_inhibition_flag(
                    refill_inhibited,
                    start_command_received,
                    stop_command_received,
                );
                if quit_command_received {
                    break;
                }
                tokio::time::sleep(sleep_duration_hundred_millis).await;
            }
        }

        // frequent update required for calculation of the error condition switch stuck high
        refill_stats_data.duration_since_last_refill = refill_stats_data
            .duration_since_last_refill
            .saturating_add(self.config.initial_wait_interval);

        let mut start_time = Instant::now();

        // main control loop for refill
        loop {
            // check if either during the main control loop or during refill execution the termination request has been received
            if quit_command_received {
                break; // exit outer loop
            }

            if self.config.active {
                // check when the last refill operation has taken place
                // if too much time has passed, issue an error condition - not impeding rest of operation
                self.check_error_condition_switch_stuck_high(
                    refill_stats_data.duration_since_last_refill,
                );

                {
                    // internal scope to limit lifetime of mutex lock
                    match mutex_refill_status.lock() {
                        Ok(mut refill_status) => {
                            refill_status.external_inhibition_request = refill_inhibited;
                        }
                        Err(_e) => {
                            log_error_chain(
                                module_path!(),
                                "error locking mutex for refill status",
                                _e,
                            );
                        }
                    };
                }

                if !quit_command_received && !refill_inhibited {
                    // access mutex for reading tank level switch signals
                    {
                        // [impl->swreq~water_read_level_switch~1]
                        match mutex_tank_level_switch_signals_clone_for_refill.lock() {
                            Ok(c) => {
                                tank_level_switch_position_stabilized =
                                    c.tank_level_switch_position_stabilized;
                                tank_level_switch_invalid = c.tank_level_switch_invalid;
                            }
                            Err(_) => {
                                // Do nothing, keep values as they are.
                            }
                        }
                    }

                    self.get_refill_stats_data(
                        &mut refill_stats_data,
                        &mut *sql_interface_refill,
                        is_multi_thread,
                    );

                    let critical_error_present = self
                        .check_volume_count_time_limits(&refill_stats_data, refill_channels)
                        || self.refill_errors.error_switch_stuck_low
                        || self.refill_errors.error_sql_update_failed
                        || self.refill_errors.error_sql_get_historic_data_failed
                        || self.refill_errors.error_join_failed;

                    {
                        // internal scope to limit lifetime of mutex lock
                        match mutex_refill_status.lock() {
                            Ok(mut refill_status) => {
                                refill_status.critical_error_present = critical_error_present;
                            }
                            Err(_e) => {
                                log_error_chain(
                                    module_path!(),
                                    "error locking mutex for refill status",
                                    _e,
                                );
                            }
                        };
                    }

                    // execute checks before initiating refill
                    if !critical_error_present {
                        // [impl->swreq~water_observe_schedule~2]
                        perform_schedule_check!(
                            refill_channels,
                            schedule_check_result,
                            self.lock_error_channel_send_schedule_check,
                            self.lock_error_channel_receive_schedule_check,
                            module_path!()
                        );

                        // [impl->swreq~water_read_level_switch~1]
                        if schedule_check_result
                            && !tank_level_switch_position_stabilized
                            && !tank_level_switch_invalid
                        {
                            {
                                // internal scope to limit lifetime of mutex lock
                                match mutex_refill_status.lock() {
                                    Ok(mut refill_status) => {
                                        refill_status.refill_in_progress_live = true;
                                        refill_status.refill_in_progress_for_database = true;
                                    }
                                    Err(_e) => {
                                        log_error_chain(
                                            module_path!(),
                                            "error locking mutex for refill status",
                                            _e,
                                        );
                                    }
                                };
                            }

                            // conditions for allowing the refill operation are given
                            quit_command_received = water_injection
                                .inject_water(
                                    refill_channels,
                                    &mut self.refill_errors,
                                    &mut sql_interface_refill,
                                    &mutex_tank_level_switch_signals_clone_for_refill,
                                    &mutex_device_scheduler_refill,
                                )
                                .await;

                            water_injected = true;

                            {
                                // internal scope to limit lifetime of mutex lock
                                match mutex_refill_status.lock() {
                                    Ok(mut refill_status) => {
                                        refill_status.refill_in_progress_live = false;
                                    }
                                    Err(_e) => {
                                        log_error_chain(
                                            module_path!(),
                                            "error locking mutex for refill status",
                                            _e,
                                        );
                                    }
                                };
                            }
                        }
                    }
                }
            }

            #[cfg(feature = "debug_refill")]
            debug!(
                target: module_path!(),
                "refill_inhibited = {}, error_switch_stuck_low={}, error_sql_update_failed={}, error_sql_get_historic_data_failed={}, schedule_check_result={}, tank_level_switch_position_stabilized={}, tank_level_switch_invalid={}",
                refill_inhibited,
                self.refill_errors.error_switch_stuck_low,
                self.refill_errors.error_sql_update_failed,
                self.refill_errors.error_sql_get_historic_data_failed,
                schedule_check_result,
                tank_level_switch_position_stabilized,
                tank_level_switch_invalid
            );

            let stop_time = Instant::now();
            let execution_duration = stop_time.duration_since(start_time);

            let mut remaining_sleep_time_millis: u64 = 0;
            if execution_duration >= cycle_time_duration {
                if !lock_warn_cycle_time_exceeded && !water_injected {
                    warn!(target: module_path!(),
                        "execution duration of {} ms exceeds cycle time of {} ms",
                        execution_duration.as_millis(),
                        cycle_time_duration.as_millis()
                    );
                    lock_warn_cycle_time_exceeded = true;
                }
                cycle_time_exceeded = true;
            } else {
                // Execution duration is smaller than cycle time in this branch.
                // No risk of underflow
                let remaining_sleep_time_duration = cycle_time_duration - execution_duration;
                remaining_sleep_time_millis = remaining_sleep_time_duration.as_millis() as u64;
                cycle_time_exceeded = false;
                lock_warn_cycle_time_exceeded = false;
            }
            water_injected = false;

            #[cfg(feature = "debug_refill")]
            debug!(
                target: module_path!(),
                "execution duration = {}, cycle time = {}, remaining_sleep_time_millis = {}",
                execution_duration_millis,
                self.config.check_interval * MILLIS_PER_SEC,
                remaining_sleep_time_millis
            );

            // skip further waiting if the quit command has been received during refilling
            if !quit_command_received {
                // wait for the configured time period before rechecking the sensor signal
                // check for external requests while waiting
                i = 0;
                while (i < remaining_sleep_time_millis) || cycle_time_exceeded {
                    match i.checked_add(100) {
                        Some(c) => i = c,
                        None => {
                            // overflow
                            break;
                        }
                    }

                    (
                        quit_command_received,
                        start_command_received,
                        stop_command_received,
                    ) = self.process_external_request(
                        &mut refill_channels.rx_refill_from_signal_handler,
                        Some(&mut refill_channels.rx_refill_from_messaging),
                    );
                    refill_inhibited = self.calc_inhibition_flag(
                        refill_inhibited,
                        start_command_received,
                        stop_command_received,
                    );
                    if quit_command_received {
                        break;
                    }
                    tokio::time::sleep(sleep_duration_hundred_millis).await;

                    self.check_timing_and_ping_database(
                        &mut *sql_interface_refill,
                        is_multi_thread,
                    );

                    cycle_time_exceeded = false;
                }
            }

            start_time = Instant::now();

            refill_stats_data.duration_since_last_refill = refill_stats_data
                .duration_since_last_refill
                .saturating_add(self.config.check_interval);
        }

        // The application received the request to terminate. That is why the loop was left.
        // Answer to the thread which sent the request for termination, so that shutdown can proceed further.
        refill_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads (relay manager, tank level switch).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination_async(
            &mut refill_channels.rx_refill_from_signal_handler,
            sleep_duration_hundred_millis,
            module_path!(),
        )
        .await;
    }

    /// Sends monitor view to Monitors channel with log flood protection.
    ///
    /// [impl->swreq~water_log_flood_protection~1]
    pub(crate) fn send_monitor_view_log_flood(
        &mut self,
        refill_channels: &mut RefillChannels,
        refill_monitor_view: RefillMonitorView,
    ) {
        let mut monitor_view_is_different = true;
        if let Some(previous_monitor_view) = &self.last_sent_monitor_view {
            if &refill_monitor_view == previous_monitor_view {
                monitor_view_is_different = false;
            }
        }
        if monitor_view_is_different {
            match refill_channels.send_to_monitor(refill_monitor_view.clone()) {
                Ok(_) => {
                    self.lock_error_channel_send_monitors = false;
                    self.last_sent_monitor_view = Some(refill_monitor_view);
                }
                Err(_e) => {
                    // [impl->swreq~water_log_flood_protection~1]
                    if !self.lock_error_channel_send_monitors {
                        log_error_chain(module_path!(), "sending view to monitor failed", _e);
                        self.lock_error_channel_send_monitors = true;
                    }
                }
            }
        }
    }
}

/// Prepares and returns a configured `Refill` control instance along with its associated interfaces for testing.
///
/// This helper function is designed exclusively for testing the `Refill` module. It sets up
/// a controlled test environment by:
/// 1.  Loading a generic or specific test configuration file.
/// 2.  Overriding the `initial_wait_interval` and `check_interval` within the `RefillConfig`.
/// 3.  Establishing a connection to and providing an `SqlInterface` and `SqlInterfaceRefill` instance.
/// 4.  Optionally injecting a predefined `RefillErrors` struct into the `Refill` instance's initial state,
///     allowing for tests to start with specific error conditions.
///
/// # Arguments
/// * `refill_errors` - An `Option<RefillErrors>`. If `Some`, the `Refill` instance is initialized
///   with these specific error flags; otherwise, default `RefillErrors` (all flags `false`) are used.
/// * `initial_wait_interval` - The `u64` value to set for `RefillConfig.initial_wait_interval` in the test setup.
/// * `check_interval` - The `u64` value to set for `RefillConfig.check_interval` in the test setup.
/// * `test_db_number_opt` - An `Option<u32>`. If `Some`, a test-specific database (`aquarium_test_XX`)
///   will be used from the configuration; otherwise, the generic test database is used.
///
/// # Returns
/// A tuple `(ConfigData, Refill, SqlInterface)`:
/// - The `ConfigData` struct, containing the loaded and modified configuration used for the test.
/// - The fully initialized `Refill` struct, ready for test execution.
/// - The `SqlInterface` instance, providing access to the overall SQL database for test assertions or further setup.
///
/// # Panics
/// This function will panic if:
/// - `read_config_file` or `read_config_file_with_test_database` fail (e.g., file not found, invalid DB number).
/// - It fails to connect to the SQL database.
/// - It fails to get a connection from the SQL pool for `SqlInterfaceRefill` initialization.
///
/// [utest->swreq~water_determine_refill~1]
pub fn prepare_refill_tests(
    refill_errors: Option<RefillErrorStates>,
    initial_wait_interval: u64,
    check_interval: u64,
    test_db_number_opt: Option<u32>,
) -> (ConfigData, Refill, SqlInterface, SqlInterfaceRefill) {
    let config_file_name = "/config/aquarium_control_test_generic.toml";

    // prepare struct Refill for subsequent tests
    let mut config: ConfigData = match test_db_number_opt {
        Some(test_db_number) => {
            read_config_file_with_test_database(config_file_name.to_string(), test_db_number)
        }
        None => read_config_file(config_file_name.to_string()).unwrap(),
    };

    config.refill.initial_wait_interval = initial_wait_interval;
    config.refill.check_interval = check_interval;

    let max_rows_refill = config.sql_interface.max_rows_data;

    println!("Testing with database {}", config.sql_interface.db_name);
    let sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };
    let sql_interface_refill =
        SqlInterfaceRefill::new(sql_interface.get_connection().unwrap(), max_rows_refill).unwrap();
    let refill_config = config.refill.clone();
    (
        config,
        match refill_errors.is_some() {
            true => Refill::new_with_errors(refill_config, refill_errors.unwrap()),
            false => Refill::new(refill_config, Duration::from_millis(1000)),
        },
        sql_interface,
        *Box::new(sql_interface_refill),
    )
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub mod tests {
    // unit tests shall not appear in code coverage measurement
    use crate::database::sql_interface_error::SqlInterfaceError;
    use crate::water::refill::Refill;

    #[test]
    // Tests the `read_refill_stats` helper function for both success and failure cases.
    /// [utest->swreq~water_startup_data_recovery~1]
    fn test_read_refill_stats() {
        // === Test Case 1: Successful read with u64 ===
        // This tests the happy path where the database returns a valid u64 value.
        let mut stats_value_u64: u64 = 0;
        let mut lock_flag_u64 = true; // Start with lock ON to see if it gets turned OFF.
        let db_result_ok_u64: Result<u64, SqlInterfaceError> = Ok(123);

        let error_occurred_u64 = Refill::read_refill_stats(
            db_result_ok_u64,
            &mut stats_value_u64,
            &mut lock_flag_u64,
            "dummy_value_type_not_used",
            "dummy_duration_not_used",
        );

        assert!(!error_occurred_u64, "Should return false on success (u64)");
        assert_eq!(
            stats_value_u64, 123,
            "stats_value should be updated on success (u64)"
        );
        assert!(
            !lock_flag_u64,
            "lock_flag should be reset to false on success (u64)"
        );

        // === Test Case 2: Successful read with f32 ===
        // This confirms the function's generic implementation works for f32 as well.
        let mut stats_value_f32: f32 = 0.0;
        let mut lock_flag_f32 = true;
        let db_result_ok_f32: Result<f32, SqlInterfaceError> = Ok(45.6);

        let error_occurred_f32 = Refill::read_refill_stats(
            db_result_ok_f32,
            &mut stats_value_f32,
            &mut lock_flag_f32,
            "dummy_value_type_not_used",
            "dummy_duration_not_used",
        );

        assert!(!error_occurred_f32, "Should return false on success (f32)");
        assert_eq!(
            stats_value_f32, 45.6,
            "stats_value should be updated on success (f32)"
        );
        assert!(
            !lock_flag_f32,
            "lock_flag should be reset to false on success (f32)"
        );

        // === Test Case 3: Failed read ===
        // This tests the failure path where the database returns an error.
        let mut stats_value_fail: u64 = 999;
        let mut lock_flag_fail = false; // Start with lock OFF.
        let db_result_err: Result<u64, SqlInterfaceError> =
            Err(SqlInterfaceError::ReadRefillStatsError);

        let error_occurred_fail = Refill::read_refill_stats(
            db_result_err,
            &mut stats_value_fail,
            &mut lock_flag_fail,
            "dummy_value_type_not_used",
            "dummy_duration_not_used",
        );

        assert!(error_occurred_fail, "Should return true on failure");
        assert_eq!(
            stats_value_fail, 999,
            "stats_value should NOT be changed on failure"
        );
        assert!(lock_flag_fail, "lock_flag should be set to true on failure");
    }

    #[test]
    // Tests the `check_volume_count_time_limit` helper function.
    /// [utest->swreq~water_faulty_switch_protection~1]
    /// [utest->swreq~water_log_flood_protection~1]
    fn test_check_volume_count_time_limit() {
        // === Test Case 1: Value is below the limit (u64) ===
        // Should return false, indicating no limit was exceeded.
        // The lock flag should not be changed.
        let mut lock_flag = false;
        let result = Refill::check_volume_count_time_limit(
            50u64,
            100u64,
            &mut lock_flag,
            "dummy_value_type_not_used",
            "dummy_duration_not_used",
        );
        assert!(!result, "Should return false when value is below limit");
        assert!(!lock_flag, "Lock flag should remain false");

        // === Test Case 2: Value is equal to the limit (u64) ===
        // Should return false, as the check is for values *greater than* the max.
        let mut lock_flag = false;
        let result = Refill::check_volume_count_time_limit(
            100u64,
            100u64,
            &mut lock_flag,
            "dummy_value_type_not_used",
            "dummy_duration_not_used",
        );
        assert!(!result, "Should return false when value is equal to limit");
        assert!(!lock_flag, "Lock flag should remain false");

        // === Test Case 3: Value exceeds the limit for the first time (u64) ===
        // Should return true and set the lock flag to prevent log spam.
        let mut lock_flag = false;
        let result = Refill::check_volume_count_time_limit(
            101u64,
            100u64,
            &mut lock_flag,
            "dummy_value_type_not_used",
            "dummy_duration_not_used",
        );
        assert!(result, "Should return true when value exceeds limit");
        assert!(
            lock_flag,
            "Lock flag should be set to true when limit is exceeded"
        );

        // === Test Case 4: Value exceeds the limit again (the lock flag is already set) ===
        // Should still return true. The logging logic would be skipped. The lock flag should remain true.
        let mut lock_flag = true;
        let result = Refill::check_volume_count_time_limit(
            102u64,
            100u64,
            &mut lock_flag,
            "dummy_value_type_not_used",
            "dummy_duration_not_used",
        );
        assert!(result, "Should return true on subsequent exceedance");
        assert!(lock_flag, "Lock flag should remain true");

        // === Test Case 5: Value returns to normal after being over the limit ===
        // Should return false and, importantly, reset the lock flag to false,
        // allowing future warnings to be logged if the limit is exceeded again.
        let mut lock_flag = true; // Start with the lock on
        let result = Refill::check_volume_count_time_limit(
            99u64,
            100u64,
            &mut lock_flag,
            "dummy_value_type_not_used",
            "dummy_duration_not_used",
        );
        assert!(!result, "Should return false when value is back to normal");
        assert!(
            !lock_flag,
            "Lock flag should be reset to false when value is back to normal"
        );

        // === Test Case 6: Test with f32 values to confirm generic implementation ===
        // Same logic as above, but with floating-point numbers.
        let mut lock_flag_f32 = false;
        let result_f32 = Refill::check_volume_count_time_limit(
            100.1f32,
            100.0f32,
            &mut lock_flag_f32,
            "dummy_value_type_not_used",
            "dummy_duration_not_used",
        );
        assert!(
            result_f32,
            "Should return true for f32 values exceeding limit"
        );
        assert!(lock_flag_f32, "Lock flag should be set for f32 values");

        let mut lock_flag_f32_reset = true;
        let result_f32_reset = Refill::check_volume_count_time_limit(
            99.9f32,
            100.0f32,
            &mut lock_flag_f32_reset,
            "dummy_value_type_not_used",
            "dummy_duration_not_used",
        );
        assert!(
            !result_f32_reset,
            "Should return false for f32 values below limit"
        );
        assert!(
            !lock_flag_f32_reset,
            "Lock flag should be reset for f32 values"
        );
    }

    #[test]
    /// Tests calc_inhibition_flag log flood protection flags (lock_info_stop_command_received, lock_info_start_command_received).
    ///
    /// [utest->swreq~water_log_flood_protection~1]
    fn test_calc_inhibition_flag_log_flood_protection() {
        let (config, _, _, _) = super::prepare_refill_tests(None, 1, 1, Some(20));
        let mut refill = Refill::new(config.refill, std::time::Duration::from_millis(1000));

        // 1. Initial lock flags
        assert!(!refill.lock_info_stop_command_received);
        assert!(!refill.lock_info_start_command_received);

        // 2. Test stop command flag handling
        let state1 = refill.calc_inhibition_flag(false, false, true);
        assert!(state1);
        assert!(refill.lock_info_stop_command_received);

        let state2 = refill.calc_inhibition_flag(true, false, true);
        assert!(state2);
        assert!(refill.lock_info_stop_command_received);

        let state3 = refill.calc_inhibition_flag(true, false, false);
        assert!(state3);
        assert!(!refill.lock_info_stop_command_received);

        // 3. Test start command flag handling
        let state4 = refill.calc_inhibition_flag(true, true, false);
        assert!(!state4);
        assert!(refill.lock_info_start_command_received);

        let state5 = refill.calc_inhibition_flag(false, false, false);
        assert!(!state5);
        assert!(!refill.lock_info_start_command_received);
    }

    #[test]
    /// Tests send_monitor_view_log_flood lock flag (lock_error_channel_send_monitors).
    ///
    /// [utest->swreq~water_log_flood_protection~1]
    fn test_send_monitor_view_log_flood_protection() {
        let (config, _, _, _) = super::prepare_refill_tests(None, 1, 1, Some(20));
        let mut refill = Refill::new(config.refill, std::time::Duration::from_millis(1000));

        assert!(!refill.lock_error_channel_send_monitors);

        let view = crate::water::refill_monitor_view::RefillMonitorView {
            refill_check_volume_last_24h: false,
            refill_check_count_last_24h: false,
            refill_check_volume_last_hour: false,
            refill_check_count_last_hour: false,
            refill_check_interval_last_refill: false,
        };

        let (tx_mon, _rx_mon) = crate::launch::channels::channel(1, false);
        let (tx_relay, _rx_relay) = crate::launch::channels::channel::<
            crate::utilities::channel_content::InternalCommand,
        >(1, false);
        let (_tx_relay_resp, rx_relay) = crate::launch::channels::channel::<bool>(1, false);
        let (tx_sig, _rx_sig) = crate::launch::channels::channel::<bool>(1, false);
        let (_tx_sig_resp, rx_sig) = crate::launch::channels::channel::<
            crate::utilities::channel_content::InternalCommand,
        >(1, false);
        let (tx_sched, _rx_sched) = crate::launch::channels::channel::<
            crate::utilities::channel_content::InternalCommand,
        >(1, false);
        let (_tx_sched_resp, rx_sched) = crate::launch::channels::channel::<bool>(1, false);
        let (_tx_msg, rx_msg) = crate::launch::channels::channel::<
            crate::utilities::channel_content::InternalCommand,
        >(1, false);

        let mut refill_channels = crate::water::refill_channels::RefillChannels {
            tx_refill_to_relay_manager: tx_relay,
            rx_refill_from_relay_manager: rx_relay,
            tx_refill_to_signal_handler: tx_sig,
            rx_refill_from_signal_handler: rx_sig,
            tx_refill_to_schedule_check: tx_sched,
            rx_refill_from_schedule_check: rx_sched,
            rx_refill_from_messaging: rx_msg,
            tx_refill_to_monitors: tx_mon,
        };

        // 1. Successful send
        refill.send_monitor_view_log_flood(&mut refill_channels, view.clone());
        assert!(!refill.lock_error_channel_send_monitors);

        // 2. Disconnected channel send failure
        let (tx_mon_fail, rx_mon_fail) = crate::launch::channels::channel(1, false);
        drop(rx_mon_fail);
        refill_channels.tx_refill_to_monitors = tx_mon_fail;

        let new_view = crate::water::refill_monitor_view::RefillMonitorView {
            refill_check_volume_last_24h: true,
            refill_check_count_last_24h: false,
            refill_check_volume_last_hour: false,
            refill_check_count_last_hour: false,
            refill_check_interval_last_refill: false,
        };

        refill.send_monitor_view_log_flood(&mut refill_channels, new_view.clone());
        assert!(
            refill.lock_error_channel_send_monitors,
            "lock_error_channel_send_monitors should be true when channel send fails"
        );

        let diff_view = crate::water::refill_monitor_view::RefillMonitorView {
            refill_check_volume_last_24h: true,
            refill_check_count_last_24h: true,
            refill_check_volume_last_hour: false,
            refill_check_count_last_hour: false,
            refill_check_interval_last_refill: false,
        };

        refill.send_monitor_view_log_flood(&mut refill_channels, diff_view);
        assert!(
            refill.lock_error_channel_send_monitors,
            "lock_error_channel_send_monitors should remain true on repeated failure"
        );
    }

    #[test]
    // Verifies check_error_condition_switch_stuck_high sets error_switch_stuck_high flag when threshold exceeded.
    // [utest->swreq~water_error_detection~1]
    fn test_check_error_condition_switch_stuck_high() {
        let (config, _, _, _) = super::prepare_refill_tests(None, 1, 1, Some(20));
        let mut refill = Refill::new(config.refill, std::time::Duration::from_millis(1000));
        refill.config.max_duration_since_last_refill = 100;

        // Within limit
        refill.check_error_condition_switch_stuck_high(50);
        assert!(!refill.refill_errors.error_switch_stuck_high);

        // Equal to limit (not exceeding)
        refill.check_error_condition_switch_stuck_high(100);
        assert!(!refill.refill_errors.error_switch_stuck_high);

        // Exceeding limit
        refill.check_error_condition_switch_stuck_high(101);
        assert!(refill.refill_errors.error_switch_stuck_high);
    }

    #[test]
    // Verifies check_volume_count_time_limits detects limit exceedances and populates monitor view.
    // [utest->swreq~water_faulty_switch_protection~1]
    fn test_check_volume_count_time_limits_has_any_issue() {
        let (config, _, _, _) = super::prepare_refill_tests(None, 1, 1, Some(20));
        let mut refill = Refill::new(config.refill, std::time::Duration::from_millis(1000));
        refill.config.max_refill_volume_24h = 10.0;
        refill.config.max_refill_count_24h = 5;
        refill.config.max_refill_volume_hour = 2.0;
        refill.config.max_refill_count_hour = 2;
        refill.config.min_interval_last_refill = 300;

        let (tx_mon, _rx_mon) = crate::launch::channels::channel(1, false);
        let (tx_relay, _rx_relay) = crate::launch::channels::channel::<
            crate::utilities::channel_content::InternalCommand,
        >(1, false);
        let (_tx_relay_resp, rx_relay) = crate::launch::channels::channel::<bool>(1, false);
        let (tx_sig, _rx_sig) = crate::launch::channels::channel::<bool>(1, false);
        let (_tx_sig_resp, rx_sig) = crate::launch::channels::channel::<
            crate::utilities::channel_content::InternalCommand,
        >(1, false);
        let (tx_sched, _rx_sched) = crate::launch::channels::channel::<
            crate::utilities::channel_content::InternalCommand,
        >(1, false);
        let (_tx_sched_resp, rx_sched) = crate::launch::channels::channel::<bool>(1, false);
        let (_tx_msg, rx_msg) = crate::launch::channels::channel::<
            crate::utilities::channel_content::InternalCommand,
        >(1, false);

        let mut refill_channels = crate::water::refill_channels::RefillChannels {
            tx_refill_to_relay_manager: tx_relay,
            rx_refill_from_relay_manager: rx_relay,
            tx_refill_to_signal_handler: tx_sig,
            rx_refill_from_signal_handler: rx_sig,
            tx_refill_to_schedule_check: tx_sched,
            rx_refill_from_schedule_check: rx_sched,
            rx_refill_from_messaging: rx_msg,
            tx_refill_to_monitors: tx_mon,
        };

        // Stats within limits
        let valid_stats = crate::water::refill_stats_data::RefillStatsData {
            duration_since_last_refill: 500,
            refill_volume_last_24h: 5.0,
            refill_count_last_24h: 2,
            refill_volume_last_hour: 1.0,
            refill_count_last_hour: 1,
        };

        let has_issue_clean =
            refill.check_volume_count_time_limits(&valid_stats, &mut refill_channels);
        assert!(!has_issue_clean);

        // Stats exceeding 24h volume limit
        let invalid_stats = crate::water::refill_stats_data::RefillStatsData {
            duration_since_last_refill: 500,
            refill_volume_last_24h: 15.0,
            refill_count_last_24h: 2,
            refill_volume_last_hour: 1.0,
            refill_count_last_hour: 1,
        };

        let has_issue_exceeded =
            refill.check_volume_count_time_limits(&invalid_stats, &mut refill_channels);
        assert!(has_issue_exceeded);
    }
}
