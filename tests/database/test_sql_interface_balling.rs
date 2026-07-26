/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests all database interactions for the Balling dosing system.

use crate::database::common::add_balling_profile;
use all_asserts::assert_le;
use aquarium_control::database::sql_interface_balling::SqlInterfaceBalling;
use aquarium_control::database::sql_test_query_strings::{
    SQL_QUERY_WRITE_BALLING_EVENT_WITH_TIMESTAMP, SQL_TABLE_BALLING_DOSING_LOG,
    SQL_TABLE_BALLING_SET_VALS,
};
use aquarium_control::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_test_query_strings,
};
use aquarium_control::utilities::config::{read_config_file_with_test_database, ConfigData};
use assert_float_eq::assert_float_absolute_eq;
use chrono::Local;
use chrono::NaiveDateTime;
use mysql::{params, prelude::Queryable};
use spin_sleep::SpinSleeper;
use std::time::Duration;

pub struct SqlBallingEvent {
    timestamp: String,
    pump_id: i32,
    dosing_volume: f32,
}

pub struct BallingEvent {
    timestamp: NaiveDateTime,
    pump_id: i32,
    dosing_volume: f32,
}

#[test]
// This test case verifies the validation logic within the `SqlInterfaceBalling::new()` constructor.
// It covers the following scenarios:
// 1. Happy Path: Initialization succeeds with valid data and row counts.
// 2. Failure on Row Limit (SetVals): Fails when `ballingsetvals` exceeds its max row count.
// 3. Failure on Row Limit (DosingLog): Fails when `ballingdosinglog` exceeds its max row count.
// 4. Deactivated Checks: Succeeds even with excess rows if the limits are set to 0.
// Test case uses test database #57.
// [itest->swreq~mineral_read_profiles~1]
/// [itest->swreq~mineral_read_profiles~1]
fn test_sql_interface_balling_new() {
    // --- Common Setup ---
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        57,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");

    // --- Test Case 1: Happy Path ---
    println!("* Testing new() with valid data (Happy Path)...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_BALLING_SET_VALS.to_string())
        .unwrap();
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_BALLING_DOSING_LOG.to_string())
        .unwrap();

    // Insert one valid row into each table
    let mut conn = sql_interface.get_connection().unwrap();
    add_balling_profile(&mut conn, 1, 1.0, 1.0, "valid".to_string());
    conn.exec_drop(
        SQL_QUERY_WRITE_BALLING_EVENT_WITH_TIMESTAMP,
        params! { "timestamp" => Local::now().naive_local(), "pump_id" => 1, "dosing_volume" => 1.0 },
    ).unwrap();

    let result = SqlInterfaceBalling::new(conn, 10, 10);
    assert!(
        result.is_ok(),
        "Expected new() to succeed on happy path, but it failed: {:?}",
        result.err()
    );
    println!("* Succeeded: Happy path initialization is successful.");

    // --- Test Case 2: Failure on ballingsetvals row limit exceeded ---
    println!("* Testing new() with too many rows in ballingsetvals...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_BALLING_SET_VALS.to_string())
        .unwrap();
    let mut conn = sql_interface.get_connection().unwrap();
    add_balling_profile(&mut conn, 1, 1.0, 1.0, "row1".to_string());
    add_balling_profile(&mut conn, 2, 1.0, 1.0, "row2".to_string());

    let result = SqlInterfaceBalling::new(conn, 1, 10); // Set the limit to 1
    assert!(
        matches!(
            result,
            Err(SqlInterfaceError::DatabaseBallingSetValTableContainsTooManyRows(_, _, _))
        ),
        "Expected row limit error for setvals, but got {:?}",
        result
    );
    println!("* Succeeded: Initialization fails if ballingsetvals exceeds row limit.");

    // --- Test Case 3: Failure on ballingdosinglog row limit exceeded ---
    println!("* Testing new() with too many rows in ballingdosinglog...");
    let spin_sleeper = SpinSleeper::default();
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_BALLING_DOSING_LOG.to_string())
        .unwrap();
    let mut conn = sql_interface.get_connection().unwrap();
    conn.exec_drop(SQL_QUERY_WRITE_BALLING_EVENT_WITH_TIMESTAMP, params! { "timestamp" => Local::now().naive_local(), "pump_id" => 1, "dosing_volume" => 1.0 }).unwrap();
    spin_sleeper.sleep(Duration::from_millis(500));
    conn.exec_drop(SQL_QUERY_WRITE_BALLING_EVENT_WITH_TIMESTAMP, params! { "timestamp" => Local::now().naive_local(), "pump_id" => 2, "dosing_volume" => 1.0 }).unwrap();

    let result = SqlInterfaceBalling::new(conn, 10, 1); // Set the limit to 1
    assert!(
        matches!(
            result,
            Err(SqlInterfaceError::DatabaseBallingDosingLogTableContainsTooManyRows(_, _, _))
        ),
        "Expected row limit error for the dosing log, but got {:?}",
        result
    );
    println!("* Succeeded: Initialization fails if ballingdosinglog exceeds row limit.");

    // --- Test Case 4: Deactivated checks (limits are 0) ---
    println!("* Testing new() with deactivated row limit checks (limits = 0)...");
    // We reuse the state from the previous test (2 rows in the dosing log)
    let conn = sql_interface.get_connection().unwrap();
    let result = SqlInterfaceBalling::new(conn, 0, 0); // Set limits to 0
    assert!(
        result.is_ok(),
        "Expected new() to succeed with deactivated checks, but it failed: {:?}",
        result.err()
    );
    println!("* Succeeded: Initialization passes when row limit checks are deactivated.");
}

