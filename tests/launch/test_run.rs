/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use aquarium_control::database::sql_interface::SqlInterface;
/// The test of the main entry point of the Aquarium Control application.
use aquarium_control::launch::run::run;
use aquarium_control::utilities::config::read_config_file;
use aquarium_control::utilities::signal_handler::TEST_SHUTDOWN_TRIGGER;
use std::sync::{atomic::Ordering, Mutex};
use std::time::Duration;

static TEST_RUN_MUTEX: Mutex<()> = Mutex::new(());

#[path = "../database/common.rs"]
mod common;

use common::{add_balling_profile, insert_current_version_information, truncate_all_tables};

use aquarium_control::database::database_interface_feed_trait::DatabaseInterfaceFeedTrait;
use aquarium_control::database::sql_interface_feed::{insert_feed_profiles, SqlInterfaceFeed};

#[test]
// Verifies that run async runtime configuration.
fn test_run_async_runtime_configuration() {
    let _lock = TEST_RUN_MUTEX.lock().unwrap();

    // Path of the dedicated config file
    let config_path = "config/aquarium_control_test_runtime.toml".to_string();

    // Reset the test shutdown trigger flag to false
    TEST_SHUTDOWN_TRIGGER.store(false, Ordering::Relaxed);

    // Delete any stale PID file from previous test executions to avoid PidCheckError
    let _ = std::fs::remove_file("/tmp/aquarium-ctrl-test.pid");

    // Connect to database and prepare the test environment within a scope so the connection is returned to the pool
    {
        let config_data = read_config_file(config_path.to_string()).unwrap();
        let mut sql_interface = SqlInterface::new(config_data.sql_interface).unwrap();

        truncate_all_tables(&mut sql_interface);

        insert_current_version_information(&mut sql_interface);

        // Insert default balling set values to satisfy startup configuration checks
        for pump_id in 1..=4 {
            add_balling_profile(
                &mut sql_interface.conn,
                pump_id,
                1.0f32,
                10.0f32,
                format!("Pump {}", pump_id),
            );
        }
    }

    // Run the main application run() function with 50 seconds test shutdown duration
    let run_result = run(Some(config_path), Some(Duration::from_secs(20)));

    assert!(
        run_result.is_ok(),
        "run() failed with error: {:?}",
        run_result.err()
    );
}

#[test]
// Verifies application behavior when a past feed schedule entry exists in the database
// and feed profile execution starts at application startup.
fn test_run_with_past_feed_schedule_entry() {
    let _lock = TEST_RUN_MUTEX.lock().unwrap();

    let config_path = "config/aquarium_control_test_runtime.toml".to_string();

    TEST_SHUTDOWN_TRIGGER.store(false, Ordering::Relaxed);
    let _ = std::fs::remove_file("/tmp/aquarium-ctrl-test.pid");

    {
        let config_data = read_config_file(config_path.to_string()).unwrap();
        let mut sql_interface = SqlInterface::new(config_data.sql_interface.clone()).unwrap();

        truncate_all_tables(&mut sql_interface);

        insert_current_version_information(&mut sql_interface);

        for pump_id in 1..=4 {
            add_balling_profile(
                &mut sql_interface.conn,
                pump_id,
                1.0f32,
                10.0f32,
                format!("Pump {}", pump_id),
            );
        }

        // Insert feed profiles using helper
        let mut sql_interface_feed = SqlInterfaceFeed::new(
            sql_interface.get_connection().unwrap(),
            config_data.sql_interface.max_rows_feed_profile,
            config_data.sql_interface.max_rows_feed_schedule,
            config_data.sql_interface.max_rows_feed_log,
        )
        .unwrap();

        insert_feed_profiles(&mut sql_interface, &mut sql_interface_feed);

        let past_timestamp =
            chrono::NaiveDateTime::parse_from_str("2024-01-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap();
        sql_interface_feed
            .insert_feed_schedule_entry(past_timestamp, 1, false, true)
            .unwrap();
    }

    let run_result = run(Some(config_path), Some(Duration::from_secs(10)));

    assert!(
        run_result.is_ok(),
        "run() failed with error: {:?}",
        run_result.err()
    );
}

#[test]
// Test case uses test database #83.
// Verifies that execution of a feed profile with a long duration (> 30s) blocks the Tokio worker thread
// due to spin_sleeper.sleep, preventing the watchdog task from petting the watchdog timer.
fn test_run_feed_profile_blocks_watchdog() {
    use mysql::params;
    use mysql::prelude::Queryable;

    let _lock = TEST_RUN_MUTEX.lock().unwrap();

    let base_config_path = "config/aquarium_control_test_runtime.toml";
    let test_config_path = "config/aquarium_control_test_83.toml";
    let watchdog_file_path = "/tmp/aquarium_watchdog_test_83.tmp";

    // Prepare custom config pointing to aquarium_test_83, port 35083, and test watchdog file
    let base_content = std::fs::read_to_string(base_config_path).unwrap();
    let updated_content = base_content
        .replace("aquarium_test_67", "aquarium_test_83")
        .replace("port = 35000", "port = 35083")
        .replace(
            "watchdog_filename = \"/dev/watchdog\"",
            &format!("watchdog_filename = \"{}\"", watchdog_file_path),
        );
    std::fs::write(test_config_path, updated_content).unwrap();

    TEST_SHUTDOWN_TRIGGER.store(false, Ordering::Relaxed);
    let _ = std::fs::remove_file("/tmp/aquarium-ctrl-test.pid");
    let _ = std::fs::remove_file(watchdog_file_path);

    {
        let config_data = read_config_file(test_config_path.to_string()).unwrap();
        let mut sql_interface = SqlInterface::new(config_data.sql_interface.clone()).unwrap();

        truncate_all_tables(&mut sql_interface);
        insert_current_version_information(&mut sql_interface);

        for pump_id in 1..=4 {
            add_balling_profile(
                &mut sql_interface.conn,
                pump_id,
                1.0f32,
                10.0f32,
                format!("Pump {}", pump_id),
            );
        }

        let mut sql_interface_feed = SqlInterfaceFeed::new(
            sql_interface.get_connection().unwrap(),
            config_data.sql_interface.max_rows_feed_profile,
            config_data.sql_interface.max_rows_feed_schedule,
            config_data.sql_interface.max_rows_feed_log,
        )
        .unwrap();

        insert_feed_profiles(&mut sql_interface, &mut sql_interface_feed);
        sql_interface_feed
            .conn
            .exec_drop(
                concat!(
                    "INSERT INTO feed_profiles VALUES ",
                    "(:profile_id, 'long_feed', ",
                    "2500,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0, ",
                    "0,0,0,0,0,0)"
                ),
                params! { "profile_id" => 83 },
            )
            .unwrap();

        let past_timestamp =
            chrono::NaiveDateTime::parse_from_str("2024-01-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap();
        sql_interface_feed
            .insert_feed_schedule_entry(past_timestamp, 83, false, true)
            .unwrap();
    }

    let run_result = run(Some(test_config_path.to_string()), Some(Duration::from_secs(3)));

    assert!(
        run_result.is_ok(),
        "run() failed with error: {:?}",
        run_result.err()
    );

    let _ = std::fs::remove_file(test_config_path);
    let _ = std::fs::remove_file(watchdog_file_path);
}

