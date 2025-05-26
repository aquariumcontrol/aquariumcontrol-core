/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use std::fmt;

/// Contains the error flags for the refill component.
pub struct RefillErrors {
    pub error_switch_stuck_high: bool,
    pub error_switch_stuck_low: bool,
    pub error_receive_tank_level_switch_position: bool,
    pub error_sql_update_failed: bool,
    pub error_sql_get_historic_data_failed: bool,
}

impl RefillErrors {
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
    }

    #[cfg(test)]
    // Checks if any error flag within the struct is currently set to `true`.
    //
    // This helper function is used exclusively in test environments to quickly
    // determine if the `Refill` module has encountered any error conditions.
    //
    // # Returns
    // `true` if at least one error flag (e.g., `error_switch_stuck_high`, `error_sql_update_failed`)
    // is set to `true`; otherwise, returns `false`.
    pub fn has_error(&mut self) -> bool {
        self.error_switch_stuck_high
            || self.error_switch_stuck_low
            || self.error_receive_tank_level_switch_position
            || self.error_sql_update_failed
            || self.error_sql_get_historic_data_failed
    }
}

impl fmt::Display for RefillErrors {
    /// Formats the `RefillErrors` struct into a concise, single-line string for development purposes.
    ///
    /// This implementation is designed for quick inspection of the refill control's
    /// error states. It displays the boolean value of each error flag, making it
    /// easy to see which specific error conditions are currently active.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to the formatter, as required by the `fmt::Display` trait.
    ///
    /// # Returns
    /// A `fmt::Result` indicating whether the formatting operation was successful.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
            "error_switch_stuck_high: {}, error_switch_stuck_low: {}, error_receive_tank_level_switch_position: {},  error_sql_update_failed: {}, error_sql_get_historic_data_failed: {}",
            self.error_switch_stuck_high,
            self.error_switch_stuck_low,
            self.error_receive_tank_level_switch_position,
            self.error_sql_update_failed,
            self.error_sql_get_historic_data_failed,
        )
    }
}
