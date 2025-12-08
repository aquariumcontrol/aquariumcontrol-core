/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the database interaction contract for the `feed` module.
//!
//! This module is central to decoupling the application's feeding logic from the
//! underlying database implementation. It achieves this by defining the
//! `DatabaseInterfaceFeedTrait`, which specifies all the database operations
//! that the `feed` module requires.
//!
//! ## Key Components
//!
//! - **`DatabaseInterfaceFeedTrait`**: An abstraction layer that defines a set of methods
//!   for querying and modifying feed-related data, such as feed schedules and
//!   historical feed events. By depending on this trait instead of a concrete
//!   database struct, the `feed` module can be tested in isolation using mock
//!   implementations.
//!
//! - **`impl DatabaseInterfaceFeedTrait for SqlInterfaceFeed`**: The production
//!   implementation of the trait. This block contains the actual SQL queries and
//!   logic required to interact with an SQL database, translating the high-level
//!   trait methods into concrete database operations.
//!
//! ## Design and Purpose
//!
//! The primary goal of this module is to enable **testability and modularity**.
//! The `feed` thread's logic is complex, and testing it against a live database
//! is slow, brittle, and requires significant setup. By using a trait object,
//! we can inject a mock database during testing.

#[cfg(test)]
use log::error;

use crate::database::pingable::Pingable;
use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_error::SqlInterfaceError;
use crate::database::sql_interface_feed::SqlInterfaceFeed;
use crate::database::sql_query_strings;
use crate::food::feed_profile::{FeedPhase, FeedProfile};
use crate::food::feed_schedule_entry::FeedScheduleEntry;
use chrono::NaiveDateTime;
use mysql::params;
use mysql::prelude::Queryable;

pub const NUM_FEEDPHASES: i32 = 10;

/// Trait for interfacing between feed and SQL database
/// This trait is prerequisite to use mock implementation for testing
pub trait DatabaseInterfaceFeedTrait: Pingable {
    fn get_current_timestamp(&mut self) -> mysql::Result<NaiveDateTime, SqlInterfaceError>;

    fn get_past_feed_schedule_entries_from_database(
        &mut self,
    ) -> mysql::Result<Option<Vec<FeedScheduleEntry>>, SqlInterfaceError>;

    fn insert_feed_event(
        &mut self,
        timestamp: NaiveDateTime,
        feeder_run_time: f64,
        profile_id: i32,
    ) -> mysql::Result<(), SqlInterfaceError>;

    fn get_single_feed_profile_from_database(
        &mut self,
        profile_id: i32,
    ) -> mysql::Result<FeedProfile, SqlInterfaceError>;

    fn update_feed_schedule_entries_in_database(
        &mut self,
        feedschedule_entries: &mut Vec<FeedScheduleEntry>,
    ) -> mysql::Result<(), SqlInterfaceError>;

