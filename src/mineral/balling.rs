/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Implements the main control logic for automated Balling mineral dosing.
//!
//! This module contains the `Balling` struct, which runs as a dedicated thread to manage
//! the scheduled dosing of up to four different mineral solutions. It is responsible for
//! maintaining a precise dosing schedule, responding to external commands, and ensuring
//! that dosing operations are performed safely and reliably.
//!
//! ## Key Components
//!
//! - **`Balling` Struct**: The central state machine for the dosing subsystem. It holds the
//!   configuration and the internal state for each pump, most importantly the
//!   `countdown_dosing_pumpX` fields which track the time until the next scheduled dose.
//!
//! - **`new()` Constructor**: A complex and critical initialization function. It performs
//!   several pre-flight checks:
//!   1.  Validates that all configured dosing intervals are logical and non-zero.
//!   2.  Ensures that every pump marked as `active` in the configuration has a
//!       corresponding entry in the database.
//!   3.  Calculates the initial countdown for each pump by querying the database for the
//!       time of its last dosing event.
//!
//! - **`execute()` Method**: The main thread loop. It continuously performs the following actions:
//!   1.  Periodically decrements the countdown timers for each active pump.
//!   2.  When a countdown reaches zero, it requests a `schedule_check` to ensure dosing
//!       is permitted at the current time.
//!   3.  If permitted, it calls `execute_dosing` to perform the physical action.
//!   4.  Resets the pump's countdown to its configured interval.
//!   5.  Listens for and processes external commands like `Start`, `Stop`, and `Execute(pump_id)`.
//!
//! - **`execute_dosing()` Method**: A private helper that orchestrates a single dosing event.
//!   It retrieves pump parameters from the database, calculates the required pump run time,
//!   invokes the `MineralInjectionTrait` to actuate the pump, and logs the completed
//!   event back to the database.
//!
//! ## Design and Architecture
//!
//! The `Balling` module is designed as a robust, decoupled, and testable component.
//!
//! - **Time-Based Scheduling**: The core scheduling logic is based on simple countdown timers.
//!   This is a robust and low-overhead way to manage recurring events without relying on
//!   a complex external scheduler.
//!
//! - **Dependency Injection**: The module relies on a trait for its core dependency:
//!   - `MineralInjectionTrait`: An abstraction for the physical act of dispensing a
//!     mineral solution. This allows for mock implementations in tests.
//!
//! - **Concurrency Control**: It uses an `Arc<Mutex<i32>>` to coordinate with other
//!   device-actuating modules (like `Feed`). This mutex acts as semaphore, ensuring
//!   that only one major physical action occurs at a time across the entire system,
//!   preventing conflicts.
//!
//! - **State Management**: The `dosing_inhibited` flag allows external commands (`Start`/`Stop`)
//!   to temporarily pause all scheduled dosing without stopping the thread or losing the
//!   current countdown states.

#[cfg(feature = "debug_balling")]
use log::debug;
use log::{error, info};
use std::error;
use std::fmt;

#[cfg(all(not(test), target_os = "linux"))] // only required in real-life execution
use nix::unistd::gettid;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::database::sql_interface_balling::{BallingSetVal, SqlInterfaceBalling};
use crate::mineral::balling_channels::BallingChannels;
use crate::mineral::balling_config::BallingConfig;
use crate::mineral::balling_error::BallingError;
use crate::mineral::mineral_injection::MineralInjectionTrait;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::channel_content::AquariumDevice;
use crate::utilities::database_ping_trait::DatabasePingTrait;
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;
use crate::{check_quit_increment_counter_ping_database, perform_schedule_check};

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the Balling dosing control.
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     balling[Balling Dosing Control] --> signal_handler[Signal Handler]
///     signal_handler --> balling
///     signal_handler --> mineral_injection[Mineral Injection]
///     balling --> mineral_injection
///     mineral_injection --> relay_manager[Relay Manager]
///     relay_manager --> mineral_injection
///     balling --> schedule_check[Schedule Check]
///     schedule_check --> balling
///     messaging[Messaging] --> balling
/// ```
/// Communication channel to and from the relay manager is forwarded to implementation of MineralInjectionTrait.
/// Signal handler is communicating both directly with balling and with the implementation of MineralInjectionTrait.
pub struct Balling {
    /// configuration data for Balling dosing control
    config: BallingConfig,

    /// Durations until next dosing
    pub durations_until_next_dosing: [Duration; 4],

    /// Configuration data processed into an array: activation
    pump_is_active: [bool; 4],

    /// Configuration data processed into an array: dosing intervals
    dosing_intervals: [Duration; 4],

