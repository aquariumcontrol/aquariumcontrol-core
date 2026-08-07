/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests for the aquarium's heating system.
use aquarium_control::database::sql_interface::SqlInterface;
use aquarium_control::database::sql_interface_heating_stats::{
    HeatingStatsEntry, SqlInterfaceHeatingStats,
};
use aquarium_control::database::sql_test_query_strings::SQL_TABLE_HEATING_STATS;
use aquarium_control::database::thermal_set_value_updater_trait::ThermalSetValueUpdaterTrait;
use aquarium_control::launch::channels::Channels;
use aquarium_control::mocks::mock_heating::tests::MockSqlInterfaceHeatingSetVals;
use aquarium_control::mocks::mock_heating_stats::tests::{
    HeatingStatsMockDataTransfer, SqlInterfaceHeatingMockMidnightCalculator,
};
use aquarium_control::mocks::mock_relay_manager::tests::mock_relay_manager;
use aquarium_control::mocks::mock_schedule_check::tests::mock_schedule_check;
use aquarium_control::sensors::sensor_manager::SensorManagerSignals;
use aquarium_control::sensors::tank_level_switch::TankLevelSwitchSignals;
use aquarium_control::thermal::heating::{Heating, CYCLE_TIME_HEATING_MILLIS};
use aquarium_control::thermal::heating_channels::HeatingChannels;
use aquarium_control::thermal::heating_mutexes::HeatingMutexes;
use aquarium_control::thermal::heating_stats_data_transfer_trait::HeatingStatsDataTransferTrait;
use aquarium_control::thermal::thermal_control_status::ThermalControlStatus;
use aquarium_control::utilities::channel_content::{
    ActuatorState, AquariumDevice, InternalCommand,
};
use aquarium_control::utilities::config::{read_config_file_with_test_database, ConfigData};
use aquarium_control::utilities::sawtooth_profile::SawToothProfileConfig;

use all_asserts::{assert_ge, assert_le};
use assert_float_eq::assert_float_absolute_eq;
use chrono::Local;
use spin_sleep::SpinSleeper;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Helper function to execute the heating task inside a test Tokio runtime.
///
/// # Arguments
/// * `heating` - A mutable reference to the `Heating` instance.
/// * `mutex_device_scheduler_heating` - Mutex coordinating access to the device scheduler.
/// * `heating_channels` - A mutable reference to the `HeatingChannels` instance.
/// * `heating_stats_transfer` - A mutable reference to the heating stats transfer object.
/// * `heating_set_val_updater` - A mutable reference to the set value updater.
/// * `heating_mutexes` - Shared mutexes for heating conditions.
/// * `sql_interface_heating` - Database interface for heating stats.
///
/// [itest->swreq~thermal_heater_heating~1]
fn execute_blocking(
    heating: &mut Heating,
    mutex_device_scheduler_heating: Arc<tokio::sync::Mutex<i32>>,
    heating_channels: &mut HeatingChannels,
    heating_stats_transfer: &mut impl HeatingStatsDataTransferTrait,
    heating_set_val_updater: &mut impl ThermalSetValueUpdaterTrait,
    heating_mutexes: HeatingMutexes,
    sql_interface_heating: SqlInterfaceHeatingStats,
) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            // blocking execution used for testing
            let is_multi_thread_test = tokio::runtime::Handle::try_current()
                .map(|h| {
                    matches!(
                        h.runtime_flavor(),
                        tokio::runtime::RuntimeFlavor::MultiThread
                    )
                })
                .unwrap_or(false);

            heating
                .execute(
                    mutex_device_scheduler_heating,
                    heating_channels,
                    heating_stats_transfer,
                    heating_set_val_updater,
                    heating_mutexes,
                    sql_interface_heating,
                    is_multi_thread_test,
                )
                .await;
        });
}

// Prepares a `Heating` control instance with a mock midnight calculator and a clean test database.
//
// This helper function is designed exclusively for testing the `Heating` module. It sets up
// a controlled environment by:
// 1.  Loading a generic test configuration and overriding specific settings (e.g., `stop_on_refill_error_delay`).
// 2.  Optionally, applying a custom `SawToothProfileConfig` for precise control over heating patterns.
// 3.  Connecting to and **truncating a dedicated test database** to ensure a clean state for heating statistics.
// 4.  Injecting a **`SqlInterfaceHeatingMockMidnightCalculator`** to return a predefined `seconds_to_midnight`,
//     allowing deterministic testing of daily statistic transfers without real-time dependency.
//
// # Arguments
// * `seconds_to_midnight` - The fixed number of seconds (as `i64`) the mock midnight calculator will return.
//   This directly controls when the `Heating` module attempts to transfer daily statistics.
// * `test_db_number` - A unique `u32` identifier (e.g., `7`, `8`, `9`) used to select a specific
//   test database (`aquarium_test_XX`) from the configuration.
// * `saw_tooth_profile_config_opt` - An `Option<SawToothProfileConfig>`. If `Some`, this custom
//   sawtooth profile configuration overrides the one loaded from the TOML file for the `Heating` module.
//
// # Returns
// A tuple `(ConfigData, Heating)`:
// - The `ConfigData` struct, containing the loaded and modified configuration.
// - The fully initialized `Heating` struct, ready for test execution.
//
// # Panics
// This function will panic if:
// - `read_config_file_with_test_database` fails (e.g., file not found, invalid DB number).
// - It fails to connect to the SQL database.
// - It fails to truncate the `heatingstats` table in the database.
// - It encounters any issue during the initialization of the `SqlInterfaceHeating` or `Heating` structs.
/// Performs the `prepare_heating_tests_mock_midnight_calculator` operation.
///
/// # Arguments
/// * `seconds_to_midnight` - A parameter representing `seconds_to_midnight`.
/// * `test_db_number` - A parameter representing `test_db_number`.
/// * `saw_tooth_profile_config_opt` - A parameter representing `saw_tooth_profile_config_opt`.
pub fn prepare_heating_tests_mock_midnight_calculator(
    seconds_to_midnight: i64,
    test_db_number: u32,
    saw_tooth_profile_config_opt: Option<SawToothProfileConfig>,
) -> (ConfigData, Heating, SqlInterface, SqlInterfaceHeatingStats) {
    // prepare struct Heating for further tests
    let mut config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        test_db_number,
    );
    config.heating.stop_on_refill_error_delay = 2;

    if let Some(saw_tooth_profile_config) = saw_tooth_profile_config_opt {
        config.heating.saw_tooth_profile_config = saw_tooth_profile_config;
    }

    println!("Testing with database {}", config.sql_interface.db_name);
    let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };

    // empty the heating stats table
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_STATS.to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let sql_interface_heating_midnight_calculator = Box::new(
        SqlInterfaceHeatingMockMidnightCalculator::new(seconds_to_midnight),
    );
    let sql_interface_heating_stats = SqlInterfaceHeatingStats::new(
        sql_interface.get_connection().unwrap(),
        config.sql_interface.max_rows_heating_stats,
        sql_interface_heating_midnight_calculator,
    )
    .unwrap();

    println!(
            "prepare_heating_tests_mock_midnight_calculator: SwitchOff temperature = {}, SwitchOn temperature = {}",
            config.heating.switch_on_temperature, config.heating.switch_off_temperature
        );
    let heating_config = config.heating.clone();
    (
        config,
        Heating::new(heating_config, Duration::from_secs(1000)),
        sql_interface,
        sql_interface_heating_stats,
    )
}

