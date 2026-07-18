/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use aquarium_control::database::database_interface_feed_trait::DatabaseInterfaceFeedTrait;
use aquarium_control::database::sql_interface::SqlInterface;
use aquarium_control::database::sql_interface_feed::{insert_feed_profiles, SqlInterfaceFeed};
use aquarium_control::database::sql_query_strings::SQL_TABLE_FEED_SCHEDULE;
use aquarium_control::database::sql_test_query_strings::SQL_TABLE_FEED_LOG;
use aquarium_control::food::feed::Feed;
use aquarium_control::launch::channels::Channels;
use aquarium_control::mocks::mock_food_injection::MockFoodInjection;
use aquarium_control::mocks::mock_sql_interface_feed::tests::MockSqlInterfaceFeed;
use aquarium_control::utilities::channel_content::InternalCommand;
use aquarium_control::utilities::config::{read_config_file, read_config_file_with_test_database, ConfigData};
use aquarium_control::utilities::logger::setup_logger;
use aquarium_control::utilities::logger_config::LoggerConfig;
use aquarium_control::food::feed_channels::FeedChannels;
use aquarium_control::food::food_injection::FoodInjectionTrait;
use aquarium_control::mocks::mock_signal_handler::create_mock_signal_handler;
use spin_sleep::SpinSleeper;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::scope;
use std::time::Duration;

/// Helper function to execute the feed task inside a test Tokio runtime.
///
/// # Arguments
/// * `feed` - A mutable reference to the `Feed` instance.
/// * `mutex_device_scheduler_feed` - Mutex coordinating access to the device scheduler.
/// * `feed_channels` - A mutable reference to the `FeedChannels` instance.
/// * `food_injection` - A mutable reference to the food injector instance.
/// * `sql_interface_feed` - Database interface for feed operations.
fn execute_blocking(
    feed: &mut Feed,
    mutex_device_scheduler_feed: Arc<Mutex<i32>>,
    feed_channels: &mut FeedChannels,
    food_injection: &mut (impl FoodInjectionTrait + Send),
    sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send>,
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

            feed.execute(
                mutex_device_scheduler_feed,
                feed_channels,
                food_injection,
                sql_interface_feed,
                is_multi_thread_test,
            )
                .await;
        });
}

#[test]
// "Happy" case executing a set of feed schedule entries. Additionally, it checks row limitation.
// Test case uses test database #07.
pub fn test_feed_schedule_execution() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let sleep_duration_8_seconds = Duration::from_millis(10000);
    let spin_sleeper = SpinSleeper::default();

    let mut channels = Channels::new_for_test();

    let mut mock_food_injection = MockFoodInjection::new();

    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        7,
    );

    let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };

    let mut sql_interface_feed_unboxed = SqlInterfaceFeed::new(
        sql_interface.get_connection().unwrap(),
        config.sql_interface.max_rows_feed_profile,
        config.sql_interface.max_rows_feed_schedule,
        config.sql_interface.max_rows_feed_log,
    )
    .unwrap();

    insert_feed_profiles(&mut sql_interface, &mut sql_interface_feed_unboxed);

    let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
        Box::new(sql_interface_feed_unboxed);

    // empty the feed schedule table
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }
    // empty the feed log table
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_LOG.to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let time_stamp_feed_schedule_entry_one_time =
        match SqlInterface::get_current_timestamp_offset_seconds(&mut sql_interface.conn, 2) {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "test_feed_schedule_execution: Could not calculate timestamp for stimuli: {e:?}"
                );
            }
        };

    let time_stamp_feed_schedule_entry_repeat =
        match SqlInterface::get_current_timestamp_offset_seconds(&mut sql_interface.conn, 4) {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "test_feed_schedule_execution: Could not calculate timestamp for stimuli: {e:?}"
                );
            }
        };

    // manipulation of the database: inserting feed schedule entries with future timestamp
    sql_interface_feed
        .insert_feed_schedule_entry(time_stamp_feed_schedule_entry_one_time, 1, false, false)
        .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

    sql_interface_feed
        .insert_feed_schedule_entry(time_stamp_feed_schedule_entry_repeat, 2, false, true)
        .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

    let mut feed = Feed::new(config.feed, Duration::from_millis(1000));

    scope(|scope| {
        let mutex_device_scheduler_feed = Arc::new(Mutex::new(0));

        // thread for mock signal handler
        scope.spawn(move || {
            create_mock_signal_handler(
                &mut channels.signal_handler.tx_signal_handler_to_feed,
                &mut channels.signal_handler.rx_signal_handler_from_feed,
                spin_sleeper,
                sleep_duration_8_seconds,
            );
        });

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        scope.spawn(move || {
            execute_blocking(
                &mut feed,
                mutex_device_scheduler_feed.clone(),
                &mut channels.feed,
                &mut mock_food_injection,
                sql_interface_feed,
            );

            // check if the right profiles have been executed in the right order
            assert_eq!(mock_food_injection.pop_profile_id_executed(), Some(2));
            assert_eq!(mock_food_injection.pop_profile_id_executed(), Some(1));
            assert_eq!(mock_food_injection.pop_profile_id_executed(), None);
        });
    });
}

