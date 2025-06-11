/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::utilities::channel_content::InternalCommand;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use std::path::PathBuf;
use std::thread::sleep;
use std::{fmt, fs};

const LOG_TARGET_DS18B20: &str = "ds18b20";

#[cfg(feature = "debug_ds18b20")]
use log::debug;

#[cfg(all(target_os = "linux", feature = "target_hw"))]
use log::error;
use log::warn;

mod ds18b20_constants {
    pub(crate) const MIN_TEMPERATURE: f32 = 0.0; // minimum viable temperature

    pub(crate) const MAX_TEMPERATURE: f32 = 100.0; // maximum viable temperature
}
/// Holds the configuration data for DS18B20 sensor communication.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct Ds18b20Config {
    /// used primarily for testing purposes
    pub active: bool,

    /// pause duration between two measurements in milliseconds
    measurement_pause_duration_millis: u64,

    /// system driver base path
    pub system_driver_base_path: String,

    /// system driver sensor-specific path
    pub system_driver_sensor_path_prefix: String,

    /// system driver file name
    pub system_driver_file_name: String,

    #[allow(unused)]
    /// max. number of retries
    pub max_retries: u32,

    #[allow(unused)]
    /// duration between retries in milliseconds
    pub retry_pause_duration_millis: u64,

    /// unique 64-bit ID of water temperature sensor
    pub water_temperature_sensor_id: String,

    /// unique 64-bit ID of ambient temperature sensor
    pub ambient_temperature_sensor_id: String,
}

#[derive(Debug)]
pub enum Ds18b20Error {
    SensorIdNotFound(String),

    ReadToStringFailure,

    InsufficientLinesInFile,

    Line0NotEndingWithYes,

    TemperatureEntryNotFound,

    UnidentifiedError,

    TemperatureParseError,

    TemperatureOutOfRange,
}

impl fmt::Display for Ds18b20Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ds18b20Error::SensorIdNotFound(id) => write!(f, "DS18B20 sensor with ID '{}' not found.", id),
            Ds18b20Error::ReadToStringFailure => write!(f, "Failed to read content from the device file."),
            Ds18b20Error::InsufficientLinesInFile => write!(f, "Device file contains insufficient lines (expected at least 2)."),
            Ds18b20Error::Line0NotEndingWithYes => write!(f, "The first line of the device file does not end with ' YES', indicating a CRC error or incomplete reading."),
            Ds18b20Error::TemperatureEntryNotFound => write!(f, "Temperature entry (t=) not found in the second line of the device file."),
            Ds18b20Error::UnidentifiedError => write!(f, "An unidentified error occurred."),
            Ds18b20Error::TemperatureParseError => write!(f, "Failed to parse temperature value as a floating-point number."),
            Ds18b20Error::TemperatureOutOfRange => write!(f, "Read temperature value is outside the expected range."),
        }
    }
}

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
}

impl ProcessExternalRequestTrait for Ds18b20 {}

impl Ds18b20 {
    /// Provides a struct of type Ds18b20.
    ///
    /// This constructor initializes the DS18B20 sensor module, preparing it to read
    /// temperatures from 1-Wire devices via the Linux sysfs interface. It validates
    /// the provided configuration paths and sensor IDs.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the DS18B20 sensor, including driver paths,
    ///   sensor IDs, and retry settings.
    ///
    /// # Returns
    /// A new `Ds18b20` struct.
    ///
    /// # Panics
    /// This function will panic if:
    /// - `system_driver_base_path`, `system_driver_sensor_path_prefix`, or `system_driver_file_name` are empty.
    /// - `active` is true, but both `water_temperature_sensor_id` and `ambient_temperature_sensor_id` are empty.
    pub fn new(config: Ds18b20Config) -> Ds18b20 {
        if config.system_driver_base_path.is_empty() {
            panic!(
                "{}: System driver base path prefix is empty!",
                LOG_TARGET_DS18B20
            );
        }
        if config.system_driver_sensor_path_prefix.is_empty() {
            panic!(
                "{}: System driver sensor path prefix is empty!",
                LOG_TARGET_DS18B20
            );
        }
        if config.system_driver_file_name.is_empty() {
            panic!("{}: System driver file name is empty!", LOG_TARGET_DS18B20);
        }
        if config.active
            && config.water_temperature_sensor_id.is_empty()
            && config.ambient_temperature_sensor_id.is_empty()
        {
            panic!(
                "{}: No water temperature sensor ID nor ambient temperature sensor ID provided!",
                LOG_TARGET_DS18B20
            );
        }
        if config.active && config.water_temperature_sensor_id == config.ambient_temperature_sensor_id {
            warn!(
                target: LOG_TARGET_DS18B20,
                "Ambient and water temperature sensor IDs are identical."
            );
        }
        Ds18b20 { config }
    }