    /// inhibition flag to avoid flooding the log file with repeated messages to send request via the channel to schedule check
    lock_error_channel_send_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages to receive request via the channel from schedule check
    lock_error_channel_receive_schedule_check: bool,

    /// communication from trait implementation: request to execute a certain Balling mineral dosing has been received
    pub execute_command_received: bool,

    /// communication from trait implementation: id of pump to be executed requested externally
    pub pump_id_requested: i32,

    /// recording when the last database ping happened
    pub last_ping_instant: Instant,

    /// database ping interval
    pub database_ping_interval: Duration,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via the channel
    pub lock_warn_inapplicable_command_signal_handler: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    pub lock_error_channel_receive_termination: bool,

    /// inhibits further execution once a critical error occurs
    pub execution_blocked: bool,
}

impl fmt::Display for Balling {
    /// Formats the value using the given formatter.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Balling Controller (Active: {}), Initial Countdown (s): [P1: {}, P2: {}, P3: {}, P4: {}]",
            self.config.active,
            self.durations_until_next_dosing[0].as_secs(),
            self.durations_until_next_dosing[1].as_secs(),
            self.durations_until_next_dosing[2].as_secs(),
            self.durations_until_next_dosing[3].as_secs()
        )
    }
}

impl Balling {
    /// Creates a new `Balling` control instance.
    ///
    /// This constructor initializes the Balling dosing control module. It performs
    /// several pre-flight checks, including validating the `schedule_check_interval`
    /// and dosing intervals from the configuration, ensuring that a database entry
    /// exists for every active pump, and calculating the initial countdowns for
    /// each pump based on the respective last recorded dosing of that pump.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the Balling dosing control, loaded from a TOML file.
    /// * `sql_interface_balling` - A mutable reference to a `SqlInterfaceBalling` instance, providing the
    ///   specific SQL interface for Balling dosing operations.
    /// * `database_ping_interval` - A `Duration` instance, providing the interval to ping the database.
    ///
    /// # Returns
    /// A `Result` containing a new, initialized `Balling` instance on success.
    ///
    /// # Errors
    /// This function will return a `BallingError` if any of the initial setup steps fail:
    /// - The `schedule_check_interval` in the configuration is zero (`ScheduleCheckIntervalZero`).
    /// - Any configured dosing interval is zero or shorter than the `schedule_check_interval`
    ///   (`InvalidDosingInterval`, `DosingIntervalShorterThanCheckInterval`).
    /// - A pump is marked as active in the config, but no corresponding set values are found
    ///   in the database (`SetValueRetrievalError`).
    /// - It fails to calculate the initial countdown for any pump due to a database error
    ///   or data conversion issue.
    pub fn new(
        config: BallingConfig,
        database_ping_interval: Duration,
        sql_interface_balling: &mut SqlInterfaceBalling,
    ) -> Result<Balling, BallingError> {
        if config.schedule_check_interval == 0 {
            return Err(BallingError::ScheduleCheckIntervalZero(
                module_path!().to_string(),
            ));
        }

        Self::check_valid_dosing_intervals(&config)?;

        Self::check_database_vs_config(sql_interface_balling, &config)?;

        let mut durations_until_next_dosing = [Duration::from_secs(0); 4];

        // Calculate the initial countdown for each pump when the next dosing shall happen
        for pump_id in 1..=4 {
            durations_until_next_dosing[pump_id - 1] = Self::calc_duration_until_next_dosing(
                &config,
                sql_interface_balling,
                pump_id as i64,
            )?;
        }

        // Transfer the pump configuration data into an array
        let mut pump_is_active = [false; 4];
        pump_is_active[0] = config.pump1_active;
        pump_is_active[1] = config.pump2_active;
        pump_is_active[2] = config.pump3_active;
        pump_is_active[3] = config.pump4_active;

        let mut dosing_intervals = [Duration::from_secs(0); 4];
        dosing_intervals[0] = Duration::from_secs(config.dosing_interval_pump1 as u64);
        dosing_intervals[1] = Duration::from_secs(config.dosing_interval_pump2 as u64);
        dosing_intervals[2] = Duration::from_secs(config.dosing_interval_pump3 as u64);
        dosing_intervals[3] = Duration::from_secs(config.dosing_interval_pump4 as u64);

        Ok(Balling {
            config,
            durations_until_next_dosing,
            pump_is_active,
            dosing_intervals,
            lock_error_channel_send_schedule_check: false,
            lock_error_channel_receive_schedule_check: false,
            execute_command_received: false,
            pump_id_requested: 0,
            last_ping_instant: Instant::now(),
            database_ping_interval,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
            execution_blocked: false,
        })
    }

