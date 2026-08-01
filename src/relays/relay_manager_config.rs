/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the central relay management system.
//!
//! This module contains the `RelayManagerConfig` struct, which is used with `serde`
//! to deserialize the `[relay_manager]` section from the application's TOML configuration
//! file. It holds all the high-level parameters that control the behavior of the
//! `RelayManager` thread, most importantly, which hardware actuation backend to use
//! and how to handle communication failures.
//!

use serde_derive::Deserialize;
use std::fmt;

/// Holds the configuration data for the relay manager.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Clone, Deserialize, Debug)]
pub struct RelayManagerConfig {
    /// Switch for testing purposes enabling main functionalities
    pub active: bool,

    /// Indicates if the thread shall be started or not
    pub execute: bool,

    /// Flag to determine if the application shall communicate with hardware directly or via TCP with a simulator
    pub use_simulator: bool,

    /// Flag to determine if the application shall use GPIO or Controllino
    pub use_gpio: bool,

    /// The number of retries in case write operation to actuator fails
    /// Applicable only to Controllino, because GPIO actuation does not execute a write operation
    pub actuation_retries: u64,

    /// Pause time in milliseconds between retries
    pub pause_between_retries_millis: u64,
}

#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
impl fmt::Display for RelayManagerConfig {
    /// Formats the `RelayManagerConfig` for display purposes.
    ///
    /// This implementation provides a clean, multi-line string representation of the
    /// relay manager's configuration, which is ideal for logging at startup or
    /// for debugging purposes.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Relay Manager Configuration:")?;
        writeln!(f, "  Active: {}", self.active)?;
        writeln!(f, "  Use Simulator: {}", self.use_simulator)?;
        writeln!(f, "  Use GPIO: {}", self.use_gpio)?;
        writeln!(f, "  Actuation Retries: {}", self.actuation_retries)?;
        write!(
            f,
            "  Pause Between Retries: {} ms",
            self.pause_between_retries_millis
        )
    }
}
