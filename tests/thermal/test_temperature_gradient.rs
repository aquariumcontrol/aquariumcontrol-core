/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Module for testing calculation of the temperature gradient of the aquarium water.

use aquarium_control::launch::channels::Channels;
use aquarium_control::sensors::sensor_manager::{SensorManagerMutexes, SensorManagerSignals};
use aquarium_control::thermal::temperature_gradient::{
    calculate_least_squares_gradient, TemperatureGradient,
};
use aquarium_control::thermal::temperature_gradient_channels::TemperatureGradientChannels;
use aquarium_control::thermal::temperature_gradient_config::TemperatureGradientConfig;
use aquarium_control::utilities::channel_content::InternalCommand;
use assert_float_eq::assert_float_absolute_eq;
use spin_sleep::SpinSleeper;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Synchronously executes the main task loop for testing.
///
/// # Arguments
/// * `temperature_gradient` - A parameter representing `temperature_gradient`.
/// * `mutex_sensor_manager_signals` - Shared mutex for consolidated sensor signals.
/// * `temperature_gradient_channels` - A parameter representing `temperature_gradient_channels`.
/// * `mutex_temperature_gradient` - A parameter representing `mutex_temperature_gradient`.
fn execute_blocking(
    temperature_gradient: &mut TemperatureGradient,
    mutex_sensor_manager_signals: Arc<Mutex<SensorManagerSignals>>,
    temperature_gradient_channels: &mut TemperatureGradientChannels,
    mutex_temperature_gradient: Arc<Mutex<f64>>,
) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            // blocking execution used for testing
            temperature_gradient
                .execute(
                    mutex_sensor_manager_signals,
                    temperature_gradient_channels,
                    mutex_temperature_gradient,
                )
                .await;
        });
}

// Check if the temperature gradient calculation works.
// Also, implicitly test if the execute function terminates after receiving Quit command.
#[test]
// Verifies that temperature gradient calculation.
pub fn test_temperature_gradient_calculation() {
    let spin_sleeper = SpinSleeper::default();

    let sampling_interval = 1;
    let sample_size = 3;

    let temperature_gradient_config = TemperatureGradientConfig {
        active: true,
        execute: true,
        sampling_interval,
        sample_size,
    };

    let mut temperature_gradient = TemperatureGradient::new(temperature_gradient_config).unwrap();

    let mut channels = Channels::new_for_test();

    // Mutexes needed for water temperature and temperature gradient
    let mutexes_sensor_manager = SensorManagerMutexes::new_for_test();
    let mutex_sensor_manager_signals_clone_for_test_environment =
        mutexes_sensor_manager.mutex_sensor_manager_signals.clone();
    let mutex_temperature_gradient = Arc::new(Mutex::new(0.0));
    let mutex_temperature_gradient_clone_for_test_environment = mutex_temperature_gradient.clone();

    {
        // scope to limit lifetime of lock
        let temperature_gradient_before_execution = mutex_temperature_gradient.lock().unwrap();
        assert_eq!(*temperature_gradient_before_execution, 0.0);
    }

    let mut signal_handler_channel_clone_for_test_environment = channels
        .signal_handler
        .tx_signal_handler_to_temperature_gradient
        .clone();

    // thread for test environment
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_sample_interval = Duration::from_secs(sampling_interval);
            // keep increasing value -> positive gradient
            for i in 1..=10 {
                {
                    // scope to limit lifetime of lock
                    let mut sensor_manager_signals =
                        mutex_sensor_manager_signals_clone_for_test_environment
                            .lock()
                            .unwrap();
                    sensor_manager_signals.water_temperature = i as f32;
                }
                spin_sleeper.sleep(sleep_duration_sample_interval);
                {
                    // scope to limit lifetime of lock
                    let temperature_gradient_during_execution =
                        mutex_temperature_gradient_clone_for_test_environment
                            .lock()
                            .unwrap();
                    if i > sample_size {
                        assert_float_absolute_eq!(
                            *temperature_gradient_during_execution,
                            1.0 / sampling_interval as f64,
                            0.1
                        );
                    }
                }
            }
            // keep value constant -> zero gradient
            for i in 1..10 {
                spin_sleeper.sleep(sleep_duration_sample_interval);
                {
                    // scope to limit lifetime of lock
                    let temperature_gradient_during_execution =
                        mutex_temperature_gradient_clone_for_test_environment
                            .lock()
                            .unwrap();
                    if i > sample_size {
                        assert_float_absolute_eq!(*temperature_gradient_during_execution, 0.0, 0.1);
                    }
                }
            }
            let _ = signal_handler_channel_clone_for_test_environment.send(InternalCommand::Quit);
        })
        .unwrap();

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            execute_blocking(
                &mut temperature_gradient,
                mutexes_sensor_manager.mutex_sensor_manager_signals,
                &mut channels.temperature_gradient,
                mutex_temperature_gradient,
            );
        })
        .unwrap();

    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");

    // receive acknowledgement from signal handler
    channels
        .signal_handler
        .rx_signal_handler_from_temperature_gradient
        .blocking_recv()
        .unwrap();

    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
#[ignore]
// This test case retrieves data from the last 24 hours from the production database
// and calculates the temperature gradient.
// The test case does not assert anything.
fn test_calculate_gradient_from_production_data() {
    use aquarium_control::database::sql_interface::SqlInterface;
    use aquarium_control::database::sql_interface_data::SqlInterfaceData;
    use aquarium_control::utilities::config::read_config_file;
    use std::collections::VecDeque;

    // --- Setup ---
    let config = read_config_file("config/aquarium_control.toml".to_string())
        .expect("Failed to read production config file.");

    let sql_interface =
        SqlInterface::new(config.sql_interface).expect("Failed to create SqlInterface.");

    let mut sql_interface_data = SqlInterfaceData::new(
        sql_interface.get_connection().unwrap(),
        sql_interface.get_connection().unwrap(),
        0,
    )
    .expect("Failed to create SqlInterfaceData.");

    // --- Get Data ---
    let temperature_data = sql_interface_data
        .get_water_temperature_last_24h()
        .expect("Failed to get water temperature data.");

    // --- Process Data ---
    let sample_size = 5;
    if temperature_data.len() < sample_size {
        println!(
            "Not enough data to calculate gradient. Need at least {} samples, but got {}.",
            sample_size,
            temperature_data.len()
        );
        return;
    }

    let mut samples: VecDeque<(f64, f64)> = VecDeque::with_capacity(sample_size);

    for (timestamp, temperature, surface_ventilation_status, heater_status) in temperature_data {
        // fill buffer with one sample at a time
        samples.push_back((timestamp, temperature));

        // limit the size of the buffer
        if samples.len() > sample_size {
            samples.pop_front();
        }

        // if buffer is filled with enough samples, calculate the gradient
        if samples.len() == sample_size {
            let gradient = calculate_least_squares_gradient(&samples);
            let last_timestamp = samples.back().unwrap().0;
            println!(
                "Timestamp: {}, Gradient: {:.6} °C/s = {:.3} °C/h, Surface Ventilation Status: {}, Heater Status: {}",
                last_timestamp, gradient, gradient * 3600.0, surface_ventilation_status, heater_status
            );
        }
    }
}
