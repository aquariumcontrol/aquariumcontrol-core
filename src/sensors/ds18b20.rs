/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(all(not(test), target_os = "linux"))]
use nix::unistd::gettid;
use spin_sleep::SpinSleeper;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use std::fs;
use std::path::PathBuf;
use std::thread::sleep;

use crate::sensors::ds18b20_channels::Ds18b20Channels;
use crate::sensors::ds18b20_config::Ds18b20Config;
use crate::sensors::ds18b20_error::Ds18b20Error;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
#[cfg(feature = "debug_ds18b20")]
use log::debug;
#[cfg(all(not(test), target_os = "linux"))]
use log::info;
#[cfg(not(test))]
use log::warn;

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
    measurement_instant: Instant,
}

impl Ds18b20ResultData {
    pub fn new(value: f32) -> Self {
        Ds18b20ResultData {
            value,
            invalid: false,
            measurement_instant: Instant::now(),
        }
    }
}
pub type Ds18b20Result = Result<Ds18b20ResultData, Ds18b20Error>;

pub struct Ds18b20 {
    #[allow(unused)]
    /// configuration for communication with Ds18b20 sensor
    config: Ds18b20Config,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex
    lock_warn_max_mutex_access_duration: bool,

    // for testing purposes: record when mutex access time is exceeded without resetting it
    #[cfg(test)]
    mutex_access_duration_exceeded: bool,
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
            #[cfg(test)]
            mutex_access_duration_exceeded: false,
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
    pub fn read(&self, sensor_id: &str) -> Ds18b20Result {
        #[cfg(test)]
        println!("{}: reading from {}", module_path!(), sensor_id);

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
    pub fn execute(
        &mut self,
        ds18b20_channels: &mut Ds18b20Channels,
        mutex_water_temperature: Arc<Mutex<Ds18b20Result>>,
        mutex_ambient_temperature: Arc<Mutex<Ds18b20Result>>,
    ) {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let sleep_duration_main_cycle = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let measurement_interval =
            Duration::from_millis(self.config.measurement_pause_duration_millis);
        let max_mutex_access_duration =
            Duration::from_millis(ds18b20_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS);

        // Use an Instant to track time since the last measurement.
        // Start with a measurement due to ensure the first loop iteration triggers a read.
        let mut last_measurement_instant = Instant::now() - measurement_interval;

        loop {
            // Check for quit command first for faster shutdown.
            if self
                .process_external_request(&ds18b20_channels.rx_ds18b20_from_signal_handler, None)
                .0
            {
                #[cfg(test)]
                println!("Quitting Ds18b20");
                break;
            }

            if self.config.active && last_measurement_instant.elapsed() >= measurement_interval {
                // --- Refactoring: Read values BEFORE locking mutexes ---
                let water_temp_result = if !self.config.water_temperature_sensor_id.is_empty() {
                    Some(self.read(self.config.water_temperature_sensor_id.as_str()))
                } else {
                    None
                };

                let ambient_temp_result = if !self.config.ambient_temperature_sensor_id.is_empty() {
                    Some(self.read(self.config.ambient_temperature_sensor_id.as_str()))
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

                // Check if access to mutex took too long
                let measured_mutex_access_duration =
                    instant_after_locking_mutex.duration_since(instant_before_locking_mutex);
                if measured_mutex_access_duration > max_mutex_access_duration {
                    if !self.lock_warn_max_mutex_access_duration {
                        self.lock_warn_max_mutex_access_duration = true;
                        #[cfg(not(test))]
                        warn!(target: module_path!(),
                            "Access to DS18B20 mutex took too long. Allowed={}ms, Measured={}ms",
                            max_mutex_access_duration.as_millis(),
                            measured_mutex_access_duration.as_millis(),
                        );

                        #[cfg(test)]
                        {
                            self.mutex_access_duration_exceeded = true;
                        }
                    }
                } else {
                    self.lock_warn_max_mutex_access_duration = false;
                }
            }

            // sleep for a defined period
            spin_sleeper.sleep(sleep_duration_main_cycle);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::sensors::ds18b20::{Ds18b20, Ds18b20Config, Ds18b20Error};
    use crate::sensors::ds18b20::{Ds18b20Result, Ds18b20ResultData};
    use crate::sensors::ds18b20_channels::Ds18b20Channels;
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::config::read_config_file;
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;
    use std::{env, thread};

    #[test]
    #[ignore]
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    // This test case reads from a real DS18B20 sensor.
    // For this test case to succeed, the DS18B20 sensor needs to be connected,
    // and the 1-wire interface needs to be enabled (see rasp-config)
    fn test_read_ds18b20() {
        let config = read_config_file("config/aquarium_control_test_generic.toml".to_string());

        let water_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20);

        match ds18b20.read(&water_temperature_sensor_id) {
            Ok(temperature) => {
                println!("Temperature: {:.2}°C ", temperature.value);
                assert!(true);
            }
            Err(e) => {
                println!("Error reading DS18B20: {}", e);
                assert!(false);
            }
        }
    }

    #[test]
    // This test case read sensor data provided by a file.
    fn test_read_ds18b20_from_file() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });

        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let water_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20).unwrap();
        match ds18b20.read(&water_temperature_sensor_id) {
            Ok(temperature) => {
                println!(
                    "{}: Temperature read from file: {:.2}°C ({} milliseconds ago)",
                    module_path!(),
                    temperature.value,
                    temperature.measurement_instant.elapsed().as_millis()
                );
                assert!(true);
            }
            Err(e) => {
                println!("Error reading DS18B20: {}", e);
                assert!(false);
            }
        }
    }

    #[test]
    // Test case checks if invalid configuration of the system driver base path is recognized during startup.
    fn test_dsb18b20_init_with_invalid_system_driver_base_path() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.ds18b20.system_driver_base_path = "".to_string();
        let result = Ds18b20::new(config.ds18b20);
        assert!(matches!(
            result,
            Err(Ds18b20Error::SystemDriverBasePathEmpty(_))
        ));
    }

    #[test]
    // Test case checks if invalid configuration of the system driver sensor path prefix is recognized during startup.
    fn test_dsb18b20_init_with_invalid_system_driver_sensor_path_prefix() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.ds18b20.system_driver_sensor_path_prefix = "".to_string();
        let result = Ds18b20::new(config.ds18b20);
        assert!(matches!(
            result,
            Err(Ds18b20Error::SystemDriverSensorPathEmpty(_))
        ));
    }

    #[test]
    // Test case checks if invalid configuration of the system driver sensor file name is recognized during startup.
    fn test_dsb18b20_init_with_invalid_system_driver_file_name() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.ds18b20.system_driver_file_name = "".to_string();
        let result = Ds18b20::new(config.ds18b20);
        assert!(matches!(
            result,
            Err(Ds18b20Error::SystemDriverFileNameEmpty(_))
        ));
    }
    #[test]
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

    #[test]
    // Test case checks if invalid configuration of the sensor ids is recognized during startup.
    fn test_dsb18b20_init_with_invalid_sensor_ids() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.ds18b20.active = true;
        config.ds18b20.water_temperature_sensor_id = "".to_string();
        config.ds18b20.ambient_temperature_sensor_id = "".to_string();
        let result = Ds18b20::new(config.ds18b20);
        assert!(matches!(result, Err(Ds18b20Error::NoSensorIdProvided(_))));
    }

    #[test]
    // Test case checks if function returns error for an incorrect sensor id
    fn test_dsb18b20_read_from_invalid_sensor_id() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });

        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let invalid_temperature_sensor_id = "33";

        let ds18b20 = Ds18b20::new(config.ds18b20).unwrap();

        assert_eq!(
            matches!(
                ds18b20.read(&invalid_temperature_sensor_id),
                Err(Ds18b20Error::SensorIdNotFound(
                    _,
                    _invalid_temperature_sensor_id
                ))
            ),
            true
        );
    }

    #[test]
    // Test case checks if function returns error when not being able to read from the device file
    fn test_dsb18b20_read_from_file_failed() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

        config.ds18b20.system_driver_file_name = "xxx".to_string(); // invalidate file name
        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });
        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let valid_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20).unwrap();

        assert_eq!(
            matches!(
                ds18b20.read(&valid_temperature_sensor_id),
                Err(Ds18b20Error::ReadToStringFailure(_))
            ),
            true
        );
    }

    #[test]
    // Test case checks if function returns error when the device file does not provide enough lines
    fn test_dsb18b20_file_with_one_line_only() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

        config.ds18b20.system_driver_file_name += "_one_line"; // file with only one line content
        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });
        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let valid_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20).unwrap();

        assert_eq!(
            matches!(
                ds18b20.read(&valid_temperature_sensor_id),
                Err(Ds18b20Error::InsufficientLinesInFile(_))
            ),
            true
        );
    }

    #[test]
    // Test case checks if function returns error when the first line of device file content does not match "YES"
    fn test_dsb18b20_file_with_line_not_ending_with_yes() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

        config.ds18b20.system_driver_file_name += "_no_yes"; // file with only one line content
        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });
        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let valid_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20).unwrap();

        assert_eq!(
            matches!(
                ds18b20.read(&valid_temperature_sensor_id),
                Err(Ds18b20Error::Line0NotEndingWithYes(_))
            ),
            true
        );
    }

    #[test]
    // Test case checks if function returns error when file content does not contain temperature
    fn test_dsb18b20_file_without_temperature() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

        config.ds18b20.system_driver_file_name += "_no_temperature"; // file with only one line content
        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });
        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let valid_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20).unwrap();

        assert_eq!(
            matches!(
                ds18b20.read(&valid_temperature_sensor_id),
                Err(Ds18b20Error::TemperatureEntryNotFound(_))
            ),
            true
        );
    }

    #[test]
    // Test case checks if function returns error when file content contains unparsable data
    fn test_dsb18b20_unparsable() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

        config.ds18b20.system_driver_file_name += "_unparsable"; // file with only one line content
        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });
        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let valid_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20).unwrap();

        assert_eq!(
            matches!(
                ds18b20.read(&valid_temperature_sensor_id),
                Err(Ds18b20Error::TemperatureParseError(_))
            ),
            true
        );
    }

    #[test]
    // Test case checks if function returns error when the temperature is too hot
    fn test_dsb18b20_temperature_too_hot() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

        config.ds18b20.system_driver_file_name += "_too_hot"; // file with only one line content
        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });
        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let valid_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20).unwrap();

        assert_eq!(
            matches!(
                ds18b20.read(&valid_temperature_sensor_id),
                Err(Ds18b20Error::TemperatureOutOfRange(_))
            ),
            true
        );
    }

    #[test]
    // Test case checks if function returns error when the temperature is too cold
    fn test_dsb18b20_temperature_too_cold() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

        config.ds18b20.system_driver_file_name += "_too_cold"; // file with only one line content
        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });
        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let valid_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20).unwrap();

        assert_eq!(
            matches!(
                ds18b20.read(&valid_temperature_sensor_id),
                Err(Ds18b20Error::TemperatureOutOfRange(_))
            ),
            true
        );
    }

    #[test]
    fn test_dsb18b20_channel_mutexes_without_locking_mutexes() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });

        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";
        config.ds18b20.ambient_temperature_sensor_id = "0114540015ab".to_string();

        // Mutexes for DS18B20 signals
        let mutex_ds18b20_water_temperature: Arc<Mutex<Ds18b20Result>> = Arc::new(Mutex::new(Ok(
            Ds18b20ResultData::new(config.sensor_manager.replacement_value_water_temperature),
        )));
        let mutex_ds18b20_ambient_temperature: Arc<Mutex<Ds18b20Result>> =
            Arc::new(Mutex::new(Ok(Ds18b20ResultData::new(
                config.sensor_manager.replacement_value_ambient_temperature,
            ))));
        //let mutex_ds18b20_water_temperature_clone_for_test_environment =
        //    mutex_ds18b20_water_temperature.clone();
        //let mutex_ds18b20_ambient_temperature_clone_for_test_environment =
        //    mutex_ds18b20_ambient_temperature.clone();
        let mutex_ds18b20_water_temperature_clone_for_asserts =
            mutex_ds18b20_water_temperature.clone();
        let mutex_ds18b20_ambient_temperature_clone_for_asserts =
            mutex_ds18b20_ambient_temperature.clone();

        let mut ds18b20 = Ds18b20::new(config.ds18b20).unwrap();

        // *** signal handler => ds18b20 ***
        let (tx_signal_handler_to_ds18b20, rx_ds18b20_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let mut ds18b20_channels = Ds18b20Channels {
            rx_ds18b20_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_ds18b20_from_signal_handler: 0,
        };

        // thread for test environment (incl. signal handler)
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                // let test run a bit longer than the measurement interval
                let sleep_time = Duration::from_millis(1000);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_time);
                tx_signal_handler_to_ds18b20
                    .send(InternalCommand::Quit)
                    .unwrap();
            })
            .unwrap();

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                ds18b20.execute(
                    &mut ds18b20_channels,
                    mutex_ds18b20_water_temperature,
                    mutex_ds18b20_ambient_temperature,
                );
                assert_eq!(ds18b20.mutex_access_duration_exceeded, false);
            })
            .unwrap();

        join_handle_test_object
            .join()
            .expect("Test object did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment did not finish.");

        match mutex_ds18b20_water_temperature_clone_for_asserts.lock() {
            Ok(result) => {
                let result_data = result.clone().unwrap();
                assert_eq!(result_data.value, 24.437);
            }
            Err(e) => {
                panic!("mutex_ds18b20_water_temperature lock poisoned: {:?}", e);
            }
        };
        match mutex_ds18b20_ambient_temperature_clone_for_asserts.lock() {
            Ok(result) => {
                let result_data = result.clone().unwrap();
                assert_eq!(result_data.value, 24.637);
            }
            Err(e) => {
                panic!("mutex_ds18b20_ambient_temperature lock poisoned: {:?}", e);
            }
        };
    }

    #[test]
    fn test_dsb18b20_channel_mutexes_with_locking_mutexes() {
        let mut config =
            read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });

        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";
        config.ds18b20.ambient_temperature_sensor_id = "0114540015ab".to_string();
        config.ds18b20.measurement_pause_duration_millis = 500;

        // Mutexes for DS18B20 signals
        let mutex_ds18b20_water_temperature: Arc<Mutex<Ds18b20Result>> = Arc::new(Mutex::new(Ok(
            Ds18b20ResultData::new(config.sensor_manager.replacement_value_water_temperature),
        )));
        let mutex_ds18b20_ambient_temperature: Arc<Mutex<Ds18b20Result>> =
            Arc::new(Mutex::new(Ok(Ds18b20ResultData::new(
                config.sensor_manager.replacement_value_ambient_temperature,
            ))));
        let mutex_ds18b20_water_temperature_clone_for_test_environment =
            mutex_ds18b20_water_temperature.clone();
        let mutex_ds18b20_water_temperature_clone_for_asserts =
            mutex_ds18b20_water_temperature.clone();
        let mutex_ds18b20_ambient_temperature_clone_for_asserts =
            mutex_ds18b20_ambient_temperature.clone();

        let mut ds18b20 = Ds18b20::new(config.ds18b20).unwrap();

        // *** signal handler => ds18b20 ***
        let (tx_signal_handler_to_ds18b20, rx_ds18b20_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let mut ds18b20_channels = Ds18b20Channels {
            rx_ds18b20_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_ds18b20_from_signal_handler: 0,
        };

        // thread for test environment (incl. signal handler)
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                // let test run a bit longer than the measurement interval
                let sleep_time = Duration::from_millis(2000);
                let spin_sleeper = SpinSleeper::default();

                {
                    // lock the mutexes and wait for triggering the detection
                    match mutex_ds18b20_water_temperature_clone_for_test_environment.lock() {
                        Ok(_) => {
                            spin_sleeper.sleep(sleep_time);
                        }
                        Err(e) => {
                            panic!("mutex_ds18b20_water_temperature lock poisoned: {:?}", e);
                        }
                    };
                }

                tx_signal_handler_to_ds18b20
                    .send(InternalCommand::Quit)
                    .unwrap();
                spin_sleeper.sleep(sleep_time);
            })
            .unwrap();

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                ds18b20.execute(
                    &mut ds18b20_channels,
                    mutex_ds18b20_water_temperature,
                    mutex_ds18b20_ambient_temperature,
                );
                assert_eq!(ds18b20.mutex_access_duration_exceeded, true);
            })
            .unwrap();

        join_handle_test_object
            .join()
            .expect("Test object did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment did not finish.");

        match mutex_ds18b20_water_temperature_clone_for_asserts.lock() {
            Ok(result) => {
                let result_data = result.clone().unwrap();
                assert_eq!(result_data.value, 24.437);
            }
            Err(e) => {
                panic!("mutex_ds18b20_water_temperature lock poisoned: {:?}", e);
            }
        };
        match mutex_ds18b20_ambient_temperature_clone_for_asserts.lock() {
            Ok(result) => {
                let result_data = result.clone().unwrap();
                assert_eq!(result_data.value, 24.637);
            }
            Err(e) => {
                panic!("mutex_ds18b20_ambient_temperature lock poisoned: {:?}", e);
            }
        };
    }
}