#[test]
// [itest->swreq~thermal_heater_heating~1]
// Check if the heater remains switched off with a high temperature.
// Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
// Test case uses test database #11.
pub fn test_heating_with_measured_temperature_high() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 11, None);

    // replacement values are used to initialize the mutexes
    config.sensor_manager.replacement_value_water_temperature = 30.0;
    config.sensor_manager.replacement_value_ambient_temperature = 24.0;

    let mut channels = Channels::new_for_test();

    // Mutex for tank level switch signals
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
            assert_eq!(actuation_events.len(), 1);
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOff(AquariumDevice::Heater)
            );
            mock_actuator_states.check_terminal_condition_heating();
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_ten_seconds = Duration::from_secs(10);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration_ten_seconds);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // make sure all mock threads are running when instantiating the test object
    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_heating_to_schedule_check_for_test_case_finish =
                channels.heating.tx_heating_to_schedule_check.clone();
            let mut tx_heating_to_relay_manager_for_test_case_finish =
                channels.heating.tx_heating_to_relay_manager.clone();

            let mut heating_stats_transfer =
                HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
            let mut heating_set_value_updater =
                MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

            let heating_mutexes = HeatingMutexes {
                mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                    &config.sensor_manager,
                ))),
                mutex_tank_level_switch_signals,
                mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut heating,
                mutex_device_scheduler_heating.clone(),
                &mut channels.heating,
                &mut heating_stats_transfer,
                &mut heating_set_value_updater,
                heating_mutexes,
                sql_interface_heating_stats,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

            println!("* [Heating] checking reaction to high temperature succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// [itest->swreq~thermal_heater_heating~1]
// Check if the heater is switched on and is not switched off with low temperature.
// Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
// Check if the heater is switched off after sending Quit command and before sending Terminate command.
// Test case uses test database #12.
pub fn test_heating_with_measured_temperature_low_without_mutex_blocked() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 12, None);

    // replacement values are used to initialize the mutexes
    config.sensor_manager.replacement_value_water_temperature = 20.0;
    config.sensor_manager.replacement_value_ambient_temperature = 24.0;

    let mut channels = Channels::new_for_test();

    // Mutex for tank level switch signals
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
            assert_eq!(actuation_events.len(), 2);
            let last_actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                last_actuation_event.command,
                InternalCommand::SwitchOff(AquariumDevice::Heater)
            );
            let first_actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                first_actuation_event.command,
                InternalCommand::SwitchOn(AquariumDevice::Heater)
            );
            mock_actuator_states.check_terminal_condition_heating();
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_ten_seconds = Duration::from_secs(10);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration_ten_seconds);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // make sure all mock threads are running when instantiating the test object
    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_heating_to_schedule_check_for_test_case_finish =
                channels.heating.tx_heating_to_schedule_check.clone();
            let mut tx_heating_to_relay_manager_for_test_case_finish =
                channels.heating.tx_heating_to_relay_manager.clone();

            let mut heating_stats_transfer =
                HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
            let mut heating_set_value_updater =
                MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

            let heating_mutexes = HeatingMutexes {
                mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                    &config.sensor_manager,
                ))),
                mutex_tank_level_switch_signals,
                mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut heating,
                mutex_device_scheduler_heating.clone(),
                &mut channels.heating,
                &mut heating_stats_transfer,
                &mut heating_set_value_updater,
                heating_mutexes,
                sql_interface_heating_stats,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

            println!("* [Heating] checking reaction to low temperature succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// [itest->swreq~thermal_heater_heating~1]
// Check if the heater is switched on and off during two cycles of the saw tooth profile.
// The duration of the heater being switched on shall equal the duration of the heater switched off (1% tolerance).
// Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
// Test case uses test database #13.
pub fn test_heating_with_measured_temperature_50_percent() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(
            3600,
            13,
            Some(SawToothProfileConfig::new(6, 1, 6, 1)),
        );

    // replacement values are used to initialize the mutexes
    config.sensor_manager.replacement_value_water_temperature = 25.0;
    config.sensor_manager.replacement_value_ambient_temperature = 24.0;

    let mut channels = Channels::new_for_test();

    // Mutex for tank level switch signals
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
            println!(
                "Mock relay manager received {} commands:",
                actuation_events.len()
            );
            for actuation_event in actuation_events.clone() {
                println!("{}", actuation_event);
            }
            // (first and) last actuation events need to be ignored
            let _last_actuation_event = actuation_events.pop().unwrap();
            let second_last_actuation_event = actuation_events.pop().unwrap();
            let third_last_actuation_event = actuation_events.pop().unwrap();
            let fourth_last_actuation_event = actuation_events.pop().unwrap();
            let fifth_last_actuation_event = actuation_events.pop().unwrap();
            let sixth_last_actuation_event = actuation_events.pop().unwrap();
            let seventh_last_actuation_event = actuation_events.pop().unwrap();

            let mut min_actuation_duration: u128 = 100000;
            let mut max_actuation_duration: u128 = 0;

            let second_last_actuation_duration = second_last_actuation_event
                .time
                .duration_since(third_last_actuation_event.time)
                .as_millis();

            println!(
                "second_last_actuation_duration={}",
                second_last_actuation_duration
            );

            min_actuation_duration = min_actuation_duration.min(second_last_actuation_duration);
            max_actuation_duration = max_actuation_duration.max(second_last_actuation_duration);

            let third_last_actuation_duration = third_last_actuation_event
                .time
                .duration_since(fourth_last_actuation_event.time)
                .as_millis();

            println!(
                "third_last_actuation_duration={}",
                third_last_actuation_duration
            );

            min_actuation_duration = min_actuation_duration.min(third_last_actuation_duration);
            max_actuation_duration = max_actuation_duration.max(third_last_actuation_duration);

            let fourth_last_actuation_duration = fourth_last_actuation_event
                .time
                .duration_since(fifth_last_actuation_event.time)
                .as_millis();

            println!(
                "fourth_last_actuation_duration={}",
                fourth_last_actuation_duration
            );

            min_actuation_duration = min_actuation_duration.min(fourth_last_actuation_duration);
            max_actuation_duration = max_actuation_duration.max(fourth_last_actuation_duration);

            let fifth_last_actuation_duration = fifth_last_actuation_event
                .time
                .duration_since(sixth_last_actuation_event.time)
                .as_millis();

            println!(
                "fifth_last_actuation_duration={}",
                fifth_last_actuation_duration
            );

            min_actuation_duration = min_actuation_duration.min(fifth_last_actuation_duration);
            max_actuation_duration = max_actuation_duration.max(fifth_last_actuation_duration);

            let sixth_last_actuation_duration = sixth_last_actuation_event
                .time
                .duration_since(seventh_last_actuation_event.time)
                .as_millis();

            println!(
                "sixth_last_actuation_duration={}",
                sixth_last_actuation_duration
            );

            min_actuation_duration = min_actuation_duration.min(sixth_last_actuation_duration);
            max_actuation_duration = max_actuation_duration.max(sixth_last_actuation_duration);

            let delta_min_max_duration = max_actuation_duration - min_actuation_duration;

            assert_le!(second_last_actuation_duration, 6600);
            assert_ge!(second_last_actuation_duration, 6400);
            assert_le!(third_last_actuation_duration, 6600);
            assert_ge!(third_last_actuation_duration, 6400);
            assert_le!(fourth_last_actuation_duration, 6600);
            assert_ge!(fourth_last_actuation_duration, 6400);
            assert_le!(fifth_last_actuation_duration, 6600);
            assert_ge!(fifth_last_actuation_duration, 6400);
            assert_le!(sixth_last_actuation_duration, 6600);
            assert_ge!(sixth_last_actuation_duration, 6400);

            println!("delta_min_max_duration={}", delta_min_max_duration);
            assert_le!(delta_min_max_duration, 70);
            mock_actuator_states.check_terminal_condition_heating();
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration = Duration::from_secs(59);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // make sure all mock threads are running when instantiating the test object
    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_heating_to_schedule_check_for_test_case_finish =
                channels.heating.tx_heating_to_schedule_check.clone();
            let mut tx_heating_to_relay_manager_for_test_case_finish =
                channels.heating.tx_heating_to_relay_manager.clone();

            let mut heating_stats_transfer =
                HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
            let mut heating_set_value_updater =
                MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

            let heating_mutexes = HeatingMutexes {
                mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                    &config.sensor_manager,
                ))),
                mutex_tank_level_switch_signals,
                mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut heating,
                mutex_device_scheduler_heating.clone(),
                &mut channels.heating,
                &mut heating_stats_transfer,
                &mut heating_set_value_updater,
                heating_mutexes,
                sql_interface_heating_stats,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

            println!("* [Heating] checking reaction to medium temperature succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// [itest->swreq~thermal_heater_heating~1]
// Check if the heater is switched on for most of the time when a measured temperature is low.
// Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
// Test case uses test database #13.
pub fn test_heating_with_measured_temperature_low_inside_interval() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(
            3600,
            13,
            Some(SawToothProfileConfig::new(6, 1, 6, 1)),
        );

    // replacement values are used to initialize the mutexes
    config.sensor_manager.replacement_value_water_temperature = 22.5;
    config.sensor_manager.replacement_value_ambient_temperature = 24.0;

    let mut channels = Channels::new_for_test();

    // Mutex for tank level switch signals
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
            println!(
                "Mock relay manager received {} commands:",
                actuation_events.len()
            );
            for actuation_event in actuation_events.clone() {
                println!("{}", actuation_event);
            }
            // (first and) last actuation events need to be ignored
            let _last_actuation_event = actuation_events.pop().unwrap();
            let second_last_actuation_event = actuation_events.pop().unwrap();
            let third_last_actuation_event = actuation_events.pop().unwrap();
            let fourth_last_actuation_event = actuation_events.pop().unwrap();
            let fifth_last_actuation_event = actuation_events.pop().unwrap();
            let sixth_last_actuation_event = actuation_events.pop().unwrap();
            let seventh_last_actuation_event = actuation_events.pop().unwrap();

            let second_last_actuation_duration = second_last_actuation_event
                .time
                .duration_since(third_last_actuation_event.time)
                .as_millis();

            println!(
                "second_last_actuation_duration={}, Command={}",
                second_last_actuation_duration, second_last_actuation_event.command
            );

            let third_last_actuation_duration = third_last_actuation_event
                .time
                .duration_since(fourth_last_actuation_event.time)
                .as_millis();

            println!(
                "third_last_actuation_duration={}, Command={}",
                third_last_actuation_duration, second_last_actuation_event.command
            );

            let fourth_last_actuation_duration = fourth_last_actuation_event
                .time
                .duration_since(fifth_last_actuation_event.time)
                .as_millis();

            println!(
                "fourth_last_actuation_duration={}, Command={}",
                fourth_last_actuation_duration, second_last_actuation_event.command
            );

            let fifth_last_actuation_duration = fifth_last_actuation_event
                .time
                .duration_since(sixth_last_actuation_event.time)
                .as_millis();

            println!(
                "fifth_last_actuation_duration={}, Command={}",
                fifth_last_actuation_duration, second_last_actuation_event.command
            );

            let sixth_last_actuation_duration = sixth_last_actuation_event
                .time
                .duration_since(seventh_last_actuation_event.time)
                .as_millis();

            println!(
                "sixth_last_actuation_duration={}, Command={}",
                sixth_last_actuation_duration, second_last_actuation_event.command
            );

            assert_le!(second_last_actuation_duration, 1600);
            assert_ge!(second_last_actuation_duration, 1400);
            assert_le!(third_last_actuation_duration, 11650);
            assert_ge!(third_last_actuation_duration, 11450);
            assert_le!(fourth_last_actuation_duration, 1600);
            assert_ge!(fourth_last_actuation_duration, 1400);
            assert_le!(fifth_last_actuation_duration, 11700);
            assert_ge!(fifth_last_actuation_duration, 11450);
            assert_le!(sixth_last_actuation_duration, 1600);
            assert_ge!(sixth_last_actuation_duration, 1400);

            mock_actuator_states.check_terminal_condition_heating();
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration = Duration::from_secs(59);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // make sure all mock threads are running when instantiating the test object
    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_heating_to_schedule_check_for_test_case_finish =
                channels.heating.tx_heating_to_schedule_check.clone();
            let mut tx_heating_to_relay_manager_for_test_case_finish =
                channels.heating.tx_heating_to_relay_manager.clone();

            let mut heating_stats_transfer =
                HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
            let mut heating_set_value_updater =
                MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

            let heating_mutexes = HeatingMutexes {
                mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                    &config.sensor_manager,
                ))),
                mutex_tank_level_switch_signals,
                mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut heating,
                mutex_device_scheduler_heating.clone(),
                &mut channels.heating,
                &mut heating_stats_transfer,
                &mut heating_set_value_updater,
                heating_mutexes,
                sql_interface_heating_stats,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

            println!("* [Heating] checking reaction to medium temperature succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// [itest->swreq~thermal_heater_heating~1]
// Check if the heater is switched off most of the time when heater measured temperature is high.
// Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
// Test case uses test database #13.
pub fn test_heating_with_measured_temperature_high_inside_interval() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(
            3600,
            13,
            Some(SawToothProfileConfig::new(6, 1, 6, 1)),
        );

    // replacement values are used to initialize the mutexes
    config.sensor_manager.replacement_value_water_temperature = 27.5;
    config.sensor_manager.replacement_value_ambient_temperature = 24.0;

    let mut channels = Channels::new_for_test();

    // Mutex for tank level switch signals
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
            println!(
                "Mock relay manager received {} commands:",
                actuation_events.len()
            );
            for actuation_event in actuation_events.clone() {
                println!("{}", actuation_event);
            }
            // (first and) last actuation events need to be ignored
            let _last_actuation_event = actuation_events.pop().unwrap();
            let second_last_actuation_event = actuation_events.pop().unwrap();
            let third_last_actuation_event = actuation_events.pop().unwrap();
            let fourth_last_actuation_event = actuation_events.pop().unwrap();
            let fifth_last_actuation_event = actuation_events.pop().unwrap();
            let sixth_last_actuation_event = actuation_events.pop().unwrap();
            let seventh_last_actuation_event = actuation_events.pop().unwrap();

            let second_last_actuation_duration = second_last_actuation_event
                .time
                .duration_since(third_last_actuation_event.time)
                .as_millis();

            println!(
                "second_last_actuation_duration={}, Command={}",
                second_last_actuation_duration, second_last_actuation_event.command
            );

            let third_last_actuation_duration = third_last_actuation_event
                .time
                .duration_since(fourth_last_actuation_event.time)
                .as_millis();

            println!(
                "third_last_actuation_duration={}, Command={}",
                third_last_actuation_duration, second_last_actuation_event.command
            );

            let fourth_last_actuation_duration = fourth_last_actuation_event
                .time
                .duration_since(fifth_last_actuation_event.time)
                .as_millis();

            println!(
                "fourth_last_actuation_duration={}, Command={}",
                fourth_last_actuation_duration, second_last_actuation_event.command
            );

            let fifth_last_actuation_duration = fifth_last_actuation_event
                .time
                .duration_since(sixth_last_actuation_event.time)
                .as_millis();

            println!(
                "fifth_last_actuation_duration={}, Command={}",
                fifth_last_actuation_duration, second_last_actuation_event.command
            );

            let sixth_last_actuation_duration = sixth_last_actuation_event
                .time
                .duration_since(seventh_last_actuation_event.time)
                .as_millis();

            println!(
                "sixth_last_actuation_duration={}, Command={}",
                sixth_last_actuation_duration, second_last_actuation_event.command
            );

            /*
            assert_le!(second_last_actuation_duration, 1600);
            assert_ge!(second_last_actuation_duration, 1400);
            assert_le!(third_last_actuation_duration, 11650);
            assert_ge!(third_last_actuation_duration, 11450);
            assert_le!(fourth_last_actuation_duration, 1600);
            assert_ge!(fourth_last_actuation_duration, 1400);
            assert_le!(fifth_last_actuation_duration, 11650);
            assert_ge!(fifth_last_actuation_duration, 11450);
            assert_le!(sixth_last_actuation_duration, 1600);
            assert_ge!(sixth_last_actuation_duration, 1400);
            */

            mock_actuator_states.check_terminal_condition_heating();
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration = Duration::from_secs(59);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // make sure all mock threads are running when instantiating the test object
    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_heating_to_schedule_check_for_test_case_finish =
                channels.heating.tx_heating_to_schedule_check.clone();
            let mut tx_heating_to_relay_manager_for_test_case_finish =
                channels.heating.tx_heating_to_relay_manager.clone();

            let mut heating_stats_transfer =
                HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
            let mut heating_set_value_updater =
                MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

            let heating_mutexes = HeatingMutexes {
                mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                    &config.sensor_manager,
                ))),
                mutex_tank_level_switch_signals,
                mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut heating,
                mutex_device_scheduler_heating.clone(),
                &mut channels.heating,
                &mut heating_stats_transfer,
                &mut heating_set_value_updater,
                heating_mutexes,
                sql_interface_heating_stats,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

            println!("* [Heating] checking reaction to medium temperature succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// [itest->swreq~thermal_heater_heating~1]
// Test case checks if the Quit command is correctly processed
// - application shall switch off the heater before exiting
// The test case uses test database #14.
pub fn test_heating_with_pending_quit_terminate_command() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3, 14, None);

    // replacement values are used to initialize the mutexes
    config.sensor_manager.replacement_value_water_temperature = 25.0;
    config.sensor_manager.replacement_value_ambient_temperature = 24.0;

    let mut channels = Channels::new_for_test();

    let start_time = Instant::now();

    // Mutex for tank level switch signals
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
            println!(
                "Mock relay manager received {} commands.",
                actuation_events.len()
            );
            assert_eq!(actuation_events.len(), 1);
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOff(AquariumDevice::Heater)
            );
            mock_actuator_states.check_terminal_condition_heating();
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater = MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(&config.sensor_manager))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(&mut heating,
                                 mutex_device_scheduler_heating.clone(),
                                 &mut channels.heating,
                                 &mut heating_stats_transfer,
                                 &mut heating_set_value_updater,
                                 heating_mutexes,
                                 sql_interface_heating_stats
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                // check if heater actuation has taken place
                let mutex_data = mutex_device_scheduler_heating.blocking_lock();
                assert_eq!(*mutex_data, 1);
                let finish_time = Instant::now();
                let execution_duration = finish_time.duration_since(start_time);
                assert_le!(execution_duration.as_millis(), 650);
                println!(
                    "* [Heating] checking heating execution with pending quit/terminate command (time limit) succeeded."
                );
            })
            .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// [itest->swreq~thermal_midnight_calculator~1]
