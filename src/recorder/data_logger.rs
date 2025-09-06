/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#![allow(non_snake_case)]
use chrono::{Local, NaiveDateTime};
#[cfg(all(not(test), target_os = "linux"))]
use log::info;
use log::{error, warn};

#[cfg(feature = "debug_data_logger")]
use log::debug;

#[cfg(all(not(test), target_os = "linux"))]
use nix::unistd::gettid;

use mysql::PooledConn;
use spin_sleep::SpinSleeper;
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_data::SqlInterfaceData;
use crate::recorder::data_injection::DataInjectionTrait;
use crate::recorder::data_logger_channels::DataLoggerChannels;
use crate::recorder::data_logger_config::DataLoggerConfig;
use crate::recorder::recorder_data_frame::RecorderDataFrame;
use crate::sensors::sensor_manager::SensorManagerSignals;
use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::channel_content::AquariumSignal;
use crate::utilities::database_ping_trait::DatabasePingTrait;
use crate::utilities::iir_filter::IIRFilter;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;
use crate::water::refill::RefillStatus;

const MILLIS_PER_SEC: u64 = 1000;

/// Contains `Arc<Mutex>`-wrapped shared data points accessed by the data logger.
///
/// These mutexes provide thread-safe access to various sensor readings and
/// device states that are collected by the data logger for recording and filtering.
pub struct DataLoggerMutexes {
    /// Mutex for sensor manager signals
    pub mutex_sensor_manager_signals: Arc<Mutex<SensorManagerSignals>>,

    /// Mutex for tank level switch signals
    pub mutex_tank_level_switch_signals: Arc<Mutex<TankLevelSwitchSignals>>,

    /// Mutex for heater status
    pub mutex_heating_status: Arc<Mutex<bool>>,

    /// Mutex for ventilation status
    pub mutex_ventilation_status: Arc<Mutex<bool>>,

    /// Mutex for refill status
    pub mutex_refill_status: Arc<Mutex<RefillStatus>>,
}

/// Contains the error definition for DataLogger
#[derive(Error, Debug)]
pub enum DataLoggerError {
    #[error("[{0}] One of the output file names for the data logging to filesystem is empty.")]
    OutputFilenameEmpty(String),

    #[error("Could not reset content of output file for {signal_name} ({output_file_name})")]
    FileResetFailure {
        location: String,
        signal_name: String,
        output_file_name: String,

        #[source]
        source: std::io::Error,
    },
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for regular data logging to SQL database
/// and writing to the file (in RAM disk).
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     atlas_scientific[AtlasScientific] -.-> data_logger
///     ambient[Ambient] -.-> data_logger
///     tank_level_switch[TankLevelSwitch] --> data_logger
///     refill[Refill control] -.-> data_logger
///     ventilation[Ventilation control] --> data_logger
///     data_logger --> signal_handler[Signal handler]
///     signal_handler[Signal handler] --> data_logger
/// ```
pub struct DataLogger {
    /// configuration data for the data logger
    config: DataLoggerConfig,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to create the timestamp
    lock_error_get_timestamp: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write refill in progress to disk
    lock_error_output_state_refill_in_progress: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write heating is on to disk
    lock_error_output_state_heating_is_on: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write ventilation is on to disk
    lock_error_output_state_ventilation_is_on: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write water temperature to disk
    lock_error_output_signal_water_temperature: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write pH to disk
    lock_error_output_signal_pH: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write conductivity to disk
    lock_error_output_signal_conductivity: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write ambient temperature to disk
    lock_error_output_signal_ambient_temperature: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write humidity to disk
    lock_error_output_signal_humidity: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write tank level switch position to disk
    lock_error_output_state_tank_level_switch_position: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write tank level switch invalid to disk
    lock_error_output_state_tank_level_switch_invalid: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write tank level switch position stabilized to disk
    lock_error_output_state_tank_level_switch_position_stabilized: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write temperature filtered to disk
    lock_error_output_signal_water_temperature_filtered: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write pH filtered to disk
    lock_error_output_signal_pH_filtered: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write conductivity filtered to disk
    lock_error_output_signal_conductivity_filtered: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write timestamp to disk
    lock_error_output_timestamp: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about having received an inapplicable command
    pub lock_warn_inapplicable_command_signal_handler: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    pub lock_error_channel_receive_termination: bool,

    /// recording when the last database ping happened
    pub last_ping_instant: Instant,

    /// database ping interval
    pub database_ping_interval: Duration,
    lock_error_mutex_signal_manager: bool,
}

impl DataLogger {
    /// Creates a new `DataLogger` instance.
    ///
    /// This constructor initializes the data logger module with its configuration.
    /// If `output_to_disk` is enabled, it validates that all required output file
    /// paths are non-empty and then resets each file by writing an empty string to it.
    /// It also initializes many internal "lock" flags designed to prevent log flooding.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the data logger, defining logging intervals,
    ///   filter coefficients, output file paths, and other operational parameters.
    /// * `database_ping_interval` - A `Duration` instance, providing the interval to ping the database.
    ///
    /// # Returns
    /// A `Result` containing a new, initialized `DataLogger` instance on success.
    ///
    /// # Errors
    /// This function will return a `DataLoggerError` if `output_to_disk` is enabled and:
    /// - Any of the configured output filenames are empty (`OutputFilenameEmpty`).
    /// - It fails to write to (and thus reset) any of the configured output files,
    ///   which could be due to permission issues or an invalid path (`FileResetFailure`).
    pub fn new(
        config: DataLoggerConfig,
        database_ping_interval: Duration,
    ) -> Result<DataLogger, DataLoggerError> {
        if config.output_to_disk {
            // By defining the files in a collection, we can use iterators to check and
            // initialize them, making the code concise, readable, and easy to maintain.
            let files_to_initialize = [
                (&config.output_file_name_timestamp, "timestamp"),
                (
                    &config.output_file_name_water_temperature,
                    "water temperature",
                ),
                (
                    &config.output_file_name_water_temperature_filtered,
                    "filtered water temperature",
                ),
                (&config.output_file_name_pH, "pH value"),
                (&config.output_file_name_pH_filtered, "filtered pH value"),
                (&config.output_file_name_conductivity, "conductivity"),
                (
                    &config.output_file_name_conductivity_filtered,
                    "filtered conductivity",
                ),
                (
                    &config.output_file_name_tank_level_switch_position,
                    "tank level switch position",
                ),
                (
                    &config.output_file_name_tank_level_switch_invalid,
                    "tank level switch invalid",
                ),
                (
                    &config.output_file_name_tank_level_switch_position_stabilized,
                    "tank level switch stabilized",
                ),
                (
                    &config.output_file_name_ventilation_is_on,
                    "ventilation status",
                ),
                (&config.output_file_name_heating_is_on, "heating status"),
                (
                    &config.output_file_name_ambient_temperature,
                    "ambient temperature",
                ),
                (&config.output_file_name_humidity, "humidity"),
                (
                    &config.output_file_name_refill_in_progress,
                    "refill in progress",
                ),
            ];

            // 1. Check if any filename is empty.
            if files_to_initialize
                .iter()
                .any(|(filename, _)| filename.is_empty())
            {
                return Err(DataLoggerError::OutputFilenameEmpty(
                    module_path!().to_string(),
                ));
            }

            // 2. Initialize all files by writing an empty string.
            for (filename, signal_name) in files_to_initialize {
                fs::write(filename, String::new()).map_err(|e| {
                    DataLoggerError::FileResetFailure {
                        location: module_path!().to_string(),
                        signal_name: signal_name.to_string(),
                        output_file_name: filename.clone(),
                        source: e,
                    }
                })?;
            }
        }

        Ok(Self {
            config,
            lock_error_get_timestamp: false,
            lock_error_output_signal_water_temperature: false,
            lock_error_output_state_refill_in_progress: false,
            lock_error_output_state_heating_is_on: false,
            lock_error_output_state_ventilation_is_on: false,
            lock_error_output_signal_pH: false,
            lock_error_output_signal_conductivity: false,
            lock_error_output_signal_ambient_temperature: false,
            lock_error_output_signal_humidity: false,
            lock_error_output_state_tank_level_switch_position: false,
            lock_error_output_state_tank_level_switch_invalid: false,
            lock_error_output_state_tank_level_switch_position_stabilized: false,
            lock_error_output_signal_water_temperature_filtered: false,
            lock_error_output_signal_pH_filtered: false,
            lock_error_output_signal_conductivity_filtered: false,
            lock_error_output_timestamp: false,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
            last_ping_instant: Instant::now(),
            database_ping_interval,
            lock_error_mutex_signal_manager: false,
        })
    }

