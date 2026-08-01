/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Testing of cor SQL database interface.

use chrono::{DateTime, Local, NaiveDate, SubsecRound, TimeZone};
use mysql::params;
use mysql::prelude::Queryable;

use crate::database::common::insert_mock_version_information;
use aquarium_control::database::sql_interface::{SqlInterface, SqlInterfaceError};
use aquarium_control::database::sql_test_query_strings::SQL_QUERY_WRITE_VERSION_INFORMATION;
use aquarium_control::utilities::config::{read_config_file_with_test_database, ConfigData};
use aquarium_control::utilities::version_information::VersionInformation;

#[test]
// This test case checks the functionality of get_string_array_from_database.
// - response when querying an empty table (should be Ok with an empty Vec)
// - response when querying a table with one entry (should be Ok with a Vec of 1)
// - response when querying a table with multiple entries (should be Ok with a Vec of n)
// Test case uses test database #56.
fn test_get_string_array_from_database() {
    // Setup
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        56,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut conn = sql_interface.get_connection().unwrap();

    let table_name = "test_string_array";

    // Create a temporary table for the test
    conn.query_drop(format!(
        "CREATE TABLE IF NOT EXISTS {} (name VARCHAR(255));",
        table_name
    ))
    .unwrap();

    // --- Test Case 1: Empty table ---
    println!("* Testing string array with empty table...");
    conn.query_drop(format!("TRUNCATE TABLE {}", table_name))
        .unwrap();

    let result = SqlInterface::get_string_array_from_database(
        &mut conn,
        &format!("SELECT name FROM {}", table_name),
    );
    let result_vec = result.unwrap();
    assert!(
        result_vec.is_empty(),
        "Expected an empty vector for an empty table"
    );
    println!("* Succeeded: Empty table returns an empty Vec.");

    // --- Test Case 2: Table with one entry ---
    println!("* Testing string array with a single entry...");
    let test_string_1 = "Hello";
    conn.exec_drop(
        format!("INSERT INTO {} (name) VALUES (:name)", table_name),
        params! { "name" => test_string_1 },
    )
    .unwrap();

    let result = SqlInterface::get_string_array_from_database(
        &mut conn,
        &format!("SELECT name FROM {}", table_name),
    );
    let result_vec = result.unwrap();
    assert_eq!(result_vec.len(), 1);
    assert_eq!(result_vec[0].single_string, test_string_1);
    println!("* Succeeded: Table with one entry returns a Vec with one element.");

    // --- Test Case 3: Table with multiple entries ---
    println!("* Testing string array with multiple entries...");
    let test_string_2 = "World";
    conn.exec_drop(
        format!("INSERT INTO {} (name) VALUES (:name)", table_name),
        params! { "name" => test_string_2 },
    )
    .unwrap();

    // Use ORDER BY to ensure a predictable order for assertions
    let result = SqlInterface::get_string_array_from_database(
        &mut conn,
        &format!("SELECT name FROM {} ORDER BY name ASC", table_name),
    );
    let result_vec = result.unwrap();
    assert_eq!(result_vec.len(), 2);
    assert_eq!(result_vec[0].single_string, test_string_1); // "Hello"
    assert_eq!(result_vec[1].single_string, test_string_2); // "World"
    println!("* Succeeded: Table with multiple entries returns a Vec with all elements.");

    // --- Test Case 4: Invalid SQL Query Error Handling ---
    println!("* Testing string array with invalid SQL query...");
    let invalid_result = SqlInterface::get_string_array_from_database(
        &mut conn,
        "SELECT invalid_col FROM nonexistent_table_xyz_123",
    );
    assert!(matches!(
        invalid_result,
        Err(SqlInterfaceError::SingleStringRequestFailure { .. })
    ));
    println!("* Succeeded: Invalid SQL query returns SingleStringRequestFailure.");

    // Cleanup
    conn.query_drop(format!("DROP TABLE {}", table_name))
        .unwrap();
}

