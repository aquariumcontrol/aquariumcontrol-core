/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the centralized error type for the entire relay actuation subsystem.
//!
//! This module contains the `RelayError` enum, which consolidates all possible errors
//! that can occur within the relay management and hardware actuation layers. By using
//! the `thiserror` crate, it provides a rich, descriptive, and easy-to-use error
//! type that supports source-chaining for detailed diagnostics.
//!
//! ## Design and Architecture
//!
//! The `RelayError` enum is designed for robustness and clear, actionable error reporting.
//!
//! - **Centralization**: Instead of each hardware module (`actuate_controllino`,
//!   `actuate_gpio`, etc.) defining its own error types, they all use this single,
//!   comprehensive enum. This simplifies the error handling logic in the `RelayManager`
//!   and other upstream components.
//!
//! - **Descriptive Messages**: Each variant uses the `#[error("...")]` attribute from
//!   `thiserror` to generate a human-readable error message. These messages often
//!   include the location (`module_path!()`) and other contextual information
//!   (e.g., the command that failed), which is invaluable for debugging.
//!
//! - **Source Chaining**: Several variants (like `ActuationFailed` and `GpioGetPinFailure`)
//!   use the `#[source]` attribute. This captures the underlying, lower-level error
//!   (e.g., an `io::Error` or `rppal::gpio::Error`), allowing for a full "error chain"
//!   to be logged. This helps pinpoint the root cause of a failure.
//!
//! - **Conditional Compilation**: The `GpioGetPinFailure` variant is conditionally
//!   compiled using `#[cfg(...)]`. This ensures that GPIO-specific errors are only
//!   part of the application when it's being built for the intended hardware target
//!   (Linux with the `target_hw` feature), preventing compilation errors on other
//!   platforms.

use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
use thiserror::Error;

/// Enum codifies the errors which the program may encounter in preparation or execution of communication with the relay manager
#[derive(Debug, Error)]
pub enum RelayError {
    /// Received an irrelevant command.
    #[error("[{0}] Received an irrelevant command {1}")]
    IrrelevantCommand(String, InternalCommand),

    /// Failure occurred in command message creation.
    #[error("[{0}] Failure occurred in command message creation")]
    FailureCommandMessageCreation(String),

    /// Error occurred when writing to relay.
    #[error("[{0}] Error occurred when writing to relay")]
    WriteError(String),

    /// Error occurred when reading relay state.
    #[error("[{0}] Error occurred when reading relay state")]
    ReadError(String),

    /// Encountered an incorrect checksum in communication with relay provider.
    #[error("[{0}] Encountered an incorrect checksum in communication with relay provider")]
    IncorrectChecksum(String),

    /// The pulse-command is not allowed for the device.
    #[error("[{0}] The pulse-command is not allowed for the device {1}")]
    PulseNotAllowedForDevice(String, AquariumDevice),

    /// The actuation failed after several retries.
    #[error(
        "[{location}] Actuation failed (attempt {last_attempt_number}/{max_attempt_allowed})."
    )]
    ActuationFailed {
        location: String,
        actuation_name: String,
        last_attempt_number: u64,
        max_attempt_allowed: u64,

        #[source]
        source: Box<RelayError>,
    },

    /// Serial port has not been initialized. This is a configuration error.
    #[error("[{0}] Serial port has not been initialized. This is a configuration error.")]
    SerialPortNotConfigured(String),

    /// Sending to the serial port failed
    #[error("[{location}] Sending to serial port failed.")]
    SendToSerialPortFailed {
        location: String,

        #[source]
        source: std::io::Error,
    },

    /// Flushing the serial port buffer failed.
    #[error("[{location}] Flushing serial port buffer failed.")]
    SerialPortFlushFailed {
        location: String,

        #[source]
        source: serialport::Error,
    },

    /// Could not get pin from GPIO interface.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{location}] Could not get pin {pin_number} from GPIO interface for {device}")]
    GpioGetPinFailure {
        location: String,
        pin_number: u8,
        device: String,

        #[source]
        source: rppal::gpio::Error,
    },

    #[error("[{0}] Simulator not initialized.")]
    SimulatorNotInitialized(String),

    #[error("[{0}] Simulator send failed.")]
    SimulatorSendFailed(String),
}
