/* Copyright 2026 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use thiserror::Error;

/// Contains error definitions for ActuateControllino
#[derive(Error, Debug)]
pub enum ActuateControllinoError {
    /// Relay ID is outside the allowed range.
    #[error("Relay ID for Controllino ({0}) is outside the allowed range ({1} ... {2}).")]
    RelayIdOutsideRange(u8, u8, u8),

    /// Found a duplicate value in definition of relay IDs.
    #[error("Found a duplicate value ({0}) in definition of relay IDs.")]
    RelayIdDuplicate(u8),

    /// Port name for communication with Controllino is empty.
    #[error("Port name for communication with Controllino is empty.")]
    EmptyPortName,

    /// Failed to open serial port for Controllino.
    #[error("Failed to open serial port for Controllino.")]
    SerialPortOpenFailed {
        #[source]
        source: serialport::Error,
    },

    /// Failed to set timeout for serial port communication.
    #[error(
        "Could not set timeout of {timeout_millis} milliseconds for serial port communication."
    )]
    SetTimeoutFailed {
        timeout_millis: u64,

        #[source]
        source: serialport::Error,
    },

    /// Failed to clear DTR of serial port communication.
    #[error("Failed to clear DTR of serial port communication.")]
    ClearDTRFailed {
        #[source]
        source: serialport::Error,
    },

    /// Failed to clear buffer of serial port communication.
    #[error("Failed to clear buffer of serial port communication.")]
    ClearBufferFailed {
        #[source]
        source: serialport::Error,
    },
}