#[test]
// Test case verifies the behavior of all functions accessing the Balling event log table.
// They are all combined in one function to limit the number of additional databases.
// Test case uses test database #30.
/// [itest->swreq~mineral_log_database~1]
pub fn test_sql_interface_balling_events() {
    let pump_id = 1;
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        30,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
    let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut sql_interface_balling = SqlInterfaceBalling::new(
        sql_interface.get_connection().unwrap(),
        max_rows_balling_set_values,
        max_rows_balling_dosing_log,
    )
    .unwrap();

    // *** check the timestamp of last Balling dosing ******************************************
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_BALLING_DOSING_LOG.to_string())
    {
        Ok(_) => {}
        Err(e) => panic!("Could not prepare test case: {e:?}"),
    }
    assert!(matches!(
        sql_interface_balling.get_last_balling_dosing_timestamp(pump_id),
        Ok(None)
    ));
    println!("* checking timestamp of last Balling dosing log succeeded.");
    // *****************************************************************************************

    // *** check timestamp of last Balling dosing **********************************************
    let balling_timestamp =
        NaiveDateTime::parse_from_str("2025-04-12 11:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
    match sql_interface_balling.insert_balling_event_for_test(balling_timestamp, pump_id, 0.5) {
        Ok(_) => {}
        Err(e) => panic!("Could not insert Balling dosing event into database: {e:?}"),
    };
    assert!(sql_interface_balling
        .get_last_balling_dosing_timestamp(pump_id)
        .is_ok());
    println!("* checking timestamp of last Balling dosing succeeded.");
    // *****************************************************************************************

    // *** check the content of last Balling dosing ********************************************
    // Read back Balling dosing event entry from the database
    let single_balling_event_entry_array = match sql_interface_balling.conn.query_map(
        sql_test_query_strings::SQL_QUERY_READ_BALLING_EVENT,
        |(timestamp, pump_id, dosing_volume)| SqlBallingEvent {
            timestamp,
            pump_id,
            dosing_volume,
        },
    ) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not read Balling dosing event from data base: {e:?}");
        }
    };
    // check if SQL response has the right structure
    assert_eq!(single_balling_event_entry_array.len(), 1);
    // check if SQL response contains the correct data
    let balling_event = BallingEvent {
        timestamp: NaiveDateTime::parse_from_str(
            single_balling_event_entry_array[0].timestamp.as_str(),
            "%Y-%m-%d %H:%M:%S",
        )
        .expect("conversion of timestamp to NaiveDate failed."),
        pump_id: single_balling_event_entry_array[0].pump_id,
        dosing_volume: single_balling_event_entry_array[0].dosing_volume,
    };
    assert_eq!(balling_event.timestamp, balling_timestamp);
    assert_eq!(balling_event.pump_id, 1);
    assert_eq!(balling_event.dosing_volume, 0.5);
    // *****************************************************************************************

    // *** check duration since last Balling dosing ********************************************
    let duration_since_last_balling_dosing =
        match sql_interface_balling.get_duration_since_last_balling_dosing(1) {
            Ok(c) => c,
            Err(e) => panic!("Could not calculate duration since last Balling dosing: {e:?}"),
        }
        .unwrap();

    // Reference calculation uses the database clock to avoid timezone and DST issues
    let db_now = SqlInterface::get_current_timestamp(&mut sql_interface_balling.conn)
        .expect("Failed to get current timestamp from DB for reference calculation.");
    let reference_duration_since_last_balling_dosing = (db_now - balling_timestamp).num_seconds();

    let duration_delta_seconds = duration_since_last_balling_dosing
        .as_secs()
        .abs_diff(reference_duration_since_last_balling_dosing as u64);
    assert_le!(duration_delta_seconds, 1);
    println!("* checking duration since last Balling dosing succeeded.");
    // *****************************************************************************************
}

