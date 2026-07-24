/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Implements the central data collection, filtering, and logging thread.
//!
//! This module provides the `DataLogger` struct, which runs as a dedicated, long-running
//! thread. Its primary responsibility is to act as the central hub for all sensor data
//! and device states within the application. It periodically collects this data,
//! processes it, and then dispatches it to two destinations: an SQL database for
//! historical persistence and a set of files on a RAM disk for real-time monitoring.
//!
//! ## Key Components
//!
//! - **`DataLogger` Struct**: The core state machine for the thread. It holds the
//!   configuration, a large number of stateful boolean "lock" flags for log
//!   suppression, and the IIR filters for signal smoothing.
//!
//! - **`execute()` Method**: The main thread loop. In each cycle, it performs a
//!   sequence of operations:
//!   1.  **Data Acquisition**: Locks the various mutexes in `DataLoggerMutexes` to
//!       get the latest raw sensor readings and device statuses.
//!   2.  **Signal Filtering**: Applies IIR (Infinite Impulse Response) filters to
//!       the raw water temperature, pH, and conductivity signals to produce
//!       smoothed, more stable values.
//!   3.  **Timestamping**: Fetches a reliable timestamp, either from the SQL
//!       database or the local system clock, based on the configuration.
//!   4.  **Data Aggregation**: Assembles all raw, filtered, and status data into a
//!       single `RecorderDataFrame`.
//!   5.  **Dual Output**:
//!       - Injects the complete `RecorderDataFrame` into the SQL database via the
//!         `DataInjectionTrait`.
//!       - Writes individual data points to separate files on a RAM disk (if
//!         enabled), used by the web server and mobile applications for live-data display
//!
//! ## Design and Architecture
//!
//! This module is designed for robustness, testability, and real-time visibility.
//!
//! - **Decoupled Data Flow**: The `DataLogger` pulls data from other threads via
//!   shared mutexes. This decouples it from the sensor-reading threads; the
//!   `DataLogger` doesn't need to know how the data is produced, only where to find it.
//!
//! - **Robust Error Handling (Log Flood Prevention)**: The extensive use of `lock_...`
//!   flags is a deliberate design choice for a resilient, long-running service. If a
//!   recurring problem occurs (e.g., failure to write to a file on the RAM disk),
//!   the error is logged *once*. The lock is then engaged to suppress identical log
//!   messages until the underlying condition is resolved. This prevents a single,
//!   persistent issue from filling up disk space with repetitive logs.
//!
//! - **Testability**: The database persistence logic is abstracted behind the
//!   `DataInjectionTrait`. This allows the `DataLogger`'s complex logic to be
//!   unit-tested by providing a `MockDataInjection` implementation, completely
//!   isolating the test from any database dependency.
//!
//! - **Dual-Destination Logging**: The separation of logging destinations serves two
//!   distinct purposes: the SQL database provides a structured, long-term historical
//!   record, while the individual files on the RAM disk offer a low-latency,
//!   easy-to-parse source for live data visualization.

#![allow(non_snake_case)]
use chrono::{Local, NaiveDateTime};
#[cfg(all(not(feature = "test-utils"), not(test), target_os = "linux"))]
// only required in real-life execution
use log::info;
use log::{error, warn};

#[cfg(feature = "debug_data_logger")]
use log::debug;

#[cfg(all(not(feature = "test-utils"), not(test), target_os = "linux"))]
// only required in real-life execution
use nix::unistd::gettid;

use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_data::SqlInterfaceData;
use crate::recorder::data_injection::DataInjectionTrait;
use crate::recorder::data_logger_channels::DataLoggerChannels;
use crate::recorder::data_logger_config::DataLoggerConfig;
use crate::recorder::data_logger_error::DataLoggerError;
use crate::recorder::data_logger_mutexes::DataLoggerMutexes;
use crate::recorder::recorder_data_frame::RecorderDataFrame;
use crate::sensors::sensor_manager::SensorManagerSignals;
use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::channel_content::AquariumSignal;
use crate::utilities::iir_filter::IIRFilter;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;