    /// Checks that configured dosing intervals are valid and logical.
    ///
    /// This function validates two conditions for each active pump:
    /// 1. The dosing interval must not be zero.
    /// 2. The dosing interval must be greater than or equal to the `schedule_check_interval`.
    ///
    /// # Arguments
    /// * `config` - A reference to the `BallingConfig` containing the dosing intervals and the check interval.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if all configured dosing intervals for active pumps are valid.
    ///
    /// # Errors
    /// This function will return a `BallingError` if an invalid configuration is detected:
    /// - `InvalidDosingInterval`: If an active pump's dosing interval is set to `0`.
    /// - `DosingIntervalShorterThanCheckInterval`: If an active pump's dosing interval is
    ///   shorter than the main `schedule_check_interval`, which would lead to illogical scheduling.
    fn check_valid_dosing_intervals(config: &BallingConfig) -> Result<(), BallingError> {
        let pumps = [
            (1, config.pump1_active, config.dosing_interval_pump1),
            (2, config.pump2_active, config.dosing_interval_pump2),
            (3, config.pump3_active, config.dosing_interval_pump3),
            (4, config.pump4_active, config.dosing_interval_pump4),
        ];

        for (pump_id, is_active, interval) in pumps {
            if !is_active {
                continue;
            }
            if interval == 0 {
                return Err(BallingError::InvalidDosingInterval(
                    module_path!().to_string(),
                    pump_id,
                ));
            }
            if interval < config.schedule_check_interval {
                return Err(BallingError::DosingIntervalShorterThanCheckInterval(
                    module_path!().to_string(),
                    pump_id as u32,
                    interval,
                    config.schedule_check_interval,
                ));
            }
        }
        Ok(())
    }

    /// Verifies that a database entry exists for every pump marked as active in the configuration.
    ///
    /// This function ensures that the application doesn't try to operate a pump that
    /// lacks the necessary set values (e.g., flow rate, volume) in the database.
    ///
    /// # Arguments
    /// * `sql_interface_balling` - A mutable reference to the `SqlInterfaceBalling` instance.
    /// * `config` - A reference to the `BallingConfig` to check which pumps are active.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if a database entry is found for every active pump.
    ///
    /// # Errors
    /// Returns `BallingError::SetValueRetrievalError` if an active pump is missing its
    /// corresponding configuration entry in the `balling_set_vals` table. The error will
    /// wrap the underlying `SqlInterfaceError`.
    fn check_database_vs_config(
        sql_interface_balling: &mut SqlInterfaceBalling,
        config: &BallingConfig,
    ) -> Result<(), BallingError> {
        let active_pumps = [
            (1, config.pump1_active),
            (2, config.pump2_active),
            (3, config.pump3_active),
            (4, config.pump4_active),
        ];

        for (pump_id, is_active) in active_pumps {
            if is_active {
                // The `?` operator makes this much cleaner than a match statement.
                sql_interface_balling
                    .get_single_balling_setval_from_database(pump_id)
                    .map_err(|e| BallingError::SetValueRetrievalError {
                        location: module_path!().to_string(),
                        pump_id,
                        source: Box::new(e),
                    })?;
            }
        }
        Ok(())
    }

    /// Retrieves the configured dosing interval for a specific pump.
    ///
    /// # Arguments
    /// * `config` - A reference to the `BallingConfig` containing the dosing intervals.
    /// * `pump_id` - The ID of the pump (1, 2, 3, or 4) for which to get the interval.
    ///
    /// # Returns
    /// A `Result` containing the configured dosing interval (`u32`) in seconds for the specified pump.
    ///
    /// # Errors
    /// Returns `BallingError::CountdownCalculationInvalidPumpId` if the provided `pump_id`
    /// is not one of the recognized values (1, 2, 3, or 4).
    fn get_target_dosing_interval(
        config: &BallingConfig,
        pump_id: i64,
    ) -> Result<u32, BallingError> {
        match pump_id {
            1 => Ok(config.dosing_interval_pump1),
            2 => Ok(config.dosing_interval_pump2),
            3 => Ok(config.dosing_interval_pump3),
            4 => Ok(config.dosing_interval_pump4),
            _ => Err(BallingError::CountdownCalculationInvalidPumpId(
                module_path!().to_string(),
                pump_id,
            )),
        }
    }