// [itest->swreq~thermal_connect_heating_stats~1]
// Test case checks if waiting for midnight and statistical calculations are computed correctly.
// Test case uses test database #15.
pub fn test_heating_wait_for_midnight_calc_stats_heater_on() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let seconds_to_midnight: i64 = 3;
    let reference_stats_update_count =
        seconds_to_midnight * 1000 / CYCLE_TIME_HEATING_MILLIS as i64;
    let (mut config, mut heating, sql_interface, mut sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(seconds_to_midnight, 15, None);

    // replacement values are used to initialize the mutexes
    config.sensor_manager.replacement_value_water_temperature = 20.0;
    config.sensor_manager.replacement_value_ambient_temperature = 24.0;

    let mut channels = Channels::new_for_test();

    // Mutex for tank level switch signals
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // get initial data for calculation of reference value
    let initial_heating_stats_entry = sql_interface_heating_stats
        .get_single_heating_stats_from_database()
        .unwrap_or_else(|_| {
            println!("Database responded with empty response - using default value");
            HeatingStatsEntry::new_for_test()
        });

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration = Duration::from_millis(5150);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater = MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(&config.sensor_manager))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(&mut heating,
                                 mutex_device_scheduler_heating.clone(),
                                 &mut channels.heating,
                                 &mut heating_stats_transfer,
                                 &mut heating_set_value_updater,
                                 heating_mutexes,
                                 sql_interface_heating_stats
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!(
                    "Initial heating stats: {}", initial_heating_stats_entry
                );
                println!(
                    "test_heating_wait_for_midnight_calc_stats_heater_on (previous): {}",
                    heating_stats_transfer.heating_stats_entry_previous,
                );
                assert_float_absolute_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .heating_control_runtime
                        - initial_heating_stats_entry.heating_control_runtime,
                    3.0,
                    0.1
                );
                assert_float_absolute_eq!(
                    heating_stats_transfer.heating_stats_entry_previous.energy,
                    0.1671666,
                    0.01
                );
                assert_float_absolute_eq!(
                    24.0,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_value,
                    0.001
                );
                assert_float_absolute_eq!(
                    20.0,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_value,
                    0.001
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_counter,
                    reference_stats_update_count + 1
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_counter,
                    reference_stats_update_count + 1
                );
                println!(
                    "test_heating_wait_for_midnight_calc_stats_heater_on (current): {}",
                    heating_stats_transfer.heating_stats_entry,
                );
                assert_float_absolute_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .heating_control_runtime
                        - initial_heating_stats_entry.heating_control_runtime,
                    2.0,
                    0.6
                );
                assert_float_absolute_eq!(0.1119444, heating_stats_transfer.heating_stats_entry.energy, 0.05);
                assert_float_absolute_eq!(24.0,
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_value,
                    0.001
                );
                assert_float_absolute_eq!(
                    20.0,
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_value,
                    0.001
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_counter,
                    4
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_counter,
                    4
                );
                println!("* [Heating] checking heating waiting for midnight and stats calculation with heater on succeeded.");
            })
            .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// [itest->swreq~thermal_midnight_calculator~1]
