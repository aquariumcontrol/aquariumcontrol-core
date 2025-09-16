/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages all database interactions for the automated feeding system.
//!
//! This module provides a dedicated interface, `SqlInterfaceFeed`, for handling
//! all SQL operations related to the feeding schedule (`feedschedule`), feed patterns
//! (`feedpatterns`), and feed event logging (`feedlog`).
//!
//! ## Key Components
//!
//! - **`SqlInterfaceFeed` Struct**: The primary struct that holds a database
//!   connection and provides methods for all feed-related database operations.
//!
//! - **`new()` Constructor**: A critical entry point that not only creates an
//!   `SqlInterfaceFeed` instance but also performs essential "fail-fast"
//!   validation. At initialization, it checks the database for:
//!   - The presence of `NULL` values in the `feedpatterns` and `feedschedule` tables.
//!   - Whether the number of rows in the `feedpatterns`, `feedschedule`, and `feedlog`
//!     tables exceeds the limits defined in the application configuration.
//!   This ensures the system doesn't start in an invalid or inconsistent state.
//!
//! - **Data-Fetching Methods**: Functions like `get_feedpattern_header_from_database`
//!   and `get_feed_phase_from_database` provide a structured way to retrieve
//!   complex feed pattern data from the database.
//!
//! ## Design and Purpose
//!
//! The main goal of this module is to provide a robust and encapsulated interface
//! for the feeding subsystem's data.
//!
//! - **Encapsulation**: All SQL queries and logic specific to feeding are contained
//!   within this module, separating concerns and making the overall database
//!   architecture cleaner.
//!
//! - **Data Integrity**: The constructor's validation logic enforces data integrity
//!   rules at startup, preventing runtime errors caused by misconfigured or
//!   overgrown database tables

use mysql::prelude::*;
use mysql::*;

use crate::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
};
use crate::food::feed_schedule_entry::FeedScheduleEntry;

#[derive(Clone)]
/// Holds the header information of a feed pattern.
pub struct SqlFeedpatternHeader {
    pub profile_id: i32,
    pub profile_name: String,
}

#[derive(Clone)]
/// Holds the data of one feed pattern phase as retrieved from the database.
/// The numeric data is post-processed to boolean in a further step.
pub struct SqlFeedPhase {
    /// duration of the pause before switching on the feeder motor in milliseconds
    pub pause_duration: i16,

    /// flag indicating if the protein skimmer shall remain switched on during the pause
    pub pause_skimmer: i16,

    /// flag indicating if the main pump #1 shall remain switched on during the pause
    pub pause_main_pump_1: i16,

    /// flag indicating if the main pump #2 shall remain switched on during the pause
    pub pause_main_pump_2: i16,

    /// flag indicating if the auxiliary pump #1 shall remain switched on during the pause
    pub pause_aux_pump_1: i16,

    /// flag indicating if the auxiliary pump #2 shall remain switched on during the pause
    pub pause_aux_pump_2: i16,

    /// duration of the feed after switching on the feeder motor in milliseconds
    pub feed_duration: i16,

    /// flag indicating if the protein skimmer shall remain switched on during the feed phase
    pub feed_skimmer: i16,

    /// flag indicating if the main pump #1 shall remain switched on during the feed phase
    pub feed_main_pump_1: i16,

    /// flag indicating if the main pump #2 shall remain switched on during the feed phase
    pub feed_main_pump_2: i16,

    /// flag indicating if the auxiliary pump #1 shall remain switched on during the feed phase
    pub feed_aux_pump_1: i16,

    /// flag indicating if the auxiliary pump #2 shall remain switched on during the feed phase
    pub feed_aux_pump_2: i16,
}

/// Holds the data of a feed schedule entry as retrieved from the database.
/// It is post-processed in a further step.
pub struct SqlFeedScheduleEntry {
    pub timestamp: String,
    pub profile_id: i32,
    pub profile_name: String,
    pub repeat_daily: i16,
}

/// Contains the configuration and the implementation of the SQL interface for Feed.
#[derive(Debug)]
pub struct SqlInterfaceFeed {
    /// connection to SQL database
    pub conn: PooledConn,
}

