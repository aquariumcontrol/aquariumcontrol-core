/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the automated freshwater refill system for the aquarium.

use all_asserts::{assert_ge, assert_le};
use aquarium_control::database::sql_interface::SqlInterface;
use aquarium_control::database::sql_test_query_strings::SQL_TABLE_REFILL;
use aquarium_control::launch::channels::{
    channel, AquaChannelError, AquaReceiver, AquaSender, Channels,
};
use aquarium_control::mocks::mock_monitors::tests::MockMonitors;
use aquarium_control::mocks::mock_schedule_check::tests::mock_schedule_check;
use aquarium_control::mocks::mock_sql_interface_refill::tests::MockSqlInterfaceRefill;
use aquarium_control::mocks::mock_water_injection::tests::MockWaterInjection;
use aquarium_control::mocks::test_command::tests::TestCommand;
use aquarium_control::sensors::tank_level_switch::TankLevelSwitchSignals;
use aquarium_control::utilities::channel_content::InternalCommand;
use aquarium_control::utilities::config::{read_config_file_with_test_database, ConfigData};
use aquarium_control::utilities::proc_ext_req::ProcessExternalRequestTrait;
use aquarium_control::water::refill::RefillErrorStates;
use aquarium_control::water::refill::{prepare_refill_tests, Refill};
use aquarium_control::water::refill_status::RefillStatus;
use spin_sleep::SpinSleeper;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use aquarium_control::database::sql_interface_refill::DatabaseInterfaceRefillTrait;
use aquarium_control::utilities::logger::setup_logger;
use aquarium_control::utilities::logger_config::LoggerConfig;
use aquarium_control::water::refill_channels::RefillChannels;
use aquarium_control::water::refill_stats_data::RefillStatsData;
use aquarium_control::water::water_injection::WaterInjectionTrait;

/// Synchronously executes the main task loop for testing.
///
/// # Arguments
/// * `refill` - A parameter representing `refill`.
/// * `mutex_device_scheduler_refill` - Mutex coordinating access to the device scheduler for refill.
/// * `refill_channels` - A mutable reference to the `RefillChannels` communication interface.
/// * `water_injection` - A parameter representing `water_injection`.
///
/// [itest->swreq~water_determine_refill~1]
fn execute_blocking(
    refill: &mut Refill,
    mutex_device_scheduler_refill: Arc<tokio::sync::Mutex<i32>>,
    refill_channels: &mut RefillChannels,
    water_injection: &mut (impl WaterInjectionTrait + Send),
    mutex_tank_level_switch_signals_clone_for_refill: Arc<Mutex<TankLevelSwitchSignals>>,
    mutex_refill_status: Arc<Mutex<RefillStatus>>,
    sql_interface_refill: Box<dyn DatabaseInterfaceRefillTrait + Sync + Send>,
) {
    tokio::runtime::Builder::new_multi_thread().enable_all()
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

            refill
                .execute(
                    mutex_device_scheduler_refill,
                    refill_channels,
                    water_injection,
                    mutex_tank_level_switch_signals_clone_for_refill,
                    mutex_refill_status,
                    sql_interface_refill,
                    is_multi_thread_test,
                )
                .await;
        });
}

/// Prepares a `Refill` control instance with a **mock database interface** for isolated testing.
///
/// This helper function is designed exclusively for unit and integration testing of the `Refill` module.
/// It sets up a controlled environment by:
/// 1.  Loading a specific test configuration file (using `test_db_number` to select the database).
/// 2.  Overriding the `initial_wait_interval` and `check_interval` within the `RefillConfig` to suit test needs.
/// 3.  **Injecting a `MockSqlInterfaceRefill` instance** as the database interface, ensuring that tests
///     do not interact with a real SQL database. This allows for predictable and fast test execution.
/// 4.  Optionally providing a predefined `RefillErrors` struct to set up specific error conditions
///     at the `Refill` instance's startup.
///
/// # Arguments
/// * `refill_errors` - An `Option<RefillErrors>`. If `Some`, the `Refill` instance is initialized
///   with these specific error flags; otherwise, default `RefillErrors` (all flags `false`) are used.
/// * `initial_wait_interval` - The `u64` value to set for `RefillConfig.initial_wait_interval` in the test setup.
/// * `check_interval` - The `u64` value to set for `RefillConfig.check_interval` in the test setup.
/// * `test_db_number` - A **unique `u32` identifier** used to select a specific test database (`aquarium_test_XX`)
///   from the configuration. This ensures test isolation even with database-related config.
///
/// # Returns
/// A tuple `(ConfigData, Refill)`:
/// - The `ConfigData` struct, containing the loaded and modified configuration used for the test.
/// - The fully initialized `Refill` struct, with its database interactions handled by `MockSqlInterfaceRefill`.
///
/// # Panics
/// This function will panic if:
/// - `read_config_file_with_test_database` fails (e.g., the configuration file is not found or the `test_db_number` is invalid).
/// - There are issues during the initialization of the `Refill` struct (e.g., due to an invalid `refill_config`).
pub fn prepare_refill_tests_with_mock_database_interface(
    refill_errors: Option<RefillErrorStates>,
    initial_wait_interval: u64,
    check_interval: u64,
    test_db_number: u32,
) -> (ConfigData, Refill, MockSqlInterfaceRefill) {
    let config_file_name = "/config/aquarium_control_test_generic.toml";

    // prepare struct Refill for subsequent tests
    let mut config: ConfigData =
        read_config_file_with_test_database(config_file_name.to_string(), test_db_number);

    config.refill.initial_wait_interval = initial_wait_interval;
    config.refill.check_interval = check_interval;

    let sql_interface_refill = MockSqlInterfaceRefill {};

    let refill_config = config.refill.clone();
    (
        config,
        match refill_errors.is_some() {
            true => Refill::new_with_errors(refill_config, refill_errors.unwrap()),
            false => Refill::new(refill_config, Duration::from_millis(1000)),
        },
        sql_interface_refill,
    )
}

