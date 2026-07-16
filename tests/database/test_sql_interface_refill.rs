/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests all database interactions for the automated water refill system.

use all_asserts::assert_le;
use chrono::{Local, NaiveDateTime, SubsecRound};
use mysql::prelude::Queryable;

use aquarium_control::database::sql_interface_refill::{
    DatabaseInterfaceRefillTrait, DatabaseRefillErrorCode, SqlInterfaceRefill,
};
use aquarium_control::database::sql_test_query_strings::SQL_TABLE_REFILL;
use aquarium_control::database::{sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_test_query_strings};
use aquarium_control::utilities::config::{read_config_file_with_test_database, ConfigData};

pub struct SqlRefillEvent {
    timestamp: String,
    duration: f32,
    volume: f32,
    error_code: i32,
}

pub struct RefillEvent {
    timestamp: NaiveDateTime,
    duration: f32,
    volume: f32,
    error_code: i32,
}
#[test]
// This test case verifies the validation logic within the `SqlInterfaceRefill::new()` constructor.
// It covers the following scenarios:
// 1. Happy Path: Initialization succeeds when the row count is within the limit.
// 2. Failure on Row Limit: Fails when the `refill` table exceeds its max row count.
// 3. Deactivated Check: Succeeds even with excess rows if the limit is set to 0.
// 4. Edge Case: Succeeds when the row count is exactly the limit.
// Test case uses a dedicated test database to avoid conflicts.
fn test_sql_interface_refill_new() {
    // --- Common Setup ---
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        62,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");

    // --- Test Case 1: Happy Path (the row count is within limit) ---
    println!("* Testing new() with valid row count (Happy Path)...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string())
        .expect("Test setup failed: Could not truncate table.");

    // Insert one row
    let mut refill_db = SqlInterfaceRefill {
        conn: sql_interface.get_connection().unwrap(),
    };
    refill_db
        .insert_refill_event(
            Local::now().naive_local(),
            1.0,
            1.0,
            DatabaseRefillErrorCode::Success,
        )
        .expect("Test setup failed: Could not insert row.");

    // Check: new() should succeed with a generous limit of 10
    let result = SqlInterfaceRefill::new(sql_interface.get_connection().unwrap(), 10);
    assert!(
        result.is_ok(),
        "Expected new() to succeed on happy path, but it failed: {:?}",
        result.err()
    );
    println!("* Succeeded: Happy path initialization is successful.");

    // --- Test Case 2: Failure on row limit exceeded ---
    println!("* Testing new() with too many rows in the refill table...");
    // Insert a second row
    let mut refill_db = SqlInterfaceRefill {
        conn: sql_interface.get_connection().unwrap(),
    };
    refill_db
        .insert_refill_event(
            Local::now().naive_local(),
            2.0,
            2.0,
            DatabaseRefillErrorCode::Success,
        )
        .expect("Test setup failed: Could not insert second row.");

    // Check: new() should fail with a strict limit of 1
    let result = SqlInterfaceRefill::new(sql_interface.get_connection().unwrap(), 1);
    assert!(
        matches!(
                result,
                Err(SqlInterfaceError::DatabaseRefillTableContainsTooManyRows(
                    _,
                    _,
                    _
                ))
            ),
        "Expected row limit error, but got {:?}",
        result
    );
    println!("* Succeeded: Initialization fails if refill table exceeds row limit.");

    // --- Test Case 3: Deactivated check (limit is 0) ---
    println!("* Testing new() with deactivated row limit check (limit = 0)...");
    // We reuse the state from the previous test (2 rows in the table)
    // Set the limit to 0
    let result = SqlInterfaceRefill::new(sql_interface.get_connection().unwrap(), 0);
    assert!(
        result.is_ok(),
        "Expected new() to succeed with deactivated check, but it failed: {:?}",
        result.err()
    );
    println!("* Succeeded: Initialization passes when row limit check is deactivated.");

    // --- Test Case 4: Edge case (row count equals limit) ---
    println!("* Testing new() with row count equal to the limit...");
    // We reuse the state from the previous test (2 rows in the table)
    // Set the limit to 2
    let result = SqlInterfaceRefill::new(sql_interface.get_connection().unwrap(), 2);
    assert!(
        result.is_ok(),
        "Expected new() to succeed when row count equals limit, but it failed: {:?}",
        result.err()
    );
    println!("* Succeeded: Initialization passes when row count is exactly the limit.");
}

#[test]
// Test case includes all checks for this table combined in one function to limit the number of additional databases.
// Test case uses test database #37.
pub fn test_sql_interface_refill() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        37,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_refill = config.sql_interface.max_rows_refill;
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut sql_interface_refill =
        SqlInterfaceRefill::new(sql_interface.get_connection().unwrap(), max_rows_refill)
            .unwrap();

    // *** check timestamp of last refill *************************************************
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not prepare test case: {e:?}")
        }
    }
    assert!(sql_interface_refill.check_empty().is_ok());
    assert!(matches!(
            sql_interface_refill.get_last_refill_timestamp(),
            Err(SqlInterfaceError::TimestampRequestFailure {
                location: _,
                query: _
            })
        ));
    println!("* checking timestamp of last refill (FAIL) succeeded.");
    // **************************************************************************************

    // *** check timestamp of last refill - ****************************************************
    let refill_timestamp =
        NaiveDateTime::parse_from_str("2025-04-12 07:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
    match sql_interface_refill.insert_refill_event(
        refill_timestamp,
        3.6,
        0.4,
        DatabaseRefillErrorCode::Success,
    ) {
        Ok(_) => {}
        Err(e) => panic!("Could not insert refill event into database: {e:?}"),
    };
    assert!(matches!(sql_interface_refill.check_empty(), Ok(1)));
    assert!(sql_interface_refill.get_last_refill_timestamp().is_ok());
    println!("* checking timestamp of last refill succeeded.");
    //**************************************************************************************

    //*** check duration since last refill - **************************************************
    let duration_since_last_refill = match sql_interface_refill.get_duration_since_last_refill()
    {
        Ok(c) => c,
        Err(e) => panic!("Could not calculate duration since last refill: {e:?}"),
    };
    // Reference calculation uses the database clock to avoid timezone and DST issues
    let db_now = SqlInterface::get_current_timestamp(&mut sql_interface_refill.conn)
        .expect("Failed to get current timestamp from DB for reference calculation.");
    let reference_duration_since_last_refill = (db_now - refill_timestamp).num_seconds();

    let duration_delta_seconds =
        duration_since_last_refill.abs_diff(reference_duration_since_last_refill as u64);
    assert_le!(duration_delta_seconds, 1);

    println!("* checking duration since last refill succeeded.");
    //**************************************************************************************

    // *** check refill count of last 24h (zero) - *********************************************
    let refill_count_of_last_24h_zero =
        match sql_interface_refill.get_refill_count_of_last_24h() {
            Ok(c) => c,
            Err(e) => panic!("Could not get refill count from last 24h (target=0): {e:?}"),
        };
    assert_eq!(refill_count_of_last_24h_zero, 0);
    println!("* checking refill count of last 24h (target zero) succeeded.");
    // **************************************************************************************

    // *** check refill count of last hour (zero) - ********************************************
    let refill_count_of_last_hour_zero =
        match sql_interface_refill.get_refill_count_of_last_hour() {
            Ok(c) => c,
            Err(e) => panic!("Could not get refill count from last hour (target=0): {e:?}"),
        };
    assert_eq!(refill_count_of_last_hour_zero, 0);
    println!("* checking refill count of last hour (target zero) succeeded.");
    // **************************************************************************************

    //*** check refill volume of last 24h (zero) - *********************************************
    let refill_volume_of_last_24h_zero =
        match sql_interface_refill.get_refill_volume_of_last_24h() {
            Ok(c) => c,
            Err(e) => panic!("Could not get refill volume from last 24h (target=0): {e:?}"),
        };
    assert_eq!(refill_volume_of_last_24h_zero, 0.0);
    println!("* checking refill volume of last 24h (target zero) succeeded.");
    // **************************************************************************************

    //*** check refill volume of last hour (zero) - *********************************************
    let refill_volume_of_last_hour_zero =
        match sql_interface_refill.get_refill_volume_of_last_hour() {
            Ok(c) => c,
            Err(e) => panic!("Could not get refill volume from last hour (target=0): {e:?}"),
        };
    assert_eq!(refill_volume_of_last_hour_zero, 0.0);
    println!("* checking refill volume of last hour (target zero) succeeded.");
    // **************************************************************************************

    // *** check refill count of last 24h (one) - *********************************************
    let naive_datetime_12h_ago = SqlInterface::get_naive_timestamp_from_past(12, 0);
    match sql_interface_refill.insert_refill_event(
        naive_datetime_12h_ago,
        5.0,
        0.25,
        DatabaseRefillErrorCode::StuckLowTimeout,
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not insert refill event in data base for test preparation: {e:?}")
        }
    }
    let refill_count_of_last_24h_one = match sql_interface_refill.get_refill_count_of_last_24h()
    {
        Ok(c) => c,
        Err(e) => panic!("Could not get refill count from last 24h (target=1): {e:?}"),
    };
    assert_eq!(refill_count_of_last_24h_one, 1);
    println!("* checking refill count of last 24h (target non-zero) succeeded.");
    // **************************************************************************************

    // *** check refill volume of last 24h (one) - *********************************************
    let refill_volume_of_last_24h_one =
        match sql_interface_refill.get_refill_volume_of_last_24h() {
            Ok(c) => c,
            Err(e) => panic!("Could not get refill volume from last 24h (target=0.25): {e:?}"),
        };
    assert_eq!(refill_volume_of_last_24h_one, 0.25);
    println!("* checking refill volume (target non-zero) succeeded.");
    // **************************************************************************************

    // *** check refill count of last hour (one) - *********************************************
    let naive_datetime_30min_ago = SqlInterface::get_naive_timestamp_from_past(0, 30);
    match sql_interface_refill.insert_refill_event(
        naive_datetime_30min_ago,
        5.0,
        0.25,
        DatabaseRefillErrorCode::StuckLowTimeout,
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Could not insert refill event in data base for test preparation: {e:?}")
        }
    }
    let refill_count_of_last_hour_one =
        match sql_interface_refill.get_refill_count_of_last_hour() {
            Ok(c) => c,
            Err(e) => panic!("Could not get refill count from last hour (target=1): {e:?}"),
        };
    assert_eq!(refill_count_of_last_hour_one, 1);
    println!("* checking refill count of last hour (target non-zero) succeeded.");
    // **************************************************************************************

    // *** check refill volume of last hour (one) - *********************************************
    let refill_volume_of_last_hour_one =
        match sql_interface_refill.get_refill_volume_of_last_hour() {
            Ok(c) => c,
            Err(e) => panic!("Could not get refill volume from last hour (target=0.25): {e:?}"),
        };
    assert_eq!(refill_volume_of_last_hour_one, 0.25);
    println!("* checking refill volume (target non-zero) succeeded.");
    // **************************************************************************************

    // *** check the content of last refill event - ********************************************
    // Read back refill event entry from the database
    let single_refill_event_entry_array = match sql_interface_refill.conn.query_map(
        sql_test_query_strings::SQL_QUERY_READ_REFILL_EVENT,
        |(timestamp, duration, volume, error_code)| SqlRefillEvent {
            timestamp,
            duration,
            volume,
            error_code,
        },
    ) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not read refill event from data base: {e:?}");
        }
    };
    // check if SQL response has the right structure
    assert_eq!(single_refill_event_entry_array.len(), 1);
    // check if SQL response contains the correct data
    let refill_event = RefillEvent {
        timestamp: NaiveDateTime::parse_from_str(
            single_refill_event_entry_array[0].timestamp.as_str(),
            "%Y-%m-%d %H:%M:%S",
        )
            .expect("conversion of timestamp to NaiveDate failed."),
        duration: single_refill_event_entry_array[0].duration,
        volume: single_refill_event_entry_array[0].volume,
        error_code: single_refill_event_entry_array[0].error_code,
    };
    assert_eq!(
        refill_event.timestamp,
        naive_datetime_30min_ago.trunc_subsecs(0)
    );
    assert_eq!(refill_event.duration, 5.0);
    assert_eq!(refill_event.volume, 0.25);
    assert_eq!(refill_event.error_code, -2);

    // *** Check max. number of rows detection ***
    // refill table already has three rows at this point.
    let test_result = SqlInterfaceRefill::new(sql_interface.get_connection().unwrap(), 1);
    assert!(matches!(
            test_result,
            Err(SqlInterfaceError::DatabaseRefillTableContainsTooManyRows(
                _,
                _,
                _
            ))
        ));
    // **************************************************************************************
}
