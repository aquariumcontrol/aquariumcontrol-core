/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::sensors::i2c_error::I2cError;
use crate::utilities::channel_content::AquariumSignal;
use std::num::ParseFloatError;
use std::str::Utf8Error;
use std::sync::mpsc::TryRecvError;
use thiserror::Error;

/// error codes for evaluation of sensor unit response
#[allow(dead_code)]
#[derive(Debug, PartialEq, Error)]
pub enum AtlasScientificError {
    /// The first character in the message sent from the sensor does not match the required magic number.
    #[error("The first character in the message sent from the sensor ({1}) does not match the required magic number ({0}).")]
    FirstCharacterNotMagic(u8, u8),

    /// The last character in the message sent from the sensor does not match the required magic number.
    #[error("The last character in the message sent from the sensor ({1}) does not match the required magic number ({0})")]
    LastCharacterNotMagic(u8, u8),

    /// The message sent from the sensor unit contains non-numeric characters in the data section
    #[error("The message sent from the sensor unit contains non-numeric character with ASCII value ('{0}') in the data section.")]
    ContainsInvalidCharacter(u8),

    /// The error is masked by other checks. Still, the signature of the function to convert from u8 array slice to String throws an error.
    #[error("An invalid UTF8 sequence occurred.")]
    InvalidUtf8Sequence {
        #[source]
        source: Utf8Error,
    },

    /// Conversion from String to f32 failed.
    #[error("Conversion from String to f32 failed.")]
    ConversionFailure {
        #[source]
        source: ParseFloatError,
    },

    /// Incorrect buffer length
    #[error("The response from {0} has incorrect length ({1}).")]
    IncorrectBufferLength(AquariumSignal, usize),

    /// Mutex poisoned
    #[error("Could not lock mutex for {signal}")]
    CouldNotLockMutex { signal: AquariumSignal },

    #[error("Error in sensor unit response for {signal}.")]
    ResponseCheckFailed {
        signal: AquariumSignal,
        #[source]
        source: Box<AtlasScientificError>,
    },

    /// Initial value, is overwritten
    #[error("Initial value")]
    Initial,

    /// Error in I2C communication for a specific signal.
    #[error("Error in I2C communication for {signal}.")]
    I2cCommunicationFailure {
        signal: AquariumSignal,

        #[source]
        source: I2cError,
    },

    /// Error is used to communicate to caller that receiving needs to be repeated.
    #[error("Channel for receiving from I2CInterface thread is empty.")]
    ChannelIsEmpty,

    /// The channel is disconnected.
    #[error("Channel for receiving from I2CInterface is disconnected.")]
    ChannelIsDisconnected {
        #[source]
        source: TryRecvError,
    },

    #[error("Initialization value - result has not been set yet")]
    NotInitialized,
}
