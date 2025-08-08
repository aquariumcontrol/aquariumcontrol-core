/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use thiserror::Error;

/// Communication of errors occurring in communication via I2C
#[derive(Error, Debug, PartialEq, Clone)]
pub enum I2cError {
    /// The message could not be sent.
    #[allow(unused)]
    #[error("I2C write operation failed")]
    WriteFailure,

    /// Command could not be sent completely to the sensor unit.
    #[allow(unused)]
    #[error("I2C write operation was incomplete")]
    InsufficientWrite,

    /// Response from the sensor unit could not be received completely.
    #[allow(unused)]
    #[error("I2C read operation was incomplete")]
    InsufficientRead,

    /// Represents a failure to set the address of the I2C sensor.
    #[allow(unused)]
    #[error("Failed to set I2C slave address to {0}")]
    SetAddressError(u16),

    /// Response length from bus participant does not match the expected value.
    #[allow(unused)]
    #[error("I2C response has incorrect length: {0}")]
    IncorrectResponseLength(usize),
}
