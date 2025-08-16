/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
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

    /// Used to initialize variable. Value is overwritten with real error.
    #[error("[{0}] Initial value")]
    InitialValue(String),

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
}
