/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the main control logic for automated Balling mineral dosing.

use std::sync::{Arc, Mutex};
use std::thread::scope;
use std::time::{Duration, Instant};
use spin_sleep::SpinSleeper;
use aquarium_control::database::sql_interface::SqlInterface;
use aquarium_control::database::sql_interface_balling::SqlInterfaceBalling;
use aquarium_control::database::sql_interface_error::SqlInterfaceError;
use aquarium_control::database::sql_test_query_strings::{SQL_TABLE_BALLING_DOSING_LOG, SQL_TABLE_BALLING_SET_VALS};
use aquarium_control::launch::channels::Channels;
use aquarium_control::mineral::balling::Balling;
use aquarium_control::mineral::balling_channels::BallingChannels;
use aquarium_control::mineral::balling_error::BallingError;
use aquarium_control::mineral::mineral_injection::MineralInjectionTrait;
use aquarium_control::mocks::mock_mineral_injection::tests::MockMineralInjection;
use aquarium_control::mocks::mock_schedule_check::tests::mock_schedule_check;
use aquarium_control::utilities::channel_content::AquariumDevice::{PeristalticPump1, PeristalticPump2, PeristalticPump3, PeristalticPump4};
use aquarium_control::utilities::channel_content::InternalCommand;
use aquarium_control::utilities::config::{read_config_file, read_config_file_with_test_database, ConfigData};

/// Helper function to execute the balling task inside a test Tokio runtime.
///
/// # Arguments
/// * `balling` - A mutable reference to the `Balling` instance.
/// * `mutex_device_scheduler_balling` - Mutex coordinating access to the device scheduler.
/// * `balling_channels` - A mutable reference to the `BallingChannels` instance.
/// * `mineral_injection` - A mutable reference to the mineral injector instance.
/// * `sql_interface_balling` - Database interface for Balling operations.
fn execute_blocking(
    balling: &mut Balling,
    mutex_device_scheduler_balling: Arc<Mutex<i32>>,
    balling_channels: &mut BallingChannels,
    mineral_injection: &mut (impl MineralInjectionTrait + Send),
    sql_interface_balling: SqlInterfaceBalling,
) {
    tokio::runtime::Builder::new_multi_thread()
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

            balling
                .execute(
                    mutex_device_scheduler_balling,
                    balling_channels,
                    mineral_injection,
                    sql_interface_balling,
                    is_multi_thread_test,
                )
                .await;
        });
}
/// Performs the `read_balling_config` operation.
fn read_balling_config() -> ConfigData {
    read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap()
}

/// Performs the `read_balling_config_with_database` operation.
///
/// # Arguments
/// * `db_number` - A parameter representing `db_number`.
fn read_balling_config_with_database(db_number: u32) -> ConfigData {
    read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        db_number,
    )
}

// Helper function to prepare for test cases which require a filled dosing log.
// This function inserts dosing events for each of the four pumps.
//
// Arguments:
// - `sql_interface`: A mutable reference to the main `SqlInterface` for database operations,
//                    used here specifically for truncating the dosing log table.
// - `sql_interface_balling`: A mutable reference to the `SqlInterfaceBalling`,
//                            used for inserting balling dosing events.
//
// The time of the dosing events is set to:
// * 100 seconds in the past for pump #1
// * 200 seconds in the past for pump #2
// * 300 seconds in the past for pump #3
// * 400 seconds in the past for pump #4
//
// All SQL operations are expected to succeed; any failure will cause a panic,
// indicating a test setup issue.
fn insert_past_dosing_events(
    sql_interface: &mut SqlInterface,
    sql_interface_balling: &mut SqlInterfaceBalling,
) {
    // empty the Balling dosing log table
    match SqlInterface::truncate_table(sql_interface, SQL_TABLE_BALLING_DOSING_LOG.to_string())
    {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }
    let naive_datetime_100secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 100);
    let naive_datetime_200secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 200);
    let naive_datetime_300secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 300);
    let naive_datetime_400secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 400);

    // insert balling dosing events
    sql_interface_balling
        .insert_balling_event_for_test(naive_datetime_100secs_ago, 1, 0.5)
        .expect("Could not insert Balling dosing event for pump #1 into database");

    sql_interface_balling
        .insert_balling_event_for_test(naive_datetime_200secs_ago, 2, 0.5)
        .expect("Could not insert Balling dosing event for pump #2 into database");

    sql_interface_balling
        .insert_balling_event_for_test(naive_datetime_300secs_ago, 3, 0.5)
        .expect("Could not insert Balling dosing event for pump #3 into database");

    sql_interface_balling
        .insert_balling_event_for_test(naive_datetime_400secs_ago, 4, 0.5)
        .expect("Could not insert Balling dosing event for pump #4 into database");
}