#[test]
// This test case checks the functionality and error handling of get_single_string_from_database.
// - response when querying an empty table (should be Err(SingleStringRequestEmptyResponse))
// - response when querying a table with one entry (should be Ok(String))
// - response when querying a table with multiple entries (should be Err(SingleStringRequestNoSingleResponse))
// Test case uses test database #56.
pub fn test_get_single_string_from_database() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        56,
    );
    let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut conn = sql_interface.get_connection().unwrap();

    let table_name = "test_single_string";
    conn.query_drop(format!(
        "CREATE TABLE IF NOT EXISTS {} (name VARCHAR(255));",
        table_name
    ))
    .unwrap();
    conn.query_drop(format!("TRUNCATE TABLE {}", table_name))
        .unwrap();

    // 1. Test empty response (0 rows)
    let res_empty = SqlInterface::get_single_string_from_database(
        &mut conn,
        &format!("SELECT name FROM {}", table_name),
    );
    assert!(matches!(
        res_empty,
        Err(SqlInterfaceError::SingleStringRequestEmptyResponse(_, _))
    ));

    // 2. Test happy path (1 row)
    conn.exec_drop(
        format!("INSERT INTO {} (name) VALUES (:name)", table_name),
        params! { "name" => "hello" },
    )
    .unwrap();
    let res_single = SqlInterface::get_single_string_from_database(
        &mut conn,
        &format!("SELECT name FROM {}", table_name),
    );
    assert_eq!(res_single.unwrap(), "hello");

    // 3. Test multiple responses (>1 rows)
    conn.exec_drop(
        format!("INSERT INTO {} (name) VALUES (:name)", table_name),
        params! { "name" => "world" },
    )
    .unwrap();
    let res_multi = SqlInterface::get_single_string_from_database(
        &mut conn,
        &format!("SELECT name FROM {}", table_name),
    );
    assert!(matches!(
        res_multi,
        Err(SqlInterfaceError::SingleStringRequestNoSingleResponse(_, _))
    ));

    // Cleanup
    conn.query_drop(format!("DROP TABLE {}", table_name))
        .unwrap();
}

#[test]
// This test case checks the functionality and error handling of get_multiple_strings_from_database.
// - response when querying an empty table (should be Err(MultipleStringRequestEmptyResponse))
// - response when querying a table with one entry (should be Err(MultipleStringRequestSingleResponse))
// - response when querying a table with multiple entries (should be Ok(Vec<String>))
// - response when executing an invalid SQL query (should be Err(SingleStringRequestFailure))
// Test case uses dedicated test database #57.
pub fn test_get_multiple_strings_from_database() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        57,
    );
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut conn = sql_interface.get_connection().unwrap();

    let table_name = "test_multiple_strings";
    conn.query_drop(format!(
        "CREATE TABLE IF NOT EXISTS {} (name VARCHAR(255));",
        table_name
    ))
    .unwrap();
    conn.query_drop(format!("TRUNCATE TABLE {}", table_name))
        .unwrap();

    // 1. Test empty table response (0 rows)
    let res_empty = sql_interface
        .get_multiple_strings_from_database(&format!("SELECT name FROM {}", table_name));
    assert!(matches!(
        res_empty,
        Err(SqlInterfaceError::MultipleStringRequestEmptyResponse(_, _))
    ));

    // 2. Test single row response (1 row)
    conn.exec_drop(
        format!("INSERT INTO {} (name) VALUES (:name)", table_name),
        params! { "name" => "item1" },
    )
    .unwrap();
    let res_single = sql_interface
        .get_multiple_strings_from_database(&format!("SELECT name FROM {}", table_name));
    assert!(matches!(
        res_single,
        Err(SqlInterfaceError::MultipleStringRequestSingleResponse(_, _))
    ));

    // 3. Test multiple rows response (2+ rows - Happy Path)
    conn.exec_drop(
        format!("INSERT INTO {} (name) VALUES (:name)", table_name),
        params! { "name" => "item2" },
    )
    .unwrap();
    let res_multi = sql_interface.get_multiple_strings_from_database(&format!(
        "SELECT name FROM {} ORDER BY name ASC",
        table_name
    ));
    assert_eq!(
        res_multi.unwrap(),
        vec!["item1".to_string(), "item2".to_string()]
    );

    // 4. Test invalid query syntax / missing table error
    let res_invalid = sql_interface
        .get_multiple_strings_from_database("SELECT invalid_col FROM nonexistent_table_xyz_999");
    assert!(matches!(
        res_invalid,
        Err(SqlInterfaceError::SingleStringRequestFailure { .. })
    ));

    // Cleanup
    conn.query_drop(format!("DROP TABLE {}", table_name))
        .unwrap();
}

