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
use std::sync::atomic::Ordering;
use std::time::Duration;

#[path = "../database/common.rs"]
mod common;

use common::{add_balling_profile, insert_current_version_information, truncate_all_tables};

#[test]
// Verifies that run async runtime configuration.
fn test_run_async_runtime_configuration() {
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

        // Also clean up other settings tables to prevent row limits issues
        use aquarium_control::database::sql_test_query_strings::{
            SQL_TABLE_BALLING_SET_VALS, SQL_TABLE_HEATING_SET_VALS, SQL_TABLE_SCHEDULE,
            SQL_TABLE_VENTILATION_SET_VALS,
        };
        let _ = SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_BALLING_SET_VALS.to_string(),
        );
        let _ = SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_HEATING_SET_VALS.to_string(),
        );
        let _ = SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_VENTILATION_SET_VALS.to_string(),
        );
        let _ = SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string());
        let _ = SqlInterface::truncate_table(&mut sql_interface, "version".to_string());

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