// Helper function to prepare for test cases which require balling pump configuration.
// This function first truncates the pump configuration table and then
// inserts a balling pump configuration into the database for each of the four pumps.
//
// Arguments:
// - `sql_interface`: A mutable reference to the main `SqlInterface` for database operations,
//                    used here specifically for truncating the balling set values table.
// - `sql_interface_balling`: A mutable reference to the `SqlInterfaceBalling`,
//                            used for inserting balling set values.
// - `pump1`: flag indicating that a pump configuration for pump #1 should be inserted.
// - `pump2`: flag indicating that a pump configuration for pump #2 should be inserted.
// - `pump3`: flag indicating that a pump configuration for pump #3 should be inserted.
// - `pump4`: flag indicating that a pump configuration for pump #4 should be inserted.
//
// All SQL operations are expected to succeed; any failure will cause a panic,
// indicating a test setup issue.
fn insert_pump_configurations(
    sql_interface: &mut SqlInterface,
    sql_interface_balling: &mut SqlInterfaceBalling,
    pump1: bool,
    pump2: bool,
    pump3: bool,
    pump4: bool,
) {
    // empty the Balling pump configuration table
    match SqlInterface::truncate_table(sql_interface, SQL_TABLE_BALLING_SET_VALS.to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }
    if pump1 {
        match sql_interface_balling.insert_pump_configuration(1, 4.0, 1.0, "pump1".to_string())
        {
            Ok(()) => { /* do nothing */ }
            Err(e) => {
                panic!(
                    "{}: Could not insert pump 1 configuration ({e:?})",
                    module_path!(),
                );
            }
        }
    }
    if pump2 {
        match sql_interface_balling.insert_pump_configuration(2, 4.0, 1.0, "pump2".to_string())
        {
            Ok(()) => { /* do nothing */ }
            Err(e) => {
                panic!(
                    "{}: Could not insert pump 2 configuration ({e:?})",
                    module_path!(),
                );
            }
        }
    }
    if pump3 {
        match sql_interface_balling.insert_pump_configuration(3, 4.0, 1.0, "pump3".to_string())
        {
            Ok(()) => { /* do nothing */ }
            Err(e) => {
                panic!(
                    "{}: Could not insert pump 3 configuration ({e:?})",
                    module_path!(),
                );
            }
        }
    }
    if pump4 {
        match sql_interface_balling.insert_pump_configuration(4, 4.0, 1.0, "pump4".to_string())
        {
            Ok(()) => { /* do nothing */ }
            Err(e) => {
                panic!(
                    "{}: Could not insert pump 4 configuration ({e:?})",
                    module_path!(),
                );
            }
        }
    }
}

#[test]
// This test case checks if the configuration file contains a valid value for the dosing interval.
// It does not operate on any data from SQL database.
pub fn test_balling_initialization_invalid_intervals() {
    let mut config: ConfigData = read_balling_config();

    config.balling.dosing_interval_pump1 = 0;
    config.balling.dosing_interval_pump2 = 0;
    config.balling.dosing_interval_pump3 = 0;
    config.balling.dosing_interval_pump4 = 0;

    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
    let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
    let sql_interface = match SqlInterface::new(config.sql_interface) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };
    let mut sql_interface_balling = SqlInterfaceBalling::new(
        sql_interface.get_connection().unwrap(),
        max_rows_balling_set_values,
        max_rows_balling_dosing_log,
    )
        .unwrap();

    // create the test object
    let test_result = Balling::new(
        config.balling,
        Duration::from_millis(100),
        &mut sql_interface_balling,
    );
    match test_result {
        Ok(balling_controller) => {
            panic!(
                "Successfully initialized Balling dosing, although error expected: {}",
                balling_controller
            );
        }
        Err(e) => {
            println!("Balling initialization correctly returned error: {e:?}. Checking if error type is correct...");
            assert!(matches!(e, BallingError::InvalidDosingInterval(_, _)));
        }
    }
}

