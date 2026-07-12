/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
//! Defines the configuration structure for the Inter-Process Communication (IPC) messaging system.
//!
//! This module contains the `MessagingConfig` struct, which is used with `serde` to
//! deserialize the `[messaging]` section from the application's TOML configuration file.
//! It holds parameters that control the behavior of the POSIX message queue listener.
//!
//! This entire module is conditionally compiled and is only available on `target_os = "linux"`
//! (or during tests), as it is part of the POSIX-based IPC mechanism.

use serde_derive::Deserialize;

/// Holds the configuration data for the messaging.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct MessagingConfig {
    #[cfg(target_os = "linux")]
    /// filename used for generating the message queue identifier
    pub mq_filename: String,

    #[cfg(target_os = "linux")]
    /// timeout for receiving messages in milliseconds
    /// after each timeout, the application checks if a shutdown has been requested
    pub timeout_millis: u64,
}