// [itest->swreq~thermal_connect_heating_stats~1]
// Test case checks if waiting for midnight and statistical calculations are computed correctly.
// Test case uses test database #16.
pub fn test_heating_wait_for_midnight_calc_stats_heater_off() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();
    let seconds_to_midnight: i64 = 3;
    let reference_stats_update_count =
        seconds_to_midnight * 1000 / CYCLE_TIME_HEATING_MILLIS as i64;

    let (mut config, mut heating, sql_interface, mut sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(seconds_to_midnight, 16, None);

    // replacement values are used to initialize the mutexes
    config.sensor_manager.replacement_value_water_temperature = 30.0;
    config.sensor_manager.replacement_value_ambient_temperature = 24.0;

    let mut channels = Channels::new_for_test();

    // get initial data for calculation of reference value
    let initial_heating_stats_entry = sql_interface_heating_stats
        .get_single_heating_stats_from_database()
        .unwrap_or_else(|_| {
            // database responded with an empty response - let's create an empty entry
            HeatingStatsEntry {
                date: Local::now().date_naive(),
                energy: 0.0,
                ambient_temperature_average_value: 0.0,
                ambient_temperature_average_counter: 0,
                water_temperature_average_value: 0.0,
                water_temperature_average_counter: 0,
                heating_control_runtime: 0.0,
            }
        });

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    // Mutex for tank level switch signals
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration = Duration::from_millis(5250);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_heating_to_schedule_check_for_test_case_finish =
                    channels.heating.tx_heating_to_schedule_check.clone();
                let mut tx_heating_to_relay_manager_for_test_case_finish =
                    channels.heating.tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer =
                    HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
                let mut heating_set_value_updater = MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

                let heating_mutexes = HeatingMutexes {
                    mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(&config.sensor_manager))),
                    mutex_tank_level_switch_signals,
                    mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                execute_blocking(&mut heating,
                                 mutex_device_scheduler_heating.clone(),
                                 &mut channels.heating,
                                 &mut heating_stats_transfer,
                                 &mut heating_set_value_updater,
                                 heating_mutexes,
                                 sql_interface_heating_stats
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!(
                    "test_heating_wait_for_midnight_calc_stats_heater_on (previous): {}",
                    heating_stats_transfer.heating_stats_entry_previous,
                );
                assert_float_absolute_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .heating_control_runtime
                        - initial_heating_stats_entry.heating_control_runtime,
                    3.02,
                    0.1
                );
                assert_float_absolute_eq!(
                    0.0,
                    heating_stats_transfer.heating_stats_entry_previous.energy,
                    0.001
                );
                assert_float_absolute_eq!(
                    24.0,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_value,
                    0.001
                );
                assert_float_absolute_eq!(
                    30.0,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_value,
                    0.001
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_counter
                        - initial_heating_stats_entry.ambient_temperature_average_counter,
                    reference_stats_update_count + 1
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_counter
                        - initial_heating_stats_entry.water_temperature_average_counter,
                    reference_stats_update_count + 1
                );
                println!(
                    "test_heating_wait_for_midnight_calc_stats_heater_off (current): {}",
                    heating_stats_transfer.heating_stats_entry,
                );
                assert_float_absolute_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .heating_control_runtime
                        - initial_heating_stats_entry.heating_control_runtime,
                    2.0,
                    0.6
                );
                assert_float_absolute_eq!(0.0, heating_stats_transfer.heating_stats_entry.energy, 0.003);
                assert_float_absolute_eq!(
                    24.0,
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_value,
                    0.001
                );
                assert_float_absolute_eq!(
                    30.0,
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_value,
                    0.001
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_counter,
                    4
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_counter,
                    4
                );
                println!("* [Heating] checking heating waiting for midnight and stats calculation with heater on succeeded.");
            })
            .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// Test case checks if a negative schedule checker result is processed correctly.
