/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages the aquarium's heating system through a dedicated control thread.
//!
//! This module implements the `Heating` control thread, a long-running process
//! responsible for controlling the aquarium's water temperature in conjunction with
//! the ventilation control. It's a sophisticated controller that integrates sensor data,
//! safety checks, scheduling, and statistical logging to provide robust and efficient thermal management.
//!
//! ## Key Responsibilities
//!
//! - **Temperature Control**: Implements a hybrid control strategy:
//!   - **Hysteresis Control**: Switches the heater fully on when the temperature drops
//!     below `switch_on_temperature` and fully off when it exceeds `switch_off_temperature`.
//!   - **Proportional Control**: Within the hysteresis band, it uses a `SawToothProfile`
//!     to achieve a "PWM-like" effect, modulating the heater's on-time to provide
//!     smoother temperature regulation.
//!
//! - **Safety Interlocks**: A critical safety feature monitors the tank's water level.
//!   If the level is detected as low for a configured duration, the heater is
//!   forcibly switched off to prevent overheating and damage.
//!
//! - **Daily Statistics Logging**: It continuously tracks energy consumption (in kWh),
//!   average water temperature, and average ambient temperature. Just after midnight,
//!   it persists these daily statistics to an SQL database for long-term analysis.
//!
//! - **External Control**: Listens for `Start` and `Stop` commands from other parts of
//!   the system (e.g., a messaging interface), allowing heating to be manually
//!   inhibited or re-enabled. It also respects a system-wide schedule.
//!
//! ## Key Components
//!
//! - **`Heating` Struct**: The core state machine for the thread. It holds the
//!   configuration, internal state, and log-suppression flags.
//!
//! - **`execute()` Method**: The main thread loop. This monolithic function orchestrates
//!   all the module's responsibilities, from reading sensors to actuating the relay
//!   and logging data.
//!
//! - **`actuate_heater()`**: A private helper that encapsulates the channel communication
//!   required to send a command to the `RelayManager` and wait for its response.
//!
//! - **`HeatingMutexes` Struct**: A container holding `Arc<Mutex<...>>` handles to all
//!   the *input* data sources (from `SensorManager` and `TankLevelSwitch`).
//!
//! ## Design and Architecture
//!
//! - **Decoupled and Concurrent**: The `Heating` thread runs independently, communicating
//!   with other threads asynchronously via channels (for commands) and mutexes (for
//!   shared state like sensor data). This decouples the control logic from the
//!   hardware drivers and data sources.
//!
//! - **Stateful and Robust**: The thread maintains its own state (`heater_state`,
//!   `heating_inhibited`) and uses flags to manage log suppression and
//!   recover from transient errors (like a failed channel send-operation).
//!
//! - **Data-Driven**: Its behavior is driven by a combination of real-time sensor data,
//!   pre-configured parameters from a TOML file, and dynamic setpoints fetched from
//!   a database.

use log::error;
#[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
use log::{info, warn};

use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(all(target_os = "linux", not(test), not(feature = "test-utils")))]
// only required in real-life execution
use nix::unistd::gettid;

use crate::database::sql_interface_heating_stats::{HeatingStatsEntry, SqlInterfaceHeatingStats};
use crate::database::thermal_set_value_updater_trait::ThermalSetValueUpdaterTrait;
use crate::thermal::heating_actuation::{switch_off_heater, switch_on_heater};
use crate::thermal::heating_channels::HeatingChannels;
use crate::thermal::heating_config::HeatingConfig;
use crate::thermal::heating_mutexes::HeatingMutexes;
pub(crate) use crate::thermal::heating_stats_data_transfer_trait::HeatingStatsDataTransferTrait;
use crate::thermal::temperature_gradient_monitor::TemperatureGradientMonitorTrait;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::channel_content::ActuatorState;
use crate::utilities::check_mutex_access_duration::CheckMutexAccessDurationTrait;
use crate::utilities::database_ping_trait::DatabasePingTrait;
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::sawtooth_profile::SawToothProfile;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;
use crate::{perform_schedule_check, update_thermal_set_values};

pub const CYCLE_TIME_HEATING_MILLIS: u64 = 500;

const TIME_INCREMENT_HEATING_SECS: f32 = CYCLE_TIME_HEATING_MILLIS as f32 / 1000.0;

/// allow max. 10 milliseconds for mutex to be blocked by any other thread
const MAX_MUTEX_ACCESS_DURATION_MILLIS: u64 = 10;

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the heating control.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     ambient[Ambient] -.-> heating
///     tank_level_switch[Tank Level Switch] -.-> heating
///     heating --> relay_manager[Relay Manager]
///     relay_manager --> heating
///     heating --> signal_handler[Signal Handler]
///     signal_handler --> heating
///     heating --> data_logger[Data Logger]
///     data_logger --> heating
///     heating --> schedule_check[Schedule Check]
///     schedule_check --> heating
///     messaging[Messaging] --> heating
/// ```
pub struct Heating {
    /// configuration data for heating control
    pub config: HeatingConfig,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive information from schedule check
    lock_error_channel_receive_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send information to schedule check
    lock_error_channel_send_schedule_check: bool,

