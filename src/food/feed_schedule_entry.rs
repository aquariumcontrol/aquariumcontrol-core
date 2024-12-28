/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use chrono::NaiveDateTime;
use log::error;
use mysql::*;
use std::fmt;

use crate::database::{
    sql_interface_feed::SqlFeedScheduleEntry, sql_interface_error::SqlInterfaceError,
};

const LOG_TARGET_FEED_SCHEDULE_ENTRY: &str = "feed_schedule_entry";

/// Holds the data of a feed schedule entry derived from data read from SQL data base.
#[derive(Debug, PartialEq, Clone)]
pub struct FeedScheduleEntry {
    pub timestamp: NaiveDateTime,
    pub profile_id: i32,
    pub profile_name: String,
    pub repeat_daily: bool,
}

impl FeedScheduleEntry {
    pub fn new(
        sql_feed_schedule_entry: &SqlFeedScheduleEntry,
    ) -> Result<FeedScheduleEntry, SqlInterfaceError> {
        let timestamp = match NaiveDateTime::parse_from_str(
            sql_feed_schedule_entry.timestamp.as_str(),
            "%Y-%m-%d %H:%M:%S",
        ) {
            Ok(c) => c,
            Err(e) => {
                error!(
                    target: LOG_TARGET_FEED_SCHEDULE_ENTRY,
                    "could not get feed schedule entry from database ({e:?})"
                );
                return Err(
                    SqlInterfaceError::FeedScheduleEntryTimeStampConversionFailure(
                        "Could not convert timestamp string read from SQL database".to_string(),
                    ),
                );
            }
        };

        Ok(FeedScheduleEntry {
            timestamp: timestamp,
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
            profile_id: profile_id,
            profile_name: profile_name.to_string(),
            repeat_daily: repeat_daily,
        }
    }
}

impl fmt::Display for FeedScheduleEntry {
    /// Function provides output of the struct for development purposes
    /// # Arguments
    /// * `f` - configuration for formatting
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "timestamp={}, profile_id={}, profile_name={}, repeat_daily={}",
            self.timestamp, self.profile_id, self.profile_name, self.repeat_daily,
        )
    }
}
