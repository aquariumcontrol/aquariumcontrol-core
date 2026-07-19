/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the centralized error type for the I2C communication subsystem.
//!
//! This module contains the `I2cError` enum, which consolidates all possible
//! errors that can occur when interacting with the I2C bus. By using
//! the `thiserror` crate, it provides a rich, descriptive, and easy-to-use error
//! type that supports source-chaining for detailed diagnostics.
//!
//! A key feature of this module is its extensive use of conditional compilation
//! (`#[cfg]`). This allows the application to be compiled and tested on platforms
//! without I2C hardware (like a developer's machine), while providing detailed,
//! hardware-specific errors when running on the target device.
//!
//! ## Design and Architecture
//!
//! The `I2cError` enum is designed for robustness and clear, actionable error reporting.
//!
//! - **Centralization**: Instead of each function defining its own error types, they all
//!   use this single, comprehensive enum. This simplifies the error handling logic in
//!   the `I2cInterface` thread and its clients.
//!
//! - **Platform Abstraction**: The `#[cfg(all(target_os = "linux", feature = "target_hw"))]`
//!   attribute is used to isolate hardware-specific errors. Variants that depend on the
//!   `rppal` crate only exist when compiling for the target hardware, preventing
//!   compilation errors on other systems.
//!
//! - **Descriptive Messages**: Each variant uses the `#[error("...")]` attribute from
//!   `thiserror` to generate a human-readable error message. These messages
//!   include the location (`module_path!()`) and other contextual information
//!   (e.g., I2C address or buffer lengths), which is invaluable for debugging.
//!
//! - **Source Chaining**: Most hardware-specific variants use the `#[source]` attribute.
//!   This captures the underlying `rppal::i2c::Error`, allowing for a full "error chain"
//!   to be logged. This helps pinpoint the root cause of a failure, such as a
//!   permission issue or a physical bus problem.

use thiserror::Error;

/// Contains error definition occurring in communication via I2C
#[derive(Error, Debug)]
pub enum I2cError {
    /// Initialization of the I2C interface failed.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{location}] Initialization of I2C interface failed.")]
    InitializationFailure {
        location: String,

        #[source]
        source: rppal::i2c::Error,
    },

    /// Initialization of the I2C interface failed.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{location}] Setting timeout for transactions failed.")]
    SetTimeoutError {
        location: String,

        #[source]
        source: rppal::i2c::Error,
    },

    #[cfg(any(test, feature = "test-utils", all(target_os = "linux", feature = "target_hw")))]
    /// The message could not be sent.
    #[error("[{location}] I2C write operation failed.")]
    WriteFailure {
        location: String,

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        #[source]
        source: rppal::i2c::Error,
    },

    /// Command could not be sent completely to the sensor unit.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{0}] I2C write operation was incomplete: Target={1}, Realized={2}")]
    InsufficientWrite(String, usize, usize),

    /// Response from the sensor unit could not be received completely.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{location}] I2C read operation was incomplete")]
    InsufficientRead {
        location: String,

        #[source]
        source: rppal::i2c::Error,
    },

    /// Represents a failure to set the address of the I2C sensor.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{location}] Failed to set I2C slave address to {address}")]
    SetAddressError {
        location: String,
        address: u16,

        #[source]
        source: rppal::i2c::Error,
    },

    /// Response length from bus participant does not match the expected value.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{0}] I2C response has incorrect length: Expected: {1} Received: {2}")]
    IncorrectResponseLength(String, usize, usize),

    /// Command is too long for buffer
    #[allow(unused)] // used in conditionally compiled code
    #[error("[{location}]: I2C command (length: {actual_len}) exceeds the maximum allowed length ({max_len}).")]
    CommandTooLong {
        location: String,
        max_len: usize,
        actual_len: usize,
    },

    /// Provided buffer is too small for the expected response.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{location}]: Provided buffer (size: {buffer_size}) is too small for the expected response (size: {expected_size}).")]
    ProvidedBufferTooSmall {
        location: String,
        buffer_size: usize,
        expected_size: usize,
    },
}