    /// recording when the last database ping happened
    pub last_ping_instant: Instant,

    /// database ping interval
    pub database_ping_interval: Duration,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex
    pub lock_warn_max_mutex_access_duration: bool,

    /// Flag to avoid flooding the log with warnings when the heating set values could not be read from the database
    lock_error_heating_set_value_read_failure: bool,

    // for testing purposes: record when mutex access time is exceeded without resetting it
    #[cfg(any(test, feature = "test-utils"))]
    pub mutex_access_duration_exceeded: bool,

    /// Inhibition flag to avoid flooding the log with warning about having received inapplicable command from the signal handler
    pub lock_warn_inapplicable_command_signal_handler: bool,

    /// Inhibition flag to avoid flooding the log with errors about the channel communication not working
    pub lock_error_channel_receive_termination: bool,

    /// Maximum permissible access duration for Mutex
    pub max_mutex_access_duration: Duration,

    /// Instant at which statistics is updated - used for energy consumption calculation
    pub last_stats_update_instant_opt: Option<Instant>,

    /// The time the actuator was last turned on continuously.
    pub last_actuator_on_time: Option<Instant>,

    /// Inhibition flag to prevent log flooding for gradient monitoring.
    pub log_inhibition_gradient: bool,

    /// flag indicating that actuator has been disabled due to indication from temperature gradient monitor
    pub actuator_disabled: bool,
}

impl TemperatureGradientMonitorTrait for Heating {
    /// Performs the `max_time_actuator_effect_monitor` operation.
    fn max_time_actuator_effect_monitor(&self) -> u64 {
        self.config.max_time_actuator_effect_monitor
    }

    /// Performs the `monitor_state` operation.
    fn monitor_state(&mut self) -> (&mut Option<Instant>, &mut bool) {
        (
            &mut self.last_actuator_on_time,
            &mut self.log_inhibition_gradient,
        )
    }
}

impl Heating {
    /// Creates a new `Heating` control instance.
    ///
    /// This constructor initializes the heating control module with its specific
    /// configuration and a dedicated SQL interface. It also sets up all internal
    /// "lock" flags to `false` by default; these flags are crucial during operation
    /// to prevent log files from being flooded with repeated error or warning messages
    /// stemming from channel communication or invalid requests.
    ///
    /// # Arguments
    /// * `config` - **Configuration data** for the heating control, loaded from a TOML file.
    ///   This includes parameters such as target temperatures, safety features, and
    ///   behavioral strategies.
    /// * `sql_interface_heating` - The **specific SQL interface** for heating-related database operations.
    ///   This instance is moved into the `Heating` struct.
    /// * `database_ping_interval` - A `Duration` instance, providing the interval to ping the database.
    ///
    /// # Returns
    /// A new **`Heating` struct**, ready to manage the aquarium's heating system.
    ///
    /// [impl->swreq~thermal_read_heating_limits~1]
    pub fn new(config: HeatingConfig, database_ping_interval: Duration) -> Heating {
        Self {
            config,
            lock_error_channel_send_schedule_check: false,
            lock_error_channel_receive_schedule_check: false,
            last_ping_instant: Instant::now(),
            database_ping_interval,
            lock_warn_max_mutex_access_duration: false,
            lock_error_heating_set_value_read_failure: false,
            #[cfg(any(test, feature = "test-utils"))]
            mutex_access_duration_exceeded: false,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
            max_mutex_access_duration: Duration::from_millis(MAX_MUTEX_ACCESS_DURATION_MILLIS),
            last_stats_update_instant_opt: None,
            last_actuator_on_time: None,
            log_inhibition_gradient: false,
            actuator_disabled: false,
        }
    }

    /// Calculates a **normalized control deviation** based on the measured temperature and configured heating thresholds.
    ///
    /// This private helper function determines how far the `measured_value` deviates
    /// from the `switch_on_temperature` relative to the total temperature control range
    /// (`switch_on_temperature` to `switch_off_temperature`). The result is a float
    /// between `0.0` and `1.0`. A value of `0.0` means the temperature is at or below
    /// `switch_on_temperature` (full heating needed), and `1.0` means it's at or above
    /// `switch_off_temperature` (no heating needed).
    ///
    /// # Arguments
    /// * `measured_value` - The current measured temperature (`f32`).
    ///
    /// # Returns
    /// An `f32` representing the normalized control deviation, ranging from `0.0` to `1.0`.
    /// Returns `0.0` if `switch_off_temperature` is not greater than `switch_on_temperature`
    /// (i.e., `target_value_delta` is not positive), indicating an invalid configuration.
    pub fn calc_normalized_control_deviation(&self, measured_value: f32) -> f32 {
        let target_value_delta: f32 =
            self.config.switch_off_temperature - self.config.switch_on_temperature;
        let measured_value_delta: f32 = measured_value - self.config.switch_on_temperature;
        //#[cfg(test)]
        //println!(
        //    "calc_normalized_control_deviation: measured_value={}, measured_value_delta={}, target_value_delta={}",
        //    measured_value, measured_value_delta, target_value_delta
        //);
        if target_value_delta > 0.0 {
            measured_value_delta / target_value_delta
        } else {
            0.0 // default value when target values are not properly set up.
        }
    }

