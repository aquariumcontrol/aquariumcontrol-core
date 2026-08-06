/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the application-level representation of a single feed schedule entry.
//!
//! This module provides the `FeedScheduleEntry` struct, which is a clean, type-safe
//! representation of a scheduled feeding event. Its primary role is to act as a
//! "hydrated" data model, converting raw data retrieved from the database into a
//! format that the application's business logic can safely and easily work with.
//!
//! ## Key Components
//!
//! - **`FeedScheduleEntry` Struct**: The core data structure holding a `NaiveDateTime`,
//!   the associated feed profile ID and name, and a boolean `repeat_daily` flag.
//!
//! - **`new()` Constructor**: A fallible constructor that takes a raw `SqlFeedScheduleEntry`
//!   (as defined in `sql_interface_feed`) and performs the critical conversion of
//!   the string-based timestamp into a `chrono::NaiveDateTime`. It also converts the
//!   numeric `repeat_daily` flag into a proper boolean.
//!
//! - **`fmt::Display` Implementation**: Provides a simple, human-readable string format
//!   for the struct, which is useful for logging and debugging purposes.
//!
//! ## Design and Architecture
//!
//! This module serves as a crucial boundary between the database layer and the
//! application's core logic.
//!
//! - **Data Hydration and Type Safety**: The main purpose of this struct is to "hydrate"
//!   data from the database. By parsing the timestamp string into a `NaiveDateTime`
//!   at the earliest opportunity, the rest of the application is protected from
//!   handling raw strings and can leverage the powerful, type-safe operations
//!   provided by the `chrono` crate. This prevents a whole class of potential bugs
//!   related to malformed date strings.
//!
//! - **Robust Error Handling**: The `new()` constructor returns a `Result`, explicitly
//!   handling the possibility that the timestamp data in the database might be
//!   corrupt or in an unexpected format. This forces the calling code to deal with
//!   potential data integrity issues gracefully.
//!
//! - **Testability**: The inclusion of a `#[cfg(test)]` constructor, `new_for_test`,
//!   allows unit tests to create instances of `FeedScheduleEntry` easily without needing
//!   to construct a raw `SqlFeedScheduleEntry` first.

use chrono::NaiveDateTime;
use mysql::*;
use std::fmt;

use crate::database::sql_interface_error::SqlInterfaceError::FeedScheduleEntryRepeatDailyOutOfRange;
use crate::database::{
    sql_interface_error::SqlInterfaceError, sql_interface_feed::SqlFeedScheduleEntry,
};

/// Holds the data of a feed schedule entry derived from data read from the database.
#[derive(Debug, PartialEq, Clone)]
pub struct FeedScheduleEntry {
    pub timestamp: NaiveDateTime,
    pub profile_id: i32,
    pub repeat_daily: bool,
}

impl FeedScheduleEntry {
    /// Creates a new `FeedScheduleEntry` by converting raw data from an `SqlFeedScheduleEntry`.
    ///
    /// This constructor takes a raw database entry (`SqlFeedScheduleEntry`) and
    /// transforms it to `FeedScheduleEntry`. This is a critical step in
    /// hydrating application-level structs from database records.
    ///
    /// # Arguments
    /// * `sql_feed_schedule_entry` - A reference to a `SqlFeedScheduleEntry` struct,
    ///   containing data typically read directly from the SQL database.
    ///
    /// # Returns
    /// A `Result` containing a new, fully typed `FeedScheduleEntry` on success.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::FeedScheduleEntryRepeatDailyOutOfRange` if
    /// `repeat_daily` is not either zero or one.
    ///
    /// [impl->swreq~food_read_schedule~1]
    pub fn new(
        sql_feed_schedule_entry: &SqlFeedScheduleEntry,
    ) -> Result<FeedScheduleEntry, SqlInterfaceError> {
        if (sql_feed_schedule_entry.repeat_daily > 1) || (sql_feed_schedule_entry.repeat_daily < 0)
        {
            return Err(FeedScheduleEntryRepeatDailyOutOfRange {
                location: module_path!().to_string(),
                repeat_daily: sql_feed_schedule_entry.repeat_daily,
            });
        }
        Ok(FeedScheduleEntry {
            timestamp: sql_feed_schedule_entry.timestamp,
            profile_id: sql_feed_schedule_entry.profile_id,
            repeat_daily: sql_feed_schedule_entry.repeat_daily != 0,
        })
    }

