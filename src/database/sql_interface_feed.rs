/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::NaiveDateTime;
use log::error;
use mysql::prelude::*;
use mysql::*;

use crate::database::sql_query_strings::{
    SQL_QUERY_CHECK_FEEDPATTERNS_NULL, SQL_QUERY_CHECK_FEEDSCHEDULE_NULL,
};
use crate::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
};
use crate::food::feed_pattern::{FeedPhase, Feedpattern};
use crate::food::feed_schedule_entry::FeedScheduleEntry;

const LOG_TARGET_SQL_INTERFACE_FEED: &str = "sql_interface_feed";

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

/// Trait for interfacing between feed and SQL database
/// This trait is prerequisite to use mock implementation for testing
pub trait DatabaseInterfaceFeedTrait {
    fn get_current_timestamp(&mut self) -> Result<NaiveDateTime, SqlInterfaceError>;

    fn get_past_feedschedule_entries_from_database(
        &mut self,
    ) -> Result<Option<Vec<FeedScheduleEntry>>, SqlInterfaceError>;

    fn insert_feed_event(
        &mut self,
        timestamp: NaiveDateTime,
        feeder_run_time: f64,
        profile_name: String,
        profile_id: i32,
    ) -> Result<(), SqlInterfaceError>;

    fn get_single_feedpattern_from_database(
        &mut self,
        profile_id: i32,
    ) -> Result<Feedpattern, SqlInterfaceError>;

    fn update_feedschedule_entries_in_database(
        &mut self,
        feedschedule_entries: &mut Vec<FeedScheduleEntry>,
    ) -> Result<(), SqlInterfaceError>;
    /// Execute a ping to the database to avoid timeout.
    fn ping(&mut self);

    #[cfg(test)]
    fn insert_feed_schedule_entry(
        &mut self,
        timestamp: NaiveDateTime,
        profile_id: i32,
        profile_name: String,
        is_weekly: bool,
        is_daily: bool,
    ) -> Result<(), SqlInterfaceError>;
}

/// Contains the configuration and the implementation of the SQL interface for Feed.
pub struct SqlInterfaceFeed {
    /// connection to SQL database
    conn: PooledConn,
}

impl DatabaseInterfaceFeedTrait for SqlInterfaceFeed {
    /// Retrieves the current timestamp from the database using the internal connection.
    ///
    /// This function acts as a proxy, calling the general `SqlInterface::get_current_timestamp`
    /// method with the `SqlInterfaceFeed`'s established database connection.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(NaiveDateTime)`: The current timestamp as reported by the database.
    /// - `Err(SqlInterfaceError)`: If any error occurs during the retrieval or parsing
    ///   of the timestamp from the database.
    fn get_current_timestamp(&mut self) -> Result<NaiveDateTime, SqlInterfaceError> {
        SqlInterface::get_current_timestamp(&mut self.conn)
    }

    /// Retrieves feed schedule entries from the database that are scheduled in the past.
    ///
    /// This function queries the database for all feed schedule entries whose timestamps
    /// are before the current time. The raw database entries are then post-processed
    /// into `FeedScheduleEntry` structs.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(Some(Vec<FeedScheduleEntry>))` if one or more past feed schedule entries are found.
    /// - `Ok(None)` if no past feed schedule entries are found in the database.
    /// - `Err(SqlInterfaceError)` if any error occurs during the database query
    ///   or during the conversion of raw database data into `FeedScheduleEntry` structs.
    fn get_past_feedschedule_entries_from_database(
        &mut self,
    ) -> Result<Option<Vec<FeedScheduleEntry>>, SqlInterfaceError> {
        self.get_feedschedule_entries_from_database(
            sql_query_strings::SQL_QUERY_READ_PAST_FEED_SCHEDULE_ENTRY,
        )
    }