    /// Retrieves the duration in seconds until the next midnight from the database.
    ///
    /// This private helper function acts as a robust wrapper around the SQL interface's
    /// `get_duration_until_midnight` method.
    ///
    /// # Returns
    /// An `i64` representing the number of seconds remaining until the next midnight.
    ///
    /// # Errors
    /// This function handles errors internally. If the database call fails, it logs
    /// the error and returns a default fallback value of 24 hours (86.400 seconds).
    /// This prevents a crash and suspends statistics recording until the next day.
    ///
    /// # Arguments
    /// * `sql_interface_heating` - Database interface connection for heating stats.
    /// * `is_multi_thread` - A boolean flag indicating if the Tokio runtime is running in multithreaded mode.
    pub fn get_duration_until_midnight(
        &mut self,
        sql_interface_heating: &mut SqlInterfaceHeatingStats,
        is_multi_thread: bool,
    ) -> u64 {
        let default_value: u64 = 24 * 3600; // default value for time in seconds until midnight

        let get_midnight_res = if is_multi_thread {
            tokio::task::block_in_place(|| sql_interface_heating.get_duration_until_midnight())
        } else {
            sql_interface_heating.get_duration_until_midnight()
        };

        #[allow(clippy::unnecessary_lazy_evaluations)]
        let database_seconds_until_midnight = get_midnight_res.unwrap_or_else(|_e| {
            let context = format!(
                "Could not get duration until midnight from database - using default value ({}).",
                default_value
            );
            log_error_chain(
                // not required during testing
                module_path!(),
                &context,
                _e,
            );
            default_value as i64
        });
        if database_seconds_until_midnight < 0 {
            error!(
                target: module_path!(),
                "Database informed invalid time until midnight - using default value ({}).",
                default_value
            );
            default_value
        } else {
            database_seconds_until_midnight as u64
        }
    }

    /// Updates the running daily statistics based on the current cycle's inputs.
    ///
    /// This private helper function is called once per `execute` loop cycle to accumulate
    /// heating statistics. It calculates the energy consumed and the time elapsed since
    /// the last update, and then calls the `update` method on the `HeatingStatsEntry`
    /// to add these values to the daily running totals for energy, average temperatures,
    /// and runtime.
    ///
    /// If sensor readings are marked as invalid (`measurement_error` is true), the
    /// temperature values are passed as `None` to the statistics entry, ensuring they
    /// are not included in the daily average calculation.
    ///
    /// # Arguments
    /// * `heating_stats_opt` - A mutable `Option` holding the `HeatingStatsEntry` for the
    ///   current day. The function does nothing if this is `None`.
    /// * `measurement_error` - A boolean flag indicating if the current sensor readings
    ///   are valid.
    /// * `measured_water_temperature` - The current water temperature reading.
    /// * `measured_ambient_temperature` - The current ambient temperature reading.
    /// * `heater_state` - The current `ActuatorState` of the heater (`On` or `Off`), used
    ///   to calculate energy consumption.
    pub fn update_heating_stats(
        &mut self,
        heating_stats_opt: &mut Option<HeatingStatsEntry>,
        measurement_error: bool,
        measured_water_temperature: f32,
        measured_ambient_temperature: f32,
        heater_state: &ActuatorState,
    ) {
        // time to update the stats
        if let Some(ref mut heating_stats) = heating_stats_opt {
            // check if there is a recording of the last stats update
            let increment_fraction =
                if let Some(last_stats_update_instant) = self.last_stats_update_instant_opt {
                    last_stats_update_instant.elapsed().as_millis() as f64 / 1000.0
                } else {
                    0.0
                };
            self.last_stats_update_instant_opt = Some(Instant::now());

            // wrap temperatures depending on the status bit
            let water_temperature_opt =
                (!measurement_error).then_some(measured_water_temperature as f64);
            let ambient_temperature_opt =
                (!measurement_error).then_some(measured_ambient_temperature as f64);
            // update the heating statistics with recently acquired data
            let energy_increment = if *heater_state == ActuatorState::On {
                (self.config.heater_power * increment_fraction as f32 / 3600.0) as f64
            } else {
                0.0
            };
            heating_stats.update(
                water_temperature_opt,
                ambient_temperature_opt,
                energy_increment,
                increment_fraction,
            );
        }
    }