    #[allow(unused)]
    /// Reads temperature from a DS18B20 1-Wire sensor via the Linux sysfs interface.
    ///
    /// This function attempts to read temperature data from a specific DS18B20 sensor,
    /// identified by its `sensor_id`. It constructs the file path based on the configured
    /// system driver paths. The function includes a retry mechanism for robust reading,
    /// as 1-Wire communication can sometimes be flaky.
    ///
    /// The sensor reports temperature in milli-celsius (e.g., 24500 for 24.5°C), which
    /// this function converts to `f32` degrees Celsius. It also verifies the "YES"
    /// flag in the sysfs output to ensure a valid reading.
    ///
    /// # Arguments
    /// * `sensor_id` - A string slice containing the unique 64-bit ID of the DS18B20 sensor
    ///   to read (e.g., "28-00000abcde12").
    ///
    /// # Returns
    /// A `Ds18b20Result`:
    /// - `Ok(f32)`: The temperature in degree Celsius if successfully read and parsed.
    /// - `Err(Ds18b20Error)`: Description of the error which occurred during reading.
    pub fn read(&self, sensor_id: &str) -> Ds18b20Result {
        let base_dir = PathBuf::from(self.config.system_driver_base_path.clone());
        let device_folder =
            base_dir.join(self.config.system_driver_sensor_path_prefix.to_string() + sensor_id);

        if !device_folder.exists() {
            return Err(Ds18b20Error::SensorIdNotFound(sensor_id.to_string()));
        }

        let device_file = device_folder.join(self.config.system_driver_file_name.clone());

        let mut retries = self.config.max_retries;
        let mut last_error_recorded = Ds18b20Error::UnidentifiedError;

        while retries > 0 {
            match fs::read_to_string(&device_file) {
                Ok(contents) => {
                    let lines: Vec<&str> = contents.lines().collect();
                    if lines.len() >= 2 {
                        println!("lines[0]={}", lines[0]);
                        if lines[0].ends_with(" YES") {
                            if let Some(temp_index) = lines[1].find("t=") {
                                if let Ok(temp_str) =
                                    lines[1][temp_index + 2..].trim().parse::<f32>()
                                {
                                    let celsius = temp_str / 1000.0;
                                    if (ds18b20_constants::MIN_TEMPERATURE
                                        ..=ds18b20_constants::MAX_TEMPERATURE)
                                        .contains(&celsius)
                                    {
                                        // let fahrenheit = (celsius * 9.0 / 5.0) + 32.0;
                                        return Ok(Ds18b20ResultData::new(celsius));
                                    }
                                    else {
                                        last_error_recorded = Ds18b20Error::TemperatureOutOfRange;
                                    }
                                } else {
                                    last_error_recorded = Ds18b20Error::TemperatureParseError;
                                }
                            } else {
                                last_error_recorded = Ds18b20Error::TemperatureEntryNotFound;
                            }
                        } else {
                            last_error_recorded = Ds18b20Error::Line0NotEndingWithYes;
                        }
                    } else {
                        last_error_recorded = Ds18b20Error::InsufficientLinesInFile;
                    }
                }
                Err(e) => {
                    last_error_recorded = Ds18b20Error::ReadToStringFailure;
                }
            }

            sleep(Duration::from_millis(
                self.config.retry_pause_duration_millis,
            ));
            retries -= 1;
        }

        Err(last_error_recorded)
    }

