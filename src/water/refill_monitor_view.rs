/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use std::fmt;

/// Struct collects the information for the reporting to the monitor
/// The information represents no execution error, but critical states of the control object.
/// In this case, the monitor view informs about excessive triggering of refill operation
/// which may be caused by a defective sensor or leakage of the main tank.
#[derive(PartialEq, Debug, Clone)]
pub struct RefillMonitorView {
    /// refill volume within the last 24h exceeded the threshold
    pub refill_check_volume_last_24h: bool,

    /// refill count within the last 24h exceeded the threshold
    pub refill_check_count_last_24h: bool,

    /// refill volume within the last hour exceeded the threshold
    pub refill_check_volume_last_hour: bool,

    /// refill count within the last hour exceeded the threshold
    pub refill_check_count_last_hour: bool,

    /// time interval since the last refill is too short
    pub refill_check_interval_last_refill: bool,
}

#[cfg(test)]
impl RefillMonitorView {
    // Helper function to assert the state of a RefillMonitorView
    pub fn assert_refill_monitor_view(
        &self,
        expected_volume_last24h: bool,
        expected_count_last24h: bool,
        expected_volume_last_h: bool,
        expected_count_last_h: bool,
        expected_interval_last_refill: bool,
    ) {
        assert_eq!(
            self.refill_check_volume_last_24h, expected_volume_last24h,
            "refill_check_volume_last_24h mismatch"
        );
        assert_eq!(
            self.refill_check_count_last_24h, expected_count_last24h,
            "refill_check_count_last_24h mismatch"
        );
        assert_eq!(
            self.refill_check_volume_last_hour, expected_volume_last_h,
            "refill_check_volume_last_hour mismatch"
        );
        assert_eq!(
            self.refill_check_count_last_hour, expected_count_last_h,
            "refill_check_count_last_hour mismatch"
        );
        assert_eq!(
            self.refill_check_interval_last_refill, expected_interval_last_refill,
            "refill_check_interval_last_refill mismatch"
        );
    }
}

// Implement the Display trait for RefillMonitorView to provide a human-readable string representation.
impl fmt::Display for RefillMonitorView {
    /// Formats the `RefillMonitorView` flags into a human-readable string.
    ///
    /// This implementation provides a concise, single-line summary of any active
    /// refill warnings. If no flags are set, it reports that all checks are OK.
    /// Otherwise, it lists the specific warnings, separated by commas.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to a `Formatter` where the output will be written.
    ///
    /// # Returns
    /// An `Ok(())` on successful formatting.
    ///
    /// # Errors
    /// Returns an `Err` containing a `std::fmt::Error` if an I/O error occurs
    /// while writing to the formatter.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut checks = Vec::new();

        if self.refill_check_volume_last_24h {
            checks.push("Volume last 24h exceeded");
        }
        if self.refill_check_count_last_24h {
            checks.push("Count last 24h exceeded");
        }
        if self.refill_check_volume_last_hour {
            checks.push("Volume last hour exceeded");
        }
        if self.refill_check_count_last_hour {
            checks.push("Count last hour exceeded");
        }
        if self.refill_check_interval_last_refill {
            checks.push("Interval since last refill too short");
        }

        if checks.is_empty() {
            write!(f, "All refill checks are OK.")
        } else {
            write!(f, "Refill issues: {}", checks.join(", "))
        }
    }
}