// Test case uses test database #17.
// [itest->swreq~thermal_observe_schedule_heater~1]
pub fn test_heating_blocked_by_schedule() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 17, None);

    // replacement values are used to initialize the mutexes
    config.sensor_manager.replacement_value_water_temperature = 20.0;
    config.sensor_manager.replacement_value_ambient_temperature = 24.0;

    let mut channels = Channels::new_for_test();

    // Mutex for tank level switch signals
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                false,
            );
        })
        .unwrap();

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
            assert_eq!(actuation_events.len(), 1);
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOff(AquariumDevice::Heater)
            );
            mock_actuator_states.check_terminal_condition_heating();
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_ten_seconds = Duration::from_secs(10);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration_ten_seconds);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // make sure all mock threads are running when instantiating the test object
    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_heating_to_schedule_check_for_test_case_finish =
                channels.heating.tx_heating_to_schedule_check.clone();
            let mut tx_heating_to_relay_manager_for_test_case_finish =
                channels.heating.tx_heating_to_relay_manager.clone();

            let mut heating_stats_transfer =
                HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
            let mut heating_set_value_updater =
                MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

            let heating_mutexes = HeatingMutexes {
                mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                    &config.sensor_manager,
                ))),
                mutex_tank_level_switch_signals,
                mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut heating,
                mutex_device_scheduler_heating.clone(),
                &mut channels.heating,
                &mut heating_stats_transfer,
                &mut heating_set_value_updater,
                heating_mutexes,
                sql_interface_heating_stats,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
            println!("* [Heating] checking if schedule checker can block actuation.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// Test case checks if component protection is executed.
// Test case uses test database #18.
// [itest->swreq~thermal_inhibit_heating_low_water~1]
pub fn test_heating_component_protection_when_tank_level_low() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3, 18, None);
    // replacement values are used to initialize the mutexes
    config.sensor_manager.replacement_value_water_temperature = 20.0;
    config.sensor_manager.replacement_value_ambient_temperature = 24.0;

    let mut channels = Channels::new_for_test();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    // Mutex for tank level switch signals
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
            assert_eq!(actuation_events.len(), 2);
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOff(AquariumDevice::Heater)
            );
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOn(AquariumDevice::Heater)
            );
            mock_actuator_states.check_terminal_condition_heating();
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_ten_seconds = Duration::from_secs(10);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration_ten_seconds);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // make sure all mock threads are running when instantiating the test object
    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_heating_to_schedule_check_for_test_case_finish =
                channels.heating.tx_heating_to_schedule_check.clone();
            let mut tx_heating_to_relay_manager_for_test_case_finish =
                channels.heating.tx_heating_to_relay_manager.clone();

            let mut heating_stats_transfer =
                HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
            let mut heating_set_value_updater =
                MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

            let heating_mutexes = HeatingMutexes {
                mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                    &config.sensor_manager,
                ))),
                mutex_tank_level_switch_signals,
                mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut heating,
                mutex_device_scheduler_heating.clone(),
                &mut channels.heating,
                &mut heating_stats_transfer,
                &mut heating_set_value_updater,
                heating_mutexes,
                sql_interface_heating_stats,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

            println!("* [Heating] checking if schedule checker can block actuation.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// Test case runs heating control and triggers inhibition by sending a stop message via the channel.
// After verification that heating is stopped,
// The test case sends the start message via the channel and verifies if heating control
// resumes operation.
// Test case uses test database #19.
// [itest->swreq~thermal_remote_heating_stop~1]
// [itest->swreq~thermal_remote_heating_start~1]
pub fn test_messaging_stops_starts_heating() {
    let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 19, None);

    // replacement values are used to initialize the mutexes
    config.sensor_manager.replacement_value_water_temperature = 20.0;
    config.sensor_manager.replacement_value_ambient_temperature = 24.0;

    let mut channels = Channels::new_for_test();

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));
    let mutex_device_scheduler_test_environment = mutex_device_scheduler_heating.clone();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    // Mutex for tank level switch signals
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
            println!("actuation_events:");
            for actuation_event in &actuation_events {
                println!("{}", actuation_event);
            }
            assert_eq!(actuation_events.len(), 4);
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOff(AquariumDevice::Heater)
            );
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOn(AquariumDevice::Heater)
            );
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOff(AquariumDevice::Heater)
            );
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOn(AquariumDevice::Heater)
            );
            mock_actuator_states.check_terminal_condition_heating();
        })
        .unwrap();

    // thread for test environment
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_100_millis = Duration::from_millis(100);
            let spin_sleeper = SpinSleeper::default();

            // initial wait so that heating control can switch on
            for _ in 0..10 {
                spin_sleeper.sleep(sleep_duration_100_millis);
            }
            // check the initial state of mutex: should be already switched on
            assert_eq!(*mutex_device_scheduler_test_environment.blocking_lock(), 1);

            // sending a message requesting stop of heating control
            match channels
                .messaging
                .tx_messaging_to_heating
                .send(InternalCommand::Stop)
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: error when sending stop command to test object ({e:?})",
                        module_path!()
                    );
                }
            }

            // wait for 1 second: heating control will switch off
            for _ in 0..10 {
                spin_sleeper.sleep(sleep_duration_100_millis);
            }
            // check if heating is actuated
            assert_eq!(*mutex_device_scheduler_test_environment.blocking_lock(), 2);

            // sending a message requesting restart of heating control
            match channels
                .messaging
                .tx_messaging_to_heating
                .send(InternalCommand::Start)
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: error when sending start command to test object ({e:?})",
                        module_path!()
                    );
                }
            }

            // Wait for 1 second. Heating control will switch on
            for _ in 0..10 {
                spin_sleeper.sleep(sleep_duration_100_millis);
            }
            // check if heating is actuated
            assert_eq!(*mutex_device_scheduler_test_environment.blocking_lock(), 3);

            // sending a message requesting stop of heating control
            match channels
                .messaging
                .tx_messaging_to_heating
                .send(InternalCommand::Stop)
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: error when sending stop command to test object ({e:?})",
                        module_path!()
                    );
                }
            }

            // wait for 1 second: Heating control will switch off
            for _ in 0..10 {
                spin_sleeper.sleep(sleep_duration_100_millis);
            }
            // check if heating is actuated
            assert_eq!(*mutex_device_scheduler_test_environment.blocking_lock(), 4);

            // requesting heating control to quit
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();

            // check if heating is not actuated
            assert_eq!(*mutex_device_scheduler_test_environment.blocking_lock(), 4);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_heating_to_schedule_check_for_test_case_finish =
                channels.heating.tx_heating_to_schedule_check.clone();
            let mut tx_heating_to_relay_manager_for_test_case_finish =
                channels.heating.tx_heating_to_relay_manager.clone();

            let mut heating_stats_transfer =
                HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
            let mut heating_set_value_updater =
                MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

            let heating_mutexes = HeatingMutexes {
                mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                    &config.sensor_manager,
                ))),
                mutex_tank_level_switch_signals,
                mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut heating,
                mutex_device_scheduler_heating.clone(),
                &mut channels.heating,
                &mut heating_stats_transfer,
                &mut heating_set_value_updater,
                heating_mutexes,
                sql_interface_heating_stats,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

            println!("* [Heating] checking if messaging can block and restart actuation.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// Check if the heater remains switched off with a high temperature initially.
// After a limited time period, increase the set values and observe if the heater is switched on.
// Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
// Test case uses test database #50.
// [itest->swreq~thermal_read_heating_limits~1]
pub fn test_heating_with_increased_set_values() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 50, None);
    // replacement values are used to initialize the mutexes
    config.sensor_manager.replacement_value_water_temperature = 30.0;
    config.sensor_manager.replacement_value_ambient_temperature = 24.0;

    let mut channels = Channels::new_for_test();

    // Mutex for tank level switch signals
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    // thread for mock TankLevelSwitch
    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
            assert_eq!(actuation_events.len(), 3);
            let actuation_event_3 = actuation_events.pop().unwrap();
            let actuation_event_2 = actuation_events.pop().unwrap();
            let actuation_event_1 = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event_1.command,
                InternalCommand::SwitchOff(AquariumDevice::Heater)
            );
            assert_eq!(
                actuation_event_2.command,
                InternalCommand::SwitchOn(AquariumDevice::Heater)
            );
            assert_eq!(
                actuation_event_3.command,
                InternalCommand::SwitchOff(AquariumDevice::Heater)
            );
            mock_actuator_states.check_terminal_condition_heating();
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_twelve_seconds = Duration::from_secs(12);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration_twelve_seconds);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // make sure all mock threads are running when instantiating the test object
    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_heating_to_schedule_check_for_test_case_finish =
                channels.heating.tx_heating_to_schedule_check.clone();
            let mut tx_heating_to_relay_manager_for_test_case_finish =
                channels.heating.tx_heating_to_relay_manager.clone();

            let mut heating_stats_transfer =
                HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());

            // set up the set value updater to actively manipulate the set values
            let mut heating_set_value_updater = MockSqlInterfaceHeatingSetVals::new(
                true,
                Some(50.0),
                Some(40.0),
                Some(Duration::from_secs(10)),
            );

            let heating_mutexes = HeatingMutexes {
                mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                    &config.sensor_manager,
                ))),
                mutex_tank_level_switch_signals,
                mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut heating,
                mutex_device_scheduler_heating.clone(),
                &mut channels.heating,
                &mut heating_stats_transfer,
                &mut heating_set_value_updater,
                heating_mutexes,
                sql_interface_heating_stats,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

            println!("* [Heating] checking reaction to high temperature succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// [itest->swreq~thermal_heater_heating~1]
// Check if the heater is switched on and is not switched off with low temperature.
// Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
// Check if the heater is switched off after sending Quit command and before sending Terminate command.
// Test case uses test database #72.
// Additionally, mutex for heater status is blocked by test environment
pub fn test_heating_with_measured_temperature_low_with_mutex_blocked() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 72, None);

    // replacement values are used to initialize the mutexes
    config.sensor_manager.replacement_value_water_temperature = 20.0;
    config.sensor_manager.replacement_value_ambient_temperature = 24.0;

    let mut channels = Channels::new_for_test();

    // Mutex for tank level switch signals
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));
    let mutex_heating_status = Arc::new(Mutex::new(ThermalControlStatus::default()));
    let mutex_heating_status_clone_for_test_environment = mutex_heating_status.clone();

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
            assert_eq!(actuation_events.len(), 2);
            let last_actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                last_actuation_event.command,
                InternalCommand::SwitchOff(AquariumDevice::Heater)
            );
            let first_actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                first_actuation_event.command,
                InternalCommand::SwitchOn(AquariumDevice::Heater)
            );
            mock_actuator_states.check_terminal_condition_heating();
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_one_second = Duration::from_secs(1);
            let spin_sleeper = SpinSleeper::default();
            for i in 0..10 {
                if i % 2 == 0 {
                    spin_sleeper.sleep(sleep_duration_one_second);
                } else {
                    // block mutex on purpose
                    {
                        match mutex_heating_status_clone_for_test_environment.lock() {
                            Ok(_guard) => {
                                spin_sleeper.sleep(sleep_duration_one_second);
                            }
                            Err(_) => {
                                // Do nothing
                            }
                        }
                    }
                }
            }
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // make sure all mock threads are running when instantiating the test object
    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_heating_to_schedule_check_for_test_case_finish =
                channels.heating.tx_heating_to_schedule_check.clone();
            let mut tx_heating_to_relay_manager_for_test_case_finish =
                channels.heating.tx_heating_to_relay_manager.clone();

            let mut heating_stats_transfer =
                HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
            let mut heating_set_value_updater =
                MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

            let heating_mutexes = HeatingMutexes {
                mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                    &config.sensor_manager,
                ))),
                mutex_tank_level_switch_signals,
                mutex_heating_status,
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut heating,
                mutex_device_scheduler_heating.clone(),
                &mut channels.heating,
                &mut heating_stats_transfer,
                &mut heating_set_value_updater,
                heating_mutexes,
                sql_interface_heating_stats,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

            #[cfg(feature = "test-utils")]
            assert!(heating.mutex_access_duration_exceeded);

            println!(
                "* [Heating] checking reaction to low temperature with mutex blocked succeeded."
            );
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// [itest->swreq~thermal_connect_heating_stats~1]
// Test case uses test database #73.
fn test_record_heating_stats_before_midnight() {
    // 1. Setup
    let (_config, mut heating, mut sql_interface, mut sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 73, None); // Midnight is far away
    let mut mock_transfer = HeatingStatsMockDataTransfer::new(&mut sql_interface.conn);
    let mut midnight_instant = Instant::now() + Duration::from_secs(60);

    let mut heating_stats = HeatingStatsEntry::new(&mut sql_interface_heating_stats.conn);
    heating_stats.energy = 1.23; // Give it some data to track
    let mut heating_stats_opt = Some(heating_stats.clone());

    // 2. Execute
    heating.record_heating_stats(
        &mut midnight_instant,
        &mut heating_stats_opt,
        &mut mock_transfer,
        &mut sql_interface_heating_stats,
        false,
    );

    // 3. Assert
    assert_eq!(
        mock_transfer.transfer_counter, 0,
        "Transfer should not be called before midnight"
    );
    assert!(
        heating_stats_opt.is_some(),
        "Stats option should still be Some"
    );
    // Statistical data should be unchanged
    assert_float_absolute_eq!(heating_stats_opt.unwrap().energy, 1.23, 0.001);
}