#[test]
// This test case checks the functionality and error handling of get_single_integer_from_database.
// - response when querying an empty table (should be Err(SingleIntegerRequestNoSingleResponse))
// - response when querying a table with one entry (should be Ok(i64))
// - response when querying a table with multiple entries (should be Err(SingleIntegerRequestNoSingleResponse))
// - response when executing an invalid SQL query (should be Err(SingleIntegerRequestFailure))
// Test case uses dedicated test database #58.
pub fn test_get_single_integer_from_database() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        58,
    );
    let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut conn = sql_interface.get_connection().unwrap();

    let table_name = "test_single_integer";
    conn.query_drop(format!(
        "CREATE TABLE IF NOT EXISTS {} (val INT);",
        table_name
    ))
    .unwrap();
    conn.query_drop(format!("TRUNCATE TABLE {}", table_name))
        .unwrap();

    // 1. Test empty table response (0 rows)
    let res_empty = SqlInterface::get_single_integer_from_database(
        &mut conn,
        &format!("SELECT val FROM {}", table_name),
    );
    assert!(matches!(
        res_empty,
        Err(SqlInterfaceError::SingleIntegerRequestNoSingleResponse(
            _,
            _
        ))
    ));

    // 2. Test single row response (1 row - Happy Path)
    conn.exec_drop(
        format!("INSERT INTO {} (val) VALUES (:val)", table_name),
        params! { "val" => 42 },
    )
    .unwrap();
    let res_single = SqlInterface::get_single_integer_from_database(
        &mut conn,
        &format!("SELECT val FROM {}", table_name),
    );
    assert_eq!(res_single.unwrap(), 42);

    // 3. Test multiple rows response (> 1 rows)
    conn.exec_drop(
        format!("INSERT INTO {} (val) VALUES (:val)", table_name),
        params! { "val" => 99 },
    )
    .unwrap();
    let res_multi = SqlInterface::get_single_integer_from_database(
        &mut conn,
        &format!("SELECT val FROM {}", table_name),
    );
    assert!(matches!(
        res_multi,
        Err(SqlInterfaceError::SingleIntegerRequestNoSingleResponse(
            _,
            _
        ))
    ));

    // 4. Test invalid query syntax / missing table error
    let res_invalid = SqlInterface::get_single_integer_from_database(
        &mut conn,
        "SELECT invalid_col FROM nonexistent_table_xyz_888",
    );
    assert!(matches!(
        res_invalid,
        Err(SqlInterfaceError::SingleIntegerRequestFailure(_, _))
    ));

    // Cleanup
    conn.query_drop(format!("DROP TABLE {}", table_name))
        .unwrap();
}

#[test]
// This test case checks the functionality and error handling of get_single_float_from_database.
// - response when querying an empty table (should be Err(SingleFloatRequestEmptyResponse))
// - response when querying a table with one entry (should be Ok(f64))
// - response when querying a table with multiple entries (should be Err(SingleFloatRequestNoSingleResponse))
// - response when executing an invalid SQL query (should be Err(SingleFloatRequestFailure))
// Test case uses dedicated test database #60.
pub fn test_get_single_float_from_database() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        60,
    );
    let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut conn = sql_interface.get_connection().unwrap();

    let table_name = "test_single_float";
    conn.query_drop(format!(
        "CREATE TABLE IF NOT EXISTS {} (val DOUBLE);",
        table_name
    ))
    .unwrap();
    conn.query_drop(format!("TRUNCATE TABLE {}", table_name))
        .unwrap();

    // 1. Test empty table response (0 rows)
    let res_empty = SqlInterface::get_single_float_from_database(
        &mut conn,
        &format!("SELECT val FROM {}", table_name),
    );
    assert!(matches!(
        res_empty,
        Err(SqlInterfaceError::SingleFloatRequestEmptyResponse(_, _))
    ));

    // 2. Test single row response (1 row - Happy Path)
    conn.exec_drop(
        format!("INSERT INTO {} (val) VALUES (:val)", table_name),
        params! { "val" => 123.456 },
    )
    .unwrap();
    let res_single = SqlInterface::get_single_float_from_database(
        &mut conn,
        &format!("SELECT val FROM {}", table_name),
    );
    assert!((res_single.unwrap() - 123.456).abs() < 0.00001);

    // 3. Test multiple rows response (> 1 rows)
    conn.exec_drop(
        format!("INSERT INTO {} (val) VALUES (:val)", table_name),
        params! { "val" => 789.012 },
    )
    .unwrap();
    let res_multi = SqlInterface::get_single_float_from_database(
        &mut conn,
        &format!("SELECT val FROM {}", table_name),
    );
    assert!(matches!(
        res_multi,
        Err(SqlInterfaceError::SingleFloatRequestNoSingleResponse(_, _))
    ));

    // 4. Test invalid query syntax / missing table error
    let res_invalid = SqlInterface::get_single_float_from_database(
        &mut conn,
        "SELECT invalid_col FROM nonexistent_table_xyz_777",
    );
    assert!(matches!(
        res_invalid,
        Err(SqlInterfaceError::SingleFloatRequestFailure(_, _))
    ));

    // Cleanup
    conn.query_drop(format!("DROP TABLE {}", table_name))
        .unwrap();
}