#[test]
// When starting, the application checks when the last dosing events took place by analyzing
// data from the database. As a prerequisite for this test case, an empty dosing log is required.
// Test case uses test database #01.
pub fn test_balling_initialization_empty_dosing_log() {
    let config: ConfigData = read_balling_config_with_database(1);

    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
    let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
    let mut sql_interface = match SqlInterface::new(config.sql_interface) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };
    let mut sql_interface_balling = SqlInterfaceBalling::new(
        sql_interface.get_connection().unwrap(),
        max_rows_balling_set_values,
        max_rows_balling_dosing_log,
    )
        .unwrap();

    // empty the Balling dosing log table
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_BALLING_DOSING_LOG.to_string())
        .expect("Could not prepare test case");

    // insert balling pump configuration
    insert_pump_configurations(
        &mut sql_interface,
        &mut sql_interface_balling,
        true,
        true,
        true,
        true,
    );

    // create the test object
    let balling = Balling::new(
        config.balling,
        Duration::from_millis(100),
        &mut sql_interface_balling,
    )
        .unwrap();

    // check if the countdown calculation was correct
    assert_eq!(balling.durations_until_next_dosing[0].as_secs(), 0);
    assert_eq!(balling.durations_until_next_dosing[1].as_secs(), 0);
    assert_eq!(balling.durations_until_next_dosing[2].as_secs(), 0);
    assert_eq!(balling.durations_until_next_dosing[3].as_secs(), 0);
}

#[test]
// Test case checks if the countdowns for each pump are calculated correctly using
// a dosing log in the database which has entries for all pumps.
// Additionally, the test case checks the maximum row limitation for the dosing log.
// Test case uses test database #02.
pub fn test_balling_initialization_full_dosing_log() {
    let config: ConfigData = read_balling_config_with_database(2);

    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
    let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
    let mut sql_interface = match SqlInterface::new(config.sql_interface) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };
    let mut sql_interface_balling = SqlInterfaceBalling::new(
        sql_interface.get_connection().unwrap(),
        max_rows_balling_set_values,
        max_rows_balling_dosing_log,
    )
        .unwrap();

    insert_pump_configurations(
        &mut sql_interface,
        &mut sql_interface_balling,
        true,
        true,
        true,
        true,
    );
    insert_past_dosing_events(&mut sql_interface, &mut sql_interface_balling);

    // create the test object
    let balling = Balling::new(
        config.balling,
        Duration::from_millis(100),
        &mut sql_interface_balling,
    )
        .unwrap();

    // check if the countdown calculation was correct:
    // the countdown depends on the pump-specific dosing interval and the timing of the last dosing event.
    assert_eq!(balling.durations_until_next_dosing[0].as_secs(), 100);
    assert_eq!(balling.durations_until_next_dosing[1].as_secs(), 0);
    assert_eq!(balling.durations_until_next_dosing[2].as_secs(), 0);
    assert_eq!(balling.durations_until_next_dosing[3].as_secs(), 0);

    // dosing log has four entries
    let test_result = SqlInterfaceBalling::new(sql_interface.get_connection().unwrap(), 0, 1);
    assert!(matches!(
            test_result,
            Err(SqlInterfaceError::DatabaseBallingDosingLogTableContainsTooManyRows(_, _, _))
        ));
}