    /// Checks if midnight has passed and, if so, records the daily heating statistics.
    ///
    /// This private helper function is called once per `execute` loop cycle. It compares
    /// the current time against a pre-calculated `midnight_instant`. When the current
    /// time is past midnight, it orchestrates the end-of-day statistics logging process.
    ///
    /// This involves taking ownership of the completed `HeatingStatsEntry`, passing it
    /// to the `heating_stats_transfer` object for persistence in the database, and then
    /// creating a fresh `HeatingStatsEntry` to begin accumulating stats for the new day.
    /// It also recalculates and sets the `midnight_instant` for the following day.
    ///
    /// # Arguments
    /// * `midnight_instant` - A mutable `Instant` that marks the moment of midnight.
    ///   This function will update it to the next day's midnight after a transfer.
    /// * `heating_stats_opt` - A mutable `Option` holding the `HeatingStatsEntry` for the
    ///   current day. The function will `take()` the value upon transfer and re-initialize it.
    /// * `heating_stats_transfer` - A mutable reference to an object implementing the
    ///   `HeatingStatsDataTransferTrait`, used to perform the actual database write.
    /// * `sql_interface_heating` - A mutable reference to the SQL interface, required for
    ///   creating a new `HeatingStatsEntry` and for calculating the time until the next midnight.
    /// * `is_multi_thread` - A boolean flag indicating if the Tokio runtime is running in multithreaded mode.
    pub fn record_heating_stats(
        &mut self,
        midnight_instant: &mut Instant,
        heating_stats_opt: &mut Option<HeatingStatsEntry>,
        heating_stats_transfer: &mut impl HeatingStatsDataTransferTrait,
        sql_interface_heating: &mut SqlInterfaceHeatingStats,
        is_multi_thread: bool,
    ) {
        // check if midnight has arrived
        if Instant::now() > *midnight_instant {
            println!("***  Transferring heating stats ***");
            // Use take() to move the value out of the Option without cloning.
            if let Some(stats) = heating_stats_opt.take() {
                let transfer_res = if is_multi_thread {
                    tokio::task::block_in_place(|| {
                        heating_stats_transfer.transfer_heating_stats(stats, sql_interface_heating)
                    })
                } else {
                    heating_stats_transfer.transfer_heating_stats(stats, sql_interface_heating)
                };

                if let Err(e) = transfer_res {
                    error!(
                        target: module_path!(),
                        "Error occurred when trying to store heating stats data in database: {e:?}"
                    );
                }
                // Reset heating stats
                let new_stats = if is_multi_thread {
                    tokio::task::block_in_place(|| {
                        HeatingStatsEntry::new(&mut sql_interface_heating.conn)
                    })
                } else {
                    HeatingStatsEntry::new(&mut sql_interface_heating.conn)
                };
                *heating_stats_opt = Some(new_stats);
                *midnight_instant = Instant::now()
                    + Duration::from_secs(
                        self.get_duration_until_midnight(sql_interface_heating, is_multi_thread),
                    );
            }
        }
    }