impl SqlInterfaceFeed {
    /// Creates a new `SqlInterfaceFeed` instance.
    ///
    /// This constructor initializes the feed management SQL interface with an
    /// established database connection. It performs several pre-flight checks to
    /// ensure data integrity and adherence to configured limits, such as verifying
    /// that tables contain no NULL values and that row counts are within their
    /// specified maximums.
    ///
    /// # Arguments
    /// * `conn` - An active, pooled database connection.
    /// * `max_rows_feed_pattern` - The maximum allowed rows in the `feedpatterns` table.
    /// * `max_rows_feed_schedule` - The maximum allowed rows in the `feedschedule` table.
    /// * `max_rows_feed_log` - The maximum allowed rows in the `feedlog` table.
    ///
    /// # Returns
    /// A `Result` containing a new `SqlInterfaceFeed` instance on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - Any of the initial database queries to get table counts or check for NULLs fail (`DatabaseCheckFeedSetValsFailure`).
    /// - Any of the retrieved counts are negative, indicating a database issue (`DatabaseFeedTableNegativeValue`).
    /// - The `feedpatterns` or `feedschedule` tables contain entries with `NULL` values (`DatabaseFeedSetValTableContainsNull`).
    /// - The number of rows in any of the checked tables exceeds its configured maximum (`DatabaseFeedTableContainsTooManyRows`). .
    pub fn new(
        mut conn: PooledConn,
        max_rows_feed_pattern: u64,
        max_rows_feed_schedule: u64,
        max_rows_feed_log: u64,
    ) -> Result<Self, SqlInterfaceError> {
        // check if feed pattern table is consistent
        let count_null_values_feedpatterns = SqlInterface::get_single_integer_from_database(
            &mut conn,
            sql_query_strings::SQL_QUERY_CHECK_FEEDPATTERNS_NULL,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckFeedSetValsFailure {
            location: module_path!().to_string(),
            table_name: sql_query_strings::SQL_TABLE_FEEDPATTERNS.to_string(),
            source: Box::new(e),
        })?;
        let count_rows_feedpatterns = SqlInterface::get_single_integer_from_database(
            &mut conn,
            sql_query_strings::SQL_QUERY_CHECK_FEEDPATTERN_COUNT,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckFeedSetValsFailure {
            location: module_path!().to_string(),
            table_name: sql_query_strings::SQL_TABLE_FEEDPATTERNS.to_string(),
            source: Box::new(e),
        })?;

        // check if the feed schedule table is consistent
        let count_null_values_feedschedule = SqlInterface::get_single_integer_from_database(
            &mut conn,
            sql_query_strings::SQL_QUERY_CHECK_FEEDSCHEDULE_NULL,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckFeedSetValsFailure {
            location: module_path!().to_string(),
            table_name: sql_query_strings::SQL_TABLE_FEEDSCHEDULE.to_string(),
            source: Box::new(e),
        })?;
        let count_rows_feedschedule = SqlInterface::get_single_integer_from_database(
            &mut conn,
            sql_query_strings::SQL_QUERY_CHECK_FEEDSCHEDULE_COUNT,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckFeedSetValsFailure {
            location: module_path!().to_string(),
            table_name: sql_query_strings::SQL_TABLE_FEEDPATTERNS.to_string(),
            source: Box::new(e),
        })?;

        let count_rows_feedlog = SqlInterface::get_single_integer_from_database(
            &mut conn,
            sql_query_strings::SQL_QUERY_CHECK_FEEDLOG_COUNT,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckFeedSetValsFailure {
            location: module_path!().to_string(),
            table_name: sql_query_strings::SQL_TABLE_FEEDLOG.to_string(),
            source: Box::new(e),
        })?;

        // check the query results
        if count_null_values_feedpatterns < 0 || count_rows_feedpatterns < 0 {
            return Err(SqlInterfaceError::DatabaseFeedTableNegativeValue(
                module_path!().to_string(),
                sql_query_strings::SQL_TABLE_FEEDPATTERNS.to_string(),
                count_null_values_feedpatterns,
                count_rows_feedpatterns,
            ));
        }
        if count_null_values_feedschedule < 0 || count_rows_feedschedule < 0 {
            return Err(SqlInterfaceError::DatabaseFeedTableNegativeValue(
                module_path!().to_string(),
                sql_query_strings::SQL_TABLE_FEEDSCHEDULE.to_string(),
                count_null_values_feedschedule,
                count_rows_feedschedule,
            ));
        }
        if count_rows_feedlog < 0 {
            return Err(SqlInterfaceError::DatabaseFeedTableNegativeValue(
                module_path!().to_string(),
                sql_query_strings::SQL_TABLE_FEEDLOG.to_string(),
                -1,
                count_rows_feedlog,
            ));
        }

        // check if there are any NULL values in the feed set value tables
        if count_null_values_feedpatterns > 0 {
            return Err(SqlInterfaceError::DatabaseFeedSetValTableContainsNull(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_CHECK_FEEDPATTERNS_NULL.to_string(),
                count_null_values_feedpatterns,
            ));
        }
        if count_null_values_feedschedule > 0 {
            return Err(SqlInterfaceError::DatabaseFeedSetValTableContainsNull(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_CHECK_FEEDSCHEDULE_NULL.to_string(),
                count_null_values_feedschedule,
            ));
        }

        if max_rows_feed_pattern > 0 {
            // execute the check only when the limit is greater than zero
            if count_rows_feedpatterns as u64 > max_rows_feed_pattern {
                return Err(SqlInterfaceError::DatabaseFeedTableContainsTooManyRows(
                    module_path!().to_string(),
                    sql_query_strings::SQL_TABLE_FEEDPATTERNS.to_string(),
                    count_rows_feedpatterns as u64,
                    max_rows_feed_pattern,
                ));
            }
        }

        if max_rows_feed_schedule > 0 {
            // execute the check only when the limit is greater than zero
            if count_rows_feedschedule as u64 > max_rows_feed_schedule {
                return Err(SqlInterfaceError::DatabaseFeedTableContainsTooManyRows(
                    module_path!().to_string(),
                    sql_query_strings::SQL_TABLE_FEEDSCHEDULE.to_string(),
                    count_rows_feedschedule as u64,
                    max_rows_feed_schedule,
                ));
            }
        }

        if max_rows_feed_log > 0 {
            // execute the check only when the limit is greater than zero
            if count_rows_feedlog as u64 > max_rows_feed_log {
                return Err(SqlInterfaceError::DatabaseFeedTableContainsTooManyRows(
                    module_path!().to_string(),
                    sql_query_strings::SQL_TABLE_FEEDLOG.to_string(),
                    count_rows_feedlog as u64,
                    max_rows_feed_log,
                ));
            }
        }

        Ok(SqlInterfaceFeed { conn })
    }

