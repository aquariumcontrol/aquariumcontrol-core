/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages temperature reading from DS18B20 1-Wire digital thermometers.
//!
//! This module provides the `Ds18b20` struct, which is designed to run as a dedicated
//! thread. It interfaces with the Linux 1-Wire subsystem via `sysfs` to periodically
//! read temperature data from one or more configured sensors.
//!
//! ## Responsibilities
//!
//! 1.  **Configuration Validation**: The `new()` constructor performs rigorous checks on the
//!     provided configuration, ensuring that all necessary paths are specified and that
//!     sensor IDs are unique and valid before the thread starts.
//! 2.  **Periodic Measurement**: The `execute()` loop periodically reads sensor data based
//!     on the `measurement_pause_duration_millis` setting in the configuration.
//! 3.  **Robust Reading**: It implements a retry mechanism with a configurable pause to
//!     handle transient CRC errors or other temporary read failures common on the
//!     1-Wire bus.
//! 4.  **State Sharing**: It updates shared `Arc<Mutex<Ds18b20Result>>` containers with
//!     the latest water and ambient temperature readings, making the data available to
//!     other parts of the application in a thread-safe manner.
//! 5.  **Graceful Shutdown**: The `execute()` loop is responsive to a `Quit` command
//!     received from the main signal handler, allowing the thread to terminate cleanly.
//! 6.  **Performance Monitoring**: It includes logic to detect and log warnings if
//!     access to the shared mutexes is blocked for longer than a predefined threshold.

#[cfg(all(not(feature = "test-utils"), not(test), target_os = "linux"))]
// only required in real-life execution
use nix::unistd::gettid;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use std::fs;
use std::path::PathBuf;
use std::thread::sleep;

use crate::sensors::ds18b20_channels::Ds18b20Channels;
use crate::sensors::ds18b20_config::Ds18b20Config;
use crate::sensors::ds18b20_error::Ds18b20Error;
use crate::utilities::check_mutex_access_duration::CheckMutexAccessDurationTrait;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
#[cfg(feature = "debug_ds18b20")]
use log::debug;
#[cfg(all(not(feature = "test-utils"), not(test), target_os = "linux"))]
// only required in real-life execution
use log::info;

mod ds18b20_constants {
    /// minimum viable temperature used for health check of sensor signal
    pub const MIN_TEMPERATURE: f32 = 0.0;

    /// maximum viable temperature used for health check of sensor signal
    pub const MAX_TEMPERATURE: f32 = 100.0;

    /// allow max. 10 milliseconds for mutex to be blocked by any other thread
    pub const MAX_MUTEX_ACCESS_DURATION_MILLIS: u64 = 10;
}

#[derive(Clone)]
pub struct Ds18b20ResultData {
    pub value: f32,

    #[allow(unused)]
    invalid: bool,

    #[allow(unused)]
    pub measurement_instant: Instant,
}

impl Ds18b20ResultData {
    /// Creates a new instance.
    ///
    /// # Arguments
    /// * `value` - The value to set.
    pub fn new(value: f32) -> Self {
        Ds18b20ResultData {
            value,
            invalid: false,
            measurement_instant: Instant::now(),
        }
    }

    /// Creates a default instance of this type.
    pub fn new_for_test() -> Self {
        Ds18b20ResultData {
            value: 0.0,
            invalid: false,
            measurement_instant: Instant::now(),
        }
    }
}

pub type Ds18b20Result = Result<Ds18b20ResultData, Ds18b20Error>;

/// Represents a Ds18b20 sensor module responsible for reading temperature signals.
///
/// This struct encapsulates the configuration, interface details, and internal state
/// required to communicate with the sensor. It handles communication with the operating system.
/// Retries, and perform post-processing (checksum validation,
/// signal calculation) before providing the final temperature and humidity values.
///
/// Thread Communication:
/// This module is designed to run in its own dedicated thread, periodically reading sensor data
/// and making the latest measurements available via a shared `Arc<Mutex<Ds18b20Result>>`.
/// Other modules (like `SensorManager` or `DataLogger`) can then read from this mutex to get the
/// current temperature and humidity. It also responds to `Quit` commands for graceful shutdown.
///
/// Platform-Specific Behavior:
/// The sensor reading logic (`read` function) will only work on Linux systems but compile for
/// any other system as well.
///
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     ds18b20[Ds18b20] -.-> sensor_manager[SensorManager]
///     ds18b20[Ds18b20] -.-> data_logger[DataLogger]
///     signal_handler[SignalHandler] --> ds18b20
/// ```
pub struct Ds18b20 {
    #[allow(unused)]
    /// configuration for communication with Ds18b20 sensor
    config: Ds18b20Config,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex
    pub lock_warn_max_mutex_access_duration: bool,