    /// Inserts a new feed event record into the database.
    ///
    /// This function logs details about a completed feed operation, including the
    /// timestamp, how long the feeder ran, and which feed profile was used.
    ///
    /// # Arguments
    /// * `timestamp` - The `NaiveDateTime` when the feed event occurred.
    /// * `feeder_run_time` - The duration (in seconds) the feeder motor was active.
    /// * `profile_name` - The name of the feed profile that was executed for this event.
    /// * `profile_id` - The unique identifier of the feed profile that was executed.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(())` if the feed event record was successfully inserted.
    /// - `Err(SqlInterfaceError::InsertFeedEventFailure)` if an error occurs
    ///   during the database insertion.
    fn insert_feed_event(
        &mut self,
        timestamp: NaiveDateTime,
        feeder_run_time: f64,
        profile_name: String,
        profile_id: i32,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_FEED_EVENT.to_owned();
        match self.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "timestamp" => timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "feeder_run_time" => feeder_run_time,
            "profile_name" => profile_name,
            "profile_id" => profile_id,
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                error!(
                    target: LOG_TARGET_SQL_INTERFACE_FEED,
                    "could not insert feed event into database ({e:?})"
                );
                Err(SqlInterfaceError::InsertFeedEventFailure)
            }
        }
    }

    /// Retrieves a complete `Feedpattern` from the SQL database, including its header and all associated phases.
    ///
    /// This function first fetches the basic header information for the specified feed profile ID.
    /// It then proceeds to retrieve data for each of the 10 expected feed phases (from 1 to 10),
    /// converting the raw numeric flags from the database into boolean values for the `FeedPhase` structs.
    ///
    /// # Arguments
    /// * `profile_id` - The unique identifier of the feed profile to retrieve.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(Feedpattern)`: A fully constructed `Feedpattern` object containing the
    ///   header and all its phases if successful.
    /// - `Err(SqlInterfaceError)`: If an error occurs during the retrieval of the
    ///   feed pattern header or any of its phases from the database. This includes
    ///   cases where the profile is not found or database query issues.
    fn get_single_feedpattern_from_database(
        &mut self,
        profile_id: i32,
    ) -> Result<Feedpattern, SqlInterfaceError> {
        let sql_feedpattern_header = match self.get_feedpattern_header_from_database(profile_id) {
            Ok(c) => c,
            Err(e) => {
                #[cfg(not(test))]
                error!(
                    target: LOG_TARGET_SQL_INTERFACE_FEED,
                    "could not retrieve feed pattern from the database ({e:?})"
                );
                return Err(e);
            }
        };
        let mut feed_phases = Vec::new();
        for nr in 1..11 {
            let sql_feed_phase = match self.get_feed_phase_from_database(profile_id, nr) {
                Ok(c) => c,
                Err(e) => {
                    return Err(e);
                }
            };
            let feed_phase = FeedPhase {
                pause_duration: sql_feed_phase.pause_duration,
                pause_skimmer: sql_feed_phase.pause_skimmer > 0,
                pause_main_pump_1: sql_feed_phase.pause_main_pump_1 > 0,
                pause_main_pump_2: sql_feed_phase.pause_main_pump_2 > 0,
                pause_aux_pump_1: sql_feed_phase.pause_aux_pump_1 > 0,
                pause_aux_pump_2: sql_feed_phase.pause_aux_pump_2 > 0,
                feed_duration: sql_feed_phase.feed_duration,
                feed_skimmer: sql_feed_phase.feed_skimmer > 0,
                feed_main_pump_1: sql_feed_phase.feed_main_pump_1 > 0,
                feed_main_pump_2: sql_feed_phase.feed_main_pump_2 > 0,
                feed_aux_pump_1: sql_feed_phase.feed_aux_pump_1 > 0,
                feed_aux_pump_2: sql_feed_phase.feed_aux_pump_2 > 0,
            };
            feed_phases.push(feed_phase);
        }
        Ok(Feedpattern {
            profile_id: sql_feedpattern_header.profile_id,
            profile_name: sql_feedpattern_header.profile_name,
            feedphases: feed_phases,
        })
    }

    /// Updates or deletes multiple feed schedule entries in the database based on their repetition setting.
    ///
    /// This function iterates through a provided list of `FeedScheduleEntry` structs.
    /// For each entry:
    /// - If `repeat_daily` is `true`, it updates the entry's timestamp in the database to tomorrow's date
    ///   while preserving the original time of day.
    /// - If `repeat_daily` is `false`, it deletes the entry from the database.
    ///
    /// The function continues processing all entries even if an individual update or deletion fails,
    /// but it will return the last encountered error.
    ///
    /// # Arguments
    /// * `feedschedule_entries` - A mutable reference to a vector of `FeedScheduleEntry` structs
    ///   to be processed. The timestamps of repeated entries will be updated.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(())` if all operations (updates and deletions) were successful.
    /// - `Err(SqlInterfaceError::FeedScheduleUpdateFailure)` if any update or deletion
    ///   operation fails for an entry. The function will return the last error encountered.
    fn update_feedschedule_entries_in_database(
        &mut self,
        feedschedule_entries: &mut Vec<FeedScheduleEntry>,
    ) -> Result<(), SqlInterfaceError> {
        let mut result = Ok(()); // initial value - will be overwritten by any error occurred
        for feedschedule_entry in feedschedule_entries {
            if feedschedule_entry.repeat_daily {
                // the feed schedule shall be repeated daily, so update the entry with
                // - tomorrow's date
                // - the same time of day as the entry
                let sql_query_string =
                    sql_query_strings::SQL_QUERY_UPDATE_FEED_SCHEDULE_ENTRY.to_owned();

                let timestamp_tomorrow_date_only = match SqlInterface::get_tomorrow_date(
                    &mut self.conn,
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_SQL_INTERFACE_FEED,
                            "could not calculate update of feed schedule entry ({e:?})"
                        );
                        result = Err(SqlInterfaceError::FeedScheduleUpdateFailure);
                        continue;
                    }
                };

                let timestamp_tomorrow = NaiveDateTime::new(
                    timestamp_tomorrow_date_only,
                    feedschedule_entry.timestamp.time(),
                );
                match self.conn.exec_drop::<_, _>(
                    sql_query_string,
                    params! {
                        "timestamp_new" => timestamp_tomorrow.format("%Y-%m-%d %H:%M:%S").to_string(),
                        "timestamp_old" => feedschedule_entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
                    },
                ) {
                    Ok(_) => { /* do nothing */ },
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_SQL_INTERFACE_FEED,
                            "could not update feed schedule entry ({e:?})"
                        );
                        result = Err(SqlInterfaceError::FeedScheduleDropFailure);
                    }
                };
            } else {
                // feed schedule shall not be repeated daily, delete the entry from the database
                let sql_query_string =
                    sql_query_strings::SQL_QUERY_DELETE_FEED_SCHEDULE_ENTRY.to_owned();
                match self.conn.exec_drop::<_, _>(
                    sql_query_string,
                    params! {
                        "timestamp" => feedschedule_entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
                    },
                ) {
                    Ok(_) => { /* do nothing */ },
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_SQL_INTERFACE_FEED,
                            "could not delete feed schedule entry ({e:?})"
                        );
                        result = Err(SqlInterfaceError::FeedScheduleDropFailure);
                    }
                };
            }
        }
        result
    }

    /// Execute a ping to the database to avoid timeout.
    fn ping(&mut self) {
        SqlInterface::ping_database(&mut self.conn, LOG_TARGET_SQL_INTERFACE_FEED)
    }

    #[cfg(test)]
    // Inserts a single feed schedule entry into the database.
    //
    // This helper function is exclusively used in test environments to
    // programmatically set up specific feed schedule configurations.
    //
    // # Arguments
    // * `timestamp` - The `NaiveDateTime` when the feed event is scheduled to occur.
    // * `profile_id` - The unique identifier of the feed profile associated with this schedule.
    // * `profile_name` - The name of the feed profile associated with this schedule.
    // * `is_weekly` - A boolean flag indicating if this schedule entry should repeat weekly.
    // * `is_daily` - A boolean flag indicating if this schedule entry should repeat daily.
    //
    // # Returns
    // A `Result` which is:
    // - `Ok(())` if the feed schedule entry was successfully inserted.
    // - `Err(SqlInterfaceError::InsertFeedEventFailure)` if an error occurs during
    //   the database insertion.
    #[cfg(test)]
    fn insert_feed_schedule_entry(
        &mut self,
        timestamp: NaiveDateTime,
        profile_id: i32,
        profile_name: String,
        is_weekly: bool,
        is_daily: bool,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY.to_owned();
        match self.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "timestamp" => timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "profile_id" => profile_id,
            "profile_name" => profile_name,
            "is_weekly" => is_weekly,
            "is_daily" => is_daily,
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                error!(
                    target: LOG_TARGET_SQL_INTERFACE_FEED,
                    "could not insert feed schedule entry ({e:?})"
                );
                Err(SqlInterfaceError::InsertFeedEventFailure)
            }
        }
    }
}

