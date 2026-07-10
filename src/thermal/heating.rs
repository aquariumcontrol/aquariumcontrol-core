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
#[cfg(not(test))] // only required in real-life execution
use log::{info, warn};

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(all(target_os = "linux", not(test)))] // only required in real-life execution
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

const CYCLE_TIME_HEATING_MILLIS: u64 = 500;

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
    config: HeatingConfig,

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
    #[cfg(test)]
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
    pub fn new(config: HeatingConfig, database_ping_interval: Duration) -> Heating {
        Self {
            config,
            lock_error_channel_send_schedule_check: false,
            lock_error_channel_receive_schedule_check: false,
            last_ping_instant: Instant::now(),
            database_ping_interval,
            lock_warn_max_mutex_access_duration: false,
            lock_error_heating_set_value_read_failure: false,
            #[cfg(test)]
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
    fn calc_normalized_control_deviation(&self, measured_value: f32) -> f32 {
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
    fn get_duration_until_midnight(
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

        let database_seconds_until_midnight = get_midnight_res.unwrap_or_else(|e| {
            let context = format!(
                "Could not get duration until midnight from database - using default value ({}).",
                default_value
            );
            log_error_chain(module_path!(), &context, e);
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
    fn update_heating_stats(
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
    fn record_heating_stats(
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
    pub async fn execute(
        &mut self,
        mutex_device_scheduler_heating: Arc<Mutex<i32>>,
        heating_channels: &mut HeatingChannels,
        heating_stats_transfer: &mut impl HeatingStatsDataTransferTrait,
        heating_set_val_updater: &mut impl ThermalSetValueUpdaterTrait,
        heating_mutexes: HeatingMutexes,
        mut sql_interface_heating: SqlInterfaceHeatingStats,
        is_multi_thread: bool,
    ) {
        #[cfg(all(target_os = "linux", not(test)))] // only required in real-life execution
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
            if stop_command_received {
                #[cfg(not(test))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                info!(
                    target: module_path!(),
                    "received Stop command. inhibiting heating."
                );

                heating_inhibited = true;
            }
            if start_command_received {
                #[cfg(not(test))] // only required in real-life execution
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
                    #[cfg(not(test))] // only required in real-life execution
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
                    );
                    heater_state = ActuatorState::Off;
                    self.actuator_disabled = true;
                    #[cfg(not(test))] // only required in real-life execution
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
                        );
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
                                    );
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
                                    );
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
                                    );
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
                                    );
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
                                    );
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
                                    );
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
                                    );
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
                                    );
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
                    #[cfg(not(test))] // only required in real-life execution
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
            );
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
pub mod tests {
    /// Helper function to execute the heating task inside a test Tokio runtime.
    ///
    /// # Arguments
    /// * `heating` - A mutable reference to the `Heating` instance.
    /// * `mutex_device_scheduler_heating` - Mutex coordinating access to the device scheduler.
    /// * `heating_channels` - A mutable reference to the `HeatingChannels` instance.
    /// * `heating_stats_transfer` - A mutable reference to the heating stats transfer object.
    /// * `heating_set_val_updater` - A mutable reference to the set value updater.
    /// * `heating_mutexes` - Shared mutexes for heating conditions.
    /// * `sql_interface_heating` - Database interface for heating stats.
    fn execute_blocking(
        heating: &mut Heating,
        mutex_device_scheduler_heating: Arc<Mutex<i32>>,
        heating_channels: &mut super::HeatingChannels,
        heating_stats_transfer: &mut impl super::HeatingStatsDataTransferTrait,
        heating_set_val_updater: &mut impl crate::database::thermal_set_value_updater_trait::ThermalSetValueUpdaterTrait,
        heating_mutexes: HeatingMutexes,
        sql_interface_heating: SqlInterfaceHeatingStats,
    ) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let is_multi_thread_test = tokio::runtime::Handle::try_current()
                    .map(|h| {
                        matches!(
                            h.runtime_flavor(),
                            tokio::runtime::RuntimeFlavor::MultiThread
                        )
                    })
                    .unwrap_or(false);

