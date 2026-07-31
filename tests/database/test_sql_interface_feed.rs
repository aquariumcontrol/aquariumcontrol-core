/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests all database interactions for the automated feeding system.

use chrono::{Duration, Local, NaiveDate, NaiveDateTime};
use mysql::{params, prelude::Queryable};

use aquarium_control::database::database_interface_feed_trait::DatabaseInterfaceFeedTrait;
use aquarium_control::database::sql_interface_feed::SqlInterfaceFeed;
use aquarium_control::database::sql_query_strings::{
    SQL_TABLE_FEED_PROFILES, SQL_TABLE_FEED_SCHEDULE,
};
use aquarium_control::database::sql_test_query_strings::SQL_TABLE_FEED_LOG;
use aquarium_control::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError,
};
use aquarium_control::database::{sql_query_strings, sql_test_query_strings};
use aquarium_control::food::feed_schedule_entry::FeedScheduleEntry;
use aquarium_control::utilities::config::{read_config_file_with_test_database, ConfigData};

pub struct SqlFeedEvent {
    timestamp: String,
    feeder_run_time: f32,
    feed_profile_id: i32,
}

pub struct FeedEvent {
    timestamp: NaiveDateTime,
    feeder_run_time: f32,
    feed_profile_id: i32,
}

/// [itest->swreq~food_connect_database~1]
#[test]
// This test case verifies the validation logic within the `SqlInterfaceFeed::new()` constructor.
// It covers the following scenarios:
// 1. Happy Path: Initialization succeeds with valid data and row counts.
// 2. Failure on Row Limits: Fails when any of the feed-related tables exceed their max row count.
// 3. Deactivated Checks: Succeeds even with excess rows if the limits are set to 0.
// Test case uses test database #59.
fn test_sql_interface_feed_new() {
    // --- Common Setup ---
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        59,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");

    // --- Test Case 1: Happy Path ---
    println!("* Testing new() with valid data (Happy Path)...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_PROFILES.to_string()).unwrap();
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string()).unwrap();
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_LOG.to_string()).unwrap();

    // Insert one valid row into each table
    let mut conn = sql_interface.get_connection().unwrap();
    conn.exec_drop(
        sql_test_query_strings::SQL_QUERY_WRITE_FEED_PROFILE_TEST,
        params! { "profile_id" => 1, "profile_name" => "test_profile1" },
    )
    .unwrap();
    conn.exec_drop(
        sql_test_query_strings::SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY,
        params! { "timestamp" => "2024-01-01 12:00:00", "profile_id" => 1, "is_weekly" => false, "is_daily" => true },
    ).unwrap();
    conn.exec_drop(
        sql_query_strings::SQL_QUERY_WRITE_FEED_EVENT,
        params! { "timestamp" => Local::now().naive_local(), "feeder_run_time" => 1.0, "profile_id" => 1 },
    ).unwrap();

    let result = SqlInterfaceFeed::new(conn, 10, 10, 10);
    assert!(
        result.is_ok(),
        "Expected new() to succeed on happy path, but it failed: {:?}",
        result.err()
    );
    println!("* Succeeded: Happy path initialization is successful.");

    // --- Test Case 2: Failure on feed profiles row limit ---
    println!("* Testing new() with too many rows in feed profiles table...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_PROFILES.to_string()).unwrap();
    let mut conn = sql_interface.get_connection().unwrap();
    conn.exec_drop(
        sql_test_query_strings::SQL_QUERY_WRITE_FEED_PROFILE_TEST,
        params! { "profile_id" => 1, "profile_name" => "row1" },
    )
    .unwrap();
    conn.exec_drop(
        sql_test_query_strings::SQL_QUERY_WRITE_FEED_PROFILE_TEST,
        params! { "profile_id" => 2, "profile_name" => "row2" },
    )
    .unwrap();

    // Set the limit to 1
    let result = SqlInterfaceFeed::new(conn, 1, 10, 10);
    assert!(
        matches!(
            result,
            Err(SqlInterfaceError::DatabaseFeedTableContainsTooManyRows(
                _,
                _,
                _,
                _
            ))
        ),
        "Expected row limit error for feed_profiles, but got {:?}",
        result
    );
    println!("* Succeeded: Initialization fails if feed_profiles exceeds row limit.");

    // --- Test Case 4: Deactivated checks (limits are 0) ---
    println!("* Testing new() with deactivated row limit checks (limits = 0)...");
    // We reuse the state from the previous test (2 rows in feed_profiles)
    let conn = sql_interface.get_connection().unwrap();
    // Set all the limits to 0
    let result = SqlInterfaceFeed::new(conn, 0, 0, 0);
    assert!(
        result.is_ok(),
        "Expected new() to succeed with deactivated checks, but it failed: {:?}",
        result.err()
    );
    println!("* Succeeded: Initialization passes when row limit checks are deactivated.");

    // --- Test Case 5: Failure when feed_profiles contains NULL values ---
    println!("* Testing new() with NULL values in feed_profiles...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_PROFILES.to_string()).unwrap();
    let mut conn = sql_interface.get_connection().unwrap();
    conn.query_drop("ALTER TABLE feed_profiles MODIFY COLUMN profile_name VARCHAR(255) NULL;")
        .unwrap();
    conn.query_drop("INSERT INTO feed_profiles (profile_id, profile_name) VALUES (1, NULL)")
        .unwrap();
    let result = SqlInterfaceFeed::new(conn, 10, 10, 10);
    assert!(
        matches!(
            result,
            Err(SqlInterfaceError::DatabaseFeedSetValTableContainsNull(
                _,
                _,
                _
            ))
        ),
        "Expected DatabaseFeedSetValTableContainsNull error for feed_profiles, but got {:?}",
        result
    );
    println!("* Succeeded: Initialization fails when feed_profiles contains NULL values.");

    // Restore feed_profiles schema
    let mut conn = sql_interface.get_connection().unwrap();
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_PROFILES.to_string()).unwrap();
    conn.query_drop("ALTER TABLE feed_profiles MODIFY COLUMN profile_name VARCHAR(255) NOT NULL;")
        .unwrap();

    // --- Test Case 6: Failure when feed_schedule contains NULL values ---
    println!("* Testing new() with NULL values in feed_schedule...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string()).unwrap();
    let mut conn = sql_interface.get_connection().unwrap();
    conn.query_drop("ALTER TABLE feed_schedule MODIFY COLUMN profile_id INT NULL;")
        .unwrap();
    conn.query_drop(
        "INSERT INTO feed_schedule (timestamp, profile_id, is_weekly, is_daily) VALUES ('2024-01-01 12:00:00', NULL, 0, 1)",
    )
    .unwrap();
    let result = SqlInterfaceFeed::new(conn, 10, 10, 10);
    assert!(
        matches!(
            result,
            Err(SqlInterfaceError::DatabaseFeedSetValTableContainsNull(
                _,
                _,
                _
            ))
        ),
        "Expected DatabaseFeedSetValTableContainsNull error for feed_schedule, but got {:?}",
        result
    );
    println!("* Succeeded: Initialization fails when feed_schedule contains NULL values.");

    // Restore feed_schedule schema
    let mut conn = sql_interface.get_connection().unwrap();
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string()).unwrap();
    conn.query_drop("ALTER TABLE feed_schedule MODIFY COLUMN profile_id INT NOT NULL;")
        .unwrap();

    // --- Test Case 7: Failure when connection is killed ---
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

    let result = SqlInterfaceFeed::new(kill_conn, 10, 10, 10);
    assert!(
        matches!(
            result,
            Err(SqlInterfaceError::DatabaseCheckFeedSetValsFailure { .. })
        ),
        "Expected DatabaseCheckFeedSetValsFailure error on killed connection, but got {:?}",
        result
    );
    println!("* Succeeded: Initialization fails when connection is killed.");
}