#[test]
// [itest->swreq~thermal_connect_heating_stats~1]
// Test case uses test database #74.
fn test_record_heating_stats_after_midnight_success() {
    // 1. Setup - mock midnight is one second away
    let (_config, mut heating, mut sql_interface, mut sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(1, 74, None);
    let mut mock_transfer = HeatingStatsMockDataTransfer::new(&mut sql_interface.conn);
    let mut midnight_instant = Instant::now() - Duration::from_secs(1); // Set midnight in the past
    let original_midnight_instant = midnight_instant;

    let mut heating_stats = HeatingStatsEntry::new(&mut sql_interface_heating_stats.conn);
    heating_stats.energy = 4.56;
    let mut heating_stats_opt = Some(heating_stats.clone());

    // 2. Execute
    heating.record_heating_stats(
        &mut midnight_instant,
        &mut heating_stats_opt,
        &mut mock_transfer,
        &mut sql_interface_heating_stats,
        false,
    );

    // 3. Assert
    assert_eq!(
        mock_transfer.transfer_counter, 1,
        "Transfer should be called once"
    );

    let transferred_stats = mock_transfer.last_transferred_heating_stats_entry;
    assert!(
        transferred_stats.is_some(),
        "Transferred stats should be captured by the mock"
    );
    // The correct stats data should be transferred
    assert_float_absolute_eq!(transferred_stats.as_ref().unwrap().energy, 4.56, 0.001);

    assert!(
        heating_stats_opt.is_some(),
        "Stats option should be re-initialized, not None"
    );
    // Stats should be reset to a new, default entry
    assert_float_absolute_eq!(heating_stats_opt.unwrap().energy, 0.0, 0.001);

    assert!(
        midnight_instant > original_midnight_instant,
        "Midnight instant should be updated to a future time"
    );
}

#[test]
// [itest->swreq~thermal_connect_heating_stats~1]
// Test case uses test database #75.
fn test_record_heating_stats_after_midnight_with_error() {
    // 1. Setup
    let (_config, mut heating, mut sql_interface, mut sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(1, 75, None);
    let mut mock_transfer = HeatingStatsMockDataTransfer::new(&mut sql_interface.conn);
    let mut midnight_instant = Instant::now() - Duration::from_secs(1);
    let original_midnight_instant = midnight_instant;

    let mut heating_stats = HeatingStatsEntry::new(&mut sql_interface_heating_stats.conn);
    heating_stats.energy = 7.89;
    let mut heating_stats_opt = Some(heating_stats.clone());

    // 2. Execute
    heating.record_heating_stats(
        &mut midnight_instant,
        &mut heating_stats_opt,
        &mut mock_transfer,
        &mut sql_interface_heating_stats,
        false,
    );

    // 3. Assert
    assert_eq!(
        mock_transfer.transfer_counter, 1,
        "Transfer should still be attempted even if it fails"
    );

    assert!(
        heating_stats_opt.is_some(),
        "Stats option should be re-initialized even on error"
    );
    // Stats should be reset to a new entry despite the transfer error
    assert_float_absolute_eq!(heating_stats_opt.unwrap().energy, 0.0, 0.001);

    assert!(
        midnight_instant > original_midnight_instant,
        "Midnight instant should still be updated for the next day"
    );
}

#[test]
// [itest->swreq~thermal_connect_heating_stats~1]
// Test case uses test database #76.
fn test_record_heating_stats_with_none_stats() {
    // 1. Setup
    let (_config, mut heating, mut sql_interface, mut sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(1, 76, None);
    let mut mock_transfer = HeatingStatsMockDataTransfer::new(&mut sql_interface.conn);
    let mut midnight_instant = Instant::now() - Duration::from_secs(1); // Midnight is in the past
    let original_midnight_instant = midnight_instant;
    let mut heating_stats_opt: Option<HeatingStatsEntry> = None;

    // 2. Execute
    heating.record_heating_stats(
        &mut midnight_instant,
        &mut heating_stats_opt,
        &mut mock_transfer,
        &mut sql_interface_heating_stats,
        false,
    );

    // 3. Assert
    assert_eq!(
        mock_transfer.transfer_counter, 0,
        "Transfer should not be called if stats are None"
    );
    assert!(
        heating_stats_opt.is_none(),
        "Stats option should remain None"
    );
    assert_eq!(
        midnight_instant, original_midnight_instant,
        "Midnight instant should not be updated if no transfer happens"
    );
}

#[test]
// [itest->swreq~thermal_connect_heating_stats~1]
// Test case uses test database #77.
fn test_update_heating_stats_heater_on_valid_data() {
    // 1. Setup
    let (config, mut heating, _sql_interface, mut sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 77, None);
    let mut heating_stats_opt = Some(HeatingStatsEntry::new(
        &mut sql_interface_heating_stats.conn,
    ));
    let sleep_duration = Duration::from_millis(500);

    // Manually set the last update time to simulate a time delta
    heating.last_stats_update_instant_opt = Some(Instant::now());
    thread::sleep(sleep_duration);

    // 2. Execute
    heating.update_heating_stats(
        &mut heating_stats_opt,
        false, // measurement_error
        25.5,  // water_temperature
        22.0,  // ambient_temperature
        &ActuatorState::On,
    );

    // 3. Assert
    let stats = heating_stats_opt.unwrap();
    let expected_energy =
        config.heating.heater_power as f64 * (sleep_duration.as_secs_f64() / 3600.0);

    assert_float_absolute_eq!(stats.energy, expected_energy, 0.01);
    assert_float_absolute_eq!(stats.water_temperature_average_value, 25.5, 0.001);
    assert_eq!(stats.water_temperature_average_counter, 1);
    assert_float_absolute_eq!(stats.ambient_temperature_average_value, 22.0, 0.001);
    assert_eq!(stats.ambient_temperature_average_counter, 1);
    assert_float_absolute_eq!(
        stats.heating_control_runtime,
        sleep_duration.as_secs_f64(),
        0.2
    );
}

#[test]
// [itest->swreq~thermal_connect_heating_stats~1]
// Test case uses test database #78.
fn test_update_heating_stats_heater_off() {
    // 1. Setup
    let (_config, mut heating, _sql_interface, mut sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 78, None);
    let mut heating_stats_opt = Some(HeatingStatsEntry::new(
        &mut sql_interface_heating_stats.conn,
    ));
    let sleep_duration = Duration::from_millis(500);

    heating.last_stats_update_instant_opt = Some(Instant::now());
    thread::sleep(sleep_duration);

    // 2. Execute
    heating.update_heating_stats(
        &mut heating_stats_opt,
        false, // measurement_error
        25.5,  // water_temperature
        22.0,  // ambient_temperature
        &ActuatorState::Off,
    );

    // 3. Assert
    let stats = heating_stats_opt.unwrap();
    // Energy should be zero when the heater is off
    assert_float_absolute_eq!(stats.energy, 0.0, 0.001);
    assert_float_absolute_eq!(stats.water_temperature_average_value, 25.5, 0.001);
    assert_eq!(stats.water_temperature_average_counter, 1);
}

#[test]
// [itest->swreq~thermal_connect_heating_stats~1]
// Test case uses test database #79.
fn test_update_heating_stats_with_measurement_error() {
    // 1. Setup
    let (_config, mut heating, _sql_interface, mut sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 79, None);
    let mut heating_stats_opt = Some(HeatingStatsEntry::new(
        &mut sql_interface_heating_stats.conn,
    ));
    let sleep_duration = Duration::from_millis(500);

    heating.last_stats_update_instant_opt = Some(Instant::now());
    thread::sleep(sleep_duration);

    // 2. Execute
    heating.update_heating_stats(
        &mut heating_stats_opt,
        true,  // measurement_error
        -99.0, // water_temperature (should be ignored)
        -99.0, // ambient_temperature (should be ignored)
        &ActuatorState::On,
    );

    // 3. Assert
    let stats = heating_stats_opt.unwrap();
    assert!(
        stats.energy > 0.0,
        "Energy should still be calculated even with measurement errors"
    );
    // Average should not be updated
    assert_float_absolute_eq!(stats.water_temperature_average_value, 0.0, 0.001);
    assert_eq!(
        stats.water_temperature_average_counter, 0,
        "Counter should not be incremented"
    );
    assert_float_absolute_eq!(stats.ambient_temperature_average_value, 0.0, 0.001);
    assert_eq!(
        stats.ambient_temperature_average_counter, 0,
        "Counter should not be incremented"
    );
}

#[test]
// [itest->swreq~thermal_connect_heating_stats~1]
// Verifies that the first call to update_heating_stats results in zero energy,
// because `last_stats_update_instant_opt` is initially None.
fn test_update_heating_stats_first_call_no_energy() {
    // 1. Setup
    let (_config, mut heating, _sql_interface, mut sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 27, None);
    let mut heating_stats_opt = Some(HeatingStatsEntry::new(
        &mut sql_interface_heating_stats.conn,
    ));

    // `heating.last_stats_update_instant_opt` is None by default from the constructor

    // 2. Execute
    heating.update_heating_stats(
        &mut heating_stats_opt,
        false,
        25.5,
        22.0,
        &ActuatorState::On,
    );

    // 3. Assert
    // Energy should be zero on the first call because increment_fraction is 0.0
    let stats = heating_stats_opt.unwrap();
    assert_float_absolute_eq!(stats.energy, 0.0, 0.001);
    // Runtime should be zero on the first call
    assert_float_absolute_eq!(stats.heating_control_runtime, 0.0, 0.001);
    assert_eq!(
        stats.water_temperature_average_counter, 1,
        "Temperatures should still be recorded"
    );
}

#[test]
// [itest->swreq~thermal_connect_heating_stats~1]
// Test case uses test database #80.
fn test_update_heating_stats_with_none_stats_opt() {
    // 1. Setup
    let (_config, mut heating, _sql_interface, _sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 80, None);
    let mut heating_stats_opt: Option<HeatingStatsEntry> = None;

    // 2. Execute
    heating.update_heating_stats(
        &mut heating_stats_opt,
        false,
        25.5,
        22.0,
        &ActuatorState::On,
    );

    // 3. Assert
    assert!(
        heating_stats_opt.is_none(),
        "The stats option should remain None"
    );
}

#[test]
// Test case uses test database #81.
// [itest->swreq~thermal_temperature_gradient_monitor~1]
pub fn test_heating_gradient_monitoring_disables_actuator() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 81, None);
    // test case is not writing to database, hence db can be reused.

    // Configure gradient monitoring to be very aggressive
    config.heating.max_time_actuator_effect_monitor = 2;
    config.heating.switch_off_when_no_effect = true;
    heating.config = config.heating;

    // replacement value is used for initializing mutex - needs to start with a low value
    config.sensor_manager.replacement_value_water_temperature = 20.0;

    let mut channels = Channels::new_for_test();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    // Mutex needed for water temperature
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));

    // Mutex for heating status to check assertions
    let mutex_heating_status = Arc::new(Mutex::new(ThermalControlStatus::default()));
    let mutex_heating_status_clone = mutex_heating_status.clone();

    // Mutex for temperature gradient
    let mutex_temperature_gradient = Arc::new(Mutex::new(0.1)); // Start with heating gradient
    let mutex_temperature_gradient_clone = mutex_temperature_gradient.clone();

    // thread for mock relay manager
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
            // Expect On, Off
            assert_eq!(actuation_events.len(), 2);
            assert_eq!(
                actuation_events[0].command,
                InternalCommand::SwitchOn(AquariumDevice::Heater)
            );
            assert_eq!(
                actuation_events[1].command,
                InternalCommand::SwitchOff(AquariumDevice::Heater)
            );

            mock_actuator_states.check_terminal_condition_heating();
        })
        .unwrap();

    // thread for controlling test phases
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_phase = Duration::from_secs(5);
            let spin_sleeper = SpinSleeper::default();

            // Phase 1: High temp, good gradient. Wait 5s.
            spin_sleeper.sleep(sleep_duration_phase);
            {
                let status = mutex_heating_status_clone.lock().unwrap();
                assert!(status.actuator_switched_on, "Phase 1: Heating should be ON");
            }

            // Phase 2: Constant temp (gradient indicates that actuator is ineffective).
            {
                let mut gradient = mutex_temperature_gradient_clone.lock().unwrap();
                *gradient = 0.0;
            }
            // Wait 5s (monitor timeout is 2s)
            spin_sleeper.sleep(sleep_duration_phase);
            {
                let status = mutex_heating_status_clone.lock().unwrap();
                assert!(
                    !status.actuator_switched_on,
                    "Phase 2: Heating should be OFF due to gradient monitor"
                );
            }

            // Terminate
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let mut tx_heating_to_schedule_check_for_test_case_finish =
                channels.heating.tx_heating_to_schedule_check.clone();
            let mut tx_heating_to_relay_manager_for_test_case_finish =
                channels.heating.tx_heating_to_relay_manager.clone();

            let mut heating_set_value_updater =
                MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

            let mut heating_stats_transfer =
                HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());

            let heating_mutexes = HeatingMutexes {
                mutex_sensor_manager_signals,
                mutex_tank_level_switch_signals: Arc::new(Mutex::new(TankLevelSwitchSignals {
                    tank_level_switch_position: true,
                    tank_level_switch_position_stabilized: true,
                    tank_level_switch_invalid: false,
                })),
                mutex_heating_status,
                mutex_temperature_gradient,
            };

            execute_blocking(
                &mut heating,
                mutex_device_scheduler_heating.clone(),
                &mut channels.heating,
                &mut heating_stats_transfer,
                &mut heating_set_value_updater,
                heating_mutexes,
                sql_interface_heating_stats,
            );

            // send Quit signal to mock threads
            let _ = tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
            println!("* [Heating] checking gradient monitoring succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check failed");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager failed");
    join_handle_test_environment
        .join()
        .expect("Test environment failed");
    join_handle_test_object.join().expect("Test object failed");
}