    /// Executes the **main control loop for the heating module**.
    ///
    /// This function runs continuously, managing the aquarium's heating system until a termination
    /// signal is received. It adjusts heater operation based on **water temperature** and
    /// **tank level switch signals** adhering to the limitations imposed by a **schedule**.
    /// The **ambient temperature** signal is considered for logging daily statistical data.
    /// It also processes external `Start`/`Stop` commands, applies a
    /// **sawtooth profile for proportional control**, and **logs daily heating statistics**.
    ///
    /// **Key Operations:**
    /// - **Sensor Data Acquisition**: Reads current water temperature, ambient temperature, and tank level switch states from shared mutexes.
    /// - **Safety Interlocks**: Implements a critical safety feature to turn off the heater if the water level is too low for a configured duration, preventing damage.
    /// - **Schedule & External Control**: Checks with the `ScheduleCheck` module and responds to `Start`/`Stop` commands to inhibit or enable heating.
    /// - **Proportional Control**: When within the active temperature range, uses a `SawToothProfile` to achieve a "PWM-like" heating effect.
    /// - **Energy Logging**: Periodically updates and transfers daily heating statistics (energy consumption, average temperatures) to the SQL database.
    /// - **Cycle Management**: Maintains a fixed cycle time, sleeping as necessary, and warning if execution duration exceeds the cycle limit.
    /// - **Graceful Shutdown**: Responds to `Quit` and `Terminate` commands from the signal handler, ensuring a safe shutdown of the heater and final data logging.
    ///
    /// # Arguments
    /// * `mutex_device_scheduler_heating` - A clone of the `Arc<Mutex<i32>>` used to coordinate device
    ///   actuation across the application, preventing conflicts and tracking activity counts.
    /// * `heating_channels` - A struct containing all sender and receiver channels for communication
    ///   with modules like `RelayManager`, `SignalHandler`, `DataLogger`, `ScheduleCheck`, and `Messaging`.
    /// * `heating_stats_transfer` - A mutable reference to an object implementing `HeatingStatsDataTransferTrait`,
    ///   responsible for persisting daily heating statistics to the database.
    /// * `heating_set_val_updater` - A mutable reference to an object implementing `ThermalSetValueUpdaterTrait`,
    ///   responsible for updating the set values dynamically by communicating with SQL database.
    /// * `heating_mutexes` - Struct containing the mutexes to access data from different threads.
    /// * `sql_interface_heating` - A `SqlInterfaceHeatingStats` instance providing direct interface to database.
    /// * `is_multi_thread` - A boolean indicating if the tokio runtime is running in multithreaded mode.
    ///
    /// # Returns
    /// This function does not return a value in the traditional sense, as it is designed
    /// to loop indefinitely. It will only break out of its loop and terminate when a `Quit`
    /// command is received from the signal handler, after which it performs final cleanup
    /// (switching off the heater, logging final stats) and confirms shutdown.
    #[allow(clippy::too_many_arguments)]
    /// [impl->swreq~thermal_heater_heating~1]
    ///
    /// [impl->swreq~thermal_inhibit_heating_low_water~1]
    pub async fn execute(
        &mut self,
        mutex_device_scheduler_heating: Arc<tokio::sync::Mutex<i32>>,
        heating_channels: &mut HeatingChannels,
        heating_stats_transfer: &mut impl HeatingStatsDataTransferTrait,
        heating_set_val_updater: &mut impl ThermalSetValueUpdaterTrait,
        heating_mutexes: HeatingMutexes,
        mut sql_interface_heating: SqlInterfaceHeatingStats,
        is_multi_thread: bool,
    ) {
        #[cfg(all(target_os = "linux", not(test), not(feature = "test-utils")))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let _refill_error = false;
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let mut saw_tooth_profile = SawToothProfile::new(&self.config.saw_tooth_profile_config);
        let mut measured_water_temperature = 0.0;
        let mut measurement_error: bool;
        let mut measured_ambient_temperature = 0.0;
        let mut measured_tank_level_switch_position = true;
        let mut measured_tank_level_switch_invalid = false;
        let mut loop_counter = 0;
        let mut heater_state: ActuatorState = ActuatorState::Undefined;
        let mut schedule_check_result: bool;
        let duration_until_midnight = Duration::from_secs(
            self.get_duration_until_midnight(&mut sql_interface_heating, is_multi_thread),
        );
        let mut midnight_instant = Instant::now() + duration_until_midnight;
        let mut tank_level_low_counter = 0;
        let mut heating_inhibited: bool = false; // state of heating control determined by if the start/stop command has been received
        let mut lock_warn_cycle_time_exceeded: bool = false;
        let mut actuation_mutex_was_blocked_during_actuation: bool = false;
        let cycle_time_duration = Duration::from_millis(CYCLE_TIME_HEATING_MILLIS);
        let mut actuator_ineffective: bool = false;

        let get_heating_stats_res = if is_multi_thread {
            tokio::task::block_in_place(|| {
                sql_interface_heating.get_single_heating_stats_from_database()
            })
        } else {
            sql_interface_heating.get_single_heating_stats_from_database()
        };

        let mut heating_stats_opt = match get_heating_stats_res {
            Ok(c) => Some(c),
            Err(_) => {
                let entry = if is_multi_thread {
                    tokio::task::block_in_place(|| {
                        HeatingStatsEntry::new(&mut sql_interface_heating.conn)
                    })
                } else {
                    HeatingStatsEntry::new(&mut sql_interface_heating.conn)
                };
                Some(entry)
            }
        };

        let mut start_time = Instant::now();
        loop {
            let (
                quit_command_received,  // the request to end the application has been received
                start_command_received, // the request to (re-)start heating has been received
                stop_command_received, // the request to (temporarily) stop heating has been received
            ) = self.process_external_request(
                &mut heating_channels.rx_heating_from_signal_handler,
                Some(&mut heating_channels.rx_heating_from_messaging),
            );
            if quit_command_received {
                break;
            }
            // [impl->swreq~thermal_remote_heating_stop~1]
            if stop_command_received {
                #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                info!(
                    target: module_path!(),
                    "received Stop command. inhibiting heating."
                );

                heating_inhibited = true;
            }
            // [impl->swreq~thermal_remote_heating_start~1]
            if start_command_received {
                #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                info!(
                    target: module_path!(),
                    "received Start command. restarting heating."
                );

                heating_inhibited = false;
            }