    /// Retrieves and post-processes feed schedule entries from the database based on a given SQL query.
    ///
    /// This private helper function executes the provided `sql_query` to fetch raw schedule data.
    /// It then attempts to convert these raw `SqlScheduleEntry` records into fully typed
    /// `FeedScheduleEntry` structs.
    ///
    /// # Arguments
    /// * `sql_query` - The SQL query string used to retrieve feed schedule entries.
    ///
    /// # Returns
    /// A `Result` containing an `Option<Vec<FeedScheduleEntry>>`:
    /// - `Ok(Some(Vec<FeedScheduleEntry>))`: If one or more schedule entries are successfully retrieved and converted.
    /// - `Ok(None)`: If the query returns no rows.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The underlying database query fails (`FeedScheduleEntryRequestFailure`). This can be due to a connection issue or an invalid query.
    /// - The query returns results, but one of the entries cannot be processed (e.g., due to a malformed timestamp), resulting in `FeedScheduleEntryTimeStampProcessingFailure`.
    pub(crate) fn get_feedschedule_entries_from_database(
        &mut self,
        sql_query: &str,
    ) -> Result<Option<Vec<FeedScheduleEntry>>, SqlInterfaceError> {
        let sql_feedschedule_entry_array = match self.conn.query_map(
            sql_query,
            |(timestamp, profile_id, profile_name, repeat_daily)| SqlFeedScheduleEntry {
                timestamp,
                profile_id,
                profile_name,
                repeat_daily,
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                return Err(SqlInterfaceError::FeedScheduleEntryRequestFailure {
                    location: module_path!().to_string(),
                    query: sql_query.to_string(),
                    source: e,
                });
            }
        };

        // return none if the query result is empty
        if sql_feedschedule_entry_array.is_empty() {
            return Ok(None);
        }

        let feedschedule_entries_result: Result<Vec<FeedScheduleEntry>, _> =
            sql_feedschedule_entry_array
                .iter()
                .map(FeedScheduleEntry::new)
                .collect();

        // Handle conversion errors that may occur during the mapping.
        let feedschedule_entries = feedschedule_entries_result.map_err(|e| {
            SqlInterfaceError::FeedScheduleEntryTimeStampProcessingFailure {
                location: module_path!().to_string(),
                number_entries: sql_feedschedule_entry_array.len(),
                source: Box::new(e),
            }
        })?;

        if feedschedule_entries.is_empty() {
            Ok(None)
        } else {
            Ok(Some(feedschedule_entries))
        }
    }

    #[cfg(test)]
    // Retrieves the single feed schedule entry that is furthest in the future.
    //
    // This private helper function is intended for test scenarios. It queries the database
    // for future-scheduled feed entries and, assuming they are ordered by timestamp,
    // returns the earliest (furthest in the future) one.
    //
    // # Returns
    // A `Result` which is:
    // - `Ok(Some(FeedScheduleEntry))` if a future feed schedule entry is found.
    // - `Ok(None)` if no future feed schedule entries are found in the database.
    // - `Err(SqlInterfaceError)` if an error occurs during the database query
    //   or post-processing of the data.
    fn get_future_feedschedule_entry_from_database(
        &mut self,
    ) -> Result<Option<FeedScheduleEntry>, SqlInterfaceError> {
        let entries_opt = self.get_feedschedule_entries_from_database(
            sql_query_strings::SQL_QUERY_READ_FUTURE_FEED_SCHEDULE_ENTRY,
        )?;
        Ok(entries_opt.and_then(|mut vec| vec.pop()))
    }

    #[cfg(test)]
    // Retrieves all feed schedule entries that are scheduled in the future.
    //
    // This private helper function is intended for test scenarios. It queries the database
    // for all feed entries whose timestamps are in the future, then processes them
    // into a vector of `FeedScheduleEntry` structs.
    //
    // # Returns
    // A `Result` which is:
    // - `Ok(Some(Vec<FeedScheduleEntry>))` if one or more future feed schedule entries are found.
    // - `Ok(None)` if no future feed schedule entries are found in the database.
    // - `Err(SqlInterfaceError)` if an error occurs during the database query
    //   or post-processing of the data.
    fn get_future_feedschedule_entries_from_database(
        &mut self,
    ) -> Result<Option<Vec<FeedScheduleEntry>>, SqlInterfaceError> {
        self.get_feedschedule_entries_from_database(
            sql_query_strings::SQL_QUERY_READ_FUTURE_FEED_SCHEDULE_ENTRY,
        )
    }

    /// Retrieves the header (basic information) of a specific feed pattern from the database.
    ///
    /// This function queries the database using the provided `profile_id` to fetch
    /// the feed pattern's unique ID and its associated name. It strictly expects
    /// exactly one matching header entry.
    ///
    /// # Arguments
    /// * `profile_id` - The unique identifier of the feed profile whose header is to be retrieved.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(SqlFeedpatternHeader)` if exactly one matching header entry is found.
    /// - `Err(SqlInterfaceError::SingleFeedpatternRequestFailure)` if the database query fails.
    /// - `Err(SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse)` if the query
    ///   returns no entry or more than one entry for the given `profile_id`.
    pub fn get_feedpattern_header_from_database(
        &mut self,
        profile_id: i32,
    ) -> Result<SqlFeedpatternHeader, SqlInterfaceError> {
        // prepare the query for header
        let params = params! { "profile_id" => profile_id};

        let single_feedpattern_entry_array = self
            .conn
            .exec_map(
                sql_query_strings::SQL_QUERY_READ_FEEDPATTERN_HEADER,
                params,
                |(profile_id, profile_name)| SqlFeedpatternHeader {
                    profile_id,
                    profile_name,
                },
            )
            .map_err(|e| SqlInterfaceError::SingleFeedpatternRequestFailure {
                location: module_path!().to_string(),
                query: sql_query_strings::SQL_QUERY_READ_FEEDPATTERN_HEADER.to_string(),
                profile_id,
                source: e,
            })?;

        // check the error case if there is no response
        if single_feedpattern_entry_array.is_empty() {
            return Err(SqlInterfaceError::SingleFeedpatternRequestEmptyResponse(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_READ_FEEDPATTERN_HEADER.to_string(),
                profile_id,
            ));
        }

        // check the error case if there is more than one response
        if single_feedpattern_entry_array.len() > 1 {
            return Err(SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_READ_FEEDPATTERN_HEADER.to_string(),
                profile_id,
            ));
        }

        let feedpattern_header = &single_feedpattern_entry_array[0];