#[test]
// [itest->swreq~thermal_heater_heating~1]
// Test case uses test database #82.
pub fn test_heating_reset_errors_enables_actuator() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 82, None);
    // test case is not writing to database, hence db can be reused.

    // Start in disabled state
    heating.actuator_disabled = true;

    // replacement value is used for initializing mutex - needs to start with a low value
    config.sensor_manager.replacement_value_water_temperature = 20.0;

    let mut channels = Channels::new_for_test();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    // Mutex needed for water temperature
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));

    // Mutex for heating status to check assertions
    let mutex_heating_status = Arc::new(Mutex::new(ThermalControlStatus::default()));
    let mutex_heating_status_clone = mutex_heating_status.clone();

    // Mutex for temperature gradient
    let mutex_temperature_gradient = Arc::new(Mutex::new(0.1)); // Start with positive gradient
    let mutex_temperature_gradient_clone = mutex_temperature_gradient.clone();

    // thread for mock relay manager
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
            // Expect SwitchOn (after reset), SwitchOff (at termination)
            assert_eq!(actuation_events.len(), 2);
            assert_eq!(
                actuation_events[0].command,
                InternalCommand::SwitchOn(AquariumDevice::Heater)
            );
            assert_eq!(
                actuation_events[1].command,
                InternalCommand::SwitchOff(AquariumDevice::Heater)
            );

            mock_actuator_states.check_terminal_condition_heating();
        })
        .unwrap();

    // thread for controlling test phases
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_phase = Duration::from_secs(5);
            let spin_sleeper = SpinSleeper::default();

            // Phase 1: Disabled state. Wait 5s.
            spin_sleeper.sleep(sleep_duration_phase);
            {
                let status = mutex_heating_status_clone.lock().unwrap();
                assert!(
                    !status.actuator_switched_on,
                    "Phase 1: Heating should be OFF (disabled)"
                );
            }

            // Send ResetAllErrors command
            channels
                .messaging
                .tx_messaging_to_heating
                .send(InternalCommand::ResetAllErrors)
                .unwrap();

            // Phase 2: Enabled state. Wait 5s.
            spin_sleeper.sleep(sleep_duration_phase);
            {
                let status = mutex_heating_status_clone.lock().unwrap();
                assert!(
                    status.actuator_switched_on,
                    "Phase 2: Heating should be ON (enabled)"
                );
            }

            // Terminate
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let mut tx_heating_to_schedule_check_for_test_case_finish =
                channels.heating.tx_heating_to_schedule_check.clone();
            let mut tx_heating_to_relay_manager_for_test_case_finish =
                channels.heating.tx_heating_to_relay_manager.clone();

            let mut heating_set_value_updater =
                MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

            let mut heating_stats_transfer =
                HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());

            let heating_mutexes = HeatingMutexes {
                mutex_sensor_manager_signals,
                mutex_tank_level_switch_signals: Arc::new(Mutex::new(TankLevelSwitchSignals {
                    tank_level_switch_position: true,
                    tank_level_switch_position_stabilized: true,
                    tank_level_switch_invalid: false,
                })),
                mutex_heating_status,
                mutex_temperature_gradient: mutex_temperature_gradient_clone,
            };

            execute_blocking(
                &mut heating,
                mutex_device_scheduler_heating.clone(),
                &mut channels.heating,
                &mut heating_stats_transfer,
                &mut heating_set_value_updater,
                heating_mutexes,
                sql_interface_heating_stats,
            );

            // send Quit signal to mock threads
            let _ = tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
            println!("* [Heating] checking reset errors enables actuator succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check failed");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager failed");
    join_handle_test_environment
        .join()
        .expect("Test environment failed");
    join_handle_test_object.join().expect("Test object failed");
}

