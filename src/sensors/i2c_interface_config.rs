/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the I2C interface service.
//!
//! This module contains the `I2cInterfaceConfig` struct, which is used with `serde`
//! to deserialize the `[i2c_interface]` section from the application's TOML
//! configuration file. It holds parameters that control the low-level behavior of the
//! `I2cInterface` thread, such as hardware timeouts.

use serde_derive::Deserialize;

/// Holds the configuration data for the I2C interface.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
/// Struct is intentionally placed here instead of creating i2c_interface.
#[derive(Deserialize, Debug)]
pub struct I2cInterfaceConfig {
    /// indicates if the thread shall be started or not
    #[cfg(feature = "target_hw")]
    pub execute: bool,

    /// maximum number of read attempts
    #[allow(unused)]
    pub max_read_attempts: u16,

    /// timeout for transactions in milliseconds
    #[allow(unused)]
    pub timeout_transaction: u32,
}