#[test]
// This test case checks the functionality and error handling of get_timestamp.
// - Happy Path: Returns Ok(NaiveDateTime) for valid timestamp query.
// - 0 Rows: Returns Err(TimestampRequestFailure) when query returns no rows.
// - Invalid Query: Returns Err(TimestampRequestFailure) when query syntax is invalid.
// Test case uses dedicated test database #62.
pub fn test_get_timestamp() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        62,
    );
    let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut conn = sql_interface.get_connection().unwrap();

    // 1. Happy Path
    let res_valid = SqlInterface::get_timestamp(&mut conn, "SELECT CURRENT_TIMESTAMP() LIMIT 1;");
    assert!(res_valid.is_ok());

    // 2. 0 Rows (None -> Err)
    let res_no_rows =
        SqlInterface::get_timestamp(&mut conn, "SELECT CURRENT_TIMESTAMP() WHERE 1=0;");
    assert!(matches!(
        res_no_rows,
        Err(SqlInterfaceError::TimestampRequestFailure { .. })
    ));

    // 3. Invalid Query (map_err -> Err)
    let res_invalid = SqlInterface::get_timestamp(
        &mut conn,
        "SELECT invalid_col FROM nonexistent_table_xyz_666;",
    );
    assert!(matches!(
        res_invalid,
        Err(SqlInterfaceError::TimestampRequestFailure { .. })
    ));
}

#[test]
// This test case checks the functionality of get_optional_timestamp.
// - response when querying an empty table (should be Ok(None))
// - response when querying a table with one entry (should be Ok(Some(timestamp)))
// - error response when querying a table with multiple entries
// Test case uses test database #56.
fn test_get_optional_timestamp() {
    // Setup
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        56,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut conn = sql_interface.get_connection().unwrap();

    let table_name = "test_optional_timestamp";

    // Create a temporary table for the test
    conn.query_drop(format!(
        "CREATE TABLE IF NOT EXISTS {} (ts TIMESTAMP);",
        table_name
    ))
    .unwrap();

    // --- Test Case 1: Empty table ---
    println!("* Testing optional timestamp with empty table...");
    conn.query_drop(format!("TRUNCATE TABLE {}", table_name))
        .unwrap();

    let result =
        SqlInterface::get_optional_timestamp(&mut conn, &format!("SELECT ts FROM {}", table_name));
    assert_eq!(result.unwrap(), None);
    println!("* Succeeded: Empty table returns Ok(None).");

    // --- Test Case 2: Table with one entry ---
    println!("* Testing optional timestamp with a single entry...");
    let test_timestamp = Local::now().naive_local().trunc_subsecs(0);
    conn.exec_drop(
        format!("INSERT INTO {} (ts) VALUES (:ts)", table_name),
        params! { "ts" => test_timestamp },
    )
    .unwrap();

    let result =
        SqlInterface::get_optional_timestamp(&mut conn, &format!("SELECT ts FROM {}", table_name));
    assert_eq!(result.unwrap(), Some(test_timestamp));
    println!("* Succeeded: Table with one entry returns Ok(Some(timestamp)).");

    // --- Test Case 3: Table with multiple entries ---
    println!("* Testing optional timestamp with multiple entries...");
    let another_timestamp = (Local::now() - chrono::Duration::minutes(5))
        .naive_local()
        .trunc_subsecs(0);
    conn.exec_drop(
        format!("INSERT INTO {} (ts) VALUES (:ts)", table_name),
        params! { "ts" => another_timestamp },
    )
    .unwrap();

    let result =
        SqlInterface::get_optional_timestamp(&mut conn, &format!("SELECT ts FROM {}", table_name));
    assert_eq!(result.unwrap(), Some(test_timestamp));
    println!("* Succeeded: Table with multiple entries returns Ok(Some(timestamp)).");

    // --- Test Case 4: Invalid query syntax error handling ---
    println!("* Testing optional timestamp with invalid SQL query...");
    let res_invalid = SqlInterface::get_optional_timestamp(
        &mut conn,
        "SELECT invalid_col FROM nonexistent_table_xyz_555",
    );
    assert!(matches!(
        res_invalid,
        Err(SqlInterfaceError::OptionalTimestampRequestFailure { .. })
    ));
    println!("* Succeeded: Invalid SQL query returns OptionalTimestampRequestFailure.");

    // Cleanup
    conn.query_drop(format!("DROP TABLE {}", table_name))
        .unwrap();
}

#[test]
// This test case checks if current timestamp is local timestamp.
// It does not modify any database.
pub fn test_get_current_timestamp() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        28,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");

    // *** check if current timestamp is local timestamp ***************************************
    let sql_timestamp = match SqlInterface::get_current_timestamp(&mut sql_interface.conn) {
        Ok(c) => c,
        Err(_) => panic!("Could not get current timestamp from the database."),
    };
    let date_time: DateTime<Local> = Local.from_local_datetime(&sql_timestamp).unwrap();
    // Allow a tolerance of 1.5 seconds
    let now = Local::now();
    let difference = (date_time - now).abs();
    let tolerance = chrono::Duration::milliseconds(1500);

    assert!(
        difference <= tolerance,
        "Timestamp assertion failed. Database time: {:?}, Local time: {:?}, Difference: {:?}",
        date_time,
        now,
        difference
    );
    println!("* checking current timestamp succeeded.");
    // *****************************************************************************************
}

