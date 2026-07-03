/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
//! Provides the concrete implementation for "petting" a system watchdog.
//!
//! This struct doesn't hold any state; its purpose is to implement the `PettingTrait`
//! for writing heartbeat messages to a watchdog file.

pub struct Petting {}

impl Petting {
    /// Creates a new `Petting` instance.
    ///
    /// This constructor initializes the watchdog petting mechanism.
    /// As the `Petting` struct is stateless, it simply returns an empty instance.
    ///
    /// # Returns
    /// A new `Petting` struct.
    pub fn new() -> Petting {
        Petting {}
    }
}

impl Default for Petting {
    fn default() -> Self {
        Self::new()
    }
}

/// Defines the interface for interacting with a system watchdog.
///
/// This trait provides a method for sending a "heartbeat" signal,
/// typically by writing to a specific file, to inform a watchdog
/// that the application is still alive and functioning.
pub trait PettingTrait {
    /// Sends a heartbeat message to the system watchdog.
    ///
    /// This method's implementation is responsible for performing the
    /// actual "petting" action, usually by writing a predefined string
    /// to a designated watchdog file.
    ///
    /// # Arguments
    /// * `filename` - The path to the watchdog file or device.
    /// * `watchdog_heartbeat` - The specific string message representing the heartbeat.
    fn pet(&mut self, filename: &str, watchdog_heartbeat: &str);
}

impl PettingTrait for Petting {
    /// Writes the heartbeat message to the specified watchdog file.
    ///
    /// This concrete implementation of `pet` uses `std::fs::write` to write the
    /// `watchdog_heartbeat` string to the `watchdog_filename`. The result of the
    /// file write operation is intentionally ignored, as the system watchdog
    /// typically performs its own checks to determine if the heartbeat was received.
    ///
    /// # Arguments
    /// * `watchdog_filename` - The path to the watchdog file.
    /// * `watchdog_heartbeat` - The string message to write as the heartbeat.
    fn pet(&mut self, watchdog_filename: &str, watchdog_heartbeat: &str) {
        let _ = std::fs::write(watchdog_filename, watchdog_heartbeat);
    }
}
