/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the sensor management thread for Atlas Scientific I2C devices.
cfg_if::cfg_if! {
    if #[cfg(feature = "test-utils")] {
        use aquarium_control::launch::channels::Channels;
        use aquarium_control::mocks::mock_i2c_interface::tests::MockI2cInterface;
        use aquarium_control::mocks::mock_monitors::tests::MockMonitors;
        use aquarium_control::sensors::atlas_scientific::{atlas_scientific_constants, AtlasScientific, AtlasScientificResultData};
        use aquarium_control::sensors::atlas_scientific_error::AtlasScientificError;
        use aquarium_control::sensors::atlas_scientific_ezo::atlas_scientific_ezo_constants;
        use aquarium_control::sensors::i2c_interface::I2cResponse;
        use aquarium_control::utilities::channel_content::{AquariumSignal, InternalCommand};
        use aquarium_control::utilities::config::{read_config_file, ConfigData};
        use spin_sleep::SpinSleeper;
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::time::Duration;
    }
}

#[cfg(feature = "test-utils")]
#[test]
// Test case checks if AtlasScientific::create_drivers returns an error if the configuration contains invalid data.
// [itest->swreq~sensors_read_data~1]
fn test_new_with_invalid_configuration() {
    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

    // Set an invalid sensor type for temperature
    config.atlas_scientific.sensor_type_temperature = "invalid_type".to_string();
    config.atlas_scientific.sensor_type_ph =
        atlas_scientific_constants::SENSOR_TYPE_OEM.to_string();
    config.atlas_scientific.sensor_type_conductivity =
        atlas_scientific_constants::SENSOR_TYPE_OEM.to_string();

    let result = AtlasScientific::create_drivers(&config.atlas_scientific);

    assert!(result.is_err());
    if let Err(AtlasScientificError::InvalidSensorType {
        location: _,
        signal,
        sensor_type,
    }) = result
    {
        assert_eq!(signal, AquariumSignal::WaterTemperature);
        assert_eq!(sensor_type, "invalid_type");
    } else {
        panic!("Expected InvalidSensorType error, got {:?}", result);
    }

    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

    // Set an invalid sensor type for pH
    config.atlas_scientific.sensor_type_temperature =
        atlas_scientific_constants::SENSOR_TYPE_OEM.to_string();
    config.atlas_scientific.sensor_type_ph = "invalid_type".to_string();
    config.atlas_scientific.sensor_type_conductivity =
        atlas_scientific_constants::SENSOR_TYPE_OEM.to_string();

    let result = AtlasScientific::create_drivers(&config.atlas_scientific);

    assert!(result.is_err());
    if let Err(AtlasScientificError::InvalidSensorType {
        location: _,
        signal,
        sensor_type,
    }) = result
    {
        assert_eq!(signal, AquariumSignal::pH);
        assert_eq!(sensor_type, "invalid_type");
    } else {
        panic!("Expected InvalidSensorType error, got {:?}", result);
    }

    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

    // Set an invalid sensor type for conductivity
    config.atlas_scientific.sensor_type_temperature =
        atlas_scientific_constants::SENSOR_TYPE_OEM.to_string();
    config.atlas_scientific.sensor_type_ph =
        atlas_scientific_constants::SENSOR_TYPE_OEM.to_string();
    config.atlas_scientific.sensor_type_conductivity = "invalid_type".to_string();

    let result = AtlasScientific::create_drivers(&config.atlas_scientific);

    assert!(result.is_err());
    if let Err(AtlasScientificError::InvalidSensorType {
        location: _,
        signal,
        sensor_type,
    }) = result
    {
        assert_eq!(signal, AquariumSignal::Conductivity);
        assert_eq!(sensor_type, "invalid_type");
    } else {
        panic!("Expected InvalidSensorType error, got {:?}", result);
    }
}