#[test]
// Test case checks the implementation of ProcessExternalRequestTrait.
// The test case uses test database #46.
// [itest->swreq~water_channel_communication~1]
pub fn test_refill_process_external_request_empty_channel() {
    let refill_errors = RefillErrorStates {
        error_switch_stuck_high: false,
        error_switch_stuck_low: false,
        error_receive_tank_level_switch_position: false,
        error_sql_update_failed: false,
        error_sql_get_historic_data_failed: false,
        error_join_failed: false,
    };
    let (_, mut refill, _, _) = prepare_refill_tests(Some(refill_errors), 2, 60, Some(46));

    let (_, mut rx_refill_from_signal_handler): (
        AquaSender<InternalCommand>,
        AquaReceiver<InternalCommand>,
    ) = channel(1, true);

    let (_, mut rx_refill_from_messaging): (
        AquaSender<InternalCommand>,
        AquaReceiver<InternalCommand>,
    ) = channel(1, true);

    let (quit_command_received, start_command_received, stop_command_received) = refill
        .process_external_request(
            &mut rx_refill_from_signal_handler,
            Some(&mut rx_refill_from_messaging),
        );

    assert!(!quit_command_received);
    assert!(!start_command_received);
    assert!(!stop_command_received);
}

#[test]
// This test case executes multiple iterations of the loop inside the .execute function, including the triggering of a refill.
// Test case uses test database #20.
pub fn test_refill_trigger_water_injection() {
    let sleep_duration_50_millis = Duration::from_millis(50);
    let spin_sleeper = SpinSleeper::default();

    let (_, mut refill, mut sql_interface, sql_interface_refill) =
        prepare_refill_tests(None, 1, 1, Some(20));

    let mut channels = Channels::new_for_test();

    // test environment-specific channels
    let (
        tx_mock_water_injection_to_test_environment,
        mut rx_test_environment_from_mock_water_injection,
    ) = channel(1, true);
    let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
        channel::<InternalCommand>(1, true);

    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
    let mutex_tank_level_switch_signals_for_refill = mutex_tank_level_switch_signals.clone();
    let refill_status = RefillStatus::default();
    let mutex_refill_status = Arc::new(Mutex::new(refill_status));

    let mut mock_monitors = MockMonitors::new();

    // empty the refill table
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let mutex_device_scheduler = Arc::new(tokio::sync::Mutex::new(0));

    let mut mock_water_injection = MockWaterInjection::new(
        Some(tx_mock_water_injection_to_test_environment),
        RefillErrorStates {
            error_switch_stuck_high: false,
            error_switch_stuck_low: false,
            error_receive_tank_level_switch_position: false,
            error_sql_update_failed: false,
            error_sql_get_historic_data_failed: false,
            error_join_failed: false,
        },
    );

    // thread for mock monitors
    let join_handle_mock_monitors = thread::Builder::new()
        .name("mock_monitor".to_string())
        .spawn(move || {
            mock_monitors.execute(
                Some(channels.monitors.rx_monitors_from_refill),
                None,
                rx_monitors_from_test_environment,
            );
        })
        .unwrap();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_refill,
                &mut channels.schedule_check.rx_schedule_check_from_refill,
                None,
                true,
            );
        })
        .unwrap();

    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_two_secs = Duration::from_secs(2);
            let spin_sleeper = SpinSleeper::default();

            // wait for the test object to trigger water injection
            let _ = rx_test_environment_from_mock_water_injection
                .blocking_recv()
                .unwrap();

            // update of mock tank level position signals
            {
                let mut mutex_tank_level_switch_signals =
                    mutex_tank_level_switch_signals.lock().unwrap();
                mutex_tank_level_switch_signals.tank_level_switch_position_stabilized = true;
                mutex_tank_level_switch_signals.tank_level_switch_position = true;
            }

            let start_time = Instant::now();
            spin_sleeper.sleep(sleep_duration_two_secs);

            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_refill
                .blocking_recv()
                .unwrap();

            let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);
            let finish_time = Instant::now();
            let finalization_duration_millis = finish_time.duration_since(start_time).as_millis();

            assert_ge!(finalization_duration_millis, 2000u128);
            assert_le!(finalization_duration_millis, 2150u128);

            // wait for the test object to finish so that mock threads receive
            // the termination request before the end of this (test environment) thread
            spin_sleeper.sleep(sleep_duration_two_secs);
        })
        .unwrap();

    // give the test environment a little time to trigger the receive-call for signal update
    spin_sleeper.sleep(sleep_duration_50_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let mut tx_refill_to_schedule_check_for_test_case_finish =
                channels.refill.tx_refill_to_schedule_check.clone();

            execute_blocking(
                &mut refill,
                mutex_device_scheduler,
                &mut channels.refill,
                &mut mock_water_injection,
                mutex_tank_level_switch_signals_for_refill,
                mutex_refill_status,
                Box::new(sql_interface_refill),
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
        })
        .unwrap();

    join_handle_mock_monitors
        .join()
        .expect("Mock monitor check did not finish.");
    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// This test case executes multiple iterations of the loop inside the .execute function, including triggering a refill and simulation of the switch stuck low scenario.
// Test case uses test database #47.
/// [itest->swreq~water_error_detection~1]
pub fn test_refill_trigger_water_injection_switch_stuck_high() {
    let sleep_duration_50_millis = Duration::from_millis(50);
    let spin_sleeper = SpinSleeper::default();

    let (_, mut refill, mut sql_interface, sql_interface_refill) =
        prepare_refill_tests(None, 1, 1, Some(47));

    let mut channels = Channels::new_for_test();

    // test environment-specific channels
    let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
        channel::<InternalCommand>(1, true);
    let (
        tx_mock_water_injection_to_test_environment,
        mut rx_test_environment_from_mock_water_injection,
    ) = channel(1, true);

    // this test case shall output debug! macros
    setup_logger(LoggerConfig::default()).unwrap();

    // empty the refill table
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let mutex_device_scheduler = Arc::new(tokio::sync::Mutex::new(0));
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
    let refill_status = RefillStatus::default();
    let mutex_refill_status = Arc::new(Mutex::new(refill_status));

    let mut mock_monitors = MockMonitors::new();

    let mut mock_water_injection = MockWaterInjection::new(
        Some(tx_mock_water_injection_to_test_environment),
        RefillErrorStates {
            error_switch_stuck_high: false,
            error_switch_stuck_low: true,
            error_receive_tank_level_switch_position: false,
            error_sql_update_failed: false,
            error_sql_get_historic_data_failed: false,
            error_join_failed: false,
        },
    );

    // thread for mock monitors
    let join_handle_mock_monitors = thread::Builder::new()
        .name("mock_monitor".to_string())
        .spawn(move || {
            mock_monitors.execute(
                Some(channels.monitors.rx_monitors_from_refill),
                None,
                rx_monitors_from_test_environment,
            );
        })
        .unwrap();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_refill,
                &mut channels.schedule_check.rx_schedule_check_from_refill,
                None,
                true,
            );
        })
        .unwrap();

    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_two_secs = Duration::from_secs(2);
            let spin_sleeper = SpinSleeper::default();

            // wait for the test object to trigger water injection
            let _ = rx_test_environment_from_mock_water_injection
                .blocking_recv()
                .unwrap();

            let start_time = Instant::now();
            spin_sleeper.sleep(sleep_duration_two_secs);

            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_refill
                .blocking_recv()
                .unwrap();
            let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);
            let finish_time = Instant::now();
            let finalization_duration_millis = finish_time.duration_since(start_time).as_millis();

            println!(
                "finalization_duration_millis = {}",
                finalization_duration_millis
            );

            assert_ge!(finalization_duration_millis, 2000u128);
            assert_le!(finalization_duration_millis, 2150u128);

            // wait for the test object to finish so that mock threads receive
            // the termination request before the end of this (test environment) thread
            spin_sleeper.sleep(sleep_duration_two_secs);
        })
        .unwrap();

    // give the test environment a little time to trigger the receive-call for signal update
    spin_sleeper.sleep(sleep_duration_50_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let mut tx_refill_to_schedule_check_for_test_case_finish =
                channels.refill.tx_refill_to_schedule_check.clone();

            execute_blocking(
                &mut refill,
                mutex_device_scheduler,
                &mut channels.refill,
                &mut mock_water_injection,
                mutex_tank_level_switch_signals,
                mutex_refill_status,
                Box::new(sql_interface_refill),
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
        })
        .unwrap();

    join_handle_mock_monitors
        .join()
        .expect("Mock monitor check did not finish.");
    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// This test case executes one iteration of the loop inside the .execute function with triggering a refill