#[test]
// Test case runs feed control and triggers inhibition by sending the stop message via the channel.
// After verification that feed is stopped,
// The test case sends the start message via the channel and verifies if feed control
// resumes operation. Afterward, the test case triggers execution of a specific feed profile
// by sending the "execute" message via the channel.
// Test case uses test database #08.
pub fn test_messaging_stops_starts_execute_feed() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let sleep_duration_2_secs = Duration::from_secs(2);
    let sleep_duration_10_secs = Duration::from_secs(10);
    let spin_sleeper = SpinSleeper::default();

    setup_logger(LoggerConfig::default()).unwrap();

    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        8,
    );

    let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };

    let mut sql_interface_feed_unboxed = SqlInterfaceFeed::new(
        sql_interface.get_connection().unwrap(),
        config.sql_interface.max_rows_feed_profile,
        config.sql_interface.max_rows_feed_schedule,
        config.sql_interface.max_rows_feed_log,
    )
    .unwrap();

    insert_feed_profiles(&mut sql_interface, &mut sql_interface_feed_unboxed);

    let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
        Box::new(sql_interface_feed_unboxed);

    // empty the feed schedule
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let time_stamp_feed_schedule_entry =
        match SqlInterface::get_current_timestamp_offset_seconds(&mut sql_interface.conn, 4) {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "test_feed_schedule_execution: Could not calculate timestamp for stimuli: {e:?}"
                );
            }
        };

    // manipulation of the database: inserting feed schedule entries with future timestamp
    sql_interface_feed
        .insert_feed_schedule_entry(time_stamp_feed_schedule_entry, 1, false, false)
        .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

    // create the test object
    let mut feed = Feed::new(config.feed, Duration::from_millis(1000));

    let mut channels = Channels::new_for_test();

    let mut mock_food_injection = MockFoodInjection::new();

    let mutex_device_scheduler_feed_test_object = Arc::new(Mutex::new(0));
    let mutex_device_scheduler_feed_test_environment =
        mutex_device_scheduler_feed_test_object.clone();

    // thread for test environment
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            // wait to make sure the test object can receive the message
            spin_sleeper.sleep(sleep_duration_100_millis);

            // sending the message requesting stop of feed control
            match channels
                .messaging
                .tx_messaging_to_feed
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

            // Wait for 10 seconds. If the test object does not follow the stop request,
            // then food injection will happen in this period.
            spin_sleeper.sleep(sleep_duration_10_secs);

            let actuation_count_1 = *mutex_device_scheduler_feed_test_environment.lock().unwrap();

            // check the amount of actuation (after stop request)
            assert_eq!(actuation_count_1, 0);

            // sending the message requesting restart of feed control
            match channels
                .messaging
                .tx_messaging_to_feed
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

            // Wait for 10 seconds. If the test object follows the start request,
            // then food injections will happen in this period.
            spin_sleeper.sleep(sleep_duration_10_secs);

            // get count of feed profiles executed
            let actuation_count_2 = *mutex_device_scheduler_feed_test_environment.lock().unwrap();

            // check the amount of actuation (after start request)
            assert_eq!(actuation_count_2, 1);

            // sending the message requesting execution of the feed profile
            match channels
                .messaging
                .tx_messaging_to_feed
                .send(InternalCommand::Execute(1))
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: error when sending execution command to test object ({e:?})",
                        module_path!()
                    );
                }
            }

            // Wait for 2 seconds. If the test object follows the "execute" request,
            // then food injection will happen in this period.
            spin_sleeper.sleep(sleep_duration_2_secs);

            // get count of feed profiles executed
            let actuation_count_3 = *mutex_device_scheduler_feed_test_environment.lock().unwrap();

            // check the amount of actuation (after the "execute" request)
            assert_eq!(actuation_count_3, 2);

            // requesting feed control to quit
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_feed
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_feed
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_feed
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            execute_blocking(
                &mut feed,
                mutex_device_scheduler_feed_test_object.clone(),
                &mut channels.feed,
                &mut mock_food_injection,
                sql_interface_feed,
            );
            println!("MockFoodInjection:\n{}", mock_food_injection);

            // check if the right feed profiles have been executed
            assert_eq!(mock_food_injection.profile_ids_executed.len(), 2);
            assert_eq!(mock_food_injection.profile_ids_executed[0], 1);
            assert_eq!(mock_food_injection.profile_ids_executed[1], 1);
        })
        .unwrap();

    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// Test case runs feed control and triggers inhibition by using a mock SQL interface which returns only errors.