    /// Executes the main control loop for the DS18B20 sensor module.
    ///
    /// This function runs continuously, managing the periodic measurement of water and ambient
    /// temperatures from DS18B20 sensors via the Linux sysfs 1-Wire interface.
    /// It updates shared mutexes with the latest sensor readings, making them available to other
    /// application threads. The loop maintains a measurement countdown and remains
    /// responsive to `Quit` commands from the signal handler for graceful shutdown.
    ///
    /// # Arguments
    /// * `rx_from_signal_handler` - The receiver channel for commands (e.g., `Quit`) from the main signal handler.
    /// * `mutex_water_temperature` - An `Arc<Mutex<Ds18b20Result>>` into which the latest water temperature reading
    ///   will be written, ensuring thread-safe access for other modules.
    /// * `mutex_ambient_temperature` - An `Arc<Mutex<Ds18b20Result>>` into which the latest ambient temperature reading
    ///   will be written, ensuring thread-safe access for other modules.
    pub fn execute(
        &mut self,
        rx_from_signal_handler: Receiver<InternalCommand>,
        mutex_water_temperature: Arc<Mutex<Ds18b20Result>>,
        mutex_ambient_temperature: Arc<Mutex<Ds18b20Result>>,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        // initialize countdown
        let mut measurement_countdown: i32 = 0;

        loop {
            if measurement_countdown > 0 {
                // count down to zero
                measurement_countdown = measurement_countdown.saturating_sub(100);
            } else {
                // count down has reached zero - initiate the measurement
                {
                    // life-time limitation of open mutex
                    let mut water_temperature = mutex_water_temperature.lock().unwrap();
                    let mut ambient_temperature = mutex_ambient_temperature.lock().unwrap();

                    #[cfg(all(target_os = "linux", feature = "target_hw"))]
                    {
                        if !self.config.ambient_temperature_sensor_id.is_empty() {
                            match self.read(self.config.ambient_temperature_sensor_id.as_str()) {
                                Ok(temp) => {
                                    *ambient_temperature = temp;
                                }
                                Err(e) => {
                                    error!(
                                        target: LOG_TARGET_DS18B20,
                                        "Encountered error ({e:?}) when reading from DS18B20 sensor."
                                    );
                                    *ambient_temperature =
                                        self.config.replacement_value_ambient_temperature;
                                }
                            }
                        } else {
                            *ambient_temperature = self.config.replacement_value_water_temperature;
                        }
                        if !self.config.water_temperature_sensor_id.is_empty() {
                            match self.read(self.config.water_temperature_sensor_id.as_str()) {
                                Ok(temp) => {
                                    *water_temperature = temp;
                                }
                                Err(e) => {
                                    error!(
                                        target: LOG_TARGET_DS18B20,
                                        "Encountered error ({e:?}) when reading from DS18B20 sensor."
                                    );
                                    *water_temperature =
                                        self.config.replacement_value_water_temperature;
                                }
                            }
                        } else {
                            *water_temperature = self.config.replacement_value_water_temperature;
                        }
                    }

                    // restart the countdown
                    measurement_countdown = self.config.measurement_pause_duration_millis as i32;

                    // avoid compiler warning - write default values for mutex
                    #[cfg(not(all(target_os = "linux", feature = "target_hw")))]
                    {
                        *ambient_temperature = Ok(Ds18b20ResultData::new(22.0));
                        *water_temperature = Ok(Ds18b20ResultData::new(25.0));
                    }
                }
            }

            let (
                quit_command_received, // a request to end the application has been received
                _,
                _,
            ) = self.process_external_request(&rx_from_signal_handler, None);

            if quit_command_received {
                println!("Quitting Ds18b20");
                break;
            }

            // sleep for an incremental duration - before checking status from the signal handler
            spin_sleeper.sleep(sleep_duration_hundred_millis);
        }
    }
}