// It checks the timing of the initial wait period and main control interval.
// Test case uses test database #21.
pub fn test_refill_initial_waiting_time() {
    let sleep_duration_50_millis = Duration::from_millis(50);
    let spin_sleeper = SpinSleeper::default();

    let (config, mut refill, mut sql_interface, sql_interface_refill) =
        prepare_refill_tests(None, 2, 60, Some(21));

    let mut channels = Channels::new_for_test();

    // test environment-specific channels
    let (
        tx_mock_water_injection_to_test_environment,
        mut rx_test_environment_from_mock_water_injection,
    ) = channel(1, true);
    let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
        channel::<InternalCommand>(1, true);

    // empty the refill table
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let mutex_device_scheduler = Arc::new(tokio::sync::Mutex::new(0));
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
    let refill_status = RefillStatus::default();
    let mutex_refill_status = Arc::new(Mutex::new(refill_status));

    let mut mock_monitors = MockMonitors::new();

    let mut mock_water_injection = MockWaterInjection::new(
        Some(tx_mock_water_injection_to_test_environment),
        RefillErrorStates {
            error_switch_stuck_high: false,
            error_switch_stuck_low: false,
            error_receive_tank_level_switch_position: false,
            error_sql_update_failed: false,
            error_sql_get_historic_data_failed: false,
            error_join_failed: false,
        },
    );

    // thread for mock monitors
    let join_handle_mock_monitors = thread::Builder::new()
        .name("mock_monitor".to_string())
        .spawn(move || {
            mock_monitors.execute(
                Some(channels.monitors.rx_monitors_from_refill),
                None,
                rx_monitors_from_test_environment,
            );
        })
        .unwrap();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_refill,
                &mut channels.schedule_check.rx_schedule_check_from_refill,
                None,
                true,
            );
        })
        .unwrap();

    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let start_time = Instant::now();

            // wait for the test object to trigger water injection
            let _ = rx_test_environment_from_mock_water_injection
                .blocking_recv()
                .unwrap();

            // record time after the test object triggered water injection
            let initial_waiting_time_passed = Instant::now();

            // now we can send the quit command
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_refill
                .blocking_recv()
                .unwrap();
            let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);
            let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);
            let finish_time = Instant::now();
            let initial_wait_duration_millis = initial_waiting_time_passed
                .duration_since(start_time)
                .as_millis();
            let finalization_duration_millis = finish_time
                .duration_since(initial_waiting_time_passed)
                .as_millis();

            assert_le!(
                initial_wait_duration_millis,
                (config.refill.initial_wait_interval * 1000 + 200) as u128
            );
            assert_ge!(
                initial_wait_duration_millis,
                (config.refill.initial_wait_interval * 1000) as u128
            );
            assert_le!(finalization_duration_millis, 150u128);
        })
        .unwrap();

    // give the test environment a little time to trigger the receive-call for signal update
    spin_sleeper.sleep(sleep_duration_50_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let mut tx_refill_to_schedule_check_for_test_case_finish =
                channels.refill.tx_refill_to_schedule_check.clone();

            execute_blocking(
                &mut refill,
                mutex_device_scheduler,
                &mut channels.refill,
                &mut mock_water_injection,
                mutex_tank_level_switch_signals,
                mutex_refill_status,
                Box::new(sql_interface_refill),
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
        })
        .unwrap();

    join_handle_mock_monitors
        .join()
        .expect("Mock monitor check did not finish.");
    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// This test case executes multiple iterations of the .execute function with the error flag error_switch_stuck_low set
