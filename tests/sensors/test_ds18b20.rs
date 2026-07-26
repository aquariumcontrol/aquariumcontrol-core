/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests temperature reading from DS18B20 1-Wire digital thermometers.

use aquarium_control::sensors::ds18b20::Ds18b20;
use aquarium_control::sensors::ds18b20_error::Ds18b20Error;
use aquarium_control::utilities::config::read_config_file;
use std::env;

cfg_if::cfg_if! {
    if #[cfg(feature = "test-utils")] {
        use std::thread;
        use std::sync::{Arc, Mutex};
        use aquarium_control::sensors::ds18b20_channels::Ds18b20Channels;
        use std::time::Duration;
        use spin_sleep::SpinSleeper;
        use aquarium_control::launch::channels::{Channels, channel, AquaSender, AquaReceiver};
        use aquarium_control::sensors::ds18b20::{Ds18b20Result, Ds18b20ResultData};
        use aquarium_control::utilities::channel_content::InternalCommand;
    }
}

#[cfg(feature = "test-utils")]
/// Synchronously executes the main task loop for testing.
///
/// # Arguments
/// * `ds18b20` - A parameter representing `ds18b20`.
/// * `ds18b20_channels` - A mutable reference to the `Ds18b20Channels` communication interface.
/// * `mutex_water_temperature` - A parameter representing `mutex_water_temperature`.
/// * `mutex_ambient_temperature` - A parameter representing `mutex_ambient_temperature`.
fn execute_blocking(
    ds18b20: &mut Ds18b20,
    ds18b20_channels: &mut Ds18b20Channels,
    mutex_water_temperature: Arc<Mutex<Ds18b20Result>>,
    mutex_ambient_temperature: Arc<Mutex<Ds18b20Result>>,
) {
    tokio::runtime::Builder::new_multi_thread().enable_all()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            // blocking execution used for testing
            ds18b20
                .execute(
                    ds18b20_channels,
                    mutex_water_temperature,
                    mutex_ambient_temperature,
                )
                .await;
        });
}

#[test]
#[ignore]
#[cfg(all(target_os = "linux", feature = "target_hw"))]
// This test case reads from a real DS18B20 sensor.
// For this test case to succeed, the DS18B20 sensor needs to be connected,
// and the 1-wire interface needs to be enabled (see rasp-config)
///

/// [itest->swreq~sensors_read_data~1]
fn test_read_ds18b20() {
    let config = read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

    let water_temperature_sensor_id = config.ds18b20.water_temperature_sensor_id.clone();

    let ds18b20 = Ds18b20::new(config.ds18b20).unwrap();

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
        }
        Err(e) => {
            panic!("Error reading DS18B20: {}", e);
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

    assert!(matches!(
        ds18b20.read(invalid_temperature_sensor_id),
        Err(Ds18b20Error::SensorIdNotFound(
            _,
            _invalid_temperature_sensor_id
        ))
    ));
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

    assert!(matches!(
        ds18b20.read(&valid_temperature_sensor_id),
        Err(Ds18b20Error::ReadToStringFailure(_))
    ));
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

    assert!(matches!(
        ds18b20.read(&valid_temperature_sensor_id),
        Err(Ds18b20Error::InsufficientLinesInFile(_))
    ));
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

    assert!(matches!(
        ds18b20.read(&valid_temperature_sensor_id),
        Err(Ds18b20Error::Line0NotEndingWithYes(_))
    ));
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

    assert!(matches!(
        ds18b20.read(&valid_temperature_sensor_id),
        Err(Ds18b20Error::TemperatureEntryNotFound(_))
    ));
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

    assert!(matches!(
        ds18b20.read(&valid_temperature_sensor_id),
        Err(Ds18b20Error::TemperatureParseError(_))
    ));
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

    assert!(matches!(
        ds18b20.read(&valid_temperature_sensor_id),
        Err(Ds18b20Error::TemperatureOutOfRange(_))
    ));
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

    assert!(matches!(
        ds18b20.read(&valid_temperature_sensor_id),
        Err(Ds18b20Error::TemperatureOutOfRange(_))
    ));
}

#[cfg(feature = "test-utils")]
#[test]
// Verifies that dsb18b20 channel mutexes without locking mutexes.
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
    let mutex_ds18b20_ambient_temperature: Arc<Mutex<Ds18b20Result>> = Arc::new(Mutex::new(Ok(
        Ds18b20ResultData::new(config.sensor_manager.replacement_value_ambient_temperature),
    )));
    let mutex_ds18b20_water_temperature_clone_for_asserts = mutex_ds18b20_water_temperature.clone();
    let mutex_ds18b20_ambient_temperature_clone_for_asserts =
        mutex_ds18b20_ambient_temperature.clone();

    let mut ds18b20 = Ds18b20::new(config.ds18b20).unwrap();

    let mut channels = Channels::new_for_test();

    // thread for test environment (incl. signal handler)
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            // let test run a bit longer than the measurement interval
            let sleep_time = Duration::from_millis(1000);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_time);
            channels
                .signal_handler
                .tx_signal_handler_to_ds18b20
                .send(InternalCommand::Quit)
                .unwrap();
        })
        .unwrap();

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            execute_blocking(
                &mut ds18b20,
                &mut channels.ds18b20,
                mutex_ds18b20_water_temperature,
                mutex_ds18b20_ambient_temperature,
            );
            assert!(!ds18b20.mutex_access_duration_exceeded);
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

#[cfg(feature = "test-utils")]
#[test]
// Verifies that dsb18b20 channel mutexes with locking mutexes.
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
    let mutex_ds18b20_ambient_temperature: Arc<Mutex<Ds18b20Result>> = Arc::new(Mutex::new(Ok(
        Ds18b20ResultData::new(config.sensor_manager.replacement_value_ambient_temperature),
    )));
    let mutex_ds18b20_water_temperature_clone_for_test_environment =
        mutex_ds18b20_water_temperature.clone();
    let mutex_ds18b20_water_temperature_clone_for_asserts = mutex_ds18b20_water_temperature.clone();
    let mutex_ds18b20_ambient_temperature_clone_for_asserts =
        mutex_ds18b20_ambient_temperature.clone();

    let mut ds18b20 = Ds18b20::new(config.ds18b20).unwrap();

    // *** signal handler => ds18b20 ***
    let (mut tx_signal_handler_to_ds18b20, rx_ds18b20_from_signal_handler): (
        AquaSender<InternalCommand>,
        AquaReceiver<InternalCommand>,
    ) = channel(1, true);
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
            execute_blocking(
                &mut ds18b20,
                &mut ds18b20_channels,
                mutex_ds18b20_water_temperature,
                mutex_ds18b20_ambient_temperature,
            );
            assert!(ds18b20.mutex_access_duration_exceeded);
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
