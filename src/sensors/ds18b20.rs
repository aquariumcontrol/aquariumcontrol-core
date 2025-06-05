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
use std::time::Duration;

use std::thread::sleep;
use std::io;
use std::fs;
use std::path::PathBuf;

const LOG_TARGET_DS18B20: &str = "ds18b20";

#[cfg(feature = "debug_ds18b20")]
use log::debug;

#[cfg(all(target_os = "linux", feature = "target_hw"))]
use log::error;

/// Holds the configuration data for DS18B20 sensor communication.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct Ds18b20Config {
    /// used primarily for testing purposes
    pub active: bool,

    /// pause duration between two measurements in milliseconds
    measurement_pause_duration_millis: u64,

    /// replacement value for ambient temperature
    pub replacement_value_ambient_temperature: f32,

    /// replacement value for water temperature
    pub replacement_value_water_temperature: f32,

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
    /// A `Result<f32, io::Error>`:
    /// - `Ok(f32)`: The temperature in degree Celsius if successfully read and parsed.
    /// - `Err(io::Error)`: If the sensor's device folder or file is not found, or if
    ///   reading fails after all retries (e.g., due to communication errors, invalid data format).
    pub fn read(&self, sensor_id: &str) -> Result<f32, io::Error> {
        let base_dir = PathBuf::from(self.config.system_driver_base_path.clone());
        let device_folder =
            base_dir.join(self.config.system_driver_sensor_path_prefix.to_string() + sensor_id);

        if !device_folder.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("DS18B20 sensor with ID '{}' not found!", sensor_id),
            ));
        }

        let device_file = device_folder.join(self.config.system_driver_file_name.clone());

        let mut retries = self.config.max_retries;
        while retries > 0 {
            let contents = fs::read_to_string(&device_file)?;
            let lines: Vec<&str> = contents.trim().split('\n').collect();

            if lines.len() >= 2 && lines[0].ends_with(" YES") {
                if let Some(temp_line) = lines[1].find("t=") {
                    if let Ok(temp_str) = lines[1][temp_line + 2..].trim().parse::<f32>() {
                        let celsius = temp_str / 1000.0;
                        // let fahrenheit = (celsius * 9.0 / 5.0) + 32.0;
                        return Ok(celsius);
                    }
                }
            }

            sleep(Duration::from_millis(
                self.config.retry_pause_duration_millis,
            ));
            retries -= 1;
        }

        Err(io::Error::other(
            "Failed to read temperature after multiple retries.",
        ))
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
    /// * `mutex_water_temperature` - An `Arc<Mutex<f32>>` into which the latest water temperature reading
    ///   will be written, ensuring thread-safe access for other modules.
    /// * `mutex_ambient_temperature` - An `Arc<Mutex<f32>>` into which the latest ambient temperature reading
    ///   will be written, ensuring thread-safe access for other modules.
    pub fn execute(
        &mut self,
        rx_from_signal_handler: Receiver<InternalCommand>,
        mutex_water_temperature: Arc<Mutex<f32>>,
        mutex_ambient_temperature: Arc<Mutex<f32>>,
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
                        *ambient_temperature = self.config.replacement_value_ambient_temperature;
                        *water_temperature = self.config.replacement_value_water_temperature;
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
    
    use crate::utilities::config::read_config_file;
    use crate::sensors::ds18b20::Ds18b20;

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
    #[cfg(any(target_os = "linux", target_os = "macos"))]
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
                    "{}: Temperature read from file: {:.2}°C ",
                    LOG_TARGET_DS18B20, temperature
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
    // Test cases checks if invalid configuration of the system driver base path triggers a panic.
    fn test_dsb18b20_init_with_invalid_system_driver_base_path() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());
        config.ds18b20.system_driver_base_path = "".to_string();
        let _ = Ds18b20::new(config.ds18b20);
    }

    #[test]
    #[should_panic]
    // Test cases checks if invalid configuration of the system driver sensor path prefix triggers a panic.
    fn test_dsb18b20_init_with_invalid_system_driver_sensor_path_prefix() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());
        config.ds18b20.system_driver_sensor_path_prefix = "".to_string();
        let _ = Ds18b20::new(config.ds18b20);
    }

    #[test]
    #[should_panic]
    // Test cases checks if invalid configuration of the system driver sensor file name triggers a panic.
    fn test_dsb18b20_init_with_invalid_system_driver_file_name() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());
        config.ds18b20.system_driver_file_name = "".to_string();
        let _ = Ds18b20::new(config.ds18b20);
    }

    #[test]
    #[should_panic]
    // Test cases checks if invalid configuration of the sensor ids triggers a panic.
    fn test_dsb18b20_init_with_invalid_sensor_ids() {
        let mut config = read_config_file("config/aquarium_control_test_generic.toml".to_string());
        config.ds18b20.active = true;
        config.ds18b20.water_temperature_sensor_id = "".to_string();
        config.ds18b20.ambient_temperature_sensor_id = "".to_string();
        let _ = Ds18b20::new(config.ds18b20);
    }
}