        Ok(feedpattern_header.clone())
    }

    /// Retrieves the specific data for a single phase of a feed pattern from the database.
    ///
    /// This function constructs a query using both the `profile_id` and the `phase_nr`
    /// to fetch the detailed configuration for that particular feed phase. It strictly
    /// expects to find exactly one matching phase entry.
    ///
    /// # Arguments
    /// * `profile_id` - The unique identifier of the feed profile.
    /// * `phase_nr` - The specific phase number (e.g., 1, 2, ..., 10) within the feed profile.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(SqlFeedPhase)` if exactly one matching phase entry is found for the given profile and phase number.
    /// - `Err(SqlInterfaceError::SingleFeedpatternRequestFailure)` if the database query fails.
    /// - `Err(SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse)` if the query
    ///   returns no entry or more than one entry for the specified profile and phase.
    pub fn get_feed_phase_from_database(
        &mut self,
        profile_id: i32,
        phase_nr: i32,
    ) -> Result<SqlFeedPhase, SqlInterfaceError> {
        let params = params!("profile_id" => profile_id);
        let sql_query = str::replace(
            sql_query_strings::SQL_QUERY_READ_FEED_PHASE,
            "#",
            format!("{phase_nr:02}").as_str(),
        );

        let single_feedpattern_entry_array = match self.conn.exec_map(
            sql_query.clone(),
            params,
            |(
                pause_duration,
                pause_skimmer,
                pause_main_pump_1,
                pause_main_pump_2,
                pause_aux_pump_1,
                pause_aux_pump_2,
                feed_duration,
                feed_skimmer,
                feed_main_pump_1,
                feed_main_pump_2,
                feed_aux_pump_1,
                feed_aux_pump_2,
            )| SqlFeedPhase {
                pause_duration,
                pause_skimmer,
                pause_main_pump_1,
                pause_main_pump_2,
                pause_aux_pump_1,
                pause_aux_pump_2,
                feed_duration,
                feed_skimmer,
                feed_main_pump_1,
                feed_main_pump_2,
                feed_aux_pump_1,
                feed_aux_pump_2,
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                return Err(SqlInterfaceError::SingleFeedpatternRequestFailure {
                    location: module_path!().to_string(),
                    query: sql_query,
                    profile_id,
                    source: e,
                });
            }
        };

        // check the error case if there is no response
        if single_feedpattern_entry_array.is_empty() {
            return Err(SqlInterfaceError::SingleFeedpatternRequestEmptyResponse(
                module_path!().to_string(),
                sql_query,
                profile_id,
            ));
        }

        // check the error case if there is more than one response
        if single_feedpattern_entry_array.len() > 1 {
            return Err(SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse(
                module_path!().to_string(),
                sql_query,
                profile_id,
            ));
        }

        let feed_phase = &single_feedpattern_entry_array[0];

        // check if pause and feed duration values are positive
        if feed_phase.pause_duration < 0 || feed_phase.feed_duration < 0 {
            return Err(SqlInterfaceError::DatabaseFeedPhaseDurationNegative(
                module_path!().to_string(),
                feed_phase.pause_duration,
                feed_phase.feed_duration,
            ));
        }
        Ok(feed_phase.clone())
    }
}

#[cfg(test)]
pub mod tests {
    use chrono::{Duration, Local, NaiveDate, NaiveDateTime};
    use mysql::{params, prelude::Queryable};

