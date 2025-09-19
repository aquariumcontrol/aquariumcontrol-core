/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use thiserror::Error;
#[derive(Debug, Error)]
/// Enum codifies the errors the program may encounter in communication with DHT sensor.
pub enum DhtError {
    /// Vcc pin acquisition failed
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error(
        "[{location}] Could not get pin {vcc_pin_nr} from GPIO interface for DHT power supply."
    )]
    VccPinAcquisitionFailed {
        location: String,
        vcc_pin_nr: u8,

        #[source]
        source: rppal::gpio::Error,
    },

    /// Data pin acquisition for output failed
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{location}] Failure to acquire pin {data_pin_nr} for triggering DHT sensor")]
    DataPinOutputAcquisitionFailed {
        location: String,
        data_pin_nr: u8,

        #[source]
        source: rppal::gpio::Error,
    },

    /// Data pin acquisition for input failed
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{location}] Failure to acquire pin {input_pin_nr} for reading from DHT sensor")]
    DataPinInputAcquisitionFailed {
        location: String,
        input_pin_nr: u8,

        #[source]
        source: rppal::gpio::Error,
    },

    /// Could not set interrupt
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{0}] Could not set interrupt")]
    CouldNotSetInterrupt(String),

    /// Could not clear the interrupt
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{0}] Could not clear the interrupt")]
    CouldNotClearInterrupt(String),

    // execution of interrupt failed
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{0}] Execution of interrupt failed")]
    InterruptFailed(String),

    // transmission yields a wrong number of valid bits
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{0}] Incorrect number of valid ticks received: target={1}, measured={2}.")]
    WrongNumberOfValidTicks(String, usize, usize),

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// calculated checksum does not match the received value
    #[error("[{0}] Calculated checksum ({1}) does not match the received value ({2})")]
    ChecksumError(String, u8, u8),

    /// timeout during transmission of data
    #[allow(unused)]
    #[error("[{0}] Timeout during transmission of data")]
    Timeout(String),

    /// the temperature value is out of range
    #[error("[{0}] The temperature value is out of range (measured {1}, limits are {2}, {3})")]
    TemperatureOutOfRange(String, f32, f32, f32),

    /// humidity value is out of range
    #[error("[{0}] The humidity is out of range (measured {1}, limits are {2}, {3})")]
    HumidityOutOfRange(String, f32, f32, f32),

    /// reported from low-level reading when called without gpio lib handle available
    #[allow(unused)] // used in conditionally compiled code
    #[error("[{0}] Gpio handle has not been provided. Check the configuration.")]
    GpioHandleNotProvided(String),

    /// reported from low-level reading function substitute for non-target platform
    #[allow(unused)] // used in conditionally compiled code
    #[error("[{0}] Dht driver has not been compiled into the SW for this platform.")]
    IncorrectPlatform(String),

    /// The measurement has been repeated several times without success.
    #[allow(unused)] // used in conditionally compiled code
    #[error("[{location}] Failed to read from DHT sensor (after {retry_counter} retries")]
    FailedAfterRetries {
        location: String,
        retry_counter: u64,

        #[source]
        source: Box<DhtError>,
    },
}