#[test]
// This test case checks the functionality of get_current_timestamp_offset_seconds.
// It verifies that the function correctly returns timestamps in the future (positive offset),
// in the past (negative offset), and at the present (zero offset).
// Test case does not modify any database.
fn test_get_current_timestamp_offset_seconds() {
    // Setup
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        56,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut conn = sql_interface.get_connection().unwrap();

    // --- Test Case 1: Positive offset (future) ---
    println!("* Testing timestamp with positive offset...");
    let offset_future: i64 = 3600; // 1 hour in the future
    let result_future =
        SqlInterface::get_current_timestamp_offset_seconds(&mut conn, offset_future as i32)
            .unwrap();
    let expected_future = Local::now().naive_local() + chrono::Duration::seconds(offset_future);
    let diff_future = (expected_future - result_future).num_seconds().abs();
    assert!(
        diff_future <= 2,
        "Future offset test failed. Expected ~{}, got {}. Difference: {}s",
        expected_future,
        result_future,
        diff_future
    );
    println!("* Succeeded: Positive offset returns a future timestamp.");

    // --- Test Case 2: Negative offset (past) ---
    println!("* Testing timestamp with negative offset...");
    let offset_past: i64 = -1800; // 30 minutes in the past
    let result_past =
        SqlInterface::get_current_timestamp_offset_seconds(&mut conn, offset_past as i32).unwrap();
    let expected_past = Local::now().naive_local() + chrono::Duration::seconds(offset_past);
    let diff_past = (expected_past - result_past).num_seconds().abs();
    assert!(
        diff_past <= 2,
        "Past offset test failed. Expected ~{}, got {}. Difference: {}s",
        expected_past,
        result_past,
        diff_past
    );
    println!("* Succeeded: Negative offset returns a past timestamp.");

    // --- Test Case 3: Zero offset ---
    println!("* Testing timestamp with zero offset...");
    let offset_zero: i64 = 0;
    let result_zero =
        SqlInterface::get_current_timestamp_offset_seconds(&mut conn, offset_zero as i32).unwrap();
    let expected_now = Local::now().naive_local();
    let diff_now = (expected_now - result_zero).num_seconds().abs();
    assert!(
        diff_now <= 2,
        "Zero offset test failed. Expected ~{}, got {}. Difference: {}s",
        expected_now,
        result_zero,
        diff_now
    );
    println!("* Succeeded: Zero offset returns the current timestamp.");
}

#[test]
// This test case checks the functionality of the private `get_date` function
// and its public wrappers.
// - Verifies the error response when a query returns no rows.
// - Verifies the correct response when a query returns exactly one row.
// - Verifies that `get_current_date` and `get_tomorrow_date` work as expected.
// Test case uses test database #56.
fn test_get_date() {
    // Setup
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        56,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut conn = sql_interface.get_connection().unwrap();

    let table_name = "test_date_table";

    // Create a temporary table for the test
    conn.query_drop(format!(
        "CREATE TABLE IF NOT EXISTS {} (d DATE);",
        table_name
    ))
    .unwrap();

    // --- Test Case 1: Empty table (should error) ---
    println!("* Testing get_date with empty table...");
    conn.query_drop(format!("TRUNCATE TABLE {}", table_name))
        .unwrap();

    let result = SqlInterface::get_date(&mut conn, &format!("SELECT d FROM {}", table_name));

    // Assert that we get the specific error for an empty response.
    if let Err(SqlInterfaceError::DateRequestFailure { .. }) = result {
        // This is the expected error path.
    } else {
        panic!(
            "Expected DateRequestFailure for empty result, but got: {:?}",
            result
        );
    }
    println!("* Succeeded: Empty table returns a DateRequestFailure error.");

    // --- Test Case 2: Table with one entry ---
    println!("* Testing get_date with a single entry...");
    let test_date = NaiveDate::from_ymd_opt(2025, 7, 15).unwrap();
    conn.exec_drop(
        format!("INSERT INTO {} (d) VALUES (:d)", table_name),
        params! { "d" => test_date },
    )
    .unwrap();

    let result = SqlInterface::get_date(&mut conn, &format!("SELECT d FROM {}", table_name));
    assert_eq!(result.unwrap(), test_date);
    println!("* Succeeded: Table with one entry returns Ok(date).");

    // --- Test Case 3: Test public wrappers ---
    println!("* Testing get_current_date and get_tomorrow_date wrappers...");
    let current_date_from_db = SqlInterface::get_current_date(&mut conn).unwrap();
    let tomorrow_date_from_db = SqlInterface::get_tomorrow_date(&mut conn).unwrap();

    let local_today = Local::now().date_naive();
    let local_tomorrow = local_today.succ_opt().unwrap();

    assert_eq!(current_date_from_db, local_today);
    assert_eq!(tomorrow_date_from_db, local_tomorrow);
    println!("* Succeeded: Public wrappers return correct dates.");

    // --- Test Case 4: Invalid query syntax error handling ---
    println!("* Testing get_date with invalid SQL query...");
    let res_invalid = SqlInterface::get_date(
        &mut conn,
        "SELECT invalid_col FROM nonexistent_table_xyz_444",
    );
    assert!(matches!(
        res_invalid,
        Err(SqlInterfaceError::DateRequestFailure { .. })
    ));
    println!("* Succeeded: Invalid SQL query returns DateRequestFailure.");

    // Cleanup
    conn.query_drop(format!("DROP TABLE {}", table_name))
        .unwrap();
}

