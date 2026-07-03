/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the system's hardware/software watchdog.
//!
//! This module provides the `WatchdogConfig` struct, which is used by `serde` to
//! deserialize the `[watchdog]` section of the main TOML configuration file. It holds
//! all the necessary parameters for the `Watchdog` thread, which is responsible for
//! periodically "petting" the system watchdog to prevent a system reset due to an
//! unresponsive application.

use serde_derive::Deserialize;

/// Holds the configuration data for the watchdog communication.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct WatchdogConfig {
    /// communication will only be active when the flag is set to true
    pub active: bool,

    /// indicates if the thread shall be started or not
    pub execute: bool,

    /// filename for sending heartbeat to system watchdog
    pub watchdog_filename: String,

    /// heartbeat to be written to watchdog file
    pub watchdog_heartbeat: String,

    /// duration between two heartbeats in milliseconds
    pub heartbeat_duration_millis: u64,

    /// deactivation code to be written to watchdog file upon termination of the application
    pub watchdog_deactivation: String,
}
