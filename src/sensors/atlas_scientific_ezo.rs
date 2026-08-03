/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Implements the driver for Atlas Scientific EZO sensors.
//!
//! This module provides the `AtlasScientificEzo` struct, which implements the
//! `AtlasScientificDriver` trait. It handles the specific communication protocol
//! required by the consumer-grade "EZO" circuits from Atlas Scientific.
//!
//! ## Protocol Details
//!
//! This driver implements a specific I2C interaction model for EZO sensors:
//! 1.  **Trigger**: Sends the ASCII command `'R'` (Read).
//! 2.  **Wait**: Sleeps for a configured duration (processing time).
//! 3.  **Read**: Reads a fixed-size 8-byte response.
//!
//! ### Response Format
//!
//! The parsing logic (implemented in `check_response`) expects the following 8-byte packet:
//!
//! | Byte   | Name        | Description                                        |
//! | -------| :---------- | -------------------------------------------------- |
//! | `[0]`  | `MagicStart`| A status byte, expected to be `1` for success.     |
//! | `[1-6]`| `Payload`   | A 6-byte ASCII string representing the float value.|
//! | `[7]`  | `MagicEnd`  | A null terminator, expected to be `0`.             |

use crate::launch::channels::AquaChannelError;
use crate::sensors::atlas_scientific::AtlasScientific;
use crate::sensors::atlas_scientific::{AtlasScientificResult, AtlasScientificResultData};
use crate::sensors::atlas_scientific_channels::AtlasScientificChannels;
use crate::sensors::atlas_scientific_driver::AtlasScientificDriver;
use crate::sensors::atlas_scientific_error::AtlasScientificError;
use crate::sensors::atlas_scientific_error::AtlasScientificError::SignalRequestCreationFailure;
use crate::sensors::i2c_interface::I2cRequest;
use crate::utilities::channel_content::AquariumSignal;
use crate::utilities::check_mutex_access_duration::CheckMutexAccessDurationTrait;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[allow(dead_code)]
/// numeric values used in communication with Atlas Scientific sensor units
pub mod atlas_scientific_ezo_constants {
    /// command sent to sensor unit for initiating response
    pub const COMMAND_READ: u8 = b'R';

    /// length of the message received from the sensor (number of bytes)
    pub const MESSAGE_SIZE: usize = 8;

    /// location of the first character to be checked inside the message from the sensor
    pub const INDEX_FIRST_CHARACTER: usize = 0;

    /// start of data inside the message from the sensor
    pub const INDEX_CONTENT_START: usize = 1;

    /// finish of data inside the message from the sensor
    pub const INDEX_PENULTIMATE_CHARACTER: usize = 6;

    /// location of the last character to be checked inside the message from the sensor
    pub const INDEX_LAST_CHARACTER: usize = 7;

    /// ASCII value for representation of figure zero
    pub const ASCII_VAL_ZERO: u8 = 48;

    /// ASCII value for representation of figure nine
    pub const ASCII_VAL_NINE: u8 = 57;

    /// decimal point
    pub const DECIMAL_POINT: u8 = b'.';

    /// negative sign (note: positive sign does not occur in the message from the sensor)
    pub const NEGATIVE_SIGN: u8 = b'-';

    /// magic number at front position inside the message which identifies valid response from sensor unit
    pub const MESSAGE_START_MAGIC: u8 = 1;

    /// magic number at finish position inside the message which identifies valid response from sensor unit
    pub const MESSAGE_FINISH_MAGIC: u8 = 0;
}

/// Driver implementation for Atlas Scientific EZO sensors.
///
/// This struct holds the configuration (I2C address, sleep times) and state
/// required to interact with an EZO circuit. It implements the `AtlasScientificDriver`
/// trait to integrate with the main `AtlasScientific` sensor loop.
#[derive(Debug)]
pub struct AtlasScientificEzo {
    /// I2C address of the device
    pub i2c_address: u16,

    /// sleep time in milliseconds applied between writing and reading from the device
    pub sleep_time_millis: u64,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for water temperature
    pub lock_warn_max_mutex_access_duration_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for pH
    pub lock_warn_max_mutex_access_duration_ph: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for conductivity
    pub lock_warn_max_mutex_access_duration_conductivity: bool,