#[test]
// Test case verifies the behavior of all functions accessing the Balling set value table.
// They are all combined in one function to limit the number of additional databases.
// Test case uses test database #31.
pub fn test_sql_interface_balling_set_vals() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        31,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
    let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut sql_interface_balling = SqlInterfaceBalling::new(
        sql_interface.get_connection().unwrap(),
        max_rows_balling_set_values,
        max_rows_balling_dosing_log,
    )
    .unwrap();

    // *** read balling set vals from empty table **********************************************
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_BALLING_SET_VALS.to_string()) {
        Ok(_) => {}
        Err(e) => panic!("Could not prepare test case: {e:?}"),
    }
    match sql_interface_balling.get_single_balling_setval_from_database(1) {
        Ok(_) => {
            panic!("Reading from empty set value table returned value.");
        }
        Err(e) => {
            assert!(matches!(
                e,
                SqlInterfaceError::SingleBallingSetValEmptyResponse(_, _, _)
            ));
        }
    }
    println!("* checking reading from empty Balling dosing set value table succeeded.");
    // *****************************************************************************************

    // *** read balling set vals from pre-filled table *****************************************
    // prepare database: insert Balling set vals
    add_balling_profile(
        &mut sql_interface_balling.conn,
        1,
        2.0,
        0.5,
        "test_pump".to_string(),
    );

    match sql_interface_balling.get_single_balling_setval_from_database(1) {
        Ok(c) => {
            assert_eq!(c.pump_id, 1);
            assert_eq!(c.dosing_speed, 2.0);
            assert_eq!(c.dosing_volume, 0.5);
            assert_eq!(c.label, "test_pump");
        }
        Err(e) => {
            panic!("Error when reading from pre-filled Balling set value table: {e:?}");
        }
    }
    println!("* checking reading existing Balling dosing set values succeeded.");

    // *** check if instantiation of SqlInterfaceBalling checks maximum number of rows ******
    // add one more Balling set value row
    add_balling_profile(
        &mut sql_interface_balling.conn,
        2,
        3.0,
        0.5,
        "test_pump2".to_string(),
    );
    let test_result = SqlInterfaceBalling::new(sql_interface.get_connection().unwrap(), 1, 0);
    assert!(matches!(
        test_result,
        Err(SqlInterfaceError::DatabaseBallingSetValTableContainsTooManyRows(_, _, _))
    ));

    // **************************************************************************************
}

#[test]
// This test case verifies the `insert_balling_event` function.
// It ensures that a Balling dosing event is correctly inserted into the database
// with the current database timestamp.
// Test case uses test database #66.
fn test_insert_balling_event() {
    let pump_id = 1;
    let dosing_volume = 1.5;
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        66,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
    let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut sql_interface_balling = SqlInterfaceBalling::new(
        sql_interface.get_connection().unwrap(),
        max_rows_balling_set_values,
        max_rows_balling_dosing_log,
    )
    .unwrap();

    // Clear the dosing log table
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_BALLING_DOSING_LOG.to_string())
    {
        Ok(_) => {}
        Err(e) => panic!("Could not prepare test case: {e:?}"),
    }

    // Insert a Balling event
    match sql_interface_balling.insert_balling_event(pump_id, dosing_volume) {
        Ok(_) => {}
        Err(e) => panic!("Could not insert Balling dosing event into database: {e:?}"),
    };

    // Read back the event to verify
    let single_balling_event_entry_array = match sql_interface_balling.conn.query_map(
        sql_test_query_strings::SQL_QUERY_READ_BALLING_EVENT,
        |(timestamp, pump_id, dosing_volume)| SqlBallingEvent {
            timestamp,
            pump_id,
            dosing_volume,
        },
    ) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not read Balling dosing event from data base: {e:?}");
        }
    };

    // Check if SQL response has the right structure
    assert_eq!(single_balling_event_entry_array.len(), 1);
    // Check if SQL response contains the correct data
    let event = &single_balling_event_entry_array[0];
    assert_eq!(event.pump_id, pump_id as i32);
    assert_float_absolute_eq!(event.dosing_volume, dosing_volume as f32, 0.001);

    // Verify timestamp is recent (within last few seconds)
    let event_timestamp =
        NaiveDateTime::parse_from_str(event.timestamp.as_str(), "%Y-%m-%d %H:%M:%S")
            .expect("conversion of timestamp to NaiveDate failed.");
    let db_now = SqlInterface::get_current_timestamp(&mut sql_interface_balling.conn)
        .expect("Failed to get current timestamp from DB.");
    let duration_diff = (db_now - event_timestamp).num_seconds().abs();
    assert_le!(duration_diff, 5, "Timestamp should be recent");

    println!("* checking insert_balling_event succeeded.");
}