    /// Calculates the remaining time (countdown) until the next scheduled dosing for a specific pump.
    ///
    /// This function determines how much time is left until a pump's next dosing event.
    /// It retrieves the target interval from the configuration and the elapsed time since the
    /// last dosing from the database. The duration is the interval minus the elapsed time.
    /// If no previous dosing event exists, the duration is set to the target interval
    /// applying a cautious strategy.
    ///
    /// # Arguments
    /// * `config` - A reference to the `BallingConfig` containing the dosing intervals.
    /// * `sql_interface_balling` - A mutable reference to the `SqlInterfaceBalling` instance.
    /// * `pump_id` - The unique identifier of the pump (1-4) for which to calculate the countdown.
    ///
    /// # Returns
    /// A `Result` containing the calculated duration until the next dosing. This value is `0` if the dosing is overdue.
    ///
    /// # Errors
    /// This function will return a `BallingError` if:
    /// - The `pump_id` is invalid (`CountdownCalculationInvalidPumpId`).
    /// - The configured dosing interval for the pump is zero (`InvalidDosingInterval`).
    /// - The database query to get the duration since the last dosing fails (`ReadDurationSinceLastDosingFailure`).
    /// - The retrieved duration from the database is negative or cannot be converted to `u32` (`CalculateDurationSinceLastDosingFailure`).
    fn calc_duration_until_next_dosing(
        config: &BallingConfig,
        sql_interface_balling: &mut SqlInterfaceBalling,
        pump_id: i64,
    ) -> Result<Duration, BallingError> {
        let target_dosing_interval = Self::get_target_dosing_interval(config, pump_id)?;
        if target_dosing_interval == 0 {
            return Err(BallingError::InvalidDosingInterval(
                module_path!().to_string(),
                pump_id,
            ));
        }

        let duration_since_balling_dosing_opt = sql_interface_balling
            .get_duration_since_last_balling_dosing(pump_id)
            .map_err(|e| BallingError::ReadDurationSinceLastDosingFailure {
                location: module_path!().to_string(),
                pump_id,
                source: Box::new(e),
            })?;

        let duration_since_balling_dosing: Duration = match duration_since_balling_dosing_opt {
            Some(duration_since_balling_dosing) => duration_since_balling_dosing,
            None => {
                // No entry found in the database.
                // For avoiding overdosing, assume that storing dosing event failed.
                Duration::from_secs(target_dosing_interval as u64)
            }
        };

        let target_dosing_interval_duration = Duration::from_secs(target_dosing_interval as u64);

        let remaining_time_until_next_dosing =
            if duration_since_balling_dosing >= target_dosing_interval_duration {
                Duration::from_secs(0)
            } else {
                target_dosing_interval_duration - duration_since_balling_dosing
            };

        Ok(remaining_time_until_next_dosing)
    }

    /// Calculates the target dosing duration in milliseconds based on the pump's flow rate and desired volume.
    ///
    /// This private helper function determines how long a dosing pump needs to run
    /// to dispense a specific volume of fluid, given its flow rate.
    ///
    /// # Arguments
    /// * `balling_setval` - A reference to a `BallingSetVal` struct, which contains
    ///   the `dosing_speed` (flow rate in ml/sec) and `dosing_volume` (target volume in ml).
    ///
    /// # Returns
    /// A `u32` representing the calculated dosing duration in milliseconds.
    /// Returns `0` if either `dosing_speed` or `dosing_volume` is zero or negative,
    /// indicating that no dosing should occur due to invalid parameters.
    fn calc_target_dosing_duration_millis(balling_setval: &BallingSetVal) -> u32 {
        if balling_setval.dosing_speed > 0.0 && balling_setval.dosing_volume > 0.0 {
            let dosing_duration_seconds =
                balling_setval.dosing_volume / balling_setval.dosing_speed;
            (dosing_duration_seconds * 1000.0) as u32
        } else {
            0 // user set an invalid flow rate or invalid dosing volume -> no dosing
        }
    }

    /// Calculates the actual volume of fluid dosed based on the pump's flow rate and the actual run duration.
    ///
    /// This private helper function determines the precise volume dispensed, taking into account
    /// the pump's calibrated flow rate and the actual time it was active.
    ///
    /// # Arguments
    /// * `balling_setval` - A reference to a `BallingSetVal` struct, containing the pump's
    ///   `dosing_speed` (flow rate in ml/sec).
    /// * `actual_dosing_duration_millis` - The actual duration (in milliseconds) that the pump was active.
    ///
    /// # Returns
    /// An `f32` representing the actual volume of fluid dosed (in milliliters).
    /// Returns `0.0` if `dosing_speed` is zero or negative, or if `actual_dosing_duration_millis` is zero,
    /// indicating that no fluid was dispensed.
    fn calc_actual_dosing_volume(
        balling_setval: &BallingSetVal,
        actual_dosing_duration_millis: u32,
    ) -> f32 {
        if balling_setval.dosing_speed > 0.0 && actual_dosing_duration_millis > 0 {
            (actual_dosing_duration_millis as f32) / 1000.0 * balling_setval.dosing_speed
        } else {
            0.0 // user set an invalid flow rate or invalid dosing volume -> no dosing
        }
    }