            if self.config.active {
                // read signals from sensor manager
                {
                    match heating_mutexes.mutex_sensor_manager_signals.lock() {
                        Ok(c) => {
                            measured_water_temperature = c.water_temperature;
                            measured_ambient_temperature = c.ambient_temperature;
                            measurement_error = false;
                        }
                        Err(_) => {
                            measurement_error = true;
                        }
                    };
                }

                // read tank level switch position and invalid bit from mutex
                {
                    (
                        measured_tank_level_switch_position,
                        measured_tank_level_switch_invalid,
                    ) = match heating_mutexes.mutex_tank_level_switch_signals.lock() {
                        Ok(c) => (c.tank_level_switch_position, c.tank_level_switch_invalid),
                        Err(_) => {
                            (
                                measured_tank_level_switch_position,
                                measured_tank_level_switch_invalid,
                            ) // use previous values
                        }
                    }
                }

                // check if the tank-level switch hints to potentially low water level
                if (measured_tank_level_switch_invalid) || (!measured_tank_level_switch_position) {
                    tank_level_low_counter += 1;
                } else {
                    tank_level_low_counter = 0;
                }

                // schedule check to see if actuation is allowed
                // [impl->swreq~thermal_observe_schedule_heater~1]
                perform_schedule_check!(
                    heating_channels,
                    schedule_check_result,
                    self.lock_error_channel_send_schedule_check,
                    self.lock_error_channel_receive_schedule_check,
                    module_path!()
                );

                // check if set values need to be updated
                update_thermal_set_values!(
                    heating_set_val_updater,
                    self.config.switch_off_temperature,
                    self.config.switch_on_temperature,
                    self.lock_error_heating_set_value_read_failure,
                    module_path!(),
                    is_multi_thread
                );

                actuator_ineffective = self.temperature_gradient_monitor(
                    &heating_mutexes.mutex_temperature_gradient,
                    heater_state == ActuatorState::On,
                    true, // heating expects a positive gradient
                    "Heating",
                );
                if actuator_ineffective
                    && self.config.switch_off_when_no_effect
                    && heater_state == ActuatorState::On
                {
                    switch_off_heater(
                        &mutex_device_scheduler_heating,
                        &mut actuation_mutex_was_blocked_during_actuation,
                        heating_channels,
                        is_multi_thread,
                    )
                    .await;
                    heater_state = ActuatorState::Off;
                    self.actuator_disabled = true;
                    #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                    #[cfg(not(tarpaulin_include))]
                    warn!("[{}]: Disabled heater because temperature gradient monitor indicates ineffectiveness.", module_path!());
                }

                if self.config.stop_on_refill_error
                    && (tank_level_low_counter > self.config.stop_on_refill_error_delay)
                {
                    // tank level switch position has not been available or indicating low water level for too long time
                    // switch off the heater if not already off to protect against overheating of the device
                    if heater_state != ActuatorState::Off {
                        switch_off_heater(
                            &mutex_device_scheduler_heating,
                            &mut actuation_mutex_was_blocked_during_actuation,
                            heating_channels,
                            is_multi_thread,
                        )
                        .await;
                        heater_state = ActuatorState::Off;
                    }
                } else {
                    // component protection allows operation of the heater
                    if schedule_check_result && !heating_inhibited {
                        // Schedule check said that actuation is allowed.
                        // Also, there is no inhibition from external request.
                        // The main control happens here (comparison measured/target, actuation).
                        if (measured_water_temperature >= self.config.switch_on_temperature)
                            && (measured_water_temperature <= self.config.switch_off_temperature)
                        {
                            // temperature is inside (dynamic) control window
                            if (self.calc_normalized_control_deviation(measured_water_temperature)
                                < saw_tooth_profile.level_normalized)
                                && !measurement_error
                            {
                                // switch on the heater if not already on
                                if heater_state != ActuatorState::On && !self.actuator_disabled {
                                    switch_on_heater(
                                        &mutex_device_scheduler_heating,
                                        &mut actuation_mutex_was_blocked_during_actuation,
                                        heating_channels,
                                        is_multi_thread,
                                    )
                                    .await;
                                    heater_state = ActuatorState::On;
                                }
                            } else {
                                // switch off the heater if not already off
                                if heater_state != ActuatorState::Off {
                                    switch_off_heater(
                                        &mutex_device_scheduler_heating,
                                        &mut actuation_mutex_was_blocked_during_actuation,
                                        heating_channels,
                                        is_multi_thread,
                                    )
                                    .await;
                                    heater_state = ActuatorState::Off;
                                }
                            }
                        } else {
                            // the temperature is either above or below (dynamic) control window
                            if measured_water_temperature > self.config.switch_off_temperature {
                                // switch off the heater if not already off
                                if heater_state != ActuatorState::Off {
                                    switch_off_heater(
                                        &mutex_device_scheduler_heating,
                                        &mut actuation_mutex_was_blocked_during_actuation,
                                        heating_channels,
                                        is_multi_thread,
                                    )
                                    .await;
                                    heater_state = ActuatorState::Off;
                                }
                            }
                            if measured_water_temperature < self.config.switch_on_temperature {
                                // switch on the heater if not already on
                                if heater_state != ActuatorState::On && !self.actuator_disabled {
                                    switch_on_heater(
                                        &mutex_device_scheduler_heating,
                                        &mut actuation_mutex_was_blocked_during_actuation,
                                        heating_channels,
                                        is_multi_thread,
                                    )
                                    .await;
                                    heater_state = ActuatorState::On;
                                }
                            }
                        }
                    } else if heating_inhibited {
                        // the external request to stop heating control has been received
                        match self.config.switch_on_when_external_stop {
                            true => {
                                // switch on the heater if not already on
                                if heater_state != ActuatorState::On {
                                    switch_on_heater(
                                        &mutex_device_scheduler_heating,
                                        &mut actuation_mutex_was_blocked_during_actuation,
                                        heating_channels,
                                        is_multi_thread,
                                    )
                                    .await;
                                    heater_state = ActuatorState::On;
                                }
                            }
                            false => {
                                // switch off the heater if not already off
                                if heater_state != ActuatorState::Off {
                                    switch_off_heater(
                                        &mutex_device_scheduler_heating,
                                        &mut actuation_mutex_was_blocked_during_actuation,
                                        heating_channels,
                                        is_multi_thread,
                                    )
                                    .await;
                                    heater_state = ActuatorState::Off;
                                }
                            }
                        }
                    } else {
                        // schedule check said that we are not allowed to actuate
                        match self.config.switch_on_when_out_of_schedule {
                            true => {
                                // switch on the heater if not already on
                                if heater_state != ActuatorState::On {
                                    switch_on_heater(
                                        &mutex_device_scheduler_heating,
                                        &mut actuation_mutex_was_blocked_during_actuation,
                                        heating_channels,
                                        is_multi_thread,
                                    )
                                    .await;
                                    heater_state = ActuatorState::On;
                                }
                            }
                            false => {
                                // switch off the heater if not already off
                                if heater_state != ActuatorState::Off {
                                    switch_off_heater(
                                        &mutex_device_scheduler_heating,
                                        &mut actuation_mutex_was_blocked_during_actuation,
                                        heating_channels,
                                        is_multi_thread,
                                    )
                                    .await;
                                    heater_state = ActuatorState::Off;
                                }
                            }
                        }
                    }
                }

                saw_tooth_profile.execute_with(TIME_INCREMENT_HEATING_SECS);

                // Update the heating statistics
                self.update_heating_stats(
                    &mut heating_stats_opt,
                    measurement_error,
                    measured_water_temperature,
                    measured_ambient_temperature,
                    &heater_state,
                );

                // Transfer the heating statistics to the database once per day
                self.record_heating_stats(
                    &mut midnight_instant,
                    &mut heating_stats_opt,
                    heating_stats_transfer,
                    &mut sql_interface_heating,
                    is_multi_thread,
                );
            }

