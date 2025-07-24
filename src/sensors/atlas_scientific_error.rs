/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use thiserror::Error;

/// error codes for evaluation of sensor unit response
#[allow(dead_code)]
#[derive(Debug, PartialEq, Clone, Error)]
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
    InvalidUtf8Sequence,

    /// Conversion from String to f32 failed.
    #[error("Conversion from String to f32 failed.")]
    ConversionFailure,

    /// Incorrect buffer length
    #[error("The buffer has incorrect length.")]
    IncorrectBufferLength,
}
