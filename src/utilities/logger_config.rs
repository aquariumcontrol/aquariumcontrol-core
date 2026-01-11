/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the application's logging system.
//!
//! This module provides the `LoggerConfig` struct, which is used by `serde` to
//! deserialize the `[logger]` section of the main TOML configuration file. It holds
//! all the necessary parameters for setting up the global logger.
//!
//! ## Key Components
//!
//! - **`LoggerConfig`**: The primary struct containing the path to the log file.
//!
//! - **`impl LoggerConfig`**: Provides a `default()` method to create a fallback
//!   configuration. This is useful for generating a default instance when one is not
//!   fully specified in the configuration file.

use serde_derive::Deserialize;

/// Holds the configuration data for the general log file
#[derive(Deserialize, Clone)]
pub struct LoggerConfig {
    /// path and file name of the log file
    pub logfile: String,
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            logfile: "/var/log/aquarium.log".to_string(),
        }
    }
}