impl SqlInterfaceFeed {
    /// Creates a new `SqlInterfaceFeed` instance.
    ///
    /// This constructor initializes the feed management SQL interface
    /// with an established database connection. Before, it checks whether the feed schedule table
    /// or the feed pattern table contain any NULL values. If so, it panics, as this indicates a
    /// data consistency issue.
    ///
    /// # Arguments
    /// * `conn` - An active database connection pool object.
    ///
    /// # Returns
    /// A new `SqlInterfaceFeed` struct.
    pub fn new(mut conn: PooledConn) -> SqlInterfaceFeed {
        if SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_FEEDPATTERNS_NULL,
        )
        .expect("Could not check feed pattern table for NULL values.")
            > 0
        {
            panic!(
                "{}: Feed pattern table may be inconsistent - it contains NULL values.",
                LOG_TARGET_SQL_INTERFACE_FEED
            );
        }
        if SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_FEEDSCHEDULE_NULL,
        )
        .expect("Could not check feed schedule table for NULL values.")
            > 0
        {
            panic!(
                "{}: Feed schedule table may be inconsistent - it contains NULL values.",
                LOG_TARGET_SQL_INTERFACE_FEED
            );
        }

        SqlInterfaceFeed { conn }
    }

    /// Retrieves and post-processes feed schedule entries from the database based on a given SQL query.
    ///
    /// This private helper function executes the provided `sql_query` to fetch raw schedule data.
    /// It then attempts to convert these raw `SqlScheduleEntry` records into fully typed
    /// `FeedScheduleEntry` structs. It handles cases where no records are found or where
    /// conversion of timestamps fails.
    ///
    /// # Arguments
    /// * `sql_query` - The SQL query string used to retrieve feed schedule entries.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(Some(Vec<FeedScheduleEntry>))` if one or more schedule entries are successfully retrieved and converted.
    /// - `Ok(None)` if the query returns an empty result set.
    /// - `Err(SqlInterfaceError::FeedScheduleEntryRequestFailure)` if the database query fails.
    /// - `Err(SqlInterfaceError::FeedScheduleEntryTimeStampConversionFailure)` if the query
    ///   returns results, but none of the timestamp strings could be successfully parsed
    ///   into `NaiveDateTime`.
    fn get_feedschedule_entries_from_database(
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
            Err(_) => {
                return Err(SqlInterfaceError::FeedScheduleEntryRequestFailure(sql_query.to_string()));
            }
        };

        // return none if the query result is empty
        if sql_feedschedule_entry_array.is_empty() {
            return Ok(None);
        }

        let mut feedschedule_entries: Vec<FeedScheduleEntry> = Vec::new();

        for sql_feedschedule_entry in sql_feedschedule_entry_array {
            match FeedScheduleEntry::new(&sql_feedschedule_entry) {
                Ok(c) => {
                    feedschedule_entries.push(c);
                }
                Err(_) => { /* do nothing */ }
            }
        }

        if feedschedule_entries.is_empty() {
            Err(SqlInterfaceError::FeedScheduleEntryTimeStampConversionFailure)
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
        let result = self.get_feedschedule_entries_from_database(
            sql_query_strings::SQL_QUERY_READ_FUTURE_FEED_SCHEDULE_ENTRY,
        );
        match result {
            Ok(vector_or_none) => match vector_or_none {
                Some(vector) => Ok(Some(vector[0].clone())),
                None => Ok(None),
            },
            Err(e) => Err(e),
        }
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
        let sql_query = str::replace(
            sql_query_strings::SQL_QUERY_READ_FEEDPATTERN_HEADER,
            "profile_id",
            &profile_id.to_string(),
        );
        let single_feedpattern_entry_array =
            match self
                .conn
                .query_map(sql_query.clone(), |(profile_id, profile_name)| {
                    SqlFeedpatternHeader {
                        profile_id,
                        profile_name,
                    }
                }) {
                Ok(c) => c,
                Err(_) => {
                    return Err(SqlInterfaceError::SingleFeedpatternRequestFailure(sql_query.as_str().to_string()));
                }
            };

        // check the error case if there is no response
        if single_feedpattern_entry_array.is_empty() {
            return Err(SqlInterfaceError::SingleFeedpatternRequestEmptyResponse(sql_query));
        }
        
        // check the error case if there is more than one response
        if single_feedpattern_entry_array.len() > 1 {
            return Err(SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse(sql_query));
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
        // prepare the query header
        let sql_query_pre = str::replace(
            sql_query_strings::SQL_QUERY_READ_FEED_PHASE,
            "profile_id",
            &profile_id.to_string(),
        );
        let sql_query = str::replace(&sql_query_pre, "#", format!("{:02}", phase_nr).as_str());

        let single_feedpattern_entry_array = match self.conn.query_map(
            sql_query.clone(),
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
            Err(_) => {
                return Err(SqlInterfaceError::SingleFeedpatternRequestFailure(sql_query));
            }
        };

        // check the error case if there is no response
        if single_feedpattern_entry_array.is_empty() {
            return Err(SqlInterfaceError::SingleFeedpatternRequestEmptyResponse(sql_query));
        }
        
        // check the error case if there is more than one response
        if single_feedpattern_entry_array.len() > 1 {
            return Err(SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse(sql_query));
        }
        
        let feed_phase = &single_feedpattern_entry_array[0];

        Ok(feed_phase.clone())
    }
}