    #[cfg(any(test, feature = "test-utils", feature = "test-utils"))]
    // for testing purposes: record when mutex access time is exceeded without resetting it
    pub mutex_access_duration_exceeded: bool,

    /// maximum allowed duration of access to mutex
    pub max_mutex_access_duration: Duration,
}

impl ProcessExternalRequestTrait for Ds18b20 {}

impl Ds18b20 {
    /// Creates a new `Ds18b20` instance, validating the provided configuration.
    ///
    /// This constructor initializes the DS18B20 sensor module, preparing it to read
    /// temperatures from 1-Wire devices via the Linux sysfs interface. It performs
    /// several critical checks on the configuration to ensure that the module can
    /// operate correctly.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the DS18B20 sensor, including driver paths,
    ///   sensor IDs, and retry settings.
    ///
    /// # Returns
    /// A `Result` containing a new `Ds18b20` struct on success.
    ///
    /// # Errors
    /// Returns a `Ds18b20Error` variant if the configuration is invalid, preventing
    /// the module from being created. This allows the application to handle setup
    /// failures gracefully. Specific errors include:
    /// - `Ds18b20Error::SystemDriverBasePathEmpty`: If the base path for the 1-Wire
    ///   driver in the configuration is empty.
    /// - `Ds18b20Error::SystemDriverSensorPathEmpty`: If the sensor path prefix is
    ///   not specified.
    /// - `Ds18b20Error::SystemDriverFileNameEmpty`: If the filename for the sensor
    ///   data (e.g., `w1_slave`) is not provided.
    /// - `Ds18b20Error::NoSensorIdProvided`: If the module is configured to be `active`
    ///   but both the water and ambient temperature sensor IDs are empty.
    /// - `Ds18b20Error::SensorIdsIdentical`: If the module is `active` and the same
    ///   ID is assigned to both the water and ambient temperature sensors.
    pub fn new(config: Ds18b20Config) -> Result<Ds18b20, Ds18b20Error> {
        if config.system_driver_base_path.is_empty() {
            return Err(Ds18b20Error::SystemDriverBasePathEmpty(
                module_path!().to_string(),
            ));
        }
        if config.system_driver_sensor_path_prefix.is_empty() {
            return Err(Ds18b20Error::SystemDriverSensorPathEmpty(
                module_path!().to_string(),
            ));
        }
        if config.system_driver_file_name.is_empty() {
            return Err(Ds18b20Error::SystemDriverFileNameEmpty(
                module_path!().to_string(),
            ));
        }
        if config.active
            && config.water_temperature_sensor_id.is_empty()
            && config.ambient_temperature_sensor_id.is_empty()
        {
            return Err(Ds18b20Error::NoSensorIdProvided(module_path!().to_string()));
        }
        if config.active
            && config.water_temperature_sensor_id == config.ambient_temperature_sensor_id
        {
            return Err(Ds18b20Error::SensorIdsIdentical(module_path!().to_string()));
        }
        Ok(Ds18b20 {
            config,
            lock_warn_max_mutex_access_duration: false,
            #[cfg(any(test, feature = "test-utils"))]
            mutex_access_duration_exceeded: false,
            max_mutex_access_duration: Duration::from_millis(
                ds18b20_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
            ),
        })
    }

