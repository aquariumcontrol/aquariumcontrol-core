/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the central data collection, filtering, and logging thread.

use aquarium_control::database::sql_interface::SqlInterface;
use aquarium_control::database::sql_interface_data::SqlInterfaceData;
use aquarium_control::launch::channels::Channels;
use aquarium_control::launch::startup_error::StartupError;
use aquarium_control::mocks::mock_data_injection::tests::MockInjectorWithQuit;
use aquarium_control::recorder::data_injection::DataInjectionTrait;
use aquarium_control::recorder::data_logger::DataLogger;
use aquarium_control::recorder::data_logger_channels::DataLoggerChannels;
use aquarium_control::recorder::data_logger_config::DataLoggerConfig;
use aquarium_control::recorder::data_logger_mutexes::DataLoggerMutexes;
use aquarium_control::recorder::recorder_data_frame::RecorderDataFrame;
use aquarium_control::sensors::sensor_manager::SensorManagerSignals;
use aquarium_control::sensors::tank_level_switch::TankLevelSwitchSignals;
use aquarium_control::thermal::thermal_control_status::ThermalControlStatus;
use aquarium_control::utilities::channel_content::InternalCommand;
use aquarium_control::utilities::config::read_config_file;
use aquarium_control::utilities::signal_handler_channels::SignalHandlerChannels;
use aquarium_control::water::refill_status::RefillStatus;
use assert_float_eq::assert_float_absolute_eq;
use std::fs;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Helper function to execute the data logger task inside a test Tokio runtime.
///
/// # Arguments
/// * `data_logger` - A mutable reference to the `DataLogger` instance.
/// * `data_injection` - Data injection trait implementation.
/// * `sql_interface_data` - Database interface for recording data.
/// * `data_logger_channels` - A mutable reference to the `DataLoggerChannels` instance.
/// * `data_logger_mutexes` - Shared mutexes for sensor data.
fn execute_blocking<D: DataInjectionTrait + Send + 'static>(
    data_logger: &mut DataLogger,
    data_injection: D,
    sql_interface_data: SqlInterfaceData,
    data_logger_channels: &mut DataLoggerChannels,
    data_logger_mutexes: DataLoggerMutexes,
) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            // blocking execution used for testing
            data_logger
                .execute(
                    data_injection,
                    sql_interface_data,
                    data_logger_channels,
                    data_logger_mutexes,
                )
                .await;
        });
}

/// Prepares all necessary parts for a data logger test.
///
/// # Arguments
/// * `config` - A parameter representing `config`.
fn prepare_data_logger_test_rig(
    config: DataLoggerConfig,
) -> (
    DataLogger,
    DataLoggerMutexes,
    DataLoggerChannels,
    SignalHandlerChannels,
    Arc<Mutex<Option<RecorderDataFrame>>>,
) {
    let data_logger = DataLogger::new(config).unwrap();

    let heating_status = ThermalControlStatus {
        actuator_switched_on: true,
        actuator_ineffective: false,
        actuator_disabled: false,
        ..Default::default()
    };

    let ventilation_status = ThermalControlStatus {
        actuator_switched_on: false,
        actuator_ineffective: true,
        actuator_disabled: true,
        ..Default::default()
    };

    let mutexes = DataLoggerMutexes {
        mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals {
            water_temperature: 25.5,
            ph: 7.8,
            conductivity: 50000.0,
            ambient_temperature: 22.1,
            ambient_humidity: 65.2,
            instant_last_recording: Instant::now(),
        })),
        mutex_tank_level_switch_signals: Arc::new(Mutex::new(TankLevelSwitchSignals {
            tank_level_switch_position: true,
            tank_level_switch_position_stabilized: true,
            tank_level_switch_invalid: false,
        })),
        mutex_heating_status: Arc::new(Mutex::new(heating_status)),
        mutex_ventilation_status: Arc::new(Mutex::new(ventilation_status)),
        mutex_refill_status: Arc::new(Mutex::new(RefillStatus::default())),
        mutex_temperature_gradient: Arc::new(Mutex::new(0.123)),
    };

    let channels = Channels::new_for_test();

    let captured_data = Arc::new(Mutex::new(None));

    (
        data_logger,
        mutexes,
        channels.data_logger,
        channels.signal_handler,
        captured_data,
    )
}