// during the execution of the test case, the error is reset.
// Refill control shall resume operation upon having reset the error.
// Test case uses test database #22.
pub fn test_refill_conditions_historic_data_error_switch_stuck_low() {
    let sleep_duration_50_millis = Duration::from_millis(50);
    let spin_sleeper = SpinSleeper::default();
    let (_, mut refill, mut sql_interface, sql_interface_refill) =
        prepare_refill_tests(None, 1, 1, Some(22));

    // set the error
    refill.refill_errors.error_switch_stuck_low = true;

    let mut channels = Channels::new_for_test();

    // test environment-specific channels
    let (
        tx_mock_water_injection_to_test_environment,
        mut rx_test_environment_from_mock_water_injection,
    ) = channel(1, true);
    let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
        channel::<InternalCommand>(1, true);

    let refill_status = RefillStatus::default();
    let mutex_refill_status = Arc::new(Mutex::new(refill_status));

    // empty the refill table
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let mutex_device_scheduler = Arc::new(tokio::sync::Mutex::new(0));
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
    let mutex_tank_level_switch_signals_clone_for_test_environment =
        mutex_tank_level_switch_signals.clone();

    let mut mock_monitors = MockMonitors::new();

    let mut mock_water_injection = MockWaterInjection::new(
        Some(tx_mock_water_injection_to_test_environment),
        RefillErrorStates {
            error_switch_stuck_high: false,
            error_switch_stuck_low: false,
            error_receive_tank_level_switch_position: false,
            error_sql_update_failed: false,
            error_sql_get_historic_data_failed: false,
            error_join_failed: false,
        },
    );

    // thread for mock monitors
    let join_handle_mock_monitors = thread::Builder::new()
        .name("mock_monitor".to_string())
        .spawn(move || {
            mock_monitors.execute(
                Some(channels.monitors.rx_monitors_from_refill),
                None,
                rx_monitors_from_test_environment,
            );
        })
        .unwrap();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_refill,
                &mut channels.schedule_check.rx_schedule_check_from_refill,
                None,
                true,
            );
        })
        .unwrap();

    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_two_secs = Duration::from_secs(2);
            let sleep_duration_two_millis = Duration::from_millis(2);
            let spin_sleeper = SpinSleeper::default();

            spin_sleeper.sleep(sleep_duration_two_secs);

            for _ in 0..999 {
                assert!(matches!(
                    rx_test_environment_from_mock_water_injection.try_recv(),
                    Err(AquaChannelError::Empty)
                ));
                spin_sleeper.sleep(sleep_duration_two_millis);
            }

            // reset errors
            println!("resetting errors...");
            match channels
                .messaging
                .tx_messaging_to_refill
                .send(InternalCommand::ResetAllErrors)
            {
                Ok(_) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "test_refill_conditions_historic_data_error_recent_refill_event:\
                        error when trying to reset refill errors from previous test runs: {e:?}"
                    );
                }
            }

            // wait for the test object to trigger water injection
            println!("waiting for water injection...");
            let _ = rx_test_environment_from_mock_water_injection
                .blocking_recv()
                .unwrap();

            spin_sleeper.sleep(sleep_duration_two_secs);

            // request update of tank level position signals to mock tank level switch
            println!("updating tank level switch...");
            {
                let mut tank_level_switch_signals =
                    mutex_tank_level_switch_signals_clone_for_test_environment
                        .lock()
                        .unwrap();
                tank_level_switch_signals.tank_level_switch_position = true;
                tank_level_switch_signals.tank_level_switch_position_stabilized = true;
            }

            spin_sleeper.sleep(sleep_duration_two_secs);

            println!("sending quit signal to test object...");
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_refill
                .blocking_recv()
                .unwrap();
            let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);

            // wait for the test object to finish so that mock threads receive
            // the termination request before the end of this (test environment) thread
            spin_sleeper.sleep(sleep_duration_two_secs);
        })
        .unwrap();

    // give the test environment a little time to trigger the receive-call for signal update
    spin_sleeper.sleep(sleep_duration_50_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let mut tx_refill_to_schedule_check_for_test_case_finish =
                channels.refill.tx_refill_to_schedule_check.clone();

            execute_blocking(
                &mut refill,
                mutex_device_scheduler,
                &mut channels.refill,
                &mut mock_water_injection,
                mutex_tank_level_switch_signals,
                mutex_refill_status,
                Box::new(sql_interface_refill),
            );

            // check if there was no water injection
            assert!(!mock_water_injection.injection_recorder.is_empty());

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
        })
        .unwrap();

    join_handle_mock_monitors
        .join()
        .expect("Mock monitor check did not finish.");
    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// This test case executes multiple iterations of the loop inside .execute function with error flag error_sql_update_failed
// The refill control shall not initiate a refill operation.
// Test case uses test database #23.
/// [itest->swreq~water_db_failure_inhibition~1]
pub fn test_refill_conditions_error_sql_update_failed() {
    let sleep_duration_50_millis = Duration::from_millis(50);
    let spin_sleeper = SpinSleeper::default();
    let (_, mut refill, mut sql_interface, sql_interface_refill) =
        prepare_refill_tests(None, 1, 1, Some(23));

    // set the error
    refill.refill_errors.error_sql_update_failed = true;

    let mut channels = Channels::new_for_test();

    // test environment-specific channels
    let (
        tx_mock_water_injection_to_test_environment,
        mut rx_test_environment_from_mock_water_injection,
    ) = channel(1, true);
    let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
        channel::<InternalCommand>(1, true);

    // empty the refill table
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let mutex_device_scheduler = Arc::new(tokio::sync::Mutex::new(0));
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
    let refill_status = RefillStatus::default();
    let mutex_refill_status = Arc::new(Mutex::new(refill_status));

    let mut mock_monitors = MockMonitors::new();

    let mut mock_water_injection = MockWaterInjection::new(
        Some(tx_mock_water_injection_to_test_environment),
        RefillErrorStates {
            error_switch_stuck_high: false,
            error_switch_stuck_low: false,
            error_receive_tank_level_switch_position: false,
            error_sql_update_failed: false,
            error_sql_get_historic_data_failed: false,
            error_join_failed: false,
        },
    );

    // thread for mock monitors
    let join_handle_mock_monitors = thread::Builder::new()
        .name("mock_monitor".to_string())
        .spawn(move || {
            mock_monitors.execute(
                Some(channels.monitors.rx_monitors_from_refill),
                None,
                rx_monitors_from_test_environment,
            );
        })
        .unwrap();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_refill,
                &mut channels.schedule_check.rx_schedule_check_from_refill,
                None,
                true,
            );
        })
        .unwrap();

    // thread for mock Test environment
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_two_secs = Duration::from_secs(2);
            let sleep_duration_two_millis = Duration::from_millis(2);
            let spin_sleeper = SpinSleeper::default();

            spin_sleeper.sleep(sleep_duration_two_secs);

            for _ in 0..999 {
                assert!(matches!(
                    rx_test_environment_from_mock_water_injection.try_recv(),
                    Err(AquaChannelError::Empty)
                ));
                spin_sleeper.sleep(sleep_duration_two_millis);
            }

            println!("sending quit signal to test object...");
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_refill
                .blocking_recv()
                .unwrap();
            let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);

            // wait for the test object to finish so that mock threads receive
            // the termination request before the end of this (test environment) thread
            spin_sleeper.sleep(sleep_duration_two_secs);
        })
        .unwrap();

    // give the test environment a little time to trigger the receive-call for signal update
    spin_sleeper.sleep(sleep_duration_50_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let sleep_duration_two_millis = Duration::from_millis(2);
            let spin_sleeper = SpinSleeper::default();

            let mut tx_refill_to_schedule_check_for_test_case_finish =
                channels.refill.tx_refill_to_schedule_check.clone();

            execute_blocking(
                &mut refill,
                mutex_device_scheduler,
                &mut channels.refill,
                &mut mock_water_injection,
                mutex_tank_level_switch_signals,
                mutex_refill_status,
                Box::new(sql_interface_refill),
            );

            // check if there was no water injection
            assert!(mock_water_injection.injection_recorder.is_empty());

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

            spin_sleeper.sleep(sleep_duration_two_millis);
        })
        .unwrap();

    join_handle_mock_monitors
        .join()
        .expect("Mock monitor check did not finish.");
    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// This test case checks the process_external_request implementation if it correctly
