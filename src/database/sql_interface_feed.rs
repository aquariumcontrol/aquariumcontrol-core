/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages all database interactions for the automated feeding system.
//!
//! This module provides a dedicated interface, `SqlInterfaceFeed`, for handling
//! all SQL operations related to the feeding schedule (`feed_schedule`), feed profiles
//! (`feed_profiles`), and feed event logging (`feed_log`).
//!
//! ## Key Components
//!
//! - **`SqlInterfaceFeed` Struct**: The primary struct that holds a database
//!   connection and provides methods for all feed-related database operations.
//!
//! - **`new()` Constructor**: A critical entry point that not only creates an
//!   `SqlInterfaceFeed` instance but also performs essential "fail-fast"
//!   validation. At initialization, it checks the database for:
//!   - The presence of `NULL` values in the `feed_profiles` and `feed_schedule` tables.
//!   - Whether the number of rows in the `feed_profiles`, `feed_schedule`, and `feed_log`
//!     tables exceeds the limits defined in the application configuration.
//!     This ensures the system doesn't start in an invalid or inconsistent state.
//!
//! - **Data-Fetching Methods**: Functions like `get_feed_profile_header_from_database`
//!   and `get_feed_phase_from_database` provide a structured way to retrieve
//!   complex feed profile data from the database.
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

use chrono::NaiveDateTime;
use mysql::prelude::*;
use mysql::*;

use crate::database::sql_query_strings::SQL_TABLE_FEED_PROFILES;
use crate::database::sql_test_query_strings::SQL_QUERY_READ_FUTURE_FEED_SCHEDULE_ENTRY;
use crate::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
    sql_test_query_strings,
};
use crate::food::feed_schedule_entry::FeedScheduleEntry;

#[derive(Clone)]
/// Holds the header information of a feed profile.
pub struct SqlFeedProfileHeader {
    pub profile_id: i32,
    pub profile_name: String,
}