    use crate::database::database_interface_feed_trait::DatabaseInterfaceFeedTrait;
    use crate::database::sql_interface_feed::{FeedScheduleEntry, SqlInterfaceFeed};
    use crate::database::sql_query_strings;
    use crate::database::sql_query_strings::{
        SQL_TABLE_FEEDLOG, SQL_TABLE_FEEDPATTERNS, SQL_TABLE_FEEDSCHEDULE,
    };
    use crate::database::{sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError};
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};

    pub struct SqlFeedEvent {
        timestamp: String,
        feeder_run_time: f32,
        feed_profile_name: String,
        feed_profile_id: i32,
    }

    pub struct FeedEvent {
        timestamp: NaiveDateTime,
        feeder_run_time: f32,
        feed_profile_name: String,
        feed_profile_id: i32,
    }

    #[cfg(test)]
    // This helper function prepares the database for feed-related test cases.
    // It truncates the 'feedpatterns' table to ensure a clean state and then
    // inserts two predefined test feed profiles.
    //
    // Arguments:
    // * `sql_interface`: A mutable reference to the general SQL interface, used for table truncation.
    // * `sql_interface_feed`: A mutable reference to the feed-specific SQL interface, used for inserting patterns.
    pub fn insert_feed_patterns(
        sql_interface: &mut SqlInterface,
        sql_interface_feed: &mut SqlInterfaceFeed,
    ) {
        // remove all feed patterns
        match SqlInterface::truncate_table(sql_interface, SQL_TABLE_FEEDPATTERNS.to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        // insert test feed profile #1
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_FEEDPATTERN_TEST.to_string();
        match sql_interface_feed.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "profile_id" => 1, "profile_name" => "test"
            },
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Error when inserting test feed pattern #1 into database: {e:?}");
            }
        };
        // insert test feed profile #2
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_FEEDPATTERN_TEST2.to_string();
        match sql_interface_feed.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "profile_id" => 2,
            },
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Error when inserting test feed pattern #2 into database: {e:?}");
            }
        };
    }

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
            "/config/aquarium_control_test_generic.toml".to_string(),
            59,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");

        // --- Test Case 1: Happy Path ---
        println!("* Testing new() with valid data (Happy Path)...");
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEEDPATTERNS.to_string())
            .unwrap();
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEEDSCHEDULE.to_string())
            .unwrap();
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEEDLOG.to_string()).unwrap();

        // Insert one valid row into each table
        let mut conn = sql_interface.get_connection().unwrap();
        conn.exec_drop(
            sql_query_strings::SQL_QUERY_WRITE_FEEDPATTERN_TEST,
            params! { "profile_id" => 1, "profile_name" => "test_profile1" },
        )
        .unwrap();
        conn.exec_drop(
            sql_query_strings::SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY,
            params! { "timestamp" => "2024-01-01 12:00:00", "profile_id" => 1, "profile_name" => "test_profile1", "is_weekly" => false, "is_daily" => true },
        ).unwrap();
        conn.exec_drop(
            sql_query_strings::SQL_QUERY_WRITE_FEED_EVENT,
            params! { "timestamp" => Local::now().naive_local(), "feeder_run_time" => 1.0, "profile_name" => "test_profile1", "profile_id" => 1 },
        ).unwrap();

        let result = SqlInterfaceFeed::new(conn, 10, 10, 10);
        assert!(
            result.is_ok(),
            "Expected new() to succeed on happy path, but it failed: {:?}",
            result.err()
        );
        println!("* Succeeded: Happy path initialization is successful.");

        // --- Test Case 2: Failure on feedpatterns row limit ---
        println!("* Testing new() with too many rows in feedpatterns...");
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEEDPATTERNS.to_string())
            .unwrap();
        let mut conn = sql_interface.get_connection().unwrap();
        conn.exec_drop(
            sql_query_strings::SQL_QUERY_WRITE_FEEDPATTERN_TEST,
            params! { "profile_id" => 1, "profile_name" => "row1" },
        )
        .unwrap();
        conn.exec_drop(
            sql_query_strings::SQL_QUERY_WRITE_FEEDPATTERN_TEST,
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
            "Expected row limit error for feedpatterns, but got {:?}",
            result
        );
        println!("* Succeeded: Initialization fails if feedpatterns exceeds row limit.");

        // --- Test Case 4: Deactivated checks (limits are 0) ---
        println!("* Testing new() with deactivated row limit checks (limits = 0)...");
        // We reuse the state from the previous test (2 rows in feedpatterns)
        let conn = sql_interface.get_connection().unwrap();
        // Set all the limits to 0
        let result = SqlInterfaceFeed::new(conn, 0, 0, 0);
        assert!(
            result.is_ok(),
            "Expected new() to succeed with deactivated checks, but it failed: {:?}",
            result.err()
        );
        println!("* Succeeded: Initialization passes when row limit checks are deactivated.");
    }

    #[test]
    // Test case includes all checks for this table combined in one function to limit the number of additional databases.
    // Test case uses test database #33.
    pub fn test_sql_interface_feedpattern() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            33,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_feed_pattern = config.sql_interface.max_rows_feed_pattern;
        let max_rows_feed_schedule = config.sql_interface.max_rows_feed_schedule;
        let max_rows_feed_log = config.sql_interface.max_rows_feed_log;
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_feed = SqlInterfaceFeed::new(
            sql_interface.get_connection().unwrap(),
            max_rows_feed_pattern,
            max_rows_feed_schedule,
            max_rows_feed_log,
        )
        .unwrap();

        //*** check retrieving of the feed pattern from empty table *********************************
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEEDPATTERNS.to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        match sql_interface_feed.get_single_feedpattern_from_database(1) {
            Ok(_) => {
                panic!("Call to get_single_feedpattern_from_database should have failed.");
            }
            Err(e) => {
                assert!(matches!(
                    e,
                    SqlInterfaceError::SingleFeedpatternRequestEmptyResponse(_, _, _)
                ));
            }
        }
        println!("* checking retrieving of Feed pattern (FAIL) succeeded.");
        // **************************************************************************************

        // *** check retrieving of existing feed pattern ****************************************
        // prepare database: insert feed profile
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_FEEDPATTERN_TEST.to_string();
        match sql_interface_feed.conn.exec_drop::<_, _>(
            sql_query_string.clone(),
            params! {
            "profile_id" => 1, "profile_name" => "test",
            },
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Error when inserting feed pattern into database: {e:?}");
            }
        };
        match sql_interface_feed.get_single_feedpattern_from_database(1) {
            Ok(c) => {
                assert_eq!(c.profile_id, 1);
                assert_eq!(c.profile_name, "test".to_string());
                assert_eq!(c.feedphases[0].pause_duration, 0);
                assert_eq!(c.feedphases[0].pause_skimmer, true);
                assert_eq!(c.feedphases[0].pause_main_pump_1, false);
                assert_eq!(c.feedphases[0].pause_main_pump_2, false);
                assert_eq!(c.feedphases[0].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[0].pause_aux_pump_2, false);
                assert_eq!(c.feedphases[0].feed_duration, 1);
                assert_eq!(c.feedphases[0].feed_skimmer, false);
                assert_eq!(c.feedphases[0].feed_main_pump_1, true);
                assert_eq!(c.feedphases[0].feed_main_pump_2, false);
                assert_eq!(c.feedphases[0].feed_aux_pump_1, false);
                assert_eq!(c.feedphases[0].feed_aux_pump_2, false);

                assert_eq!(c.feedphases[1].pause_duration, 2);
                assert_eq!(c.feedphases[1].pause_skimmer, false);
                assert_eq!(c.feedphases[1].pause_main_pump_1, false);
                assert_eq!(c.feedphases[1].pause_main_pump_2, true);
                assert_eq!(c.feedphases[1].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[1].pause_aux_pump_2, false);
                assert_eq!(c.feedphases[1].feed_duration, 3);
                assert_eq!(c.feedphases[1].feed_skimmer, false);
                assert_eq!(c.feedphases[1].feed_main_pump_1, false);
                assert_eq!(c.feedphases[1].feed_main_pump_2, false);
                assert_eq!(c.feedphases[1].feed_aux_pump_1, true);
                assert_eq!(c.feedphases[1].feed_aux_pump_2, false);

                assert_eq!(c.feedphases[2].pause_duration, 4);
                assert_eq!(c.feedphases[2].pause_skimmer, false);
                assert_eq!(c.feedphases[2].pause_main_pump_1, false);
                assert_eq!(c.feedphases[2].pause_main_pump_2, false);
                assert_eq!(c.feedphases[2].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[2].pause_aux_pump_2, true);
                assert_eq!(c.feedphases[2].feed_duration, 5);
                assert_eq!(c.feedphases[2].feed_skimmer, true);
                assert_eq!(c.feedphases[2].feed_main_pump_1, true);
                assert_eq!(c.feedphases[2].feed_main_pump_2, false);
                assert_eq!(c.feedphases[2].feed_aux_pump_1, false);
                assert_eq!(c.feedphases[2].feed_aux_pump_2, false);

                assert_eq!(c.feedphases[3].pause_duration, 6);
                assert_eq!(c.feedphases[3].pause_skimmer, true);
                assert_eq!(c.feedphases[3].pause_main_pump_1, false);
                assert_eq!(c.feedphases[3].pause_main_pump_2, true);
                assert_eq!(c.feedphases[3].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[3].pause_aux_pump_2, false);
                assert_eq!(c.feedphases[3].feed_duration, 7);
                assert_eq!(c.feedphases[3].feed_skimmer, true);
                assert_eq!(c.feedphases[3].feed_main_pump_1, false);
                assert_eq!(c.feedphases[3].feed_main_pump_2, false);
                assert_eq!(c.feedphases[3].feed_aux_pump_1, true);
                assert_eq!(c.feedphases[3].feed_aux_pump_2, false);

                assert_eq!(c.feedphases[4].pause_duration, 8);
                assert_eq!(c.feedphases[4].pause_skimmer, true);
                assert_eq!(c.feedphases[4].pause_main_pump_1, false);
                assert_eq!(c.feedphases[4].pause_main_pump_2, false);
                assert_eq!(c.feedphases[4].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[4].pause_aux_pump_2, true);
                assert_eq!(c.feedphases[4].feed_duration, 9);
                assert_eq!(c.feedphases[4].feed_skimmer, true);
                assert_eq!(c.feedphases[4].feed_main_pump_1, true);
                assert_eq!(c.feedphases[4].feed_main_pump_2, true);
                assert_eq!(c.feedphases[4].feed_aux_pump_1, false);
                assert_eq!(c.feedphases[4].feed_aux_pump_2, false);

                assert_eq!(c.feedphases[5].pause_duration, 10);
                assert_eq!(c.feedphases[5].pause_skimmer, true);
                assert_eq!(c.feedphases[5].pause_main_pump_1, true);
                assert_eq!(c.feedphases[5].pause_main_pump_2, true);
                assert_eq!(c.feedphases[5].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[5].pause_aux_pump_2, false);
                assert_eq!(c.feedphases[5].feed_duration, 11);
                assert_eq!(c.feedphases[5].feed_skimmer, true);
                assert_eq!(c.feedphases[5].feed_main_pump_1, true);
                assert_eq!(c.feedphases[5].feed_main_pump_2, false);
                assert_eq!(c.feedphases[5].feed_aux_pump_1, false);
                assert_eq!(c.feedphases[5].feed_aux_pump_2, true);

                assert_eq!(c.feedphases[6].pause_duration, 12);
                assert_eq!(c.feedphases[6].pause_skimmer, true);
                assert_eq!(c.feedphases[6].pause_main_pump_1, true);
                assert_eq!(c.feedphases[6].pause_main_pump_2, true);
                assert_eq!(c.feedphases[6].pause_aux_pump_1, true);
                assert_eq!(c.feedphases[6].pause_aux_pump_2, false);
                assert_eq!(c.feedphases[6].feed_duration, 13);
                assert_eq!(c.feedphases[6].feed_skimmer, true);
                assert_eq!(c.feedphases[6].feed_main_pump_1, true);
                assert_eq!(c.feedphases[6].feed_main_pump_2, true);
                assert_eq!(c.feedphases[6].feed_aux_pump_1, false);
                assert_eq!(c.feedphases[6].feed_aux_pump_2, true);

                assert_eq!(c.feedphases[7].pause_duration, 14);
                assert_eq!(c.feedphases[7].pause_skimmer, true);
                assert_eq!(c.feedphases[7].pause_main_pump_1, true);
                assert_eq!(c.feedphases[7].pause_main_pump_2, true);
                assert_eq!(c.feedphases[7].pause_aux_pump_1, true);
                assert_eq!(c.feedphases[7].pause_aux_pump_2, true);
                assert_eq!(c.feedphases[7].feed_duration, 15);
                assert_eq!(c.feedphases[7].feed_skimmer, true);
                assert_eq!(c.feedphases[7].feed_main_pump_1, true);
                assert_eq!(c.feedphases[7].feed_main_pump_2, true);
                assert_eq!(c.feedphases[7].feed_aux_pump_1, true);
                assert_eq!(c.feedphases[7].feed_aux_pump_2, false);

                assert_eq!(c.feedphases[8].pause_duration, 16);
                assert_eq!(c.feedphases[8].pause_skimmer, true);
                assert_eq!(c.feedphases[8].pause_main_pump_1, true);
                assert_eq!(c.feedphases[8].pause_main_pump_2, true);
                assert_eq!(c.feedphases[8].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[8].pause_aux_pump_2, false);
                assert_eq!(c.feedphases[8].feed_duration, 17);
                assert_eq!(c.feedphases[8].feed_skimmer, true);
                assert_eq!(c.feedphases[8].feed_main_pump_1, true);
                assert_eq!(c.feedphases[8].feed_main_pump_2, false);
                assert_eq!(c.feedphases[8].feed_aux_pump_1, false);
                assert_eq!(c.feedphases[8].feed_aux_pump_2, false);

                assert_eq!(c.feedphases[9].pause_duration, 18);
                assert_eq!(c.feedphases[9].pause_skimmer, true);
                assert_eq!(c.feedphases[9].pause_main_pump_1, false);
                assert_eq!(c.feedphases[9].pause_main_pump_2, false);
                assert_eq!(c.feedphases[9].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[9].pause_aux_pump_2, false);
                assert_eq!(c.feedphases[9].feed_duration, 19);
                assert_eq!(c.feedphases[9].feed_skimmer, false);
                assert_eq!(c.feedphases[9].feed_main_pump_1, false);
                assert_eq!(c.feedphases[9].feed_main_pump_2, false);
                assert_eq!(c.feedphases[9].feed_aux_pump_1, false);
                assert_eq!(c.feedphases[9].feed_aux_pump_2, false);
            }
            Err(e) => {
                panic!("Call to get_single_feedpattern_from_database has failed: {e:?}.");
            }
        }
        println!("* checking retrieving of feed pattern succeeded.");

        match sql_interface_feed.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "profile_id" => 2, "profile_name" => "test2"
            },
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Error when inserting feed pattern into database: {e:?}");
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
        println!("* checking of feed pattern max rows limitation succeeded.");

        // *****************************************************************************************
    }

    #[test]
    // Test case includes all checks for this table combined in one function to limit the number of additional databases.
    // Test case uses test database #34.
    pub fn test_sql_interface_feedlog() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            34,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_feed_pattern = config.sql_interface.max_rows_feed_pattern;
        let max_rows_feed_schedule = config.sql_interface.max_rows_feed_schedule;
        let max_rows_feed_log = config.sql_interface.max_rows_feed_log;
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_feed = SqlInterfaceFeed::new(
            sql_interface.get_connection().unwrap(),
            max_rows_feed_pattern,
            max_rows_feed_schedule,
            max_rows_feed_log,
        )
        .unwrap();

        //*** check inserting of feed event into empty table ***************************************
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEEDLOG.to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        let feed_timestamp =
            NaiveDateTime::parse_from_str("2022-04-11 11:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        match sql_interface_feed.insert_feed_event(feed_timestamp, 5.5, "Test".to_string(), 1) {
            Ok(_) => {}
            Err(e) => panic!("Could not insert feed event into database: {e:?}"),
        }
        // Read back feed log entry from the database
        let single_feed_event_entry_array = match sql_interface_feed.conn.query_map(
            sql_query_strings::SQL_QUERY_READ_LAST_FEED_EVENT,
            |(timestamp, feeder_run_time, feed_profile_name, feed_profile_id)| SqlFeedEvent {
                timestamp,
                feeder_run_time,
                feed_profile_name,
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
            feed_profile_name: single_feed_event_entry_array[0].feed_profile_name.clone(),
            feed_profile_id: single_feed_event_entry_array[0].feed_profile_id,
        };
        assert_eq!(feed_event.timestamp, feed_timestamp);
        assert_eq!(feed_event.feeder_run_time, 5.5);
        assert_eq!(feed_event.feed_profile_name, "Test");
        assert_eq!(feed_event.feed_profile_id, 1);
        println!("* checking inserting of feed event into log succeeded.");
        // *****************************************************************************************

        // add one more entry to check max row check
        let feed_timestamp =
            NaiveDateTime::parse_from_str("2022-04-12 11:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        match sql_interface_feed.insert_feed_event(feed_timestamp, 5.5, "Test2".to_string(), 2) {
            Ok(_) => {}
            Err(e) => panic!("Could not insert feed event into database: {e:?}"),
        }
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

    #[test]
    // Test case includes all checks for this table combined in one function to limit the number of additional databases.
    // Test case uses test database #35.
    pub fn test_sql_interface_feedschedule() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            35,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_feed_pattern = config.sql_interface.max_rows_feed_pattern;
        let max_rows_feed_schedule = config.sql_interface.max_rows_feed_schedule;
        let max_rows_feed_log = config.sql_interface.max_rows_feed_log;
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_feed = SqlInterfaceFeed::new(
            sql_interface.get_connection().unwrap(),
            max_rows_feed_pattern,
            max_rows_feed_schedule,
            max_rows_feed_log,
        )
        .unwrap();
        // *** check trying to load feed schedule entry when table is empty *********************
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEEDSCHEDULE.to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        match sql_interface_feed.get_past_feedschedule_entries_from_database() {
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
        let profile_name_reference = "test";
        let repeat_daily_reference = true;

        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY.to_string();
        match sql_interface_feed.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "timestamp" => timestamp_reference,
            "profile_id" => profile_id_reference,
            "profile_name" => profile_name_reference,
            "is_weekly" => false,
            "is_daily" => repeat_daily_reference,
            },
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Error when inserting feed schedule entry into database: {e:?}");
            }
        };

        let feedschedule_entry_before_update = match sql_interface_feed
            .get_past_feedschedule_entries_from_database()
        {
            Ok(feedschedule_entry_vector_opt) => match feedschedule_entry_vector_opt {
                Some(feedschedule_entry_vector) => {
                    if feedschedule_entry_vector.len() != 1 {
                        panic!(
                            "expected to find one feed schedule entry in data base, found {}",
                            feedschedule_entry_vector.len()
                        )
                    }
                    let feedschedule_entry = feedschedule_entry_vector.get(0).cloned().unwrap();
                    assert_eq!(
                        feedschedule_entry.timestamp,
                        NaiveDateTime::parse_from_str(timestamp_reference, "%Y-%m-%d %H:%M:%S")
                            .unwrap()
                    );
                    println!(
                        "timestamp read from the database: {}",
                        feedschedule_entry.timestamp
                    );
                    assert_eq!(feedschedule_entry.profile_id, profile_id_reference);
                    assert_eq!(feedschedule_entry.profile_name, profile_name_reference);
                    assert_eq!(feedschedule_entry.repeat_daily, repeat_daily_reference);
                    feedschedule_entry
                }
                None => {
                    panic!("SQL database returned empty result although feed schedule entry was inserted.");
                }
            },
            Err(_) => {
                panic!("Error occurred when trying to load feed schedule entry.");
            }
        };
        println!("* reading existing entry from feed schedule succeeded.");
        // *****************************************************************************************

        // *** check updating feed schedule with repetition ****************************************
        println!(
            "Feed schedule entry before database update: {}",
            feedschedule_entry_before_update
        );

        // calculate dynamic reference
        let local_date_tomorrow: NaiveDate =
            (Local::now().naive_local() + Duration::days(1)).into();
        let reference_timestamp_tomorrow_same_time = NaiveDateTime::new(
            local_date_tomorrow,
            feedschedule_entry_before_update.timestamp.time(),
        );

        let mut feedschedule_entries_before_update: Vec<FeedScheduleEntry> = Vec::new();
        feedschedule_entries_before_update.push(feedschedule_entry_before_update);

        match sql_interface_feed
            .update_feedschedule_entries_in_database(&mut feedschedule_entries_before_update)
        {
            Ok(()) => {}
            Err(e) => {
                panic!("Error occurred when trying to update feedschedule entry which is marked for repetition: {e:?}");
            }
        }

        let mut feedschedule_entry_after_update = match sql_interface_feed
            .get_future_feedschedule_entry_from_database()
        {
            Ok(c) => match c {
                Some(d) => {
                    assert_eq!(d.timestamp, reference_timestamp_tomorrow_same_time);
                    assert_eq!(d.profile_id, profile_id_reference);
                    assert_eq!(d.profile_name, profile_name_reference);
                    assert_eq!(d.repeat_daily, repeat_daily_reference);
                    d
                }
                None => {
                    panic!("SQL database returned empty result although feed schedule entry was inserted.");
                }
            },
            Err(_) => {
                panic!("Error occurred when trying to load feed schedule entry.");
            }
        };
        println!(
            "Feed schedule entry after database update: {}",
            feedschedule_entry_after_update
        );
        println!("* update existing entry from feed schedule succeeded.");
        // *****************************************************************************************

        // *** test updating feed schedule without repetition **************************************
        // modify existing feed schedule entry -> it shall be deleted with a call to update
        feedschedule_entry_after_update.repeat_daily = false;
        let mut feedschedule_entries_after_update: Vec<FeedScheduleEntry> = Vec::new();
        feedschedule_entries_after_update.push(feedschedule_entry_after_update);

        match sql_interface_feed
            .update_feedschedule_entries_in_database(&mut feedschedule_entries_after_update)
        {
            Ok(()) => {}
            Err(e) => {
                panic!("Error occurred when trying to update feedschedule entry which is not marked for repetition: {e:?}");
            }
        }

        match sql_interface_feed.get_past_feedschedule_entries_from_database() {
            Ok(c) => {
                assert_eq!(c, None);
            }
            Err(_) => {
                panic!("Error when trying to load feed schedule entry from empty table.");
            }
        };
        println!("* deleting existing entry from feed schedule succeeded.");
        // *****************************************************************************************

        // *** check updating multiple feed schedule entries with repetition ***********************
        // calculate dynamic reference
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEEDSCHEDULE.to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }

        let timestamp_0 = "2024-04-10 14:00:00";
        let profile_id_0 = 0;
        let profile_name_0 = "test0";
        let repeat_daily_0 = true;

        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY.to_string();
        match sql_interface_feed.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
                "timestamp" => timestamp_0,
                "profile_id" => profile_id_0,
                "profile_name" => profile_name_0,
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
        let profile_name_1 = "test1";
        let repeat_daily_1 = true;

        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY.to_string();
        match sql_interface_feed.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
                "timestamp" => timestamp_1,
                "profile_id" => profile_id_1,
                "profile_name" => profile_name_1,
                "is_weekly" => false,
                "is_daily" => repeat_daily_1,
            },
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Error when inserting feed schedule entry into database: {e:?}");
            }
        };

        let mut feedschedule_entries_before_update = match sql_interface_feed
            .get_past_feedschedule_entries_from_database()
        {
            Ok(feedschedule_entry_vector_opt) => match feedschedule_entry_vector_opt {
                Some(feedschedule_entry_vector) => feedschedule_entry_vector,
                None => {
                    panic!("SQL database returned empty result although feed schedule entries were inserted.");
                }
            },
            Err(_) => {
                panic!("Error occurred when trying to load feed schedule entry.");
            }
        };

        let local_date_tomorrow: NaiveDate =
            (Local::now().naive_local() + Duration::days(1)).into();

        let reference_timestamp0_tomorrow_same_time = NaiveDateTime::new(
            local_date_tomorrow,
            feedschedule_entries_before_update
                .get(1)
                .unwrap()
                .timestamp
                .time(),
        );
        let reference_timestamp1_tomorrow_same_time = NaiveDateTime::new(
            local_date_tomorrow,
            feedschedule_entries_before_update
                .get(0)
                .unwrap()
                .timestamp
                .time(),
        );

        match sql_interface_feed
            .update_feedschedule_entries_in_database(&mut feedschedule_entries_before_update)
        {
            Ok(()) => {}
            Err(e) => {
                panic!("Error occurred when trying to update feedschedule entry which is marked for repetition: {e:?}");
            }
        }

        match sql_interface_feed.get_future_feedschedule_entries_from_database() {
            Ok(c) => match c {
                Some(d) => {
                    assert_eq!(d.len(), 2);
                    assert_eq!(
                        d.get(0).unwrap().timestamp,
                        reference_timestamp0_tomorrow_same_time
                    );
                    assert_eq!(d.get(0).unwrap().profile_id, profile_id_0);
                    assert_eq!(d.get(0).unwrap().profile_name, profile_name_0);
                    assert_eq!(d.get(0).unwrap().repeat_daily, repeat_daily_0);
                    assert_eq!(
                        d.get(1).unwrap().timestamp,
                        reference_timestamp1_tomorrow_same_time
                    );
                    assert_eq!(d.get(1).unwrap().profile_id, profile_id_1);
                    assert_eq!(d.get(1).unwrap().profile_name, profile_name_1);
                    assert_eq!(d.get(1).unwrap().repeat_daily, repeat_daily_1);
                }
                None => {
                    panic!("SQL database returned empty result although feed schedule entry was inserted.");
                }
            },
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
}