// This test case does not require any communication with the database.
pub fn test_messaging_sql_error_inhibition_feed() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let sleep_duration_10_secs = Duration::from_secs(10);
    let spin_sleeper = SpinSleeper::default();

    setup_logger(LoggerConfig::default()).unwrap();

    let config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

    // mock interface will be used in operation of the test object
    let mock_sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
        Box::new(MockSqlInterfaceFeed::new());

    // create the test object
    let mut feed = Feed::new(config.feed, Duration::from_millis(1000));

    let mut channels = Channels::new_for_test();

    let mut mock_food_injection = MockFoodInjection::new();

    let mutex_device_scheduler_feed_test_object = Arc::new(Mutex::new(0));
    let mutex_device_scheduler_feed_test_environment =
        mutex_device_scheduler_feed_test_object.clone();

    // thread for test environment
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            // Wait for 10 seconds to be able to afterward check
            // if the test object still managed to execute a feed operation
            spin_sleeper.sleep(sleep_duration_10_secs);

            let actuation_count_1 = *mutex_device_scheduler_feed_test_environment.lock().unwrap();

            // check the amount of actuation (after stop request)
            assert_eq!(actuation_count_1, 0);

            // requesting feed control to quit
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_feed
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_feed
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_feed
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            execute_blocking(
                &mut feed,
                mutex_device_scheduler_feed_test_object.clone(),
                &mut channels.feed,
                &mut mock_food_injection,
                mock_sql_interface_feed,
            );
            println!("MockFoodInjection:\n{}", mock_food_injection);

            // check if the right feed profiles have been executed
            assert_eq!(mock_food_injection.profile_ids_executed.len(), 0);
        })
        .unwrap();

    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// Test case test implementation with multiple feed schedule entries in the past.
// The youngest feed schedule entry shall be selected.
// Test case uses test database #09.
pub fn test_feed_schedule_execution_past_entries_select_youngest() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let sleep_duration_8_seconds = Duration::from_millis(10000);
    let spin_sleeper = SpinSleeper::default();

    let mut channels = Channels::new_for_test();

    setup_logger(LoggerConfig::default()).unwrap();

    let mut mock_food_injection = MockFoodInjection::new();

    let config: ConfigData = read_config_file_with_test_database(
        "/config/aquarium_control_test_generic.toml".to_string(),
        9,
    );

    let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };

    let mut sql_interface_feed_unboxed = SqlInterfaceFeed::new(
        sql_interface.get_connection().unwrap(),
        config.sql_interface.max_rows_feed_profile,
        config.sql_interface.max_rows_feed_schedule,
        config.sql_interface.max_rows_feed_log,
    )
        .unwrap();

    insert_feed_profiles(&mut sql_interface, &mut sql_interface_feed_unboxed);

    let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
        Box::new(sql_interface_feed_unboxed);

    // empty the feed schedule table
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string())
    {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let time_stamp_feed_schedule_entry_one_time =
        match SqlInterface::get_current_timestamp_offset_seconds(&mut sql_interface.conn, -2) {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "test_feed_schedule_execution: Could not calculate timestamp for stimuli: {e:?}"
                );
            }
        };

    let time_stamp_feed_schedule_entry_repeat =
        match SqlInterface::get_current_timestamp_offset_seconds(&mut sql_interface.conn, -4) {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "test_feed_schedule_execution: Could not calculate timestamp for stimuli: {e:?}"
                );
            }
        };

    // manipulation of the database: inserting feed schedule entries with future timestamp
    sql_interface_feed
        .insert_feed_schedule_entry(time_stamp_feed_schedule_entry_one_time, 1, false, false)
        .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

    sql_interface_feed
        .insert_feed_schedule_entry(time_stamp_feed_schedule_entry_repeat, 2, false, true)
        .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

    let mut feed = Feed::new(config.feed, Duration::from_millis(1000));

    scope(|scope| {
        let mutex_device_scheduler_feed = Arc::new(Mutex::new(0));

        // thread for mock signal handler
        scope.spawn(move || {
            create_mock_signal_handler(
                &mut channels.signal_handler.tx_signal_handler_to_feed,
                &mut channels.signal_handler.rx_signal_handler_from_feed,
                spin_sleeper,
                sleep_duration_8_seconds,
            );
        });

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        scope.spawn(move || {
            execute_blocking(
                &mut feed,
                mutex_device_scheduler_feed.clone(),
                &mut channels.feed,
                &mut mock_food_injection,
                sql_interface_feed,
            );

            // check if the right profiles have been executed in the right order
            assert_eq!(mock_food_injection.pop_profile_id_executed(), Some(1));
            assert_eq!(mock_food_injection.pop_profile_id_executed(), None);
        });
    });
}

