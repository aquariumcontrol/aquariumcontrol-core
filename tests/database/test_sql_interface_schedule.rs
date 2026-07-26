/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests database interaction of time-based scheduling for various application modules.

use aquarium_control::database::sql_interface::SqlInterface;
use aquarium_control::database::sql_interface_error::SqlInterfaceError;
use aquarium_control::database::sql_interface_schedule::{
    ScheduleEntry, ScheduleType, SqlInterfaceSchedule, SqlScheduleEntry,
};
use aquarium_control::database::sql_test_query_strings;
use aquarium_control::database::sql_test_query_strings::SQL_TABLE_SCHEDULE;
use aquarium_control::utilities::config::{read_config_file_with_test_database, ConfigData};
use mysql::params;
use mysql::prelude::Queryable;
use std::collections::HashMap;

#[test]
// This test case verifies the validation and data processing logic for SqlInterfaceSchedule.
// It covers the following scenarios:
// 1. `new()`: Happy path, failure on NULL, and failure on row limits.
// 2. `read_schedule()`: Failure on invalid data (bad schedule type) and partial success.
// Test case uses test database #63.
// [itest->swreq~schedule_read_entries~1]
/// [itest->swreq~schedule_read_entries~1]
fn test_sql_interface_schedule_new_and_read() {
    // --- Common Setup ---
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        63,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface.clone())
        .expect("Initialization of SQL interface for test failed.");

    // --- Test `new()`: Happy Path (empty table) ---
    println!("* Testing new() with an empty table (Happy Path)...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string())
        .expect("Test setup failed: Could not truncate table.");

    let result = SqlInterfaceSchedule::new(sql_interface.get_connection().unwrap(), 10);
    assert!(
        result.is_ok(),
        "Expected new() to succeed with an empty table, but it failed: {:?}",
        result.err()
    );
    println!("* Succeeded: Happy path initialization is successful on an empty table.");

    // --- Test `new()`: Failure on too many rows ---
    println!("* Testing new() with more rows than the limit...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string())
        .expect("Test setup failed: Could not truncate table.");
    let mut conn = sql_interface.get_connection().unwrap();
    conn.exec_drop(
        sql_test_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
        params! { "schedule_name" => "Balling", "start_time" => "01:00:00", "stop_time" => "02:00:00" },
    )
        .unwrap();
    conn.exec_drop(
        sql_test_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
        params! { "schedule_name" => "Refill", "start_time" => "03:00:00", "stop_time" => "04:00:00" },
    )
        .unwrap();

    // Set the limit to 1
    let result = SqlInterfaceSchedule::new(conn, 1);
    assert!(
        matches!(
            result,
            Err(SqlInterfaceError::DatabaseScheduleTableContainsTooManyRows(
                _,
                _,
                _
            ))
        ),
        "Expected row limit error, but got {:?}",
        result
    );
    println!("* Succeeded: Initialization fails if table exceeds row limit.");

    // --- Test `read_schedule()`: Failure on invalid data ---
    println!("* Testing read_schedule() with an invalid schedule type...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string())
        .expect("Test setup failed: Could not truncate table.");
    let mut conn = sql_interface.get_connection().unwrap();
    conn.exec_drop(
        sql_test_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
        params! { "schedule_name" => "InvalidType", "start_time" => "01:00:00", "stop_time" => "02:00:00" },
    )
        .unwrap();

    let mut schedule_db = SqlInterfaceSchedule::new(conn, 10).unwrap();
    let read_result = schedule_db.read_schedule();

    assert!(
        matches!(read_result, Err(ref errors) if !errors.is_empty()),
        "Expected read_schedule to return a vector of errors"
    );

    if let Err(errors) = read_result {
        assert!(matches!(
            errors[0],
            SqlInterfaceError::ScheduleProcessingFailure { .. }
        ));
        println!("* Succeeded: read_schedule correctly fails on invalid data.");
    }

    // --- Test `read_schedule()`: Partial success ---
    println!("* Testing read_schedule() with mixed valid and invalid data...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string())
        .expect("Test setup failed: Could not truncate table.");
    let mut conn = sql_interface.get_connection().unwrap();
    // Insert one valid row
    conn.exec_drop(
        sql_test_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
        params! { "schedule_name" => "Balling", "start_time" => "01:00:00", "stop_time" => "02:00:00" },
    )
        .unwrap();
    // Insert one invalid row
    conn.exec_drop(
        sql_test_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
        params! { "schedule_name" => "AnotherInvalidType", "start_time" => "03:00:00", "stop_time" => "04:00:00" },
    )
        .unwrap();

    let mut schedule_db = SqlInterfaceSchedule::new(conn, 10).unwrap();
    let read_result = schedule_db.read_schedule();

    // Expect an error, but also check that the valid data was processed.
    assert!(
        read_result.is_err(),
        "Expected an error due to the invalid entry"
    );
    assert_eq!(
        schedule_db.schedules.len(),
        1,
        "Expected exactly one valid schedule to be processed"
    );
    assert_eq!(
        schedule_db.schedules[0].schedule_type,
        ScheduleType::Balling
    );
    println!("* Succeeded: read_schedule correctly processes valid data while reporting errors for invalid data.");
}

#[test]
// Test case reads entries from the SQL database.
// Since the application does not modify schedule data in the SQL database,
// results are compared to fixed references.
// Test case uses test database #38.
pub fn test_sql_interface_schedule() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        38,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_schedule = config.sql_interface.max_rows_schedule;
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string()).unwrap();

    let test_data = [
        ("Balling", "01:00:00", "23:00:00"),
        ("Heating", "00:00:00", "23:59:00"),
        ("Ventilation", "03:00:00", "21:00:00"),
        ("Refill", "02:00:00", "22:00:00"),
    ];

    for (name, start, stop) in &test_data {
        sql_interface
            .conn
            .exec_drop(
                sql_test_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
                params! {
                    "schedule_name" => *name,
                    "start_time" => *start,
                    "stop_time" => *stop,
                },
            )
            .unwrap();
    }

    let mut sql_interface_schedule =
        SqlInterfaceSchedule::new(sql_interface.get_connection().unwrap(), max_rows_schedule)
            .unwrap();

    match sql_interface_schedule.read_schedule() {
        Ok(_) => {
            println!("Reading schedule succeeded checking content now.");
        }
        Err(e) => {
            panic!("Reading schedule from SQL database failed: {e:?}");
        }
    }

    let references: HashMap<String, _> = test_data
        .iter()
        .map(|(name, start, stop)| {
            let entry = ScheduleEntry::new(SqlScheduleEntry {
                schedule_type: name.to_string(),
                start_time: start.to_string().parse().unwrap(),
                stop_time: stop.to_string().parse().unwrap(),
                is_active: 1,
            })
            .unwrap();
            (name.to_string(), entry)
        })
        .collect();

    assert_eq!(sql_interface_schedule.schedules.len(), 4);

    for schedule_entry in sql_interface_schedule.schedules {
        let reference = references
            .get(&schedule_entry.schedule_type.to_string())
            .unwrap();
        assert_eq!(reference.start_time, schedule_entry.start_time);
        assert_eq!(reference.stop_time, schedule_entry.stop_time);
        assert_eq!(reference.is_active, schedule_entry.is_active);
    }
    // *** check if row limitation is active ***
    let test_result = SqlInterfaceSchedule::new(sql_interface.conn, 1);
    assert!(matches!(
        test_result,
        Err(SqlInterfaceError::DatabaseScheduleTableContainsTooManyRows(
            _,
            _,
            _
        ))
    ));
}

#[test]
// Test case verifies that the correct error is returned when a schedule is not found.
// Test case uses test database #53.
pub fn test_sql_interface_schedule_not_found() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        53,
    );
    println!(
        "Testing ScheduleTypeNotFound with database {}",
        config.sql_interface.db_name
    );
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface.clone())
        .expect("Initialization of SQL interface for test failed.");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string()).unwrap();

    // Insert a SUBSET of schedules to test the "not found" case
    let test_data = [
        ("Heating", "00:00:00", "23:59:00"),
        ("Ventilation", "03:00:00", "21:00:00"),
    ];

    for (name, start, stop) in &test_data {
        sql_interface
            .conn
            .exec_drop(
                sql_test_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
                params! {
                    "schedule_name" => *name,
                    "start_time" => *start,
                    "stop_time" => *stop,
                },
            )
            .unwrap();
    }

    let mut sql_interface_schedule = SqlInterfaceSchedule::new(
        sql_interface.get_connection().unwrap(),
        config.sql_interface.max_rows_schedule,
    )
    .unwrap();

    sql_interface_schedule
        .read_schedule()
        .expect("Reading schedule from SQL database failed");

    // Now, try to find a schedule that was NOT inserted (e.g., Balling)
    println!("Checking for non-existent schedule entry (Balling).");
    let find_result = sql_interface_schedule.find_schedule(ScheduleType::Balling);

    // Assert that we get the correct error variant
    assert!(matches!(
        find_result,
        Err(SqlInterfaceError::ScheduleTypeNotFoundOtherEntriesExisting(
            _,
            _,
            _
        ))
    ));

    // For more robust testing, also verify the content of the error message.
    if let Err(SqlInterfaceError::ScheduleTypeNotFoundOtherEntriesExisting(
        _,
        requested,
        available,
    )) = find_result
    {
        assert_eq!(requested, "Balling");
        assert!(available.contains("Heating") && available.contains("Ventilation"));
        println!(
            "Correctly received ScheduleTypeNotFound for '{}'. Available types: '{}'",
            requested, available
        );
    }
}