/// [itest->swreq~food_read_profiles~1]
#[test]
pub fn test_sql_interface_feed_profiles() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        33,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_feed_profiles = config.sql_interface.max_rows_feed_profile;
    let max_rows_feed_schedule = config.sql_interface.max_rows_feed_schedule;
    let max_rows_feed_log = config.sql_interface.max_rows_feed_log;
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut sql_interface_feed = SqlInterfaceFeed::new(
        sql_interface.get_connection().unwrap(),
        max_rows_feed_profiles,
        max_rows_feed_schedule,
        max_rows_feed_log,
    )
    .unwrap();

    //*** check retrieving of the feed profile from empty table *********************************
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_PROFILES.to_string()) {
        Ok(_) => {}
        Err(e) => panic!("Could not prepare test case: {e:?}"),
    }
    match sql_interface_feed.get_single_feed_profile_from_database(1) {
        Ok(_) => {
            panic!("Call to get_single_feed_profile_from_database should have failed.");
        }
        Err(e) => {
            assert!(matches!(
                e,
                SqlInterfaceError::SingleFeedProfileRequestEmptyResponse(_, _, _)
            ));
        }
    }
    println!("* checking retrieving of Feed profile (FAIL) succeeded.");
    let phase_empty_res = sql_interface_feed.get_feed_phase_from_database(1, 1);
    assert!(
        matches!(
            phase_empty_res,
            Err(SqlInterfaceError::SingleFeedProfileRequestEmptyResponse(
                _,
                _,
                _
            ))
        ),
        "Expected SingleFeedProfileRequestEmptyResponse, got {:?}",
        phase_empty_res.err()
    );
    println!("* checking get_feed_phase_from_database (empty table) succeeded.");
    // **************************************************************************************

    // *** check retrieving of existing feed profile ****************************************
    // prepare database: insert feed profile
    let sql_query_string = sql_test_query_strings::SQL_QUERY_WRITE_FEED_PROFILE_TEST.to_string();
    match sql_interface_feed.conn.exec_drop::<_, _>(
        sql_query_string.clone(),
        params! {
        "profile_id" => 1, "profile_name" => "test",
        },
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Error when inserting feed profile into database: {e:?}");
        }
    };
    match sql_interface_feed.get_single_feed_profile_from_database(1) {
        Ok(c) => {
            assert_eq!(c.profile_id, 1);
            assert_eq!(c.profile_name, "test".to_string());
            assert_eq!(c.feedphases[0].pause_duration, 0);
            assert!(c.feedphases[0].pause_skimmer);
            assert!(!c.feedphases[0].pause_main_pump_1);
            assert!(!c.feedphases[0].pause_main_pump_2);
            assert!(!c.feedphases[0].pause_aux_pump_1);
            assert!(!c.feedphases[0].pause_aux_pump_2);
            assert_eq!(c.feedphases[0].feed_duration, 1);
            assert!(!c.feedphases[0].feed_skimmer);
            assert!(c.feedphases[0].feed_main_pump_1);
            assert!(!c.feedphases[0].feed_main_pump_2);
            assert!(!c.feedphases[0].feed_aux_pump_1);
            assert!(!c.feedphases[0].feed_aux_pump_2);

            assert_eq!(c.feedphases[1].pause_duration, 2);
            assert!(!c.feedphases[1].pause_skimmer);
            assert!(!c.feedphases[1].pause_main_pump_1);
            assert!(c.feedphases[1].pause_main_pump_2);
            assert!(!c.feedphases[1].pause_aux_pump_1);
            assert!(!c.feedphases[1].pause_aux_pump_2);
            assert_eq!(c.feedphases[1].feed_duration, 3);
            assert!(!c.feedphases[1].feed_skimmer);
            assert!(!c.feedphases[1].feed_main_pump_1);
            assert!(!c.feedphases[1].feed_main_pump_2);
            assert!(c.feedphases[1].feed_aux_pump_1);
            assert!(!c.feedphases[1].feed_aux_pump_2);

            assert_eq!(c.feedphases[2].pause_duration, 4);
            assert!(!c.feedphases[2].pause_skimmer);
            assert!(!c.feedphases[2].pause_main_pump_1);
            assert!(!c.feedphases[2].pause_main_pump_2);
            assert!(!c.feedphases[2].pause_aux_pump_1);
            assert!(c.feedphases[2].pause_aux_pump_2);
            assert_eq!(c.feedphases[2].feed_duration, 5);
            assert!(c.feedphases[2].feed_skimmer);
            assert!(c.feedphases[2].feed_main_pump_1);
            assert!(!c.feedphases[2].feed_main_pump_2);
            assert!(!c.feedphases[2].feed_aux_pump_1);
            assert!(!c.feedphases[2].feed_aux_pump_2);

            assert_eq!(c.feedphases[3].pause_duration, 6);
            assert!(c.feedphases[3].pause_skimmer);
            assert!(!c.feedphases[3].pause_main_pump_1);
            assert!(c.feedphases[3].pause_main_pump_2);
            assert!(!c.feedphases[3].pause_aux_pump_1);
            assert!(!c.feedphases[3].pause_aux_pump_2);
            assert_eq!(c.feedphases[3].feed_duration, 7);
            assert!(c.feedphases[3].feed_skimmer);
            assert!(!c.feedphases[3].feed_main_pump_1);
            assert!(!c.feedphases[3].feed_main_pump_2);
            assert!(c.feedphases[3].feed_aux_pump_1);
            assert!(!c.feedphases[3].feed_aux_pump_2);

            assert_eq!(c.feedphases[4].pause_duration, 8);
            assert!(c.feedphases[4].pause_skimmer);
            assert!(!c.feedphases[4].pause_main_pump_1);
            assert!(!c.feedphases[4].pause_main_pump_2);
            assert!(!c.feedphases[4].pause_aux_pump_1);
            assert!(c.feedphases[4].pause_aux_pump_2);
            assert_eq!(c.feedphases[4].feed_duration, 9);
            assert!(c.feedphases[4].feed_skimmer);
            assert!(c.feedphases[4].feed_main_pump_1);
            assert!(c.feedphases[4].feed_main_pump_2);
            assert!(!c.feedphases[4].feed_aux_pump_1);
            assert!(!c.feedphases[4].feed_aux_pump_2);

            assert_eq!(c.feedphases[5].pause_duration, 10);
            assert!(c.feedphases[5].pause_skimmer);
            assert!(c.feedphases[5].pause_main_pump_1);
            assert!(c.feedphases[5].pause_main_pump_2);
            assert!(!c.feedphases[5].pause_aux_pump_1);
            assert!(!c.feedphases[5].pause_aux_pump_2);
            assert_eq!(c.feedphases[5].feed_duration, 11);
            assert!(c.feedphases[5].feed_skimmer);
            assert!(c.feedphases[5].feed_main_pump_1);
            assert!(!c.feedphases[5].feed_main_pump_2);
            assert!(!c.feedphases[5].feed_aux_pump_1);
            assert!(c.feedphases[5].feed_aux_pump_2);

            assert_eq!(c.feedphases[6].pause_duration, 12);
            assert!(c.feedphases[6].pause_skimmer);
            assert!(c.feedphases[6].pause_main_pump_1);
            assert!(c.feedphases[6].pause_main_pump_2);
            assert!(c.feedphases[6].pause_aux_pump_1);
            assert!(!c.feedphases[6].pause_aux_pump_2);
            assert_eq!(c.feedphases[6].feed_duration, 13);
            assert!(c.feedphases[6].feed_skimmer);
            assert!(c.feedphases[6].feed_main_pump_1);
            assert!(c.feedphases[6].feed_main_pump_2);
            assert!(!c.feedphases[6].feed_aux_pump_1);
            assert!(c.feedphases[6].feed_aux_pump_2);

            assert_eq!(c.feedphases[7].pause_duration, 14);
            assert!(c.feedphases[7].pause_skimmer);
            assert!(c.feedphases[7].pause_main_pump_1);
            assert!(c.feedphases[7].pause_main_pump_2);
            assert!(c.feedphases[7].pause_aux_pump_1);
            assert!(c.feedphases[7].pause_aux_pump_2);
            assert_eq!(c.feedphases[7].feed_duration, 15);
            assert!(c.feedphases[7].feed_skimmer);
            assert!(c.feedphases[7].feed_main_pump_1);
            assert!(c.feedphases[7].feed_main_pump_2);
            assert!(c.feedphases[7].feed_aux_pump_1);
            assert!(!c.feedphases[7].feed_aux_pump_2);

            assert_eq!(c.feedphases[8].pause_duration, 16);
            assert!(c.feedphases[8].pause_skimmer);
            assert!(c.feedphases[8].pause_main_pump_1);
            assert!(c.feedphases[8].pause_main_pump_2);
            assert!(!c.feedphases[8].pause_aux_pump_1);
            assert!(!c.feedphases[8].pause_aux_pump_2);
            assert_eq!(c.feedphases[8].feed_duration, 17);
            assert!(c.feedphases[8].feed_skimmer);
            assert!(c.feedphases[8].feed_main_pump_1);
            assert!(!c.feedphases[8].feed_main_pump_2);
            assert!(!c.feedphases[8].feed_aux_pump_1);
            assert!(!c.feedphases[8].feed_aux_pump_2);

            assert_eq!(c.feedphases[9].pause_duration, 18);
            assert!(c.feedphases[9].pause_skimmer);
            assert!(!c.feedphases[9].pause_main_pump_1);
            assert!(!c.feedphases[9].pause_main_pump_2);
            assert!(!c.feedphases[9].pause_aux_pump_1);
            assert!(!c.feedphases[9].pause_aux_pump_2);
            assert_eq!(c.feedphases[9].feed_duration, 19);
            assert!(!c.feedphases[9].feed_skimmer);
            assert!(!c.feedphases[9].feed_main_pump_1);
            assert!(!c.feedphases[9].feed_main_pump_2);
            assert!(!c.feedphases[9].feed_aux_pump_1);
            assert!(!c.feedphases[9].feed_aux_pump_2);
        }
        Err(e) => {
            panic!("Call to get_single_feed_profile_from_database has failed: {e:?}.");
        }
    }
    println!("* checking retrieving of feed profile succeeded.");

    match sql_interface_feed.conn.exec_drop::<_, _>(
        sql_query_string,
        params! {
        "profile_id" => 2, "profile_name" => "test2"
        },
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Error when inserting feed profile into database: {e:?}");
        }
    };
    let test_result: Result<SqlInterfaceFeed, SqlInterfaceError> =
        SqlInterfaceFeed::new(sql_interface.get_connection().unwrap(), 1, 100, 0);
    assert!(matches!(
        test_result,
        Err(SqlInterfaceError::DatabaseFeedTableContainsTooManyRows(
            _,
            _,
            2,
            1
        ))
    ));
    println!("* checking of feed profile max rows limitation succeeded.");
    // *****************************************************************************************

    // *** check error response when multiple rows exist for the same profile_id **************
    let mut conn = sql_interface.get_connection().unwrap();
    let _ = conn.query_drop("ALTER TABLE feed_profiles MODIFY COLUMN profile_id INT NOT NULL;");
    let _ = conn.query_drop("ALTER TABLE feed_profiles DROP PRIMARY KEY;");
    let sql_query_string = sql_test_query_strings::SQL_QUERY_WRITE_FEED_PROFILE_TEST.to_string();
    conn.exec_drop::<_, _>(
        sql_query_string,
        params! { "profile_id" => 1, "profile_name" => "test_duplicate" },
    )
    .unwrap();
    let multi_profile_res = sql_interface_feed.get_feed_profile_header_from_database(1);
    assert!(
        matches!(
            multi_profile_res,
            Err(SqlInterfaceError::SingleFeedProfileRequestNoSingleResponse(
                _,
                _,
                _
            ))
        ),
        "Expected SingleFeedProfileRequestNoSingleResponse, got {:?}",
        multi_profile_res.err()
    );
    println!("* checking multiple profiles for same profile_id error response succeeded.");
    let phase_multi_res = sql_interface_feed.get_feed_phase_from_database(1, 1);
    assert!(
        matches!(
            phase_multi_res,
            Err(SqlInterfaceError::SingleFeedProfileRequestNoSingleResponse(
                _,
                _,
                _
            ))
        ),
        "Expected SingleFeedProfileRequestNoSingleResponse, got {:?}",
        phase_multi_res.err()
    );
    println!("* checking get_feed_phase_from_database (multiple profiles) succeeded.");

    // *** check error response when feed phase duration is negative *************************
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_PROFILES.to_string()).unwrap();
    let mut conn = sql_interface.get_connection().unwrap();
    let sql_query_string = sql_test_query_strings::SQL_QUERY_WRITE_FEED_PROFILE_TEST.to_string();
    conn.exec_drop::<_, _>(
        sql_query_string,
        params! { "profile_id" => 1, "profile_name" => "test_neg" },
    )
    .unwrap();
    conn.query_drop("UPDATE feed_profiles SET pause_01_duration = -1 WHERE profile_id = 1;")
        .unwrap();
    let neg_dur_res = sql_interface_feed.get_feed_phase_from_database(1, 1);
    assert!(
        matches!(
            neg_dur_res,
            Err(SqlInterfaceError::DatabaseFeedPhaseDurationNegative(
                _,
                -1,
                _
            ))
        ),
        "Expected DatabaseFeedPhaseDurationNegative, got {:?}",
        neg_dur_res.err()
    );
    println!("* checking get_feed_phase_from_database (negative duration) succeeded.");

    // Restore PRIMARY KEY and AUTO_INCREMENT on feed_profiles
    let mut conn = sql_interface.get_connection().unwrap();
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_PROFILES.to_string()).unwrap();
    let _ = conn.query_drop("ALTER TABLE feed_profiles ADD PRIMARY KEY (profile_id);");
    let _ = conn.query_drop(
        "ALTER TABLE feed_profiles MODIFY COLUMN profile_id INT NOT NULL AUTO_INCREMENT;",
    );

    // *** check error response when feed_schedule entry repeat_daily is out of range *********
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string()).unwrap();
    let mut conn = sql_interface.get_connection().unwrap();
    conn.exec_drop(
        sql_test_query_strings::SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY,
        params! {
            "timestamp" => "2030-01-01 12:00:00",
            "profile_id" => 1,
            "is_weekly" => false,
            "is_daily" => 2
        },
    )
    .unwrap();
    let invalid_entry_res = sql_interface_feed.get_future_feed_schedule_entries_from_database();
    assert!(
        matches!(
            invalid_entry_res,
            Err(SqlInterfaceError::FeedScheduleEntryTimeStampProcessingFailure { .. })
        ),
        "Expected FeedScheduleEntryTimeStampProcessingFailure, got {:?}",
        invalid_entry_res
    );
    println!("* checking out-of-range repeat_daily error response succeeded.");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string()).unwrap();

    // *** check error response when connection is killed **************************************
    let connection_id: u64 = sql_interface_feed
        .conn
        .query_first("SELECT CONNECTION_ID();")
        .unwrap()
        .unwrap();
    let mut helper_conn = sql_interface.get_connection().unwrap();
    helper_conn
        .query_drop(format!("KILL {};", connection_id))
        .unwrap();

    let killed_profile_res = sql_interface_feed.get_feed_profile_header_from_database(1);
    assert!(
        matches!(
            killed_profile_res,
            Err(SqlInterfaceError::SingleFeedProfileRequestFailure { .. })
        ),
        "Expected SingleFeedProfileRequestFailure on killed connection, got {:?}",
        killed_profile_res.err()
    );
    println!("* checking get_feed_profile_header_from_database error response on killed connection succeeded.");
    let killed_phase_res = sql_interface_feed.get_feed_phase_from_database(1, 1);
    assert!(
        matches!(
            killed_phase_res,
            Err(SqlInterfaceError::SingleFeedProfileRequestFailure { .. })
        ),
        "Expected SingleFeedProfileRequestFailure on killed connection, got {:?}",
        killed_phase_res.err()
    );
    println!(
        "* checking get_feed_phase_from_database error response on killed connection succeeded."
    );
}

