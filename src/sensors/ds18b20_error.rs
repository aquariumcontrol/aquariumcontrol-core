/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum Ds18b20Error {
    /// DS18B20 sensor with matching ID has not been found.
    #[error("DS18B20 sensor with ID {0} not found.")]
    SensorIdNotFound(String),

    #[error("Failed to read content from the device file.")]
    ReadToStringFailure,

    #[error("Device file contains insufficient lines (expected at least 2).")]
    InsufficientLinesInFile,

    #[error("The first line of the device file does not end with ' YES', indicating a CRC error or incomplete reading.")]
    Line0NotEndingWithYes,

    #[error("Temperature entry (t=) not found in the second line of the device file.")]
    TemperatureEntryNotFound,

    #[error("An unidentified error occurred.")]
    UnidentifiedError,

    #[error("Failed to parse temperature value as a floating-point number.")]
    TemperatureParseError,

    #[error("Temperature value read from sensor is outside the expected range.")]
    TemperatureOutOfRange,
}
