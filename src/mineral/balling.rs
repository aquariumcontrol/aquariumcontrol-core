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
use std::fmt;

#[cfg(all(not(test), target_os = "linux"))] // only required in real-life execution
use nix::unistd::gettid;

use spin_sleep::SpinSleeper;
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
    durations_until_next_dosing: [Duration; 4],

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
}

impl fmt::Display for Balling {
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

        #[cfg(test)]
        println!(
            "{}, Duration since last dosing for pump {} is {} seconds.",
            module_path!(),
            pump_id,
            duration_since_balling_dosing.as_secs()
        );

        let target_dosing_interval_duration = Duration::from_secs(target_dosing_interval as u64);

        let remaining_time_until_next_dosing =
            if duration_since_balling_dosing >= target_dosing_interval_duration {
                Duration::from_secs(0)
            } else {
                target_dosing_interval_duration - duration_since_balling_dosing
            };

        #[cfg(test)]
        println!(
            "{}: Duration until next dosing dosing of pump {} is {} seconds.",
            module_path!(),
            pump_id,
            remaining_time_until_next_dosing.as_secs()
        );

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
    fn execute_dosing(
        &mut self,
        pump_id: u32,
        pump_device: AquariumDevice,
        mineral_injection: &mut impl MineralInjectionTrait,
        balling_channels: &mut BallingChannels,
        sql_interface_balling: &mut SqlInterfaceBalling,
        mutex_device_scheduler_balling: &Arc<Mutex<i32>>,
    ) -> bool {
        let mut quit_command_received: bool = false;
        let actual_dosing_duration_millis: u32;

        match sql_interface_balling.get_single_balling_setval_from_database(pump_id.into()) {
            Ok(balling_setval) => {
                let dosing_duration_millis =
                    Self::calc_target_dosing_duration_millis(&balling_setval);

                #[cfg(test)]
                println!(
                    "{}: Calculated target dosing duration of {} ms for pump #{}",
                    module_path!(),
                    dosing_duration_millis,
                    pump_id
                );

                if dosing_duration_millis > 0 {
                    // inner scope to limit the lifetime of unlocked mutex
                    {
                        let mut mutex_data = mutex_device_scheduler_balling.lock().unwrap();

                        (quit_command_received, actual_dosing_duration_millis) = mineral_injection
                            .inject_mineral(balling_channels, pump_device, dosing_duration_millis);

                        *mutex_data = mutex_data.saturating_add(1);
                    }

                    let actual_dosing_volume = Self::calc_actual_dosing_volume(
                        &balling_setval,
                        actual_dosing_duration_millis,
                    );
                    if let Err(e) = sql_interface_balling
                        .insert_balling_event(pump_id.into(), actual_dosing_volume.into())
                    {
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
    pub fn execute(
        &mut self,
        mutex_device_scheduler_balling: Arc<Mutex<i32>>,
        balling_channels: &mut BallingChannels,
        mineral_injection: &mut impl MineralInjectionTrait,
        mut sql_interface_balling: SqlInterfaceBalling,
    ) {
        #[cfg(all(target_os = "linux", not(test)))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let sleep_duration_hundred_millis = Duration::from_millis(100); // used to reduce the frequency of channel polling
        let sleep_duration_pump_switch =
            Duration::from_millis(self.config.pump_switch_delay_millis.into()); // wait between actuation of pumps
        let spin_sleeper = SpinSleeper::default();
        let mut loop_counter = 0; // used to reduce the frequency of DB requests
        let mut quit_command_received: bool; // the request to end the application has been received
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

                // debug information
                #[cfg(feature = "debug_balling")]
                {
                    // output countdowns every 10 seconds
                    debug!(
                        target: module_path!(),
                        "countdown 1 = {}, countdown 2 = {}, countdown 3 = {}, countdown 4 = {}",
                        self.countdown_dosing_pump1,
                        self.countdown_dosing_pump2,
                        self.countdown_dosing_pump3,
                        self.countdown_dosing_pump4,
                    );
                }
                #[allow(clippy::needless_range_loop)]
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
                        quit_command_received = self.execute_dosing(
                            pump_id,
                            pump_device,
                            mineral_injection,
                            balling_channels,
                            &mut sql_interface_balling,
                            &mutex_device_scheduler_balling,
                        );
                        if quit_command_received {
                            break; // Quit command received during dosing
                        }
                        // wait a little period between dosings of each pump
                        spin_sleeper.sleep(sleep_duration_pump_switch);
                        target_instants_dosing[pump_array_index] +=
                            self.dosing_intervals[pump_array_index];
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
                balling_channels.rx_balling_from_messaging_opt.as_mut(),
            );

            if self.execute_command_received && self.config.active {
                let pump_device_opt = Self::get_pump_from_id(self.pump_id_requested);
                match pump_device_opt {
                    Some((pump_id, pump_device)) => {
                        // reduce terminal output when testing
                        #[cfg(not(test))] // only required in real-life execution
                        #[cfg(not(tarpaulin_include))]
                        info!(
                            target: module_path!(),
                            "executing external request for actuation of pump #{}",
                            self.pump_id_requested
                        );
                        quit_command_received = self.execute_dosing(
                            pump_id,
                            pump_device,
                            mineral_injection,
                            balling_channels,
                            &mut sql_interface_balling,
                            &mutex_device_scheduler_balling,
                        );
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
                spin_sleeper,
                sleep_duration_hundred_millis,
                loop_counter,
                self,
                &mut sql_interface_balling
            );
        }

        balling_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads.
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination(
            &mut balling_channels.rx_balling_from_signal_handler,
            sleep_duration_hundred_millis,
            module_path!(),
        );
    }
}

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface::SqlInterface;
    use crate::database::sql_interface_balling::{BallingSetVal, SqlInterfaceBalling};
    use crate::database::sql_interface_error::SqlInterfaceError;
    use crate::database::sql_query_strings::{
        SQL_TABLE_BALLING_DOSING_LOG, SQL_TABLE_BALLING_SET_VALS,
    };
    use crate::launch::channels::Channels;
    use crate::mineral::balling::{Balling, BallingError};
    use crate::mocks::mock_mineral_injection::tests::MockMineralInjection;
    use crate::mocks::mock_schedule_check::tests::mock_schedule_check;
    use crate::utilities::channel_content::AquariumDevice::{
        PeristalticPump1, PeristalticPump2, PeristalticPump3, PeristalticPump4,
    };
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::config::{
        read_config_file, read_config_file_with_test_database, ConfigData,
    };
    use spin_sleep::SpinSleeper;
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::{Duration, Instant};

    fn read_balling_config() -> ConfigData {
        read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap()
    }

    fn read_balling_config_with_database(db_number: u32) -> ConfigData {
        read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            db_number,
        )
    }

    #[test]
    // This test case checks if the configuration file contains a valid value for the dosing interval.
    // It does not operate on any data from SQL database.
    pub fn test_balling_initialization_invalid_intervals() {
        let mut config: ConfigData = read_balling_config();

        config.balling.dosing_interval_pump1 = 0;
        config.balling.dosing_interval_pump2 = 0;
        config.balling.dosing_interval_pump3 = 0;
        config.balling.dosing_interval_pump4 = 0;

        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
        let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
        let sql_interface = match SqlInterface::new(config.sql_interface) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let mut sql_interface_balling = SqlInterfaceBalling::new(
            sql_interface.get_connection().unwrap(),
            max_rows_balling_set_values,
            max_rows_balling_dosing_log,
        )
        .unwrap();

        // create the test object
        let test_result = Balling::new(
            config.balling,
            Duration::from_millis(100),
            &mut sql_interface_balling,
        );
        match test_result {
            Ok(balling_controller) => {
                panic!(
                    "Successfully initialized Balling dosing, although error expected: {}",
                    balling_controller
                );
            }
            Err(e) => {
                println!("Balling initialization correctly returned error: {e:?}. Checking if error type is correct...");
                assert!(matches!(e, BallingError::InvalidDosingInterval(_, _)));
            }
        }
    }

    #[test]
    // When starting, the application checks when the last dosing events took place by analyzing
    // data from the database. As a prerequisite for this test case, an empty dosing log is required.
    // Test case uses test database #01.
    pub fn test_balling_initialization_empty_dosing_log() {
        let config: ConfigData = read_balling_config_with_database(1);

        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
        let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
        let mut sql_interface = match SqlInterface::new(config.sql_interface) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let mut sql_interface_balling = SqlInterfaceBalling::new(
            sql_interface.get_connection().unwrap(),
            max_rows_balling_set_values,
            max_rows_balling_dosing_log,
        )
        .unwrap();

        // empty the Balling dosing log table
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_BALLING_DOSING_LOG.to_string())
            .expect("Could not prepare test case");

        // insert balling pump configuration
        insert_pump_configurations(
            &mut sql_interface,
            &mut sql_interface_balling,
            true,
            true,
            true,
            true,
        );

        // create the test object
        let balling = Balling::new(
            config.balling,
            Duration::from_millis(100),
            &mut sql_interface_balling,
        )
        .unwrap();

        // check if the countdown calculation was correct
        assert_eq!(balling.durations_until_next_dosing[0].as_secs(), 0);
        assert_eq!(balling.durations_until_next_dosing[1].as_secs(), 0);
        assert_eq!(balling.durations_until_next_dosing[2].as_secs(), 0);
        assert_eq!(balling.durations_until_next_dosing[3].as_secs(), 0);
    }

    // Helper function to prepare for test cases which require a filled dosing log.
    // This function inserts dosing events for each of the four pumps.
    //
    // Arguments:
    // - `sql_interface`: A mutable reference to the main `SqlInterface` for database operations,
    //                    used here specifically for truncating the dosing log table.
    // - `sql_interface_balling`: A mutable reference to the `SqlInterfaceBalling`,
    //                            used for inserting balling dosing events.
    //
    // The time of the dosing events is set to:
    // * 100 seconds in the past for pump #1
    // * 200 seconds in the past for pump #2
    // * 300 seconds in the past for pump #3
    // * 400 seconds in the past for pump #4
    //
    // All SQL operations are expected to succeed; any failure will cause a panic,
    // indicating a test setup issue.
    fn insert_past_dosing_events(
        sql_interface: &mut SqlInterface,
        sql_interface_balling: &mut SqlInterfaceBalling,
    ) {
        // empty the Balling dosing log table
        match SqlInterface::truncate_table(sql_interface, SQL_TABLE_BALLING_DOSING_LOG.to_string())
        {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }
        let naive_datetime_100secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 100);
        let naive_datetime_200secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 200);
        let naive_datetime_300secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 300);
        let naive_datetime_400secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 400);

        // insert balling dosing events
        sql_interface_balling
            .insert_balling_event_for_test(naive_datetime_100secs_ago, 1, 0.5)
            .expect("Could not insert Balling dosing event for pump #1 into database");

        sql_interface_balling
            .insert_balling_event_for_test(naive_datetime_200secs_ago, 2, 0.5)
            .expect("Could not insert Balling dosing event for pump #2 into database");

        sql_interface_balling
            .insert_balling_event_for_test(naive_datetime_300secs_ago, 3, 0.5)
            .expect("Could not insert Balling dosing event for pump #3 into database");

        sql_interface_balling
            .insert_balling_event_for_test(naive_datetime_400secs_ago, 4, 0.5)
            .expect("Could not insert Balling dosing event for pump #4 into database");
    }

    #[test]
    // Test case checks if the countdowns for each pump are calculated correctly using
    // a dosing log in the database which has entries for all pumps.
    // Additionally, the test case checks the maximum row limitation for the dosing log.
    // Test case uses test database #02.
    pub fn test_balling_initialization_full_dosing_log() {
        let config: ConfigData = read_balling_config_with_database(2);

        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
        let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
        let mut sql_interface = match SqlInterface::new(config.sql_interface) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let mut sql_interface_balling = SqlInterfaceBalling::new(
            sql_interface.get_connection().unwrap(),
            max_rows_balling_set_values,
            max_rows_balling_dosing_log,
        )
        .unwrap();

        insert_pump_configurations(
            &mut sql_interface,
            &mut sql_interface_balling,
            true,
            true,
            true,
            true,
        );
        insert_past_dosing_events(&mut sql_interface, &mut sql_interface_balling);

        // create the test object
        let balling = Balling::new(
            config.balling,
            Duration::from_millis(100),
            &mut sql_interface_balling,
        )
        .unwrap();

        // check if the countdown calculation was correct:
        // the countdown depends on the pump-specific dosing interval and the timing of the last dosing event.
        assert_eq!(balling.durations_until_next_dosing[0].as_secs(), 100);
        assert_eq!(balling.durations_until_next_dosing[1].as_secs(), 0);
        assert_eq!(balling.durations_until_next_dosing[2].as_secs(), 0);
        assert_eq!(balling.durations_until_next_dosing[3].as_secs(), 0);

        // dosing log has four entries
        let test_result = SqlInterfaceBalling::new(sql_interface.get_connection().unwrap(), 0, 1);
        assert!(matches!(
            test_result,
            Err(SqlInterfaceError::DatabaseBallingDosingLogTableContainsTooManyRows(_, _, _))
        ));
    }

    // Helper function to prepare for test cases which require balling pump configuration.
    // This function first truncates the pump configuration table and then
    // inserts a balling pump configuration into the database for each of the four pumps.
    //
    // Arguments:
    // - `sql_interface`: A mutable reference to the main `SqlInterface` for database operations,
    //                    used here specifically for truncating the balling set values table.
    // - `sql_interface_balling`: A mutable reference to the `SqlInterfaceBalling`,
    //                            used for inserting balling set values.
    // - `pump1`: flag indicating that a pump configuration for pump #1 should be inserted.
    // - `pump2`: flag indicating that a pump configuration for pump #2 should be inserted.
    // - `pump3`: flag indicating that a pump configuration for pump #3 should be inserted.
    // - `pump4`: flag indicating that a pump configuration for pump #4 should be inserted.
    //
    // All SQL operations are expected to succeed; any failure will cause a panic,
    // indicating a test setup issue.
    fn insert_pump_configurations(
        sql_interface: &mut SqlInterface,
        sql_interface_balling: &mut SqlInterfaceBalling,
        pump1: bool,
        pump2: bool,
        pump3: bool,
        pump4: bool,
    ) {
        // empty the Balling pump configuration table
        match SqlInterface::truncate_table(sql_interface, SQL_TABLE_BALLING_SET_VALS.to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }
        if pump1 {
            match sql_interface_balling.insert_pump_configuration(1, 4.0, 1.0, "pump1".to_string())
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: Could not insert pump 1 configuration ({e:?})",
                        module_path!(),
                    );
                }
            }
        }
        if pump2 {
            match sql_interface_balling.insert_pump_configuration(2, 4.0, 1.0, "pump2".to_string())
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: Could not insert pump 2 configuration ({e:?})",
                        module_path!(),
                    );
                }
            }
        }
        if pump3 {
            match sql_interface_balling.insert_pump_configuration(3, 4.0, 1.0, "pump3".to_string())
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: Could not insert pump 3 configuration ({e:?})",
                        module_path!(),
                    );
                }
            }
        }
        if pump4 {
            match sql_interface_balling.insert_pump_configuration(4, 4.0, 1.0, "pump4".to_string())
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: Could not insert pump 4 configuration ({e:?})",
                        module_path!(),
                    );
                }
            }
        }
    }

    // This function executes the test object and test environment in separate threads.
    // The test case depends on the configuration and reference values provided for the asserts.
    pub fn test_balling_dosing(
        test_db_number: u32,
        schedule_allows: bool,
        execution_count_reference: i32,
        duration_millis: u64,
        pump1_engagement_count_reference: usize,
        pump2_engagement_count_reference: usize,
        pump3_engagement_count_reference: usize,
        pump4_engagement_count_reference: usize,
        schedule_check_interval: u32,
        dosing_intervals: Option<(u32, u32, u32, u32)>,
    ) {
        let sleep_duration_test_environment = Duration::from_millis(duration_millis);
        let spin_sleeper_test_environment = SpinSleeper::default();
        let mut config: ConfigData = read_balling_config_with_database(test_db_number);

        config.balling.pump1_active = pump1_engagement_count_reference > 0;
        config.balling.pump2_active = pump2_engagement_count_reference > 0;
        config.balling.pump3_active = pump3_engagement_count_reference > 0;
        config.balling.pump4_active = pump4_engagement_count_reference > 0;
        config.balling.schedule_check_interval = schedule_check_interval;

        match dosing_intervals {
            Some((
                dosing_interval_pump1,
                dosing_interval_pump2,
                dosing_interval_pump3,
                dosing_interval_pump4,
            )) => {
                config.balling.dosing_interval_pump1 = dosing_interval_pump1;
                config.balling.dosing_interval_pump2 = dosing_interval_pump2;
                config.balling.dosing_interval_pump3 = dosing_interval_pump3;
                config.balling.dosing_interval_pump4 = dosing_interval_pump4;
            }
            None => { /* do nothing */ }
        }

        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
        let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
        let mut sql_interface = match SqlInterface::new(config.sql_interface) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let mut sql_interface_balling = SqlInterfaceBalling::new(
            sql_interface.get_connection().unwrap(),
            max_rows_balling_set_values,
            max_rows_balling_dosing_log,
        )
        .unwrap();

        insert_pump_configurations(
            &mut sql_interface,
            &mut sql_interface_balling,
            pump1_engagement_count_reference > 0,
            pump2_engagement_count_reference > 0,
            pump3_engagement_count_reference > 0,
            pump4_engagement_count_reference > 0,
        );
        insert_past_dosing_events(&mut sql_interface, &mut sql_interface_balling);

        let mut channels = Channels::new_for_test();

        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_device_scheduler_environment = mutex_device_scheduler.clone();

        // create the test object
        let mut balling = Balling::new(
            config.balling,
            Duration::from_millis(100),
            &mut sql_interface_balling,
        )
        .unwrap();

        let mut mock_mineral_injection = MockMineralInjection::new();

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_balling,
                    &mut channels.schedule_check.rx_schedule_check_from_balling,
                    None,
                    schedule_allows,
                )
            });

            // thread for the rest of test environment
            scope.spawn(move || {
                let execution_count: i32;
                spin_sleeper_test_environment.sleep(sleep_duration_test_environment);

                // internal scope to for releasing mutex after read
                {
                    execution_count = *mutex_device_scheduler_environment.lock().unwrap();
                }

                println!("test_balling_dosing: execution_count={}", execution_count);
                assert_eq!(execution_count, execution_count_reference);

                // now we can send the quit command
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_balling
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_balling
                    .recv()
                    .unwrap();
                println!(
                    "test_balling_dosing: Received confirmation of Quit command from test object."
                );
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_balling
                    .send(InternalCommand::Terminate);
            });

            // thread for the test object
            scope.spawn(move || {
                let mut tx_balling_to_schedule_check_for_test_case_finish =
                    channels.balling.tx_balling_to_schedule_check.clone();

                balling.execute(
                    mutex_device_scheduler,
                    &mut channels.balling,
                    &mut mock_mineral_injection,
                    sql_interface_balling,
                );

                let _ =
                    tx_balling_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

                println!("{}", mock_mineral_injection);

                assert_eq!(
                    mock_mineral_injection.pump1_engagement_durations.len(),
                    pump1_engagement_count_reference
                );
                assert_eq!(
                    mock_mineral_injection.pump2_engagement_durations.len(),
                    pump2_engagement_count_reference
                );
                assert_eq!(
                    mock_mineral_injection.pump3_engagement_durations.len(),
                    pump3_engagement_count_reference
                );
                assert_eq!(
                    mock_mineral_injection.pump4_engagement_durations.len(),
                    pump4_engagement_count_reference
                );
            });
        });
    }

    #[test]
    // The test case executes the test object with the following boundaries:
    // - pump #1 is active
    // - the schedule checker is allowing operation
    // Test case uses test database #03.
    pub fn test_balling_dosing_schedule_allows_single_dosing() {
        test_balling_dosing(
            3,                        // index for determining test the database
            true,                     // schedule allows
            1,                        // execution count
            10000,                    // maximum time in milliseconds for test execution
            1,                        // number of dosing events for pump #1
            0,                        // number of dosing events for pump #2
            0,                        // number of dosing events for pump #3
            0,                        // number of dosing events for pump #4
            40,                       // schedule check interval
            Some((50, 200, 100, 50)), // dosing intervals
        );
    }

    #[test]
    // The test case executes the test object with the following boundaries:
    // - all pumps are active
    // - schedule checker is not allowing operation
    // Test case uses test database #04.
    pub fn test_balling_dosing_schedule_forbids() {
        test_balling_dosing(
            4,     // index for determining the test database
            false, // schedule does not allow
            0,     // execution count
            10000, // maximum time in milliseconds for test execution
            0,     // number of dosings for pump #1
            0,     // number of dosings for pump #2
            0,     // number of dosings for pump #3
            0,     // number of dosings for pump #4
            60,    // schedule check interval
            None,
        );
    }

    #[test]
    // The test case executes the test object with the following boundaries:
    // - all pumps are active
    // - the schedule checker is allowing operation
    // - dosing intervals are short so that lots of dosing events are triggered
    // Test case uses test database #05.
    pub fn test_balling_dosing_schedule_allows_multiple_dosings() {
        test_balling_dosing(
            5,                  // index for determining test database
            true,               // schedule allows
            28,                 // execution count
            34000,              // maximum time in milliseconds for test execution
            12,                 // number of dosing events for pump #1
            7,                  // number of dosing events for pump #2
            5,                  // number of dosing events for pump #3
            4,                  // number of dosing events for pump #4
            1,                  // schedule check interval
            Some((3, 5, 7, 9)), // dosing intervals
        );
    }

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

    #[test]
    // Test case runs Balling dosing control and triggers inhibition by sending the stop message.
    // After verification that Balling dosing is stopped,
    // The test case sends the start message and verifies if Balling dosing control
    // resumes operation.
    // Test case uses test database #06.
    pub fn test_messaging_stops_starts_balling_dosing() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let sleep_duration_7_secs = Duration::from_secs(7);
        let spin_sleeper = SpinSleeper::default();

        let mut config: ConfigData = read_balling_config_with_database(6);

        config.balling.pump2_active = false;
        config.balling.pump3_active = false;
        config.balling.pump4_active = false;
        config.balling.schedule_check_interval = 1;
        config.balling.dosing_interval_pump1 = 5;

        let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
        let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
        let mut sql_interface = match SqlInterface::new(config.sql_interface) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        match SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_BALLING_DOSING_LOG.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mut sql_interface_balling = SqlInterfaceBalling::new(
            sql_interface.get_connection().unwrap(),
            max_rows_balling_set_values,
            max_rows_balling_dosing_log,
        )
        .unwrap();

        // empty the Balling dosing log table
        match SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_BALLING_DOSING_LOG.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        // insert a valid pump configuration for pump #1
        insert_pump_configurations(
            &mut sql_interface,
            &mut sql_interface_balling,
            true,
            false,
            false,
            false,
        );

        // create test objects
        let mut balling = Balling::new(
            config.balling,
            Duration::from_millis(100),
            &mut sql_interface_balling,
        )
        .unwrap();

        let mut channels = Channels::new_for_test();

        let mut tx_balling_to_schedule_check_for_test_case_finish =
            channels.balling.tx_balling_to_schedule_check.clone();

        let mutex_device_scheduler = Arc::new(Mutex::new(0));

        let mut mock_mineral_injection = MockMineralInjection::new();

        scope(|scope| {
            // thread for mock schedule check: runs until it receives Quit command
            // thread for schedule check
            scope.spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_balling,
                    &mut channels.schedule_check.rx_schedule_check_from_balling,
                    None,
                    true,
                )
            });

            let mutex_device_scheduler_balling_test_environment = mutex_device_scheduler.clone();

            // thread IPC messaging - this scope controls execution of the test case
            scope.spawn(move || {
                // wait for up to 5 seconds to make sure the test object can receive the message and execute first dosing
                let timeout_initial = Duration::from_secs(5);
                let start_initial = Instant::now();
                let mut actuation_count_0: i32;
                loop {
                    spin_sleeper.sleep(sleep_duration_100_millis);
                    actuation_count_0 = *mutex_device_scheduler_balling_test_environment
                        .lock()
                        .unwrap();
                    if actuation_count_0 >= 1 || start_initial.elapsed() > timeout_initial {
                        break;
                    }
                }

                // check the initial amount of actuation
                assert_eq!(actuation_count_0, 1);

                // sending the message requesting stop of Balling dosing control
                match channels
                    .messaging
                    .tx_messaging_to_balling
                    .send(InternalCommand::Stop)
                {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending stop command to test object ({e:?})",
                            module_path!()
                        );
                    }
                }

                // Wait for 7 seconds. If the test object does not follow the stop request,
                // then dosings will happen in this period.
                spin_sleeper.sleep(sleep_duration_7_secs);

                let actuation_count_1 = *mutex_device_scheduler_balling_test_environment
                    .lock()
                    .unwrap();
                // check the amount of actuation (after stop request)
                assert_eq!(actuation_count_1, 1);

                // sending the message requesting restart of Balling dosing control
                match channels
                    .messaging
                    .tx_messaging_to_balling
                    .send(InternalCommand::Start)
                {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending start command to test object ({e:?})",
                            module_path!()
                        );
                    }
                }

                // Wait for up to 10 seconds. If the test object follows the start request,
                // then dosings will happen in this period.
                let timeout_resume = Duration::from_secs(10);
                let start_resume = Instant::now();
                let mut actuation_count_2: i32;
                loop {
                    spin_sleeper.sleep(sleep_duration_100_millis);
                    actuation_count_2 = *mutex_device_scheduler_balling_test_environment
                        .lock()
                        .unwrap();
                    if actuation_count_2 >= 3 || start_resume.elapsed() > timeout_resume {
                        break;
                    }
                }
                // check the amount of actuation (after start request)
                assert_eq!(actuation_count_2, 3);

                // requesting Balling dosing control to quit
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_balling
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_balling
                    .recv()
                    .unwrap();

                let actuation_count_3 = *mutex_device_scheduler_balling_test_environment
                    .lock()
                    .unwrap();
                // check the amount of actuation (after Quit command)
                assert_eq!(actuation_count_3, 3);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_balling
                    .send(InternalCommand::Terminate);
            });

            spin_sleeper.sleep(sleep_duration_100_millis);

            // thread for the test object and assertions
            scope.spawn(move || {
                balling.execute(
                    mutex_device_scheduler,
                    &mut channels.balling,
                    &mut mock_mineral_injection,
                    sql_interface_balling,
                );
                println!("MockMineralInjection:\n{}", mock_mineral_injection);

                assert_eq!(mock_mineral_injection.pump1_engagement_durations.len(), 3);
                assert_eq!(mock_mineral_injection.pump2_engagement_durations.len(), 0);
                assert_eq!(mock_mineral_injection.pump3_engagement_durations.len(), 0);
                assert_eq!(mock_mineral_injection.pump4_engagement_durations.len(), 0);

                assert_eq!(
                    mock_mineral_injection
                        .pump1_engagement_durations
                        .pop()
                        .unwrap(),
                    250
                );
                assert_eq!(
                    mock_mineral_injection
                        .pump1_engagement_durations
                        .pop()
                        .unwrap(),
                    250
                );

                let _ =
                    tx_balling_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            });

            println!(
                "* [Messaging] checking if messaging can stop and start balling dosing succeeded."
            );
        });
    }

    #[test]
    // Test case checks if during instantiation of the struct, the implementation checks
    // if there is a database entry for a pump that is configured as active.
    // The code shall return an error of type BallingError if there is no matching database entry.
    // Test case uses test database #48.
    pub fn test_check_database_vs_config() {
        let mut config: ConfigData = read_balling_config_with_database(48);

        config.balling.pump1_active = true;
        config.balling.pump2_active = false;
        config.balling.pump3_active = false;
        config.balling.pump4_active = false;

        let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
        let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
        let mut sql_interface = match SqlInterface::new(config.sql_interface) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        match SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_BALLING_DOSING_LOG.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }
        let mut sql_interface_balling = SqlInterfaceBalling::new(
            sql_interface.get_connection().unwrap(),
            max_rows_balling_set_values,
            max_rows_balling_dosing_log,
        )
        .unwrap();

        // insert a valid pump configuration for pump #1
        insert_pump_configurations(
            &mut sql_interface,
            &mut sql_interface_balling,
            false,
            false,
            false,
            true, // a different pump is configured as active
        );

        // create test objects
        let result = Balling::new(
            config.balling,
            Duration::from_millis(100),
            &mut sql_interface_balling,
        );
        assert!(matches!(
            result,
            Err(BallingError::SetValueRetrievalError {
                location: _,
                pump_id: _,
                source: _
            })
        ));
    }

    #[test]
    // The test case executes the test object using dosing requests via the external interface.
    pub fn test_balling_dosing_external_requests() {
        let test_db_number = 49;
        let schedule_allows = true;
        let execution_count_reference: i32 = 4;
        let duration_millis = 5000;
        let sleep_duration_test_environment = Duration::from_millis(duration_millis);
        let spin_sleeper_test_environment = SpinSleeper::default();
        let mut config: ConfigData = read_balling_config_with_database(test_db_number);

        config.balling.pump1_active = true;
        config.balling.dosing_interval_pump1 = 3600 * 24; // long interval to avoid regular dosing
        config.balling.pump2_active = true;
        config.balling.dosing_interval_pump2 = 3600 * 24; // long interval to avoid regular dosing
        config.balling.pump3_active = true;
        config.balling.dosing_interval_pump3 = 3600 * 24; // long interval to avoid regular dosing
        config.balling.pump4_active = true;
        config.balling.dosing_interval_pump4 = 3600 * 24; // long interval to avoid regular dosing
        config.balling.schedule_check_interval = 100;

        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
        let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
        let mut sql_interface = match SqlInterface::new(config.sql_interface) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        match SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_BALLING_DOSING_LOG.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }
        let mut sql_interface_balling = SqlInterfaceBalling::new(
            sql_interface.get_connection().unwrap(),
            max_rows_balling_set_values,
            max_rows_balling_dosing_log,
        )
        .unwrap();

        insert_pump_configurations(
            &mut sql_interface,
            &mut sql_interface_balling,
            true,
            true,
            true,
            true,
        );
        insert_past_dosing_events(&mut sql_interface, &mut sql_interface_balling);

        let mut channels = Channels::new_for_test();

        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_device_scheduler_environment = mutex_device_scheduler.clone();

        // create the test object
        let mut balling = Balling::new(
            config.balling,
            Duration::from_secs(1000),
            &mut sql_interface_balling,
        )
        .unwrap();

        let mut mock_mineral_injection = MockMineralInjection::new();

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_balling,
                    &mut channels.schedule_check.rx_schedule_check_from_balling,
                    None,
                    schedule_allows,
                )
            });

            // thread for the rest of test environment
            scope.spawn(move || {
                let execution_count1: i32;
                spin_sleeper_test_environment.sleep(sleep_duration_test_environment);
                // internal scope to for releasing mutex after read
                {
                    execution_count1 = *mutex_device_scheduler_environment.lock().unwrap();
                }
                println!("test_balling_dosing: execution_count={}", execution_count1);
                assert_eq!(execution_count1, 0);

                // send the dosing requests to Balling
                channels
                    .messaging
                    .tx_messaging_to_balling
                    .send(InternalCommand::Execute(2))
                    .unwrap();
                channels
                    .messaging
                    .tx_messaging_to_balling
                    .send(InternalCommand::Execute(4))
                    .unwrap();
                channels
                    .messaging
                    .tx_messaging_to_balling
                    .send(InternalCommand::Execute(3))
                    .unwrap();
                channels
                    .messaging
                    .tx_messaging_to_balling
                    .send(InternalCommand::Execute(1))
                    .unwrap();

                let execution_count2: i32;
                spin_sleeper_test_environment.sleep(sleep_duration_test_environment);
                // internal scope to for releasing mutex after read
                {
                    execution_count2 = *mutex_device_scheduler_environment.lock().unwrap();
                }
                println!("test_balling_dosing: execution_count={}", execution_count2);
                assert_eq!(execution_count2, execution_count_reference);

                // now we can send the quit command
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_balling
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_balling
                    .recv()
                    .unwrap();
                println!(
                    "test_balling_dosing: Received confirmation of Quit command from test object."
                );
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_balling
                    .send(InternalCommand::Terminate);
            });

            // thread for the test object
            scope.spawn(move || {
                let mut tx_balling_to_schedule_check_for_test_case_finish =
                    channels.balling.tx_balling_to_schedule_check.clone();

                balling.execute(
                    mutex_device_scheduler,
                    &mut channels.balling,
                    &mut mock_mineral_injection,
                    sql_interface_balling,
                );

                let _ =
                    tx_balling_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

                println!("{}", mock_mineral_injection);

                let reference_pump_protocol = vec![
                    PeristalticPump2,
                    PeristalticPump4,
                    PeristalticPump3,
                    PeristalticPump1,
                ];

                assert_eq!(
                    mock_mineral_injection.pump_protocol,
                    reference_pump_protocol
                );
            });
        });
    }

    #[test]
    // This test case checks if the code checks the dosing interval against the check interval.
    // It does not operate on any data from SQL database.
    pub fn test_balling_initialization_invalid_dosing_check_interval() {
        let mut config: ConfigData = read_balling_config();

        config.balling.dosing_interval_pump1 = 100;
        config.balling.dosing_interval_pump2 = 100;
        config.balling.dosing_interval_pump3 = 100;
        config.balling.dosing_interval_pump4 = 100;
        config.balling.schedule_check_interval = 200;

        let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
        let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
        let mut sql_interface = match SqlInterface::new(config.sql_interface) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        match SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_BALLING_DOSING_LOG.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }
        let mut sql_interface_balling = SqlInterfaceBalling::new(
            sql_interface.get_connection().unwrap(),
            max_rows_balling_set_values,
            max_rows_balling_dosing_log,
        )
        .unwrap();

        // create the test object
        let test_result = Balling::new(
            config.balling,
            Duration::from_secs(1000),
            &mut sql_interface_balling,
        );
        match test_result {
            Ok(balling_controller) => {
                panic!(
                    "Successfully initialized Balling dosing, although error expected: {}",
                    balling_controller
                );
            }
            Err(e) => {
                println!("Balling initialization correctly returned error: {e:?}. Checking if error type is correct...");
                assert!(matches!(
                    e,
                    BallingError::DosingIntervalShorterThanCheckInterval(_, _, _, _)
                ));
            }
        }
    }

    #[test]
    // This test case checks that `Balling::new` returns an error when the
    // schedule_check_interval is set to zero in the configuration.
    pub fn test_balling_initialization_zero_check_interval() {
        let mut config: ConfigData = read_balling_config();
        config.balling.schedule_check_interval = 0;

        let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
        let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
        let mut sql_interface = SqlInterface::new(config.sql_interface).unwrap();
        match SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_BALLING_DOSING_LOG.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }
        let mut sql_interface_balling = SqlInterfaceBalling::new(
            sql_interface.get_connection().unwrap(),
            max_rows_balling_set_values,
            max_rows_balling_dosing_log,
        )
        .unwrap();

        // create the test object
        let test_result = Balling::new(
            config.balling,
            Duration::from_secs(1000),
            &mut sql_interface_balling,
        );

        match test_result {
            Ok(balling_controller) => {
                panic!(
                    "Successfully initialized Balling dosing, although an error was expected: {}",
                    balling_controller
                );
            }
            Err(e) => {
                println!("Balling initialization correctly returned error: {e:?}. Checking if error type is correct...");
                assert!(matches!(e, BallingError::ScheduleCheckIntervalZero(_)));
            }
        }
    }
}