#[test]
// This test case checks if the code checks the dosing interval against the check interval.
// It does not operate on any data from SQL database.
pub fn test_balling_initialization_invalid_dosing_check_interval() {
    let mut config: ConfigData = read_balling_config();

    config.balling.dosing_interval_pump1 = 100;
    config.balling.dosing_interval_pump2 = 100;
    config.balling.dosing_interval_pump3 = 100;
    config.balling.dosing_interval_pump4 = 100;
    config.balling.schedule_check_interval = 200;

    let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
    let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
    let mut sql_interface = match SqlInterface::new(config.sql_interface) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };
    match SqlInterface::truncate_table(
        &mut sql_interface,
        SQL_TABLE_BALLING_DOSING_LOG.to_string(),
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }
    let mut sql_interface_balling = SqlInterfaceBalling::new(
        sql_interface.get_connection().unwrap(),
        max_rows_balling_set_values,
        max_rows_balling_dosing_log,
    )
        .unwrap();

    // create the test object
    let test_result = Balling::new(
        config.balling,
        Duration::from_secs(1000),
        &mut sql_interface_balling,
    );
    match test_result {
        Ok(balling_controller) => {
            panic!(
                "Successfully initialized Balling dosing, although error expected: {}",
                balling_controller
            );
        }
        Err(e) => {
            println!("Balling initialization correctly returned error: {e:?}. Checking if error type is correct...");
            assert!(matches!(
                    e,
                    BallingError::DosingIntervalShorterThanCheckInterval(_, _, _, _)
                ));
        }
    }
}

#[test]
// This test case checks that `Balling::new` returns an error when the
// schedule_check_interval is set to zero in the configuration.
pub fn test_balling_initialization_zero_check_interval() {
    let mut config: ConfigData = read_balling_config();
    config.balling.schedule_check_interval = 0;

    let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
    let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
    let mut sql_interface = SqlInterface::new(config.sql_interface).unwrap();
    match SqlInterface::truncate_table(
        &mut sql_interface,
        SQL_TABLE_BALLING_DOSING_LOG.to_string(),
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }
    let mut sql_interface_balling = SqlInterfaceBalling::new(
        sql_interface.get_connection().unwrap(),
        max_rows_balling_set_values,
        max_rows_balling_dosing_log,
    )
        .unwrap();

    // create the test object
    let test_result = Balling::new(
        config.balling,
        Duration::from_secs(1000),
        &mut sql_interface_balling,
    );

    match test_result {
        Ok(balling_controller) => {
            panic!(
                "Successfully initialized Balling dosing, although an error was expected: {}",
                balling_controller
            );
        }
        Err(e) => {
            println!("Balling initialization correctly returned error: {e:?}. Checking if error type is correct...");
            assert!(matches!(e, BallingError::ScheduleCheckIntervalZero(_)));
        }
    }
}

