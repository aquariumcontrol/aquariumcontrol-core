/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines a struct for tracking the persistent error states of the `Refill` module.
//!
//! This module provides the `RefillErrorStates` struct, which acts as a state machine
//! for tracking various non-fatal but critical error conditions that can occur within
//! the `Refill` thread. Unlike transient `Result::Err` values, these states persist
//! until they are explicitly reset.
//!
//! ## Purpose and Design
//!
//! The `Refill` system needs to remember certain failures over time to prevent
//! unsafe operations. For example, if a database read fails, the system should
//! inhibit all future refills until the problem is resolved and the error is reset.
//! This struct centralizes all such boolean error flags.
//!
//! ## Key Components
//!
//! - **`RefillErrorStates`**: The main struct containing boolean flags for specific
//!   error conditions, such as:
//!   - `error_switch_stuck_high`: The level switch has not indicated a low level for an
//!     unusually long time.
//!   - `error_switch_stuck_low`: The level switch failed to indicate a high level after
//!     a refill operation.
//!   - `error_sql_get_historic_data_failed`: A database query to fetch historical
//!     refill data failed.
//!
//! - **`has_error()`**: A convenient helper method to check if *any* of the error flags
//!   are currently set to `true`. This allows the `Refill` thread to quickly determine
//!   if it should inhibit its operations.
//!
//! - **`reset_all_errors()`**: A method to clear all error flags, typically triggered by
//!   an external command (e.g., from the `Messaging` system) after an operator has
//!   addressed the underlying issue.
//!
//! ## Usage
//!
//! An instance of `RefillErrorStates` is owned by the main `Refill` struct. In its
//! main loop, the `Refill` thread checks `self.errors.has_error()` at the beginning
//! of each cycle. If it returns `true`, the refill logic is skipped until the errors
//! are cleared.

use std::fmt;

/// Contains the error flags for the refill component.
pub struct RefillErrorStates {
    pub error_switch_stuck_high: bool,
    pub error_switch_stuck_low: bool,
    pub error_receive_tank_level_switch_position: bool,
    pub error_sql_update_failed: bool,
    pub error_sql_get_historic_data_failed: bool,
    pub error_join_failed: bool,
}

impl RefillErrorStates {
    /// Resets all internal error flags to `false`.
    ///
    /// This method clears all error conditions within the `RefillErrors` struct,
    /// effectively setting the refill system back to a state where no current
    /// errors are actively reported. This is typically invoked after the user having addressed
    /// the root cause of an error.
    pub fn reset_all_errors(&mut self) {
        self.error_switch_stuck_high = false;
        self.error_switch_stuck_low = false;
        self.error_receive_tank_level_switch_position = false;
        self.error_sql_update_failed = false;
        self.error_sql_get_historic_data_failed = false;
        self.error_join_failed = false;
    }

    /// Checks if any error flag within the struct is currently set to `true`.
    ///
    /// This helper function is used exclusively in test environments to quickly
    /// determine if the `Refill` module has encountered any error conditions.
    ///
    /// # Returns
    /// `true` if at least one error flag (e.g., `error_switch_stuck_high`, `error_sql_update_failed`)
    /// is set to `true`; otherwise, returns `false`.
    pub fn has_error(&mut self) -> bool {
        self.error_switch_stuck_high
            || self.error_switch_stuck_low
            || self.error_receive_tank_level_switch_position
            || self.error_sql_update_failed
            || self.error_sql_get_historic_data_failed
            || self.error_join_failed
    }
}

impl fmt::Display for RefillErrorStates {
    /// Formats the `RefillErrorStates` struct into a concise, single-line string for development purposes.
    ///
    /// This implementation is designed for quick inspection of the refill control's
    /// error states. It displays the boolean value of each error flag, making it
    /// easy to see which specific error conditions are currently active.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to the formatter, as required by the `fmt::Display` trait.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) on successful formatting.
    ///
    /// # Errors
    /// Returns a `fmt::Error` if an I/O error occurs while writing to the formatter.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
            "error_switch_stuck_high: {}, error_switch_stuck_low: {}, error_receive_tank_level_switch_position: {},  error_sql_update_failed: {}, error_sql_get_historic_data_failed: {}, error_join_failed: {}",
            self.error_switch_stuck_high,
            self.error_switch_stuck_low,
            self.error_receive_tank_level_switch_position,
            self.error_sql_update_failed,
            self.error_sql_get_historic_data_failed,
            self.error_join_failed,
        )
    }
}
