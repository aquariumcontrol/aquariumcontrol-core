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
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
        use std::thread::sleep;
        use std::io;
        use std::fs;
        use std::path::PathBuf;

        const LOG_TARGET_DS18B20: &str = "ds18b20";

        #[cfg(feature = "debug_ds18b20")]
        use log::debug;

        use log::error;
    }
}

/// Holds the configuration data for DS18B20 sensor communication.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct Ds18b20Config {
    /// pause duration between two measurements in milliseconds
    measurement_pause_duration_millis: i32,

    /// replacement value for ambient temperature
    pub replacement_value_ambient_temperature: f32,

    /// replacement value for water temperature
    pub replacement_value_water_temperature: f32,

    /// system driver base path
    #[allow(unused)]
    pub system_driver_base_path: String,

    /// system driver sensor-specific path
    #[allow(unused)]
    pub system_driver_sensor_path_prefix: String,

    /// system driver file name
    #[allow(unused)]
    pub system_driver_file_name: String,

    /// max. number of retries
    #[allow(unused)]
    pub max_retries: u32,

    /// duration between retries in milliseconds
    #[allow(unused)]
    pub retry_pause_duration_millis: u64,
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
        Ds18b20 { config: config }
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// reads temperature from DS18B20 sensor
    pub fn read(&self) -> Result<f32, io::Error> {
        let base_dir = PathBuf::from(self.config.system_driver_base_path.clone());
        let mut device_folder: Option<PathBuf> = None;

        // Find the DS18B20 device folder
        if let Ok(entries) = fs::read_dir(base_dir) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let file_name = entry.file_name();
                    if let Some(name_str) = file_name.to_str() {
                        if name_str.starts_with(&self.config.system_driver_sensor_path_prefix) {
                            device_folder = Some(entry.path());
                            break;
                        }
                    }
                }
            }
        }

        let device_folder = match device_folder {
            Some(folder) => folder,
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "DS18B20 sensor not found!",
                ))
            }
        };

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
                        *ambient_temperature = self.config.replacement_value_ambient_temperature;
                        match self.read() {
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
                    }

                    // restart the countdown
                    measurement_countdown = self.config.measurement_pause_duration_millis;

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
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    use crate::sensors::ds18b20::Ds18b20;

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    use crate::utilities::config::read_config_file;

    // This test case reads from DS18B20 sensor.
    // For this test case to succeed, the DS18B20 sensor needs to be connected
    // and the 1-wire interface needs to be enabled (see rasp-config)
    #[test]
    #[ignore]
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    fn test_read_ds18b20() {
        let config = read_config_file("config/aquarium_control_test_generic.toml".to_string());

        let ds18b20 = Ds18b20::new(config.ds18b20);

        match ds18b20.read() {
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
}