// This function executes the test object and test environment in separate threads.
// The test case depends on the configuration and reference values provided for the asserts.
#[allow(clippy::too_many_arguments)]
pub fn test_balling_dosing(
    test_db_number: u32,
    schedule_allows: bool,
    execution_count_reference: i32,
    duration_millis: u64,
    pump1_engagement_count_reference: usize,
    pump2_engagement_count_reference: usize,
    pump3_engagement_count_reference: usize,
    pump4_engagement_count_reference: usize,
    schedule_check_interval: u32,
    dosing_intervals: Option<(u32, u32, u32, u32)>,
) {
    let sleep_duration_test_environment = Duration::from_millis(duration_millis);
    let spin_sleeper_test_environment = SpinSleeper::default();
    let mut config: ConfigData = read_balling_config_with_database(test_db_number);

    config.balling.pump1_active = pump1_engagement_count_reference > 0;
    config.balling.pump2_active = pump2_engagement_count_reference > 0;
    config.balling.pump3_active = pump3_engagement_count_reference > 0;
    config.balling.pump4_active = pump4_engagement_count_reference > 0;
    config.balling.schedule_check_interval = schedule_check_interval;

    match dosing_intervals {
        Some((
                 dosing_interval_pump1,
                 dosing_interval_pump2,
                 dosing_interval_pump3,
                 dosing_interval_pump4,
             )) => {
            config.balling.dosing_interval_pump1 = dosing_interval_pump1;
            config.balling.dosing_interval_pump2 = dosing_interval_pump2;
            config.balling.dosing_interval_pump3 = dosing_interval_pump3;
            config.balling.dosing_interval_pump4 = dosing_interval_pump4;
        }
        None => { /* do nothing */ }
    }

    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
    let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
    let mut sql_interface = match SqlInterface::new(config.sql_interface) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_BALLING_SET_VALS.to_string())
        .unwrap();
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_BALLING_DOSING_LOG.to_string())
        .unwrap();

    let mut sql_interface_balling = SqlInterfaceBalling::new(
        sql_interface.get_connection().unwrap(),
        max_rows_balling_set_values,
        max_rows_balling_dosing_log,
    )
        .unwrap();

    insert_pump_configurations(
        &mut sql_interface,
        &mut sql_interface_balling,
        pump1_engagement_count_reference > 0,
        pump2_engagement_count_reference > 0,
        pump3_engagement_count_reference > 0,
        pump4_engagement_count_reference > 0,
    );
    insert_past_dosing_events(&mut sql_interface, &mut sql_interface_balling);

    let mut channels = Channels::new_for_test();

    let mutex_device_scheduler = Arc::new(Mutex::new(0));
    let mutex_device_scheduler_environment = mutex_device_scheduler.clone();

    // create the test object
    let mut balling = Balling::new(
        config.balling,
        Duration::from_millis(100),
        &mut sql_interface_balling,
    )
        .unwrap();

    let mut mock_mineral_injection = MockMineralInjection::new();

    scope(|scope| {
        // thread for schedule check
        scope.spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_balling,
                &mut channels.schedule_check.rx_schedule_check_from_balling,
                None,
                schedule_allows,
            )
        });

        // thread for the rest of test environment
        scope.spawn(move || {
            let execution_count: i32;
            spin_sleeper_test_environment.sleep(sleep_duration_test_environment);

            // internal scope to for releasing mutex after read
            {
                execution_count = *mutex_device_scheduler_environment.lock().unwrap();
            }

            println!("test_balling_dosing: execution_count={}", execution_count);
            assert_eq!(execution_count, execution_count_reference);

            // now we can send the quit command
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_balling
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_balling
                .blocking_recv()
                .unwrap();
            println!(
                "test_balling_dosing: Received confirmation of Quit command from test object."
            );
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_balling
                .send(InternalCommand::Terminate);
        });

        // thread for the test object
        scope.spawn(move || {
            let mut tx_balling_to_schedule_check_for_test_case_finish =
                channels.balling.tx_balling_to_schedule_check.clone();

            execute_blocking(
                &mut balling,
                mutex_device_scheduler,
                &mut channels.balling,
                &mut mock_mineral_injection,
                sql_interface_balling,
            );

            let _ =
                tx_balling_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

            println!("{}", mock_mineral_injection);

            assert_eq!(
                mock_mineral_injection.pump1_engagement_durations.len(),
                pump1_engagement_count_reference
            );
            assert_eq!(
                mock_mineral_injection.pump2_engagement_durations.len(),
                pump2_engagement_count_reference
            );
            assert_eq!(
                mock_mineral_injection.pump3_engagement_durations.len(),
                pump3_engagement_count_reference
            );
            assert_eq!(
                mock_mineral_injection.pump4_engagement_durations.len(),
                pump4_engagement_count_reference
            );
        });
    });
}