    /// Outputs a non-binary sensor signal to a specified file, typically located on a RAM disk.
    ///
    /// This private helper function converts a floating-point `signal` into a human-readable
    /// string format (determined by `signal_type`) and attempts to write it to the given
    /// `output_file_name`. The operation only proceeds if `output_to_disk` is enabled in
    /// the data logger's configuration.
    ///
    /// Error logging for writing failures is managed by the `inhibit_error_log` flag,
    /// preventing a flood of repetitive error messages.
    ///
    /// # Arguments
    /// * `signal` - The `f32` sensor value to be written to the file.
    /// * `signal_type` - An `AquariumSignal` enum variant, used to determine the correct
    ///   formatting for the `signal` value (e.g., number of decimal places).
    /// * `output_file_name` - A reference to the `String` containing the full path to the output file.
    /// * `inhibit_error_log` - A boolean flag that, if `true`, will prevent this specific
    ///   error from being logged to the console/main log if a write operation fails.
    ///
    /// # Returns
    /// `true` if an error occurred during the file write operation; `false` otherwise (including
    /// when `output_to_disk` is `false` and no write is attempted).
    fn output_signal(
        &self,
        signal: f32,
        signal_type: AquariumSignal,
        output_file_name: &String,
        inhibit_error_log: bool,
    ) -> bool {
        if self.config.output_to_disk {
            let signal_string = signal_type.output_file_string(signal);

            match fs::write(output_file_name, signal_string.clone()) {
                Ok(_) => {
                    /* do nothing */
                    false // indicate no error
                }
                Err(e) => {
                    if !inhibit_error_log {
                        error!(
                            target: module_path!(),
                            "output of value {signal_string} to {output_file_name} failed: {e:?}"
                        );
                    }
                    true // indicate error
                }
            }
        } else {
            false // no output = no error
        }
    }

    /// Outputs a boolean (binary) signal's state to a specified file, typically located on a RAM disk.
    ///
    /// This private helper function converts a boolean `state` into a descriptive string
    /// (e.g., "ON"/"OFF", "HIGH"/"LOW") using provided labels and attempts to write it
    /// to the given `output_file_name`. This operation only occurs if `output_to_disk`
    /// is enabled in the data logger's configuration.
    ///
    /// Error logging for writing failures is controlled by the `inhibit_error_log` flag,
    /// preventing a flood of repetitive error messages.
    ///
    /// # Arguments
    /// * `state` - The `bool` value of the binary signal (e.g., `true` for ON, `false` for OFF).
    /// * `state_description_true` - The string label to write to the file when `state` is `true`.
    /// * `state_description_false` - The string label to write to the file when `state` is `false`.
    /// * `output_file_name` - A reference to the `String` containing the full path to the output file.
    /// * `inhibit_error_log` - A boolean flag that, if `true`, will prevent this specific
    ///   error from being logged to the console/main log if a write operation fails.
    ///
    /// # Returns
    /// - `true`: If an error occurred during the file write operation.
    /// - `false`: If the write-operation was successful or if `output_to_disk` is disabled.
    fn output_state(
        &self,
        state: bool,
        state_description_true: &str,
        state_description_false: &str,
        output_file_name: &String,
        inhibit_error_log: bool,
    ) -> bool {
        if self.config.output_to_disk {
            let state_string = match state {
                true => state_description_true,
                false => state_description_false,
            };

            match fs::write(output_file_name, state_string) {
                Ok(_) => {
                    /* do nothing */
                    false // indicate no error
                }
                Err(e) => {
                    if !inhibit_error_log {
                        error!(
                            target: module_path!(),
                            "output of value {state_string} to {output_file_name} failed: {e:?}"
                        );
                    }
                    true // indicate error
                }
            }
        } else {
            false // no output = no error
        }
    }

    /// Outputs a `NaiveDateTime` timestamp to a specified file, typically located on a RAM disk.
    ///
    /// This private helper function converts a `NaiveDateTime` into its string representation
    /// and attempts to write it to the given `output_file_name`. This operation only occurs
    /// if `output_to_disk` is enabled in the data logger's configuration.
    ///
    /// Error logging for writing failures is controlled by the `inhibit_error_log` flag,
    /// preventing a flood of repetitive error messages.
    ///
    /// # Arguments
    /// * `timestamp` - The `NaiveDateTime` value to be written to the file.
    /// * `output_file_name` - A reference to the `String` containing the full path to the output file.
    /// * `inhibit_error_log` - A boolean flag that, if `true`, will prevent this specific
    ///   error from being logged to the console/main log if a write operation fails.
    ///
    /// # Returns
    /// - `true`: If an error occurred during the file write operation.
    /// - `false`: If the write-operation was successful or if `output_to_disk` is disabled.
    fn output_timestamp(
        &self,
        timestamp: NaiveDateTime,
        output_file_name: &String,
        inhibit_error_log: bool,
    ) -> bool {
        if self.config.output_to_disk {
            match fs::write(output_file_name, timestamp.to_string()) {
                Ok(_) => {
                    /* do nothing */
                    false // indicate no error
                }
                Err(e) => {
                    if !inhibit_error_log {
                        error!(
                            target: module_path!(),
                            "output of value {timestamp} to {output_file_name} failed: {e:?}"
                        );
                    }
                    true // indicate error
                }
            }
        } else {
            false // no output = no error
        }
    }