            let instant_before_locking_mutex = Instant::now();
            let mut instant_after_locking_mutex = Instant::now(); // initialization is overwritten

            // Write signal to mutex
            {
                match heating_mutexes.mutex_heating_status.lock() {
                    Ok(mut heating_status) => {
                        instant_after_locking_mutex = Instant::now();
                        heating_status.actuator_switched_on = match heater_state {
                            ActuatorState::On => true,
                            ActuatorState::Off => false,
                            _ => false,
                        };
                        heating_status.actuator_disabled = self.actuator_disabled;
                        heating_status.actuator_ineffective = actuator_ineffective;
                    }
                    Err(_) => {
                        // Do nothing
                    }
                }
            }

            // check if access to mutex took too long
            self.check_mutex_access_duration(
                None,
                instant_after_locking_mutex,
                instant_before_locking_mutex,
            );

            loop_counter += 1;

            // avoid overflow
            if loop_counter == 1_000_000 {
                loop_counter = 0;
            }

            // Manage the loop's cycle time, sleeping or logging a warning as needed.
            let stop_time = Instant::now();
            let execution_duration = stop_time.duration_since(start_time);

            if execution_duration > cycle_time_duration {
                if !lock_warn_cycle_time_exceeded && !actuation_mutex_was_blocked_during_actuation {
                    #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                    #[cfg(not(tarpaulin_include))]
                    warn!(target: module_path!(),
                        "execution duration of {} milliseconds exceeds cycle time of {}",
                        execution_duration.as_millis(),
                        CYCLE_TIME_HEATING_MILLIS);
                    lock_warn_cycle_time_exceeded = true;
                }
            } else {
                let remaining_sleep_time = cycle_time_duration - execution_duration;
                tokio::time::sleep(remaining_sleep_time).await;
                lock_warn_cycle_time_exceeded = false;
            }

            // Reset the actuation block flag for the next cycle.
            actuation_mutex_was_blocked_during_actuation = false;

            // Reset the start time for the next cycle.
            start_time = Instant::now();