/// [itest->swreq~food_log_database~1]
#[test]
// Test case includes all checks for this table combined in one function to limit the number of additional databases.
// Test case uses test database #34.
pub fn test_sql_interface_feedlog() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        34,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_feed_profile = config.sql_interface.max_rows_feed_profile;
    let max_rows_feed_schedule = config.sql_interface.max_rows_feed_schedule;
    let max_rows_feed_log = config.sql_interface.max_rows_feed_log;
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut sql_interface_feed = SqlInterfaceFeed::new(
        sql_interface.get_connection().unwrap(),
        max_rows_feed_profile,
        max_rows_feed_schedule,
        max_rows_feed_log,
    )
    .unwrap();

    //*** check inserting of feed event into empty table ***************************************
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_LOG.to_string()) {
        Ok(_) => {}
        Err(e) => panic!("Could not prepare test case: {e:?}"),
    }
    let feed_timestamp =
        NaiveDateTime::parse_from_str("2022-04-11 11:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
    match sql_interface_feed.insert_feed_event(feed_timestamp, 5.5, 1) {
        Ok(_) => {}
        Err(e) => panic!("Could not insert feed event into database: {e:?}"),
    }
    // Read back feed log entry from the database
    let single_feed_event_entry_array = match sql_interface_feed.conn.query_map(
        sql_test_query_strings::SQL_QUERY_READ_LAST_FEED_EVENT,
        |(timestamp, feeder_run_time, feed_profile_id)| SqlFeedEvent {
            timestamp,
            feeder_run_time,
            feed_profile_id,
        },
    ) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not read feed event from data base: {e:?}");
        }
    };
    // check if SQL response has the right structure
    assert_eq!(single_feed_event_entry_array.len(), 1);
    // check if SQL response contains the correct data
    let feed_event = FeedEvent {
        timestamp: NaiveDateTime::parse_from_str(
            single_feed_event_entry_array[0].timestamp.as_str(),
            "%Y-%m-%d %H:%M:%S",
        )
        .expect("conversion of timestamp to NaiveDate failed."),
        feeder_run_time: single_feed_event_entry_array[0].feeder_run_time,
        feed_profile_id: single_feed_event_entry_array[0].feed_profile_id,
    };
    assert_eq!(feed_event.timestamp, feed_timestamp);
    assert_eq!(feed_event.feeder_run_time, 5.5);
    assert_eq!(feed_event.feed_profile_id, 1);
    println!("* checking inserting of feed event into log succeeded.");
    // *****************************************************************************************

    // add one more entry to check max row check
    let feed_timestamp =
        NaiveDateTime::parse_from_str("2022-04-12 11:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
    match sql_interface_feed.insert_feed_event(feed_timestamp, 5.5, 2) {
        Ok(_) => {}
        Err(e) => panic!("Could not insert feed event into database: {e:?}"),
    }

    // *** check error path of insert_feed_event when table is missing ***
    sql_interface_feed
        .conn
        .query_drop("DROP TABLE feed_log")
        .unwrap();
    let invalid_result = sql_interface_feed.insert_feed_event(feed_timestamp, 5.5, 1);
    assert!(
        matches!(
            invalid_result,
            Err(SqlInterfaceError::InsertFeedEventFailure { .. })
        ),
        "Expected InsertFeedEventFailure when table is missing, but got {:?}",
        invalid_result
    );
    println!("* checking error handling for insert_feed_event (FAIL) succeeded.");

    // Recreate feed_log table and populate 2 entries for remaining checks
    sql_interface_feed.conn.query_drop(
        "CREATE TABLE IF NOT EXISTS feed_log (timestamp DATETIME NOT NULL, feeder_on_time DOUBLE NOT NULL, profile_id INT NOT NULL)"
    ).unwrap();
    sql_interface_feed
        .insert_feed_event(feed_timestamp, 5.5, 1)
        .unwrap();
    sql_interface_feed
        .insert_feed_event(feed_timestamp, 5.5, 2)
        .unwrap();

    let test_result = SqlInterfaceFeed::new(sql_interface.get_connection().unwrap(), 0, 0, 1);
    assert!(matches!(
        test_result,
        Err(SqlInterfaceError::DatabaseFeedTableContainsTooManyRows(
            _,
            _,
            _,
            _
        ))
    ));
}