    // for testing purposes: record when mutex access time is exceeded without resetting it
    #[cfg(any(test, feature = "test-utils"))]
    pub mutex_temperature_access_duration_exceeded: bool,
    #[cfg(any(test, feature = "test-utils"))]
    pub mutex_ph_access_duration_exceeded: bool,
    #[cfg(any(test, feature = "test-utils"))]
    pub mutex_conductivity_access_duration_exceeded: bool,

    pub max_mutex_access_duration: Duration,
}

impl AtlasScientificEzo {
    /// Creates a new instance of the EZO driver.
    ///
    /// # Arguments
    /// * `i2c_address` - The I2C address of the EZO circuit.
    /// * `sleep_time_millis` - The time to wait between sending the read command and reading the response.
    /// * `max_mutex_access_duration_millis` - Threshold for logging mutex contention warnings.
    ///
    /// [impl->swreq~sensors_read_data~1]
    pub fn new(
        i2c_address: u16,
        sleep_time_millis: u64,
        max_mutex_access_duration_millis: u64,
    ) -> Self {
        AtlasScientificEzo {
            i2c_address,
            sleep_time_millis,
            lock_warn_max_mutex_access_duration_temperature: false,
            lock_warn_max_mutex_access_duration_ph: false,
            lock_warn_max_mutex_access_duration_conductivity: false,
            #[cfg(any(test, feature = "test-utils"))]
            mutex_temperature_access_duration_exceeded: false,
            #[cfg(any(test, feature = "test-utils"))]
            mutex_ph_access_duration_exceeded: false,
            #[cfg(any(test, feature = "test-utils"))]
            mutex_conductivity_access_duration_exceeded: false,
            max_mutex_access_duration: Duration::from_millis(max_mutex_access_duration_millis),
        }
    }

    /// Checks if a given character is a valid part of a numeric string, considering optional negative signs.
    ///
    /// This private helper function is used during the parsing of sensor responses to validate
    /// that each character in the data section is either a digit (`0`-`9`), a decimal point (`.`),
    /// or, if `negative_allowed` is `true`, a negative sign (`-`).
    ///
    /// # Arguments
    /// * `character` - The `u8` byte value of the character to be checked (typically an ASCII representation).
    /// * `negative_allowed` - A `bool` flag. If `true`, a negative sign character (`-`) will be considered
    ///   valid; otherwise, it will be considered invalid.
    ///
    /// # Returns
    /// `true` if the character is a valid numeric component based on the rules; `false` otherwise.
    #[allow(unused)] // used in conditionally compiled code
    fn is_numeric(character: u8, negative_allowed: bool) -> bool {
        match character {
            atlas_scientific_ezo_constants::ASCII_VAL_ZERO
                ..=atlas_scientific_ezo_constants::ASCII_VAL_NINE => true,
            atlas_scientific_ezo_constants::DECIMAL_POINT => true,
            atlas_scientific_ezo_constants::NEGATIVE_SIGN => negative_allowed,
            _ => false,
        }
    }

