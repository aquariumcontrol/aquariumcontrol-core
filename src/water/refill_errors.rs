/* Copyright 2024 Uwe Martinuuu

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
    /// Resets all error bits of the struct.
    pub fn reset_all_errors(&mut self) {
        self.error_switch_stuck_high = false;
        self.error_switch_stuck_low = false;
        self.error_receive_tank_level_switch_position = false;
        self.error_sql_update_failed = false;
        self.error_sql_get_historic_data_failed = false;
    }

    #[cfg(test)]
    pub fn has_error(&mut self) -> bool {
        self.error_switch_stuck_high
            || self.error_switch_stuck_low
            || self.error_receive_tank_level_switch_position
            || self.error_sql_update_failed
            || self.error_sql_get_historic_data_failed
    }
}

impl fmt::Display for RefillErrors {
    /// Function provides output of RefillErrors for development purposes
    /// # Arguments
    /// * `f` - configuration for formatting
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
