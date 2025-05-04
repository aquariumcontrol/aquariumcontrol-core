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

cfg_if::cfg_if! {
    if #[cfg(any(target_os = "linux", target_os = "macos"))] {
        use std::thread::sleep;
        use std::io;
        use std::fs;
        use std::path::PathBuf;

        const LOG_TARGET_DS18B20: &str = "ds18b20";

        #[cfg(feature = "debug_ds18b20")]
        use log::debug;
    }
}

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

    /// max. number of retries
    #[allow(unused)]
    pub max_retries: u32,

    /// duration between retries in milliseconds
    #[allow(unused)]
    pub retry_pause_duration_millis: u64,

    /// unique 64-bit ID of water temperature sensor
    pub water_temperature_sensor_id: String,

    /// unique 64-bit ID of ambient temperature sensor
    pub ambient_temperature_sensor_id: String,
}

pub mod ds18b20_constants {}

#[derive(Debug, PartialEq)]
pub enum Ds18b20Error {}

pub struct Ds18b20 {
    // configuration for communication with DHT sensor
    #[allow(unused)]
    config: Ds18b20Config,
}

impl ProcessExternalRequestTrait for Ds18b20 {}

impl Ds18b20 {
    /// Provides a struct of type Ds18b20 - Linux platform-specific version
    /// # Arguments
    /// * `config` - configuration for communication with DHT sensor
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

    /// reads temperature from DS18B20 sensor
    #[allow(unused)]
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

        Err(io::Error::new(
            io::ErrorKind::Other,
            "Failed to read temperature after multiple retries.",
        ))
    }

    /// Executes the main control in a loop until the signal handler commands quitting and termination.
    /// # Arguments
    /// * `rx_from_signal_handler` - Receiver part of the channel for communication from signal handler                
    /// * `mutex_water_temperature` - Mutex for water temperature                
    /// * `mutex_ambient_temperature` - Mutex for ambient temperature                
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
                            *water_temperature = self.config.replacement_value_water_temperature;
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
    use crate::sensors::ds18b20::LOG_TARGET_DS18B20;
    use std::env;

    cfg_if::cfg_if! {
        if #[cfg(any(target_os = "linux", target_os = "macos"))] {
            use crate::sensors::ds18b20::Ds18b20;
            use crate::utilities::config::read_config_file;
        }
    }

    // This test case reads from a real DS18B20 sensor.
    // For this test case to succeed, the DS18B20 sensor needs to be connected,
    // and the 1-wire interface needs to be enabled (see rasp-config)
    #[test]
    #[ignore]
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
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
    #[cfg(any(target_os = "linux", target_os = "macos"))]
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
