/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the central `Monitors` thread.
//!
//! This module provides the `MonitorsConfig` struct, which is used by `serde` to
//! deserialize the `[monitors]` section of the main TOML configuration file. It
//! holds parameters that control the behavior and resource usage of the `Monitors`
//! thread, which is responsible for aggregating data from other modules.

use serde_derive::Deserialize;

/// Holds the configuration data for the monitors.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct MonitorsConfig {
    /// monitors will only be active when the flag is set to true
    pub active: bool,

    /// indicate if the thread shall be started or not
    pub execute: bool,

    /// Maximum number of monitoring entries for refill
    pub max_length_refill_monitor_views: usize,
}