    /// Retrieves current ON/OFF status signals from Refill, Heating, and Ventilation control modules via channels.
    ///
    /// This function sends a `RequestSignal` command to each of the specified
    /// control modules (Refill, Heating, Ventilation) and updates the provided
    /// status flags based on their responses. It also tracks channel disconnection
    /// status to prevent repeated errors.
    ///
    /// # Arguments
    /// * `refill_status` - A mutable reference for indicating if the refill is in progress.
    /// * `heater_status` - A mutable reference for indicating the heater state.
    /// * `ventilation_status` - A mutable reference for indicating the ventilation state.
    fn update_signals(
        &mut self,
        refill_status_live: &mut bool,
        refill_status_for_database: &mut bool,
        refill_reset_for_database: bool,
        heater_status: &mut bool,
        ventilation_status: &mut bool,
        data_logger_mutexes: &DataLoggerMutexes,
    ) {
        {
            // internal scope to limit lifetime of mutex lock
            match data_logger_mutexes.mutex_refill_status.lock() {
                Ok(mut refill_status) => {
                    self.lock_error_output_state_refill_in_progress = self.output_state(
                        refill_status.refill_in_progress_live, // use the live signal
                        "ON",
                        "OFF",
                        &self.config.output_file_name_refill_in_progress,
                        self.lock_error_output_state_refill_in_progress,
                    );
                    *refill_status_live = refill_status.refill_in_progress_live;
                    *refill_status_for_database = refill_status.refill_in_progress_for_database;
                    if refill_reset_for_database {
                        refill_status.refill_in_progress_for_database =
                            refill_status.refill_in_progress_live;
                    }
                }
                Err(_) => {
                    self.lock_error_output_state_refill_in_progress = true;
                    error!(target: module_path!(), "error locking mutex for refill status");
                }
            };
        }

        {
            // internal scope to limit lifetime of mutex lock
            *heater_status = match data_logger_mutexes.mutex_heating_status.lock() {
                Ok(c) => {
                    self.lock_error_output_state_heating_is_on = self.output_state(
                        *c,
                        "ON",
                        "OFF",
                        &self.config.output_file_name_heating_is_on,
                        self.lock_error_output_state_heating_is_on,
                    );
                    *c
                }
                Err(_) => {
                    self.lock_error_output_state_heating_is_on = true;
                    error!(target: module_path!(), "error locking mutex for heating status");
                    false
                }
            };
        }

        {
            // internal scope to limit lifetime of mutex lock
            *ventilation_status = match data_logger_mutexes.mutex_ventilation_status.lock() {
                Ok(c) => {
                    self.lock_error_output_state_ventilation_is_on = self.output_state(
                        *c,
                        "ON",
                        "OFF",
                        &self.config.output_file_name_ventilation_is_on,
                        self.lock_error_output_state_ventilation_is_on,
                    );
                    *c
                }
                Err(_) => {
                    self.lock_error_output_state_ventilation_is_on = true;
                    error!(target: module_path!(), "error locking mutex for ventilation status");
                    false
                }
            };
        }
    }

    /// Retrieves current sensor readings from the `SensorManager` via a shared mutex and outputs them to files.
    ///
    /// This private helper function locks the `mutex_sensor_manager_signals` to get the latest
    /// readings for water temperature, pH, conductivity, ambient temperature, and humidity.
    /// It updates the corresponding `Option<f32>` variables with the retrieved values and then
    /// calls `output_signal` to write these readings to their respective configured files on disk.
    /// If the mutex lock fails, an error is logged, and the function returns without updating the values.
    ///
    /// # Arguments
    /// * `mutex_sensor_manager_signals` - An `Arc<Mutex<SensorManagerSignals>>` holding the latest sensor readings.
    /// * `water_temperature_opt` - A mutable reference to an `Option<f32>` where the retrieved water temperature will be stored.
    /// * `ph_opt` - A mutable reference to an `Option<f32>` where the retrieved pH value will be stored.
    /// * `conductivity_opt` - A mutable reference to an `Option<f32>` where the retrieved conductivity value will be stored.
    /// * `ambient_temperature_opt` - A mutable reference to an `Option<f32>` where the retrieved ambient temperature will be stored.
    /// * `humidity_opt` - A mutable reference to an `Option<f32>` where the retrieved humidity will be stored.
    fn update_sensor_manager_signals(
        &mut self,
        mutex_sensor_manager_signals: Arc<Mutex<SensorManagerSignals>>,
        water_temperature_opt: &mut Option<f32>,
        ph_opt: &mut Option<f32>,
        conductivity_opt: &mut Option<f32>,
        ambient_temperature_opt: &mut Option<f32>,
        humidity_opt: &mut Option<f32>,
    ) {
        let water_temperature: f32;
        let ph: f32;
        let conductivity: f32;
        let ambient_temperature: f32;
        let humidity: f32;
        {
            // internal scope to limit lifetime of mutex lock
            match mutex_sensor_manager_signals.lock() {
                Ok(c) => {
                    self.lock_error_mutex_signal_manager = false;
                    water_temperature = c.water_temperature;
                    ph = c.ph;
                    conductivity = c.conductivity;
                    ambient_temperature = c.ambient_temperature;
                    humidity = c.ambient_humidity;
                }
                Err(_) => {
                    if !self.lock_error_mutex_signal_manager {
                        self.lock_error_mutex_signal_manager = true;
                        error!(target: module_path!(), "error locking mutex for sensor manager");
                    }
                    return;
                }
            };
            self.lock_error_output_signal_water_temperature = self.output_signal(
                water_temperature,
                AquariumSignal::WaterTemperature,
                &self.config.output_file_name_water_temperature,
                self.lock_error_output_signal_water_temperature,
            );
            self.lock_error_output_signal_pH = self.output_signal(
                ph,
                AquariumSignal::pH,
                &self.config.output_file_name_pH,
                self.lock_error_output_signal_pH,
            );
            self.lock_error_output_signal_conductivity = self.output_signal(
                conductivity,
                AquariumSignal::Conductivity,
                &self.config.output_file_name_conductivity,
                self.lock_error_output_signal_conductivity,
            );
            self.lock_error_output_signal_ambient_temperature = self.output_signal(
                ambient_temperature,
                AquariumSignal::AmbientTemperature,
                &self.config.output_file_name_ambient_temperature,
                self.lock_error_output_signal_ambient_temperature,
            );
            self.lock_error_output_signal_humidity = self.output_signal(
                humidity,
                AquariumSignal::AmbientHumidity,
                &self.config.output_file_name_humidity,
                self.lock_error_output_signal_humidity,
            );

            *water_temperature_opt = Some(water_temperature);
            *ph_opt = Some(ph);
            *conductivity_opt = Some(conductivity);
            *ambient_temperature_opt = Some(ambient_temperature);
            *humidity_opt = Some(humidity);
        }
    }

    /// Retrieves current signals from the `TankLevelSwitch` via a shared mutex and outputs them to files.
    ///
    /// This private helper function locks the `mutex_tank_level_switch_signals` to get the latest
    /// state for the switch's position, its stabilized position, and its invalid status.
    /// It updates the corresponding `Option<bool>` variables with the retrieved values and then
    /// calls `output_state` to write these states to their respective configured files on disk.
    /// If the mutex lock fails, the `Option` variables are set to `None`.
    ///
    /// # Arguments
    /// * `mutex_tank_level_switch_signals` - An `Arc<Mutex<TankLevelSwitchSignals>>` holding the latest switch states.
    /// * `tank_level_switch_position_opt` - A mutable reference to an `Option<bool>` for the switch's current position.
    /// * `tank_level_switch_position_stabilized_opt` - A mutable reference to an `Option<bool>` for the switch's stabilized position.
    /// * `tank_level_switch_invalid_opt` - A mutable reference to an `Option<bool>` for the switch's invalid status.
    fn update_tank_level_switch_signals(
        &mut self,
        mutex_tank_level_switch_signals: &Arc<Mutex<TankLevelSwitchSignals>>,
        tank_level_switch_position_opt: &mut Option<bool>,
        tank_level_switch_position_stabilized_opt: &mut Option<bool>,
        tank_level_switch_invalid_opt: &mut Option<bool>,
    ) {
        {
            // internal scope to limit lifetime of mutex lock
            match mutex_tank_level_switch_signals.lock() {
                Ok(tank_level_switch_signals) => {
                    *tank_level_switch_position_opt =
                        Some(tank_level_switch_signals.tank_level_switch_position);
                    *tank_level_switch_position_stabilized_opt =
                        Some(tank_level_switch_signals.tank_level_switch_position_stabilized);
                    *tank_level_switch_invalid_opt =
                        Some(tank_level_switch_signals.tank_level_switch_invalid);

                    self.lock_error_output_state_tank_level_switch_position = self.output_state(
                        tank_level_switch_signals.tank_level_switch_position,
                        "HIGH",
                        "LOW",
                        &self.config.output_file_name_tank_level_switch_position,
                        self.lock_error_output_state_tank_level_switch_position,
                    );

                    self.lock_error_output_state_tank_level_switch_position_stabilized = self
                        .output_state(
                            tank_level_switch_signals.tank_level_switch_position_stabilized,
                            "HIGH",
                            "LOW",
                            &self
                                .config
                                .output_file_name_tank_level_switch_position_stabilized,
                            self.lock_error_output_state_tank_level_switch_position_stabilized,
                        );

                    self.lock_error_output_state_tank_level_switch_invalid = self.output_state(
                        tank_level_switch_signals.tank_level_switch_invalid,
                        "INVALID",
                        "VALID",
                        &self.config.output_file_name_tank_level_switch_invalid,
                        self.lock_error_output_state_tank_level_switch_invalid,
                    );
                }
                Err(_) => {
                    *tank_level_switch_position_opt = None;
                    *tank_level_switch_position_stabilized_opt = None;
                    *tank_level_switch_invalid_opt = None;
                }
            };
        }
    }