#[cfg(test)]
pub mod tests {
    cfg_if::cfg_if! {
        if #[cfg(any(target_os = "linux", target_os = "macos"))] {
            use crate::sensors::ds18b20::LOG_TARGET_DS18B20;
            use std::env;
        }
    }

    use std::env;
    use crate::sensors::ds18b20::{Ds18b20, Ds18b20Error, LOG_TARGET_DS18B20};
    use crate::utilities::config::read_config_file;

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
                println!("Temperature: {:.2}°C ", temperature);
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
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());

        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });

        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let water_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20);
        match ds18b20.read(&water_temperature_sensor_id) {
            Ok(temperature) => {
                println!(
                    "{}: Temperature read from file: {:.2}°C ({} milliseconds ago)",
                    LOG_TARGET_DS18B20, temperature.value, temperature.measurement_instant.elapsed().as_millis()
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
    #[should_panic]
    // Test case checks if invalid configuration of the system driver base path triggers a panic.
    fn test_dsb18b20_init_with_invalid_system_driver_base_path() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());
        config.ds18b20.system_driver_base_path = "".to_string();
        let _ = Ds18b20::new(config.ds18b20);
    }

    #[test]
    #[should_panic]
    // Test case checks if invalid configuration of the system driver sensor path prefix triggers a panic.
    fn test_dsb18b20_init_with_invalid_system_driver_sensor_path_prefix() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());
        config.ds18b20.system_driver_sensor_path_prefix = "".to_string();
        let _ = Ds18b20::new(config.ds18b20);
    }

    #[test]
    #[should_panic]
    // Test case checks if invalid configuration of the system driver sensor file name triggers a panic.
    fn test_dsb18b20_init_with_invalid_system_driver_file_name() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());
        config.ds18b20.system_driver_file_name = "".to_string();
        let _ = Ds18b20::new(config.ds18b20);
    }

    #[test]
    #[should_panic]
    // Test case checks if invalid configuration of the sensor ids triggers a panic.
    fn test_dsb18b20_init_with_invalid_sensor_ids() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());
        config.ds18b20.active = true;
        config.ds18b20.water_temperature_sensor_id = "".to_string();
        config.ds18b20.ambient_temperature_sensor_id = "".to_string();
        let _ = Ds18b20::new(config.ds18b20);
    }

    #[test]
    // Test case checks if function returns error for an incorrect sensor id
    fn test_dsb18b20_read_from_invalid_sensor_id() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());

        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });

        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let invalid_temperature_sensor_id = "33";

        let ds18b20 = Ds18b20::new(config.ds18b20);

        assert_eq!(
            matches!(
                ds18b20.read(&invalid_temperature_sensor_id),
                Err(Ds18b20Error::SensorIdNotFound(_invalid_temperature_sensor_id))
            ),
            true
        );
    }

    #[test]
    // Test case checks if function returns error when not being able to read from the device file
    fn test_dsb18b20_read_from_file_failed() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());

        config.ds18b20.system_driver_file_name = "xxx".to_string(); // invalidate file name
        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });
        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let valid_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20);

        assert_eq!(
            matches!(
                ds18b20.read(&valid_temperature_sensor_id),
                Err(Ds18b20Error::ReadToStringFailure)
            ),
            true
        );
    }

    #[test]
    // Test case checks if function returns error when device file does not provide enough lines
    fn test_dsb18b20_file_with_one_line_only() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());

        config.ds18b20.system_driver_file_name += "_one_line"; // file with only one line content
        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });
        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let valid_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20);

        assert_eq!(
            matches!(
                ds18b20.read(&valid_temperature_sensor_id),
                Err(Ds18b20Error::InsufficientLinesInFile)),
            true
        );
    }

    #[test]
    // Test case checks if function returns error when first line of device file content does not match "YES"
    fn test_dsb18b20_file_with_line_not_ending_with_yes() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());

        config.ds18b20.system_driver_file_name += "_no_yes"; // file with only one line content
        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });
        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let valid_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20);

        assert_eq!(
            matches!(
                ds18b20.read(&valid_temperature_sensor_id),
                Err(Ds18b20Error::Line0NotEndingWithYes)),
            true
        );
    }

    #[test]
    // Test case checks if function returns error when file content does not contain temperature
    fn test_dsb18b20_file_without_temperature() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());

        config.ds18b20.system_driver_file_name += "_no_temperature"; // file with only one line content
        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });
        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let valid_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20);

        assert_eq!(
            matches!(
                ds18b20.read(&valid_temperature_sensor_id),
                Err(Ds18b20Error::TemperatureEntryNotFound)),
            true
        );
    }

    #[test]
    // Test case checks if function returns error when file content contains unparsable data
    fn test_dsb18b20_unparsable() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());

        config.ds18b20.system_driver_file_name += "_unparsable"; // file with only one line content
        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });
        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let valid_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20);

        assert_eq!(
            matches!(
                ds18b20.read(&valid_temperature_sensor_id),
                Err(Ds18b20Error::TemperatureParseError)),
            true
        );
    }

    #[test]
    // Test case checks if function returns error when temperature is too hot
    fn test_dsb18b20_temperature_too_hot() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());

        config.ds18b20.system_driver_file_name += "_too_hot"; // file with only one line content
        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });
        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let valid_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20);

        assert_eq!(
            matches!(
                ds18b20.read(&valid_temperature_sensor_id),
                Err(Ds18b20Error::TemperatureOutOfRange)),
            true
        );
    }

    #[test]
    // Test case checks if function returns error when temperature is too cold
    fn test_dsb18b20_temperature_too_cold() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());

        config.ds18b20.system_driver_file_name += "_too_cold"; // file with only one line content
        let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
            panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
        });
        config.ds18b20.system_driver_base_path = out_dir + "/tests/fixtures";

        let valid_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

        let ds18b20 = Ds18b20::new(config.ds18b20);

        assert_eq!(
            matches!(
                ds18b20.read(&valid_temperature_sensor_id),
                Err(Ds18b20Error::TemperatureOutOfRange)),
            true
        );
    }
}