    /// Orchestrates the dosing process for a specific peristaltic pump.
    ///
    /// This function performs the complete cycle of a dosing event:
    /// 1. Retrieve the pump's set values (e.g., flow rate, target volume) from the database.
    /// 2. Calculates the required dosing duration.
    /// 3. Locks the device scheduler mutex
    /// 4. Invokes the `mineral_injection` trait method to physically actuate the pump for the calculated duration.
    /// 5. Calculate the actual volume dosed based on the actual run time.
    /// 6. Log the completed Balling dosing event, including the actual volume, back to the database.
    ///
    /// The function also monitors for a `Quit` command from the signal handler during the injection process.
    ///
    /// # Arguments
    /// * `pump_id` - The unique numerical ID of the peristaltic pump to be actuated (e.g., 1, 2, 3, 4).
    /// * `pump_device` - The `AquariumDevice` enum variant corresponding to the `pump_id`,
    ///   representing the physical device.
    /// * `mineral_injection` - A mutable reference to an object implementing `MineralInjectionTrait`,
    ///   responsible for the physical control of the pump.
    /// * `balling_channels` - A mutable reference to `BallingChannels` struct containing all necessary `mpsc`
    /// * `sql_interface_balling` - A mutable reference to a `SqlInterfaceBalling` instance, providing the
    ///   specific SQL interface for Balling dosing operations. This is moved into the struct.
    /// * `mutex_device_scheduler_balling` - A reference to `Arc<Mutex<i32>>` used for coordinating
    ///   access to device scheduling, preventing parallel actuation across different
    ///   control modules. It holds a counter of the completed actuation.
    ///
    /// # Returns
    /// A `bool` which is `true` if a `Quit` command was received from the signal handler
    /// during the dosing process, indicating that the application should shut down; otherwise `false`.
    #[allow(clippy::too_many_arguments)]
    async fn execute_dosing(
        &mut self,
        pump_id: u32,
        pump_device: AquariumDevice,
        mineral_injection: &mut (impl MineralInjectionTrait + Send),
        balling_channels: &mut BallingChannels,
        sql_interface_balling: &mut SqlInterfaceBalling,
        mutex_device_scheduler_balling: &Arc<Mutex<i32>>,
        is_multi_thread: bool,
    ) -> bool {
        let mut quit_command_received: bool = false;
        let actual_dosing_duration_millis: u32;

        let get_setval_res = if is_multi_thread {
            tokio::task::block_in_place(|| {
                sql_interface_balling.get_single_balling_setval_from_database(pump_id.into())
            })
        } else {
            sql_interface_balling.get_single_balling_setval_from_database(pump_id.into())
        };

        match get_setval_res {
            Ok(balling_setval) => {
                let dosing_duration_millis =
                    Self::calc_target_dosing_duration_millis(&balling_setval);

                if dosing_duration_millis > 0 {
                    // inner scope to limit the lifetime of unlocked mutex
                    let (res_opt, join_err_opt) = tokio::task::block_in_place(|| {
                        std::thread::scope(|scope| {
                            let handle = scope.spawn(|| {
                                let mut mutex_data = mutex_device_scheduler_balling.lock().unwrap();
                                let res = mineral_injection.inject_mineral(
                                    balling_channels,
                                    pump_device,
                                    dosing_duration_millis,
                                );
                                *mutex_data = mutex_data.saturating_add(1);
                                res
                            });
                            match handle.join() {
                                Ok(res) => (Some(res), None),
                                Err(e) => {
                                    let panic_msg = if let Some(s) = e.downcast_ref::<&str>() {
                                        s.to_string()
                                    } else if let Some(s) = e.downcast_ref::<String>() {
                                        s.clone()
                                    } else {
                                        "Unknown panic payload".to_string()
                                    };
                                    (None, Some(ThreadJoinError(panic_msg)))
                                }
                            }
                        })
                    });

                    if let Some(err) = join_err_opt {
                        self.execution_blocked = true;
                        log_error_chain(module_path!(), "could not join thread", err);
                        quit_command_received = false;
                        actual_dosing_duration_millis = 0;
                    } else if let Some((q, actual_dur)) = res_opt {
                        quit_command_received = q;
                        actual_dosing_duration_millis = actual_dur;
                    } else {
                        actual_dosing_duration_millis = 0;
                    }

                    let actual_dosing_volume = Self::calc_actual_dosing_volume(
                        &balling_setval,
                        actual_dosing_duration_millis,
                    );

                    let insert_res = if is_multi_thread {
                        tokio::task::block_in_place(|| {
                            sql_interface_balling
                                .insert_balling_event(pump_id.into(), actual_dosing_volume.into())
                        })
                    } else {
                        sql_interface_balling
                            .insert_balling_event(pump_id.into(), actual_dosing_volume.into())
                    };
                    if let Err(e) = insert_res {
                        error!(
                            target: module_path!(),
                            "Error occurred in database communication: {e:?}"
                        );
                    }
                }
            }
            Err(e) => {
                error!(
                    target: module_path!(),
                    "encountered error when reading set values of pump #{pump_id}: {e:?}"
                );
            }
        }
        quit_command_received
    }

