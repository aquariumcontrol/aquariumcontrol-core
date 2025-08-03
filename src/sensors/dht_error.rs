/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use thiserror::Error;
#[derive(Debug, PartialEq, Error)]
/// Enum codifies the errors the program may encounter in communication with DHT sensor.
pub enum DhtError {
    /// Vcc pin acquisition failed
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("Could not get pin {0} from GPIO interface for DHT power supply.")]
    VccPinAcquisitionFailed(u8),

    /// Data pin acquisition for output failed
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("Failure to acquire pin {0} for triggering DHT sensor")]
    DataPinOutputAcquisitionFailed(u8),

    /// Data pin acquisition for input failed
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("Failure to acquire pin {0} for reading from DHT sensor")]
    DataPinInputAcquisitionFailed(u8),

    /// Could not set interrupt
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("Could not set interrupt")]
    CouldNotSetInterrupt,

    /// Could not clear the interrupt
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("Could not clear the interrupt")]
    CouldNotClearInterrupt,

    // execution of interrupt failed
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("Execution of interrupt failed")]
    InterruptFailed,

    // transmission yields a wrong number of valid bits
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("Incorrect number of valid ticks received: target={0}, measured={1}.")]
    WrongNumberOfValidTicks(usize, usize),

    /// the error occurred during the calculation of the checksum
    #[error("An error occurred during the calculation of the checksum")]
    ChecksumCalculationError,

    /// calculated checksum does not match the received value
    #[error("Calculated checksum ({0}) does not match the received value ({1})")]
    ChecksumError(u8, u8),

    /// timeout during transmission of data
    #[allow(unused)]
    #[error("Timeout during transmission of data")]
    Timeout,

    /// the temperature value is out of range
    #[error("The temperature value is out of range (measured {0}, limits are {1}, {2})")]
    TemperatureOutOfRange(f32, f32, f32),

    /// humidity value is out of range
    #[error("The humidity is out of range (measured {0}, limits are {1}, {2})")]
    HumidityOutOfRange(f32, f32, f32),

    /// reported from low-level reading when called without gpio lib handle available
    #[error("This SW is not compiled for this platform.")]
    IncorrectPlatform,

    /// The measurement has been repeated several times without success.
    #[error("failed to read from DHT sensor (after {retry_counter} retries")]
    FailedAfterRetries {
        retry_counter: u64,

        #[source]
        source: Box<DhtError>,
    },
}