#[test]
// Test case test implementation with multiple feed schedule entries in the past.
// The oldest feed schedule entry shall be selected.
// Test case uses test database #10.
pub fn test_feed_schedule_execution_past_entries_select_oldest() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let sleep_duration_8_seconds = Duration::from_millis(10000);
    let spin_sleeper = SpinSleeper::default();

    let mut channels = Channels::new_for_test();

    setup_logger(LoggerConfig::default()).unwrap();

    let mut mock_food_injection = MockFoodInjection::new();

    let mut config: ConfigData = read_config_file_with_test_database(
        "/config/aquarium_control_test_generic.toml".to_string(),
        10,
    );

    config.feed.strategy_multiple_schedule_entries_in_past = 99;

    let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };

    let mut sql_interface_feed_unboxed = SqlInterfaceFeed::new(
        sql_interface.get_connection().unwrap(),
        config.sql_interface.max_rows_feed_profile,
        config.sql_interface.max_rows_feed_schedule,
        config.sql_interface.max_rows_feed_log,
    )
        .unwrap();

    insert_feed_profiles(&mut sql_interface, &mut sql_interface_feed_unboxed);

    let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
        Box::new(sql_interface_feed_unboxed);

    // empty the feed schedule table
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string())
    {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }

    let time_stamp_feed_schedule_entry_one_time =
        match SqlInterface::get_current_timestamp_offset_seconds(&mut sql_interface.conn, -2) {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "test_feed_schedule_execution: Could not calculate timestamp for stimuli: {e:?}"
                );
            }
        };

    let time_stamp_feed_schedule_entry_repeat =
        match SqlInterface::get_current_timestamp_offset_seconds(&mut sql_interface.conn, -4) {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "test_feed_schedule_execution: Could not calculate timestamp for stimuli: {e:?}"
                );
            }
        };

    // manipulation of the database: inserting feed schedule entries with future timestamp
    sql_interface_feed
        .insert_feed_schedule_entry(time_stamp_feed_schedule_entry_one_time, 1, false, false)
        .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

    sql_interface_feed
        .insert_feed_schedule_entry(time_stamp_feed_schedule_entry_repeat, 2, false, true)
        .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

    let mut feed = Feed::new(config.feed, Duration::from_millis(1000));

    scope(|scope| {
        let mutex_device_scheduler_feed = Arc::new(Mutex::new(0));

        // thread for mock signal handler
        scope.spawn(move || {
            create_mock_signal_handler(
                &mut channels.signal_handler.tx_signal_handler_to_feed,
                &mut channels.signal_handler.rx_signal_handler_from_feed,
                spin_sleeper,
                sleep_duration_8_seconds,
            );
        });

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        scope.spawn(move || {
            execute_blocking(
                &mut feed,
                mutex_device_scheduler_feed.clone(),
                &mut channels.feed,
                &mut mock_food_injection,
                sql_interface_feed,
            );

            // check if the right profiles have been executed in the right order
            assert_eq!(mock_food_injection.pop_profile_id_executed(), Some(2));
            assert_eq!(mock_food_injection.pop_profile_id_executed(), None);
        });
    });
}