// processes the ResetAllErrors command.
// The test case checks the struct attribute after having executed the function.
// Test case uses test database #24.
pub fn test_refill_process_external_request_reset_all_errors_command() {
    // set the errors
    let refill_errors = RefillErrorStates {
        error_switch_stuck_high: true,
        error_switch_stuck_low: true,
        error_receive_tank_level_switch_position: true,
        error_sql_update_failed: true,
        error_sql_get_historic_data_failed: true,
        error_join_failed: false,
    };

    let (_, mut refill, mut sql_interface, sql_interface_refill) =
        prepare_refill_tests(Some(refill_errors), 2, 60, Some(24));

    let sleep_duration_50_millis = Duration::from_millis(50);
    let spin_sleeper = SpinSleeper::default();

    let mut channels = Channels::new_for_test();

    let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
        channel::<InternalCommand>(1, true);
    let (
        tx_mock_water_injection_to_test_environment,
        _rx_test_environment_from_mock_water_injection,
    ) = channel(1, true);

    let mut mock_monitors = MockMonitors::new();

    // empty the refill table
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let mutex_device_scheduler = Arc::new(tokio::sync::Mutex::new(0));
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
    let refill_status = RefillStatus::default();
    let mutex_refill_status = Arc::new(Mutex::new(refill_status));

    let mut mock_water_injection = MockWaterInjection::new(
        Some(tx_mock_water_injection_to_test_environment),
        RefillErrorStates {
            error_switch_stuck_high: false,
            error_switch_stuck_low: false,
            error_receive_tank_level_switch_position: false,
            error_sql_update_failed: false,
            error_sql_get_historic_data_failed: false,
            error_join_failed: false,
        },
    );

    // thread for mock monitors
    let join_handle_mock_monitors = thread::Builder::new()
        .name("mock_monitor".to_string())
        .spawn(move || {
            mock_monitors.execute(
                Some(channels.monitors.rx_monitors_from_refill),
                None,
                rx_monitors_from_test_environment,
            );
        })
        .unwrap();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_refill,
                &mut channels.schedule_check.rx_schedule_check_from_refill,
                None,
                true,
            );
        })
        .unwrap();

    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_two_secs = Duration::from_secs(2);
            let spin_sleeper = SpinSleeper::default();

            spin_sleeper.sleep(sleep_duration_two_secs);
            let result = channels
                .messaging
                .tx_messaging_to_refill
                .send(InternalCommand::ResetAllErrors);
            assert!(result.is_ok());
            spin_sleeper.sleep(sleep_duration_two_secs);

            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_refill
                .blocking_recv()
                .unwrap();
            let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);

            // wait for the test object to finish so that mock threads receive
            // the termination request before the end of this (test environment) thread
            spin_sleeper.sleep(sleep_duration_two_secs);
        })
        .unwrap();

    // give the test environment a little time to trigger the receive-call for signal update
    spin_sleeper.sleep(sleep_duration_50_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let sleep_duration_two_millis = Duration::from_millis(2);
            let spin_sleeper = SpinSleeper::default();

            // check if errors have been set
            assert!(refill.refill_errors.has_error());

            let mut tx_refill_to_schedule_check_for_test_case_finish =
                channels.refill.tx_refill_to_schedule_check.clone();

            execute_blocking(
                &mut refill,
                mutex_device_scheduler,
                &mut channels.refill,
                &mut mock_water_injection,
                mutex_tank_level_switch_signals,
                mutex_refill_status,
                Box::new(sql_interface_refill),
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

            println!("{}", refill.refill_errors);

            // check if errors have been reset
            assert!(!refill.refill_errors.has_error());

            spin_sleeper.sleep(sleep_duration_two_millis);
        })
        .unwrap();

    join_handle_mock_monitors
        .join()
        .expect("Mock monitor check did not finish.");
    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// Executes loop of the .execute function manipulating the sensor signals only and giving a negative schedule check.
// Checks if the test object correctly processes the negative schedule check result.
// Test case uses test database #25.
/// [itest->swreq~water_observe_schedule~1]
pub fn test_refill_blocked_by_schedule() {
    let sleep_duration_50_millis = Duration::from_millis(50);
    let spin_sleeper = SpinSleeper::default();
    let (_, mut refill, mut sql_interface, sql_interface_refill) =
        prepare_refill_tests(None, 1, 1, Some(25));

    let mut channels = Channels::new_for_test();

    let (
        tx_mock_water_injection_to_test_environment,
        mut rx_test_environment_from_mock_water_injection,
    ) = channel(1, true);
    let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
        channel::<InternalCommand>(1, true);

    // empty the refill table
    match SqlInterface::truncate_table(&mut sql_interface, "refill_log".to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let mutex_device_scheduler = Arc::new(tokio::sync::Mutex::new(0));
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
    let refill_status = RefillStatus::default();
    let mutex_refill_status = Arc::new(Mutex::new(refill_status));

    let mut mock_monitors = MockMonitors::new();

    let mut mock_water_injection = MockWaterInjection::new(
        Some(tx_mock_water_injection_to_test_environment),
        RefillErrorStates {
            error_switch_stuck_high: false,
            error_switch_stuck_low: false,
            error_receive_tank_level_switch_position: false,
            error_sql_update_failed: false,
            error_sql_get_historic_data_failed: false,
            error_join_failed: false,
        },
    );

    // thread for mock monitors
    let join_handle_mock_monitors = thread::Builder::new()
        .name("mock_monitor".to_string())
        .spawn(move || {
            mock_monitors.execute(
                Some(channels.monitors.rx_monitors_from_refill),
                None,
                rx_monitors_from_test_environment,
            );
        })
        .unwrap();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_refill,
                &mut channels.schedule_check.rx_schedule_check_from_refill,
                None,
                false,
            );
        })
        .unwrap();

    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_two_secs = Duration::from_secs(2);
            let sleep_duration_two_millis = Duration::from_millis(2);
            let spin_sleeper = SpinSleeper::default();

            spin_sleeper.sleep(sleep_duration_two_secs);

            for _ in 0..999 {
                assert!(matches!(
                    rx_test_environment_from_mock_water_injection.try_recv(),
                    Err(AquaChannelError::Empty)
                ));
                spin_sleeper.sleep(sleep_duration_two_millis);
            }

            println!("sending quit signal to test object...");
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_refill
                .blocking_recv()
                .unwrap();
            let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);

            // wait for the test object to finish so that mock threads receive
            // the termination request before the end of this (test environment) thread
            spin_sleeper.sleep(sleep_duration_two_secs);
        })
        .unwrap();

    // give the test environment a little time to trigger the receive-call for signal update
    spin_sleeper.sleep(sleep_duration_50_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let sleep_duration_two_millis = Duration::from_millis(2);
            let spin_sleeper = SpinSleeper::default();

            let mut tx_refill_to_schedule_check_for_test_case_finish =
                channels.refill.tx_refill_to_schedule_check.clone();

            execute_blocking(
                &mut refill,
                mutex_device_scheduler,
                &mut channels.refill,
                &mut mock_water_injection,
                mutex_tank_level_switch_signals,
                mutex_refill_status,
                Box::new(sql_interface_refill),
            );

            // check if there was no water injection
            assert!(mock_water_injection.injection_recorder.is_empty());

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

            spin_sleeper.sleep(sleep_duration_two_millis);
        })
        .unwrap();

    join_handle_mock_monitors
        .join()
        .expect("Mock monitor check did not finish.");
    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// Test case executes the loop of the .execute function manipulating the sensor signals only