#[test]
// This test case checks
// - if required tables are existing
// Test case uses test database #28.
pub fn test_sql_interface_required_tables_existing() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        28,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let database_name_from_config = config.sql_interface.db_name.clone();
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");

    let database_name_from_db = match sql_interface.get_database() {
        Ok(c) => c,
        Err(e) => panic!("Could not get database name: {e:?}"),
    };
    assert_eq!(database_name_from_db, database_name_from_config);
    println!(
        "* checking database name succeeded. database_name_from_db={}",
        database_name_from_db
    );

    // *** check if required tables are existing - *********************************************
    let result_check_required_tables_existing = sql_interface.check_required_tables_existing();
    if let Err(e) = result_check_required_tables_existing {
        panic!("* checking required tables failed: {e:?}")
    } else {
        println!("* checking required tables succeeded.");
    }
    // *****************************************************************************************

    // *** check if required tables are existing - *********************************************
    let dummy_table = "dummy".to_string();
    sql_interface.config.db_tables.push(dummy_table.clone());
    assert!(matches!(
        sql_interface.check_required_tables_existing(),
        Err(SqlInterfaceError::RequiredTablesNotExisting(_, _, _))
    ));
    println!("* checking required tables succeeded.");
    // *****************************************************************************************

    // *** check if the current date is local date *********************************************
    let sql_date =
        match SqlInterface::get_current_date(&mut sql_interface.get_connection().unwrap()) {
            Ok(c) => c,
            Err(_) => panic!("Could not get current date from the database."),
        };
    let sql_date_string = sql_date.format("%Y-%m-%d").to_string();
    let local_date_string = Local::now().trunc_subsecs(0).format("%Y-%m-%d").to_string();
    assert_eq!(sql_date_string, local_date_string);
    println!("* checking current date succeeded.");
    // *****************************************************************************************
}

#[test]
// This test case verifies that `check_wait_timeout` successfully validates
// that the database's `wait_timeout` setting is greater than or equal to
// the minimum value specified in the configuration file.
// Test case does not modify any database.
fn test_check_wait_timeout_success() {
    // Setup
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        56,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let min_wait_timeout_from_config = config.sql_interface.db_min_wait_timeout;

    // We need a connection to run the check. The `new` function itself
    // already runs this check, but we call it explicitly for a focused test.
    let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut conn = sql_interface.get_connection().unwrap();

    // --- Test Execution ---
    println!(
        "* Testing check_wait_timeout with value from config: {}s",
        min_wait_timeout_from_config
    );
    let result = SqlInterface::check_wait_timeout(&mut conn, min_wait_timeout_from_config);

    // --- Assertion ---
    assert!(
        result.is_ok(),
        "check_wait_timeout failed, but was expected to succeed. Result: {:?}",
        result
    );
    println!("* Succeeded: Database wait_timeout is sufficient.");
}

#[test]
// This test case checks if a zero value for the minimum wait timeout allows instantiation.
pub fn test_sql_interface_min_wait_timeout_check_deactivated() {
    let mut config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        28,
    );
    config.sql_interface.db_min_wait_timeout = 0;
    println!("Testing with database {}", config.sql_interface.db_name);
    _ = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed with min wait timeout=0.");
}

#[test]
// This test case checks if an excessively high value for the minimum wait timeout triggers an error.
pub fn test_sql_interface_min_wait_timeout_check() {
    let mut config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        28,
    );
    // set an extraordinary high value so that implementation returns with error
    config.sql_interface.db_min_wait_timeout = 100_000_000;
    println!("Testing with database {}", config.sql_interface.db_name);
    let result = SqlInterface::new(config.sql_interface);

    assert!(matches!(
        result,
        Err(SqlInterfaceError::WaitTimeoutTooLow(_, _, _))
    ));
}

#[test]
// This test case checks error handling when the connection pool creation fails due to an invalid URL format.
pub fn test_sql_interface_new_connection_pool_failure() {
    let mut config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        28,
    );
    config.sql_interface.db_user = "invalid user with spaces#".to_string();
    let result = SqlInterface::new(config.sql_interface);
    assert!(matches!(
        result,
        Err(SqlInterfaceError::ConnectionPoolFailure { .. })
    ));
}