    #[cfg(test)]
    fn insert_feed_schedule_entry(
        &mut self,
        timestamp: NaiveDateTime,
        profile_id: i32,
        is_weekly: bool,
        is_daily: bool,
    ) -> mysql::Result<(), SqlInterfaceError>;
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
    fn get_current_timestamp(&mut self) -> mysql::Result<NaiveDateTime, SqlInterfaceError> {
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
    fn get_past_feed_schedule_entries_from_database(
        &mut self,
    ) -> mysql::Result<Option<Vec<FeedScheduleEntry>>, SqlInterfaceError> {
        self.get_feed_schedule_entries_from_database(
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
    /// * `profile_id` - The unique identifier of the feed profile that was executed.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the feed event record was successfully inserted.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::InsertFeedEventFailure` if the `INSERT` query fails.
    /// This can happen due to a lost connection, constraint violation (e.g., foreign key),
    /// or incorrect data types. The original `mysql::Error` is included as the source.
    fn insert_feed_event(
        &mut self,
        timestamp: NaiveDateTime,
        feeder_run_time: f64,
        profile_id: i32,
    ) -> mysql::Result<(), SqlInterfaceError> {
        self.conn
            .exec_drop::<_, _>(
                sql_query_strings::SQL_QUERY_WRITE_FEED_EVENT.to_owned(),
                params! {
                "timestamp" => timestamp,
                "feeder_run_time" => feeder_run_time,
                "profile_id" => profile_id,
                },
            )
            .map_err(|e| SqlInterfaceError::InsertFeedEventFailure {
                location: module_path!().to_string(),
                source: e,
            })?;

        Ok(())
    }

    /// Retrieves a complete `Feedpattern` from the SQL database, including its header and all associated phases.
    ///
    /// This function first fetches the header information for the specified `profile_id`.
    /// It then retrieves data for each of the 10 expected feed phases, converting raw
    /// database flags into booleans.
    ///
    /// # Arguments
    /// * `profile_id` - The unique identifier of the feed profile to retrieve.
    ///
    /// # Returns
    /// A `Result` containing a fully constructed `Feedpattern` object on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The query for the feed pattern header fails, the header is not found, or multiple headers are found.
    /// - The query for any of the 10 feed phases fails, a phase is not found, or multiple phases are found.
    /// - A retrieved feed phase contains an invalid (negative) duration value.
    fn get_single_feed_profile_from_database(
        &mut self,
        profile_id: i32,
    ) -> mysql::Result<FeedProfile, SqlInterfaceError> {
        let sql_feedpattern_header = self.get_feed_profile_header_from_database(profile_id)?;

        let mut feed_phases = Vec::new();
        for nr in 1..=NUM_FEEDPHASES {
            let sql_feed_phase = self.get_feed_phase_from_database(profile_id, nr)?;

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
        Ok(FeedProfile {
            profile_id: sql_feedpattern_header.profile_id,
            profile_name: sql_feedpattern_header.profile_name,
            feedphases: feed_phases,
        })
    }

    /// Updates or deletes multiple feed schedule entries in the database.
    ///
    /// This function iterates through a list of `FeedScheduleEntry` structs. For each entry:
    /// - If `repeat_daily` is `true`, it updates the entry's timestamp in the database to tomorrow's date at the same time.
    /// - If `repeat_daily` is `false`, it deletes the entry from the database.
    ///
    /// The entire operation is transactional in nature: it will stop and return an error on the first failure.
    ///
    /// # Arguments
    /// * `feedschedule_entries` - A mutable reference to a vector of `FeedScheduleEntry` structs to be processed.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if all update and delete operations were successful.
    ///
    /// # Errors
    /// This function will return an error if an operation fails. This can be:
    /// - `FeedScheduleUpdateFailure`: If fetching tomorrow's date fails.
    /// - `FeedScheduleDropFailure`: If a database `UPDATE` or `DELETE` command fails. This can be due to a lost connection or other database-side issues.
    fn update_feed_schedule_entries_in_database(
        &mut self,
        feedschedule_entries: &mut Vec<FeedScheduleEntry>,
    ) -> mysql::Result<(), SqlInterfaceError> {
        let mut result = Ok(()); // an initial value - will be overwritten by any error occurred
        for feedschedule_entry in feedschedule_entries {
            if feedschedule_entry.repeat_daily {
                // the feed schedule shall be repeated daily, so update the entry with
                // - tomorrow's date
                // - the same time of day as the entry
                let sql_query_string =
                    sql_query_strings::SQL_QUERY_UPDATE_FEED_SCHEDULE_ENTRY.to_owned();

                let timestamp_tomorrow_date_only =
                    match SqlInterface::get_tomorrow_date(&mut self.conn) {
                        Ok(c) => c,
                        Err(e) => {
                            result = Err(SqlInterfaceError::FeedScheduleUpdateFailure {
                                location: module_path!().to_string(),
                                source: Box::new(e),
                            });
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
                        "timestamp_new" => timestamp_tomorrow,
                        "timestamp_old" => feedschedule_entry.timestamp,
                    },
                ) {
                    Ok(_) => { /* do nothing */ }
                    Err(e) => {
                        result = Err(SqlInterfaceError::FeedScheduleDropFailure {
                            location: module_path!().to_string(),
                            source: e,
                        });
                    }
                };
            } else {
                // feed schedule shall not be repeated daily, delete the entry from the database
                let sql_query_string =
                    sql_query_strings::SQL_QUERY_DELETE_FEED_SCHEDULE_ENTRY.to_owned();
                match self.conn.exec_drop::<_, _>(
                    sql_query_string,
                    params! {
                        "timestamp" => feedschedule_entry.timestamp,
                    },
                ) {
                    Ok(_) => { /* do nothing */ }
                    Err(e) => {
                        result = Err(SqlInterfaceError::FeedScheduleDropFailure {
                            location: module_path!().to_string(),
                            source: e,
                        });
                    }
                };
            }
        }
        result
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
    // An empty `Result` (`Ok(())`) if operation was successful.
    //
    // # Errors
    // This function will return an `InsertFeedEventFailure` if the database operation fails.
    #[cfg(test)]
    fn insert_feed_schedule_entry(
        &mut self,
        timestamp: NaiveDateTime,
        profile_id: i32,
        is_weekly: bool,
        is_daily: bool,
    ) -> mysql::Result<(), SqlInterfaceError> {
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY.to_owned();
        match self.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "timestamp" => timestamp,
            "profile_id" => profile_id,
            "is_weekly" => is_weekly,
            "is_daily" => is_daily,
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                error!( // logging for testing purposes only
                    target: module_path!(),
                    "could not insert feed schedule entry ({e:?})"
                );
                Err(SqlInterfaceError::InsertFeedEventFailure {
                    location: module_path!().to_string(),
                    source: e,
                })
            }
        }
    }
}