#[test]
/// Verifies that `DataLogger::new()` successfully creates and resets all
/// specified output files when `output_to_disk` is enabled.
///
/// [itest->swreq~recorder_store_measurements~1]
fn test_new_success_with_file_creation() {
    // 1. Setup
    let global_config =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let config = global_config.data_logger;

    let output_file_name_water_temperature = config.output_file_name_water_temperature.clone();
    // 2. Execute
    let result = DataLogger::new(config);

    // 3. Assert
    assert!(result.is_ok(), "DataLogger::new should succeed");

    // Verify that one of the files was created and is empty
    let mut file = fs::File::open(output_file_name_water_temperature).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    assert!(contents.is_empty(), "File should be created and empty");
}
#[test]
/// Tests one full cycle of the `execute` loop, verifying that data is
/// correctly acquired from mutexes, filtered, and injected.
fn test_execute_data_aggregation_and_injection() {
    // 1. Setup
    let global_config =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let config = global_config.data_logger;

    let (
        mut data_logger,
        mutexes,
        mut datalogger_channels,
        mut signal_handler_channels,
        captured_data,
    ) = prepare_data_logger_test_rig(config);

    let mut tx_signal_handler_to_data_logger_clone = signal_handler_channels
        .tx_signal_handler_to_data_logger
        .clone();

    let mock_injector = MockInjectorWithQuit {
        injected_data: captured_data.clone(),
        tx_signal_handler_to_data_logger: signal_handler_channels.tx_signal_handler_to_data_logger,
    };

    // 2. Execute
    let logger_thread = std::thread::spawn(move || {
        let sql_interface = SqlInterface::new(global_config.sql_interface).unwrap();
        let database_connection_data_for_timestamp = sql_interface
            .get_connection()
            .map_err(|e| StartupError::Database {
                source: Box::new(e),
            })
            .unwrap();
        let database_connection_data_for_data = sql_interface
            .get_connection()
            .map_err(|e| StartupError::Database {
                source: Box::new(e),
            })
            .unwrap();
        let sql_interface_data = SqlInterfaceData::new(
            database_connection_data_for_timestamp,
            database_connection_data_for_data,
            0,
        )
        .unwrap();
        execute_blocking(
            &mut data_logger,
            mock_injector,
            sql_interface_data,
            &mut datalogger_channels,
            mutexes,
        );
    });

    let mock_signal_handler_thread = std::thread::spawn(move || {
        let _ = signal_handler_channels
            .rx_signal_handler_from_data_logger
            .blocking_recv()
            .unwrap();
        tx_signal_handler_to_data_logger_clone
            .send(InternalCommand::Terminate)
            .unwrap();
    });

    logger_thread.join().expect("DataLogger thread panicked");
    mock_signal_handler_thread
        .join()
        .expect("SignalHandler thread panicked");

    // 3. Assert
    let final_data = captured_data.lock().unwrap();
    assert!(final_data.is_some(), "Data should have been injected");

    let data_frame = final_data.as_ref().unwrap();
    assert_float_absolute_eq!(data_frame.water_temperature.unwrap(), 25.5, 0.001);
    assert_float_absolute_eq!(data_frame.ph.unwrap(), 7.8, 0.001);
    assert_eq!(data_frame.heater_status, Some(true));
    assert_eq!(data_frame.surface_ventilation_status, Some(false));
    assert_eq!(data_frame.tank_level_switch_invalid, Some(false));

    // Check if filtering logic was applied
    assert!(
        data_frame.water_temperature_filtered.is_some(),
        "Filtered water temperature should be calculated"
    );
}

