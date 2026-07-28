/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Testing the central sensor data aggregation and management thread.
#![cfg(feature = "test-utils")]

use aquarium_control::launch::channels::Channels;
use aquarium_control::sensors::atlas_scientific::AtlasScientificResultData;
use aquarium_control::sensors::ds18b20::Ds18b20ResultData;
use aquarium_control::sensors::sensor_manager::{
    SensorManager, SensorManagerChannels, SensorManagerError, SensorManagerMutexes,
    SensorManagerSignals,
};
use aquarium_control::utilities::channel_content::InternalCommand;
use aquarium_control::utilities::config::{read_config_file, ConfigData};
use aquarium_control::utilities::signal_handler_channels::SignalHandlerChannels;
use spin_sleep::SpinSleeper;
use std::sync::{Arc, Mutex};
use std::thread::scope;
use std::time::Duration;

#[test]
// Verifies that sensor manager error water temperature source is invalid.
pub fn test_sensor_manager_error_water_temperature_source_is_invalid() {
    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.sensor_manager.source_water_temperature = "invalid".to_string();
    let test_result = SensorManager::new(config.sensor_manager);
    assert!(matches!(
        test_result,
        Err(SensorManagerError::InvalidWaterTemperatureConfiguration(
            _,
            _,
            _,
            _
        ))
    ));
}

#[test]
// Verifies that sensor manager error ambient temperature source is invalid.
pub fn test_sensor_manager_error_ambient_temperature_source_is_invalid() {
    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.sensor_manager.source_ambient_temperature = "invalid".to_string();
    let test_result = SensorManager::new(config.sensor_manager);
    assert!(matches!(
        test_result,
        Err(SensorManagerError::InvalidAmbientTemperatureConfiguration(
            _,
            _,
            _,
            _
        ))
    ));
}

/// Performs the `mock_signal_handler_for_sensor_manager` operation.
///
/// # Arguments
/// * `signal_handler_channels` - A parameter representing `signal_handler_channels`.
/// * `mutex_sensor_manager_opt` - A parameter representing `mutex_sensor_manager_opt`.
fn mock_signal_handler_for_sensor_manager(
    mut signal_handler_channels: SignalHandlerChannels,
    mutex_sensor_manager_opt: Option<Arc<Mutex<SensorManagerSignals>>>,
) {
    let spin_sleeper = SpinSleeper::default();
    // mock signal handler
    if let Some(mutex_sensor_manager) = mutex_sensor_manager_opt {
        let sleep_duration_1_second = Duration::from_millis(1000);
        for i in 0..8 {
            if i % 2 > 0 {
                {
                    // block the mutex on purpose
                    let _unused = mutex_sensor_manager.lock().unwrap();
                    spin_sleeper.sleep(sleep_duration_1_second);
                }
            } else {
                spin_sleeper.sleep(sleep_duration_1_second);
            }
        }
    } else {
        let sleep_duration_8_seconds = Duration::from_millis(8000);
        spin_sleeper.sleep(sleep_duration_8_seconds);
    }
    let _ = signal_handler_channels
        .tx_signal_handler_to_sensor_manager
        .send(InternalCommand::Quit);
    signal_handler_channels
        .rx_signal_handler_from_sensor_manager
        .blocking_recv()
        .unwrap();
    let _ = signal_handler_channels
        .tx_signal_handler_to_sensor_manager
        .send(InternalCommand::Terminate);
}

/// Helper function to execute the sensor manager task inside a test Tokio runtime.
///
/// # Arguments
/// * `sensor_manager` - A mutable reference to the `SensorManager` instance.
/// * `sensor_manager_channels` - A mutable reference to the `SensorManagerChannels` instance.
/// * `mutexes` - Shared mutexes for accessing sensor readings.
fn execute_blocking(
    sensor_manager: &mut SensorManager,
    sensor_manager_channels: &mut SensorManagerChannels,
    mutexes: SensorManagerMutexes,
) {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            // blocking execution used for testing
            sensor_manager
                .execute(sensor_manager_channels, mutexes)
                .await;
        });
}