#[test]
// This test case checks error handling when the connection pool fails to retrieve an initial connection.
pub fn test_sql_interface_new_connection_from_pool_failure() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        28,
    );
    let mut sql_interface = SqlInterface::new(config.sql_interface.clone()).unwrap();
    // Temporarily lower max_connections to 1 so get_conn fails
    if sql_interface
        .conn
        .query_drop("SET GLOBAL max_connections = 1;")
        .is_ok()
    {
        let result = SqlInterface::new(config.sql_interface.clone());
        // Restore max_connections immediately
        let _ = sql_interface
            .conn
            .query_drop("SET GLOBAL max_connections = 151;");
        assert!(matches!(
            result,
            Err(SqlInterfaceError::ConnectionFromPool { .. })
                | Err(SqlInterfaceError::ConnectionPoolFailure { .. })
        ));
    }
}

#[test]
// This test case checks
// - error response when querying empty table
// - error response when version information does not match entry in database
// - check error response when the hash is incorrect
// Test case uses test database #29.
pub fn test_sql_interface_version() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        29,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");

    // *** check error response when querying empty table **************************************
    let version_information = VersionInformation::new().unwrap();
    match SqlInterface::truncate_table(&mut sql_interface, "version".to_string()) {
        Ok(_) => {}
        Err(e) => panic!("Could not prepare test case: {e:?}"),
    }
    match sql_interface.get_hash_from_database(&version_information) {
        Ok(_) => {
            panic!("Found version information in truncated table.");
        }
        Err(e) => {
            assert!(matches!(
                e,
                SqlInterfaceError::SingleVersionRequestNotListed(_, _, _, _, _)
            ));
        }
    }
    // *****************************************************************************************

    insert_mock_version_information(&mut sql_interface);

    // *** check error response when version information does not match entry in database ******
    match sql_interface.get_hash_from_database(&version_information) {
        Ok(_) => {
            panic!("Found correct version information although having inserted incorrect version information table.");
        }
        Err(e) => {
            assert!(matches!(
                e,
                SqlInterfaceError::SingleVersionRequestNotListed(_, _, _, _, _)
            ))
        }
    }

    // *** check response when the hash is correct ********************************************
    let version_information_with_wrong_hash = VersionInformation {
        major: version_information.major,
        minor: version_information.minor,
        build: version_information.build,
        hash: "wrong".to_string(),
    };
    let sql_query_string = SQL_QUERY_WRITE_VERSION_INFORMATION.to_string();
    match sql_interface.conn.exec_drop::<_, _>(
        sql_query_string,
        params! {
        "major" => version_information_with_wrong_hash.major,
        "minor" => version_information_with_wrong_hash.minor,
        "build" => version_information_with_wrong_hash.build,
        "hash" => version_information_with_wrong_hash.hash.clone(),
        },
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Error when inserting version information into database: {e:?}");
        }
    };
    match sql_interface.get_hash_from_database(&version_information) {
        Ok(c) => {
            let reference_hash = &version_information_with_wrong_hash.hash;
            assert_eq!(&c, reference_hash);
        }
        Err(e) => {
            panic!("Error retrieving version information from the database: {e:?}");
        }
    }
    // ***************************************************************************************

    // *** check error response when version table contains multiple matching results *********
    sql_interface
        .conn
        .exec_drop::<_, _>(
            SQL_QUERY_WRITE_VERSION_INFORMATION,
            params! {
                "major" => version_information.major,
                "minor" => version_information.minor,
                "build" => version_information.build,
                "hash" => "duplicate_hash".to_string(),
            },
        )
        .unwrap();
    let multi_res = sql_interface.get_hash_from_database(&version_information);
    assert!(
        matches!(
            multi_res,
            Err(SqlInterfaceError::SingleVersionRequestMultipleResults(
                _,
                _,
                _,
                _,
                _
            ))
        ),
        "Expected SingleVersionRequestMultipleResults, got {:?}",
        multi_res
    );
    println!("* checking multiple results error response succeeded.");

    // *** check error response when version table is missing **********************************
    sql_interface.conn.query_drop("DROP TABLE version").unwrap();
    let fail_res = sql_interface.get_hash_from_database(&version_information);
    assert!(
        matches!(
            fail_res,
            Err(SqlInterfaceError::SingleVersionRequestFailure(
                _,
                _,
                _,
                _,
                _
            ))
        ),
        "Expected SingleVersionRequestFailure when version table is dropped, got {:?}",
        fail_res
    );
    println!("* checking version table missing error response succeeded.");

    // Recreate version table for cleanup
    sql_interface
        .conn
        .query_drop(
            "CREATE TABLE IF NOT EXISTS version (major INT NOT NULL, minor INT NOT NULL, build INT NOT NULL, hash VARCHAR(255) NOT NULL)"
        )
        .unwrap();
}

