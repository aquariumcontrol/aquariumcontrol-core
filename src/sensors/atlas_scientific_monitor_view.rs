/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the data structure for communicating Atlas Scientific sensor status to the monitor.
//!
//! This module provides the `AtlasScientificMonitorView` struct, which acts as a Data Transfer
//! Object (DTO). Its primary purpose is to create a "snapshot" of the Atlas Scientific sensor
//! threshold checks at a specific moment in time and communicate it to the central `Monitors` thread.
//!
//! ## Purpose and Design
//!
//! The `AtlasScientific` thread continuously reads temperature, pH, and conductivity values and
//! compares them against configured safety thresholds. If a value exceeds its acceptable range,
//! it could indicate a problem with the aquarium environment or sensor malfunction.
//!
//! This `AtlasScientificMonitorView` struct captures the boolean result of each of these checks.
//! By sending this compact struct to the `Monitors` thread, the application can:
//!
//! - **Decouple Logic**: The `Monitors` thread doesn't need to know *how* the `AtlasScientific`
//!   module performs its checks; it only needs to see the results.
//! - **Store Historical State**: The `Monitors` thread can maintain a history of these
//!   views, allowing for later analysis of recurring issues.
//! - **Efficient Communication**: It's a small, simple struct, making for efficient
//!   inter-thread communication.
//!
//! ## Key Components
//!
//! - **`AtlasScientificMonitorView`**: The main struct containing a boolean flag for each sensor
//!   check performed by the `AtlasScientific` module.
//! - **`fmt::Display`**: Provides a human-readable summary of the view's status, which
//!   is useful for logging and debugging.

use std::fmt;

/// Struct collects the information for the reporting to the monitor.
/// The information represents no execution error, but critical states of the sensor readings.
/// In this case, the monitor view informs about sensor values exceeding their configured
/// threshold ranges, which may indicate environmental problems or sensor malfunctions.
#[derive(PartialEq, Debug, Clone, Default)]
pub struct AtlasScientificMonitorView {
    /// Water temperature is outside the configured min/max range
    pub temperature_out_of_range: bool,

    /// pH value is outside the configured min/max range
    pub ph_out_of_range: bool,

    /// Conductivity value is outside the configured min/max range
    pub conductivity_out_of_range: bool,
}

// Implement the Display trait for AtlasScientificMonitorView to provide a human-readable string representation.
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
impl fmt::Display for AtlasScientificMonitorView {
    /// Formats the `AtlasScientificMonitorView` flags into a human-readable string.
    ///
    /// This implementation provides a concise, single-line summary of any active
    /// sensor threshold violations. If no flags are set, it reports that all checks are OK.
    /// Otherwise, it lists the specific violations, separated by commas.
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

        if self.temperature_out_of_range {
            checks.push("Temperature out of range");
        }
        if self.ph_out_of_range {
            checks.push("pH out of range");
        }
        if self.conductivity_out_of_range {
            checks.push("Conductivity out of range");
        }

        if checks.is_empty() {
            write!(f, "All Atlas Scientific sensor checks are OK.")
        } else {
            write!(f, "Atlas Scientific sensor issues: {}", checks.join(", "))
        }
    }
}