// and giving a positive schedule check.
// Test case checks if the test object correctly processes the inhibition requested via the channel.
// Test case uses test database #26.
pub fn test_messaging_stops_starts_refill() {
    let sleep_duration_50_millis = Duration::from_millis(50);
    let spin_sleeper = SpinSleeper::default();
    let (_, mut refill, mut sql_interface, sql_interface_refill) =
        prepare_refill_tests(None, 10, 60, Some(26));

    let mut channels = Channels::new_for_test();

    let (
        tx_mock_water_injection_to_test_environment,
        mut rx_test_environment_from_mock_water_injection,
    ) = channel(1, true);
    let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
        channel::<InternalCommand>(1, true);

    // empty the refill table
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let mut mock_monitors = MockMonitors::new();

    let mutex_device_scheduler = Arc::new(tokio::sync::Mutex::new(0));
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
    let mutex_tank_level_switch_signals_clone_for_test_environment =
        mutex_tank_level_switch_signals.clone();
    let refill_status = RefillStatus::default();
    let mutex_refill_status = Arc::new(Mutex::new(refill_status));

    let mut mock_water_injection = MockWaterInjection::new(
        Some(tx_mock_water_injection_to_test_environment),
        RefillErrorStates {
            error_switch_stuck_high: false,
            error_switch_stuck_low: false,
            error_receive_tank_level_switch_position: false,
            error_sql_update_failed: false,
            error_sql_get_historic_data_failed: false,
            error_join_failed: false,
        },
    );

    // thread for mock monitors
    let join_handle_mock_monitors = thread::Builder::new()
        .name("mock_monitor".to_string())
        .spawn(move || {
            mock_monitors.execute(
                Some(channels.monitors.rx_monitors_from_refill),
                None,
                rx_monitors_from_test_environment,
            );
        })
        .unwrap();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_refill,
                &mut channels.schedule_check.rx_schedule_check_from_refill,
                None,
                true,
            );
        })
        .unwrap();

    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_one_sec = Duration::from_secs(1);
            let sleep_duration_two_secs = Duration::from_secs(2);
            let sleep_duration_two_millis = Duration::from_millis(2);
            let spin_sleeper = SpinSleeper::default();

            spin_sleeper.sleep(sleep_duration_one_sec);

            match channels
                .messaging
                .tx_messaging_to_refill
                .send(InternalCommand::Stop)
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: could not send stop command to refill ({e:?})",
                        module_path!()
                    );
                }
            }

            spin_sleeper.sleep(sleep_duration_one_sec);

            {
                let mut tank_level_switch_signals =
                    mutex_tank_level_switch_signals_clone_for_test_environment
                        .lock()
                        .unwrap();
                tank_level_switch_signals.tank_level_switch_position = false;
                tank_level_switch_signals.tank_level_switch_position_stabilized = false;
            }

            // expect to see no event
            for _ in 0..999 {
                assert!(matches!(
                    rx_test_environment_from_mock_water_injection.try_recv(),
                    Err(AquaChannelError::Empty)
                ));
                spin_sleeper.sleep(sleep_duration_two_millis);
            }

            spin_sleeper.sleep(sleep_duration_one_sec);

            match channels
                .messaging
                .tx_messaging_to_refill
                .send(InternalCommand::Start)
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: could not send start command to refill ({e:?})",
                        module_path!()
                    );
                }
            }

            // expect to see one event
            assert!(matches!(
                rx_test_environment_from_mock_water_injection.blocking_recv(),
                Ok(TestCommand::InformWaterInjectionEvent)
            ));

            println!("sending quit signal to test object...");
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_refill
                .blocking_recv()
                .unwrap();
            let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);

            // wait for the test object to finish so that mock threads receive
            // the termination request before the end of this (test environment) thread
            spin_sleeper.sleep(sleep_duration_two_secs);
        })
        .unwrap();

    // give the test environment a little time to trigger the receive-call for signal update
    spin_sleeper.sleep(sleep_duration_50_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let sleep_duration_two_millis = Duration::from_millis(2);
            let spin_sleeper = SpinSleeper::default();

            let mut tx_refill_to_schedule_check_for_test_case_finish =
                channels.refill.tx_refill_to_schedule_check.clone();

            execute_blocking(
                &mut refill,
                mutex_device_scheduler,
                &mut channels.refill,
                &mut mock_water_injection,
                mutex_tank_level_switch_signals,
                mutex_refill_status,
                Box::new(sql_interface_refill),
            );

            // check if there was only one water injection
            assert_eq!(mock_water_injection.injection_recorder.len(), 1);

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

            spin_sleeper.sleep(sleep_duration_two_millis);
        })
        .unwrap();

    join_handle_mock_monitors
        .join()
        .expect("Mock monitor check did not finish.");
    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// This test case executes multiple iterations of the loop inside .execute function