/// Performs the `execute_and_assert_sensor_manager` operation.
///
/// # Arguments
/// * `sensor_manager_channels` - A mutable reference to the `SensorManagerChannels` communication interface.
/// * `sensor_manager` - A parameter representing `sensor_manager`.
/// * `mutexes` - Shared mutexes for accessing sensor readings.
/// * `target_value_water_temperature` - A parameter representing `target_value_water_temperature`.
/// * `target_value_ambient_temperature` - A parameter representing `target_value_ambient_temperature`.
/// * `reference_mutex_access_time_exceeded` - A parameter representing `reference_mutex_access_time_exceeded`.
fn execute_and_assert_sensor_manager(
    mut sensor_manager_channels: SensorManagerChannels,
    mut sensor_manager: SensorManager,
    mutexes: SensorManagerMutexes,
    target_value_water_temperature: f32,
    target_value_ambient_temperature: f32,
    reference_mutex_access_time_exceeded: bool,
) {
    let mutexes_clone_for_asserts = mutexes.clone();

    // test object
    execute_blocking(&mut sensor_manager, &mut sensor_manager_channels, mutexes);

    match mutexes_clone_for_asserts
        .mutex_sensor_manager_signals
        .lock()
    {
        Ok(mutex_sensor_manager_signals_after_run) => {
            assert_eq!(
                mutex_sensor_manager_signals_after_run.water_temperature,
                target_value_water_temperature
            );
            assert_eq!(
                mutex_sensor_manager_signals_after_run.ambient_temperature,
                target_value_ambient_temperature
            );
        }
        Err(e) => {
            panic!("Could not lock mutex: {e:?}")
        }
    };

    assert_eq!(
        sensor_manager.mutex_access_duration_exceeded,
        reference_mutex_access_time_exceeded
    );
}

#[test]
// Test case verifies if the test object is processing the interfaces correctly using input
// from Ds18b20 sensors.
pub fn test_sensor_manager_interfaces_using_ds18b20_as_input_without_mutex_blocked() {
    let channels = Channels::new_for_test();

    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.sensor_manager.use_simulator = false;
    config.sensor_manager.source_ambient_temperature = "ds18b20".to_string();
    config.sensor_manager.source_water_temperature = "ds18b20".to_string();

    let target_value_water_temperature = config.sensor_manager.replacement_value_water_temperature;
    let target_value_ambient_temperature =
        config.sensor_manager.replacement_value_ambient_temperature;

    let mutex_ds18b20_water_temperature = Arc::new(Mutex::new(Ok(Ds18b20ResultData::new(
        target_value_water_temperature,
    ))));
    let mutex_ds18b20_ambient_temperature = Arc::new(Mutex::new(Ok(Ds18b20ResultData::new(
        target_value_ambient_temperature,
    ))));
    let mutex_atlas_scientific_temperature = Arc::new(Mutex::new(AtlasScientificResultData::new(
        target_value_water_temperature + 1.0,
    )));
    let mutex_atlas_scientific_conductivity = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_conductivity,
    )));
    let mutex_atlas_scientific_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_ph,
    )));
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));
    let dht_result = Ok((target_value_ambient_temperature + 1.0, 2.0));
    let mutex_dht = Arc::new(Mutex::new(dht_result));

    let sensor_manager = SensorManager::new(config.sensor_manager).unwrap();

    let mutexes = SensorManagerMutexes {
        mutex_ds18b20_ambient_temperature,
        mutex_ds18b20_water_temperature,
        mutex_dht,
        mutex_atlas_scientific_temperature,
        mutex_atlas_scientific_ph,
        mutex_atlas_scientific_conductivity,
        mutex_sensor_manager_signals,
    };

    scope(|scope| {
        scope.spawn(move || {
            mock_signal_handler_for_sensor_manager(channels.signal_handler, None);
        });

        scope.spawn(move || {
            execute_and_assert_sensor_manager(
                channels.sensor_manager,
                sensor_manager,
                mutexes,
                target_value_water_temperature,
                target_value_ambient_temperature,
                false, // no mutex blocking in this test case
            );
        });
    });
}