const MILLIS_PER_SEC: u64 = 1000;

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

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to lock heating status mutex
    lock_error_output_state_heating_status: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write heating is on to disk
    lock_error_output_state_heating_is_on: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write heating actuator effectiveness status to disk
    lock_error_output_state_heating_actuator_ineffective: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write heating actuator disabled status to disk
    lock_error_output_state_heating_actuator_disabled: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to lock ventilation status mutex
    lock_error_output_state_ventilation_status: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write ventilation is on to disk
    lock_error_output_state_ventilation_is_on: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write ventilation actuator effectiveness status to disk
    lock_error_output_state_ventilation_actuator_ineffective: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write ventilation actuator disabled status to disk
    lock_error_output_state_ventilation_actuator_disabled: bool,

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

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write temperature gradient to disk
    lock_error_output_signal_temperature_gradient: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write timestamp to disk
    lock_error_output_timestamp: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about having received an inapplicable command
    pub lock_warn_inapplicable_command_signal_handler: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    pub lock_error_channel_receive_termination: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to lock mutex
    lock_error_mutex_signal_manager: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write the refill controller state to disk
    lock_error_output_description_refill_controller_state: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write the heating controller state to disk
    lock_error_output_description_heating_controller_state: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write the ventilation controller state to disk
    lock_error_output_description_ventilation_controller_state: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to execute a task for retrieving date from the database
    lock_error_task_failure: bool,
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
    ///
    /// # Returns
    /// A `Result` containing a new, initialized `DataLogger` instance on success.
    ///
    /// # Errors
    /// This function will return a `DataLoggerError` if `output_to_disk` is enabled and:
    /// - Any of the configured output filenames are empty (`OutputFilenameEmpty`).
    /// - It fails to write to (and thus reset) any of the configured output files,
    ///   which could be due to permission issues or an invalid path (`FileResetFailure`).
    pub fn new(config: DataLoggerConfig) -> Result<DataLogger, DataLoggerError> {
        if config.output_to_disk {
            // By defining the files in a collection, we can use iterators to check and
            // initialize them
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
                (
                    &config.output_file_name_ventilation_actuator_ineffective,
                    "ventilation actuator ineffective",
                ),
                (
                    &config.output_file_name_ventilation_actuator_disabled,
                    "ventilation actuator disabled",
                ),
                (&config.output_file_name_heating_is_on, "heating status"),
                (
                    &config.output_file_name_heating_actuator_ineffective,
                    "heating actuator ineffective",
                ),
                (
                    &config.output_file_name_heating_actuator_disabled,
                    "heating actuator disabled",
                ),
                (
                    &config.output_file_name_ambient_temperature,
                    "ambient temperature",
                ),
                (&config.output_file_name_humidity, "humidity"),
                (
                    &config.output_file_name_temperature_gradient,
                    "temperature gradient",
                ),
                (
                    &config.output_file_name_refill_in_progress,
                    "refill in progress",
                ),
                (
                    &config.output_file_name_refill_controller_state,
                    "refill controller state",
                ),
                (
                    &config.output_file_name_heating_controller_state,
                    "heating controller state",
                ),
                (
                    &config.output_file_name_ventilation_controller_state,
                    "ventilation controller state",
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

        // check the validity of filter coefficients
        if !(config.filter_coefficient_conductivity.filter_coefficient > 0.0
            && config.filter_coefficient_conductivity.filter_coefficient <= 1.0)
        {
            return Err(DataLoggerError::InvalidFilterCoefficient(
                module_path!().to_string(),
                "conductivity".to_string(),
                config.filter_coefficient_conductivity.filter_coefficient,
            ));
        }
        if !(config
            .filter_coefficient_water_temperature
            .filter_coefficient
            > 0.0
            && config
                .filter_coefficient_water_temperature
                .filter_coefficient
                <= 1.0)
        {
            return Err(DataLoggerError::InvalidFilterCoefficient(
                module_path!().to_string(),
                "water temperature".to_string(),
                config
                    .filter_coefficient_water_temperature
                    .filter_coefficient,
            ));
        }
        if !(config.filter_coefficient_pH.filter_coefficient > 0.0
            && config.filter_coefficient_pH.filter_coefficient <= 1.0)
        {
            return Err(DataLoggerError::InvalidFilterCoefficient(
                module_path!().to_string(),
                "pH".to_string(),
                config.filter_coefficient_pH.filter_coefficient,
            ));
        }

        Ok(Self {
            config,
            lock_error_get_timestamp: false,
            lock_error_output_signal_water_temperature: false,
            lock_error_output_state_refill_in_progress: false,
            lock_error_output_state_heating_status: false,
            lock_error_output_state_heating_is_on: false,
            lock_error_output_state_heating_actuator_ineffective: false,
            lock_error_output_state_heating_actuator_disabled: false,
            lock_error_output_state_ventilation_status: false,
            lock_error_output_state_ventilation_is_on: false,
            lock_error_output_state_ventilation_actuator_ineffective: false,
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
            lock_error_output_signal_temperature_gradient: false,
            lock_error_output_timestamp: false,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
            lock_error_mutex_signal_manager: false,
            lock_error_output_description_refill_controller_state: false,
            lock_error_output_description_heating_controller_state: false,
            lock_error_output_state_ventilation_actuator_disabled: false,
            lock_error_output_description_ventilation_controller_state: false,
            lock_error_task_failure: false,
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

    /// Outputs a string to a specified file, typically located on a RAM disk.
    ///
    /// This private helper function outputs a descriptive string (e.g., "ACTIVE", "ERROR")
    /// to the given `output_file_name`. This operation only occurs if `output_to_disk`
    /// is enabled in the data logger's configuration.
    ///
    /// Error logging for writing failures is controlled by the `inhibit_error_log` flag,
    /// preventing a flood of repetitive error messages.
    ///
    /// # Arguments
    /// * `description` - The text to be written to the output file.
    /// * `output_file_name` - A reference to the `String` containing the full path to the output file.
    /// * `inhibit_error_log` - A boolean flag that, if `true`, will prevent this specific
    ///   error from being logged to the console/main log if a write operation fails.
    ///
    /// # Returns
    /// - `true`: If an error occurred during the file write operation.
    /// - `false`: If the write-operation was successful or if `output_to_disk` is disabled.
    fn output_description(
        &self,
        description: &str,
        output_file_name: &String,
        inhibit_error_log: bool,
    ) -> bool {
        if self.config.output_to_disk {
            match fs::write(output_file_name, description) {
                Ok(_) => {
                    /* do nothing */
                    false // indicate no error
                }
                Err(e) => {
                    if !inhibit_error_log {
                        error!(
                            target: module_path!(),
                            "output of value {description} to {output_file_name} failed: {e:?}"
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
    /// * `refill_status_live` - Unmodified refill status.
    /// * `refill_status_for_database` - Refill status adapted for database recording.
    /// * `refill_reset_for_database` - Refill reset flag adapted for database recording.
    /// * `data_logger_mutexes` - A parameter representing `data_logger_mutexes`.
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
            //#[cfg(feature = "debug_data_logger")]
            //log::debug!(target: module_path!(), "data_logger: Attempting to lock mutex_refill_status");
            match data_logger_mutexes.mutex_refill_status.lock() {
                Ok(mut refill_status) => {
                    self.lock_error_output_state_refill_in_progress = self.output_state(
                        refill_status.refill_in_progress_live, // use the live signal
                        "ON",
                        "OFF",
                        &self.config.output_file_name_refill_in_progress,
                        self.lock_error_output_state_refill_in_progress,
                    );
                    self.lock_error_output_description_refill_controller_state = self
                        .output_description(
                            refill_status.description(), // specific description
                            &self.config.output_file_name_refill_controller_state,
                            self.lock_error_output_description_refill_controller_state,
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
            //#[cfg(feature = "debug_data_logger")]
            //debug!(target: module_path!(), "Releasing lock on mutex_refill_status");
        }

        {
            // internal scope to limit lifetime of mutex lock
            //#[cfg(feature = "debug_data_logger")]
            //log::debug!(target: module_path!(), "data_logger: Attempting to lock mutex_heating_status");
            *heater_status = match data_logger_mutexes.mutex_heating_status.lock() {
                Ok(heating_status) => {
                    self.lock_error_output_description_heating_controller_state = self
                        .output_description(
                            heating_status.description(),
                            &self.config.output_file_name_heating_controller_state,
                            self.lock_error_output_description_heating_controller_state,
                        );
                    self.lock_error_output_state_heating_is_on = self.output_state(
                        heating_status.actuator_switched_on,
                        "ON",
                        "OFF",
                        &self.config.output_file_name_heating_is_on,
                        self.lock_error_output_state_heating_is_on,
                    );
                    self.lock_error_output_state_heating_actuator_ineffective = self.output_state(
                        heating_status.actuator_ineffective,
                        "TRUE",
                        "FALSE",
                        &self.config.output_file_name_heating_actuator_ineffective,
                        self.lock_error_output_state_heating_actuator_ineffective,
                    );
                    self.lock_error_output_state_heating_actuator_disabled = self.output_state(
                        heating_status.actuator_disabled,
                        "TRUE",
                        "FALSE",
                        &self.config.output_file_name_heating_actuator_disabled,
                        self.lock_error_output_state_heating_actuator_disabled,
                    );
                    heating_status.actuator_switched_on
                }
                Err(_) => {
                    if !self.lock_error_output_state_heating_status {
                        error!(target: module_path!(), "error locking mutex for heating status");
                        self.lock_error_output_state_heating_status = true;
                    }
                    false
                }
            };
            //#[cfg(feature = "debug_data_logger")]
            //debug!(target: module_path!(), "Releasing lock on heating_status");
        }

        {
            // internal scope to limit lifetime of mutex lock
            //#[cfg(feature = "debug_data_logger")]
            //log::debug!(target: module_path!(), "data_logger: Attempting to lock mutex_ventilation_status");
            *ventilation_status = match data_logger_mutexes.mutex_ventilation_status.lock() {
                Ok(ventilation_status) => {
                    self.lock_error_output_description_ventilation_controller_state = self
                        .output_description(
                            ventilation_status.description(),
                            &self.config.output_file_name_ventilation_controller_state,
                            self.lock_error_output_description_ventilation_controller_state,
                        );
                    self.lock_error_output_state_ventilation_is_on = self.output_state(
                        ventilation_status.actuator_switched_on,
                        "ON",
                        "OFF",
                        &self.config.output_file_name_ventilation_is_on,
                        self.lock_error_output_state_ventilation_is_on,
                    );
                    self.lock_error_output_state_ventilation_actuator_ineffective = self
                        .output_state(
                            ventilation_status.actuator_ineffective,
                            "TRUE",
                            "FALSE",
                            &self
                                .config
                                .output_file_name_ventilation_actuator_ineffective,
                            self.lock_error_output_state_ventilation_actuator_ineffective,
                        );
                    self.lock_error_output_state_ventilation_actuator_disabled = self.output_state(
                        ventilation_status.actuator_disabled,
                        "TRUE",
                        "FALSE",
                        &self.config.output_file_name_ventilation_actuator_disabled,
                        self.lock_error_output_state_ventilation_actuator_disabled,
                    );
                    ventilation_status.actuator_switched_on
                }
                Err(_) => {
                    if !self.lock_error_output_state_ventilation_status {
                        self.lock_error_output_state_ventilation_status = true;
                        error!(target: module_path!(), "error locking mutex for ventilation status");
                    }
                    false
                }
            };
            //#[cfg(feature = "debug_data_logger")]
            //debug!(target: module_path!(), "Releasing lock on ventilation_status");
        }
    }

    /// Retrieves the current temperature gradient from the shared mutex and outputs it to a file.
    ///
    /// This private helper function locks the `mutex_temperature_gradient` to get the latest
    /// calculated gradient value. It then calls `output_signal` to write this value to the
    /// configured output file on disk.
    ///
    /// If the mutex lock fails or the file write fails, an error is logged, and an internal
    /// flag is set to suppress further error messages.
    ///
    /// # Arguments
    /// * `mutex_temperature_gradient` - An `Arc<Mutex<f64>>` holding the latest temperature gradient value.
    fn update_temperature_gradient(&mut self, mutex_temperature_gradient: &Arc<Mutex<f64>>) {
        if !self.lock_error_output_signal_temperature_gradient {
            #[cfg(feature = "debug_data_logger")]
            log::debug!(target: module_path!(), "data_logger: Attempting to lock mutex_temperature_gradient");
            match mutex_temperature_gradient.lock() {
                Ok(temperature_gradient) => {
                    self.lock_error_output_signal_temperature_gradient = self.output_signal(
                        *temperature_gradient as f32,
                        AquariumSignal::TemperatureGradient,
                        &self.config.output_file_name_temperature_gradient,
                        false,
                    );
                }
                Err(e) => {
                    if !self.lock_error_output_signal_temperature_gradient {
                        self.lock_error_output_signal_temperature_gradient = true;
                        error!(target: module_path!(), "error locking mutex for temperature gradient: {e:?}");
                    }
                }
            }
            #[cfg(feature = "debug_data_logger")]
            log::debug!(target: module_path!(), "data_logger: Releasing lock on mutex_temperature_gradient");
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
            #[cfg(feature = "debug_data_logger")]
            log::debug!(target: module_path!(), "data_logger: Attempting to lock mutex_sensor_manager_signals");
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
            //#[cfg(feature = "debug_data_logger")]
            //debug!(target: module_path!(), "output sequence starting");
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
            //#[cfg(feature = "debug_data_logger")]
            //debug!(target: module_path!(), "output sequence finished");
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
            //#[cfg(feature = "debug_data_logger")]
            //log::debug!(target: module_path!(), "data_logger: Attempting to lock mutex_tank_level_switch_signals");
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
            //#[cfg(feature = "debug_data_logger")]
            //log::debug!(target: module_path!(), "data_logger: Releasing lock on mutex_tank_level_switch_signals");
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
    /// * `filter_water_temperature` - filter configuration for water temperature.
    /// * `filter_pH` - filter configuration for pH.
    /// * `filter_conductivity` - filter configuration for conductivity.
    fn calculate_filtered_signals(
        &mut self,
        water_temperature: Option<f32>,
        ph: Option<f32>,
        conductivity: Option<f32>,
        filter_water_temperature: &mut IIRFilter,
        filter_pH: &mut IIRFilter,
        filter_conductivity: &mut IIRFilter,
    ) -> (Option<f32>, Option<f32>, Option<f32>) {
        #[cfg(feature = "debug_data_logger")]
        log::debug!(target: module_path!(), "calculating filtered_signals");
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
    async fn confirm_quit_and_wait_for_termination_command(
        &mut self,
        data_logger_channels: &mut DataLoggerChannels,
    ) {
        #[cfg(feature = "debug_data_logger")]
        log::debug!(target: module_path!(), "waiting for final termination command");
        let sleep_duration_hundred_millis = Duration::from_millis(100);

        data_logger_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads.
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination_async(
            &mut data_logger_channels.rx_data_logger_from_signal_handler,
            sleep_duration_hundred_millis,
            module_path!(),
        )
        .await;
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
    pub async fn execute<D: DataInjectionTrait + Send + 'static>(
        &mut self,
        data_injection: D,
        mut sql_interface_data: SqlInterfaceData,
        data_logger_channels: &mut DataLoggerChannels,
        data_logger_mutexes: DataLoggerMutexes,
    ) {
        #[cfg(all(not(feature = "test-utils"), not(test), target_os = "linux"))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let cycle_time_duration =
            Duration::from_millis(self.config.logging_interval * MILLIS_PER_SEC);
        let sleep_duration_hundred_millis = Duration::from_millis(100);
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
        let mut sql_date_request_failed: bool = false;
        let mut sql_data_injection_failed: bool = false;
        let mut conn_for_timestamp_opt = Some(sql_interface_data.conn_for_timestamp);
        let mut conn_for_injection_opt = Some(sql_interface_data.conn);
        let mut data_injection_opt = Some(data_injection);

        // update signals by accessing mutex for Atlas Scientific signals
        self.update_sensor_manager_signals(
            data_logger_mutexes.mutex_sensor_manager_signals.clone(),
            &mut water_temperature,
            &mut ph,
            &mut conductivity,
            &mut ambient_temperature,
            &mut ambient_humidity,
        );

        self.update_temperature_gradient(&data_logger_mutexes.mutex_temperature_gradient);

        let mut filter_water_temperature = IIRFilter::new(
            &self.config.filter_coefficient_water_temperature,
            water_temperature,
            #[cfg(all(not(test), not(feature = "test-utils")))]
            // only required in real-life execution
            "water temperature",
        );
        let mut filter_pH = IIRFilter::new(
            &self.config.filter_coefficient_pH,
            ph,
            #[cfg(all(not(test), not(feature = "test-utils")))]
            // only required in real-life execution
            "pH",
        );

        let mut filter_conductivity = IIRFilter::new(
            &self.config.filter_coefficient_conductivity,
            conductivity,
            #[cfg(all(not(test), not(feature = "test-utils")))]
            // only required in real-life execution
            "conductivity",
        );
        #[cfg(feature = "debug_data_logger")]
        debug!(target: module_path!(), "initialized IIR filters");

        // wait for a dedicated period of time before starting the main loop
        let initial_sleep_time_start = Instant::now();
        while Instant::now()
            .duration_since(initial_sleep_time_start)
            .as_millis()
            < self.config.initial_sleep_time_millis.into()
        {
            tokio::time::sleep(sleep_duration_hundred_millis).await;
            #[cfg(feature = "debug_data_logger")]
            debug!(target: module_path!(), "checking external request");
            (quit_command_received, _, _) = self.process_external_request(
                &mut data_logger_channels.rx_data_logger_from_signal_handler,
                None,
            );
            #[cfg(feature = "debug_data_logger")]
            debug!(target: module_path!(), "checked external request");
            if quit_command_received {
                #[cfg(feature = "debug_data_logger")]
                debug!(target: module_path!(), "confirming quit and waiting for termination command");
                self.confirm_quit_and_wait_for_termination_command(data_logger_channels)
                    .await;
                return;
            }
        }
        #[cfg(feature = "debug_data_logger")]
        debug!(target: module_path!(), "initial waiting time passed");

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

                self.update_temperature_gradient(&data_logger_mutexes.mutex_temperature_gradient);

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

                // Retrieve the current timestamp, either from the SQL database or generated
                // locally by the application based on the `use_sql_timestamp` configuration
                // setting. If `use_sql_timestamp` is `true`, it attempts to fetch the timestamp
                // from the connected SQL database. If that fails, it falls back to using a locally
                // generated timestamp and logs an error. If `use_sql_timestamp` is `false`,
                // it always uses a local timestamp.
                let use_sql_timestamp = self.config.use_sql_timestamp & !sql_date_request_failed;
                let current_timestamp = if use_sql_timestamp {
                    if let Some(mut conn_for_timestamp) = conn_for_timestamp_opt.take() {
                        // database request take significant amount of time, so they must
                        // run on a dedicated thread
                        #[cfg(feature = "debug_data_logger")]
                        log::debug!(target: module_path!(), "data_logger: Spawning database task to get timestamp");
                        let timestamp_task_result = tokio::task::spawn_blocking(move || {
                            let res = SqlInterface::get_current_timestamp(&mut conn_for_timestamp);
                            (res, conn_for_timestamp)
                        })
                        .await;
                        #[cfg(feature = "debug_data_logger")]
                        log::debug!(target: module_path!(), "data_logger: Database task to get timestamp finished");

                        match timestamp_task_result {
                            Ok((res, conn_back)) => {
                                // Success: Put the connection back into the struct
                                sql_interface_data.conn_for_timestamp = conn_back;
                                conn_for_timestamp_opt =
                                    Some(sql_interface_data.conn_for_timestamp);
                                match res {
                                    Ok(c) => {
                                        self.lock_error_get_timestamp = false;
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
                                }
                            }
                            Err(e) => {
                                // The task (likely panicked). Use fallback strategy
                                // and prevent using database in next loop.
                                sql_date_request_failed = true;
                                if !self.lock_error_task_failure {
                                    self.lock_error_task_failure = true;
                                    error!(
                                        target: module_path!(),
                                        "could not execute task for retrieving timestamp from database: ({e:?}) - using application-generated timestamp."
                                    )
                                }
                                Local::now().naive_local()
                            }
                        }
                    } else {
                        // Defensive fallback just in case use_sql_timestamp was true but the Option was None
                        sql_date_request_failed = true;
                        Local::now().naive_local()
                    }
                } else {
                    Local::now().naive_local()
                };

                self.lock_error_output_timestamp = self.output_timestamp(
                    current_timestamp,
                    &self.config.output_file_name_timestamp,
                    self.lock_error_output_timestamp,
                );

                // all data received, let's construct a data frame
                let data_frame = RecorderDataFrame {
                    timestamp: current_timestamp,
                    water_temperature,
                    water_temperature_filtered,
                    ph,
                    ph_filtered: pH_filtered,
                    conductivity,
                    conductivity_filtered,
                    refill_in_progress: Some(refill_in_progress_for_database),
                    tank_level_switch_position,
                    tank_level_switch_position_stabilized,
                    tank_level_switch_invalid,
                    surface_ventilation_status: Some(ventilation_is_on),
                    ambient_temperature,
                    ambient_humidity,
                    heater_status: Some(heater_is_on),
                };

                if !sql_data_injection_failed {
                    if let Some(conn_for_injection) = conn_for_injection_opt.take() {
                        if let Some(mut data_injection) = data_injection_opt.take() {
                            #[cfg(feature = "debug_data_logger")]
                            log::debug!(target: module_path!(), "data_logger: Spawning database task to inject data frame");
                            let injection_task_result = tokio::task::spawn_blocking(move || {
                                let conn_for_injection_back =
                                    data_injection.inject_data(data_frame, conn_for_injection);
                                (data_injection, conn_for_injection_back)
                            })
                            .await;
                            #[cfg(feature = "debug_data_logger")]
                            log::debug!(target: module_path!(), "data_logger: Database task to inject data frame finished");
                            match injection_task_result {
                                Ok((data_injection_back, conn_for_injection_back)) => {
                                    // get back ownership
                                    conn_for_injection_opt = Some(conn_for_injection_back);
                                    data_injection_opt = Some(data_injection_back);
                                }
                                Err(injection_task_error) => {
                                    error!(
                                        target: module_path!(),
                                        "task for data injection failed: {injection_task_error:?}"
                                    );
                                    // block any further execution of injection
                                    sql_data_injection_failed = true;
                                }
                            }
                        }
                    }
                }
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
            #[cfg(feature = "debug_data_logger")]
            log::debug!(target: module_path!(), "data_logger: Entering idle sleep loop for {} ms", remaining_sleep_time_millis);
            i = 0;
            while i < remaining_sleep_time_millis || cycle_time_exceeded {
                i += 100;
                tokio::time::sleep(sleep_duration_hundred_millis).await;
                (quit_command_received, _, _) = self.process_external_request(
                    &mut data_logger_channels.rx_data_logger_from_signal_handler,
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

            start_time = Instant::now(); // start time for the next cycle
        }

        #[cfg(feature = "debug_data_logger")]
        log::debug!(target: module_path!(), "data_logger: Exited main loop, calling confirm_quit_and_wait_for_termination_command");
        self.confirm_quit_and_wait_for_termination_command(data_logger_channels)
            .await;
    }
}

#[cfg(test)]
pub mod tests {
    use crate::recorder::data_logger::DataLogger;
    use crate::recorder::data_logger_config::DataLoggerConfig;
    use crate::recorder::data_logger_error::DataLoggerError;
    use crate::utilities::iir_filter::IIRFilterConfig;

    #[test]
    /// Verifies that `DataLogger::new()` returns an `OutputFilenameEmpty` error
    /// if `output_to_disk` is true but a filename is an empty string.
    fn test_new_failure_on_empty_filename() {
        // 1. Setup
        let config = DataLoggerConfig {
            active: false,
            execute: false,
            use_sql_timestamp: false,
            logging_interval: 0,
            filter_coefficient_water_temperature: IIRFilterConfig {
                filter_coefficient: 0.0,
                fallback_initial_value: 25.0,
            },
            filter_coefficient_pH: IIRFilterConfig {
                filter_coefficient: 0.0,
                fallback_initial_value: 7.0,
            },
            filter_coefficient_conductivity: IIRFilterConfig {
                filter_coefficient: 0.0,
                fallback_initial_value: 50000.0,
            },
            output_to_disk: true,
            output_file_name_timestamp: "".to_string(),
            output_file_name_water_temperature: String::new(), // Empty filename
            output_file_name_water_temperature_filtered: "".to_string(),
            output_file_name_pH: "".to_string(),
            output_file_name_pH_filtered: "".to_string(),
            output_file_name_conductivity: "".to_string(),
            output_file_name_conductivity_filtered: "".to_string(),
            output_file_name_tank_level_switch_position: "".to_string(),
            output_file_name_tank_level_switch_invalid: "".to_string(),
            output_file_name_tank_level_switch_position_stabilized: "".to_string(),
            output_file_name_ventilation_controller_state: "".to_string(),
            output_file_name_ventilation_is_on: "".to_string(),
            output_file_name_ventilation_actuator_ineffective: "".to_string(),
            output_file_name_ventilation_actuator_disabled: "".to_string(),
            output_file_name_heating_controller_state: "".to_string(),
            output_file_name_heating_is_on: "".to_string(),
            output_file_name_heating_actuator_ineffective: "".to_string(),
            output_file_name_heating_actuator_disabled: "".to_string(),
            output_file_name_ambient_temperature: "".to_string(),
            output_file_name_humidity: "".to_string(),
            output_file_name_temperature_gradient: "".to_string(),
            output_file_name_refill_in_progress: "".to_string(),
            output_file_name_refill_controller_state: "".to_string(),
            initial_sleep_time_millis: 0,
        };

        // 2. Execute
        let result = DataLogger::new(config);

        // 3. Assert
        assert!(result.is_err());
        match result.err().unwrap() {
            DataLoggerError::OutputFilenameEmpty(_) => { /* Test passes */ }
            _ => panic!("Expected OutputFilenameEmpty error"),
        }
    }
}