// using a mock version of SQL interface which returns only errors.
// The refill control shall not initiate a refill operation.
// Test case uses test database #27.
pub fn test_refill_conditions_error_get_historic_data_failed() {
    let sleep_duration_50_millis = Duration::from_millis(50);
    let spin_sleeper = SpinSleeper::default();
    let (_, mut refill, sql_interface_refill) =
        prepare_refill_tests_with_mock_database_interface(None, 1, 1, 27);
    // this test case does not use the SQL database, so no conflict arises
    // when concurrently executing other test cases on the same DB.

    let mut channels = Channels::new_for_test();

    let (
        tx_mock_water_injection_to_test_environment,
        mut rx_test_environment_from_mock_water_injection,
    ) = channel(1, true);
    let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
        channel::<InternalCommand>(1, true);

    let mutex_device_scheduler = Arc::new(tokio::sync::Mutex::new(0));
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
    let refill_status = RefillStatus::default();
    let mutex_refill_status = Arc::new(Mutex::new(refill_status));

    let mut mock_monitors = MockMonitors::new();

    let mut mock_water_injection = MockWaterInjection::new(
        Some(tx_mock_water_injection_to_test_environment),
        RefillErrorStates {
            error_switch_stuck_high: false,
            error_switch_stuck_low: false,
            error_receive_tank_level_switch_position: false,
            error_sql_update_failed: false,
            error_sql_get_historic_data_failed: false,
            error_join_failed: false,
        },
    );

    // thread for mock monitors
    let join_handle_mock_monitors = thread::Builder::new()
        .name("mock_monitor".to_string())
        .spawn(move || {
            mock_monitors.execute(
                Some(channels.monitors.rx_monitors_from_refill),
                None,
                rx_monitors_from_test_environment,
            );
        })
        .unwrap();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_refill,
                &mut channels.schedule_check.rx_schedule_check_from_refill,
                None,
                true,
            );
        })
        .unwrap();

    // thread for mock Test environment
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_two_secs = Duration::from_secs(2);
            let sleep_duration_two_millis = Duration::from_millis(2);
            let spin_sleeper = SpinSleeper::default();

            spin_sleeper.sleep(sleep_duration_two_secs);

            for _ in 0..999 {
                assert!(matches!(
                    rx_test_environment_from_mock_water_injection.try_recv(),
                    Err(AquaChannelError::Empty)
                ));
                spin_sleeper.sleep(sleep_duration_two_millis);
            }

            println!("sending quit signal to test object...");
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_refill
                .blocking_recv()
                .unwrap();
            let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);

            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);

            // wait for the test object to finish so that mock threads receive
            // the termination request before the end of this (test environment) thread
            spin_sleeper.sleep(sleep_duration_two_secs);
        })
        .unwrap();

    // give the test environment a little time to trigger the receive-call for signal update
    spin_sleeper.sleep(sleep_duration_50_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let sleep_duration_two_millis = Duration::from_millis(2);
            let spin_sleeper = SpinSleeper::default();

            let mut tx_refill_to_schedule_check_for_test_case_finish =
                channels.refill.tx_refill_to_schedule_check.clone();

            execute_blocking(
                &mut refill,
                mutex_device_scheduler,
                &mut channels.refill,
                &mut mock_water_injection,
                mutex_tank_level_switch_signals,
                mutex_refill_status,
                Box::new(sql_interface_refill),
            );

            // check if there was no water injection
            assert!(mock_water_injection.injection_recorder.is_empty());

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

            spin_sleeper.sleep(sleep_duration_two_millis);
        })
        .unwrap();

    join_handle_mock_monitors
        .join()
        .expect("Mock monitor check did not finish.");
    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// This test case executes the check_volume_count_time_limits function of refill.
// It also instantiates Monitor to capture the monitor view sent via the channel.
/// [itest->swreq~water_faulty_switch_protection~1]
pub fn test_refill_check_volume_count_time_limits() {
    let mut mock_monitors = MockMonitors::new();
    let sleep_duration_200_millis = Duration::from_millis(200);
    let spin_sleeper = SpinSleeper::default();

    let mut channels = Channels::new_for_test();

    let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
        channel::<InternalCommand>(1, true);

    // thread for mock monitors
    let join_handle_mock_monitors = thread::Builder::new()
        .name("mock_monitor".to_string())
        .spawn(move || {
            mock_monitors.execute(
                Some(channels.monitors.rx_monitors_from_refill),
                None,
                rx_monitors_from_test_environment,
            );
            println!("{}", mock_monitors);
            let (_, refill_monitor_view) = mock_monitors.refill_monitor_views.pop().unwrap();
            refill_monitor_view.assert_refill_monitor_view(false, false, false, false, false);

            let (_, refill_monitor_view) = mock_monitors.refill_monitor_views.pop().unwrap();
            refill_monitor_view.assert_refill_monitor_view(false, false, false, false, true);

            let (_, refill_monitor_view) = mock_monitors.refill_monitor_views.pop().unwrap();
            refill_monitor_view.assert_refill_monitor_view(false, false, true, false, false);

            let (_, refill_monitor_view) = mock_monitors.refill_monitor_views.pop().unwrap();
            refill_monitor_view.assert_refill_monitor_view(true, false, false, false, false);

            let (_, refill_monitor_view) = mock_monitors.refill_monitor_views.pop().unwrap();
            refill_monitor_view.assert_refill_monitor_view(false, false, false, true, false);

            let (_, refill_monitor_view) = mock_monitors.refill_monitor_views.pop().unwrap();
            refill_monitor_view.assert_refill_monitor_view(false, true, false, false, false);

            let (_, refill_monitor_view) = mock_monitors.refill_monitor_views.pop().unwrap();
            refill_monitor_view.assert_refill_monitor_view(false, false, false, false, false);
        })
        .unwrap();

    let (config, mut refill, _sql_interface_refill) =
        prepare_refill_tests_with_mock_database_interface(None, 1, 1, 27);
    // this test case does not use the SQL database, so no conflict arises
    // when concurrently executing other test cases on the same DB.

    let mut sql_refill_stats_data = RefillStatsData {
        duration_since_last_refill: config.refill.min_interval_last_refill + 1,
        ..Default::default()
    };

    // first execution without any errors
    assert!(!refill.check_volume_count_time_limits(&sql_refill_stats_data, &mut channels.refill));
    spin_sleeper.sleep(sleep_duration_200_millis);

    // induce errors before executing again
    sql_refill_stats_data.refill_count_last_24h = config.refill.max_refill_count_24h + 1;

    assert!(refill.check_volume_count_time_limits(&sql_refill_stats_data, &mut channels.refill));
    spin_sleeper.sleep(sleep_duration_200_millis);

    // reset the previous error and induce other error before executing again
    sql_refill_stats_data.refill_count_last_24h = 0;
    sql_refill_stats_data.refill_count_last_hour = config.refill.max_refill_count_hour + 1;

    assert!(refill.check_volume_count_time_limits(&sql_refill_stats_data, &mut channels.refill));
    spin_sleeper.sleep(sleep_duration_200_millis);

    // reset the previous error and induce other error before executing again
    sql_refill_stats_data.refill_count_last_hour = 0;
    sql_refill_stats_data.refill_volume_last_24h = config.refill.max_refill_volume_24h + 1.0;

    assert!(refill.check_volume_count_time_limits(&sql_refill_stats_data, &mut channels.refill));
    spin_sleeper.sleep(sleep_duration_200_millis);

    // reset the previous error and induce other error before executing again
    sql_refill_stats_data.refill_volume_last_24h = 0.0;
    sql_refill_stats_data.refill_volume_last_hour = config.refill.max_refill_volume_hour + 1.0;

    assert!(refill.check_volume_count_time_limits(&sql_refill_stats_data, &mut channels.refill));
    spin_sleeper.sleep(sleep_duration_200_millis);

    // reset the previous error and induce other error before executing again
    sql_refill_stats_data.refill_volume_last_hour = 0.0;
    sql_refill_stats_data.duration_since_last_refill = config.refill.min_interval_last_refill - 1;

    assert!(refill.check_volume_count_time_limits(&sql_refill_stats_data, &mut channels.refill));
    spin_sleeper.sleep(sleep_duration_200_millis);

    // reset the previous error and do not induce any further error
    sql_refill_stats_data.duration_since_last_refill = config.refill.min_interval_last_refill + 1;
    assert!(!refill.check_volume_count_time_limits(&sql_refill_stats_data, &mut channels.refill));
    spin_sleeper.sleep(sleep_duration_200_millis);

    let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);
    // give thread time to finish
    spin_sleeper.sleep(sleep_duration_200_millis);

    join_handle_mock_monitors
        .join()
        .expect("Mock monitor check did not finish.");
}