#[test]
// Test case runs Balling dosing control and triggers inhibition by sending the stop message.
// After verification that Balling dosing is stopped,
// The test case sends the start message and verifies if Balling dosing control
// resumes operation.
// Test case uses test database #06.
pub fn test_messaging_stops_starts_balling_dosing() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let sleep_duration_7_secs = Duration::from_secs(7);
    let spin_sleeper = SpinSleeper::default();

    let mut config: ConfigData = read_balling_config_with_database(6);

    config.balling.pump2_active = false;
    config.balling.pump3_active = false;
    config.balling.pump4_active = false;
    config.balling.schedule_check_interval = 1;
    config.balling.dosing_interval_pump1 = 5;

    let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
    let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
    let mut sql_interface = match SqlInterface::new(config.sql_interface) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };
    match SqlInterface::truncate_table(
        &mut sql_interface,
        SQL_TABLE_BALLING_DOSING_LOG.to_string(),
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let mut sql_interface_balling = SqlInterfaceBalling::new(
        sql_interface.get_connection().unwrap(),
        max_rows_balling_set_values,
        max_rows_balling_dosing_log,
    )
        .unwrap();

    // empty the Balling dosing log table
    match SqlInterface::truncate_table(
        &mut sql_interface,
        SQL_TABLE_BALLING_DOSING_LOG.to_string(),
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    // insert a valid pump configuration for pump #1
    insert_pump_configurations(
        &mut sql_interface,
        &mut sql_interface_balling,
        true,
        false,
        false,
        false,
    );

    // create test objects
    let mut balling = Balling::new(
        config.balling,
        Duration::from_millis(100),
        &mut sql_interface_balling,
    )
        .unwrap();

    let mut channels = Channels::new_for_test();

    let mut tx_balling_to_schedule_check_for_test_case_finish =
        channels.balling.tx_balling_to_schedule_check.clone();

    let mutex_device_scheduler = Arc::new(Mutex::new(0));

    let mut mock_mineral_injection = MockMineralInjection::new();

    scope(|scope| {
        // thread for mock schedule check: runs until it receives Quit command
        // thread for schedule check
        scope.spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_balling,
                &mut channels.schedule_check.rx_schedule_check_from_balling,
                None,
                true,
            )
        });

        let mutex_device_scheduler_balling_test_environment = mutex_device_scheduler.clone();

        // thread IPC messaging - this scope controls execution of the test case
        scope.spawn(move || {
            // wait for up to 5 seconds to make sure the test object can receive the message and execute first dosing
            let timeout_initial = Duration::from_secs(5);
            let start_initial = Instant::now();
            let mut actuation_count_0: i32;
            loop {
                spin_sleeper.sleep(sleep_duration_100_millis);
                actuation_count_0 = *mutex_device_scheduler_balling_test_environment
                    .lock()
                    .unwrap();
                if actuation_count_0 >= 1 || start_initial.elapsed() > timeout_initial {
                    break;
                }
            }

            // check the initial amount of actuation
            assert_eq!(actuation_count_0, 1);

            // sending the message requesting stop of Balling dosing control
            match channels
                .messaging
                .tx_messaging_to_balling
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

            // Wait for 7 seconds. If the test object does not follow the stop request,
            // then dosings will happen in this period.
            spin_sleeper.sleep(sleep_duration_7_secs);

            let actuation_count_1 = *mutex_device_scheduler_balling_test_environment
                .lock()
                .unwrap();
            // check the amount of actuation (after stop request)
            assert_eq!(actuation_count_1, 1);

            // sending the message requesting restart of Balling dosing control
            match channels
                .messaging
                .tx_messaging_to_balling
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

            // Wait for up to 10 seconds. If the test object follows the start request,
            // then dosings will happen in this period.
            let timeout_resume = Duration::from_secs(10);
            let start_resume = Instant::now();
            let mut actuation_count_2: i32;
            loop {
                spin_sleeper.sleep(sleep_duration_100_millis);
                actuation_count_2 = *mutex_device_scheduler_balling_test_environment
                    .lock()
                    .unwrap();
                if actuation_count_2 >= 3 || start_resume.elapsed() > timeout_resume {
                    break;
                }
            }
            // check the amount of actuation (after start request)
            assert_eq!(actuation_count_2, 3);

            // requesting Balling dosing control to quit
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_balling
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_balling
                .blocking_recv()
                .unwrap();

            let actuation_count_3 = *mutex_device_scheduler_balling_test_environment
                .lock()
                .unwrap();
            // check the amount of actuation (after Quit command)
            assert_eq!(actuation_count_3, 3);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_balling
                .send(InternalCommand::Terminate);
        });

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object and assertions
        scope.spawn(move || {
            execute_blocking(
                &mut balling,
                mutex_device_scheduler,
                &mut channels.balling,
                &mut mock_mineral_injection,
                sql_interface_balling,
            );
            println!("MockMineralInjection:\n{}", mock_mineral_injection);

            assert_eq!(mock_mineral_injection.pump1_engagement_durations.len(), 3);
            assert_eq!(mock_mineral_injection.pump2_engagement_durations.len(), 0);
            assert_eq!(mock_mineral_injection.pump3_engagement_durations.len(), 0);
            assert_eq!(mock_mineral_injection.pump4_engagement_durations.len(), 0);

            assert_eq!(
                mock_mineral_injection
                    .pump1_engagement_durations
                    .pop()
                    .unwrap(),
                250
            );
            assert_eq!(
                mock_mineral_injection
                    .pump1_engagement_durations
                    .pop()
                    .unwrap(),
                250
            );

            let _ =
                tx_balling_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
        });

        println!(
            "* [Messaging] checking if messaging can stop and start balling dosing succeeded."
        );
    });
}
#[test]
// Test case checks if during instantiation of the struct, the implementation checks
// if there is a database entry for a pump that is configured as active.
// The code shall return an error of type BallingError if there is no matching database entry.
// Test case uses test database #48.
pub fn test_check_database_vs_config() {
    let mut config: ConfigData = read_balling_config_with_database(48);

    config.balling.pump1_active = true;
    config.balling.pump2_active = false;
    config.balling.pump3_active = false;
    config.balling.pump4_active = false;

    let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
    let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
    let mut sql_interface = match SqlInterface::new(config.sql_interface) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };
    match SqlInterface::truncate_table(
        &mut sql_interface,
        SQL_TABLE_BALLING_DOSING_LOG.to_string(),
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }
    let mut sql_interface_balling = SqlInterfaceBalling::new(
        sql_interface.get_connection().unwrap(),
        max_rows_balling_set_values,
        max_rows_balling_dosing_log,
    )
        .unwrap();

    // insert a valid pump configuration for pump #1
    insert_pump_configurations(
        &mut sql_interface,
        &mut sql_interface_balling,
        false,
        false,
        false,
        true, // a different pump is configured as active
    );

    // create test objects
    let result = Balling::new(
        config.balling,
        Duration::from_millis(100),
        &mut sql_interface_balling,
    );
    assert!(matches!(
            result,
            Err(BallingError::SetValueRetrievalError {
                location: _,
                pump_id: _,
                source: _
            })
        ));
}

