/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum Ds18b20Error {
    /// Indicates that a DS18B20 sensor with the specified ID could not be found on the 1-Wire bus.
    #[error("[{0}] DS18B20 sensor with ID {1} not found.")]
    SensorIdNotFound(String, String),

    /// Represents a failure to read the entire content of the sensor's device file into a string.
    #[error("[{0}] Failed to read content from the device file.")]
    ReadToStringFailure(String),

    /// Occurs when the sensor's device file does not contain the expected two lines of data.
    #[error("[{0}] Device file contains insufficient lines (expected at least 2).")]
    InsufficientLinesInFile(String),

    /// The first line from the sensor file must end with ' YES' to confirm a successful CRC check.
    /// This error means the check failed, indicating a data transmission error.
    #[error("[{0}] The first line of the device file does not end with ' YES', indicating a CRC error or incomplete reading.")]
    Line0NotEndingWithYes(String),

    /// The second line of the sensor file is expected to contain a temperature value prefixed with 't='.
    /// This error indicates the prefix was not found.
    #[error("[{0}] Temperature entry (t=) not found in the second line of the device file.")]
    TemperatureEntryNotFound(String),

    /// A generic error for cases that do not fit any other specific error variant.
    #[error("[{0}] An unidentified error occurred.")]
    UnidentifiedError(String),

    /// The temperature value found in the sensor file could not be successfully parsed into a numeric type.
    #[error("[{0}] Failed to parse temperature value as a floating-point number.")]
    TemperatureParseError(String),

    /// The parsed temperature value is outside the plausible range for an aquarium,
    /// suggesting a sensor malfunction or reading error.
    #[error("[{0}] Temperature value read from sensor is outside the expected range.")]
    TemperatureOutOfRange(String),

    /// System driver base path prefix is empty.
    #[error("[{0}] System driver base path prefix is empty.")]
    SystemDriverBasePathEmpty(String),

    /// System driver sensor path prefix is empty.
    #[error("[{0}] System driver sensor path prefix is empty.")]
    SystemDriverSensorPathEmpty(String),

    /// System driver file name is empty.
    #[error("[{0}] System driver file name is empty.")]
    SystemDriverFileNameEmpty(String),

    /// No water temperature sensor ID nor ambient temperature sensor ID provided.
    #[error("[{0}] No water temperature sensor ID nor ambient temperature sensor ID provided!")]
    NoSensorIdProvided(String),

    /// Ambient and water temperature sensor IDs are identical.
    #[error("Ambient and water temperature sensor IDs are identical.")]
    SensorIdsIdentical(String),
}
