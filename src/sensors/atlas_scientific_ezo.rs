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
pub(crate) mod atlas_scientific_ezo_constants {
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

pub struct AtlasScientificEzo {
    pub i2c_address: u16,

    pub sleep_time_millis: u64,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for water temperature
    pub lock_warn_max_mutex_access_duration_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for pH
    pub lock_warn_max_mutex_access_duration_ph: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for conductivity
    pub lock_warn_max_mutex_access_duration_conductivity: bool,

    // for testing purposes: record when mutex access time is exceeded without resetting it
    #[cfg(test)]
    pub mutex_temperature_access_duration_exceeded: bool,
    #[cfg(test)]
    pub mutex_ph_access_duration_exceeded: bool,
    #[cfg(test)]
    pub mutex_conductivity_access_duration_exceeded: bool,

    pub max_mutex_access_duration: Duration,
}

impl AtlasScientificEzo {
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
            #[cfg(test)]
            mutex_temperature_access_duration_exceeded: false,
            #[cfg(test)]
            mutex_ph_access_duration_exceeded: false,
            #[cfg(test)]
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
}