    /// Calculates filtered values for water temperature, pH, and conductivity, then outputs them to files.
    ///
    /// This private helper function takes raw (unfiltered) sensor readings. For each valid reading,
    /// it applies its corresponding `IIRFilter` to smooth the data. The resulting filtered values
    /// are then stored in new `Option<f32>` variables and written to their designated files on disk.
    /// If a raw reading is `None`, no filtering or output occurs for that signal.
    ///
    /// # Arguments
    /// * `water_temperature` - The measured water temperature, wrapped in an `Option`.
    /// * `ph` - The measured pH value, wrapped in an `Option`.
    /// * `conductivity` - The measured conductivity value, wrapped in an `Option`.
    ///
    /// # Returns
    /// A tuple `(Option<f32>, Option<f32>, Option<f32>)` containing the filtered
    /// water temperature, pH, and conductivity values, respectively. Each value is
    /// `Some` if the corresponding input was valid and filtered, or `None` otherwise.
    fn calculate_filtered_signals(
        &mut self,
        water_temperature: Option<f32>,
        ph: Option<f32>,
        conductivity: Option<f32>,
        filter_water_temperature: &mut IIRFilter,
        filter_pH: &mut IIRFilter,
        filter_conductivity: &mut IIRFilter,
    ) -> (Option<f32>, Option<f32>, Option<f32>) {
        let water_temperature_filtered = match water_temperature {
            Some(water_temperature_data) => {
                let filtered_water_temperature =
                    filter_water_temperature.get_filtered_value(water_temperature_data);
                self.lock_error_output_signal_water_temperature_filtered = self.output_signal(
                    filtered_water_temperature,
                    AquariumSignal::WaterTemperature,
                    &self.config.output_file_name_water_temperature_filtered,
                    self.lock_error_output_signal_water_temperature_filtered,
                );
                Some(filtered_water_temperature)
            }
            None => None,
        };

        let pH_filtered = match ph {
            Some(ph_data) => {
                let filtered_pH = filter_pH.get_filtered_value(ph_data);
                self.lock_error_output_signal_pH_filtered = self.output_signal(
                    filtered_pH,
                    AquariumSignal::pH,
                    &self.config.output_file_name_pH_filtered,
                    self.lock_error_output_signal_pH_filtered,
                );
                Some(filtered_pH)
            }
            None => None,
        };

        let conductivity_filtered = match conductivity {
            Some(conductivity_data) => {
                let filtered_conductivity =
                    filter_conductivity.get_filtered_value(conductivity_data);
                self.lock_error_output_signal_conductivity_filtered = self.output_signal(
                    filtered_conductivity,
                    AquariumSignal::Conductivity,
                    &self.config.output_file_name_conductivity_filtered,
                    self.lock_error_output_signal_conductivity_filtered,
                );
                Some(filtered_conductivity)
            }
            None => None,
        };

        (
            water_temperature_filtered,
            pH_filtered,
            conductivity_filtered,
        )
    }

    /// Retrieves the current timestamp, either from the SQL database or generated locally by the application.
    ///
    /// This private helper function determines the timestamp source based on the `use_sql_timestamp`
    /// configuration setting. If `use_sql_timestamp` is `true`, it attempts to fetch the timestamp
    /// from the connected SQL database. If that fails, it falls back to using a locally generated
    /// timestamp and logs an error. If `use_sql_timestamp` is `false`, it always uses a local timestamp.
    /// The chosen timestamp is also written to a configured file on the disk.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active SQL database connection, used if the SQL timestamp option is enabled.
    ///
    /// # Returns
    /// A `NaiveDateTime` representing the current timestamp, sourced either from the database or the local system.
    fn update_timestamp(&mut self, conn: &mut PooledConn) -> NaiveDateTime {
        match self.config.use_sql_timestamp {
            true => match SqlInterface::get_current_timestamp(conn) {
                Ok(c) => {
                    self.lock_error_get_timestamp = false;
                    self.lock_error_output_timestamp = self.output_timestamp(
                        c,
                        &self.config.output_file_name_timestamp,
                        self.lock_error_output_timestamp,
                    );
                    c
                }
                Err(e) => {
                    if !self.lock_error_get_timestamp {
                        error!(
                            target: module_path!(),
                            "could not retrieve timestamp from database: ({e:?}) - using application-generated timestamp."
                        );
                        self.lock_error_get_timestamp = true;
                    }
                    Local::now().naive_local()
                }
            },
            false => Local::now().naive_local(),
        }
    }

    /// Confirms the application's readiness to quit and then waits for a specific termination command from the signal handler.
    ///
    /// This function is called when the thread has received the Quit-command from the signal handler.
    /// It first sends a confirmation back to the signal handler,
    /// indicating that this component is ready to proceed with shutdown.
    ///
    /// After confirming, it enters a loop, continuously listening for an `InternalCommand::Terminate`
    /// signal from the `signal_handler`. This ensures that the application's internal threads,
    /// which might be listening to this component's channels, stop receiving commands.
    ///
    /// # Arguments
    ///
    /// * `data_logger_channels` - A mutable reference to the struct that contains the channels.
    fn confirm_quit_and_wait_for_termination_command(
        &mut self,
        data_logger_channels: &mut DataLoggerChannels,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);

        data_logger_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads.
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination(
            &data_logger_channels.rx_data_logger_from_signal_handler,
            sleep_duration_hundred_millis,
            module_path!(),
        );
    }

