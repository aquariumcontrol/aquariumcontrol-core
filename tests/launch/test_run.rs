/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use aquarium_control::database::sql_interface::SqlInterface;
/// The test of the main entry point of the Aquarium Control application.
use aquarium_control::launch::run::run;
use aquarium_control::launch::startup_error::StartupError;
use aquarium_control::utilities::config::read_config_file;
use aquarium_control::utilities::signal_handler::TEST_SHUTDOWN_TRIGGER;
use std::sync::{atomic::Ordering, Mutex};
use std::time::Duration;

static TEST_RUN_MUTEX: Mutex<()> = Mutex::new(());

#[path = "../database/common.rs"]
mod common;

use common::{add_balling_profile, insert_current_version_information, truncate_all_tables};

#[cfg(feature = "test-utils")]
use aquarium_control::database::database_interface_feed_trait::DatabaseInterfaceFeedTrait;
#[cfg(feature = "test-utils")]
use aquarium_control::database::sql_interface_feed::insert_feed_profiles;
#[cfg(feature = "test-utils")]
use aquarium_control::database::sql_interface_feed::insert_feed_schedule_entry;
#[cfg(feature = "test-utils")]
use aquarium_control::database::sql_interface_feed::SqlInterfaceFeed;

#[test]
// Verifies that run async runtime configuration.
fn test_run_async_runtime_configuration() {
    let _lock = TEST_RUN_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

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
#[cfg(feature = "test-utils")]
// Verifies application behavior when a past feed schedule entry exists in the database
// and feed profile execution starts at application startup.
// [itest->swreq~food_execute_schedule~1]
fn test_run_with_past_feed_schedule_entry() {
    let _lock = TEST_RUN_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

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
#[cfg(feature = "test-utils")]
// Test case uses test database #83.
// Verifies that execution of a feed profile with a long duration (> 30s) blocks the Tokio worker thread
// due to spin_sleeper.sleep, preventing the watchdog task from petting the watchdog timer.
/// [itest->swreq~food_execute_schedule~1]
fn test_run_feed_profile_blocks_watchdog() {
    // The test case asserts that the watchdog task was not blocked and operated continuously
    // through two key mechanisms:
    //
    // 1. Watchdog Petting Assertion
    // The test configures a custom watchdog filename /tmp/aquarium_watchdog_test_83.tmp in the
    // test configuration and assert its content:
    //    assert!(
    //        std::path::Path::new(watchdog_file_path).exists(),
    //        "Watchdog file was not petted during feed profile execution!"
    //    );
    // - When Watchdog::execute runs asynchronously on Tokio, it writes its heartbeat to
    // watchdog_file_path on each interval using PettingTrait::pet.
    // - If the Tokio worker thread were blocked by during the feed profile execution, the watchdog
    // task would be starved of CPU execution time, preventing it from creating or updating
    // this file.
    // 2. Runtime Completion Assertion (test_run.rs)
    //    let run_result = run(Some(test_config_path.to_string()), Some(Duration::from_secs(3)));
    //
    //    assert!(
    //        run_result.is_ok(),
    //        "run() failed with error: {:?}",
    //        run_result.err()
    //    );
    // - run(...) executes the main application async runtime for 3 seconds.
    // During this window, feed profile #83 (configured with an extended pause duration) is actively
    // executing via food_injection.inject_food(...).
    // Asserting run_result.is_ok() verifies that all Tokio tasks (including feed, watchdog,
    // sensor_manager, and signal handler) join cleanly and exit without timing out or crashing.

    let _lock = TEST_RUN_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    // configuration is derived from previous test case test_run_async_runtime_configuration
    let test_config_path = "config/aquarium_control_test_runtime_blocked.toml";

    TEST_SHUTDOWN_TRIGGER.store(false, Ordering::Relaxed);
    let _ = std::fs::remove_file("/tmp/aquarium-ctrl-test2.pid");

    let watchdog_file_path = "/tmp/aquarium_watchdog_test2.tmp";
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
        insert_feed_schedule_entry(&mut sql_interface_feed);
    }

    let run_result = run(
        Some(test_config_path.to_string()),
        Some(Duration::from_secs(3)),
    );

    assert!(
        run_result.is_ok(),
        "run() failed with error: {:?}",
        run_result.err()
    );

    assert!(
        std::path::Path::new(watchdog_file_path).exists(),
        "Watchdog file was not petted during feed profile execution!"
    );

    let _ = std::fs::remove_file(watchdog_file_path);
}

#[test]
// Verifies that run() returns StartupError::InvalidThermalConfig when heating and ventilation temperature settings clash.
fn test_run_invalid_thermal_config() {
    let _lock = TEST_RUN_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    let _ = std::fs::remove_file("/tmp/aquarium-ctrl-test.pid");

    let original_content =
        std::fs::read_to_string("config/aquarium_control_test_runtime.toml").unwrap();
    let invalid_content = original_content.replace(
        "switch_off_temperature = 28.0",
        "switch_off_temperature = 30.0",
    );
    let temp_config_path = "config/aquarium_control_test_invalid_thermal.toml";
    std::fs::write(temp_config_path, invalid_content).unwrap();

    let run_result = run(Some(temp_config_path.to_string()), None);
    let _ = std::fs::remove_file(temp_config_path);
    let _ = std::fs::remove_file("/tmp/aquarium-ctrl-test.pid");

    assert!(
        matches!(run_result, Err(StartupError::InvalidThermalConfig)),
        "Expected StartupError::InvalidThermalConfig, got {:?}",
        run_result
    );
}
