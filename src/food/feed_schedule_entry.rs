/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use chrono::NaiveDateTime;
use mysql::*;
use std::fmt;

use crate::database::{
    sql_interface_error::SqlInterfaceError, sql_interface_feed::SqlFeedScheduleEntry,
};

/// Holds the data of a feed schedule entry derived from data read from the database.
#[derive(Debug, PartialEq, Clone)]
pub struct FeedScheduleEntry {
    pub timestamp: NaiveDateTime,
    pub profile_id: i32,
    pub profile_name: String,
    pub repeat_daily: bool,
}

impl FeedScheduleEntry {
    /// Creates a new `FeedScheduleEntry` struct by converting raw data from `SqlFeedScheduleEntry`.
    ///
    /// This constructor takes a raw database entry (`SqlFeedScheduleEntry`) and
    /// transforms its string-based timestamp into a `NaiveDateTime` and its
    /// numeric `repeat_daily` flag into a boolean.
    ///
    /// # Arguments
    /// * `sql_feed_schedule_entry` - A reference to a `SqlFeedScheduleEntry` struct,
    ///   containing data typically read directly from the SQL database.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(FeedScheduleEntry)`: A new `FeedScheduleEntry` struct with all data successfully converted.
    /// - `Err(SqlInterfaceError::FeedScheduleEntryTimeStampConversionFailure)`: If the
    ///   `timestamp` string within `sql_feed_schedule_entry` cannot be parsed into a
    ///   valid `NaiveDateTime`.
    pub fn new(
        sql_feed_schedule_entry: &SqlFeedScheduleEntry,
    ) -> Result<FeedScheduleEntry, SqlInterfaceError> {
        let timestamp = NaiveDateTime::parse_from_str(
            sql_feed_schedule_entry.timestamp.as_str(),
            "%Y-%m-%d %H:%M:%S",
        )
        .map_err(
            |e| SqlInterfaceError::FeedScheduleEntryTimeStampConversionFailure { source: e },
        )?;

        Ok(FeedScheduleEntry {
            timestamp,
            profile_id: sql_feed_schedule_entry.profile_id,
            profile_name: sql_feed_schedule_entry.profile_name.clone(),
            repeat_daily: sql_feed_schedule_entry.repeat_daily != 0,
        })
    }

    #[cfg(test)]
    pub fn new_for_test(
        timestamp_string: &str,
        profile_id: i32,
        profile_name: &str,
        repeat_daily: bool,
    ) -> FeedScheduleEntry {
        FeedScheduleEntry {
            timestamp: NaiveDateTime::parse_from_str(timestamp_string, "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            profile_id,
            profile_name: profile_name.to_string(),
            repeat_daily,
        }
    }
}

impl fmt::Display for FeedScheduleEntry {
    /// Formats the `FeedScheduleEntry` struct into a concise, human-readable string.
    ///
    /// This implementation is designed for development and debugging purposes,
    /// providing a compact representation of the schedule entry's key details
    /// (timestamp, profile ID, profile name, and daily repetition status) on a single line.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to the formatter, as required by the `fmt::Display` trait.
    ///
    /// # Returns
    /// A `fmt::Result` indicating whether the formatting operation was successful.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "timestamp={}, profile_id={}, profile_name={}, repeat_daily={}",
            self.timestamp, self.profile_id, self.profile_name, self.repeat_daily,
        )
    }
}