    /// Executes the main control loop for the data logger module.
    ///
    /// This function runs continuously, acting as the central data collection and logging hub for
    /// the application.
    /// Before entering the loop, the function will set up Infinite Impulse Response (IIR) filters
    /// for various sensor signals.  
    /// The IIR filters for water temperature, pH, and conductivity are initialized
    /// with available data and a 1-second time step.
    ///
    /// It performs the following key tasks in a recurring cycle:
    /// 1. **Signal Retrieval**: Gathers current status (ON/OFF) from control modules (Refill, Heating, Ventilation) via channels.
    /// 2. **Sensor Data Acquisition**: Reads raw sensor values (temperatures, pH, conductivity, ambient conditions, tank level switch states) from shared mutexes, which are updated by other sensor-reading threads.
    /// 3. **Signal Filtering**: Applies IIR (Infinite Impulse Response) filters to sensor signals (e.g., water temperature, pH, conductivity) to produce smoothed values.
    /// 4. **Timestamping**: Determines the current timestamp, either from the SQL database or generated locally, based on configuration.
    /// 5. **DataFrame Construction**: Assembles all collected and processed data into a comprehensive `RecorderDataFrame`.
    /// 6. **Data Injection & Disk Output**: Transfers the `RecorderDataFrame` to the SQL database (via the `data_injection` trait) and writes various individual signal/state values to designated files on a RAM disk (if configured).
    /// 7. **Cycle Management**: Calculates the execution time of the current cycle and idles for the remaining duration to meet the configured `logging_interval`, ensuring precise data logging frequency. It warns if the cycle time is exceeded.
    /// 8. **Responsiveness**: Remains responsive to `Quit` commands from the signal handler during idle periods to enable graceful shutdown.
    ///
    /// The loop continues indefinitely until a `Quit` command is received from the signal handler.
    /// Upon receiving `Quit`, it performs a graceful exit sequence: sending confirmation back
    /// to the signal handler and then waiting for a `Terminate` command before finally exiting the thread.
    ///
    /// # Arguments
    /// * `data_injection` - An object implementing the `DataInjectionTrait`, used for transferring the collected `RecorderDataFrame` to the SQL database or a mock data structure during testing.
    /// * `sql_interface_data` - A `SqlInterfaceData` instance, providing the direct interface for writing `RecorderDataFrame`s to the SQL database.
    /// * `data_logger_channels` - A `DataLoggerChannels` struct, containing all the `mpsc` sender and receiver channels necessary for inter-thread communication with other modules and the signal handler.
    /// * `data_logger_mutexes` - A `DataLoggerMutexes` struct, providing access to shared `Arc<Mutex>` protected sensor data and device states.
    pub fn execute(
        &mut self,
        data_injection: &mut impl DataInjectionTrait,
        mut sql_interface_data: SqlInterfaceData,
        data_logger_channels: &mut DataLoggerChannels,
        data_logger_mutexes: DataLoggerMutexes,
    ) {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let cycle_time_duration =
            Duration::from_millis(self.config.logging_interval * MILLIS_PER_SEC);
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut i;

        // define and initialize with values which will be overwritten in the first iteration
        let mut water_temperature: Option<f32> = Some(10.0);
        let mut ph: Option<f32> = Some(0.0);
        let mut conductivity: Option<f32> = Some(0.0);
        let mut ambient_temperature: Option<f32> = Some(0.0);
        let mut ambient_humidity: Option<f32> = Some(0.0);
        let mut tank_level_switch_invalid: Option<bool> = Some(false);
        let mut tank_level_switch_position: Option<bool> = Some(true);
        let mut tank_level_switch_position_stabilized: Option<bool> = Some(true);
        let mut heater_is_on: bool = false;
        let mut ventilation_is_on: bool = false;
        let mut refill_in_progress_live: bool = false;
        let mut refill_in_progress_for_database: bool = false;
        let mut quit_command_received = false;
        let mut lock_warn_cycle_time_exceeded = false;
        let mut cycle_time_exceeded: bool; // ensures that signal handler/messaging are polled at least once even if cycle time is exceeded

        // update signals by accessing mutex for Atlas Scientific signals
        self.update_sensor_manager_signals(
            data_logger_mutexes.mutex_sensor_manager_signals.clone(),
            &mut water_temperature,
            &mut ph,
            &mut conductivity,
            &mut ambient_temperature,
            &mut ambient_humidity,
        );

        let mut filter_water_temperature = IIRFilter::new_requires_initial(
            &self.config.filter_coefficient_water_temperature,
            water_temperature.unwrap_or(25.0),
            1.0, // 1 second time step
        );
        let mut filter_pH = IIRFilter::new_requires_initial(
            &self.config.filter_coefficient_pH,
            ph.unwrap_or(7.0),
            1.0, // 1 second time step
        );
        let mut filter_conductivity = IIRFilter::new_requires_initial(
            &self.config.filter_coefficient_conductivity,
            conductivity.unwrap_or(50_000.0),
            1.0, // 1 second time step
        );

        let initial_sleep_time_start = Instant::now();
        while Instant::now()
            .duration_since(initial_sleep_time_start)
            .as_millis()
            < self.config.initial_sleep_time_millis.into()
        {
            spin_sleeper.sleep(sleep_duration_hundred_millis);
            (quit_command_received, _, _) = self.process_external_request(
                &data_logger_channels.rx_data_logger_from_signal_handler,
                None,
            );
            if quit_command_received {
                self.confirm_quit_and_wait_for_termination_command(data_logger_channels);
                return;
            }
        }

        // Initialize reference for cycle time measurement
        let mut start_time = Instant::now();

        loop {
            if self.config.active {
                self.update_signals(
                    &mut refill_in_progress_live,
                    &mut refill_in_progress_for_database,
                    true,
                    &mut heater_is_on,
                    &mut ventilation_is_on,
                    &data_logger_mutexes,
                );

                // update signals by accessing mutex for Atlas Scientific signals
                self.update_sensor_manager_signals(
                    data_logger_mutexes.mutex_sensor_manager_signals.clone(),
                    &mut water_temperature,
                    &mut ph,
                    &mut conductivity,
                    &mut ambient_temperature,
                    &mut ambient_humidity,
                );

                // update signals by accessing mutex for tank level switch signals
                self.update_tank_level_switch_signals(
                    &data_logger_mutexes.mutex_tank_level_switch_signals,
                    &mut tank_level_switch_position,
                    &mut tank_level_switch_position_stabilized,
                    &mut tank_level_switch_invalid,
                );

                let (water_temperature_filtered, pH_filtered, conductivity_filtered) = self
                    .calculate_filtered_signals(
                        water_temperature,
                        ph,
                        conductivity,
                        &mut filter_water_temperature,
                        &mut filter_pH,
                        &mut filter_conductivity,
                    );

                let current_timestamp = self.update_timestamp(&mut sql_interface_data.conn);

                // all data received, let's construct a data frame
                let data_frame = RecorderDataFrame {
                    timestamp: current_timestamp,
                    water_temperature,
                    water_temperature_filtered,
                    ph,
                    ph_filtered: pH_filtered,
                    conductivity,
                    conductivity_filtered,
                    conductivity_compensated: None,
                    refill_in_progress: Some(refill_in_progress_for_database),
                    tank_level_switch_position,
                    tank_level_switch_position_stabilized,
                    tank_level_switch_invalid,
                    surface_ventilation_status: Some(ventilation_is_on),
                    ambient_temperature,
                    ambient_humidity,
                    heater_status: Some(heater_is_on),
                };

                data_injection.inject_data(data_frame, &mut sql_interface_data);
            }
            let stop_time = Instant::now();
            let execution_duration = stop_time.duration_since(start_time);

            // deduct the execution time of the thread from the target cycle time
            let remaining_sleep_time_millis: u64;
            if execution_duration > cycle_time_duration {
                if !lock_warn_cycle_time_exceeded {
                    warn!(target: module_path!(),
                        "execution duration of {} milliseconds exceeds cycle time of {}",
                        execution_duration.as_millis(),
                        cycle_time_duration.as_millis());
                    lock_warn_cycle_time_exceeded = true;
                }
                remaining_sleep_time_millis = 0;
                cycle_time_exceeded = true;
            } else {
                // underflow cannot happen because the else statement requires that the first operand is higher than the second
                lock_warn_cycle_time_exceeded = false;
                let remaining_sleep_time_duration = cycle_time_duration - execution_duration;
                remaining_sleep_time_millis = remaining_sleep_time_duration.as_millis() as u64;
                cycle_time_exceeded = false;
            }

            #[cfg(feature = "debug_data_logger")]
            debug!(
                target: module_path!(),
                "inserted data into database, now idling for {} milliseconds",
                remaining_sleep_time_millis
            );

            // wait the remaining time period to complete the target cycle time
            i = 0;
            while i < remaining_sleep_time_millis || cycle_time_exceeded {
                i += 100;
                spin_sleeper.sleep(sleep_duration_hundred_millis);
                (quit_command_received, _, _) = self.process_external_request(
                    &data_logger_channels.rx_data_logger_from_signal_handler,
                    None,
                );
                if quit_command_received {
                    #[cfg(feature = "debug_data_logger")]
                    debug!(
                        target: module_path!(),
                        "received QUIT command from signal handler"
                    );
                    break;
                }

                if self.config.active {
                    // once per second, retrieve signals and write them to the RAM disk
                    if i % 1000 == 0 {
                        self.update_signals(
                            &mut refill_in_progress_live,
                            &mut refill_in_progress_for_database,
                            false,
                            &mut heater_is_on,
                            &mut ventilation_is_on,
                            &data_logger_mutexes,
                        );

                        // update signals by accessing mutex for Atlas Scientific signals
                        self.update_sensor_manager_signals(
                            data_logger_mutexes.mutex_sensor_manager_signals.clone(),
                            &mut water_temperature,
                            &mut ph,
                            &mut conductivity,
                            &mut ambient_temperature,
                            &mut ambient_humidity,
                        );

                        // update signals by accessing mutex for tank level switch signals
                        self.update_tank_level_switch_signals(
                            &data_logger_mutexes.mutex_tank_level_switch_signals,
                            &mut tank_level_switch_position,
                            &mut tank_level_switch_position_stabilized,
                            &mut tank_level_switch_invalid,
                        );
                    }
                }
                cycle_time_exceeded = false;
            }
            if quit_command_received {
                break;
            }

            #[cfg(feature = "debug_data_logger")]
            debug!(
                target: module_path!(),
                "finished idling restarting loop"
            );

            self.check_timing_and_ping_database(&mut sql_interface_data);

            start_time = Instant::now(); // start time for the next cycle
        }

        self.confirm_quit_and_wait_for_termination_command(data_logger_channels);
    }
}

