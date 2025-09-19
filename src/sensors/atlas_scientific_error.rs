/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the centralized error type for the Atlas Scientific sensor subsystem.
//!
//! This module contains the `AtlasScientificError` enum, which consolidates all possible
//! errors that can occur within the Atlas Scientific sensor management thread. By using
//! the `thiserror` crate, it provides a rich, descriptive, and easy-to-use error
//! type that supports source-chaining for detailed diagnostics.
//!
//! ## Design and Architecture
//!
//! The `AtlasScientificError` enum is designed for robustness and clear, actionable error reporting.
//!
//! - **Centralization**: Instead of each function defining its own error types, they all
//!   use this single, comprehensive enum. This simplifies the error handling logic in
//!   the `AtlasScientific` thread.
//!
//! - **Descriptive Messages**: Each variant uses the `#[error("...")]` attribute from
//!   `thiserror` to generate a human-readable error message. These messages
//!   include the location (`module_path!()`) and other contextual information
//!   (e.g., the sensor signal that failed).
//!
//! - **Source Chaining**: Many variants (like `I2cCommunicationFailure` and `ConversionFailure`)
//!   use the `#[source]` attribute. This captures the underlying, lower-level error
//!   (e.g., an `I2cError` or `ParseFloatError`), allowing for a full "error chain"
//!   to be logged.
//!
//! - **Control Flow via Errors**: The `ChannelIsEmpty` variant is used not just as an
//!   error but as a signal for control flow. It indicates to the calling code that
//!   a non-blocking receive operation found no message, and the operation should be
//!   retried later.

use crate::launch::channels::AquaChannelError;
use crate::sensors::i2c_error::I2cError;
use crate::utilities::channel_content::AquariumSignal;
use std::num::ParseFloatError;
use std::str::Utf8Error;
use thiserror::Error;

/// error codes for evaluation of sensor unit response
#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum AtlasScientificError {
    /// The first character in the message sent from the sensor does not match the required magic number.
    #[error("[{0}] The first character in the message sent from the sensor ({2}) does not match the required magic number ({1}).")]
    FirstCharacterNotMagic(String, u8, u8),

    /// The last character in the message sent from the sensor does not match the required magic number.
    #[error("[{0}] The last character in the message sent from the sensor ({2}) does not match the required magic number ({1})")]
    LastCharacterNotMagic(String, u8, u8),

    /// The message sent from the sensor unit contains non-numeric characters in the data section
    #[error("[{0}] The message sent from the sensor unit contains non-numeric character with ASCII value ('{1}') in the data section.")]
    ContainsInvalidCharacter(String, u8),

    /// The error is masked by other checks. Still, the signature of the function to convert from u8 array slice to String throws an error.
    #[error("[{location}] An invalid UTF8 sequence occurred.")]
    InvalidUtf8Sequence {
        location: String,

        #[source]
        source: Utf8Error,
    },

    /// Conversion from String to f32 failed.
    #[error("[{location}] Conversion from String to f32 failed.")]
    ConversionFailure {
        location: String,

        #[source]
        source: ParseFloatError,
    },

    /// Incorrect buffer length
    #[error("[{0}] The response from {1} has incorrect length ({2}).")]
    IncorrectBufferLength(String, AquariumSignal, usize),

    /// Mutex poisoned
    #[error("[{location}] Could not lock mutex for {signal}")]
    CouldNotLockMutex {
        location: String,
        signal: AquariumSignal,
    },

    /// Error in sensor unit response for a specific signal.
    #[error("[{location}] Error in sensor unit response for {signal}.")]
    ResponseCheckFailed {
        location: String,
        signal: AquariumSignal,
        #[source]
        source: Box<AtlasScientificError>,
    },

    /// Error in I2C communication for a specific signal.
    #[error("[{location}] Error in I2C communication for {signal}.")]
    I2cCommunicationFailure {
        location: String,
        signal: AquariumSignal,

        #[source]
        source: I2cError,
    },

    /// Error is used to communicate to caller that receiving needs to be repeated.
    #[error("[{0}] Channel for receiving from I2CInterface thread is empty.")]
    ChannelIsEmpty(String),

    /// The channel is disconnected.
    #[error("[{location}] Channel for receiving from I2CInterface is disconnected.")]
    ChannelIsDisconnected {
        location: String,
        #[source]
        source: AquaChannelError,
    },

    /// Logic error: Signal is unknown
    #[error("[{0}] Ignoring invalid signal request ({1})")]
    InvalidSignal(String, AquariumSignal),

    /// Signal request could not be created.
    #[error("[{location}] Signal request creation failed.")]
    SignalRequestCreationFailure {
        location: String,

        #[source]
        source: I2cError,
    },

    /// Sending request to I2cInterface thread failed.
    #[error("[{location}] Sending request to I2cInterface thread failed.")]
    SendingRequestToI2cInterfaceFailed {
        location: String,

        #[source]
        source: AquaChannelError,
    },

    /// Enum value covering unreachable branch in match
    #[error("Internal error - this should never happen.")]
    UnknownError(String),
}
