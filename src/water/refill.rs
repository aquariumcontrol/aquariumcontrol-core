/* Copyright 2024 Uwe Martin

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

use log::warn;

#[cfg(not(test))]
use log::info;

#[cfg(all(not(test), target_os = "linux"))]
use nix::unistd::gettid;

use spin_sleep::SpinSleeper;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::database::sql_interface_error::SqlInterfaceError;
use crate::database::sql_interface_refill::DatabaseInterfaceRefillTrait;
use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::database_ping_trait::DatabasePingTrait;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;
use crate::utilities::logger::log_error_chain;
use crate::perform_schedule_check;
use crate::water::refill_channels::RefillChannels;
use crate::water::refill_config::RefillConfig;
use crate::water::refill_error_states::RefillErrorStates;
use crate::water::refill_monitor_view::RefillMonitorView;
use crate::water::refill_stats_data::RefillStatsData;
use crate::water::water_injection::WaterInjectionTrait;

const MILLIS_PER_SEC: u64 = 1000;

/// Struct contains the status of the refill control for communication to the data logger.
pub struct RefillStatus {
    /// Status bit of refill control: It follows the actuation of the refill pump.
    pub refill_in_progress_live: bool,

    /// Status bit of refill control for data logger:
    /// - It is set with the start when the refill pump is actuated.
    /// - It is unset by the data logger to make sure the data logger has captured the refill.
    pub refill_in_progress_for_database: bool,
}

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
    pub fn new(config: RefillConfig, database_ping_interval: Duration) -> Refill {
        Self {
            config,
            refill_errors: RefillErrorStates {
                error_switch_stuck_high: false,
                error_switch_stuck_low: false,
                error_receive_tank_level_switch_position: false,
                error_sql_update_failed: false,
                error_sql_get_historic_data_failed: false,
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

    #[cfg(test)]
    // Creates a new `Refill` struct, allowing for explicit control over initial error states.
    //
    // This constructor is exclusively used in test environments. It initializes the
    // refill control module with a provided configuration, a specific SQL interface,
    // and a predefined `RefillErrors` struct. This allows tests to simulate scenarios
    // where certain error conditions are already active at startup, enabling more
    // granular and precise testing of error-handling logic.
    //
    // # Arguments
    // * `config` - **Configuration data** for the refill control, loaded from a TOML file.
    // * `refill_errors` - A **`RefillErrors` struct** containing the initial states of various
    //   error flags, allowing tests to set up specific error conditions.
    //
    // # Returns
    // A new **`Refill` struct**, with its `refill_errors` field initialized to the provided value.
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
        #[cfg(not(test))] check_type: &str,
        #[cfg(not(test))] duration: &str,
    ) -> bool {
        match database_result {
            Ok(value) => {
                *lock_flag = false;
                *stats_value = value;
                false
            }
            Err(_e) => {
                #[cfg(not(test))]
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
    fn get_refill_stats_data(
        &mut self,
        refill_stats_data: &mut RefillStatsData,
        sql_interface_refill: &mut dyn DatabaseInterfaceRefillTrait,
    ) {
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
                        self.refill_errors.error_sql_get_historic_data_failed = true;
                        #[cfg(not(test))]
                        if !self.lock_error_read_duration_since_last_refill {
                            log_error_chain(
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
                #[cfg(not(test))]
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
            #[cfg(not(test))]
            "volume",
            #[cfg(not(test))]
            "24h",
        );

        self.refill_errors.error_sql_get_historic_data_failed |= Self::read_refill_stats(
            sql_interface_refill.get_refill_count_of_last_24h(),
            &mut refill_stats_data.refill_count_last_24h,
            &mut self.locks_error_read.count_last_24h,
            #[cfg(not(test))]
            "count",
            #[cfg(not(test))]
            "24h",
        );

        self.refill_errors.error_sql_get_historic_data_failed |= Self::read_refill_stats(
            sql_interface_refill.get_refill_volume_of_last_hour(),
            &mut refill_stats_data.refill_volume_last_hour,
            &mut self.locks_error_read.volume_last_hour,
            #[cfg(not(test))]
            "volume",
            #[cfg(not(test))]
            "hour",
        );

        self.refill_errors.error_sql_get_historic_data_failed |= Self::read_refill_stats(
            sql_interface_refill.get_refill_count_of_last_hour(),
            &mut refill_stats_data.refill_count_last_hour,
            &mut self.locks_error_read.count_last_hour,
            #[cfg(not(test))]
            "count",
            #[cfg(not(test))]
            "hour",
        );
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
    fn check_error_condition_switch_stuck_high(&mut self, duration_since_last_refill: u64) {
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
    fn calc_inhibition_flag(
        &mut self,
        current_state: bool,
        start_command_received: bool,
        stop_command_received: bool,
    ) -> bool {
        // initialize return value with current state
        let mut new_state: bool = current_state;

        if stop_command_received {
            if !self.lock_info_stop_command_received {
                #[cfg(not(test))]
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
            if !self.lock_info_start_command_received {
                #[cfg(not(test))]
                info!(
                    target: module_path!(),
                    "received Start command. Restarting refill control."
                );

                self.lock_info_start_command_received = true;
            } else {
                self.lock_info_start_command_received = true;
            }
            new_state = false;
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
    fn check_volume_count_time_limit<T: std::fmt::Display + PartialOrd>(
        actual_value: T,
        max_value: T,
        lock_flag: &mut bool,
        #[cfg(not(test))] check_type: &str,
        #[cfg(not(test))] duration: &str,
    ) -> bool {
        if actual_value > max_value {
            if !*lock_flag {
                #[cfg(not(test))]
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
    fn check_volume_count_time_limits(
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
            #[cfg(not(test))]
            "volume",
            #[cfg(not(test))]
            "24h",
        );

        let check_result_count_last_24h = Self::check_volume_count_time_limit(
            sql_refill_data.refill_count_last_24h,
            self.config.max_refill_count_24h,
            &mut self.locks_info_check.count_last_24h,
            #[cfg(not(test))]
            "count",
            #[cfg(not(test))]
            "24h",
        );

        let check_result_volume_last_hour = Self::check_volume_count_time_limit(
            sql_refill_data.refill_volume_last_hour,
            self.config.max_refill_volume_hour,
            &mut self.locks_info_check.volume_last_hour,
            #[cfg(not(test))]
            "volume",
            #[cfg(not(test))]
            "hour",
        );

        let check_result_count_last_hour = Self::check_volume_count_time_limit(
            sql_refill_data.refill_count_last_hour,
            self.config.max_refill_count_hour,
            &mut self.locks_info_check.count_last_hour,
            #[cfg(not(test))]
            "count",
            #[cfg(not(test))]
            "hour",
        );

        let refill_check_interval_last_refill =
            sql_refill_data.duration_since_last_refill < self.config.min_interval_last_refill;
        if refill_check_interval_last_refill {
            if !self.lock_info_refill_check_interval_last_refill {
                #[cfg(not(test))]
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

        // Only send the view if it's different from the last one we sent.
        let mut monitor_view_is_different = true;
        if self.last_sent_monitor_view.is_some() {
            let previous_monitor_view = self.last_sent_monitor_view.clone().unwrap();
            if refill_monitor_view == previous_monitor_view {
                monitor_view_is_different = false;
            }
        }
        if monitor_view_is_different {
            match refill_channels.send_to_monitor(refill_monitor_view) {
                Ok(_) => {
                    self.lock_error_channel_send_monitors = false;
                }
                Err(e) => {
                    if !self.lock_error_channel_send_monitors {
                        log_error_chain(module_path!(), "sending view to monitor failed", e);
                        self.lock_error_channel_send_monitors = true;
                    }
                }
            }
        }

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
    pub fn execute(
        &mut self,
        mutex_device_scheduler_refill: Arc<Mutex<i32>>,
        refill_channels: &mut RefillChannels,
        water_injection: &mut impl WaterInjectionTrait,
        mutex_tank_level_switch_signals_clone_for_refill: Arc<Mutex<TankLevelSwitchSignals>>,
        mutex_refill_status: Arc<Mutex<RefillStatus>>,
        mut sql_interface_refill: Box<dyn DatabaseInterfaceRefillTrait + Sync + Send>,
    ) {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let cycle_time_duration =
            Duration::from_millis(self.config.check_interval * MILLIS_PER_SEC);
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let mut i = 0;
        let spin_sleeper = SpinSleeper::default();

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

        // retrieving data from SQL data base
        let mut refill_stats_data = RefillStatsData::default();
        self.get_refill_stats_data(&mut refill_stats_data, &mut *sql_interface_refill);

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
                    refill_channels.rx_refill_from_messaging_opt.as_mut(),
                );
                refill_inhibited = self.calc_inhibition_flag(
                    refill_inhibited,
                    start_command_received,
                    stop_command_received,
                );
                if quit_command_received {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_hundred_millis);
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

                if !quit_command_received && !refill_inhibited {
                    // access mutex for reading tank level switch signals
                    {
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

                    self.get_refill_stats_data(&mut refill_stats_data, &mut *sql_interface_refill);

                    // execute checks before initiating refill
                    if !self.check_volume_count_time_limits(&refill_stats_data, refill_channels)
                        && !self.refill_errors.error_switch_stuck_low
                        && !self.refill_errors.error_sql_update_failed
                        && !self.refill_errors.error_sql_get_historic_data_failed
                    {
                        perform_schedule_check!(
                            refill_channels,
                            schedule_check_result,
                            self.lock_error_channel_send_schedule_check,
                            self.lock_error_channel_receive_schedule_check,
                            module_path!()
                        );

                        if schedule_check_result
                            && !tank_level_switch_position_stabilized
                            && !tank_level_switch_invalid
                        {
                            #[cfg(feature = "debug_refill")]
                            debug!(
                                target: module_path!(),
                                "acquiring semaphore...",
                            );

                            // conditions for allowing the refill operation are given
                            let mut mutex_data = mutex_device_scheduler_refill.lock().unwrap();
                            // now we are given exclusive access to actuators

                            #[cfg(feature = "debug_refill")]
                            debug!(
                                target: module_path!(),
                                "mutex_data = {}",
                                mutex_data
                            );

                            {
                                // internal scope to limit lifetime of mutex lock
                                match mutex_refill_status.lock() {
                                    Ok(mut refill_status) => {
                                        refill_status.refill_in_progress_live = true;
                                        refill_status.refill_in_progress_for_database = true;
                                    }
                                    Err(e) => {
                                        log_error_chain(
                                            module_path!(),
                                            "error locking mutex for refill status",
                                            e,
                                        );
                                    }
                                };
                            }

                            quit_command_received = water_injection.inject_water(
                                refill_channels,
                                &mut self.refill_errors,
                                &mut sql_interface_refill,
                                &mutex_tank_level_switch_signals_clone_for_refill,
                            );
                            *mutex_data = mutex_data.saturating_add(1);
                            water_injected = true;

                            {
                                // internal scope to limit lifetime of mutex lock
                                match mutex_refill_status.lock() {
                                    Ok(mut refill_status) => {
                                        refill_status.refill_in_progress_live = false;
                                    }
                                    Err(e) => {
                                        log_error_chain(
                                            module_path!(),
                                            "error locking mutex for refill status",
                                            e,
                                        );
                                    }
                                };
                            }

                            #[cfg(feature = "debug_refill")]
                            debug!(
                                target: module_path!(),
                                "releasing semaphore..."
                            );
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
                        refill_channels.rx_refill_from_messaging_opt.as_mut(),
                    );
                    refill_inhibited = self.calc_inhibition_flag(
                        refill_inhibited,
                        start_command_received,
                        stop_command_received,
                    );
                    if quit_command_received {
                        break;
                    }
                    spin_sleeper.sleep(sleep_duration_hundred_millis);

                    self.check_timing_and_ping_database(&mut *sql_interface_refill);

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
        #[cfg(test)]
        println!(
            "{}: sending Quit confirmation to signal handler.",
            module_path!()
        );

        refill_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads (relay manager, tank level switch).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination(
            &mut refill_channels.rx_refill_from_signal_handler,
            sleep_duration_hundred_millis,
            module_path!(),
        );
    }
}

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface_error::SqlInterfaceError;
    use crate::database::sql_query_strings::SQL_TABLE_REFILL;
    use crate::database::{sql_interface::SqlInterface, sql_interface_refill::SqlInterfaceRefill};
    use crate::launch::channels::{channel, AquaChannelError, AquaReceiver, AquaSender, Channels};
    use crate::mocks::mock_monitors::tests::MockMonitors;
    use crate::mocks::mock_schedule_check::tests::mock_schedule_check;
    use crate::mocks::mock_sql_interface_refill::tests::MockSqlInterfaceRefill;
    use crate::mocks::mock_water_injection::tests::MockWaterInjection;
    use crate::mocks::test_command::tests::TestCommand;
    use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::config::{
        read_config_file, read_config_file_with_test_database, ConfigData,
    };
    use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
    use crate::water::refill::Refill;
    use crate::water::refill::RefillErrorStates;
    use crate::water::refill::RefillStatus;
    use all_asserts::{assert_ge, assert_le};
    use spin_sleep::SpinSleeper;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::utilities::logger::setup_logger;
    use crate::utilities::logger_config::LoggerConfig;
    use crate::water::refill_stats_data::RefillStatsData;

    // Prepares and returns a configured `Refill` control instance along with its associated interfaces for testing.
    //
    // This helper function is designed exclusively for testing the `Refill` module. It sets up
    // a controlled test environment by:
    // 1.  Loading a generic or specific test configuration file.
    // 2.  Overriding the `initial_wait_interval` and `check_interval` within the `RefillConfig`.
    // 3.  Establishing a connection to and providing an `SqlInterface` and `SqlInterfaceRefill` instance.
    // 4.  Optionally injecting a predefined `RefillErrors` struct into the `Refill` instance's initial state,
    //     allowing for tests to start with specific error conditions.
    //
    // # Arguments
    // * `refill_errors` - An `Option<RefillErrors>`. If `Some`, the `Refill` instance is initialized
    //   with these specific error flags; otherwise, default `RefillErrors` (all flags `false`) are used.
    // * `initial_wait_interval` - The `u64` value to set for `RefillConfig.initial_wait_interval` in the test setup.
    // * `check_interval` - The `u64` value to set for `RefillConfig.check_interval` in the test setup.
    // * `test_db_number_opt` - An `Option<u32>`. If `Some`, a test-specific database (`aquarium_test_XX`)
    //   will be used from the configuration; otherwise, the generic test database is used.
    //
    // # Returns
    // A tuple `(ConfigData, Refill, SqlInterface)`:
    // - The `ConfigData` struct, containing the loaded and modified configuration used for the test.
    // - The fully initialized `Refill` struct, ready for test execution.
    // - The `SqlInterface` instance, providing access to the overall SQL database for test assertions or further setup.
    //
    // # Panics
    // This function will panic if:
    // - `read_config_file` or `read_config_file_with_test_database` fail (e.g., file not found, invalid DB number).
    // - It fails to connect to the SQL database.
    // - It fails to get a connection from the SQL pool for `SqlInterfaceRefill` initialization.
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
            SqlInterfaceRefill::new(sql_interface.get_connection().unwrap(), max_rows_refill)
                .unwrap();
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

    /// Prepares a `Refill` control instance with a **mock database interface** for isolated testing.
    ///
    /// This helper function is designed exclusively for unit and integration testing of the `Refill` module.
    /// It sets up a controlled environment by:
    /// 1.  Loading a specific test configuration file (using `test_db_number` to select the database).
    /// 2.  Overriding the `initial_wait_interval` and `check_interval` within the `RefillConfig` to suit test needs.
    /// 3.  **Injecting a `MockSqlInterfaceRefill` instance** as the database interface, ensuring that tests
    ///     do not interact with a real SQL database. This allows for predictable and fast test execution.
    /// 4.  Optionally providing a predefined `RefillErrors` struct to set up specific error conditions
    ///     at the `Refill` instance's startup.
    ///
    /// # Arguments
    /// * `refill_errors` - An `Option<RefillErrors>`. If `Some`, the `Refill` instance is initialized
    ///   with these specific error flags; otherwise, default `RefillErrors` (all flags `false`) are used.
    /// * `initial_wait_interval` - The `u64` value to set for `RefillConfig.initial_wait_interval` in the test setup.
    /// * `check_interval` - The `u64` value to set for `RefillConfig.check_interval` in the test setup.
    /// * `test_db_number` - A **unique `u32` identifier** used to select a specific test database (`aquarium_test_XX`)
    ///   from the configuration. This ensures test isolation even with database-related config.
    ///
    /// # Returns
    /// A tuple `(ConfigData, Refill)`:
    /// - The `ConfigData` struct, containing the loaded and modified configuration used for the test.
    /// - The fully initialized `Refill` struct, with its database interactions handled by `MockSqlInterfaceRefill`.
    ///
    /// # Panics
    /// This function will panic if:
    /// - `read_config_file_with_test_database` fails (e.g., the configuration file is not found or the `test_db_number` is invalid).
    /// - There are issues during the initialization of the `Refill` struct (e.g., due to an invalid `refill_config`).
    pub fn prepare_refill_tests_with_mock_database_interface(
        refill_errors: Option<RefillErrorStates>,
        initial_wait_interval: u64,
        check_interval: u64,
        test_db_number: u32,
    ) -> (ConfigData, Refill, MockSqlInterfaceRefill) {
        let config_file_name = "/config/aquarium_control_test_generic.toml";

        // prepare struct Refill for subsequent tests
        let mut config: ConfigData =
            read_config_file_with_test_database(config_file_name.to_string(), test_db_number);

        config.refill.initial_wait_interval = initial_wait_interval;
        config.refill.check_interval = check_interval;

        let sql_interface_refill = MockSqlInterfaceRefill {};

        let refill_config = config.refill.clone();
        (
            config,
            match refill_errors.is_some() {
                true => Refill::new_with_errors(refill_config, refill_errors.unwrap()),
                false => Refill::new(refill_config, Duration::from_millis(1000)),
            },
            sql_interface_refill,
        )
    }

    #[test]
    // Test case checks the implementation of ProcessExternalRequestTrait.
    // The test case uses test database #46.
    pub fn test_refill_process_external_request_empty_channel() {
        let refill_errors = RefillErrorStates {
            error_switch_stuck_high: false,
            error_switch_stuck_low: false,
            error_receive_tank_level_switch_position: false,
            error_sql_update_failed: false,
            error_sql_get_historic_data_failed: false,
        };
        let (_, mut refill, _, _) = prepare_refill_tests(Some(refill_errors), 2, 60, Some(46));

        let (_, mut rx_refill_from_signal_handler): (
            AquaSender<InternalCommand>,
            AquaReceiver<InternalCommand>,
        ) = channel(1);

        let (_, mut rx_refill_from_messaging): (
            AquaSender<InternalCommand>,
            AquaReceiver<InternalCommand>,
        ) = channel(1);

        let (quit_command_received, start_command_received, stop_command_received) = refill
            .process_external_request(
                &mut rx_refill_from_signal_handler,
                Some(&mut rx_refill_from_messaging),
            );

        assert_eq!(quit_command_received, false);
        assert_eq!(start_command_received, false);
        assert_eq!(stop_command_received, false);
    }

    #[test]
    // This test case executes multiple iterations of the loop inside the .execute function, including the triggering of a refill.
    // Test case uses test database #20.
    pub fn test_refill_trigger_water_injection() {
        let sleep_duration_50_millis = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut refill, mut sql_interface, sql_interface_refill) =
            prepare_refill_tests(None, 1, 1, Some(20));

        let mut channels = Channels::new_for_test();

        // test environment-specific channels
        let (
            tx_mock_water_injection_to_test_environment,
            mut rx_test_environment_from_mock_water_injection,
        ) = channel(1);
        let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
            channel::<InternalCommand>(1);

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
        let mutex_tank_level_switch_signals_for_refill = mutex_tank_level_switch_signals.clone();
        let refill_status = RefillStatus {
            refill_in_progress_live: false,
            refill_in_progress_for_database: false,
        };
        let mutex_refill_status = Arc::new(Mutex::new(refill_status));

        let mut mock_monitors = MockMonitors::new();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mutex_device_scheduler = Arc::new(Mutex::new(0));

        let mut mock_water_injection = MockWaterInjection::new(
            Some(tx_mock_water_injection_to_test_environment),
            RefillErrorStates {
                error_switch_stuck_high: false,
                error_switch_stuck_low: false,
                error_receive_tank_level_switch_position: false,
                error_sql_update_failed: false,
                error_sql_get_historic_data_failed: false,
            },
        );

        // thread for mock monitors
        let join_handle_mock_monitors = thread::Builder::new()
            .name("mock_monitor".to_string())
            .spawn(move || {
                mock_monitors.execute(
                    Some(channels.monitors.rx_monitors_from_refill),
                    None,
                    rx_monitors_from_test_environment,
                );
            })
            .unwrap();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_refill,
                    &mut channels.schedule_check.rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_secs = Duration::from_secs(2);
                let spin_sleeper = SpinSleeper::default();

                // wait for the test object to trigger water injection
                let _ = rx_test_environment_from_mock_water_injection
                    .recv()
                    .unwrap();

                // update of mock tank level position signals
                {
                    let mut mutex_tank_level_switch_signals =
                        mutex_tank_level_switch_signals.lock().unwrap();
                    mutex_tank_level_switch_signals.tank_level_switch_position_stabilized = true;
                    mutex_tank_level_switch_signals.tank_level_switch_position = true;
                }

                let start_time = Instant::now();
                spin_sleeper.sleep(sleep_duration_two_secs);

                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_refill
                    .recv()
                    .unwrap();

                let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Terminate);
                let finish_time = Instant::now();
                let finalization_duration_millis =
                    finish_time.duration_since(start_time).as_millis();

                assert_ge!(finalization_duration_millis, 2000u128);
                assert_le!(finalization_duration_millis, 2150u128);

                // wait for the test object to finish so that mock threads receive
                // the termination request before the end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive-call for signal update
        spin_sleeper.sleep(sleep_duration_50_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let mut tx_refill_to_schedule_check_for_test_case_finish =
                    channels.refill.tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler,
                    &mut channels.refill,
                    &mut mock_water_injection,
                    mutex_tank_level_switch_signals_for_refill,
                    mutex_refill_status,
                    Box::new(sql_interface_refill),
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            })
            .unwrap();

        join_handle_mock_monitors
            .join()
            .expect("Mock monitor check did not finish.");
        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // This test case executes multiple iterations of the loop inside the .execute function, including triggering a refill and simulation of the switch stuck low scenario.
    // Test case uses test database #47.
    pub fn test_refill_trigger_water_injection_switch_stuck_high() {
        let sleep_duration_50_millis = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut refill, mut sql_interface, sql_interface_refill) =
            prepare_refill_tests(None, 1, 1, Some(47));

        let mut channels = Channels::new_for_test();

        // test environment-specific channels
        let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
            channel::<InternalCommand>(1);
        let (
            tx_mock_water_injection_to_test_environment,
            mut rx_test_environment_from_mock_water_injection,
        ) = channel(1);

        // this test case shall output debug! macros
        setup_logger(LoggerConfig::default()).unwrap();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
        let refill_status = RefillStatus {
            refill_in_progress_live: false,
            refill_in_progress_for_database: false,
        };
        let mutex_refill_status = Arc::new(Mutex::new(refill_status));

        let mut mock_monitors = MockMonitors::new();

        let mut mock_water_injection = MockWaterInjection::new(
            Some(tx_mock_water_injection_to_test_environment),
            RefillErrorStates {
                error_switch_stuck_high: false,
                error_switch_stuck_low: true,
                error_receive_tank_level_switch_position: false,
                error_sql_update_failed: false,
                error_sql_get_historic_data_failed: false,
            },
        );

        // thread for mock monitors
        let join_handle_mock_monitors = thread::Builder::new()
            .name("mock_monitor".to_string())
            .spawn(move || {
                mock_monitors.execute(
                    Some(channels.monitors.rx_monitors_from_refill),
                    None,
                    rx_monitors_from_test_environment,
                );
            })
            .unwrap();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_refill,
                    &mut channels.schedule_check.rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_secs = Duration::from_secs(2);
                let spin_sleeper = SpinSleeper::default();

                // wait for the test object to trigger water injection
                let _ = rx_test_environment_from_mock_water_injection
                    .recv()
                    .unwrap();

                let start_time = Instant::now();
                spin_sleeper.sleep(sleep_duration_two_secs);

                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_refill
                    .recv()
                    .unwrap();
                let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Terminate);
                let finish_time = Instant::now();
                let finalization_duration_millis =
                    finish_time.duration_since(start_time).as_millis();

                println!(
                    "finalization_duration_millis = {}",
                    finalization_duration_millis
                );

                assert_ge!(finalization_duration_millis, 2000u128);
                assert_le!(finalization_duration_millis, 2150u128);

                // wait for the test object to finish so that mock threads receive
                // the termination request before the end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive-call for signal update
        spin_sleeper.sleep(sleep_duration_50_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let mut tx_refill_to_schedule_check_for_test_case_finish =
                    channels.refill.tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler,
                    &mut channels.refill,
                    &mut mock_water_injection,
                    mutex_tank_level_switch_signals,
                    mutex_refill_status,
                    Box::new(sql_interface_refill),
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            })
            .unwrap();

        join_handle_mock_monitors
            .join()
            .expect("Mock monitor check did not finish.");
        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // This test case executes one iteration of the loop inside the .execute function with triggering a refill
    // It checks the timing of the initial wait period and main control interval.
    // Test case uses test database #21.
    pub fn test_refill_initial_waiting_time() {
        let sleep_duration_50_millis = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();

        let (config, mut refill, mut sql_interface, sql_interface_refill) =
            prepare_refill_tests(None, 2, 60, Some(21));

        let mut channels = Channels::new_for_test();

        // test environment-specific channels
        let (
            tx_mock_water_injection_to_test_environment,
            mut rx_test_environment_from_mock_water_injection,
        ) = channel(1);
        let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
            channel::<InternalCommand>(1);

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
        let refill_status = RefillStatus {
            refill_in_progress_live: false,
            refill_in_progress_for_database: false,
        };
        let mutex_refill_status = Arc::new(Mutex::new(refill_status));

        let mut mock_monitors = MockMonitors::new();

        let mut mock_water_injection = MockWaterInjection::new(
            Some(tx_mock_water_injection_to_test_environment),
            RefillErrorStates {
                error_switch_stuck_high: false,
                error_switch_stuck_low: false,
                error_receive_tank_level_switch_position: false,
                error_sql_update_failed: false,
                error_sql_get_historic_data_failed: false,
            },
        );

        // thread for mock monitors
        let join_handle_mock_monitors = thread::Builder::new()
            .name("mock_monitor".to_string())
            .spawn(move || {
                mock_monitors.execute(
                    Some(channels.monitors.rx_monitors_from_refill),
                    None,
                    rx_monitors_from_test_environment,
                );
            })
            .unwrap();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_refill,
                    &mut channels.schedule_check.rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let start_time = Instant::now();

                // wait for the test object to trigger water injection
                let _ = rx_test_environment_from_mock_water_injection
                    .recv()
                    .unwrap();

                // record time after the test object triggered water injection
                let initial_waiting_time_passed = Instant::now();

                // now we can send the quit command
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_refill
                    .recv()
                    .unwrap();
                let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);
                let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Terminate);
                let finish_time = Instant::now();
                let initial_wait_duration_millis = initial_waiting_time_passed
                    .duration_since(start_time)
                    .as_millis();
                let finalization_duration_millis = finish_time
                    .duration_since(initial_waiting_time_passed)
                    .as_millis();

                assert_le!(
                    initial_wait_duration_millis,
                    (config.refill.initial_wait_interval * 1000 + 200) as u128
                );
                assert_ge!(
                    initial_wait_duration_millis,
                    (config.refill.initial_wait_interval * 1000) as u128
                );
                assert_le!(finalization_duration_millis, 150u128);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive-call for signal update
        spin_sleeper.sleep(sleep_duration_50_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let mut tx_refill_to_schedule_check_for_test_case_finish =
                    channels.refill.tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler,
                    &mut channels.refill,
                    &mut mock_water_injection,
                    mutex_tank_level_switch_signals,
                    mutex_refill_status,
                    Box::new(sql_interface_refill),
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            })
            .unwrap();

        join_handle_mock_monitors
            .join()
            .expect("Mock monitor check did not finish.");
        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // This test case executes multiple iterations of the .execute function with the error flag error_switch_stuck_low set
    // during the execution of the test case, the error is reset.
    // Refill control shall resume operation upon having reset the error.
    // Test case uses test database #22.
    pub fn test_refill_conditions_historic_data_error_switch_stuck_low() {
        let sleep_duration_50_millis = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill, mut sql_interface, sql_interface_refill) =
            prepare_refill_tests(None, 1, 1, Some(22));

        // set the error
        refill.refill_errors.error_switch_stuck_low = true;

        let mut channels = Channels::new_for_test();

        // test environment-specific channels
        let (
            tx_mock_water_injection_to_test_environment,
            mut rx_test_environment_from_mock_water_injection,
        ) = channel(1);
        let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
            channel::<InternalCommand>(1);

        let refill_status = RefillStatus {
            refill_in_progress_live: false,
            refill_in_progress_for_database: false,
        };
        let mutex_refill_status = Arc::new(Mutex::new(refill_status));

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
        let mutex_tank_level_switch_signals_clone_for_test_environment =
            mutex_tank_level_switch_signals.clone();

        let mut mock_monitors = MockMonitors::new();

        let mut mock_water_injection = MockWaterInjection::new(
            Some(tx_mock_water_injection_to_test_environment),
            RefillErrorStates {
                error_switch_stuck_high: false,
                error_switch_stuck_low: false,
                error_receive_tank_level_switch_position: false,
                error_sql_update_failed: false,
                error_sql_get_historic_data_failed: false,
            },
        );

        // thread for mock monitors
        let join_handle_mock_monitors = thread::Builder::new()
            .name("mock_monitor".to_string())
            .spawn(move || {
                mock_monitors.execute(
                    Some(channels.monitors.rx_monitors_from_refill),
                    None,
                    rx_monitors_from_test_environment,
                );
            })
            .unwrap();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_refill,
                    &mut channels.schedule_check.rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_secs = Duration::from_secs(2);
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                spin_sleeper.sleep(sleep_duration_two_secs);

                for _ in 0..999 {
                    assert!(matches!(
                        rx_test_environment_from_mock_water_injection.try_recv(),
                        Err(AquaChannelError::Empty)
                    ));
                    spin_sleeper.sleep(sleep_duration_two_millis);
                }

                // reset errors
                println!("resetting errors...");
                match channels
                    .messaging
                    .tx_messaging_to_refill
                    .send(InternalCommand::ResetAllErrors)
                {
                    Ok(_) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "test_refill_conditions_historic_data_error_recent_refill_event:\
                        error when trying to reset refill errors from previous test runs: {e:?}"
                        );
                    }
                }

                // wait for the test object to trigger water injection
                println!("waiting for water injection...");
                let _ = rx_test_environment_from_mock_water_injection
                    .recv()
                    .unwrap();

                spin_sleeper.sleep(sleep_duration_two_secs);

                // request update of tank level position signals to mock tank level switch
                println!("updating tank level switch...");
                {
                    let mut tank_level_switch_signals =
                        mutex_tank_level_switch_signals_clone_for_test_environment
                            .lock()
                            .unwrap();
                    tank_level_switch_signals.tank_level_switch_position = true;
                    tank_level_switch_signals.tank_level_switch_position_stabilized = true;
                }

                spin_sleeper.sleep(sleep_duration_two_secs);

                println!("sending quit signal to test object...");
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_refill
                    .recv()
                    .unwrap();
                let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Terminate);

                // wait for the test object to finish so that mock threads receive
                // the termination request before the end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive-call for signal update
        spin_sleeper.sleep(sleep_duration_50_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let mut tx_refill_to_schedule_check_for_test_case_finish =
                    channels.refill.tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler,
                    &mut channels.refill,
                    &mut mock_water_injection,
                    mutex_tank_level_switch_signals,
                    mutex_refill_status,
                    Box::new(sql_interface_refill),
                );

                // check if there was no water injection
                assert_eq!(mock_water_injection.injection_recorder.is_empty(), false);

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            })
            .unwrap();

        join_handle_mock_monitors
            .join()
            .expect("Mock monitor check did not finish.");
        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // This test case executes multiple iterations of the loop inside .execute function with error flag error_sql_update_failed
    // The refill control shall not initiate a refill operation.
    // Test case uses test database #23.
    pub fn test_refill_conditions_error_sql_update_failed() {
        let sleep_duration_50_millis = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill, mut sql_interface, sql_interface_refill) =
            prepare_refill_tests(None, 1, 1, Some(23));

        // set the error
        refill.refill_errors.error_sql_update_failed = true;

        let mut channels = Channels::new_for_test();

        // test environment-specific channels
        let (
            tx_mock_water_injection_to_test_environment,
            mut rx_test_environment_from_mock_water_injection,
        ) = channel(1);
        let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
            channel::<InternalCommand>(1);

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
        let refill_status = RefillStatus {
            refill_in_progress_live: false,
            refill_in_progress_for_database: false,
        };
        let mutex_refill_status = Arc::new(Mutex::new(refill_status));

        let mut mock_monitors = MockMonitors::new();

        let mut mock_water_injection = MockWaterInjection::new(
            Some(tx_mock_water_injection_to_test_environment),
            RefillErrorStates {
                error_switch_stuck_high: false,
                error_switch_stuck_low: false,
                error_receive_tank_level_switch_position: false,
                error_sql_update_failed: false,
                error_sql_get_historic_data_failed: false,
            },
        );

        // thread for mock monitors
        let join_handle_mock_monitors = thread::Builder::new()
            .name("mock_monitor".to_string())
            .spawn(move || {
                mock_monitors.execute(
                    Some(channels.monitors.rx_monitors_from_refill),
                    None,
                    rx_monitors_from_test_environment,
                );
            })
            .unwrap();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_refill,
                    &mut channels.schedule_check.rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        // thread for mock Test environment
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_secs = Duration::from_secs(2);
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                spin_sleeper.sleep(sleep_duration_two_secs);

                for _ in 0..999 {
                    assert!(matches!(
                        rx_test_environment_from_mock_water_injection.try_recv(),
                        Err(AquaChannelError::Empty)
                    ));
                    spin_sleeper.sleep(sleep_duration_two_millis);
                }

                println!("sending quit signal to test object...");
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_refill
                    .recv()
                    .unwrap();
                let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Terminate);

                // wait for the test object to finish so that mock threads receive
                // the termination request before the end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive-call for signal update
        spin_sleeper.sleep(sleep_duration_50_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                let mut tx_refill_to_schedule_check_for_test_case_finish =
                    channels.refill.tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler,
                    &mut channels.refill,
                    &mut mock_water_injection,
                    mutex_tank_level_switch_signals,
                    mutex_refill_status,
                    Box::new(sql_interface_refill),
                );

                // check if there was no water injection
                assert_eq!(mock_water_injection.injection_recorder.is_empty(), true);

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

                spin_sleeper.sleep(sleep_duration_two_millis);
            })
            .unwrap();

        join_handle_mock_monitors
            .join()
            .expect("Mock monitor check did not finish.");
        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // This test case checks the process_external_request implementation if it correctly
    // processes the ResetAllErrors command.
    // The test case checks the struct attribute after having executed the function.
    // Test case uses test database #24.
    pub fn test_refill_process_external_request_reset_all_errors_command() {
        // set the errors
        let refill_errors = RefillErrorStates {
            error_switch_stuck_high: true,
            error_switch_stuck_low: true,
            error_receive_tank_level_switch_position: true,
            error_sql_update_failed: true,
            error_sql_get_historic_data_failed: true,
        };

        let (_, mut refill, mut sql_interface, sql_interface_refill) =
            prepare_refill_tests(Some(refill_errors), 2, 60, Some(24));

        let sleep_duration_50_millis = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();

        let mut channels = Channels::new_for_test();

        let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
            channel::<InternalCommand>(1);
        let (
            tx_mock_water_injection_to_test_environment,
            _rx_test_environment_from_mock_water_injection,
        ) = channel(1);

        let mut mock_monitors = MockMonitors::new();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
        let refill_status = RefillStatus {
            refill_in_progress_live: false,
            refill_in_progress_for_database: false,
        };
        let mutex_refill_status = Arc::new(Mutex::new(refill_status));

        let mut mock_water_injection = MockWaterInjection::new(
            Some(tx_mock_water_injection_to_test_environment),
            RefillErrorStates {
                error_switch_stuck_high: false,
                error_switch_stuck_low: false,
                error_receive_tank_level_switch_position: false,
                error_sql_update_failed: false,
                error_sql_get_historic_data_failed: false,
            },
        );

        // thread for mock monitors
        let join_handle_mock_monitors = thread::Builder::new()
            .name("mock_monitor".to_string())
            .spawn(move || {
                mock_monitors.execute(
                    Some(channels.monitors.rx_monitors_from_refill),
                    None,
                    rx_monitors_from_test_environment,
                );
            })
            .unwrap();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_refill,
                    &mut channels.schedule_check.rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_secs = Duration::from_secs(2);
                let spin_sleeper = SpinSleeper::default();

                spin_sleeper.sleep(sleep_duration_two_secs);
                let result = channels
                    .messaging
                    .tx_messaging_to_refill
                    .send(InternalCommand::ResetAllErrors);
                assert!(result.is_ok());
                spin_sleeper.sleep(sleep_duration_two_secs);

                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_refill
                    .recv()
                    .unwrap();
                let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Terminate);

                // wait for the test object to finish so that mock threads receive
                // the termination request before the end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive-call for signal update
        spin_sleeper.sleep(sleep_duration_50_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                // check if errors have been set
                assert_eq!(refill.refill_errors.has_error(), true);

                let mut tx_refill_to_schedule_check_for_test_case_finish =
                    channels.refill.tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler,
                    &mut channels.refill,
                    &mut mock_water_injection,
                    mutex_tank_level_switch_signals,
                    mutex_refill_status,
                    Box::new(sql_interface_refill),
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

                println!("{}", refill.refill_errors);

                // check if errors have been reset
                assert_eq!(refill.refill_errors.has_error(), false);

                spin_sleeper.sleep(sleep_duration_two_millis);
            })
            .unwrap();

        join_handle_mock_monitors
            .join()
            .expect("Mock monitor check did not finish.");
        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Executes loop of the .execute function manipulating the sensor signals only and giving a negative schedule check.
    // Checks if the test object correctly processes the negative schedule check result.
    // Test case uses test database #25.
    pub fn test_refill_blocked_by_schedule() {
        let sleep_duration_50_millis = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill, mut sql_interface, sql_interface_refill) =
            prepare_refill_tests(None, 1, 1, Some(25));

        let mut channels = Channels::new_for_test();

        let (
            tx_mock_water_injection_to_test_environment,
            mut rx_test_environment_from_mock_water_injection,
        ) = channel(1);
        let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
            channel::<InternalCommand>(1);

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, "refill_log".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
        let refill_status = RefillStatus {
            refill_in_progress_live: false,
            refill_in_progress_for_database: false,
        };
        let mutex_refill_status = Arc::new(Mutex::new(refill_status));

        let mut mock_monitors = MockMonitors::new();

        let mut mock_water_injection = MockWaterInjection::new(
            Some(tx_mock_water_injection_to_test_environment),
            RefillErrorStates {
                error_switch_stuck_high: false,
                error_switch_stuck_low: false,
                error_receive_tank_level_switch_position: false,
                error_sql_update_failed: false,
                error_sql_get_historic_data_failed: false,
            },
        );

        // thread for mock monitors
        let join_handle_mock_monitors = thread::Builder::new()
            .name("mock_monitor".to_string())
            .spawn(move || {
                mock_monitors.execute(
                    Some(channels.monitors.rx_monitors_from_refill),
                    None,
                    rx_monitors_from_test_environment,
                );
            })
            .unwrap();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_refill,
                    &mut channels.schedule_check.rx_schedule_check_from_refill,
                    None,
                    false,
                );
            })
            .unwrap();

        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_secs = Duration::from_secs(2);
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                spin_sleeper.sleep(sleep_duration_two_secs);

                for _ in 0..999 {
                    assert!(matches!(
                        rx_test_environment_from_mock_water_injection.try_recv(),
                        Err(AquaChannelError::Empty)
                    ));
                    spin_sleeper.sleep(sleep_duration_two_millis);
                }

                println!("sending quit signal to test object...");
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_refill
                    .recv()
                    .unwrap();
                let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Terminate);

                // wait for the test object to finish so that mock threads receive
                // the termination request before the end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive-call for signal update
        spin_sleeper.sleep(sleep_duration_50_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                let mut tx_refill_to_schedule_check_for_test_case_finish =
                    channels.refill.tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler,
                    &mut channels.refill,
                    &mut mock_water_injection,
                    mutex_tank_level_switch_signals,
                    mutex_refill_status,
                    Box::new(sql_interface_refill),
                );

                // check if there was no water injection
                assert_eq!(mock_water_injection.injection_recorder.is_empty(), true);

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

                spin_sleeper.sleep(sleep_duration_two_millis);
            })
            .unwrap();

        join_handle_mock_monitors
            .join()
            .expect("Mock monitor check did not finish.");
        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case executes the loop of the .execute function manipulating the sensor signals only
    // and giving a positive schedule check.
    // Test case checks if the test object correctly processes the inhibition requested via the channel.
    // Test case uses test database #26.
    pub fn test_messaging_stops_starts_refill() {
        let sleep_duration_50_millis = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill, mut sql_interface, sql_interface_refill) =
            prepare_refill_tests(None, 10, 60, Some(26));

        let mut channels = Channels::new_for_test();

        let (
            tx_mock_water_injection_to_test_environment,
            mut rx_test_environment_from_mock_water_injection,
        ) = channel(1);
        let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
            channel::<InternalCommand>(1);

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mut mock_monitors = MockMonitors::new();

        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
        let mutex_tank_level_switch_signals_clone_for_test_environment =
            mutex_tank_level_switch_signals.clone();
        let refill_status = RefillStatus {
            refill_in_progress_live: false,
            refill_in_progress_for_database: false,
        };
        let mutex_refill_status = Arc::new(Mutex::new(refill_status));

        let mut mock_water_injection = MockWaterInjection::new(
            Some(tx_mock_water_injection_to_test_environment),
            RefillErrorStates {
                error_switch_stuck_high: false,
                error_switch_stuck_low: false,
                error_receive_tank_level_switch_position: false,
                error_sql_update_failed: false,
                error_sql_get_historic_data_failed: false,
            },
        );

        // thread for mock monitors
        let join_handle_mock_monitors = thread::Builder::new()
            .name("mock_monitor".to_string())
            .spawn(move || {
                mock_monitors.execute(
                    Some(channels.monitors.rx_monitors_from_refill),
                    None,
                    rx_monitors_from_test_environment,
                );
            })
            .unwrap();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_refill,
                    &mut channels.schedule_check.rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_one_sec = Duration::from_secs(1);
                let sleep_duration_two_secs = Duration::from_secs(2);
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                spin_sleeper.sleep(sleep_duration_one_sec);

                match channels
                    .messaging
                    .tx_messaging_to_refill
                    .send(InternalCommand::Stop)
                {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: could not send stop command to refill ({e:?})",
                            module_path!()
                        );
                    }
                }

                spin_sleeper.sleep(sleep_duration_one_sec);

                {
                    let mut tank_level_switch_signals =
                        mutex_tank_level_switch_signals_clone_for_test_environment
                            .lock()
                            .unwrap();
                    tank_level_switch_signals.tank_level_switch_position = false;
                    tank_level_switch_signals.tank_level_switch_position_stabilized = false;
                }

                // expect to see no event
                for _ in 0..999 {
                    assert!(matches!(
                        rx_test_environment_from_mock_water_injection.try_recv(),
                        Err(AquaChannelError::Empty)
                    ));
                    spin_sleeper.sleep(sleep_duration_two_millis);
                }

                spin_sleeper.sleep(sleep_duration_one_sec);

                match channels
                    .messaging
                    .tx_messaging_to_refill
                    .send(InternalCommand::Start)
                {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: could not send start command to refill ({e:?})",
                            module_path!()
                        );
                    }
                }

                // expect to see one event
                assert!(matches!(
                    rx_test_environment_from_mock_water_injection.recv(),
                    Ok(TestCommand::InformWaterInjectionEvent)
                ));

                println!("sending quit signal to test object...");
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_refill
                    .recv()
                    .unwrap();
                let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Terminate);

                // wait for the test object to finish so that mock threads receive
                // the termination request before the end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive-call for signal update
        spin_sleeper.sleep(sleep_duration_50_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                let mut tx_refill_to_schedule_check_for_test_case_finish =
                    channels.refill.tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler,
                    &mut channels.refill,
                    &mut mock_water_injection,
                    mutex_tank_level_switch_signals,
                    mutex_refill_status,
                    Box::new(sql_interface_refill),
                );

                // check if there was only one water injection
                assert_eq!(mock_water_injection.injection_recorder.len(), 1);

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

                spin_sleeper.sleep(sleep_duration_two_millis);
            })
            .unwrap();

        join_handle_mock_monitors
            .join()
            .expect("Mock monitor check did not finish.");
        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // This test case executes multiple iterations of the loop inside .execute function
    // using a mock version of SQL interface which returns only errors.
    // The refill control shall not initiate a refill operation.
    // Test case uses test database #27.
    pub fn test_refill_conditions_error_get_historic_data_failed() {
        let sleep_duration_50_millis = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill, sql_interface_refill) =
            prepare_refill_tests_with_mock_database_interface(None, 1, 1, 27);
        // this test case does not use the SQL database, so no conflict arises
        // when concurrently executing other test cases on the same DB.

        let mut channels = Channels::new_for_test();

        let (
            tx_mock_water_injection_to_test_environment,
            mut rx_test_environment_from_mock_water_injection,
        ) = channel(1);
        let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
            channel::<InternalCommand>(1);

        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
        let refill_status = RefillStatus {
            refill_in_progress_live: false,
            refill_in_progress_for_database: false,
        };
        let mutex_refill_status = Arc::new(Mutex::new(refill_status));

        let mut mock_monitors = MockMonitors::new();

        let mut mock_water_injection = MockWaterInjection::new(
            Some(tx_mock_water_injection_to_test_environment),
            RefillErrorStates {
                error_switch_stuck_high: false,
                error_switch_stuck_low: false,
                error_receive_tank_level_switch_position: false,
                error_sql_update_failed: false,
                error_sql_get_historic_data_failed: false,
            },
        );

        // thread for mock monitors
        let join_handle_mock_monitors = thread::Builder::new()
            .name("mock_monitor".to_string())
            .spawn(move || {
                mock_monitors.execute(
                    Some(channels.monitors.rx_monitors_from_refill),
                    None,
                    rx_monitors_from_test_environment,
                );
            })
            .unwrap();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_refill,
                    &mut channels.schedule_check.rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        // thread for mock Test environment
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_secs = Duration::from_secs(2);
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                spin_sleeper.sleep(sleep_duration_two_secs);

                for _ in 0..999 {
                    assert!(matches!(
                        rx_test_environment_from_mock_water_injection.try_recv(),
                        Err(AquaChannelError::Empty)
                    ));
                    spin_sleeper.sleep(sleep_duration_two_millis);
                }

                println!("sending quit signal to test object...");
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_refill
                    .recv()
                    .unwrap();
                let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Terminate);

                // wait for the test object to finish so that mock threads receive
                // the termination request before the end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive-call for signal update
        spin_sleeper.sleep(sleep_duration_50_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                let mut tx_refill_to_schedule_check_for_test_case_finish =
                    channels.refill.tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler,
                    &mut channels.refill,
                    &mut mock_water_injection,
                    mutex_tank_level_switch_signals,
                    mutex_refill_status,
                    Box::new(sql_interface_refill),
                );

                // check if there was no water injection
                assert_eq!(mock_water_injection.injection_recorder.is_empty(), true);

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

                spin_sleeper.sleep(sleep_duration_two_millis);
            })
            .unwrap();

        join_handle_mock_monitors
            .join()
            .expect("Mock monitor check did not finish.");
        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // This test case executes the check_volume_count_time_limits function of refill.
    // It also instantiates Monitor to capture the monitor view sent via the channel.
    pub fn test_refill_check_volume_count_time_limits() {
        let mut mock_monitors = MockMonitors::new();
        let sleep_duration_200_millis = Duration::from_millis(200);
        let spin_sleeper = SpinSleeper::default();

        let mut channels = Channels::new_for_test();

        let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
            channel::<InternalCommand>(1);

        // thread for mock monitors
        let join_handle_mock_monitors = thread::Builder::new()
            .name("mock_monitor".to_string())
            .spawn(move || {
                mock_monitors.execute(
                    Some(channels.monitors.rx_monitors_from_refill),
                    None,
                    rx_monitors_from_test_environment,
                );
                println!("{}", mock_monitors);
                let (_, refill_monitor_view) = mock_monitors.refill_monitor_views.pop().unwrap();
                refill_monitor_view.assert_refill_monitor_view(false, false, false, false, false);

                let (_, refill_monitor_view) = mock_monitors.refill_monitor_views.pop().unwrap();
                refill_monitor_view.assert_refill_monitor_view(false, false, false, false, true);

                let (_, refill_monitor_view) = mock_monitors.refill_monitor_views.pop().unwrap();
                refill_monitor_view.assert_refill_monitor_view(false, false, true, false, false);

                let (_, refill_monitor_view) = mock_monitors.refill_monitor_views.pop().unwrap();
                refill_monitor_view.assert_refill_monitor_view(true, false, false, false, false);

                let (_, refill_monitor_view) = mock_monitors.refill_monitor_views.pop().unwrap();
                refill_monitor_view.assert_refill_monitor_view(false, false, false, true, false);

                let (_, refill_monitor_view) = mock_monitors.refill_monitor_views.pop().unwrap();
                refill_monitor_view.assert_refill_monitor_view(false, true, false, false, false);

                let (_, refill_monitor_view) = mock_monitors.refill_monitor_views.pop().unwrap();
                refill_monitor_view.assert_refill_monitor_view(false, false, false, false, false);
            })
            .unwrap();

        let (config, mut refill, _sql_interface_refill) =
            prepare_refill_tests_with_mock_database_interface(None, 1, 1, 27);
        // this test case does not use the SQL database, so no conflict arises
        // when concurrently executing other test cases on the same DB.

        let mut sql_refill_stats_data = RefillStatsData::default();
        sql_refill_stats_data.duration_since_last_refill =
            config.refill.min_interval_last_refill + 1;

        // first execution without any errors
        assert_eq!(
            refill.check_volume_count_time_limits(&sql_refill_stats_data, &mut channels.refill),
            false
        );
        spin_sleeper.sleep(sleep_duration_200_millis);

        // induce errors before executing again
        sql_refill_stats_data.refill_count_last_24h = config.refill.max_refill_count_24h + 1;

        assert_eq!(
            refill.check_volume_count_time_limits(&sql_refill_stats_data, &mut channels.refill),
            true
        );
        spin_sleeper.sleep(sleep_duration_200_millis);

        // reset the previous error and induce other error before executing again
        sql_refill_stats_data.refill_count_last_24h = 0;
        sql_refill_stats_data.refill_count_last_hour = config.refill.max_refill_count_hour + 1;

        assert_eq!(
            refill.check_volume_count_time_limits(&sql_refill_stats_data, &mut channels.refill),
            true
        );
        spin_sleeper.sleep(sleep_duration_200_millis);

        // reset the previous error and induce other error before executing again
        sql_refill_stats_data.refill_count_last_hour = 0;
        sql_refill_stats_data.refill_volume_last_24h = config.refill.max_refill_volume_24h + 1.0;

        assert_eq!(
            refill.check_volume_count_time_limits(&sql_refill_stats_data, &mut channels.refill),
            true
        );
        spin_sleeper.sleep(sleep_duration_200_millis);

        // reset the previous error and induce other error before executing again
        sql_refill_stats_data.refill_volume_last_24h = 0.0;
        sql_refill_stats_data.refill_volume_last_hour = config.refill.max_refill_volume_hour + 1.0;

        assert_eq!(
            refill.check_volume_count_time_limits(&sql_refill_stats_data, &mut channels.refill),
            true
        );
        spin_sleeper.sleep(sleep_duration_200_millis);

        // reset the previous error and induce other error before executing again
        sql_refill_stats_data.refill_volume_last_hour = 0.0;
        sql_refill_stats_data.duration_since_last_refill =
            config.refill.min_interval_last_refill - 1;

        assert_eq!(
            refill.check_volume_count_time_limits(&sql_refill_stats_data, &mut channels.refill),
            true
        );
        spin_sleeper.sleep(sleep_duration_200_millis);

        // reset the previous error and do not induce any further error
        sql_refill_stats_data.duration_since_last_refill =
            config.refill.min_interval_last_refill + 1;
        assert_eq!(
            refill.check_volume_count_time_limits(&sql_refill_stats_data, &mut channels.refill),
            false
        );
        spin_sleeper.sleep(sleep_duration_200_millis);

        let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);
        // give thread time to finish
        spin_sleeper.sleep(sleep_duration_200_millis);

        join_handle_mock_monitors
            .join()
            .expect("Mock monitor check did not finish.");
    }

    #[test]
    // Tests the `read_refill_stats` helper function for both success and failure cases.
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
            #[cfg(not(test))]
            "dummy_value_type_not_used",
            #[cfg(not(test))]
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
            #[cfg(not(test))]
            "dummy_value_type_not_used",
            #[cfg(not(test))]
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
            #[cfg(not(test))]
            "dummy_value_type_not_used",
            #[cfg(not(test))]
            "dummy_duration_not_used",
        );

        assert!(error_occurred_fail, "Should return true on failure");
        assert_eq!(
            stats_value_fail, 999,
            "stats_value should NOT be changed on failure"
        );
        // Because of #[cfg(not(test))], the logging and lock flag modification
        // are compiled out in test builds. We verify the flag is unchanged.
        assert!(
            !lock_flag_fail,
            "lock_flag should NOT be changed on failure in test builds"
        );
    }

    #[test]
    // Tests the `check_volume_count_time_limit` helper function.
    fn test_check_volume_count_time_limit() {
        // === Test Case 1: Value is below the limit (u64) ===
        // Should return false, indicating no limit was exceeded.
        // The lock flag should not be changed.
        let mut lock_flag = false;
        let result = Refill::check_volume_count_time_limit(
            50u64,
            100u64,
            &mut lock_flag,
            #[cfg(not(test))]
            "dummy_value_type_not_used",
            #[cfg(not(test))]
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
            #[cfg(not(test))]
            "dummy_value_type_not_used",
            #[cfg(not(test))]
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
            #[cfg(not(test))]
            "dummy_value_type_not_used",
            #[cfg(not(test))]
            "dummy_duration_not_used",
        );
        assert!(result, "Should return true when value exceeds limit");
        assert!(
            lock_flag,
            "Lock flag should be set to true when limit is exceeded"
        );

        // === Test Case 4: Value exceeds the limit again (the lock flag is already set) ===
        // Should still return true. The logging logic (which is compiled out in tests)
        // would be skipped. The lock flag should remain true.
        let mut lock_flag = true;
        let result = Refill::check_volume_count_time_limit(
            102u64,
            100u64,
            &mut lock_flag,
            #[cfg(not(test))]
            "dummy_value_type_not_used",
            #[cfg(not(test))]
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
            #[cfg(not(test))]
            "dummy_value_type_not_used",
            #[cfg(not(test))]
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
            #[cfg(not(test))]
            "dummy_value_type_not_used",
            #[cfg(not(test))]
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
            #[cfg(not(test))]
            "dummy_value_type_not_used",
            #[cfg(not(test))]
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
}