    #[allow(unused)]
    /// Reads temperature from a DS18B20 1-Wire sensor via the Linux sysfs interface.
    ///
    /// This function attempts to read temperature data from a specific DS18B20 sensor,
    /// identified by its `sensor_id`. It constructs the file path based on the configured
    /// system driver paths. The function includes a retry mechanism for robust reading,
    /// as 1-Wire communication can sometimes be flaky.
    ///
    /// # Arguments
    /// * `sensor_id` - A string slice containing the unique 64-bit ID of the DS18B20 sensor
    ///   to read (e.g., "28-00000abcde12").
    ///
    /// # Returns
    /// A `Ds18b20Result` containing `Ds18b20ResultData` on success, which includes the
    /// temperature in degrees Celsius.
    ///
    /// # Errors
    /// Returns a `Ds18b20Error` variant if the reading fails, even after retries.
    /// Specific errors include:
    /// - `Ds18b20Error::SensorIdNotFound`: If the directory for the given `sensor_id` does not exist.
    /// - `Ds18b20Error::ReadToStringFailure`: If the underlying file read from the sysfs fails.
    /// - `Ds18b20Error::InsufficientLinesInFile`: If the sensor file does not contain at least two lines.
    /// - `Ds18b20Error::Line0NotEndingWithYes`: If the first line of the sensor file does not end with "YES",
    ///   indicating a CRC error or invalid reading.
    /// - `Ds18b20Error::TemperatureEntryNotFound`: If the second line does not contain the "t=" marker.
    /// - `Ds18b20Error::TemperatureParseError`: If the temperature value after "t=" cannot be parsed into a number.
    /// - `Ds18b20Error::TemperatureOutOfRange`: If the parsed temperature is outside the plausible range defined
    ///   by `MIN_TEMPERATURE` and `MAX_TEMPERATURE`.
    ///
    /// [impl->swreq~sensors_read_data~1]
    pub fn read(&self, sensor_id: &str) -> Ds18b20Result {
        let base_dir = PathBuf::from(self.config.system_driver_base_path.clone());
        let device_folder =
            base_dir.join(self.config.system_driver_sensor_path_prefix.to_string() + sensor_id);

        if !device_folder.exists() {
            return Err(Ds18b20Error::SensorIdNotFound(
                module_path!().to_string(),
                sensor_id.to_string(),
            ));
        }

        let device_file = device_folder.join(self.config.system_driver_file_name.clone());
        let mut last_error = Ds18b20Error::UnidentifiedError(module_path!().to_string());

        // Use a `for` loop for a cleaner retry mechanism.
        for _ in 0..=self.config.max_retries {
            match fs::read_to_string(&device_file) {
                Ok(contents) => {
                    // Call the new helper function to handle parsing.
                    match self.parse_contents(&contents) {
                        Ok(celsius) => return Ok(Ds18b20ResultData::new(celsius)),
                        Err(e) => last_error = e, // Store the error and retry.
                    }
                }
                Err(_) => {
                    last_error = Ds18b20Error::ReadToStringFailure(module_path!().to_string());
                }
            }
            sleep(Duration::from_millis(
                self.config.retry_pause_duration_millis,
            ));
        }

        // If all retries fail, return the last error encountered.
        Err(last_error)
    }

    /// Parses the string contents of a DS18B20 sysfs file.
    /// This private helper contains the core parsing logic.
    fn parse_contents(&self, contents: &str) -> Result<f32, Ds18b20Error> {
        let lines: Vec<&str> = contents.lines().collect();

        if lines.len() < 2 {
            return Err(Ds18b20Error::InsufficientLinesInFile(
                module_path!().to_string(),
            ));
        }

        if !lines[0].ends_with(" YES") {
            return Err(Ds18b20Error::Line0NotEndingWithYes(
                module_path!().to_string(),
            ));
        }

        let temp_line = lines[1];
        if let Some(temp_index) = temp_line.find("t=") {
            let temp_str = temp_line[temp_index + 2..].trim();
            // Use `map_err` for more concise error conversion.
            let temp_val = temp_str
                .parse::<f32>()
                .map_err(|_| Ds18b20Error::TemperatureParseError(module_path!().to_string()))?;

            let celsius = temp_val / 1000.0;
            if (ds18b20_constants::MIN_TEMPERATURE..=ds18b20_constants::MAX_TEMPERATURE)
                .contains(&celsius)
            {
                Ok(celsius)
            } else {
                Err(Ds18b20Error::TemperatureOutOfRange(
                    module_path!().to_string(),
                ))
            }
        } else {
            Err(Ds18b20Error::TemperatureEntryNotFound(
                module_path!().to_string(),
            ))
        }
    }