#[cfg(test)]
pub mod tests {
    use crate::database::{sql_interface::SqlInterface, sql_interface_data::SqlInterfaceData};
    use crate::launch::channels::Channels;
    use crate::mocks::mock_data_injection::tests::MockDataInjection;
    use crate::mocks::mock_heating::tests::mock_heating;
    use crate::mocks::mock_signal::tests::MockSignal;
    use crate::mocks::mock_ventilation::tests::mock_ventilation;
    use crate::mocks::test_command::tests::TestCommand;
    use crate::recorder::data_logger::{DataLogger, DataLoggerConfig, DataLoggerMutexes};
    use crate::sensors::sensor_manager::SensorManagerSignals;
    use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};
    use crate::water::refill::RefillStatus;
    use spin_sleep::SpinSleeper;
    use std::sync::{Arc, Mutex};
    use std::{fs, sync::mpsc, thread, time::Duration};

    #[cfg(test)]
    // Asserts the content of all data logger output files on disk.
    //
    // This comprehensive helper function is used in test environments to verify that
    // the `DataLogger` correctly writes various sensor signals and device states
    // to their respective configured files. It reads each file, parses its content
    // (for floating-point numbers) or compares it directly (for boolean states),
    // and asserts that it matches the expected target values.
    //
    // Arguments:
    // * `data_logger_config`: The `DataLoggerConfig` struct containing the names of all output files.
    // * `target_water_temperature`: The expected `f32` value for water temperature.
    // * `target_water_temperature_filtered`: The expected `f32` value for filtered water temperature.
    // * `target_water_ph`: The expected `f32` value for pH.
    // * `target_water_ph_filtered`: The expected `f32` value for filtered pH.
    // * `target_water_conductivity`: The expected `f32` value for conductivity.
    // * `target_water_conductivity_filtered`: The expected `f32` value for filtered conductivity.
    // * `_target_water_conductivity_compensated`: (Ignored) Placeholder for a compensated conductivity value.
    // * `target_refill_in_progress`: The expected `bool` state for refill in progress.
    // * `target_tank_level_switch_position`: The expected `bool` state for tank level switch position.
    // * `target_tank_level_switch_invalid`: The expected `bool` state for tank level switch invalid status.
    // * `target_tank_level_switch_position_stabilized`: The expected `bool` state for stabilized tank level switch position.
    // * `target_surface_ventilation_status`: The expected `bool` state for surface ventilation status.
    // * `target_ambient_temperature`: The expected `f32` value for ambient temperature.
    // * `target_ambient_humidity`: The expected `f32` value for ambient humidity.
    // * `target_heater_status`: The expected `bool` state for heater status.
    // * `target_water_temperature_ds18b20`: The expected `f32` value for DS18B20 water temperature.
    // * `target_ambient_temperature_ds18b20`: The expected `f32` value for DS18B20 ambient temperature.
    //
    // Panics:
    // This function will panic if:
    // - It fails to read any of the specified output files.
    // - It fails to parse a file's content into the expected numeric type.
    // - The content of any file does not match its corresponding target value.
    fn assert_output_file_contents(
        data_logger_config: &DataLoggerConfig,
        target_water_temperature: f32,
        target_water_temperature_filtered: f32,
        target_water_ph: f32,
        target_water_ph_filtered: f32,
        target_water_conductivity: f32,
        target_water_conductivity_filtered: f32,
        _target_water_conductivity_compensated: f32,
        target_refill_in_progress: bool,
        target_tank_level_switch_position: bool,
        target_tank_level_switch_invalid: bool,
        target_tank_level_switch_position_stabilized: bool,
        target_surface_ventilation_status: bool,
        target_ambient_temperature: f32,
        target_ambient_humidity: f32,
        target_heater_status: bool,
    ) {
        // Helper to assert the content of a file containing a f32 value.
        let assert_file_f32 = |filename: &str, expected: f32| {
            let content = fs::read_to_string(filename)
                .unwrap_or_else(|_| panic!("Could not read file: {}", filename));
            let value = content
                .parse::<f32>()
                .unwrap_or_else(|_| panic!("Could not parse f32 from file: {}", filename));
            assert_eq!(value, expected, "Mismatch in file: {}", filename);
        };

        // Helper to assert the content of a file containing a boolean state.
        let assert_file_bool =
            |filename: &str, expected_bool: bool, true_str: &str, false_str: &str| {
                let content = fs::read_to_string(filename)
                    .unwrap_or_else(|_| panic!("Could not read file: {}", filename));
                let expected_str = if expected_bool { true_str } else { false_str };
                assert_eq!(content, expected_str, "Mismatch in file: {}", filename);
            };

        // Now, the assertions are clean, single-line calls.
        assert_file_f32(
            &data_logger_config.output_file_name_water_temperature,
            target_water_temperature,
        );
        assert_file_f32(
            &data_logger_config.output_file_name_water_temperature_filtered,
            target_water_temperature_filtered,
        );
        assert_file_f32(&data_logger_config.output_file_name_pH, target_water_ph);
        assert_file_f32(
            &data_logger_config.output_file_name_pH_filtered,
            target_water_ph_filtered,
        );
        assert_file_f32(
            &data_logger_config.output_file_name_conductivity,
            target_water_conductivity,
        );
        assert_file_f32(
            &data_logger_config.output_file_name_conductivity_filtered,
            target_water_conductivity_filtered,
        );
        assert_file_f32(
            &data_logger_config.output_file_name_ambient_temperature,
            target_ambient_temperature,
        );
        assert_file_f32(
            &data_logger_config.output_file_name_humidity,
            target_ambient_humidity,
        );

        assert_file_bool(
            &data_logger_config.output_file_name_refill_in_progress,
            target_refill_in_progress,
            "ON",
            "OFF",
        );
        assert_file_bool(
            &data_logger_config.output_file_name_tank_level_switch_position,
            target_tank_level_switch_position,
            "HIGH",
            "LOW",
        );
        assert_file_bool(
            &data_logger_config.output_file_name_tank_level_switch_invalid,
            target_tank_level_switch_invalid,
            "INVALID",
            "VALID",
        );
        assert_file_bool(
            &data_logger_config.output_file_name_tank_level_switch_position_stabilized,
            target_tank_level_switch_position_stabilized,
            "HIGH",
            "LOW",
        );
        assert_file_bool(
            &data_logger_config.output_file_name_ventilation_is_on,
            target_surface_ventilation_status,
            "ON",
            "OFF",
        );
        assert_file_bool(
            &data_logger_config.output_file_name_heating_is_on,
            target_heater_status,
            "ON",
            "OFF",
        );
    }

    #[test]
    // test case checks if DataLogger correctly processes all information:
    // - storage in SQL database using mock implementation
    // - writing output to the filesystem
    // Test case uses test database #45.
    pub fn test_data_log() {
        let mut channels = Channels::new_for_test();

        // test environment-specific channels
        let (tx_test_environment_to_ventilation, rx_ventilation_from_test_environment) =
            mpsc::channel();
        let (tx_test_environment_to_heating, rx_heating_from_test_environment) = mpsc::channel();

        let mut mock_data_injection = MockDataInjection::new();

        let mut config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            45,
        );
        config.data_logger.logging_interval = 1;

        let mut config2: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            45,
        );
        config2.data_logger.logging_interval = 1;

        let max_rows_data = config.sql_interface.max_rows_data;

        let sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_data =
            SqlInterfaceData::new(sql_interface.get_connection().unwrap(), max_rows_data).unwrap();
        let mut data_logger =
            DataLogger::new(config.data_logger, Duration::from_millis(1000)).unwrap();

        // replacement values are used to initialize the mutex content
        config.sensor_manager.replacement_value_water_temperature = 10.0;
        config.sensor_manager.replacement_value_ph = 11.0;
        config.sensor_manager.replacement_value_conductivity = 12.0;

        let mutex_ds18b20_water_temperature = Arc::new(Mutex::new(15.0));
        let mutex_ds18b20_ambient_temperature = Arc::new(Mutex::new(16.0));
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
        let mutex_tank_level_switch_signals_clone = mutex_tank_level_switch_signals.clone();
        let mutex_heating_status = Arc::new(Mutex::new(false));
        let mutex_heating_status_clone_for_data_logger = mutex_heating_status.clone();
        let mutex_ventilation_status = Arc::new(Mutex::new(false));
        let mutex_ventilation_status_clone_for_data_logger = mutex_ventilation_status.clone();
        let refill_status = RefillStatus {
            refill_in_progress_live: false,
            refill_in_progress_for_database: false,
        };
        let mutex_refill_status = Arc::new(Mutex::new(refill_status));
        let mutex_refill_status_clone_for_data_logger = mutex_refill_status.clone();
        let mutex_refill_status_clone_for_test_environment = mutex_refill_status.clone();
        config.sensor_manager.replacement_value_ambient_temperature = 13.0;
        config.sensor_manager.replacement_value_ambient_humidity = 14.0;
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));
        let mutex_sensor_manager_signals_clone_for_data_logger =
            mutex_sensor_manager_signals.clone();
        // thread for test environment and mock signal handler
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let spin_sleeper = SpinSleeper::default();
                let sleep_duration_1_second = Duration::from_millis(1000);
                let sleep_duration_100_millis = Duration::from_millis(100);
                spin_sleeper.sleep(sleep_duration_1_second);
                spin_sleeper.sleep(sleep_duration_100_millis);

                let _ = tx_test_environment_to_ventilation.send(TestCommand::UpdateSignal(
                    MockSignal::VentilationControlStatus(true),
                ));
                {
                    let mut sensor_manager_signals = mutex_sensor_manager_signals.lock().unwrap();
                    sensor_manager_signals.water_temperature = 29.0;
                }

                spin_sleeper.sleep(sleep_duration_1_second);

                let _ = tx_test_environment_to_heating.send(TestCommand::UpdateSignal(
                    MockSignal::HeatingControlStatus(true),
                ));

                {
                    let mut sensor_manager_signals = mutex_sensor_manager_signals.lock().unwrap();
                    sensor_manager_signals.ph = 7.0;
                }

                spin_sleeper.sleep(sleep_duration_1_second);

                {
                    let mut sensor_manager_signals = mutex_sensor_manager_signals.lock().unwrap();
                    sensor_manager_signals.conductivity = 50000.0;
                }

                spin_sleeper.sleep(sleep_duration_1_second);

                {
                    let mut tank_level_switch_signals =
                        mutex_tank_level_switch_signals.lock().unwrap();
                    tank_level_switch_signals.tank_level_switch_invalid = true;
                }

                {
                    let mut sensor_manager_signals = mutex_sensor_manager_signals.lock().unwrap();
                    sensor_manager_signals.ambient_temperature = 35.0;
                }

                {
                    let mut refill_status = mutex_refill_status_clone_for_test_environment
                        .lock()
                        .unwrap();
                    refill_status.refill_in_progress_for_database = true;
                }

                spin_sleeper.sleep(sleep_duration_1_second);

                {
                    let mut tank_level_switch_signals =
                        mutex_tank_level_switch_signals.lock().unwrap();
                    tank_level_switch_signals.tank_level_switch_position = true;
                }

                {
                    let mut sensor_manager_signals = mutex_sensor_manager_signals.lock().unwrap();
                    sensor_manager_signals.ambient_humidity = 66.0;
                }

                spin_sleeper.sleep(sleep_duration_1_second);

                {
                    let mut water_temperature_ds18b20 =
                        mutex_ds18b20_water_temperature.lock().unwrap();
                    *water_temperature_ds18b20 = 15.5;
                    let mut ambient_temperature_ds18b20 =
                        mutex_ds18b20_ambient_temperature.lock().unwrap();
                    *ambient_temperature_ds18b20 = 16.5;
                }

                {
                    let mut tank_level_switch_signals =
                        mutex_tank_level_switch_signals.lock().unwrap();
                    tank_level_switch_signals.tank_level_switch_position_stabilized = true;
                }

                spin_sleeper.sleep(sleep_duration_1_second);

                // cease operation of the test object
                let _ = channels.signal_handler.send_to_data_logger(InternalCommand::Quit);
                channels.signal_handler.receive_from_data_logger().unwrap();

                // signal end of test case execution to mock threads
                let _ = channels.signal_handler.send_to_heating(InternalCommand::Quit);
                let _ = channels.signal_handler.send_to_ventilation(InternalCommand::Quit);

                spin_sleeper.sleep(sleep_duration_1_second);
                let _ = channels.signal_handler.send_to_data_logger(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for mock heating
        let join_handle_mock_heating = thread::Builder::new()
            .name("mock_heating".to_string())
            .spawn(move || {
                mock_heating(
                    None,
                    None,
                    &channels.heating.rx_heating_from_signal_handler,
                    Some(&rx_heating_from_test_environment),
                    mutex_heating_status, // initial heating control status
                );
            })
            .unwrap();

        // thread for mock ventilation
        let join_handle_mock_ventilation = thread::Builder::new()
            .name("mock_ventilation".to_string())
            .spawn(move || {
                mock_ventilation(
                    &channels.ventilation.rx_ventilation_from_signal_handler,
                    Some(&rx_ventilation_from_test_environment),
                    mutex_ventilation_status, // initial ventilation control status
                );
            })
            .unwrap();

        // give mock threads some time to establish
        let spin_sleeper = SpinSleeper::default();
        let sleep_duration_100_millis = Duration::from_millis(100);
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object and asserts
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {

                let data_logger_mutexes = DataLoggerMutexes {
                    mutex_sensor_manager_signals:
                        mutex_sensor_manager_signals_clone_for_data_logger,
                    mutex_tank_level_switch_signals: mutex_tank_level_switch_signals_clone,
                    mutex_heating_status: mutex_heating_status_clone_for_data_logger,
                    mutex_ventilation_status: mutex_ventilation_status_clone_for_data_logger,
                    mutex_refill_status: mutex_refill_status_clone_for_data_logger,
                };

                data_logger.execute(
                    &mut mock_data_injection,
                    sql_interface_data,
                    &mut channels.data_logger,
                    data_logger_mutexes,
                );

                println!("Printing recorded data:");
                mock_data_injection.output();

                // Assert if data logger has set the refill-in-progress bit to false.
                {
                    let refill_status = mutex_refill_status.lock().unwrap();
                    println!(
                        "Checking if data logger has set the refill-in-progress bit to false..."
                    );
                    assert_eq!(refill_status.refill_in_progress_for_database, false);
                }

                // Dataframe #5
                let mut data_frame = mock_data_injection.data_frames.pop().unwrap();
                println!("Asserting data frame #5");
                data_frame.assert(
                    Some(29.0),     // target_water_temperature
                    Some(28.40625), // target_water_temperature_filtered
                    Some(7.0),      // target_water_ph
                    Some(7.125),    // target_water_ph_filtered
                    Some(50_000.0), // target_water_conductivity
                    Some(46875.75), // target_water_conductivity_filtered
                    None,           // target_water_conductivity_compensated
                    Some(false),    // target_refill_in_progress
                    Some(true),     // target_tank_level_switch_position
                    Some(true),     // target_tank_level_switch_position_stabilized
                    Some(true),     // target_tank_level_switch_invalid
                    Some(true),     // target_surface_ventilation_status
                    Some(35.0),     // target_ambient_temperature
                    Some(66.0),     // target_ambient_humidity
                    Some(true),     // target_heater_status
                );

                // Dataframe #4
                data_frame = mock_data_injection.data_frames.pop().unwrap();
                println!("Asserting data frame #4");
                data_frame.assert(
                    Some(29.0),     // target_water_temperature
                    Some(27.8125),  // target_water_temperature_filtered
                    Some(7.0),      // target_water_ph
                    Some(7.25),     // target_water_ph_filtered
                    Some(50_000.0), // target_water_conductivity
                    Some(43751.5),  // target_water_conductivity_filtered
                    None,           // target_water_conductivity_compensated
                    Some(false),    // target_refill_in_progress
                    Some(true),     // target_tank_level_switch_position
                    Some(false),    // target_tank_level_switch_position_stabilized
                    Some(true),     // target_tank_level_switch_invalid
                    Some(true),     // target_surface_ventilation_status
                    Some(35.0),     // target_ambient_temperature
                    Some(66.0),     // target_ambient_humidity
                    Some(true),     // target_heater_status
                );

                // Dataframe #3
                data_frame = mock_data_injection.data_frames.pop().unwrap();
                println!("Asserting data frame #3");
                data_frame.assert(
                    Some(29.0),     // target_water_temperature
                    Some(26.625),   // target_water_temperature_filtered
                    Some(7.0),      // target_water_ph
                    Some(7.5),      // target_water_ph_filtered
                    Some(50_000.0), // target_water_conductivity
                    Some(37503.0),  // target_water_conductivity_filtered
                    None,           // target_water_conductivity_compensated
                    Some(true),     // target_refill_in_progress
                    Some(false),    // target_tank_level_switch_position
                    Some(false),    // target_tank_level_switch_position_stabilized
                    Some(true),     // target_tank_level_switch_invalid
                    Some(true),     // target_surface_ventilation_status
                    Some(35.0),     // target_ambient_temperature
                    Some(14.0),     // target_ambient_humidity
                    Some(true),     // target_heater_status
                );

                // Dataframe #2
                data_frame = mock_data_injection.data_frames.pop().unwrap();
                println!("Asserting data frame #2");
                data_frame.assert(
                    Some(29.0),     // target_water_temperature
                    Some(24.25),    // target_water_temperature_filtered
                    Some(7.0),      // target_water_ph
                    Some(8.0),      // target_water_ph_filtered
                    Some(50_000.0), // target_water_conductivity
                    Some(25006.0),  // target_water_conductivity_filtered
                    None,           // target_water_conductivity_compensated
                    Some(false),    // target_refill_in_progress
                    Some(false),    // target_tank_level_switch_position
                    Some(false),    // target_tank_level_switch_position_stabilized
                    Some(false),    // target_tank_level_switch_invalid
                    Some(true),     // target_surface_ventilation_status
                    Some(13.0),     // target_ambient_temperature
                    Some(14.0),     // target_ambient_humidity
                    Some(true),     // target_heater_status
                );

                // Dataframe #1
                data_frame = mock_data_injection.data_frames.pop().unwrap();
                println!("Asserting data frame #1");
                data_frame.assert(
                    Some(29.0),  // target_water_temperature
                    Some(19.5),  // target_water_temperature_filtered
                    Some(7.0),   // target_water_ph
                    Some(9.0),   // target_water_ph_filtered
                    Some(12.0),  // target_water_conductivity
                    Some(12.0),  // target_water_conductivity_filtered
                    None,        // target_water_conductivity_compensated
                    Some(false), // target_refill_in_progress
                    Some(false), // target_tank_level_switch_position
                    Some(false), // target_tank_level_switch_position_stabilized
                    Some(false), // target_tank_level_switch_invalid
                    Some(true),  // target_surface_ventilation_status
                    Some(13.0),  // target_ambient_temperature
                    Some(14.0),  // target_ambient_humidity
                    Some(true),  // target_heater_status
                );

                assert!(mock_data_injection.data_frames.is_empty());

                assert_output_file_contents(
                    &config2.data_logger, // data_logger_config,
                    29.0,                 // target_water_temperature
                    28.4,                 // target_water_temperature_filtered
                    7.0,                  // target_water_ph
                    7.12,                 // target_water_ph_filtered
                    50_000.0,             // target_water_conductivity
                    46876.0,              // target_water_conductivity_filtered
                    0.0,                  // target_water_conductivity_compensated
                    false,                // target_refill_in_progress
                    true,                 // target_tank_level_switch_position
                    true,                 // target_tank_level_switch_invalid
                    true,                 // target_tank_level_switch_position_stabilized
                    true,                 // target_surface_ventilation_status
                    35.0,                 // target_ambient_temperature
                    66.0,                 // target_ambient_humidity
                    true,                 // target_heater_status
                );
            })
            .unwrap();

        join_handle_mock_ventilation
            .join()
            .expect("Mock ventilation thread did not finish.");
        join_handle_mock_heating
            .join()
            .expect("Mock heating thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Mock data logger thread did not finish.");
    }
}