#[test]
/// Verifies that the `execute` loop correctly writes sensor and state
/// data to files when `output_to_disk` is enabled.
fn test_execute_file_output() {
    // 1. Setup
    let global_config =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let mut config = global_config.data_logger;

    // Make all file paths unique for this test case to avoid concurrency issues.
    let unique_suffix = "_test_file_output";
    config.output_file_name_timestamp += unique_suffix;
    config.output_file_name_water_temperature += unique_suffix;
    config.output_file_name_water_temperature_filtered += unique_suffix;
    config.output_file_name_pH += unique_suffix;
    config.output_file_name_pH_filtered += unique_suffix;
    config.output_file_name_conductivity += unique_suffix;
    config.output_file_name_conductivity_filtered += unique_suffix;
    config.output_file_name_tank_level_switch_position += unique_suffix;
    config.output_file_name_tank_level_switch_invalid += unique_suffix;
    config.output_file_name_tank_level_switch_position_stabilized += unique_suffix;
    config.output_file_name_ventilation_is_on += unique_suffix;
    config.output_file_name_ventilation_actuator_ineffective += unique_suffix;
    config.output_file_name_ventilation_actuator_disabled += unique_suffix;
    config.output_file_name_heating_is_on += unique_suffix;
    config.output_file_name_heating_actuator_ineffective += unique_suffix;
    config.output_file_name_heating_actuator_disabled += unique_suffix;
    config.output_file_name_ambient_temperature += unique_suffix;
    config.output_file_name_humidity += unique_suffix;
    config.output_file_name_refill_in_progress += unique_suffix;
    config.output_file_name_refill_controller_state += unique_suffix;
    config.output_file_name_temperature_gradient += unique_suffix;
    config.output_file_name_heating_controller_state += unique_suffix;
    config.output_file_name_ventilation_controller_state += unique_suffix;

    let output_file_name_water_temperature = config.output_file_name_water_temperature.clone();
    let output_file_name_heating_is_on = config.output_file_name_heating_is_on.clone();
    let output_file_name_heating_actuator_ineffective =
        config.output_file_name_heating_actuator_ineffective.clone();
    let output_file_name_heating_actuator_disabled =
        config.output_file_name_heating_actuator_disabled.clone();
    let output_file_name_ventilation_actuator_ineffective = config
        .output_file_name_ventilation_actuator_ineffective
        .clone();
    let output_file_name_ventilation_actuator_disabled = config
        .output_file_name_ventilation_actuator_disabled
        .clone();
    let output_file_name_temperature_gradient =
        config.output_file_name_temperature_gradient.clone();
    let output_file_name_heating_controller_state =
        config.output_file_name_heating_controller_state.clone();
    let output_file_name_ventilation_controller_state =
        config.output_file_name_ventilation_controller_state.clone();

    let sql_interface = SqlInterface::new(global_config.sql_interface).unwrap();
    let database_connection_data_for_timestamp = sql_interface
        .get_connection()
        .map_err(|e| StartupError::Database {
            source: Box::new(e),
        })
        .unwrap();
    let database_connection_data_for_data = sql_interface
        .get_connection()
        .map_err(|e| StartupError::Database {
            source: Box::new(e),
        })
        .unwrap();
    let sql_interface_data = SqlInterfaceData::new(
        database_connection_data_for_timestamp,
        database_connection_data_for_data,
        0,
    )
    .unwrap();

    let (
        mut data_logger,
        mutexes,
        mut data_logger_channels,
        mut signal_handler_channels,
        captured_data,
    ) = prepare_data_logger_test_rig(config);

    let mut tx_signal_handler_to_data_logger_clone = signal_handler_channels
        .tx_signal_handler_to_data_logger
        .clone();

    let mock_injector = MockInjectorWithQuit {
        injected_data: captured_data,
        tx_signal_handler_to_data_logger: signal_handler_channels.tx_signal_handler_to_data_logger,
    };

    let mock_signal_handler_thread = std::thread::spawn(move || {
        let _ = signal_handler_channels
            .rx_signal_handler_from_data_logger
            .blocking_recv()
            .unwrap();
        tx_signal_handler_to_data_logger_clone
            .send(InternalCommand::Terminate)
            .unwrap();
    });

    // 2. Execute
    let logger_thread = std::thread::spawn(move || {
        execute_blocking(
            &mut data_logger,
            mock_injector,
            sql_interface_data,
            &mut data_logger_channels,
            mutexes,
        );
    });

    logger_thread.join().expect("DataLogger thread panicked");
    mock_signal_handler_thread
        .join()
        .expect("SignalHandler thread panicked");

    // 3. Assert
    // Check that the files were written with the correct content
    let temp_content = fs::read_to_string(output_file_name_water_temperature).unwrap();
    assert_eq!(temp_content, "25.5");

    let heat_content = fs::read_to_string(output_file_name_heating_is_on).unwrap();
    assert_eq!(heat_content, "ON");

    let content = fs::read_to_string(output_file_name_heating_actuator_ineffective).unwrap();
    assert_eq!(content, "FALSE");

    let content = fs::read_to_string(output_file_name_heating_actuator_disabled).unwrap();
    assert_eq!(content, "FALSE");

    let content = fs::read_to_string(output_file_name_ventilation_actuator_ineffective).unwrap();
    assert_eq!(content, "TRUE");

    let content = fs::read_to_string(output_file_name_ventilation_actuator_disabled).unwrap();
    assert_eq!(content, "TRUE");

    let content = fs::read_to_string(output_file_name_temperature_gradient).unwrap();
    assert_eq!(content, "0.123");

    let heating_status = ThermalControlStatus {
        actuator_switched_on: true,
        actuator_ineffective: false,
        actuator_disabled: false,
        ..Default::default()
    };

    let content = fs::read_to_string(output_file_name_heating_controller_state).unwrap();
    assert_eq!(content, heating_status.description());

    let ventilation_status = ThermalControlStatus {
        actuator_switched_on: false,
        actuator_ineffective: true,
        actuator_disabled: true,
        ..Default::default()
    };

    let content = fs::read_to_string(output_file_name_ventilation_controller_state).unwrap();
    assert_eq!(content, ventilation_status.description());
}