#[test]
// This test case checks if the ping query can be executed
pub fn test_sql_interface_ping() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        28,
    );
    let sql_interface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed");

    SqlInterface::ping_database(&mut sql_interface.get_connection().unwrap()).unwrap();
}

#[test]
// This test case checks error handling of get_database when the connection is killed.
// Test case uses dedicated test database #61.
pub fn test_sql_interface_get_database_error() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        61,
    );
    let mut sql_interface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");

    // Retrieve connection ID for the test connection
    let connection_id: u64 = sql_interface
        .conn
        .query_first("SELECT CONNECTION_ID();")
        .unwrap()
        .unwrap();

    // Create a temporary helper connection from the pool to kill the test connection
    let mut helper_conn = sql_interface.get_connection().unwrap();
    helper_conn
        .query_drop(format!("KILL {};", connection_id))
        .unwrap();

    // Call get_database on the killed connection -> expects DatabaseNameRequestFailure
    let result = sql_interface.get_database();
    assert!(
        matches!(
            result,
            Err(SqlInterfaceError::DatabaseNameRequestFailure { .. })
        ),
        "Expected DatabaseNameRequestFailure on killed connection, got {:?}",
        result.as_ref().err()
    );
}

#[test]
// This test case checks error handling of get_tables when the connection is killed.
// Test case uses dedicated test database #63.
pub fn test_sql_interface_get_tables_error() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        63,
    );
    let mut sql_interface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");

    // Retrieve connection ID for the test connection
    let connection_id: u64 = sql_interface
        .conn
        .query_first("SELECT CONNECTION_ID();")
        .unwrap()
        .unwrap();

    // Create a temporary helper connection from the pool to kill the test connection
    let mut helper_conn = sql_interface.get_connection().unwrap();
    helper_conn
        .query_drop(format!("KILL {};", connection_id))
        .unwrap();

    // Call get_tables on the killed connection -> expects ShowTablesWithoutProperResult
    let result = sql_interface.get_tables();
    assert!(
        matches!(
            result,
            Err(SqlInterfaceError::ShowTablesWithoutProperResult(_))
        ),
        "Expected ShowTablesWithoutProperResult on killed connection, got {:?}",
        result.as_ref().err()
    );
}

#[test]
// This test case checks error handling of ping_database when the connection is killed.
// Test case uses dedicated test database #64.
pub fn test_sql_interface_ping_database_error() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        64,
    );
    let sql_interface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut test_conn = sql_interface.get_connection().unwrap();

    // Retrieve connection ID for the test connection
    let connection_id: u64 = test_conn
        .query_first("SELECT CONNECTION_ID();")
        .unwrap()
        .unwrap();

    // Create a temporary helper connection from the pool to kill the test connection
    let mut helper_conn = sql_interface.get_connection().unwrap();
    helper_conn
        .query_drop(format!("KILL {};", connection_id))
        .unwrap();

    // Call ping_database on the killed connection -> expects DatabasePingFailure
    let result = SqlInterface::ping_database(&mut test_conn);
    assert!(
        matches!(result, Err(SqlInterfaceError::DatabasePingFailure { .. })),
        "Expected DatabasePingFailure on killed connection, got {:?}",
        result.as_ref().err()
    );
}

#[test]
// This test case checks get_duration_from_naive_timestamp_to_local_now
// with valid, non-existent (DST spring-forward gap), and ambiguous (DST fall-back overlap) timestamps.
pub fn test_get_duration_from_naive_timestamp_to_local_now() {
    // 1. Valid timestamp (Single)
    let valid_naive = Local::now().naive_local() - chrono::Duration::seconds(10);
    let valid_duration = SqlInterface::get_duration_from_naive_timestamp_to_local_now(valid_naive);
    assert!(valid_duration.is_some());

    // 2. DST spring-forward non-existent timestamp (None)
    let spring_forward_gap = NaiveDate::from_ymd_opt(2025, 3, 30)
        .unwrap()
        .and_hms_opt(2, 30, 0)
        .unwrap();
    let gap_res = SqlInterface::get_duration_from_naive_timestamp_to_local_now(spring_forward_gap);
    assert!(gap_res.is_none());

    // 3. DST fall-back ambiguous timestamp (Ambiguous)
    let fall_back_overlap = NaiveDate::from_ymd_opt(2025, 10, 26)
        .unwrap()
        .and_hms_opt(2, 30, 0)
        .unwrap();
    let overlap_res =
        SqlInterface::get_duration_from_naive_timestamp_to_local_now(fall_back_overlap);
    assert!(overlap_res.is_none());
}