    /// Evaluates a raw byte array response from an Atlas Scientific sensor unit, validating its format and converting it to `f32`.
    ///
    /// This private helper function performs several critical steps to parse sensor data:
    /// 1.  **Magic Number Check**: Verifies the protocol's start and end "magic" bytes.
    /// 2.  **Numeric Content Validation**: Ensures all characters within the data section are valid numeric components.
    /// 3.  **UTF-8 Conversion & Float Parsing**: Converts the validated byte sequence into a string and parses it into an `f32` value.
    ///
    /// The `is_ph` flag specifically controls whether a negative sign is allowed, as pH values are non-negative by definition.
    ///
    /// # Arguments
    /// * `buffer` - A fixed-size array of `u8` bytes representing the complete message received from the sensor.
    /// * `is_ph` - A `bool` flag indicating if the response is from a pH sensor (`true`) or another type (`false`).
    ///
    /// # Returns
    /// A `Result` containing the successfully parsed `f32` sensor reading.
    ///
    /// # Errors
    /// Returns an `AtlasScientificError` variant if any validation or parsing step fails:
    /// - `FirstCharacterNotMagic`: If the message's first byte is incorrect.
    /// - `LastCharacterNotMagic`: If the message's last byte is incorrect.
    /// - `ContainsInvalidCharacter`: If the data section contains non-numeric characters.
    /// - `InvalidUtf8Sequence`: If the numeric byte sequence is not valid UTF-8.
    /// - `ConversionFailure`: If the valid numeric string cannot be parsed into an `f32`.
    #[allow(unused)] // used in conditionally compiled code
    fn check_response(
        buffer: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE],
        is_ph: bool,
    ) -> Result<f32, AtlasScientificError> {
        println!("check_response buffer: {:?}", buffer);

        // first check: first character not magic
        let first_character = buffer[atlas_scientific_ezo_constants::INDEX_FIRST_CHARACTER];
        if first_character != atlas_scientific_ezo_constants::MESSAGE_START_MAGIC {
            return Err(AtlasScientificError::FirstCharacterNotMagic(
                module_path!().to_string(),
                atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
                first_character,
            ));
        }
        // second check: last character not magic
        let last_character = buffer[atlas_scientific_ezo_constants::INDEX_LAST_CHARACTER];
        if last_character != atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC {
            return Err(AtlasScientificError::LastCharacterNotMagic(
                module_path!().to_string(),
                atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
                last_character,
            ));
        }

        // get a slice of the array
        let numeric_content = &buffer[atlas_scientific_ezo_constants::INDEX_CONTENT_START
            ..atlas_scientific_ezo_constants::INDEX_PENULTIMATE_CHARACTER];

        // third check: all characters in between belong to a number
        for character in numeric_content {
            if !Self::is_numeric(*character, !is_ph) {
                return Err(AtlasScientificError::ContainsInvalidCharacter(
                    module_path!().to_string(),
                    *character,
                ));
            }
        }

        // conversion to String
        let numeric_string = str::from_utf8(numeric_content).map_err(|e| {
            AtlasScientificError::InvalidUtf8Sequence {
                location: module_path!().to_string(),
                source: e,
            }
        })?;

        // conversion to f32
        numeric_string
            .parse::<f32>()
            .map_err(|e| AtlasScientificError::ConversionFailure {
                location: module_path!().to_string(),
                source: e.clone(),
            })
    }
}

impl AtlasScientificDriver for AtlasScientificEzo {
    /// Constructs and sends a measurement request to the I2C interface thread for reading
    /// from AtlasScientific EZO sensor.
    ///
    /// This function creates an `I2cRequest` based on the specified `signal`. It
    /// retrieves the correct I2C address and sleep time from the configuration
    /// for the given sensor. The request is then sent over the provided channel
    /// to the I2C thread for execution.
    ///
    /// # Arguments
    /// * `atlas_scientific_channels` - A mutable reference to the struct containing the channels.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the measurement request was successfully created and sent.
    ///
    /// # Errors
    /// Returns an `AtlasScientificError` if any step fails:
    /// - `InvalidSignal`: If an unsupported `AquariumSignal` is provided.
    /// - `SignalRequestCreationFailure`: If the underlying `I2cRequest::new` call fails (e.g., command too long).
    /// - `SendingRequestToI2cInterfaceFailed`: If sending the request over the channel fails, which
    ///   typically means the receiver (the I2C thread) has been dropped.
    #[allow(unused)] // used in conditionally compiled code
    fn send_request_to_i2c_interface(
        &self,
        atlas_scientific_channels: &mut AtlasScientificChannels,
    ) -> Result<(), AtlasScientificError> {
        let command_buffer = [atlas_scientific_ezo_constants::COMMAND_READ];

        let request = I2cRequest::new(
            self.i2c_address,
            &command_buffer,
            self.sleep_time_millis,
            atlas_scientific_ezo_constants::MESSAGE_SIZE,
        )
        .map_err(|e| SignalRequestCreationFailure {
            location: module_path!().to_string(),
            source: e,
        })?;

        match atlas_scientific_channels
            .tx_atlas_scientific_to_i2c_interface
            .send(request)
        {
            Ok(()) => Ok(()),
            Err(e) => Err(AtlasScientificError::SendingRequestToI2cInterfaceFailed {
                location: module_path!().to_string(),
                source: e,
            }),
        }
    }

