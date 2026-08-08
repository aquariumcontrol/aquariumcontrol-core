/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Test the aggregation and persistence of daily heating statistics.

use mysql::prelude::Queryable;

use all_asserts::{assert_ge, assert_le};
use aquarium_control::database::sql_interface_heating_stats::{
    HeatingStatsEntry, SqlInterfaceHeatingStats,
};
use aquarium_control::database::sql_interface_midnight::SqlInterfaceMidnightCalculator;
use aquarium_control::database::sql_test_query_strings::SQL_TABLE_HEATING_STATS;
use aquarium_control::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError,
};
use aquarium_control::utilities::config::{read_config_file_with_test_database, ConfigData};
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime};

#[test]
// This test case verifies the validation logic within the `SqlInterfaceHeatingStats::new()` constructor.
// It covers the following scenarios:
// 1. Happy Path: Initialization succeeds when the table has a valid number of rows.
// 2. Failure on Row Limit: Fails when `heatingstats` contains more rows than the configured limit.
// 3. Deactivated Check: Succeeds even with excess rows if the limit is set to 0.
// Test case uses test database #61.
// [itest->swreq~thermal_heater_heating~1]
// [itest->swreq~thermal_connect_heating_stats~1]
// [itest->swreq~thermal_midnight_calculator~1]
fn test_sql_interface_heating_stats_new() {
    // --- Common Setup ---
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        61, // Using database #61 as requested
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface.clone())
        .expect("Initialization of SQL interface for test failed.");

    // --- Test Case 1: Happy Path (empty table) ---
    println!("* Testing new() with an empty table (Happy Path)...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_STATS.to_string())
        .expect("Test setup failed: Could not truncate table.");

    let midnight_calc = Box::new(SqlInterfaceMidnightCalculator::new(
        sql_interface.get_connection().unwrap(),
    ));
    let result_happy_case =
        SqlInterfaceHeatingStats::new(sql_interface.get_connection().unwrap(), 10, midnight_calc);
    assert!(
        result_happy_case.is_ok(),
        "Expected new() to succeed with an empty table, but it failed: {:?}",
        result_happy_case.err()
    );
    println!("* Succeeded: Happy path initialization is successful on an empty table.");

    // --- Test Case 2: Failure on too many rows ---
    println!("* Testing new() with more rows than the limit...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_STATS.to_string())
        .expect("Test setup failed: Could not truncate table.");
    let conn = sql_interface.get_connection().unwrap();

    let mut heating_stats_entry = HeatingStatsEntry::new_for_test();
    let mut sql_interface_heating_stats_for_test_setup = result_happy_case.unwrap();
    _ = sql_interface_heating_stats_for_test_setup.insert_heating_stats_entry(&heating_stats_entry);
    heating_stats_entry.increment_date();
    _ = sql_interface_heating_stats_for_test_setup.insert_heating_stats_entry(&heating_stats_entry);

    let midnight_calc = Box::new(SqlInterfaceMidnightCalculator::new(
        sql_interface.get_connection().unwrap(),
    ));
    // Set the limit to 1
    let result = SqlInterfaceHeatingStats::new(conn, 1, midnight_calc);
    assert!(matches!(
        result,
        Err(SqlInterfaceError::DatabaseHeatingStatsTableContainsTooManyRows(_, _, _))
    ));
    println!("* Succeeded: Initialization fails if table exceeds row limit.");

    // --- Test Case 3: Deactivated check (limit is 0) ---
    println!("* Testing new() with a deactivated row limit check (limit = 0)...");
    // We reuse the state from the previous test (2 rows in the table)
    let midnight_calc = Box::new(SqlInterfaceMidnightCalculator::new(
        sql_interface.get_connection().unwrap(),
    ));
    // Set the limit to 0
    let result =
        SqlInterfaceHeatingStats::new(sql_interface.get_connection().unwrap(), 0, midnight_calc);
    assert!(
        result.is_ok(),
        "Expected new() to succeed with deactivated check, but it failed: {:?}",
        result.err()
    );
    println!("* Succeeded: Initialization passes when row limit check is deactivated.");

    // --- Test Case 4: HeatingStatsEntry::new fallback on date query error ---
    println!("* Testing HeatingStatsEntry::new() date fallback on killed connection...");
    let mut kill_conn = sql_interface.get_connection().unwrap();
    let connection_id: u64 = kill_conn
        .query_first("SELECT CONNECTION_ID();")
        .unwrap()
        .unwrap();
    let mut helper_conn = sql_interface.get_connection().unwrap();
    helper_conn
        .query_drop(format!("KILL {};", connection_id))
        .unwrap();

    let entry_fallback = HeatingStatsEntry::new(&mut kill_conn);
    assert_eq!(
        entry_fallback.date,
        Local::now().date_naive(),
        "Expected fallback date to match Local::now()"
    );
    println!("* Succeeded: HeatingStatsEntry::new() date fallback works.");

    // --- Test Case 5: Failure on NULL values in heating_stats ---
    println!("* Testing new() with NULL values in heating_stats table...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_STATS.to_string())
        .expect("Test setup failed: Could not truncate table.");
    let mut conn = sql_interface.get_connection().unwrap();
    conn.query_drop("ALTER TABLE heating_stats MODIFY COLUMN energy DOUBLE NULL;")
        .unwrap();
    conn.query_drop("INSERT INTO heating_stats (date, energy, ambient_temp_average, ambient_temp_counter, water_temp_average, water_temp_counter, heating_control_runtime) VALUES ('2024-01-01', NULL, 20.0, 1, 25.0, 1, 10.0);")
        .unwrap();

    let midnight_calc = Box::new(SqlInterfaceMidnightCalculator::new(
        sql_interface.get_connection().unwrap(),
    ));
    let result = SqlInterfaceHeatingStats::new(conn, 10, midnight_calc);
    assert!(
        matches!(
            result,
            Err(SqlInterfaceError::DatabaseHeatingStatsTableContainsNull(
                _,
                _
            ))
        ),
        "Expected DatabaseHeatingStatsTableContainsNull, but got {:?}",
        result.err()
    );
    println!("* Succeeded: Initialization fails if heating_stats contains NULL values.");

    // Restore heating_stats schema
    let mut conn = sql_interface.get_connection().unwrap();
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_STATS.to_string()).unwrap();
    conn.query_drop("ALTER TABLE heating_stats MODIFY COLUMN energy DOUBLE NOT NULL DEFAULT 0;")
        .unwrap();

    // --- Test Case 6: Failure on killed connection in new() ---
    println!("* Testing new() with killed database connection...");
    let mut kill_conn = sql_interface.get_connection().unwrap();
    let connection_id: u64 = kill_conn
        .query_first("SELECT CONNECTION_ID();")
        .unwrap()
        .unwrap();
    let mut helper_conn = sql_interface.get_connection().unwrap();
    helper_conn
        .query_drop(format!("KILL {};", connection_id))
        .unwrap();

    let midnight_calc = Box::new(SqlInterfaceMidnightCalculator::new(
        sql_interface.get_connection().unwrap(),
    ));
    let result = SqlInterfaceHeatingStats::new(kill_conn, 10, midnight_calc);
    assert!(
        matches!(
            result,
            Err(SqlInterfaceError::DatabaseCheckHeatingStatsFailure { .. })
        ),
        "Expected DatabaseCheckHeatingStatsFailure, but got {:?}",
        result.err()
    );
    println!("* Succeeded: Initialization fails when connection is killed.");
}

#[test]
// [itest->swreq~recorder_record_database~1]
// Test case includes all checks for this table combined in one function to limit the number of additional databases.
// Test case uses test database #65.
pub fn test_sql_interface_heating() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        65,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_heating_stats = config.sql_interface.max_rows_heating_stats;
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let sql_interface_heating_midnight_calculator = Box::new(SqlInterfaceMidnightCalculator::new(
        sql_interface.get_connection().unwrap(),
    ));
    let mut sql_interface_heating = SqlInterfaceHeatingStats::new(
        sql_interface.get_connection().unwrap(),
        max_rows_heating_stats,
        sql_interface_heating_midnight_calculator,
    )
    .unwrap();

    // *** retrieve heating stats - ************************************************************
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_STATS.to_string()) {
        Ok(_) => {}
        Err(e) => panic!("Could not prepare test case: {e:?}"),
    }
    match sql_interface_heating.get_single_heating_stats_from_database() {
        Ok(_) => {
            panic!("Call to get_single_heating_stats_from_database should have failed.");
        }
        Err(e) => {
            assert!(matches!(
                e,
                SqlInterfaceError::SingleHeatingStatsRequestEmptyResponse(_, _)
            ));
        }
    }
    println!("* checking reading from empty heating stat table succeeded.");
    // *****************************************************************************************

    // *** insert heating stats ****************************************************************
    // prepare data for inserting into the database
    let today = match SqlInterface::get_current_date(&mut sql_interface.get_connection().unwrap()) {
        Ok(c) => c,
        Err(_) => panic!("Could not get current date."),
    };
    let mut heating_stats_entry = HeatingStatsEntry {
        date: today,
        energy: 0.9,
        ambient_temperature_average_value: 22.0,
        ambient_temperature_average_counter: 72000,
        water_temperature_average_value: 25.0,
        water_temperature_average_counter: 78000,
        heating_control_runtime: 64000.0,
    };
    assert!(matches!(
        sql_interface_heating.insert_heating_stats_entry(&heating_stats_entry),
        Ok(())
    ));
    println!("* checking inserting heating stats succeeded.");
    // *****************************************************************************************

    // *** retrieve stats **********************************************************************
    match sql_interface_heating.get_single_heating_stats_from_database() {
        Ok(c) => {
            assert_eq!(c.date, today);
            assert_eq!(c.energy, 0.9);
            assert_eq!(c.ambient_temperature_average_value, 22.0);
            assert_eq!(c.ambient_temperature_average_counter, 72000);
            assert_eq!(c.water_temperature_average_value, 25.0);
            assert_eq!(c.water_temperature_average_counter, 78000);
            assert_eq!(c.heating_control_runtime, 64000.0);
        }
        Err(_) => {
            panic!("Call to get_single_heating_stats_from_database has failed.");
        }
    }
    println!("* checking retrieving heating stats succeeded.");

    // *** retrieve stats **********************************************************************
    // update data and trigger SQL statement (ON DUPLICATE UPDATE)
    heating_stats_entry.energy += 0.1;
    heating_stats_entry.ambient_temperature_average_value += 0.1;
    heating_stats_entry.ambient_temperature_average_counter += 1;
    heating_stats_entry.water_temperature_average_value += 0.1;
    heating_stats_entry.water_temperature_average_counter += 1;
    heating_stats_entry.heating_control_runtime += 1.0;
    assert!(matches!(
        sql_interface_heating.insert_heating_stats_entry(&heating_stats_entry),
        Ok(())
    ));
    match sql_interface_heating.get_single_heating_stats_from_database() {
        Ok(c) => {
            assert_eq!(c.date, today);
            assert_eq!(c.energy, 1.0);
            assert_eq!(c.ambient_temperature_average_value, 22.1);
            assert_eq!(c.ambient_temperature_average_counter, 72001);
            assert_eq!(c.water_temperature_average_value, 25.1);
            assert_eq!(c.water_temperature_average_counter, 78001);
            assert_le!(c.heating_control_runtime, 64001.5);
            assert_ge!(c.heating_control_runtime, 63999.5);
        }
        Err(_) => {
            panic!("Call to get_single_heating_stats_from_database has failed.");
        }
    }
    println!("* checking retrieving updated heating stats succeeded.");
    // *****************************************************************************************

    // *** calculate duration until midnight ***************************************************
    // reference calculation in Rust
    // Get current date components
    let now = Local::now();
    let year = now.year();
    let month = now.month();
    let day = now.day();

    // Create NaiveDate from components
    let today: NaiveDate = NaiveDate::from_ymd_opt(year, month, day).unwrap();

    // Create a NaiveDateTime for midnight
    let midnight = NaiveDateTime::new(today, chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    // Calculate the difference
    let duration_until_midnight = 24 * 3600
        - SqlInterface::get_duration_from_naive_timestamp_to_local_now(midnight)
            .unwrap()
            .num_seconds();
    match sql_interface_heating.get_duration_until_midnight() {
        Ok(c) => {
            assert_eq!(c, duration_until_midnight);
        }
        Err(_) => {
            panic!("Call to get_duration_until_midnight failed.");
        }
    }
    println!("* checking duration until midnight in seconds succeeded.");

    // *** check if too many rows are detected ***
    // insert one more entry with modified date
    heating_stats_entry.date =
        SqlInterface::get_tomorrow_date(&mut sql_interface.get_connection().unwrap()).unwrap();
    assert!(matches!(
        sql_interface_heating.insert_heating_stats_entry(&heating_stats_entry),
        Ok(())
    ));
    let sql_interface_heating_midnight_calculator = Box::new(SqlInterfaceMidnightCalculator::new(
        sql_interface.get_connection().unwrap(),
    ));
    let test_result = SqlInterfaceHeatingStats::new(
        sql_interface.get_connection().unwrap(),
        1,
        sql_interface_heating_midnight_calculator,
    );
    assert!(matches!(
        test_result,
        Err(SqlInterfaceError::DatabaseHeatingStatsTableContainsTooManyRows(_, _, _))
    ));
    println!("* detecting too many rows succeeded.");

    // *** check error responses on killed connection ******************************************
    let connection_id: u64 = sql_interface_heating
        .conn
        .query_first("SELECT CONNECTION_ID();")
        .unwrap()
        .unwrap();
    let mut helper_conn = sql_interface.get_connection().unwrap();
    helper_conn
        .query_drop(format!("KILL {};", connection_id))
        .unwrap();

    let insert_err = sql_interface_heating.insert_heating_stats_entry(&heating_stats_entry);
    assert!(
        matches!(
            insert_err,
            Err(SqlInterfaceError::InsertHeatingStatsEntryFailure { .. })
        ),
        "Expected InsertHeatingStatsEntryFailure on killed connection, got {:?}",
        insert_err
    );
    println!(
        "* checking insert_heating_stats_entry error response on killed connection succeeded."
    );

    let get_err = sql_interface_heating.get_single_heating_stats_from_database();
    assert!(
        matches!(
            get_err,
            Err(SqlInterfaceError::SingleHeatingStatsRequestFailure { .. })
        ),
        "Expected SingleHeatingStatsRequestFailure on killed connection, got {:?}",
        get_err.err()
    );
    println!("* checking get_single_heating_stats_from_database error response on killed connection succeeded.");
    // *****************************************************************************************
}