#[test]
// Verifies that water refill injection is inhibited when device_scheduler mutex is blocked.
// The test environment thread acquires and holds the device_scheduler mutex lock before water injection occurs.
// While the lock is held, water injection is blocked. Once released, water injection proceeds normally.
// Test case uses test database #68.
/// [itest->swreq~water_actuate_pump~1]
pub fn test_refill_trigger_water_injection_with_mutex_blocked() {
    let sleep_duration_50_millis = Duration::from_millis(50);
    let spin_sleeper = SpinSleeper::default();

    let (_, mut refill, mut sql_interface, sql_interface_refill) =
        prepare_refill_tests(None, 1, 1, Some(68));

    let mut channels = Channels::new_for_test();

    // test environment-specific channels
    let (
        tx_mock_water_injection_to_test_environment,
        mut rx_test_environment_from_mock_water_injection,
    ) = channel(1, true);
    let (mut tx_test_environment_to_monitors, rx_monitors_from_test_environment) =
        channel::<InternalCommand>(1, true);

    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
    let mutex_tank_level_switch_signals_for_refill = mutex_tank_level_switch_signals.clone();
    let refill_status = RefillStatus::default();
    let mutex_refill_status = Arc::new(Mutex::new(refill_status));

    let mut mock_monitors = MockMonitors::new();

    // empty the refill table
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let mutex_device_scheduler = Arc::new(tokio::sync::Mutex::new(0));
    let mutex_device_scheduler_environment = mutex_device_scheduler.clone();

    let mut mock_water_injection = MockWaterInjection::new(
        Some(tx_mock_water_injection_to_test_environment),
        RefillErrorStates {
            error_switch_stuck_high: false,
            error_switch_stuck_low: false,
            error_receive_tank_level_switch_position: false,
            error_sql_update_failed: false,
            error_sql_get_historic_data_failed: false,
            error_join_failed: false,
        },
    );

    // thread for mock monitors
    let join_handle_mock_monitors = thread::Builder::new()
        .name("mock_monitor".to_string())
        .spawn(move || {
            mock_monitors.execute(
                Some(channels.monitors.rx_monitors_from_refill),
                None,
                rx_monitors_from_test_environment,
            );
        })
        .unwrap();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_refill,
                &mut channels.schedule_check.rx_schedule_check_from_refill,
                None,
                true,
            );
        })
        .unwrap();

    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_two_secs = Duration::from_secs(2);
            let spin_sleeper = SpinSleeper::default();

            // 1. Acquire lock on device_scheduler mutex before refill injection executes
            let lock_guard = mutex_device_scheduler_environment.blocking_lock();

            // Wait 2 seconds while holding the lock
            spin_sleeper.sleep(Duration::from_secs(2));

            // Verify execution count remains 0 while lock is held
            assert_eq!(*lock_guard, 0);

            // 2. Release lock so refill can proceed
            drop(lock_guard);

            // wait for the test object to trigger water injection
            let _ = rx_test_environment_from_mock_water_injection
                .blocking_recv()
                .unwrap();

            // update of mock tank level position signals
            {
                let mut mutex_tank_level_switch_signals =
                    mutex_tank_level_switch_signals.lock().unwrap();
                mutex_tank_level_switch_signals.tank_level_switch_position_stabilized = true;
                mutex_tank_level_switch_signals.tank_level_switch_position = true;
            }

            let start_time = Instant::now();
            spin_sleeper.sleep(sleep_duration_two_secs);

            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_refill
                .blocking_recv()
                .unwrap();

            let _ = tx_test_environment_to_monitors.send(InternalCommand::Quit);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);
            let finish_time = Instant::now();
            let finalization_duration_millis = finish_time.duration_since(start_time).as_millis();

            assert_ge!(finalization_duration_millis, 2000u128);
            assert_le!(finalization_duration_millis, 2150u128);

            // wait for the test object to finish
            spin_sleeper.sleep(sleep_duration_two_secs);
        })
        .unwrap();

    // give the test environment a little time to trigger the receive-call for signal update
    spin_sleeper.sleep(sleep_duration_50_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let mut tx_refill_to_schedule_check_for_test_case_finish =
                channels.refill.tx_refill_to_schedule_check.clone();

            execute_blocking(
                &mut refill,
                mutex_device_scheduler,
                &mut channels.refill,
                &mut mock_water_injection,
                mutex_tank_level_switch_signals_for_refill,
                mutex_refill_status,
                Box::new(sql_interface_refill),
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
        })
        .unwrap();

    join_handle_mock_monitors
        .join()
        .expect("Mock monitor check did not finish.");
    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}