                heating
                    .execute(
                        mutex_device_scheduler_heating,
                        heating_channels,
                        heating_stats_transfer,
                        heating_set_val_updater,
                        heating_mutexes,
                        sql_interface_heating,
                        is_multi_thread_test,
                    )
                    .await;
            });
    }

    use crate::database::sql_interface::SqlInterface;
    use crate::database::sql_interface_heating_stats::{
        HeatingStatsEntry, SqlInterfaceHeatingStats,
    };
    use crate::launch::channels::Channels;
    use crate::mocks::mock_heating::tests::MockSqlInterfaceHeatingSetVals;
    use crate::mocks::mock_relay_manager::tests::mock_relay_manager;
    use crate::mocks::mock_schedule_check::tests::mock_schedule_check;
    use crate::sensors::sensor_manager::SensorManagerSignals;
    use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
    use crate::thermal::heating::{Heating, CYCLE_TIME_HEATING_MILLIS};
    use crate::thermal::heating_mutexes::HeatingMutexes;
    use crate::utilities::channel_content::{ActuatorState, AquariumDevice, InternalCommand};
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};
    use crate::utilities::sawtooth_profile::SawToothProfileConfig;
    use all_asserts::{assert_ge, assert_le};
    use assert_float_eq::assert_float_absolute_eq;
    use chrono::Local;

    use crate::mocks::mock_heating_stats::tests::{
        HeatingStatsMockDataTransfer, SqlInterfaceHeatingMockMidnightCalculator,
    };
    use spin_sleep::SpinSleeper;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::database::sql_query_strings::SQL_TABLE_HEATING_STATS;
    use crate::thermal::thermal_control_status::ThermalControlStatus;

    // Prepares a `Heating` control instance with a mock midnight calculator and a clean test database.
    //
    // This helper function is designed exclusively for testing the `Heating` module. It sets up
    // a controlled environment by:
    // 1.  Loading a generic test configuration and overriding specific settings (e.g., `stop_on_refill_error_delay`).
    // 2.  Optionally, applying a custom `SawToothProfileConfig` for precise control over heating patterns.
    // 3.  Connecting to and **truncating a dedicated test database** to ensure a clean state for heating statistics.
    // 4.  Injecting a **`SqlInterfaceHeatingMockMidnightCalculator`** to return a predefined `seconds_to_midnight`,
    //     allowing deterministic testing of daily statistic transfers without real-time dependency.
    //
    // # Arguments
    // * `seconds_to_midnight` - The fixed number of seconds (as `i64`) the mock midnight calculator will return.
    //   This directly controls when the `Heating` module attempts to transfer daily statistics.
    // * `test_db_number` - A unique `u32` identifier (e.g., `7`, `8`, `9`) used to select a specific
    //   test database (`aquarium_test_XX`) from the configuration.
    // * `saw_tooth_profile_config_opt` - An `Option<SawToothProfileConfig>`. If `Some`, this custom
    //   sawtooth profile configuration overrides the one loaded from the TOML file for the `Heating` module.
    //
    // # Returns
    // A tuple `(ConfigData, Heating)`:
    // - The `ConfigData` struct, containing the loaded and modified configuration.
    // - The fully initialized `Heating` struct, ready for test execution.
    //
    // # Panics
    // This function will panic if:
    // - `read_config_file_with_test_database` fails (e.g., file not found, invalid DB number).
    // - It fails to connect to the SQL database.
    // - It fails to truncate the `heatingstats` table in the database.
    // - It encounters any issue during the initialization of the `SqlInterfaceHeating` or `Heating` structs.
    /// Performs the `prepare_heating_tests_mock_midnight_calculator` operation.
    ///
    /// # Arguments
    /// * `seconds_to_midnight` - A parameter representing `seconds_to_midnight`.
    /// * `test_db_number` - A parameter representing `test_db_number`.
    /// * `saw_tooth_profile_config_opt` - A parameter representing `saw_tooth_profile_config_opt`.
    pub fn prepare_heating_tests_mock_midnight_calculator(
        seconds_to_midnight: i64,
        test_db_number: u32,
        saw_tooth_profile_config_opt: Option<SawToothProfileConfig>,
    ) -> (ConfigData, Heating, SqlInterface, SqlInterfaceHeatingStats) {
        // prepare struct Heating for further tests
        let mut config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            test_db_number,
        );
        config.heating.stop_on_refill_error_delay = 2;

        if let Some(saw_tooth_profile_config) = saw_tooth_profile_config_opt {
            config.heating.saw_tooth_profile_config = saw_tooth_profile_config;
        }

        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };

        // empty the heating stats table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_STATS.to_string())
        {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let sql_interface_heating_midnight_calculator = Box::new(
            SqlInterfaceHeatingMockMidnightCalculator::new(seconds_to_midnight),
        );
        let sql_interface_heating_stats = SqlInterfaceHeatingStats::new(
            sql_interface.get_connection().unwrap(),
            config.sql_interface.max_rows_heating_stats,
            sql_interface_heating_midnight_calculator,
        )
        .unwrap();

        println!(
            "prepare_heating_tests_mock_midnight_calculator: SwitchOff temperature = {}, SwitchOn temperature = {}",
            config.heating.switch_on_temperature, config.heating.switch_off_temperature
        );
        let heating_config = config.heating.clone();
        (
            config,
            Heating::new(heating_config, Duration::from_secs(1000)),
            sql_interface,
            sql_interface_heating_stats,
        )
    }

    #[test]
    // Check if the heater remains switched off with a high temperature.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Test case uses test database #11.
    pub fn test_heating_with_measured_temperature_high() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3600, 11, None);

        // replacement values are used to initialize the mutexes
        config.sensor_manager.replacement_value_water_temperature = 30.0;
        config.sensor_manager.replacement_value_ambient_temperature = 24.0;

        let mut channels = Channels::new_for_test();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
                assert_eq!(actuation_events.len(), 1);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
                mock_actuator_states.check_terminal_condition_heating();
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater =
                    MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                        &config.sensor_manager,
                    ))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(
                    &mut heating,
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking reaction to high temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Check if the heater is switched on and is not switched off with low temperature.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Check if the heater is switched off after sending Quit command and before sending Terminate command.
    // Test case uses test database #12.
    pub fn test_heating_with_measured_temperature_low_without_mutex_blocked() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3600, 12, None);

        // replacement values are used to initialize the mutexes
        config.sensor_manager.replacement_value_water_temperature = 20.0;
        config.sensor_manager.replacement_value_ambient_temperature = 24.0;

        let mut channels = Channels::new_for_test();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
                assert_eq!(actuation_events.len(), 2);
                let last_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    last_actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
                let first_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    first_actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Heater)
                );
                mock_actuator_states.check_terminal_condition_heating();
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater =
                    MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                        &config.sensor_manager,
                    ))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(
                    &mut heating,
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking reaction to low temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Check if the heater is switched on and off during two cycles of the saw tooth profile.
    // The duration of the heater being switched on shall equal the duration of the heater switched off (1% tolerance).
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Test case uses test database #13.
    pub fn test_heating_with_measured_temperature_50_percent() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(
                3600,
                13,
                Some(SawToothProfileConfig::new(6, 1, 6, 1)),
            );

        // replacement values are used to initialize the mutexes
        config.sensor_manager.replacement_value_water_temperature = 25.0;
        config.sensor_manager.replacement_value_ambient_temperature = 24.0;

        let mut channels = Channels::new_for_test();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
                println!(
                    "Mock relay manager received {} commands:",
                    actuation_events.len()
                );
                for actuation_event in actuation_events.clone() {
                    println!("{}", actuation_event);
                }
                // (first and) last actuation events need to be ignored
                let _last_actuation_event = actuation_events.pop().unwrap();
                let second_last_actuation_event = actuation_events.pop().unwrap();
                let third_last_actuation_event = actuation_events.pop().unwrap();
                let fourth_last_actuation_event = actuation_events.pop().unwrap();
                let fifth_last_actuation_event = actuation_events.pop().unwrap();
                let sixth_last_actuation_event = actuation_events.pop().unwrap();
                let seventh_last_actuation_event = actuation_events.pop().unwrap();

                let mut min_actuation_duration: u128 = 100000;
                let mut max_actuation_duration: u128 = 0;

                let second_last_actuation_duration = second_last_actuation_event
                    .time
                    .duration_since(third_last_actuation_event.time)
                    .as_millis();

                println!(
                    "second_last_actuation_duration={}",
                    second_last_actuation_duration
                );

                min_actuation_duration = min_actuation_duration.min(second_last_actuation_duration);
                max_actuation_duration = max_actuation_duration.max(second_last_actuation_duration);

                let third_last_actuation_duration = third_last_actuation_event
                    .time
                    .duration_since(fourth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "third_last_actuation_duration={}",
                    third_last_actuation_duration
                );

                min_actuation_duration = min_actuation_duration.min(third_last_actuation_duration);
                max_actuation_duration = max_actuation_duration.max(third_last_actuation_duration);

                let fourth_last_actuation_duration = fourth_last_actuation_event
                    .time
                    .duration_since(fifth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "fourth_last_actuation_duration={}",
                    fourth_last_actuation_duration
                );

                min_actuation_duration = min_actuation_duration.min(fourth_last_actuation_duration);
                max_actuation_duration = max_actuation_duration.max(fourth_last_actuation_duration);

                let fifth_last_actuation_duration = fifth_last_actuation_event
                    .time
                    .duration_since(sixth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "fifth_last_actuation_duration={}",
                    fifth_last_actuation_duration
                );

                min_actuation_duration = min_actuation_duration.min(fifth_last_actuation_duration);
                max_actuation_duration = max_actuation_duration.max(fifth_last_actuation_duration);

                let sixth_last_actuation_duration = sixth_last_actuation_event
                    .time
                    .duration_since(seventh_last_actuation_event.time)
                    .as_millis();

                println!(
                    "sixth_last_actuation_duration={}",
                    sixth_last_actuation_duration
                );

                min_actuation_duration = min_actuation_duration.min(sixth_last_actuation_duration);
                max_actuation_duration = max_actuation_duration.max(sixth_last_actuation_duration);

                let delta_min_max_duration = max_actuation_duration - min_actuation_duration;

                assert_le!(second_last_actuation_duration, 6600);
                assert_ge!(second_last_actuation_duration, 6400);
                assert_le!(third_last_actuation_duration, 6600);
                assert_ge!(third_last_actuation_duration, 6400);
                assert_le!(fourth_last_actuation_duration, 6600);
                assert_ge!(fourth_last_actuation_duration, 6400);
                assert_le!(fifth_last_actuation_duration, 6600);
                assert_ge!(fifth_last_actuation_duration, 6400);
                assert_le!(sixth_last_actuation_duration, 6600);
                assert_ge!(sixth_last_actuation_duration, 6400);

                println!("delta_min_max_duration={}", delta_min_max_duration);
                assert_le!(delta_min_max_duration, 70);
                mock_actuator_states.check_terminal_condition_heating();
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration = Duration::from_secs(59);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater =
                    MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                        &config.sensor_manager,
                    ))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(
                    &mut heating,
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking reaction to medium temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Check if the heater is switched on for most of the time when a measured temperature is low.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Test case uses test database #13.
    pub fn test_heating_with_measured_temperature_low_inside_interval() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(
                3600,
                13,
                Some(SawToothProfileConfig::new(6, 1, 6, 1)),
            );

        // replacement values are used to initialize the mutexes
        config.sensor_manager.replacement_value_water_temperature = 22.5;
        config.sensor_manager.replacement_value_ambient_temperature = 24.0;

        let mut channels = Channels::new_for_test();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
                println!(
                    "Mock relay manager received {} commands:",
                    actuation_events.len()
                );
                for actuation_event in actuation_events.clone() {
                    println!("{}", actuation_event);
                }
                // (first and) last actuation events need to be ignored
                let _last_actuation_event = actuation_events.pop().unwrap();
                let second_last_actuation_event = actuation_events.pop().unwrap();
                let third_last_actuation_event = actuation_events.pop().unwrap();
                let fourth_last_actuation_event = actuation_events.pop().unwrap();
                let fifth_last_actuation_event = actuation_events.pop().unwrap();
                let sixth_last_actuation_event = actuation_events.pop().unwrap();
                let seventh_last_actuation_event = actuation_events.pop().unwrap();

                let second_last_actuation_duration = second_last_actuation_event
                    .time
                    .duration_since(third_last_actuation_event.time)
                    .as_millis();

                println!(
                    "second_last_actuation_duration={}, Command={}",
                    second_last_actuation_duration, second_last_actuation_event.command
                );

                let third_last_actuation_duration = third_last_actuation_event
                    .time
                    .duration_since(fourth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "third_last_actuation_duration={}, Command={}",
                    third_last_actuation_duration, second_last_actuation_event.command
                );

                let fourth_last_actuation_duration = fourth_last_actuation_event
                    .time
                    .duration_since(fifth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "fourth_last_actuation_duration={}, Command={}",
                    fourth_last_actuation_duration, second_last_actuation_event.command
                );

                let fifth_last_actuation_duration = fifth_last_actuation_event
                    .time
                    .duration_since(sixth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "fifth_last_actuation_duration={}, Command={}",
                    fifth_last_actuation_duration, second_last_actuation_event.command
                );

                let sixth_last_actuation_duration = sixth_last_actuation_event
                    .time
                    .duration_since(seventh_last_actuation_event.time)
                    .as_millis();

                println!(
                    "sixth_last_actuation_duration={}, Command={}",
                    sixth_last_actuation_duration, second_last_actuation_event.command
                );

                assert_le!(second_last_actuation_duration, 1600);
                assert_ge!(second_last_actuation_duration, 1400);
                assert_le!(third_last_actuation_duration, 11650);
                assert_ge!(third_last_actuation_duration, 11450);
                assert_le!(fourth_last_actuation_duration, 1600);
                assert_ge!(fourth_last_actuation_duration, 1400);
                assert_le!(fifth_last_actuation_duration, 11700);
                assert_ge!(fifth_last_actuation_duration, 11450);
                assert_le!(sixth_last_actuation_duration, 1600);
                assert_ge!(sixth_last_actuation_duration, 1400);

                mock_actuator_states.check_terminal_condition_heating();
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration = Duration::from_secs(59);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater =
                    MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                        &config.sensor_manager,
                    ))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(
                    &mut heating,
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking reaction to medium temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Check if the heater is switched off most of the time when heater measured temperature is high.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Test case uses test database #13.
    pub fn test_heating_with_measured_temperature_high_inside_interval() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(
                3600,
                13,
                Some(SawToothProfileConfig::new(6, 1, 6, 1)),
            );

        // replacement values are used to initialize the mutexes
        config.sensor_manager.replacement_value_water_temperature = 27.5;
        config.sensor_manager.replacement_value_ambient_temperature = 24.0;

        let mut channels = Channels::new_for_test();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
                println!(
                    "Mock relay manager received {} commands:",
                    actuation_events.len()
                );
                for actuation_event in actuation_events.clone() {
                    println!("{}", actuation_event);
                }
                // (first and) last actuation events need to be ignored
                let _last_actuation_event = actuation_events.pop().unwrap();
                let second_last_actuation_event = actuation_events.pop().unwrap();
                let third_last_actuation_event = actuation_events.pop().unwrap();
                let fourth_last_actuation_event = actuation_events.pop().unwrap();
                let fifth_last_actuation_event = actuation_events.pop().unwrap();
                let sixth_last_actuation_event = actuation_events.pop().unwrap();
                let seventh_last_actuation_event = actuation_events.pop().unwrap();

                let second_last_actuation_duration = second_last_actuation_event
                    .time
                    .duration_since(third_last_actuation_event.time)
                    .as_millis();

                println!(
                    "second_last_actuation_duration={}, Command={}",
                    second_last_actuation_duration, second_last_actuation_event.command
                );

                let third_last_actuation_duration = third_last_actuation_event
                    .time
                    .duration_since(fourth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "third_last_actuation_duration={}, Command={}",
                    third_last_actuation_duration, second_last_actuation_event.command
                );

                let fourth_last_actuation_duration = fourth_last_actuation_event
                    .time
                    .duration_since(fifth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "fourth_last_actuation_duration={}, Command={}",
                    fourth_last_actuation_duration, second_last_actuation_event.command
                );

                let fifth_last_actuation_duration = fifth_last_actuation_event
                    .time
                    .duration_since(sixth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "fifth_last_actuation_duration={}, Command={}",
                    fifth_last_actuation_duration, second_last_actuation_event.command
                );

                let sixth_last_actuation_duration = sixth_last_actuation_event
                    .time
                    .duration_since(seventh_last_actuation_event.time)
                    .as_millis();

                println!(
                    "sixth_last_actuation_duration={}, Command={}",
                    sixth_last_actuation_duration, second_last_actuation_event.command
                );

                /*
                assert_le!(second_last_actuation_duration, 1600);
                assert_ge!(second_last_actuation_duration, 1400);
                assert_le!(third_last_actuation_duration, 11650);
                assert_ge!(third_last_actuation_duration, 11450);
                assert_le!(fourth_last_actuation_duration, 1600);
                assert_ge!(fourth_last_actuation_duration, 1400);
                assert_le!(fifth_last_actuation_duration, 11650);
                assert_ge!(fifth_last_actuation_duration, 11450);
                assert_le!(sixth_last_actuation_duration, 1600);
                assert_ge!(sixth_last_actuation_duration, 1400);
                */

                mock_actuator_states.check_terminal_condition_heating();
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration = Duration::from_secs(59);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater =
                    MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                        &config.sensor_manager,
                    ))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(
                    &mut heating,
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking reaction to medium temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case checks if the Quit command is correctly processed
    // - application shall switch off the heater before exiting
    // The test case uses test database #14.
    pub fn test_heating_with_pending_quit_terminate_command() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3, 14, None);

        // replacement values are used to initialize the mutexes
        config.sensor_manager.replacement_value_water_temperature = 25.0;
        config.sensor_manager.replacement_value_ambient_temperature = 24.0;

        let mut channels = Channels::new_for_test();

        let start_time = Instant::now();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock relay manager
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
                println!(
                    "Mock relay manager received {} commands.",
                    actuation_events.len()
                );
                assert_eq!(actuation_events.len(), 1);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
                mock_actuator_states.check_terminal_condition_heating();
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater = MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(&config.sensor_manager))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(&mut heating, 
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                // check if heater actuation has taken place
                let mutex_data = mutex_device_scheduler_heating.lock().unwrap();
                assert_eq!(*mutex_data, 1);
            let finish_time = Instant::now();
            let execution_duration = finish_time.duration_since(start_time);
            assert_le!(execution_duration.as_millis(), 650);
            println!(
                "* [Heating] checking heating execution with pending quit/terminate command (time limit) succeeded."
            );
        })
        .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case checks if waiting for midnight and statistical calculations are computed correctly.
    // Test case uses test database #15.
    pub fn test_heating_wait_for_midnight_calc_stats_heater_on() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let seconds_to_midnight: i64 = 3;
        let reference_stats_update_count =
            seconds_to_midnight * 1000 / CYCLE_TIME_HEATING_MILLIS as i64;
        let (mut config, mut heating, sql_interface, mut sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(seconds_to_midnight, 15, None);

        // replacement values are used to initialize the mutexes
        config.sensor_manager.replacement_value_water_temperature = 20.0;
        config.sensor_manager.replacement_value_ambient_temperature = 24.0;

        let mut channels = Channels::new_for_test();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // get initial data for calculation of reference value
        let initial_heating_stats_entry = sql_interface_heating_stats
            .get_single_heating_stats_from_database()
            .unwrap_or_else(|_| {
                println!("Database responded with empty response - using default value");
                HeatingStatsEntry::default()
            });

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock relay manager
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration = Duration::from_millis(5150);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater = MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(&config.sensor_manager))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(&mut heating, 
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!(
                    "Initial heating stats: {}", initial_heating_stats_entry
                );
                println!(
                    "test_heating_wait_for_midnight_calc_stats_heater_on (previous): {}",
                    heating_stats_transfer.heating_stats_entry_previous,
                );
                assert_float_absolute_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .heating_control_runtime
                        - initial_heating_stats_entry.heating_control_runtime,
                    3.0,
                    0.1
                );
                assert_float_absolute_eq!(
                    heating_stats_transfer.heating_stats_entry_previous.energy,
                    0.1671666,
                    0.01
                );
                assert_float_absolute_eq!(
                    24.0,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_value,
                    0.001
                );
                assert_float_absolute_eq!(
                    20.0,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_value,
                    0.001
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_counter,
                    reference_stats_update_count + 1
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_counter,
                    reference_stats_update_count + 1
                );
                println!(
                    "test_heating_wait_for_midnight_calc_stats_heater_on (current): {}",
                    heating_stats_transfer.heating_stats_entry,
                );
                assert_float_absolute_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .heating_control_runtime
                        - initial_heating_stats_entry.heating_control_runtime,
                    2.0,
                    0.6
                );
                assert_float_absolute_eq!(0.1119444, heating_stats_transfer.heating_stats_entry.energy, 0.05);
                assert_float_absolute_eq!(24.0,
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_value,
                    0.001
                );
                assert_float_absolute_eq!(
                    20.0,
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_value,
                    0.001
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_counter,
                    4
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_counter,
                    4
                );
                println!("* [Heating] checking heating waiting for midnight and stats calculation with heater on succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case checks if waiting for midnight and statistical calculations are computed correctly.
    // Test case uses test database #16.
    pub fn test_heating_wait_for_midnight_calc_stats_heater_off() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let seconds_to_midnight: i64 = 3;
        let reference_stats_update_count =
            seconds_to_midnight * 1000 / CYCLE_TIME_HEATING_MILLIS as i64;

        let (mut config, mut heating, sql_interface, mut sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(seconds_to_midnight, 16, None);

        // replacement values are used to initialize the mutexes
        config.sensor_manager.replacement_value_water_temperature = 30.0;
        config.sensor_manager.replacement_value_ambient_temperature = 24.0;

        let mut channels = Channels::new_for_test();

        // get initial data for calculation of reference value
        let initial_heating_stats_entry = sql_interface_heating_stats
            .get_single_heating_stats_from_database()
            .unwrap_or_else(|_| {
                // database responded with an empty response - let's create an empty entry
                HeatingStatsEntry {
                    date: Local::now().date_naive(),
                    energy: 0.0,
                    ambient_temperature_average_value: 0.0,
                    ambient_temperature_average_counter: 0,
                    water_temperature_average_value: 0.0,
                    water_temperature_average_counter: 0,
                    heating_control_runtime: 0.0,
                }
            });

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock relay manager
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration = Duration::from_millis(5250);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater = MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(&config.sensor_manager))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(&mut heating, 
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!(
                    "test_heating_wait_for_midnight_calc_stats_heater_on (previous): {}",
                    heating_stats_transfer.heating_stats_entry_previous,
                );
                assert_float_absolute_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .heating_control_runtime
                        - initial_heating_stats_entry.heating_control_runtime,
                    3.02,
                    0.1
                );
                assert_float_absolute_eq!(
                    0.0,
                    heating_stats_transfer.heating_stats_entry_previous.energy,
                    0.001
                );
                assert_float_absolute_eq!(
                    24.0,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_value,
                    0.001
                );
                assert_float_absolute_eq!(
                    30.0,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_value,
                    0.001
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_counter
                        - initial_heating_stats_entry.ambient_temperature_average_counter,
                    reference_stats_update_count + 1
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_counter
                        - initial_heating_stats_entry.water_temperature_average_counter,
                    reference_stats_update_count + 1
                );
                println!(
                    "test_heating_wait_for_midnight_calc_stats_heater_off (current): {}",
                    heating_stats_transfer.heating_stats_entry,
                );
                assert_float_absolute_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .heating_control_runtime
                        - initial_heating_stats_entry.heating_control_runtime,
                    2.0,
                    0.6
                );
                assert_float_absolute_eq!(0.0, heating_stats_transfer.heating_stats_entry.energy, 0.003);
                assert_float_absolute_eq!(
                    24.0,
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_value,
                    0.001
                );
                assert_float_absolute_eq!(
                    30.0,
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_value,
                    0.001
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_counter,
                    4
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_counter,
                    4
                );
                println!("* [Heating] checking heating waiting for midnight and stats calculation with heater on succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case checks if a negative schedule checker result is processed correctly.
    // Test case uses test database #17.
    pub fn test_heating_blocked_by_schedule() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3600, 17, None);

        // replacement values are used to initialize the mutexes
        config.sensor_manager.replacement_value_water_temperature = 20.0;
        config.sensor_manager.replacement_value_ambient_temperature = 24.0;

        let mut channels = Channels::new_for_test();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    false,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
                assert_eq!(actuation_events.len(), 1);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
                mock_actuator_states.check_terminal_condition_heating();
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater =
                    MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                        &config.sensor_manager,
                    ))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(
                    &mut heating,
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
                println!("* [Heating] checking if schedule checker can block actuation.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case checks if component protection is executed.
    // Test case uses test database #18.
    pub fn test_heating_component_protection_when_tank_level_low() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3, 18, None);
        // replacement values are used to initialize the mutexes
        config.sensor_manager.replacement_value_water_temperature = 20.0;
        config.sensor_manager.replacement_value_ambient_temperature = 24.0;

        let mut channels = Channels::new_for_test();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
                assert_eq!(actuation_events.len(), 2);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Heater)
                );
                mock_actuator_states.check_terminal_condition_heating();
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater =
                    MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                        &config.sensor_manager,
                    ))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(
                    &mut heating,
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking if schedule checker can block actuation.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case runs heating control and triggers inhibition by sending a stop message via the channel.
    // After verification that heating is stopped,
    // The test case sends the start message via the channel and verifies if heating control
    // resumes operation.
    // Test case uses test database #19.
    pub fn test_messaging_stops_starts_heating() {
        let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3600, 19, None);

        // replacement values are used to initialize the mutexes
        config.sensor_manager.replacement_value_water_temperature = 20.0;
        config.sensor_manager.replacement_value_ambient_temperature = 24.0;

        let mut channels = Channels::new_for_test();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));
        let mutex_device_scheduler_test_environment = mutex_device_scheduler_heating.clone();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
                println!("actuation_events:");
                for actuation_event in &actuation_events {
                    println!("{}", actuation_event);
                }
                assert_eq!(actuation_events.len(), 4);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Heater)
                );
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Heater)
                );
                mock_actuator_states.check_terminal_condition_heating();
            })
            .unwrap();

        // thread for test environment
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_100_millis = Duration::from_millis(100);
                let spin_sleeper = SpinSleeper::default();

                // initial wait so that heating control can switch on
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millis);
                }
                // check the initial state of mutex: should be already switched on
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 1);

                // sending a message requesting stop of heating control
                match channels
                    .messaging
                    .tx_messaging_to_heating
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

                // wait for 1 second: heating control will switch off
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millis);
                }
                // check if heating is actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 2);

                // sending a message requesting restart of heating control
                match channels
                    .messaging
                    .tx_messaging_to_heating
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

                // Wait for 1 second. Heating control will switch on
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millis);
                }
                // check if heating is actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 3);

                // sending a message requesting stop of heating control
                match channels
                    .messaging
                    .tx_messaging_to_heating
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

                // wait for 1 second: Heating control will switch off
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millis);
                }
                // check if heating is actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 4);

                // requesting heating control to quit
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();

                // check if heating is not actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 4);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater =
                    MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                        &config.sensor_manager,
                    ))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(
                    &mut heating,
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking if messaging can block and restart actuation.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Check if the heater remains switched off with a high temperature initially.
    // After a limited time period, increase the set values and observe if the heater is switched on.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Test case uses test database #50.
    pub fn test_heating_with_increased_set_values() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3600, 50, None);
        // replacement values are used to initialize the mutexes
        config.sensor_manager.replacement_value_water_temperature = 30.0;
        config.sensor_manager.replacement_value_ambient_temperature = 24.0;

        let mut channels = Channels::new_for_test();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        // thread for mock TankLevelSwitch
        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
                assert_eq!(actuation_events.len(), 3);
                let actuation_event_3 = actuation_events.pop().unwrap();
                let actuation_event_2 = actuation_events.pop().unwrap();
                let actuation_event_1 = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event_1.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
                assert_eq!(
                    actuation_event_2.command,
                    InternalCommand::SwitchOn(AquariumDevice::Heater)
                );
                assert_eq!(
                    actuation_event_3.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
                mock_actuator_states.check_terminal_condition_heating();
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_twelve_seconds = Duration::from_secs(12);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_twelve_seconds);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());

                // set up the set value updater to actively manipulate the set values
                let mut heating_set_value_updater = MockSqlInterfaceHeatingSetVals::new(
                    true,
                    Some(50.0),
                    Some(40.0),
                    Some(Duration::from_secs(10)),
                );

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                        &config.sensor_manager,
                    ))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(
                    &mut heating,
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking reaction to high temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Check if the heater is switched on and is not switched off with low temperature.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Check if the heater is switched off after sending Quit command and before sending Terminate command.
    // Test case uses test database #12.
    // Additionally, mutex for heater status is blocked by test environment
    pub fn test_heating_with_measured_temperature_low_with_mutex_blocked() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3600, 12, None);

        // replacement values are used to initialize the mutexes
        config.sensor_manager.replacement_value_water_temperature = 20.0;
        config.sensor_manager.replacement_value_ambient_temperature = 24.0;

        let mut channels = Channels::new_for_test();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));
        let mutex_heating_status = Arc::new(Mutex::new(ThermalControlStatus::default()));
        let mutex_heating_status_clone_for_test_environment = mutex_heating_status.clone();

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
                assert_eq!(actuation_events.len(), 2);
                let last_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    last_actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
                let first_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    first_actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Heater)
                );
                mock_actuator_states.check_terminal_condition_heating();
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_one_second = Duration::from_secs(1);
                let spin_sleeper = SpinSleeper::default();
                for i in 0..10 {
                    if i % 2 == 0 {
                        spin_sleeper.sleep(sleep_duration_one_second);
                    } else {
                        // block mutex on purpose
                        {
                            match mutex_heating_status_clone_for_test_environment.lock() {
                                Ok(_) => {
                                    spin_sleeper.sleep(sleep_duration_one_second);
                                }
                                Err(_) => {
                                    // Do nothing
                                }
                            }
                        }
                    }
                }
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater =
                    MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                        &config.sensor_manager,
                    ))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status,
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(&mut heating, 
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                assert_eq!(heating.mutex_access_duration_exceeded, true);

                println!("* [Heating] checking reaction to low temperature with mutex blocked succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Verifies that `record_heating_stats` does nothing when called before midnight.
    fn test_record_heating_stats_before_midnight() {
        // 1. Setup
        let (_config, mut heating, mut sql_interface, mut sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3600, 20, None); // Midnight is far away
        let mut mock_transfer = HeatingStatsMockDataTransfer::new(&mut sql_interface.conn);
        let mut midnight_instant = Instant::now() + Duration::from_secs(60);

        let mut heating_stats = HeatingStatsEntry::new(&mut sql_interface_heating_stats.conn);
        heating_stats.energy = 1.23; // Give it some data to track
        let mut heating_stats_opt = Some(heating_stats.clone());

        // 2. Execute
        heating.record_heating_stats(
            &mut midnight_instant,
            &mut heating_stats_opt,
            &mut mock_transfer,
            &mut sql_interface_heating_stats,
            false,
        );

        // 3. Assert
        assert_eq!(
            mock_transfer.transfer_counter, 0,
            "Transfer should not be called before midnight"
        );
        assert!(
            heating_stats_opt.is_some(),
            "Stats option should still be Some"
        );
        // Statistical data should be unchanged
        assert_float_absolute_eq!(heating_stats_opt.unwrap().energy, 1.23, 0.001);
    }

    #[test]
    // Verifies that `record_heating_stats` correctly transfers data after midnight.
    fn test_record_heating_stats_after_midnight_success() {
        // 1. Setup - mock midnight is one second away
        let (_config, mut heating, mut sql_interface, mut sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(1, 21, None);
        let mut mock_transfer = HeatingStatsMockDataTransfer::new(&mut sql_interface.conn);
        let mut midnight_instant = Instant::now() - Duration::from_secs(1); // Set midnight in the past
        let original_midnight_instant = midnight_instant;

        let mut heating_stats = HeatingStatsEntry::new(&mut sql_interface_heating_stats.conn);
        heating_stats.energy = 4.56;
        let mut heating_stats_opt = Some(heating_stats.clone());

        // 2. Execute
        heating.record_heating_stats(
            &mut midnight_instant,
            &mut heating_stats_opt,
            &mut mock_transfer,
            &mut sql_interface_heating_stats,
            false,
        );

        // 3. Assert
        assert_eq!(
            mock_transfer.transfer_counter, 1,
            "Transfer should be called once"
        );

        let transferred_stats = mock_transfer.last_transferred_heating_stats_entry;
        assert!(
            transferred_stats.is_some(),
            "Transferred stats should be captured by the mock"
        );
        // The correct stats data should be transferred
        assert_float_absolute_eq!(transferred_stats.as_ref().unwrap().energy, 4.56, 0.001);

        assert!(
            heating_stats_opt.is_some(),
            "Stats option should be re-initialized, not None"
        );
        // Stats should be reset to a new, default entry
        assert_float_absolute_eq!(heating_stats_opt.unwrap().energy, 0.0, 0.001);

        assert!(
            midnight_instant > original_midnight_instant,
            "Midnight instant should be updated to a future time"
        );
    }

    #[test]
    // Verifies that the function handles a database transfer error gracefully.
    fn test_record_heating_stats_after_midnight_with_error() {
        // 1. Setup
        let (_config, mut heating, mut sql_interface, mut sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(1, 22, None);
        let mut mock_transfer = HeatingStatsMockDataTransfer::new(&mut sql_interface.conn);
        let mut midnight_instant = Instant::now() - Duration::from_secs(1);
        let original_midnight_instant = midnight_instant;

        let mut heating_stats = HeatingStatsEntry::new(&mut sql_interface_heating_stats.conn);
        heating_stats.energy = 7.89;
        let mut heating_stats_opt = Some(heating_stats.clone());

        // 2. Execute
        heating.record_heating_stats(
            &mut midnight_instant,
            &mut heating_stats_opt,
            &mut mock_transfer,
            &mut sql_interface_heating_stats,
            false,
        );

        // 3. Assert
        assert_eq!(
            mock_transfer.transfer_counter, 1,
            "Transfer should still be attempted even if it fails"
        );

        assert!(
            heating_stats_opt.is_some(),
            "Stats option should be re-initialized even on error"
        );
        // Stats should be reset to a new entry despite the transfer error
        assert_float_absolute_eq!(heating_stats_opt.unwrap().energy, 0.0, 0.001);

        assert!(
            midnight_instant > original_midnight_instant,
            "Midnight instant should still be updated for the next day"
        );
    }

    #[test]
    // Verifies that the function does nothing trigger the transfer call if the optional parameter is None when midnight occurs.
    fn test_record_heating_stats_with_none_stats() {
        // 1. Setup
        let (_config, mut heating, mut sql_interface, mut sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(1, 23, None);
        let mut mock_transfer = HeatingStatsMockDataTransfer::new(&mut sql_interface.conn);
        let mut midnight_instant = Instant::now() - Duration::from_secs(1); // Midnight is in the past
        let original_midnight_instant = midnight_instant;
        let mut heating_stats_opt: Option<HeatingStatsEntry> = None;

        // 2. Execute
        heating.record_heating_stats(
            &mut midnight_instant,
            &mut heating_stats_opt,
            &mut mock_transfer,
            &mut sql_interface_heating_stats,
            false,
        );

        // 3. Assert
        assert_eq!(
            mock_transfer.transfer_counter, 0,
            "Transfer should not be called if stats are None"
        );
        assert!(
            heating_stats_opt.is_none(),
            "Stats option should remain None"
        );
        assert_eq!(
            midnight_instant, original_midnight_instant,
            "Midnight instant should not be updated if no transfer happens"
        );
    }

    #[test]
    // Verifies that stats are updated correctly when the heater is ON and sensor data is valid.
    fn test_update_heating_stats_heater_on_valid_data() {
        // 1. Setup
        let (config, mut heating, _sql_interface, mut sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3600, 24, None);
        let mut heating_stats_opt = Some(HeatingStatsEntry::new(
            &mut sql_interface_heating_stats.conn,
        ));
        let sleep_duration = Duration::from_millis(500);

        // Manually set the last update time to simulate a time delta
        heating.last_stats_update_instant_opt = Some(Instant::now());
        thread::sleep(sleep_duration);

        // 2. Execute
        heating.update_heating_stats(
            &mut heating_stats_opt,
            false, // measurement_error
            25.5,  // water_temperature
            22.0,  // ambient_temperature
            &ActuatorState::On,
        );

        // 3. Assert
        let stats = heating_stats_opt.unwrap();
        let expected_energy =
            config.heating.heater_power as f64 * (sleep_duration.as_secs_f64() / 3600.0);

        assert_float_absolute_eq!(stats.energy, expected_energy, 0.01);
        assert_float_absolute_eq!(stats.water_temperature_average_value, 25.5, 0.001);
        assert_eq!(stats.water_temperature_average_counter, 1);
        assert_float_absolute_eq!(stats.ambient_temperature_average_value, 22.0, 0.001);
        assert_eq!(stats.ambient_temperature_average_counter, 1);
        assert_float_absolute_eq!(
            stats.heating_control_runtime,
            sleep_duration.as_secs_f64(),
            0.2
        );
    }

    #[test]
    // Verifies that no energy is added when the heater is OFF.
    fn test_update_heating_stats_heater_off() {
        // 1. Setup
        let (_config, mut heating, _sql_interface, mut sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3600, 25, None);
        let mut heating_stats_opt = Some(HeatingStatsEntry::new(
            &mut sql_interface_heating_stats.conn,
        ));
        let sleep_duration = Duration::from_millis(500);

        heating.last_stats_update_instant_opt = Some(Instant::now());
        thread::sleep(sleep_duration);

        // 2. Execute
        heating.update_heating_stats(
            &mut heating_stats_opt,
            false, // measurement_error
            25.5,  // water_temperature
            22.0,  // ambient_temperature
            &ActuatorState::Off,
        );

        // 3. Assert
        let stats = heating_stats_opt.unwrap();
        // Energy should be zero when the heater is off
        assert_float_absolute_eq!(stats.energy, 0.0, 0.001);
        assert_float_absolute_eq!(stats.water_temperature_average_value, 25.5, 0.001);
        assert_eq!(stats.water_temperature_average_counter, 1);
    }

    #[test]
    // Verifies that temperatures are not included in the average when a measurement error occurs.
    fn test_update_heating_stats_with_measurement_error() {
        // 1. Setup
        let (_config, mut heating, _sql_interface, mut sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3600, 26, None);
        let mut heating_stats_opt = Some(HeatingStatsEntry::new(
            &mut sql_interface_heating_stats.conn,
        ));
        let sleep_duration = Duration::from_millis(500);

        heating.last_stats_update_instant_opt = Some(Instant::now());
        thread::sleep(sleep_duration);

        // 2. Execute
        heating.update_heating_stats(
            &mut heating_stats_opt,
            true,  // measurement_error
            -99.0, // water_temperature (should be ignored)
            -99.0, // ambient_temperature (should be ignored)
            &ActuatorState::On,
        );

        // 3. Assert
        let stats = heating_stats_opt.unwrap();
        assert!(
            stats.energy > 0.0,
            "Energy should still be calculated even with measurement errors"
        );
        // Average should not be updated
        assert_float_absolute_eq!(stats.water_temperature_average_value, 0.0, 0.001);
        assert_eq!(
            stats.water_temperature_average_counter, 0,
            "Counter should not be incremented"
        );
        assert_float_absolute_eq!(stats.ambient_temperature_average_value, 0.0, 0.001);
        assert_eq!(
            stats.ambient_temperature_average_counter, 0,
            "Counter should not be incremented"
        );
    }

    #[test]
    // Verifies that the first call to update_heating_stats results in zero energy,
    // because `last_stats_update_instant_opt` is initially None.
    fn test_update_heating_stats_first_call_no_energy() {
        // 1. Setup
        let (_config, mut heating, _sql_interface, mut sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3600, 27, None);
        let mut heating_stats_opt = Some(HeatingStatsEntry::new(
            &mut sql_interface_heating_stats.conn,
        ));

        // `heating.last_stats_update_instant_opt` is None by default from the constructor

        // 2. Execute
        heating.update_heating_stats(
            &mut heating_stats_opt,
            false,
            25.5,
            22.0,
            &ActuatorState::On,
        );

        // 3. Assert
        // Energy should be zero on the first call because increment_fraction is 0.0
        let stats = heating_stats_opt.unwrap();
        assert_float_absolute_eq!(stats.energy, 0.0, 0.001);
        // Runtime should be zero on the first call
        assert_float_absolute_eq!(stats.heating_control_runtime, 0.0, 0.001);
        assert_eq!(
            stats.water_temperature_average_counter, 1,
            "Temperatures should still be recorded"
        );
    }

    #[test]
    // Verifies that the function does not trigger the update if the heating statistic entry is None.
    fn test_update_heating_stats_with_none_stats_opt() {
        // 1. Setup
        let (_config, mut heating, _sql_interface, _sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3600, 28, None);
        let mut heating_stats_opt: Option<HeatingStatsEntry> = None;

        // 2. Execute
        heating.update_heating_stats(
            &mut heating_stats_opt,
            false,
            25.5,
            22.0,
            &ActuatorState::On,
        );

        // 3. Assert
        assert!(
            heating_stats_opt.is_none(),
            "The stats option should remain None"
        );
    }

    #[test]
    pub fn test_heating_gradient_monitoring_disables_actuator() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3600, 28, None);
        // test case is not writing to database, hence db can be reused.

        // Configure gradient monitoring to be very aggressive
        config.heating.max_time_actuator_effect_monitor = 2;
        config.heating.switch_off_when_no_effect = true;
        heating.config = config.heating;

        // replacement value is used for initializing mutex - needs to start with a low value
        config.sensor_manager.replacement_value_water_temperature = 20.0;

        let mut channels = Channels::new_for_test();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex needed for water temperature
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // Mutex for heating status to check assertions
        let mutex_heating_status = Arc::new(Mutex::new(ThermalControlStatus::default()));
        let mutex_heating_status_clone = mutex_heating_status.clone();

        // Mutex for temperature gradient
        let mutex_temperature_gradient = Arc::new(Mutex::new(0.1)); // Start with heating gradient
        let mutex_temperature_gradient_clone = mutex_temperature_gradient.clone();

        // thread for mock relay manager
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
                // Expect On, Off
                assert_eq!(actuation_events.len(), 2);
                assert_eq!(
                    actuation_events[0].command,
                    InternalCommand::SwitchOn(AquariumDevice::Heater)
                );
                assert_eq!(
                    actuation_events[1].command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );

                mock_actuator_states.check_terminal_condition_heating();
            })
            .unwrap();

        // thread for controlling test phases
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_phase = Duration::from_secs(5);
                let spin_sleeper = SpinSleeper::default();

                // Phase 1: High temp, good gradient. Wait 5s.
                spin_sleeper.sleep(sleep_duration_phase);
                {
                    let status = mutex_heating_status_clone.lock().unwrap();
                    assert!(status.actuator_switched_on, "Phase 1: Heating should be ON");
                }

                // Phase 2: Constant temp (gradient indicates that actuator is ineffective).
                {
                    let mut gradient = mutex_temperature_gradient_clone.lock().unwrap();
                    *gradient = 0.0;
                }
                // Wait 5s (monitor timeout is 2s)
                spin_sleeper.sleep(sleep_duration_phase);
                {
                    let status = mutex_heating_status_clone.lock().unwrap();
                    assert!(
                        !status.actuator_switched_on,
                        "Phase 2: Heating should be OFF due to gradient monitor"
                    );
                }

                // Terminate
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_set_value_updater =
                    MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals,
                    mutex_tank_level_switch_signals: Arc::new(Mutex::new(TankLevelSwitchSignals {
                        tank_level_switch_position: true,
                        tank_level_switch_position_stabilized: true,
                        tank_level_switch_invalid: false,
                    })),
                    mutex_heating_status,
                    mutex_temperature_gradient,
                };

                execute_blocking(
                    &mut heating,
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats,
                );

                // send Quit signal to mock threads
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
                println!("* [Heating] checking gradient monitoring succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check failed");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager failed");
        join_handle_test_environment
            .join()
            .expect("Test environment failed");
        join_handle_test_object.join().expect("Test object failed");
    }

    #[test]
    pub fn test_heating_reset_errors_enables_actuator() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
            prepare_heating_tests_mock_midnight_calculator(3600, 28, None);
        // test case is not writing to database, hence db can be reused.

        // Start in disabled state
        heating.actuator_disabled = true;

        // replacement value is used for initializing mutex - needs to start with a low value
        config.sensor_manager.replacement_value_water_temperature = 20.0;

        let mut channels = Channels::new_for_test();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_heating,
                    &mut channels.schedule_check.rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex needed for water temperature
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // Mutex for heating status to check assertions
        let mutex_heating_status = Arc::new(Mutex::new(ThermalControlStatus::default()));
        let mutex_heating_status_clone = mutex_heating_status.clone();

        // Mutex for temperature gradient
        let mutex_temperature_gradient = Arc::new(Mutex::new(0.1)); // Start with positive gradient
        let mutex_temperature_gradient_clone = mutex_temperature_gradient.clone();

        // thread for mock relay manager
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_heating,
                    &mut channels.relay_manager.rx_relay_manager_from_heating,
                );
                // Expect SwitchOn (after reset), SwitchOff (at termination)
                assert_eq!(actuation_events.len(), 2);
                assert_eq!(
                    actuation_events[0].command,
                    InternalCommand::SwitchOn(AquariumDevice::Heater)
                );
                assert_eq!(
                    actuation_events[1].command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );

                mock_actuator_states.check_terminal_condition_heating();
            })
            .unwrap();

        // thread for controlling test phases
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_phase = Duration::from_secs(5);
                let spin_sleeper = SpinSleeper::default();

                // Phase 1: Disabled state. Wait 5s.
                spin_sleeper.sleep(sleep_duration_phase);
                {
                    let status = mutex_heating_status_clone.lock().unwrap();
                    assert!(
                        !status.actuator_switched_on,
                        "Phase 1: Heating should be OFF (disabled)"
                    );
                }

                // Send ResetAllErrors command
                channels
                    .messaging
                    .tx_messaging_to_heating
                    .send(InternalCommand::ResetAllErrors)
                    .unwrap();

                // Phase 2: Enabled state. Wait 5s.
                spin_sleeper.sleep(sleep_duration_phase);
                {
                    let status = mutex_heating_status_clone.lock().unwrap();
                    assert!(
                        status.actuator_switched_on,
                        "Phase 2: Heating should be ON (enabled)"
                    );
                }

                // Terminate
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_heating
                    .blocking_recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_heating
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_set_value_updater =
                    MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals,
                    mutex_tank_level_switch_signals: Arc::new(Mutex::new(TankLevelSwitchSignals {
                        tank_level_switch_position: true,
                        tank_level_switch_position_stabilized: true,
                        tank_level_switch_invalid: false,
                    })),
                    mutex_heating_status,
                    mutex_temperature_gradient: mutex_temperature_gradient_clone,
                };

                execute_blocking(
                    &mut heating,
                    mutex_device_scheduler_heating.clone(),
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating_stats,
                );

                // send Quit signal to mock threads
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
                println!("* [Heating] checking reset errors enables actuator succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check failed");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager failed");
        join_handle_test_environment
            .join()
            .expect("Test environment failed");
        join_handle_test_object.join().expect("Test object failed");
    }
}