#[test]
// [itest->swreq~thermal_heater_heating~1]
// Verifies that heater actuation is inhibited when device_scheduler mutex is blocked.
// The test environment thread acquires and holds the device_scheduler mutex lock before heater actuation occurs.
// While the lock is held, heater actuation is blocked. Once released, heater actuation proceeds normally.
// Test case uses test database #70.
pub fn test_heating_actuation_with_device_scheduler_mutex_blocked() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut heating, sql_interface, sql_interface_heating_stats) =
        prepare_heating_tests_mock_midnight_calculator(3600, 70, None);

    config.sensor_manager.replacement_value_water_temperature = 20.0;
    config.sensor_manager.replacement_value_ambient_temperature = 24.0;

    let mut channels = Channels::new_for_test();

    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_heating,
                &mut channels.schedule_check.rx_schedule_check_from_heating,
                None,
                true,
            );
        })
        .unwrap();

    let mutex_device_scheduler_heating = Arc::new(tokio::sync::Mutex::new(0));
    let mutex_device_scheduler_environment = mutex_device_scheduler_heating.clone();

    // thread for mock relay manager
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_heating,
                &mut channels.relay_manager.rx_relay_manager_from_heating,
            );
            assert!(!actuation_events.is_empty());
            mock_actuator_states.check_terminal_condition_heating();
        })
        .unwrap();

    // thread for controlling duration of test run and blocking mutex
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let spin_sleeper = SpinSleeper::default();

            // 1. Acquire lock on device_scheduler mutex before heater actuation executes
            let lock_guard = mutex_device_scheduler_environment.blocking_lock();

            // Hold lock for 3 seconds
            spin_sleeper.sleep(Duration::from_secs(3));

            // Verify execution count remains 0 while lock is held
            assert_eq!(*lock_guard, 0);

            // 2. Release lock so heater actuation can proceed
            drop(lock_guard);

            // Wait 3 seconds for actuation to complete
            spin_sleeper.sleep(Duration::from_secs(3));

            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_heating
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let mut tx_heating_to_schedule_check_for_test_case_finish =
                channels.heating.tx_heating_to_schedule_check.clone();
            let mut tx_heating_to_relay_manager_for_test_case_finish =
                channels.heating.tx_heating_to_relay_manager.clone();

            let mut heating_stats_transfer =
                HeatingStatsMockDataTransfer::new(&mut sql_interface.get_connection().unwrap());
            let mut heating_set_value_updater =
                MockSqlInterfaceHeatingSetVals::new(false, None, None, None);

            let heating_mutexes = HeatingMutexes {
                mutex_sensor_manager_signals: Arc::new(Mutex::new(SensorManagerSignals::new(
                    &config.sensor_manager,
                ))),
                mutex_tank_level_switch_signals,
                mutex_heating_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut heating,
                mutex_device_scheduler_heating.clone(),
                &mut channels.heating,
                &mut heating_stats_transfer,
                &mut heating_set_value_updater,
                heating_mutexes,
                sql_interface_heating_stats,
            );

            let _ = tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

            assert!(*mutex_device_scheduler_heating.blocking_lock() >= 1);
            println!(
                "* [Heating] checking actuation with device scheduler mutex blocked succeeded."
            );
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}