#[cfg(test)]
pub mod tests {
    use chrono::{Duration, Local, NaiveDate, NaiveDateTime};
    use mysql::{params, prelude::Queryable};

    use crate::database::sql_interface_feed::{
        DatabaseInterfaceFeedTrait, FeedScheduleEntry, SqlInterfaceFeed,
    };
    use crate::database::sql_query_strings;
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
        match SqlInterface::truncate_table(sql_interface, "feedpatterns".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        // insert test feed profile #1
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_FEEDPATTERN_TEST1.to_string();
        match sql_interface_feed.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "profile_id" => 1,
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
    // Test case includes all checks for this table combined in one function to limit the number of additional databases.
    // Test case uses test database #33.
    pub fn test_sql_interface_feedpattern() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            33,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_feed = SqlInterfaceFeed::new(sql_interface.get_connection());

        //*** check retrieving of the feed pattern from empty table - FAIL **************************
        match SqlInterface::truncate_table(&mut sql_interface, "feedpatterns".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        match sql_interface_feed.get_single_feedpattern_from_database(1) {
            Ok(_) => {
                panic!("Call to get_single_feedpattern_from_database should have failed.");
            }
            Err(e) => {
                assert_eq!(
                    e,
                    SqlInterfaceError::SingleFeedpatternRequestEmptyResponse(
                        "SELECT ProfileID, ProfileName FROM feedpatterns WHERE ProfileID=1;"
                            .to_string()
                    )
                );
            }
        }
        println!("* checking retrieving of Feed pattern (FAIL) succeeded.");
        // **************************************************************************************

        // *** check retrieving of existing feed pattern - FAIL *********************************
        // prepare database: insert feed profile
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_FEEDPATTERN_TEST1.to_string();
        match sql_interface_feed.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "profile_id" => 1,
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
        println!("* checking retrieving of feed pattern (PASS) succeeded.");
        // **************************************************************************************
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
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_feed = SqlInterfaceFeed::new(sql_interface.get_connection());
        //*** check inserting of feed event into empty table - FAIL ****************************
        match SqlInterface::truncate_table(&mut sql_interface, "feedlog".to_string()) {
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
        println!("* checking inserting of feed event into log (PASS) succeeded.");
        // **************************************************************************************
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
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_feed = SqlInterfaceFeed::new(sql_interface.get_connection());
        // *** check trying to load feed schedule entry when table is empty *********************
        match SqlInterface::truncate_table(&mut sql_interface, "feedschedule".to_string()) {
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
        println!("* reading existing entry from feed schedule (PASS) succeeded.");
        // **************************************************************************************

        // *** check updating feed schedule with repetition ***************************************
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
        println!("* update existing entry from feed schedule (PASS) succeeded.");
        // **************************************************************************************

        // *** test updating feed schedule without repetition ************************************
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
        println!("* deleting existing entry from feed schedule (PASS) succeeded.");
        // **************************************************************************************

        // *** check updating multiple feed schedule entries with repetition **********************
        // calculate dynamic reference
        match SqlInterface::truncate_table(&mut sql_interface, "feedschedule".to_string()) {
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
        println!("* update of existing entries from feed schedule (PASS) succeeded.");
        // **************************************************************************************
    }
}