    /// Executes the main control loop for the DS18B20 sensor module.
    ///
    /// This function runs continuously, managing the periodic measurement of water and ambient
    /// temperatures from DS18B20 sensors via the Linux sysfs 1-Wire interface.
    /// It updates shared mutexes with the latest sensor readings, making them available to other
    /// application threads. The loop remains responsive to `Quit` commands from the
    /// signal handler for graceful shutdown.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - The receiver channel for commands (e.g., `Quit`) from the main signal handler.
    /// * `mutex_water_temperature` - An `Arc<Mutex<Ds18b20Result>>` into which the latest water temperature reading
    ///   will be written, ensuring thread-safe access for other modules.
    /// * `mutex_ambient_temperature` - An `Arc<Mutex<Ds18b20Result>>` into which the latest ambient temperature reading
    ///   will be written, ensuring thread-safe access for other modules.
    /// * `ds18b20_channels` - A mutable reference to the `Ds18b20Channels` communication interface.
    pub async fn execute(
        &mut self,
        ds18b20_channels: &mut Ds18b20Channels,
        mutex_water_temperature: Arc<Mutex<Ds18b20Result>>,
        mutex_ambient_temperature: Arc<Mutex<Ds18b20Result>>,
    ) {
        #[cfg(all(target_os = "linux", not(test), not(feature = "test-utils")))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let sleep_duration_main_cycle = Duration::from_millis(100);
        let measurement_interval =
            Duration::from_millis(self.config.measurement_pause_duration_millis);

        // Use an Instant to track time since the last measurement.
        // Start with a measurement due to ensure the first loop iteration triggers a read.
        let mut last_measurement_instant = Instant::now() - measurement_interval;

        loop {
            // Check for quit command first for faster shutdown.
            if self
                .process_external_request(
                    &mut ds18b20_channels.rx_ds18b20_from_signal_handler,
                    None,
                )
                .0
            {
                break;
            }

            if self.config.active && (last_measurement_instant.elapsed() >= measurement_interval) {
                let water_temp_result = if !self.config.water_temperature_sensor_id.is_empty() {
                    Some(tokio::task::block_in_place(|| {
                        self.read(self.config.water_temperature_sensor_id.as_str())
                    }))
                } else {
                    None
                };

                let ambient_temp_result = if !self.config.ambient_temperature_sensor_id.is_empty() {
                    Some(tokio::task::block_in_place(|| {
                        self.read(self.config.ambient_temperature_sensor_id.as_str())
                    }))
                } else {
                    None
                };

                // Now, lock the mutexes for the shortest possible time to write the results.
                let instant_before_locking_mutex = Instant::now();

                if let Some(result) = water_temp_result {
                    // The lock is held only for the duration of this assignment.
                    *mutex_water_temperature.lock().unwrap() = result;
                }

                if let Some(result) = ambient_temp_result {
                    *mutex_ambient_temperature.lock().unwrap() = result;
                }

                let instant_after_locking_mutex = Instant::now();
                last_measurement_instant = Instant::now(); // Reset the timer

                // check if access to mutex took too long
                self.check_mutex_access_duration(
                    None,
                    instant_after_locking_mutex,
                    instant_before_locking_mutex,
                );
            }

            // sleep for a defined period
            tokio::time::sleep(sleep_duration_main_cycle).await;
        }
    }
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub mod tests { // unit tests shall not appear in code coverage measurement
    use crate::sensors::ds18b20::{Ds18b20, Ds18b20Config, Ds18b20Error};

    #[test]
    // Verifies that new fails with identical sensor ids.
    fn test_new_fails_with_identical_sensor_ids() {
        // Arrange: Create a configuration where both water and ambient sensors
        // are assigned the exact same ID.
        let identical_id = "28-00000abcdef12".to_string();
        let config = Ds18b20Config {
            active: true,
            water_temperature_sensor_id: identical_id.clone(),
            ambient_temperature_sensor_id: identical_id,
            // Provide valid paths to pass other validation checks.
            system_driver_base_path: "/sys/bus/w1/devices".to_string(),
            system_driver_sensor_path_prefix: "28-".to_string(),
            system_driver_file_name: "w1_slave".to_string(),
            max_retries: 3,
            retry_pause_duration_millis: 100,
            measurement_pause_duration_millis: 1000,
            execute: true,
        };

        // Act: Attempt to create a new Ds18b20 instance with the invalid config.
        let result = Ds18b20::new(config);

        // Assert: Verify that the result is an error and that the error variant
        // is specifically `SensorIdsIdentical`.
        assert!(matches!(result, Err(Ds18b20Error::SensorIdsIdentical(_))));
    }
}