    /// Creates a new `FeedScheduleEntry` for use in test environments.
    ///
    /// This test-only helper function simplifies the creation of `FeedScheduleEntry`
    /// instances by accepting primitive types directly.
    ///
    /// # Arguments
    /// * `timestamp_string` - A string slice representing the timestamp in `YYYY-MM-DD HH:MM:SS` format.
    /// * `profile_id` - The numeric identifier for the feed profile.
    /// * `repeat_daily` - A boolean indicating if the schedule should repeat daily.
    ///
    /// # Returns
    /// A new `FeedScheduleEntry` instance.
    ///
    /// # Panics
    /// This function will panic if the provided `timestamp_string` cannot be parsed
    /// into a valid `NaiveDateTime`. This is intentional, as test setup data is
    /// expected to be valid, and a failure indicates a flaw in the test itself.
    pub fn new_for_test(
        timestamp_string: &str,
        profile_id: i32,
        repeat_daily: bool,
    ) -> FeedScheduleEntry {
        FeedScheduleEntry {
            timestamp: NaiveDateTime::parse_from_str(timestamp_string, "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            profile_id,
            repeat_daily,
        }
    }
}

impl fmt::Display for FeedScheduleEntry {
    /// Formats the `FeedScheduleEntry` for display.
    ///
    /// This implementation provides a concise, single-line string representation
    /// of the schedule entry, suitable for logging and debugging. It includes the
    /// timestamp, profile ID, profile name, and daily repetition status.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to the formatter, as required by the `fmt::Display` trait.
    ///
    /// # Returns
    /// A `fmt::Result` containing `Ok(())` if the formatting was successful.
    ///
    /// # Errors
    /// Returns an `Err` if the `write!` macro fails to write to the underlying
    /// formatter. This is an uncommon error but can occur if the formatter
    /// itself is in an error state or if the destination (e.g., a file or buffer)
    /// reports an I/O error.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "timestamp={}, profile_id={}, repeat_daily={}",
            self.timestamp, self.profile_id, self.repeat_daily,
        )
    }
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    /// Helper function to create a `SqlFeedScheduleEntry` for testing.
    ///
    /// # Arguments
    /// * `repeat_daily` - A parameter representing `repeat_daily`.
    fn create_sql_entry(repeat_daily: i16) -> SqlFeedScheduleEntry {
        SqlFeedScheduleEntry {
            timestamp: NaiveDate::from_ymd_opt(2024, 1, 1)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
            profile_id: 101,
            repeat_daily,
        }
    }

    #[test]
    // Tests the successful creation of a `FeedScheduleEntry` when `repeat_daily` is 0.
    // [utest->swreq~food_read_schedule~1]
    fn test_new_with_repeat_daily_zero() {
        println!("* Testing FeedScheduleEntry::new with repeat_daily = 0...");
        // --- Setup ---
        let sql_entry = create_sql_entry(0);

        // --- Execution ---
        let result = FeedScheduleEntry::new(&sql_entry);

        // --- Assertions ---
        assert!(result.is_ok(), "Expected Ok, but got Err: {:?}", result);
        let entry = result.unwrap();
        assert!(
            !entry.repeat_daily,
            "repeat_daily should be false when the input is 0"
        );
        assert_eq!(entry.profile_id, 101);
        println!("* Succeeded: Correctly created entry with repeat_daily=false.");
    }

    #[test]
    // Tests the successful creation of a `FeedScheduleEntry` when `repeat_daily` is 1.
    /// [utest->swreq~food_read_schedule~1]
    fn test_new_with_repeat_daily_one() {
        println!("* Testing FeedScheduleEntry::new with repeat_daily = 1...");
        // --- Setup ---
        let sql_entry = create_sql_entry(1);

        // --- Execution ---
        let result = FeedScheduleEntry::new(&sql_entry);

        // --- Assertions ---
        assert!(result.is_ok(), "Expected Ok, but got Err: {:?}", result);
        let entry = result.unwrap();
        assert!(
            entry.repeat_daily,
            "repeat_daily should be true when the input is 1"
        );
        println!("* Succeeded: Correctly created entry with repeat_daily=true.");
    }

    #[test]
    // Tests that `new` returns an error when `repeat_daily` is greater than 1.
    /// [utest->swreq~food_read_schedule~1]
    fn test_new_with_repeat_daily_out_of_range_positive() {
        println!("* Testing FeedScheduleEntry::new with repeat_daily > 1...");
        // --- Setup ---
        let sql_entry = create_sql_entry(2);

        // --- Execution ---
        let result = FeedScheduleEntry::new(&sql_entry);

        // --- Assertions ---
        assert!(result.is_err(), "Expected an error for out-of-range value");
        let error = result.unwrap_err();
        assert!(
            matches!(
                error,
                FeedScheduleEntryRepeatDailyOutOfRange {
                    repeat_daily: 2,
                    ..
                }
            ),
            "Expected FeedScheduleEntryRepeatDailyOutOfRange error with value 2, but got {:?}",
            error
        );
        println!("* Succeeded: Correctly returned error for repeat_daily=2.");
    }

    #[test]
    // Tests that `new` returns an error when `repeat_daily` is less than 0.
    /// [utest->swreq~food_read_schedule~1]
    fn test_new_with_repeat_daily_out_of_range_negative() {
        println!("* Testing FeedScheduleEntry::new with repeat_daily < 0...");
        // --- Setup ---
        let sql_entry = create_sql_entry(-1);

        // --- Execution ---
        let result = FeedScheduleEntry::new(&sql_entry);

        // --- Assertions ---
        assert!(result.is_err(), "Expected an error for out-of-range value");
        let error = result.unwrap_err();
        assert!(
            matches!(
                error,
                FeedScheduleEntryRepeatDailyOutOfRange {
                    repeat_daily: -1,
                    ..
                }
            ),
            "Expected FeedScheduleEntryRepeatDailyOutOfRange error with value -1, but got {:?}",
            error
        );
        println!("* Succeeded: Correctly returned error for repeat_daily=-1.");
    }
}
