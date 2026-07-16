/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Testing of cor SQL database interface.

use chrono::{DateTime, Local, NaiveDate, SubsecRound, TimeZone};
use mysql::params;
use mysql::prelude::Queryable;

use aquarium_control::database::sql_interface::{SqlInterface, SqlInterfaceError};
use aquarium_control::database::sql_test_query_strings::SQL_QUERY_WRITE_VERSION_INFORMATION;
use aquarium_control::utilities::config::{read_config_file_with_test_database, ConfigData};
use aquarium_control::utilities::version_information::VersionInformation;

// Helper function to insert mock version information into database
pub fn insert_mock_version_information(sql_interface: &mut SqlInterface) {
    let unknown_version_information = VersionInformation {
        major: -1,
        minor: -1,
        build: -1,
        hash: "void".to_string(),
    };
    let sql_query_string = SQL_QUERY_WRITE_VERSION_INFORMATION.to_string();
    match sql_interface.conn.exec_drop::<_, _>(
        sql_query_string,
        params! {
        "major" => unknown_version_information.major,
        "minor" => unknown_version_information.minor,
        "build" => unknown_version_information.build,
        "hash" => unknown_version_information.hash.clone(),
        },
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Error when inserting version information into database: {e:?}");
        }
    };
}

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

    // Cleanup
    conn.query_drop(format!("DROP TABLE {}", table_name))
        .unwrap();
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

    let result = SqlInterface::get_optional_timestamp(
        &mut conn,
        &format!("SELECT ts FROM {}", table_name),
    );
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

    let result = SqlInterface::get_optional_timestamp(
        &mut conn,
        &format!("SELECT ts FROM {}", table_name),
    );
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

    let result = SqlInterface::get_optional_timestamp(
        &mut conn,
        &format!("SELECT ts FROM {}", table_name),
    );
    assert_eq!(result.unwrap(), Some(test_timestamp));
    println!("* Succeeded: Table with multiple entries returns Ok(Some(timestamp)).");

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
    assert_eq!(date_time, Local::now().trunc_subsecs(0));
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
        SqlInterface::get_current_timestamp_offset_seconds(&mut conn, offset_past as i32)
            .unwrap();
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
        SqlInterface::get_current_timestamp_offset_seconds(&mut conn, offset_zero as i32)
            .unwrap();
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
    config.sql_interface.db_min_wait_timeout = 1000000;
    println!("Testing with database {}", config.sql_interface.db_name);
    let result = SqlInterface::new(config.sql_interface);

    assert!(matches!(
        result,
        Err(SqlInterfaceError::WaitTimeoutTooLow(_, _, _))
    ));
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
