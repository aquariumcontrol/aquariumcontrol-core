/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the data structure for holding historical refill statistics.
//!
//! This module provides the `RefillStatsData` struct, which acts as a Data Transfer
//! Object (DTO). Its primary purpose is to serve as a container for the historical
//! data that the `Refill` thread retrieves from the database before making any
//! operational decisions.
//!
//! ## Purpose and Design
//!
//! Before initiating a water top-off, the `Refill` thread must check against
//! configured safety limits to prevent over-filling or reacting to faulty sensors.
//! To do this, it needs to know about recent refill activity.
//!
//! This struct holds the results of those database queries, such as
//! - The total volume and number of refills in the last hour.
//! - The total volume and number of refills in the last 24 hours.
//! - The time elapsed since the most recent refill.
//!
//! By populating this single struct, the `Refill` thread has all the necessary
//! historical context to enforce its safety rules.
//!
//! ## Key Components
//!
//! - **`RefillStatsData`**: The main struct containing fields for each piece of
//!   historical data.
//! - **`Default` Trait**: Implemented to provide a convenient, zeroed-out instance,
//!   which is useful for initialization.
//! - **`fmt::Display`**: Provides a human-readable summary of the stats, which is
//!   valuable for logging and debugging.
//! 
use std::fmt;

/// Contains the data retrieved from the SQL database.
/// It is also used to communicate data to be updated in the SQL database.
#[derive(Default)]
pub struct RefillStatsData {
    /// Duration since the last refill operation in seconds
    pub duration_since_last_refill: u64,

    /// Total volume of refills in the last 24 hours in liters
    pub refill_volume_last_24h: f64,

    /// Count of refills in the last 24 hours
    pub refill_count_last_24h: u64,

    /// Total volume of refills in the last hour in liters
    pub refill_volume_last_hour: f64,

    /// Count of refills in the last hour
    pub refill_count_last_hour: u64,
}

impl fmt::Display for RefillStatsData {
    /// Provides a multi-line, human-readable output of the struct's values for debugging purposes.
    ///
    /// This implementation formats all fields of the `RefillStatsData` struct into a detailed
    /// string representation, summarizing historical refill statistics such as duration since
    /// the last refill, and total volume and count for both the last 24 hours and the last hour.
    /// This is particularly useful for inspecting refill data during development and logging.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to the formatter, as required by the `fmt::Display` trait.
    ///
    /// # Returns
    /// An `Ok(())` on successful formatting.
    ///
    /// # Errors
    /// Returns an `Err` containing a `std::fmt::Error` if an I/O error occurs
    /// while writing to the formatter.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            " {}: Duration since last refill is {} seconds.\n \
            Refill: volume of last 24h is {} L.\n Refill: Count of last 24h is {}.\n \
            Refill: volume of last hour is {} L.\n Refill: Count of last hour is {}.",
            module_path!(),
            self.duration_since_last_refill,
            self.refill_volume_last_24h,
            self.refill_count_last_24h,
            self.refill_volume_last_hour,
            self.refill_count_last_hour
        )
    }
}