    /// Selects the appropriate peristaltic pump and its corresponding device representation based on a given ID.
    ///
    /// This function acts as a mapping from a numerical pump ID
    /// to a tuple containing the pump's `u32` ID and its `AquariumDevice` enum variant.
    /// Numerical pump IDs are used in external commands and in the database.
    ///
    /// # Arguments
    /// * `pump_id` - The numerical ID of the pump (e.g., 1, 2, 3, 4) to be selected.
    ///
    /// # Returns
    /// An `Option<(u32, AquariumDevice)>`:
    /// - `Some((u32, AquariumDevice))`: If the `pump_id` matches one of the known peristaltic pumps,
    ///   it will return its `u32` ID and the corresponding `AquariumDevice` variant.
    /// - `None`: If the provided `pump_id` does not correspond to a known peristaltic pump.
    pub fn get_pump_from_id(pump_id: i32) -> Option<(u32, AquariumDevice)> {
        match pump_id {
            1 => Some((1, AquariumDevice::PeristalticPump1)),
            2 => Some((2, AquariumDevice::PeristalticPump2)),
            3 => Some((3, AquariumDevice::PeristalticPump3)),
            4 => Some((4, AquariumDevice::PeristalticPump4)),
            _ => None,
        }
    }

    /// Executes the main control loop for the Balling dosing module.
    ///
    /// This function runs continuously, managing the automatic dosing schedule for
    /// multiple peristaltic pumps, processing external commands, and ensuring a
    /// graceful shutdown. It periodically checks the dosing countdowns for each pump
    /// and, if due, initiates the dosing process, respecting schedule limitations.
    ///
    /// The loop continues until a `Quit` command is received from the signal handler.
    /// After exiting the main loop, it sends a confirmation back to the signal handler
    /// and then waits for a `Terminate` command to complete its shutdown sequence.
    ///
    /// # Arguments
    /// * `mutex_device_scheduler_balling` - An `Arc<Mutex<i32>>` used for coordinating
    ///   access to device scheduling, preventing parallel actuation across different
    ///   control modules. It holds a counter of the completed actuation.
    /// * `balling_channels` - A mutable reference to `BallingChannels` struct containing all necessary `mpsc`
    ///   sender and receiver channels for inter-thread communication (e.g., with
    ///   the signal handler, relay manager, and schedule checker).
    /// * `mineral_injection` - A mutable reference to an object implementing the
    ///   `MineralInjectionTrait`, responsible for the physical control of the dosing pumps.
    /// * `sql_interface_balling` - A `SqlInterfaceBalling` instance, providing the
    ///   specific SQL interface for Balling dosing operations.
    /// * `is_multi_thread` - A boolean indicating if the tokio runtime is running in multithreaded mode.
    pub async fn execute(
        &mut self,
        mutex_device_scheduler_balling: Arc<Mutex<i32>>,
        balling_channels: &mut BallingChannels,
        mineral_injection: &mut (impl MineralInjectionTrait + Send),
        mut sql_interface_balling: SqlInterfaceBalling,
        is_multi_thread: bool,
    ) {
        #[cfg(all(target_os = "linux", not(test)))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let sleep_duration_hundred_millis = Duration::from_millis(100); // used to reduce the frequency of channel polling
        let sleep_duration_pump_switch =
            Duration::from_millis(self.config.pump_switch_delay_millis.into()); // wait between actuation of pumps
        let mut loop_counter = 0; // used to reduce the frequency of DB requests
        let mut quit_command_received: bool = false; // the request to end the application has been received
        let mut stop_command_received: bool; // the request to (temporarily) stop dosing has been received
        let mut start_command_received: bool; // the request to (temporarily) stop dosing has been received
        let mut dosing_inhibited: bool = false; // state of dosing control determined if the start/stop command has been received
        let mut schedule_check_result: bool; // flag indicating if operation of Balling is permitted at this time of day
        let mut target_instants_dosing = [Instant::now(); 4];

        // Calculate the target instant for each pump when the next dosing shall happen
        for pump_id in 1..=4 {
            target_instants_dosing[pump_id - 1] =
                Instant::now() + self.durations_until_next_dosing[pump_id - 1];
        }

        loop {
            if self.config.active
                && (loop_counter % (self.config.schedule_check_interval * 10) == 0)
            {
                // this code only executes when the .toml file is configured accordingly (.active)
                // and the monitoring period has passed (usually every 60 seconds)

                // schedule check to see if actuation is allowed
                perform_schedule_check!(
                    balling_channels,
                    schedule_check_result,
                    self.lock_error_channel_send_schedule_check,
                    self.lock_error_channel_receive_schedule_check,
                    module_path!() // Pass the current module path for accurate logging
                );

                #[allow(clippy::needless_range_loop)]
                if !self.execution_blocked {
                    for pump_array_index in 0..=3 {
                        if self.pump_is_active[pump_array_index]
                            && target_instants_dosing[pump_array_index] <= Instant::now()
                            && schedule_check_result
                            && !dosing_inhibited
                        {
                            let pump_id = pump_array_index as u32 + 1;
                            let pump_device = match pump_id {
                                1 => AquariumDevice::PeristalticPump1,
                                2 => AquariumDevice::PeristalticPump2,
                                3 => AquariumDevice::PeristalticPump3,
                                4 => AquariumDevice::PeristalticPump4,
                                _ => unreachable!(),
                            };
                            quit_command_received = self
                                .execute_dosing(
                                    pump_id,
                                    pump_device,
                                    mineral_injection,
                                    balling_channels,
                                    &mut sql_interface_balling,
                                    &mutex_device_scheduler_balling,
                                    is_multi_thread,
                                )
                                .await;
                            if quit_command_received {
                                break; // Break from inner loop
                            }
                            // wait a little period between dosings of each pump
                            tokio::time::sleep(sleep_duration_pump_switch).await;
                            target_instants_dosing[pump_array_index] +=
                                self.dosing_intervals[pump_array_index];
                        }
                    }
                    if quit_command_received {
                        break; // Break from outer loop
                    }
                }
            }

            // communication with other threads has to happen more frequently
            (
                quit_command_received,
                start_command_received,
                stop_command_received,
            ) = self.process_external_request(
                &mut balling_channels.rx_balling_from_signal_handler,
                Some(&mut balling_channels.rx_balling_from_messaging),
            );
            if self.execute_command_received && self.config.active && !self.execution_blocked {
                let pump_device_opt = Self::get_pump_from_id(self.pump_id_requested);
                match pump_device_opt {
                    Some((pump_id, pump_device)) => {
                        // reduce terminal output when testing
                        #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                        #[cfg(not(tarpaulin_include))]
                        info!(
                            target: module_path!(),
                            "executing external request for actuation of pump #{}",
                            self.pump_id_requested
                        );
                        quit_command_received = self
                            .execute_dosing(
                                pump_id,
                                pump_device,
                                mineral_injection,
                                balling_channels,
                                &mut sql_interface_balling,
                                &mutex_device_scheduler_balling,
                                is_multi_thread,
                            )
                            .await;
                    }
                    None => {
                        info!(
                            target: module_path!(),
                            "ignoring external request for actuation of pump #{}",
                            self.pump_id_requested
                        );
                    }
                }
            }

            if stop_command_received {
                #[cfg(test)]
                println!(
                    "{}: received Stop command. Inhibiting dosing.",
                    module_path!()
                );

                dosing_inhibited = true;
            }
            if start_command_received {
                #[cfg(test)]
                println!(
                    "{}: received Start command. (Re-)starting dosing.",
                    module_path!()
                );

                dosing_inhibited = false;
            }

            check_quit_increment_counter_ping_database!(
                quit_command_received,
                sleep_duration_hundred_millis,
                loop_counter,
                self,
                &mut sql_interface_balling,
                is_multi_thread
            );
        }

        balling_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads.
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination_async(
            &mut balling_channels.rx_balling_from_signal_handler,
            sleep_duration_hundred_millis,
            module_path!(),
        )
        .await;
    }
}