#[test]
// The test case executes the test object using dosing requests via the external interface.
pub fn test_balling_dosing_external_requests() {
    let test_db_number = 49;
    let schedule_allows = true;
    let execution_count_reference: i32 = 4;
    let duration_millis = 5000;
    let sleep_duration_test_environment = Duration::from_millis(duration_millis);
    let spin_sleeper_test_environment = SpinSleeper::default();
    let mut config: ConfigData = read_balling_config_with_database(test_db_number);

    config.balling.pump1_active = true;
    config.balling.dosing_interval_pump1 = 3600 * 24; // long interval to avoid regular dosing
    config.balling.pump2_active = true;
    config.balling.dosing_interval_pump2 = 3600 * 24; // long interval to avoid regular dosing
    config.balling.pump3_active = true;
    config.balling.dosing_interval_pump3 = 3600 * 24; // long interval to avoid regular dosing
    config.balling.pump4_active = true;
    config.balling.dosing_interval_pump4 = 3600 * 24; // long interval to avoid regular dosing
    config.balling.schedule_check_interval = 100;

    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
    let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
    let mut sql_interface = match SqlInterface::new(config.sql_interface) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };
    match SqlInterface::truncate_table(
        &mut sql_interface,
        SQL_TABLE_BALLING_DOSING_LOG.to_string(),
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }
    let mut sql_interface_balling = SqlInterfaceBalling::new(
        sql_interface.get_connection().unwrap(),
        max_rows_balling_set_values,
        max_rows_balling_dosing_log,
    )
        .unwrap();

    insert_pump_configurations(
        &mut sql_interface,
        &mut sql_interface_balling,
        true,
        true,
        true,
        true,
    );
    insert_past_dosing_events(&mut sql_interface, &mut sql_interface_balling);

    let mut channels = Channels::new_for_test();

    let mutex_device_scheduler = Arc::new(Mutex::new(0));
    let mutex_device_scheduler_environment = mutex_device_scheduler.clone();

    // create the test object
    let mut balling = Balling::new(
        config.balling,
        Duration::from_secs(1000),
        &mut sql_interface_balling,
    )
        .unwrap();

    let mut mock_mineral_injection = MockMineralInjection::new();

    scope(|scope| {
        // thread for schedule check
        scope.spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_balling,
                &mut channels.schedule_check.rx_schedule_check_from_balling,
                None,
                schedule_allows,
            )
        });

        // thread for the rest of test environment
        scope.spawn(move || {
            let execution_count1: i32;
            spin_sleeper_test_environment.sleep(sleep_duration_test_environment);
            // internal scope to for releasing mutex after read
            {
                execution_count1 = *mutex_device_scheduler_environment.lock().unwrap();
            }
            println!("test_balling_dosing: execution_count={}", execution_count1);
            assert_eq!(execution_count1, 0);

            // send the dosing requests to Balling
            channels
                .messaging
                .tx_messaging_to_balling
                .send(InternalCommand::Execute(2))
                .unwrap();
            channels
                .messaging
                .tx_messaging_to_balling
                .send(InternalCommand::Execute(4))
                .unwrap();
            channels
                .messaging
                .tx_messaging_to_balling
                .send(InternalCommand::Execute(3))
                .unwrap();
            channels
                .messaging
                .tx_messaging_to_balling
                .send(InternalCommand::Execute(1))
                .unwrap();

            let execution_count2: i32;
            spin_sleeper_test_environment.sleep(sleep_duration_test_environment);
            // internal scope to for releasing mutex after read
            {
                execution_count2 = *mutex_device_scheduler_environment.lock().unwrap();
            }
            println!("test_balling_dosing: execution_count={}", execution_count2);
            assert_eq!(execution_count2, execution_count_reference);

            // now we can send the quit command
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_balling
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_balling
                .blocking_recv()
                .unwrap();
            println!(
                "test_balling_dosing: Received confirmation of Quit command from test object."
            );
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_balling
                .send(InternalCommand::Terminate);
        });

        // thread for the test object
        scope.spawn(move || {
            let mut tx_balling_to_schedule_check_for_test_case_finish =
                channels.balling.tx_balling_to_schedule_check.clone();

            execute_blocking(
                &mut balling,
                mutex_device_scheduler,
                &mut channels.balling,
                &mut mock_mineral_injection,
                sql_interface_balling,
            );

            let _ =
                tx_balling_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

            println!("{}", mock_mineral_injection);

            let reference_pump_protocol = vec![
                PeristalticPump2,
                PeristalticPump4,
                PeristalticPump3,
                PeristalticPump1,
            ];

            assert_eq!(
                mock_mineral_injection.pump_protocol,
                reference_pump_protocol
            );
        });
    });
}