#[cfg(feature = "test-utils")]
#[test]
// Test function receive_response_from_i2c_interface
//
// [itest->swreq~sensors_read_data~1]
pub fn test_atlas_scientific_interfaces_with_blocking_mutexes() {
    let mut stimuli_array_temperature = [0u8; 64];
    let message_temperature: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
        atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
        b'2',
        b'3',
        atlas_scientific_ezo_constants::DECIMAL_POINT,
        b'1',
        b'0',
        b'0',
        atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
    ];
    stimuli_array_temperature[..message_temperature.len()].copy_from_slice(&message_temperature);
    let stimuli_i2c_response_temperature = I2cResponse {
        read_buf: stimuli_array_temperature,
        length: atlas_scientific_ezo_constants::MESSAGE_SIZE,
    };

    let mut stimuli_array_conductivity = [0u8; 64];
    let message_conductivity: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
        atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
        b'4',
        b'8',
        b'0',
        b'0',
        b'0',
        b'0',
        atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
    ];
    stimuli_array_conductivity[..message_conductivity.len()].copy_from_slice(&message_conductivity);
    let stimuli_i2c_response_conductivity = I2cResponse {
        read_buf: stimuli_array_conductivity,
        length: atlas_scientific_ezo_constants::MESSAGE_SIZE,
    };

    let mut stimuli_array_ph = [0u8; 64];
    let message_ph: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
        atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
        b'7',
        atlas_scientific_ezo_constants::DECIMAL_POINT,
        b'2',
        b'0',
        b'0',
        b'0',
        atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
    ];
    stimuli_array_ph[..message_ph.len()].copy_from_slice(&message_ph);
    let stimuli_i2c_response_ph = I2cResponse {
        read_buf: stimuli_array_ph,
        length: atlas_scientific_ezo_constants::MESSAGE_SIZE,
    };

    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.atlas_scientific.sensor_type_temperature =
        atlas_scientific_constants::SENSOR_TYPE_EZO.to_string();
    config.atlas_scientific.sensor_type_ph =
        atlas_scientific_constants::SENSOR_TYPE_EZO.to_string();
    config.atlas_scientific.sensor_type_conductivity =
        atlas_scientific_constants::SENSOR_TYPE_EZO.to_string();

    let measurement_interval_millis_for_test_environment =
        config.atlas_scientific.measurement_interval_millis;

    let mut mock_i2c_interface = MockI2cInterface::new(
        &config.atlas_scientific,
        stimuli_i2c_response_temperature.clone(),
        stimuli_i2c_response_ph.clone(),
        stimuli_i2c_response_conductivity.clone(),
    );

    let mut mock_monitors = MockMonitors::new();

    let drivers = AtlasScientific::create_drivers(&config.atlas_scientific).unwrap();
    let mut atlas_scientific = AtlasScientific::new(config.atlas_scientific, drivers);

    let mut channels = Channels::new_for_test();

    // Mutexes for Atlas Scientific signals
    let mutex_atlas_scientific_temperature = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_water_temperature,
    )));
    let mutex_atlas_scientific_temperature_clone_for_asserts =
        mutex_atlas_scientific_temperature.clone();
    let mutex_atlas_scientific_temperature_clone_for_test_environment =
        mutex_atlas_scientific_temperature.clone();

    let mutex_atlas_scientific_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_ph,
    )));
    let mutex_atlas_scientific_ph_clone_for_asserts = mutex_atlas_scientific_ph.clone();
    let mutex_atlas_scientific_ph_clone_for_test_environment = mutex_atlas_scientific_ph.clone();

    let mutex_atlas_scientific_conductivity = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_conductivity,
    )));
    let mutex_atlas_scientific_conductivity_clone_for_asserts =
        mutex_atlas_scientific_conductivity.clone();
    let mutex_atlas_scientific_conductivity_clone_for_test_environment =
        mutex_atlas_scientific_conductivity.clone();

    // thread for test environment (incl. signal handler)
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_time =
                Duration::from_millis(measurement_interval_millis_for_test_environment + 5000);
            let spin_sleeper = SpinSleeper::default();

            {
                // lock the mutexes one by one and wait for triggering the detection
                let _guard_temp = mutex_atlas_scientific_temperature_clone_for_test_environment
                    .lock()
                    .unwrap_or_else(|e| {
                        panic!("mutex_atlas_scientific_temperature lock poisoned: {:?}", e)
                    });
                // give the test object some time to try to claim the lock
                println!("Test environment acquired lock for temperature");
                spin_sleeper.sleep(sleep_time);
                println!("Test environment released lock for temperature");
            }

            {
                let _guard_ph = mutex_atlas_scientific_ph_clone_for_test_environment
                    .lock()
                    .unwrap_or_else(|e| panic!("mutex_atlas_scientific_ph lock poisoned: {:?}", e));
                // give the test object some time to try to claim the lock
                println!("Test environment acquired lock for pH");
                spin_sleeper.sleep(sleep_time);
                println!("Test environment released lock for pH");
            }

            {
                let _guard_cond = mutex_atlas_scientific_conductivity_clone_for_test_environment
                    .lock()
                    .unwrap_or_else(|e| {
                        panic!("mutex_atlas_scientific_conductivity lock poisoned: {:?}", e)
                    });
                // give the test object some time to try to claim the lock
                println!("Test environment acquired lock for conductivity");
                spin_sleeper.sleep(sleep_time);
                println!("Test environment released lock for conductivity");
            }

            channels
                .signal_handler
                .tx_signal_handler_to_atlas_scientific
                .send(InternalCommand::Quit)
                .unwrap();
            channels
                .signal_handler
                .tx_signal_handler_to_i2c_interface
                .send(InternalCommand::Quit)
                .unwrap();
            channels
                .signal_handler
                .tx_signal_handler_to_monitors
                .send(InternalCommand::Quit)
                .unwrap();
            channels
                .signal_handler
                .tx_signal_handler_to_atlas_scientific
                .send(InternalCommand::Terminate)
                .unwrap();
        })
        .unwrap();

    // thread for mock i2c interface
    let join_handle_mock_i2c_interface = thread::Builder::new()
        .name("i2c_interface".to_string())
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    // blocking execution used for testing
                    mock_i2c_interface.execute(channels.i2c_interface).await;
                });
        })
        .unwrap();

    // thread for mock Monitors
    let join_handle_mock_monitors = thread::Builder::new()
        .name("mock_monitors".to_string())
        .spawn(move || {
            mock_monitors.execute(
                None,
                Some(channels.monitors.rx_monitors_from_atlas_scientific),
                channels.monitors.rx_monitors_from_signal_handler,
            );
        })
        .unwrap();

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // avoid race condition - let test environment lock the mutexes first
            let sleep_time = Duration::from_millis(1000);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_time);
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    // blocking execution used for testing
                    atlas_scientific
                        .execute(
                            &mut channels.atlas_scientific,
                            mutex_atlas_scientific_temperature,
                            mutex_atlas_scientific_ph,
                            mutex_atlas_scientific_conductivity,
                        )
                        .await;
                });
            let driver_temperature = atlas_scientific
                .drivers
                .get(&AquariumSignal::WaterTemperature)
                .unwrap();
            let driver_ph = atlas_scientific.drivers.get(&AquariumSignal::pH).unwrap();
            let driver_conductivity = atlas_scientific
                .drivers
                .get(&AquariumSignal::Conductivity)
                .unwrap();
            assert!(driver_temperature
                .get_mutex_access_duration_exceeded(&AquariumSignal::WaterTemperature));
            assert!(driver_ph.get_mutex_access_duration_exceeded(&AquariumSignal::pH));
            assert!(driver_conductivity
                .get_mutex_access_duration_exceeded(&AquariumSignal::Conductivity));
        })
        .unwrap();

    join_handle_test_object
        .join()
        .expect("Test object did not finish.");
    join_handle_mock_i2c_interface
        .join()
        .expect("Mock i2c interface did not finish.");
    join_handle_mock_monitors
        .join()
        .expect("Mock Monitors did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment did not finish.");

    match mutex_atlas_scientific_temperature_clone_for_asserts.lock() {
        Ok(result) => {
            let result_data = result.clone();
            assert_eq!(result_data.value, 23.1);
        }
        Err(e) => {
            panic!("mutex_atlas_scientific_temperature lock poisoned: {:?}", e);
        }
    };

    match mutex_atlas_scientific_ph_clone_for_asserts.lock() {
        Ok(result) => {
            let result_data = result.clone();
            assert_eq!(result_data.value, 7.2);
        }
        Err(e) => {
            panic!("mutex_atlas_scientific_ph lock poisoned: {:?}", e);
        }
    };

    match mutex_atlas_scientific_conductivity_clone_for_asserts.lock() {
        Ok(result) => {
            let result_data = result.clone();
            assert_eq!(result_data.value, 48000.0);
        }
        Err(e) => {
            panic!("mutex_atlas_scientific_conductivity lock poisoned: {:?}", e);
        }
    };
}