    /// Receives and processes a measurement response from the I2C interface thread.
    ///
    /// This function orchestrates the full cycle of handling a sensor reading. It performs a
    /// non-blocking receive on the I2C channel. If a message is available, it processes
    /// the result, which can be either a successful data payload or an I2C-level error.
    ///
    /// For a successful payload, the function:
    /// 1. Parses the raw byte buffer using `check_response`.
    /// 2. Converts the valid data into an `AtlasScientificResultData` struct.
    /// 3. Atomically writes this result to the correct shared `Mutex` using `write_result_to_mutex`.
    /// 4. Monitors the duration of the mutex lock, issuing a warning if it exceeds the threshold.
    ///
    /// # Arguments
    /// * `signal` - The `AquariumSignal` indicating which sensor's data is expected.
    /// * `atlas_scientific_channels` - A mutable reference to the struct containing the channels.
    /// * `mutex_water_temperature` - The shared `Mutex` for the water temperature result.
    /// * `mutex_ph` - The shared `Mutex` for the pH result.
    /// * `mutex_conductivity` - The shared `Mutex` for the conductivity result.
    ///
    /// # Returns
    /// An `AtlasScientificResult` containing the `AtlasScientificResultData` if a valid sensor
    /// reading was successfully received, parsed, and stored.
    ///
    /// # Errors
    /// Returns an `AtlasScientificError` variant if any error occurred:
    ///   - `ChannelIsEmpty`: A recoverable error indicating no message was available.
    ///   - `ChannelIsDisconnected`: A critical error indicating the I2C thread has terminated.
    ///   - `I2cCommunicationFailure`: The I2C thread reported a hardware-level communication error.
    ///   - `ResponseCheckFailed` / `IncorrectBufferLength`: The received data payload was malformed.
    ///   - `CouldNotLockMutex`: The target mutex was poisoned and could not be updated.
    #[allow(unused)] // used in conditionally compiled code
    fn receive_response_from_i2c_interface(
        &mut self,
        signal: &AquariumSignal,
        atlas_scientific_channels: &mut AtlasScientificChannels,
        mutex_water_temperature: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_ph: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_conductivity: &Arc<Mutex<AtlasScientificResultData>>,
    ) -> AtlasScientificResult {
        let is_ph = matches!(signal, AquariumSignal::pH);
        let result: AtlasScientificResult;

        match atlas_scientific_channels.receive_from_i2c_interface() {
            Ok(i2c_result) => match i2c_result {
                // Result from channel is available
                Ok(i2c_response) => {
                    // First, check if the response has the expected length for an Atlas sensor.
                    if i2c_response.length != atlas_scientific_ezo_constants::MESSAGE_SIZE {
                        result = Err(AtlasScientificError::IncorrectBufferLength(
                            module_path!().to_string(),
                            signal.clone(),
                            i2c_response.length,
                        ));
                    } else {
                        // The length is correct. Convert the relevant part of the buffer
                        // into the [u8; 8] array that `check_response` expects.
                        let response_buffer: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] =
                            i2c_response.read_buf[..atlas_scientific_ezo_constants::MESSAGE_SIZE]
                                .try_into()
                                .unwrap(); // This unwrap is safe due to the length check above.

                        // Now, parse the validated buffer.
                        let parsed_result = match Self::check_response(response_buffer, is_ph) {
                            Ok(value) => Ok(AtlasScientificResultData::new(value)),
                            Err(e) => Err(AtlasScientificError::ResponseCheckFailed {
                                location: module_path!().to_string(),
                                signal: signal.clone(),
                                source: Box::new(e),
                            }),
                        };

                        // If parsing was successful, write the result to the shared mutex.
                        if let Ok(result_data) = parsed_result {
                            let instant_before_locking_mutex = Instant::now();
                            let result_data_clone = result_data.clone();
                            let instant_after_locking_mutex =
                                AtlasScientific::write_result_to_mutex(
                                    signal,
                                    result_data,
                                    mutex_water_temperature,
                                    mutex_ph,
                                    mutex_conductivity,
                                )?; // The '?' will propagate a mutex lock error.

                            // Check if access to mutex took too long.
                            self.check_mutex_access_duration(
                                Some(signal),
                                instant_after_locking_mutex,
                                instant_before_locking_mutex,
                            );

                            // The final result of this arm is the successfully parsed data.
                            result = Ok(result_data_clone);
                        } else {
                            // If parsing failed, propagate the specific parsing error.
                            result = parsed_result;
                        }
                    }
                }
                Err(e) => {
                    result = Err(AtlasScientificError::I2cCommunicationFailure {
                        location: module_path!().to_string(),
                        signal: signal.clone(),
                        source: e,
                    })
                }
            },
            Err(e) => match e {
                #[cfg(feature = "debug_channels")]
                AquaChannelError::Full => {
                    result = Err(AtlasScientificError::UnknownError(
                        module_path!().to_string(),
                    ));
                }
                AquaChannelError::Empty => {
                    result = Err(AtlasScientificError::ChannelIsEmpty(
                        module_path!().to_string(),
                    ));
                }
                AquaChannelError::Disconnected => {
                    result = Err(AtlasScientificError::ChannelIsDisconnected {
                        location: module_path!().to_string(),
                        source: e,
                    });
                }
            },
        }
        result
    }

    /// Dummy trait implementation of `device_init` operation.
    ///
    /// # Arguments
    /// * `atlas_scientific_channels`: MPSC channel pair - not used.
    /// * `signal`: sensor type to be initialized - not used.
    fn device_init(
        &self,
        _atlas_scientific_channels: &mut AtlasScientificChannels,
        _signal: &AquariumSignal,
    ) -> Result<(), AtlasScientificError> {
        // do nothing - implementation is only required for EZO type sensor
        Ok(())
    }

    /// Retrieves the mutex access duration exceeded.
    ///
    /// # Arguments
    /// * `signal` - sensor type.
    #[cfg(any(test, feature = "test-utils"))]
    fn get_mutex_access_duration_exceeded(&self, signal: &AquariumSignal) -> bool {
        match signal {
            AquariumSignal::WaterTemperature => self.mutex_temperature_access_duration_exceeded,
            AquariumSignal::pH => self.mutex_ph_access_duration_exceeded,
            AquariumSignal::Conductivity => self.mutex_conductivity_access_duration_exceeded,
            _ => panic!("Signal type not supported."),
        }
    }
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub mod tests {
    // unit tests shall not appear in code coverage measurement
    use crate::sensors::atlas_scientific_error::AtlasScientificError;
    use crate::sensors::atlas_scientific_ezo::{
        atlas_scientific_ezo_constants, AtlasScientificEzo,
    };
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    use crate::sensors::i2c_interface::I2cInterface;
    use assert_float_eq::*;

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    use std::io;

    #[test]
    // happy case for evaluating response from pH sensor unit
    pub fn test_atlas_scientific_ezo_check_response_valid_ph() {
        let stimuli: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
            atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
            b'8',
            atlas_scientific_ezo_constants::DECIMAL_POINT,
            b'0',
            b'0',
            b'0',
            b'0',
            atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
        ];
        let result = AtlasScientificEzo::check_response(stimuli, true);
        match result {
            Ok(c) => {
                assert_float_absolute_eq!(c, 8.0, 0.001);
            }
            Err(e) => {
                panic!("test_atlas_scientific_ezo_check_response_valid_ph returned error: {e:?}");
            }
        }
    }

    #[test]
    // evaluating response from pH sensor unit
    // checking if negative values are correctly identified
    pub fn test_atlas_scientific_ezo_check_response_invalid_ph_negative() {
        let sign = b'-';

        let stimuli: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
            atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
            sign,
            b'8',
            atlas_scientific_ezo_constants::DECIMAL_POINT,
            b'0',
            b'0',
            b'0',
            atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
        ];

        let result = AtlasScientificEzo::check_response(stimuli, true);

        let error_result = result.unwrap_err();

        assert!(matches!(
            error_result,
            AtlasScientificError::ContainsInvalidCharacter(_, _sign)
        ));
    }

    #[test]
    // happy case for evaluating response from conductivity sensor
    pub fn test_atlas_scientific_ezo_check_response_valid_conductivity() {
        let stimuli: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
            atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
            b'5',
            b'0',
            b'0',
            b'0',
            b'0',
            b'0',
            atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
        ];
        let result = AtlasScientificEzo::check_response(stimuli, false);

        match result {
            Ok(c) => {
                assert_float_absolute_eq!(c, 50000.0, 0.001);
            }
            Err(e) => {
                panic!("test_atlas_scientific_ezo_check_response_valid_conductivity returned error: {e:?}");
            }
        }
    }

    // happy case for evaluating the response from the temperature sensor unit
    #[test]
    // Verifies that atlas scientific ezo check response valid temperature.
    pub fn test_atlas_scientific_ezo_check_response_valid_temperature() {
        let stimuli: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
            atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
            b'2',
            b'5',
            atlas_scientific_ezo_constants::DECIMAL_POINT,
            b'0',
            b'0',
            b'0',
            atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
        ];
        let result = AtlasScientificEzo::check_response(stimuli, false);
        match result {
            Ok(c) => {
                assert_float_absolute_eq!(c, 25.0, 0.001);
            }
            Err(e) => {
                panic!(
                    "test_atlas_scientific_ezo_check_response_valid_temperature returned error: {e:?}"
                );
            }
        }
    }

    #[test]
    // checking if an incorrect magic number in the front of the message is identified correctly
    pub fn test_atlas_scientific_ezo_check_response_invalid_magic_front() {
        let incorrect_magic_front = atlas_scientific_ezo_constants::MESSAGE_START_MAGIC - 1;

        let stimuli: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
            incorrect_magic_front,
            b'2',
            b'5',
            atlas_scientific_ezo_constants::DECIMAL_POINT,
            b'0',
            b'0',
            b'0',
            atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
        ];

        let result = AtlasScientificEzo::check_response(stimuli, false);

        let error_result = result.unwrap_err();

        assert!(matches!(
            error_result,
            AtlasScientificError::FirstCharacterNotMagic(
                _,
                atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
                _incorrect_magic_front
            )
        ));
    }

    #[test]
    // checking if an incorrect magic number in the rear part of the message is identified correctly
    pub fn test_atlas_scientific_check_response_invalid_magic_rear() {
        let incorrect_magic_rear = atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC + 1;

        let stimuli: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
            atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
            b'2',
            b'5',
            atlas_scientific_ezo_constants::DECIMAL_POINT,
            b'0',
            b'0',
            b'0',
            incorrect_magic_rear,
        ];

        let result = AtlasScientificEzo::check_response(stimuli, false);

        let error_result = result.unwrap_err();

        assert!(matches!(
            error_result,
            AtlasScientificError::LastCharacterNotMagic(
                _,
                atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
                _incorrect_magic_rear
            )
        ));
    }

    #[test]
    // checking if an incorrect sequence of numeric characters is identified correctly
    pub fn test_atlas_scientific_ezo_check_response_invalid_conversion() {
        let stimuli: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
            atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
            b'2',
            b'5',
            atlas_scientific_ezo_constants::NEGATIVE_SIGN,
            b'0',
            b'0',
            b'0',
            atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
        ];
        assert!(matches!(
            AtlasScientificEzo::check_response(stimuli, false),
            Err(AtlasScientificError::ConversionFailure {
                location: _,
                source: _
            })
        ));
    }

    #[test]
    // Test case checks if the check_response function correctly identifies an
    // invalid non-numeric character (like an alphabet letter) in the data payload.
    fn test_atlas_scientific_ezo_check_response_invalid_character_in_payload() {
        let stimuli: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
            atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
            b'2',
            b'5',
            atlas_scientific_ezo_constants::DECIMAL_POINT,
            b'A', // Invalid character
            b'0',
            b'0',
            atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
        ];
        let result = AtlasScientificEzo::check_response(stimuli, false);

        let error_result = result.unwrap_err();

        assert!(matches!(
            error_result,
            AtlasScientificError::ContainsInvalidCharacter(_, b'A')
        ));
    }

    #[test]
    // Test case checks if the is_numeric function correctly identifies valid digits.
    fn test_is_numeric_with_valid_digits() {
        // Test with '0'
        assert!(AtlasScientificEzo::is_numeric(b'0', false));
        assert!(AtlasScientificEzo::is_numeric(b'0', true));

        // Test with '5'
        assert!(AtlasScientificEzo::is_numeric(b'5', false));
        assert!(AtlasScientificEzo::is_numeric(b'5', true));

        // Test with '9'
        assert!(AtlasScientificEzo::is_numeric(b'9', false));
        assert!(AtlasScientificEzo::is_numeric(b'9', true));
    }

    #[test]
    // Test case checks if the is_numeric function correctly identifies the decimal point.
    fn test_is_numeric_with_decimal_point() {
        assert!(AtlasScientificEzo::is_numeric(b'.', false));
        assert!(AtlasScientificEzo::is_numeric(b'.', true));
    }

    #[test]
    // Test case checks if the is_numeric function correctly handles the negative sign
    // when negative values are allowed.
    fn test_is_numeric_with_negative_sign_allowed() {
        assert!(AtlasScientificEzo::is_numeric(b'-', true));
    }

    #[test]
    // Test case checks if the is_numeric function correctly handles the negative sign
    // when negative values are not allowed (e.g., for pH values).
    fn test_is_numeric_with_negative_sign_not_allowed() {
        assert!(!AtlasScientificEzo::is_numeric(b'-', false));
    }

    #[test]
    // Test case checks if the is_numeric function correctly identifies various
    // invalid characters that are not part of a number.
    fn test_is_numeric_with_invalid_characters() {
        // Test with an alphabet character
        assert!(!AtlasScientificEzo::is_numeric(b'a', false));
        assert!(!AtlasScientificEzo::is_numeric(b'X', true));

        // Test with a special symbol
        assert!(!AtlasScientificEzo::is_numeric(b'?', false));
        assert!(!AtlasScientificEzo::is_numeric(b'#', true));

        // Test with a space
        assert!(!AtlasScientificEzo::is_numeric(b' ', false));
        assert!(!AtlasScientificEzo::is_numeric(b' ', true));

        // Test with a newline character
        assert!(!AtlasScientificEzo::is_numeric(b'\n', false));
        assert!(!AtlasScientificEzo::is_numeric(b'\n', true));
    }

    #[test]
    // Verifies that send_request_to_i2c_interface returns an error when the I2C channel is disconnected.
    fn test_send_request_to_i2c_interface_send_failed() {
        use crate::launch::channels::channel;
        use crate::sensors::atlas_scientific_channels::AtlasScientificChannels;
        use crate::sensors::atlas_scientific_driver::AtlasScientificDriver;
        use crate::sensors::atlas_scientific_monitor_view::AtlasScientificMonitorView;
        use crate::sensors::i2c_interface::I2cRequest;

        let ezo = AtlasScientificEzo::new(0x66, 10, 10);

        let (tx_i2c, rx_i2c) = channel::<I2cRequest>(10, false);
        drop(rx_i2c); // Drop receiver so channel send fails

        let (_tx_i2c_res, rx_i2c_res) = channel(10, false);
        let (_tx_sig, rx_sig) = channel(10, false);
        let (tx_ack, _rx_ack) = channel(10, false);
        let (tx_mon, _rx_mon) = channel::<AtlasScientificMonitorView>(10, false);

        let mut channels = AtlasScientificChannels {
            tx_atlas_scientific_to_i2c_interface: tx_i2c,
            rx_atlas_scientific_from_i2c_interface: rx_i2c_res,
            tx_atlas_scientific_to_signal_handler: tx_ack,
            rx_atlas_scientific_from_signal_handler: rx_sig,
            tx_atlas_scientific_to_monitors: tx_mon,
        };

        let res = ezo.send_request_to_i2c_interface(&mut channels);
        assert!(matches!(
            res,
            Err(AtlasScientificError::SendingRequestToI2cInterfaceFailed { .. })
        ));
    }

    #[test]
    // Verifies that receive_response_from_i2c_interface handles channel empty, length mismatch, check failure, I2C failure, and disconnection errors.
    fn test_receive_response_from_i2c_interface_error_branches() {
        use crate::launch::channels::channel;
        use crate::sensors::atlas_scientific::AtlasScientificResultData;
        use crate::sensors::atlas_scientific_channels::AtlasScientificChannels;
        use crate::sensors::atlas_scientific_driver::AtlasScientificDriver;
        use crate::sensors::atlas_scientific_monitor_view::AtlasScientificMonitorView;
        use crate::sensors::i2c_error::I2cError;
        use crate::sensors::i2c_interface::{I2cResponse, I2cResult};
        use crate::utilities::channel_content::AquariumSignal;
        use std::sync::{Arc, Mutex};

        let mut ezo = AtlasScientificEzo::new(0x66, 10, 10);

        let (tx_i2c, _rx_i2c) = channel(10, false);
        let (mut tx_i2c_res, rx_i2c_res) = channel::<I2cResult>(10, false);
        let (_tx_sig, rx_sig) = channel(10, false);
        let (tx_ack, _rx_ack) = channel(10, false);
        let (tx_mon, _rx_mon) = channel::<AtlasScientificMonitorView>(10, false);

        let mut channels = AtlasScientificChannels {
            tx_atlas_scientific_to_i2c_interface: tx_i2c,
            rx_atlas_scientific_from_i2c_interface: rx_i2c_res,
            tx_atlas_scientific_to_signal_handler: tx_ack,
            rx_atlas_scientific_from_signal_handler: rx_sig,
            tx_atlas_scientific_to_monitors: tx_mon,
        };

        let mutex_temp = Arc::new(Mutex::new(AtlasScientificResultData::new(25.0)));
        let mutex_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(7.0)));
        let mutex_cond = Arc::new(Mutex::new(AtlasScientificResultData::new(50000.0)));

        // 1. ChannelIsEmpty
        let res_empty = ezo.receive_response_from_i2c_interface(
            &AquariumSignal::WaterTemperature,
            &mut channels,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );
        assert!(matches!(
            res_empty,
            Err(AtlasScientificError::ChannelIsEmpty(_))
        ));

        // 2. IncorrectBufferLength (length = 4 != 8)
        let response_short = I2cResponse {
            read_buf: [0u8; 64],
            length: 4,
        };
        tx_i2c_res.send(Ok(response_short)).unwrap();
        let res_short = ezo.receive_response_from_i2c_interface(
            &AquariumSignal::WaterTemperature,
            &mut channels,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );
        assert!(matches!(
            res_short,
            Err(AtlasScientificError::IncorrectBufferLength(_, _, 4))
        ));

        // 3. ResponseCheckFailed (non-numeric payload)
        let mut bad_buf = [0u8; 64];
        bad_buf[0] = 1; // start magic
        bad_buf[1] = b'A'; // invalid character
        bad_buf[7] = 0; // finish magic
        let response_bad = I2cResponse {
            read_buf: bad_buf,
            length: 8,
        };
        tx_i2c_res.send(Ok(response_bad)).unwrap();
        let res_bad = ezo.receive_response_from_i2c_interface(
            &AquariumSignal::WaterTemperature,
            &mut channels,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );
        assert!(matches!(
            res_bad,
            Err(AtlasScientificError::ResponseCheckFailed { .. })
        ));

        // 4. I2cCommunicationFailure
        tx_i2c_res
            .send(Err(I2cError::WriteFailure {
                location: module_path!().to_string(),
            }))
            .unwrap();
        let res_i2c_err = ezo.receive_response_from_i2c_interface(
            &AquariumSignal::WaterTemperature,
            &mut channels,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );
        assert!(matches!(
            res_i2c_err,
            Err(AtlasScientificError::I2cCommunicationFailure { .. })
        ));

        // 5. ChannelIsDisconnected
        drop(tx_i2c_res);
        let res_disc = ezo.receive_response_from_i2c_interface(
            &AquariumSignal::WaterTemperature,
            &mut channels,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );
        assert!(matches!(
            res_disc,
            Err(AtlasScientificError::ChannelIsDisconnected { .. })
        ));
    }

    #[test]
    // Verifies device_init execution and get_mutex_access_duration_exceeded reporting for pH and Conductivity signals.
    fn test_device_init_and_mutex_duration_exceeded() {
        use crate::launch::channels::channel;
        use crate::sensors::atlas_scientific_channels::AtlasScientificChannels;
        use crate::sensors::atlas_scientific_driver::AtlasScientificDriver;
        use crate::sensors::atlas_scientific_monitor_view::AtlasScientificMonitorView;
        use crate::utilities::channel_content::AquariumSignal;

        let mut ezo = AtlasScientificEzo::new(0x66, 10, 10);

        let (tx_i2c, _rx_i2c) = channel(10, false);
        let (_tx_i2c_res, rx_i2c_res) = channel(10, false);
        let (_tx_sig, rx_sig) = channel(10, false);
        let (tx_ack, _rx_ack) = channel(10, false);
        let (tx_mon, _rx_mon) = channel::<AtlasScientificMonitorView>(10, false);

        let mut channels = AtlasScientificChannels {
            tx_atlas_scientific_to_i2c_interface: tx_i2c,
            rx_atlas_scientific_from_i2c_interface: rx_i2c_res,
            tx_atlas_scientific_to_signal_handler: tx_ack,
            rx_atlas_scientific_from_signal_handler: rx_sig,
            tx_atlas_scientific_to_monitors: tx_mon,
        };

        // device_init returns Ok(())
        assert!(ezo
            .device_init(&mut channels, &AquariumSignal::WaterTemperature)
            .is_ok());

        // get_mutex_access_duration_exceeded for pH and Conductivity
        ezo.mutex_ph_access_duration_exceeded = true;
        ezo.mutex_conductivity_access_duration_exceeded = true;

        assert!(ezo.get_mutex_access_duration_exceeded(&AquariumSignal::pH));
        assert!(ezo.get_mutex_access_duration_exceeded(&AquariumSignal::Conductivity));
    }
}