#[test]
// The test case executes the test object with the following boundaries:
// - pump #1 is active
// - the schedule checker is allowing operation
// Test case uses test database #03.
pub fn test_balling_dosing_schedule_allows_single_dosing() {
    test_balling_dosing(
        3,                        // index for determining test the database
        true,                     // schedule allows
        1,                        // execution count
        10000,                    // maximum time in milliseconds for test execution
        1,                        // number of dosing events for pump #1
        0,                        // number of dosing events for pump #2
        0,                        // number of dosing events for pump #3
        0,                        // number of dosing events for pump #4
        40,                       // schedule check interval
        Some((50, 200, 100, 50)), // dosing intervals
    );
}

#[test]
// The test case executes the test object with the following boundaries:
// - all pumps are active
// - schedule checker is not allowing operation
// Test case uses test database #04.
pub fn test_balling_dosing_schedule_forbids() {
    test_balling_dosing(
        4,     // index for determining the test database
        false, // schedule does not allow
        0,     // execution count
        10000, // maximum time in milliseconds for test execution
        0,     // number of dosings for pump #1
        0,     // number of dosings for pump #2
        0,     // number of dosings for pump #3
        0,     // number of dosings for pump #4
        60,    // schedule check interval
        None,
    );
}

#[test]
// The test case executes the test object with the following boundaries:
// - all pumps are active
// - the schedule checker is allowing operation
// - dosing intervals are short so that lots of dosing events are triggered
// Test case uses test database #05.
pub fn test_balling_dosing_schedule_allows_multiple_dosings() {
    test_balling_dosing(
        5,                  // index for determining test database
        true,               // schedule allows
        28,                 // execution count
        34000,              // maximum time in milliseconds for test execution
        12,                 // number of dosing events for pump #1
        7,                  // number of dosing events for pump #2
        5,                  // number of dosing events for pump #3
        4,                  // number of dosing events for pump #4
        1,                  // schedule check interval
        Some((3, 5, 7, 9)), // dosing intervals
    );
}