#[test]
// Test case verifies if the test object is processing the interfaces correctly using input
// from Ds18b20 sensors.
pub fn test_sensor_manager_interfaces_using_atlas_scientific_and_dht_as_input() {
    let channels = Channels::new_for_test();

    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.sensor_manager.use_simulator = false;
    config.sensor_manager.source_ambient_temperature = "dht".to_string();
    config.sensor_manager.source_water_temperature = "atlas_scientific".to_string();

    let target_value_water_temperature = config.sensor_manager.replacement_value_water_temperature;
    let target_value_ambient_temperature =
        config.sensor_manager.replacement_value_ambient_temperature;

    let mutex_ds18b20_water_temperature = Arc::new(Mutex::new(Ok(Ds18b20ResultData::new(
        target_value_water_temperature + 1.0,
    ))));
    let mutex_ds18b20_ambient_temperature = Arc::new(Mutex::new(Ok(Ds18b20ResultData::new(
        target_value_ambient_temperature + 1.0,
    ))));
    let mutex_atlas_scientific_temperature = Arc::new(Mutex::new(AtlasScientificResultData::new(
        target_value_water_temperature,
    )));
    let mutex_atlas_scientific_conductivity = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_conductivity,
    )));
    let mutex_atlas_scientific_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_ph,
    )));
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));
    let dht_result = Ok((target_value_ambient_temperature, 2.0));
    let mutex_dht = Arc::new(Mutex::new(dht_result));

    let sensor_manager = SensorManager::new(config.sensor_manager).unwrap();

    let mutexes = SensorManagerMutexes {
        mutex_ds18b20_ambient_temperature,
        mutex_ds18b20_water_temperature,
        mutex_dht,
        mutex_atlas_scientific_temperature,
        mutex_atlas_scientific_ph,
        mutex_atlas_scientific_conductivity,
        mutex_sensor_manager_signals,
    };

    scope(|scope| {
        scope.spawn(move || {
            mock_signal_handler_for_sensor_manager(channels.signal_handler, None);
        });

        scope.spawn(move || {
            execute_and_assert_sensor_manager(
                channels.sensor_manager,
                sensor_manager,
                mutexes,
                target_value_water_temperature,
                target_value_ambient_temperature,
                false, // no mutex blocking in this test case
            );
        });
    });
}

#[test]
// Test case verifies if the test object is processing the interfaces correctly using input
// from Ds18b20 sensors while having mutex blocked
pub fn test_sensor_manager_interfaces_using_ds18b20_as_input_with_mutex_blocked() {
    let channels = Channels::new_for_test();

    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.sensor_manager.use_simulator = false;
    config.sensor_manager.source_ambient_temperature = "ds18b20".to_string();
    config.sensor_manager.source_water_temperature = "ds18b20".to_string();

    let target_value_water_temperature = config.sensor_manager.replacement_value_water_temperature;
    let target_value_ambient_temperature =
        config.sensor_manager.replacement_value_ambient_temperature;

    let mutex_ds18b20_water_temperature = Arc::new(Mutex::new(Ok(Ds18b20ResultData::new(
        target_value_water_temperature,
    ))));
    let mutex_ds18b20_ambient_temperature = Arc::new(Mutex::new(Ok(Ds18b20ResultData::new(
        target_value_ambient_temperature,
    ))));
    let mutex_atlas_scientific_temperature = Arc::new(Mutex::new(AtlasScientificResultData::new(
        target_value_water_temperature + 1.0,
    )));
    let mutex_atlas_scientific_conductivity = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_conductivity,
    )));
    let mutex_atlas_scientific_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_ph,
    )));
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));
    let mutex_sensor_manager_signals_clone_for_test_environment =
        mutex_sensor_manager_signals.clone();

    let dht_result = Ok((target_value_ambient_temperature + 1.0, 2.0));
    let mutex_dht = Arc::new(Mutex::new(dht_result));

    let sensor_manager = SensorManager::new(config.sensor_manager).unwrap();

    let mutexes = SensorManagerMutexes {
        mutex_ds18b20_ambient_temperature,
        mutex_ds18b20_water_temperature,
        mutex_dht,
        mutex_atlas_scientific_temperature,
        mutex_atlas_scientific_ph,
        mutex_atlas_scientific_conductivity,
        mutex_sensor_manager_signals,
    };

    scope(|scope| {
        scope.spawn(move || {
            mock_signal_handler_for_sensor_manager(
                channels.signal_handler,
                Some(mutex_sensor_manager_signals_clone_for_test_environment),
            );
        });

        scope.spawn(move || {
            execute_and_assert_sensor_manager(
                channels.sensor_manager,
                sensor_manager,
                mutexes,
                target_value_water_temperature,
                target_value_ambient_temperature,
                true, // mutex blocking in this test case
            );
        });
    });
}