            self.check_timing_and_ping_database(&mut sql_interface_heating, is_multi_thread);
        }

        // switch off the heater if not already off
        if heater_state != ActuatorState::Off {
            switch_off_heater(
                &mutex_device_scheduler_heating,
                &mut actuation_mutex_was_blocked_during_actuation,
                heating_channels,
                is_multi_thread,
            )
            .await;
        }

        // update the database with the heating stats data recorded
        // so far for this date
        if let Some(stats) = heating_stats_opt.take() {
            let transfer_res = if is_multi_thread {
                tokio::task::block_in_place(|| {
                    heating_stats_transfer.transfer_heating_stats(stats, &mut sql_interface_heating)
                })
            } else {
                heating_stats_transfer.transfer_heating_stats(stats, &mut sql_interface_heating)
            };

            if let Err(e) = transfer_res {
                error!("Error occurred when trying to store heating stats data in database (during termination): {e:?}");
            }
        }

        heating_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads.
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination_async(
            &mut heating_channels.rx_heating_from_signal_handler,
            sleep_duration_hundred_millis,
            module_path!(),
        )
        .await;
    }
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub mod tests {
    use super::*;
    use crate::utilities::sawtooth_profile::SawToothProfileConfig;

    fn create_dummy_config(switch_on: f32, switch_off: f32) -> HeatingConfig {
        HeatingConfig {
            active: true,
            execute: true,
            switch_on_temperature: switch_on,
            switch_off_temperature: switch_off,
            stop_on_refill_error: false,
            stop_on_refill_error_delay: 2,
            saw_tooth_profile_config: SawToothProfileConfig::new(6, 1, 6, 1),
            heater_power: 300.0,
            switch_on_when_out_of_schedule: false,
            switch_on_when_external_stop: false,
            set_value_check_interval: 10,
            max_time_actuator_effect_monitor: 60,
            switch_off_when_no_effect: false,
        }
    }

    #[test]
    // Verifies calc_normalized_control_deviation calculates normalized ratios for valid limits and returns 0.0 for invalid configs.
    // [utest->swreq~thermal_read_heating_limits~1]
    fn test_calc_normalized_control_deviation_valid_and_invalid_configs() {
        let config = create_dummy_config(22.0, 26.0);
        let heating = Heating::new(config, Duration::from_secs(3600));

        // At switch_on: (22 - 22) / 4 = 0.0
        assert_eq!(heating.calc_normalized_control_deviation(22.0), 0.0);
        // At midpoint: (24 - 22) / 4 = 0.5
        assert_eq!(heating.calc_normalized_control_deviation(24.0), 0.5);
        // At switch_off: (26 - 22) / 4 = 1.0
        assert_eq!(heating.calc_normalized_control_deviation(26.0), 1.0);

        // Invalid config: switch_off <= switch_on
        let invalid_config = create_dummy_config(25.0, 25.0);
        let invalid_heating = Heating::new(invalid_config, Duration::from_secs(3600));
        assert_eq!(invalid_heating.calc_normalized_control_deviation(26.0), 0.0);
    }

    #[test]
    // Verifies get_duration_until_midnight returns default 86400 seconds when database returns negative values or error.
    // [utest->swreq~thermal_read_heating_limits~1]
    fn test_get_duration_until_midnight_handles_negative_values() {
        use crate::database::sql_interface::SqlInterface;
        use crate::mocks::mock_heating_stats::tests::SqlInterfaceHeatingMockMidnightCalculator;
        use crate::utilities::config::read_config_file_with_test_database;

        let config_data = read_config_file_with_test_database(
            "config/aquarium_control_test_generic.toml".to_string(),
            1,
        );
        let sql_interface = SqlInterface::new(config_data.sql_interface.clone()).unwrap();
        let conn = sql_interface.get_connection().unwrap();

        let mock_calc = Box::new(SqlInterfaceHeatingMockMidnightCalculator::new(-100));
        let mut sql_stats = SqlInterfaceHeatingStats::new(conn, 10, mock_calc).unwrap();

        let mut heating = Heating::new(config_data.heating, Duration::from_secs(3600));
        let duration = heating.get_duration_until_midnight(&mut sql_stats, false);
        assert_eq!(duration, 86400);
    }

    #[test]
    // Verifies update_heating_stats processes measurement error and calculates energy increments correctly for On and Off states.
    // [utest->swreq~thermal_read_heating_limits~1]
    fn test_update_heating_stats_measurement_error_and_energy() {
        use crate::database::sql_interface::SqlInterface;
        use crate::utilities::config::read_config_file_with_test_database;

        let config_data = read_config_file_with_test_database(
            "config/aquarium_control_test_generic.toml".to_string(),
            1,
        );
        let sql_interface = SqlInterface::new(config_data.sql_interface.clone()).unwrap();
        let mut conn = sql_interface.get_connection().unwrap();

        let mut heating = Heating::new(config_data.heating, Duration::from_secs(3600));
        let mut stats_entry = Some(HeatingStatsEntry::new(&mut conn));

        // First update initializes last_stats_update_instant_opt
        heating.update_heating_stats(&mut stats_entry, false, 24.5, 22.0, &ActuatorState::On);
        assert!(heating.last_stats_update_instant_opt.is_some());

        // Second update with measurement_error = true
        heating.update_heating_stats(&mut stats_entry, true, 24.5, 22.0, &ActuatorState::Off);
    }
}