#[derive(Clone)]
/// Holds the data of one feed profile phase as retrieved from the database.
/// The numeric data is post-processed to boolean in a further step.
pub struct SqlFeedPhase {
    /// duration of the pause before switching on the feeder motor in milliseconds
    pub pause_duration: i32,

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
    pub feed_duration: i32,

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
    pub timestamp: NaiveDateTime,
    pub profile_id: i32,
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
    /// * `max_rows_feed_profiles` - The maximum allowed rows in the `feed_profiles` table.
    /// * `max_rows_feed_schedule` - The maximum allowed rows in the `feed_schedule` table.
    /// * `max_rows_feed_log` - The maximum allowed rows in the `feedlog` table.
    ///
    /// # Returns
    /// A `Result` containing a new `SqlInterfaceFeed` instance on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - Any of the initial database queries to get table counts or check for NULLs fail (`DatabaseCheckFeedSetValsFailure`).
    /// - Any of the retrieved counts are negative, indicating a database issue (`DatabaseFeedTableNegativeValue`).
    /// - The `feed_profiles` or `feed_schedule` tables contain entries with `NULL` values (`DatabaseFeedSetValTableContainsNull`).
    /// - The number of rows in any of the checked tables exceeds its configured maximum (`DatabaseFeedTableContainsTooManyRows`). .
    pub fn new(
        mut conn: PooledConn,
        max_rows_feed_profiles: u64,
        max_rows_feed_schedule: u64,
        max_rows_feed_log: u64,
    ) -> Result<Self, SqlInterfaceError> {
        // check if feed profiles table is consistent
        let count_null_values_feed_profiles = SqlInterface::get_single_integer_from_database(
            &mut conn,
            sql_query_strings::SQL_QUERY_CHECK_FEED_PROFILES_NULL,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckFeedSetValsFailure {
            location: module_path!().to_string(),
            table_name: SQL_TABLE_FEED_PROFILES.to_string(),
            source: Box::new(e),
        })?;
        let count_rows_feed_profiles = SqlInterface::get_single_integer_from_database(
            &mut conn,
            sql_query_strings::SQL_QUERY_CHECK_FEED_PROFILES_COUNT,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckFeedSetValsFailure {
            location: module_path!().to_string(),
            table_name: SQL_TABLE_FEED_PROFILES.to_string(),
            source: Box::new(e),
        })?;

        // check if the feed schedule table is consistent
        let count_null_values_feed_schedule = SqlInterface::get_single_integer_from_database(
            &mut conn,
            sql_query_strings::SQL_QUERY_CHECK_FEED_SCHEDULE_NULL,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckFeedSetValsFailure {
            location: module_path!().to_string(),
            table_name: sql_query_strings::SQL_TABLE_FEED_SCHEDULE.to_string(),
            source: Box::new(e),
        })?;
        let count_rows_feed_schedule = SqlInterface::get_single_integer_from_database(
            &mut conn,
            sql_query_strings::SQL_QUERY_CHECK_FEED_SCHEDULE_COUNT,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckFeedSetValsFailure {
            location: module_path!().to_string(),
            table_name: SQL_TABLE_FEED_PROFILES.to_string(),
            source: Box::new(e),
        })?;

        let count_rows_feedlog = SqlInterface::get_single_integer_from_database(
            &mut conn,
            sql_query_strings::SQL_QUERY_CHECK_FEED_LOG_COUNT,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckFeedSetValsFailure {
            location: module_path!().to_string(),
            table_name: sql_test_query_strings::SQL_TABLE_FEED_LOG.to_string(),
            source: Box::new(e),
        })?;

        // check the query results
        if count_null_values_feed_profiles < 0 || count_rows_feed_profiles < 0 {
            return Err(SqlInterfaceError::DatabaseFeedTableNegativeValue(
                module_path!().to_string(),
                SQL_TABLE_FEED_PROFILES.to_string(),
                count_null_values_feed_profiles,
                count_rows_feed_profiles,
            ));
        }
        if count_null_values_feed_schedule < 0 || count_rows_feed_schedule < 0 {
            return Err(SqlInterfaceError::DatabaseFeedTableNegativeValue(
                module_path!().to_string(),
                sql_query_strings::SQL_TABLE_FEED_SCHEDULE.to_string(),
                count_null_values_feed_schedule,
                count_rows_feed_schedule,
            ));
        }
        if count_rows_feedlog < 0 {
            return Err(SqlInterfaceError::DatabaseFeedTableNegativeValue(
                module_path!().to_string(),
                sql_test_query_strings::SQL_TABLE_FEED_LOG.to_string(),
                -1,
                count_rows_feedlog,
            ));
        }

        // check if there are any NULL values in the feed set value tables
        if count_null_values_feed_profiles > 0 {
            return Err(SqlInterfaceError::DatabaseFeedSetValTableContainsNull(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_CHECK_FEED_PROFILES_NULL.to_string(),
                count_null_values_feed_profiles,
            ));
        }
        if count_null_values_feed_schedule > 0 {
            return Err(SqlInterfaceError::DatabaseFeedSetValTableContainsNull(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_CHECK_FEED_SCHEDULE_NULL.to_string(),
                count_null_values_feed_schedule,
            ));
        }

        if max_rows_feed_profiles > 0 {
            // execute the check only when the limit is greater than zero
            if count_rows_feed_profiles as u64 > max_rows_feed_profiles {
                return Err(SqlInterfaceError::DatabaseFeedTableContainsTooManyRows(
                    module_path!().to_string(),
                    SQL_TABLE_FEED_PROFILES.to_string(),
                    count_rows_feed_profiles as u64,
                    max_rows_feed_profiles,
                ));
            }
        }

        if max_rows_feed_schedule > 0 {
            // execute the check only when the limit is greater than zero
            if count_rows_feed_schedule as u64 > max_rows_feed_schedule {
                return Err(SqlInterfaceError::DatabaseFeedTableContainsTooManyRows(
                    module_path!().to_string(),
                    sql_query_strings::SQL_TABLE_FEED_SCHEDULE.to_string(),
                    count_rows_feed_schedule as u64,
                    max_rows_feed_schedule,
                ));
            }
        }

        if max_rows_feed_log > 0 {
            // execute the check only when the limit is greater than zero
            if count_rows_feedlog as u64 > max_rows_feed_log {
                return Err(SqlInterfaceError::DatabaseFeedTableContainsTooManyRows(
                    module_path!().to_string(),
                    sql_test_query_strings::SQL_TABLE_FEED_LOG.to_string(),
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
    pub(crate) fn get_feed_schedule_entries_from_database(
        &mut self,
        sql_query: &str,
    ) -> Result<Option<Vec<FeedScheduleEntry>>, SqlInterfaceError> {
        let sql_feed_schedule_entry_array =
            match self
                .conn
                .query_map(sql_query, |(timestamp, profile_id, repeat_daily)| {
                    SqlFeedScheduleEntry {
                        timestamp,
                        profile_id,
                        repeat_daily,
                    }
                }) {
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
        if sql_feed_schedule_entry_array.is_empty() {
            return Ok(None);
        }

        let feed_schedule_entries_result: Result<Vec<FeedScheduleEntry>, _> =
            sql_feed_schedule_entry_array
                .iter()
                .map(FeedScheduleEntry::new)
                .collect();

        // Handle conversion errors that may occur during the mapping.
        let feed_schedule_entries = feed_schedule_entries_result.map_err(|e| {
            SqlInterfaceError::FeedScheduleEntryTimeStampProcessingFailure {
                location: module_path!().to_string(),
                number_entries: sql_feed_schedule_entry_array.len(),
                source: Box::new(e),
            }
        })?;

        if feed_schedule_entries.is_empty() {
            Ok(None)
        } else {
            Ok(Some(feed_schedule_entries))
        }
    }

    /// Retrieves the single feed schedule entry that is furthest in the future.
    ///
    /// This private helper function is intended for test scenarios. It queries the database
    /// for future-scheduled feed entries and, assuming they are ordered by timestamp,
    /// returns the earliest (furthest in the future) one.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(Some(FeedScheduleEntry))` if a future feed schedule entry is found.
    /// - `Ok(None)` if no future feed schedule entries are found in the database.
    /// - `Err(SqlInterfaceError)` if an error occurs during the database query
    ///   or post-processing of the data.
    pub fn get_future_feed_schedule_entry_from_database(
        &mut self,
    ) -> Result<Option<FeedScheduleEntry>, SqlInterfaceError> {
        let entries_opt = self
            .get_feed_schedule_entries_from_database(SQL_QUERY_READ_FUTURE_FEED_SCHEDULE_ENTRY)?;
        Ok(entries_opt.and_then(|mut vec| vec.pop()))
    }

    /// Retrieves all feed schedule entries that are scheduled in the future.
    ///
    /// This private helper function is intended for test scenarios. It queries the database
    /// for all feed entries whose timestamps are in the future, then processes them
    /// into a vector of `FeedScheduleEntry` structs.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(Some(Vec<FeedScheduleEntry>))` if one or more future feed schedule entries are found.
    /// - `Ok(None)` if no future feed schedule entries are found in the database.
    /// - `Err(SqlInterfaceError)` if an error occurs during the database query
    ///   or post-processing of the data.
    pub fn get_future_feed_schedule_entries_from_database(
        &mut self,
    ) -> Result<Option<Vec<FeedScheduleEntry>>, SqlInterfaceError> {
        self.get_feed_schedule_entries_from_database(SQL_QUERY_READ_FUTURE_FEED_SCHEDULE_ENTRY)
    }

    /// Retrieves the header (basic information) of a specific feed profile from the database.
    ///
    /// This function queries the database using the provided `profile_id` to fetch
    /// the feed profile's unique ID and its associated name. It strictly expects
    /// exactly one matching header entry.
    ///
    /// # Arguments
    /// * `profile_id` - The unique identifier of the feed profile whose header is to be retrieved.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(SqlFeedProfileHeader)` if exactly one matching header entry is found.
    /// - `Err(SqlInterfaceError::SingleFeedProfileRequestFailure)` if the database query fails.
    /// - `Err(SqlInterfaceError::SingleFeedProfileRequestNoSingleResponse)` if the query
    ///   returns no entry or more than one entry for the given `profile_id`.
    pub fn get_feed_profile_header_from_database(
        &mut self,
        profile_id: i32,
    ) -> Result<SqlFeedProfileHeader, SqlInterfaceError> {
        // prepare the query for header
        let params = params! { "profile_id" => profile_id};

        let single_feed_profiles_entry_array = self
            .conn
            .exec_map(
                sql_query_strings::SQL_QUERY_READ_FEED_PROFILE_HEADER,
                params,
                |(profile_id, profile_name)| SqlFeedProfileHeader {
                    profile_id,
                    profile_name,
                },
            )
            .map_err(|e| SqlInterfaceError::SingleFeedProfileRequestFailure {
                location: module_path!().to_string(),
                query: sql_query_strings::SQL_QUERY_READ_FEED_PROFILE_HEADER.to_string(),
                profile_id,
                source: e,
            })?;

        // check the error case if there is no response
        if single_feed_profiles_entry_array.is_empty() {
            return Err(SqlInterfaceError::SingleFeedProfileRequestEmptyResponse(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_READ_FEED_PROFILE_HEADER.to_string(),
                profile_id,
            ));
        }

        // check the error case if there is more than one response
        if single_feed_profiles_entry_array.len() > 1 {
            return Err(SqlInterfaceError::SingleFeedProfileRequestNoSingleResponse(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_READ_FEED_PROFILE_HEADER.to_string(),
                profile_id,
            ));
        }

        let feed_profile_header = &single_feed_profiles_entry_array[0];

        Ok(feed_profile_header.clone())
    }

    /// Retrieves the specific data for a single phase of a feed profile from the database.
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
    /// - `Err(SqlInterfaceError::SingleFeedProfileRequestFailure)` if the database query fails.
    /// - `Err(SqlInterfaceError::SingleFeedProfileRequestNoSingleResponse)` if the query
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

        let single_feed_profile_entry_array = match self.conn.exec_map(
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
                return Err(SqlInterfaceError::SingleFeedProfileRequestFailure {
                    location: module_path!().to_string(),
                    query: sql_query,
                    profile_id,
                    source: e,
                });
            }
        };

        // check the error case if there is no response
        if single_feed_profile_entry_array.is_empty() {
            return Err(SqlInterfaceError::SingleFeedProfileRequestEmptyResponse(
                module_path!().to_string(),
                sql_query,
                profile_id,
            ));
        }

        // check the error case if there is more than one response
        if single_feed_profile_entry_array.len() > 1 {
            return Err(SqlInterfaceError::SingleFeedProfileRequestNoSingleResponse(
                module_path!().to_string(),
                sql_query,
                profile_id,
            ));
        }

        let feed_phase = &single_feed_profile_entry_array[0];

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

/// This helper function prepares the database for feed-related test cases.
/// It truncates the 'feed_profiles' table to ensure a clean state and then
/// inserts two predefined test feed profiles.
///
/// Arguments:
/// * `sql_interface`: A mutable reference to the general SQL interface, used for table truncation.
/// * `sql_interface_feed`: A mutable reference to the feed-specific SQL interface, used for inserting feed profiles.
pub fn insert_feed_profiles(
    sql_interface: &mut SqlInterface,
    sql_interface_feed: &mut SqlInterfaceFeed,
) {
    // remove all feed profiles
    match SqlInterface::truncate_table(sql_interface, SQL_TABLE_FEED_PROFILES.to_string()) {
        Ok(_) => {}
        Err(e) => panic!("Could not prepare test case: {e:?}"),
    }
    // insert test feed profile #1
    let sql_query_string = sql_test_query_strings::SQL_QUERY_WRITE_FEED_PROFILE_TEST.to_string();
    match sql_interface_feed.conn.exec_drop::<_, _>(
        sql_query_string,
        params! {
        "profile_id" => 1, "profile_name" => "test"
        },
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Error when inserting test feed profile #1 into database: {e:?}");
        }
    };
    // insert test feed profile #2
    let sql_query_string = sql_test_query_strings::SQL_QUERY_WRITE_FEED_PROFILE_TEST2.to_string();
    match sql_interface_feed.conn.exec_drop::<_, _>(
        sql_query_string,
        params! {
        "profile_id" => 2,
        },
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Error when inserting test feed profile #2 into database: {e:?}");
        }
    };
}