/// [itest->swreq~food_read_schedule~1]
#[test]
// Test case includes all checks for this table combined in one function to limit the number of additional databases.
// Test case uses test database #35.
pub fn test_sql_interface_feed_schedule() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        35,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_feed_profile = config.sql_interface.max_rows_feed_profile;
    let max_rows_feed_schedule = config.sql_interface.max_rows_feed_schedule;
    let max_rows_feed_log = config.sql_interface.max_rows_feed_log;
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut sql_interface_feed = SqlInterfaceFeed::new(
        sql_interface.get_connection().unwrap(),
        max_rows_feed_profile,
        max_rows_feed_schedule,
        max_rows_feed_log,
    )
    .unwrap();
    // *** check trying to load feed schedule entry when table is empty *********************
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string()) {
        Ok(_) => {}
        Err(e) => panic!("Could not prepare test case: {e:?}"),
    }
    match sql_interface_feed.get_past_feed_schedule_entries_from_database() {
        Ok(c) => {
            assert_eq!(c, None);
        }
        Err(_) => {
            panic!("Error when trying to load feed schedule entry from empty table.");
        }
    }
    println!("* reading from empty feed schedule (no result) succeeded.");
    // **************************************************************************************

    // *** check trying to load feed schedule entry *****************************************
    let timestamp_reference = "2024-04-10 14:00:00";
    let profile_id_reference = 1;
    let repeat_daily_reference = true;

    let sql_query_string = sql_test_query_strings::SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY.to_string();
    match sql_interface_feed.conn.exec_drop::<_, _>(
        sql_query_string,
        params! {
        "timestamp" => timestamp_reference,
        "profile_id" => profile_id_reference,
        "is_weekly" => false,
        "is_daily" => repeat_daily_reference,
        },
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Error when inserting feed schedule entry into database: {e:?}");
        }
    };
    println!("* wrote feed schedule entry into database.");

    let feed_schedule_entry_before_update = match sql_interface_feed
        .get_past_feed_schedule_entries_from_database()
    {
        Ok(feed_schedule_entry_vector_opt) => {
            match feed_schedule_entry_vector_opt {
                Some(feed_schedule_entry_vector) => {
                    if feed_schedule_entry_vector.len() != 1 {
                        panic!(
                            "expected to find one feed schedule entry in data base, found {}",
                            feed_schedule_entry_vector.len()
                        )
                    }
                    let feed_schedule_entry = feed_schedule_entry_vector.first().cloned().unwrap();
                    assert_eq!(
                        feed_schedule_entry.timestamp,
                        NaiveDateTime::parse_from_str(timestamp_reference, "%Y-%m-%d %H:%M:%S")
                            .unwrap()
                    );
                    println!(
                        "timestamp read from the database: {}",
                        feed_schedule_entry.timestamp
                    );
                    assert_eq!(feed_schedule_entry.profile_id, profile_id_reference);
                    assert_eq!(feed_schedule_entry.repeat_daily, repeat_daily_reference);
                    feed_schedule_entry
                }
                None => {
                    panic!("SQL database returned empty result although feed schedule entry was inserted.");
                }
            }
        }
        Err(_) => {
            panic!("Error occurred when trying to load feed schedule entry.");
        }
    };
    println!("* reading existing entry from feed schedule succeeded.");
    // *****************************************************************************************

    // *** check updating feed schedule with repetition ****************************************
    println!(
        "Feed schedule entry before database update: {}",
        feed_schedule_entry_before_update
    );

    // calculate dynamic reference
    let local_date_tomorrow: NaiveDate = (Local::now().naive_local() + Duration::days(1)).into();
    let reference_timestamp_tomorrow_same_time = NaiveDateTime::new(
        local_date_tomorrow,
        feed_schedule_entry_before_update.timestamp.time(),
    );

    let mut feed_schedule_entries_before_update: Vec<FeedScheduleEntry> = Vec::new();
    feed_schedule_entries_before_update.push(feed_schedule_entry_before_update);

    match sql_interface_feed
        .update_feed_schedule_entries_in_database(&mut feed_schedule_entries_before_update)
    {
        Ok(()) => {}
        Err(e) => {
            panic!("Error occurred when trying to update feed schedule entry which is marked for repetition: {e:?}");
        }
    }

    println!("* updated database entry");

    let mut feed_schedule_entry_after_update = match sql_interface_feed
        .get_future_feed_schedule_entry_from_database()
    {
        Ok(c) => {
            match c {
                Some(d) => {
                    assert_eq!(d.timestamp, reference_timestamp_tomorrow_same_time);
                    assert_eq!(d.profile_id, profile_id_reference);
                    assert_eq!(d.repeat_daily, repeat_daily_reference);
                    d
                }
                None => {
                    panic!("SQL database returned empty result although feed schedule entry was inserted.");
                }
            }
        }
        Err(_) => {
            panic!("Error occurred when trying to load feed schedule entry.");
        }
    };
    println!(
        "Feed schedule entry after database update: {}",
        feed_schedule_entry_after_update
    );
    println!("* update existing entry from feed schedule succeeded.");
    // *****************************************************************************************

    // *** test updating feed schedule without repetition **************************************
    // modify existing feed schedule entry -> it shall be deleted with a call to update
    feed_schedule_entry_after_update.repeat_daily = false;
    let mut feed_schedule_entries_after_update: Vec<FeedScheduleEntry> = Vec::new();
    feed_schedule_entries_after_update.push(feed_schedule_entry_after_update);

    match sql_interface_feed
        .update_feed_schedule_entries_in_database(&mut feed_schedule_entries_after_update)
    {
        Ok(()) => {}
        Err(e) => {
            panic!("Error occurred when trying to update feed schedule entry which is not marked for repetition: {e:?}");
        }
    }

    match sql_interface_feed.get_past_feed_schedule_entries_from_database() {
        Ok(c) => {
            assert_eq!(c, None);
        }
        Err(_) => {
            panic!("Error when trying to load feed schedule entry from empty table.");
        }
    };
    println!("* deleting existing entry from feed schedule succeeded.");

    // *** check error paths of update_feed_schedule_entries_in_database (table missing) ***
    let mut entries_repeat = vec![FeedScheduleEntry {
        timestamp: NaiveDateTime::parse_from_str("2024-04-10 14:00:00", "%Y-%m-%d %H:%M:%S")
            .unwrap(),
        profile_id: 1,
        repeat_daily: true,
    }];
    sql_interface_feed
        .conn
        .query_drop("DROP TABLE feed_schedule")
        .unwrap();
    let err_update =
        sql_interface_feed.update_feed_schedule_entries_in_database(&mut entries_repeat);
    assert!(
        matches!(
            err_update,
            Err(SqlInterfaceError::FeedScheduleDropFailure { .. })
        ),
        "Expected FeedScheduleDropFailure when table is missing during update, but got {:?}",
        err_update
    );

    let mut entries_no_repeat = vec![FeedScheduleEntry {
        timestamp: NaiveDateTime::parse_from_str("2024-04-10 14:00:00", "%Y-%m-%d %H:%M:%S")
            .unwrap(),
        profile_id: 1,
        repeat_daily: false,
    }];
    let err_delete =
        sql_interface_feed.update_feed_schedule_entries_in_database(&mut entries_no_repeat);
    assert!(
        matches!(
            err_delete,
            Err(SqlInterfaceError::FeedScheduleDropFailure { .. })
        ),
        "Expected FeedScheduleDropFailure when table is missing during delete, but got {:?}",
        err_delete
    );

    // Recreate table feed_schedule for remaining checks
    sql_interface_feed.conn.query_drop(
        "CREATE TABLE IF NOT EXISTS feed_schedule (timestamp DATETIME NOT NULL, profile_id INT NOT NULL, is_weekly BOOLEAN NOT NULL, is_daily BOOLEAN NOT NULL)"
    ).unwrap();
    println!(
        "* checking error handling for update_feed_schedule_entries_in_database (FAIL) succeeded."
    );
    // *****************************************************************************************

    // *** check updating multiple feed schedule entries with repetition ***********************
    // calculate dynamic reference
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string()) {
        Ok(_) => {}
        Err(e) => panic!("Could not prepare test case: {e:?}"),
    }

    let timestamp_0 = "2024-04-10 14:00:00";
    let profile_id_0 = 0;
    let repeat_daily_0 = true;

    let sql_query_string = sql_test_query_strings::SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY.to_string();
    match sql_interface_feed.conn.exec_drop::<_, _>(
        sql_query_string,
        params! {
            "timestamp" => timestamp_0,
            "profile_id" => profile_id_0,
            "is_weekly" => false,
            "is_daily" => repeat_daily_0,
        },
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Error when inserting feed schedule entry into database: {e:?}");
        }
    };

    let timestamp_1 = "2024-04-10 15:00:00";
    let profile_id_1 = 1;
    let repeat_daily_1 = true;

    let sql_query_string = sql_test_query_strings::SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY.to_string();
    match sql_interface_feed.conn.exec_drop::<_, _>(
        sql_query_string,
        params! {
            "timestamp" => timestamp_1,
            "profile_id" => profile_id_1,
            "is_weekly" => false,
            "is_daily" => repeat_daily_1,
        },
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Error when inserting feed schedule entry into database: {e:?}");
        }
    };

    feed_schedule_entries_before_update = match sql_interface_feed
        .get_past_feed_schedule_entries_from_database()
    {
        Ok(feed_schedule_entry_vector_opt) => match feed_schedule_entry_vector_opt {
            Some(feed_schedule_entry_vector) => feed_schedule_entry_vector,
            None => {
                panic!("SQL database returned empty result although feed schedule entries were inserted.");
            }
        },
        Err(_) => {
            panic!("Error occurred when trying to load feed schedule entry.");
        }
    };

    let local_date_tomorrow: NaiveDate = (Local::now().naive_local() + Duration::days(1)).into();

    let reference_timestamp0_tomorrow_same_time = NaiveDateTime::new(
        local_date_tomorrow,
        feed_schedule_entries_before_update
            .get(1)
            .unwrap()
            .timestamp
            .time(),
    );
    let reference_timestamp1_tomorrow_same_time = NaiveDateTime::new(
        local_date_tomorrow,
        feed_schedule_entries_before_update
            .first()
            .unwrap()
            .timestamp
            .time(),
    );

    match sql_interface_feed
        .update_feed_schedule_entries_in_database(&mut feed_schedule_entries_before_update)
    {
        Ok(()) => {}
        Err(e) => {
            panic!("Error occurred when trying to update feed schedule entry which is marked for repetition: {e:?}");
        }
    }

    match sql_interface_feed.get_future_feed_schedule_entries_from_database() {
        Ok(c) => {
            match c {
                Some(d) => {
                    assert_eq!(d.len(), 2);
                    assert_eq!(
                        d.first().unwrap().timestamp,
                        reference_timestamp0_tomorrow_same_time
                    );
                    assert_eq!(d.first().unwrap().profile_id, profile_id_0);
                    assert_eq!(d.first().unwrap().repeat_daily, repeat_daily_0);
                    assert_eq!(
                        d.get(1).unwrap().timestamp,
                        reference_timestamp1_tomorrow_same_time
                    );
                    assert_eq!(d.get(1).unwrap().profile_id, profile_id_1);
                    assert_eq!(d.get(1).unwrap().repeat_daily, repeat_daily_1);
                }
                None => {
                    panic!("SQL database returned empty result although feed schedule entry was inserted.");
                }
            }
        }
        Err(_) => {
            panic!("Error occurred when trying to load feed schedule entry.");
        }
    };
    println!("* update of existing entries from feed schedule succeeded.");
    // **************************************************************************************

    let test_result: Result<SqlInterfaceFeed, SqlInterfaceError> =
        SqlInterfaceFeed::new(sql_interface.get_connection().unwrap(), 100, 1, 0);
    assert!(matches!(
        test_result,
        Err(SqlInterfaceError::DatabaseFeedTableContainsTooManyRows(
            _,
            _,
            2,
            1
        ))
    ));
    println!("* checking of feed schedule max rows limitation succeeded.");
}