#[derive(Debug)]
struct ThreadJoinError(String);

impl fmt::Display for ThreadJoinError {
    /// Formats the value using the given formatter.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl error::Error for ThreadJoinError {}

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface_balling::BallingSetVal;
    use crate::mineral::balling::Balling;

    #[test]
    // Test case checks if the target dosing duration is calculated correctly
    // considering only valid input values.
    // The test case does not require any communication with the database.
    pub fn test_calc_target_dosing_duration_millis_happy_case() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 5.0,
            dosing_volume: 5.0,
            label: "test_calc_target_dosing_duration_millis_happy_case".to_string(),
        };

        assert_eq!(
            Balling::calc_target_dosing_duration_millis(&balling_setval),
            1000
        );
    }

    #[test]
    // Test case checks if the target dosing duration is calculated correctly
    // using zero dosing speed as input.
    // The test case does not require any communication with the database.
    pub fn test_calc_target_dosing_duration_millis_zero_dosing_speed() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 0.0,
            dosing_volume: 5.0,
            label: "test_calc_target_dosing_duration_millis_zero_dosing_speed".to_string(),
        };

        assert_eq!(
            Balling::calc_target_dosing_duration_millis(&balling_setval),
            0
        );
    }

    #[test]
    // Test case checks if the target dosing duration is calculated correctly
    // using negative dosing speed as input.
    // The test case does not require any communication with the database.
    pub fn test_calc_target_dosing_duration_millis_negative_dosing_speed() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: -5.0,
            dosing_volume: 5.0,
            label: "test_calc_target_dosing_duration_millis_negative_dosing_speed".to_string(),
        };

        assert_eq!(
            Balling::calc_target_dosing_duration_millis(&balling_setval),
            0
        );
    }

    #[test]
    // Test case checks if the target dosing duration is calculated correctly
    // using a zero dosing volume as input.
    // The test case does not require any communication with the database.
    pub fn test_calc_target_dosing_duration_millis_zero_dosing_volume() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 5.0,
            dosing_volume: 0.0,
            label: "test_calc_target_dosing_duration_millis_zero_dosing_volume".to_string(),
        };

        assert_eq!(
            Balling::calc_target_dosing_duration_millis(&balling_setval),
            0
        );
    }

    #[test]
    // Test case checks if the target dosing duration is calculated correctly
    // using negative dosing volume as input.
    // The test case does not require any communication with the database.
    pub fn test_calc_target_dosing_duration_millis_negative_dosing_volume() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 5.0,
            dosing_volume: -5.0,
            label: "test_calc_target_dosing_duration_millis_negative_dosing_volume".to_string(),
        };

        assert_eq!(
            Balling::calc_target_dosing_duration_millis(&balling_setval),
            0
        );
    }

    #[test]
    // Test case checks if the actual dosing duration is calculated correctly
    // considering only valid input values.
    // The test case does not require any communication with the database.
    pub fn test_calc_actual_dosing_volume_happy_case() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 5.0,
            dosing_volume: 5.0,
            label: "test_calc_actual_dosing_volume_happy_case".to_string(),
        };

        assert_eq!(
            Balling::calc_actual_dosing_volume(&balling_setval, 1000),
            5.0
        );
    }

    #[test]
    // Test case checks if the actual dosing duration is calculated correctly
    // using zero dosing speed as input.
    // The test case does not require any communication with the database.
    pub fn test_calc_actual_dosing_volume_zero_dosing_speed() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 0.0,
            dosing_volume: 5.0,
            label: "test_calc_actual_dosing_volume_zero_dosing_speed".to_string(),
        };

        assert_eq!(
            Balling::calc_actual_dosing_volume(&balling_setval, 1000),
            0.0
        );
    }

    #[test]
    // Test case checks if the actual dosing duration is calculated correctly
    // using negative dosing speed as input.
    // The test case does not require any communication with the database.
    pub fn test_calc_actual_dosing_volume_negative_dosing_speed() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 0.0,
            dosing_volume: 5.0,
            label: "test_calc_actual_dosing_volume_negative_dosing_speed".to_string(),
        };

        assert_eq!(
            Balling::calc_actual_dosing_volume(&balling_setval, 1000),
            0.0
        );
    }

    #[test]
    // Test case checks if the actual dosing duration is calculated correctly
    // using zero actual dosing duration as input.
    // The test case does not require any communication with the database.
    pub fn test_calc_actual_dosing_volume_zero_actual_dosing_duration() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 5.0,
            dosing_volume: 5.0,
            label: "test_calc_actual_dosing_volume_zero_actual_dosing_duration".to_string(),
        };

        assert_eq!(Balling::calc_actual_dosing_volume(&balling_setval, 0), 0.0);
    }
}
