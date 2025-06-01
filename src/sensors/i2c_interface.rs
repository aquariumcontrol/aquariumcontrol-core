/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::{str, time::Duration};

use crate::utilities::channel_content::AquariumSignal;

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
        use log::{error, warn};
        use std::sync::mpsc::{Sender, Receiver};
        use rppal::i2c::I2c; // required for communication with AtlasScientific sensors

        #[cfg(feature = "debug_i2c_interface")]
        use log::debug;

        use crate::utilities::channel_content::InternalCommand;
    }
}

/// Holds the configuration data for the I2C interface.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
/// Struct is intentionally placed here instead of create i2c_interface.
#[derive(Deserialize, Debug)]
pub struct I2cInterfaceConfig {
    /// I2C address of Atlas Scientific pH sensor unit
    #[allow(unused)]
    address_atlas_scientific_ph: u16,

    /// I2C address of Atlas Scientific conductivity sensor unit
    #[allow(unused)]
    address_atlas_scientific_conductivity: u16,

    /// I2C address of Atlas Scientific temperature sensor unit    
    #[allow(unused)]
    address_atlas_scientific_temperature: u16,

    /// maximum number of read attempts
    #[allow(unused)]
    max_read_attempts: u16,

    /// timeout for transactions in milliseconds
    #[allow(unused)]
    timeout_transaction: u32,

    /// sleep time in milliseconds before after writing before reading from the pH sensor unit
    #[allow(unused)]
    sleep_time_atlas_scientific_ph: u64,

    /// sleep time in milliseconds before after writing before reading from the conductivity sensor unit
    #[allow(unused)]
    sleep_time_atlas_scientific_conductivity: u64,

    /// sleep time in milliseconds before after writing before reading from the temperature sensor unit
    #[allow(unused)]
    sleep_time_atlas_scientific_temperature: u64,

    /// maximum allowed value for pH used to assess if the temperature sensor reading is valid
    #[allow(unused)]
    max_val_atlas_scientific_ph: f32,

    /// minimum allowed value for pH used to assess if the temperature sensor reading is valid
    #[allow(unused)]
    min_val_atlas_scientific_ph: f32,

    /// maximum allowed value for temperature used to assess if the temperature sensor reading is valid
    #[allow(unused)]
    max_val_atlas_scientific_temperature: f32,

    /// minimum allowed value for temperature used to assess if the temperature sensor reading is valid
    #[allow(unused)]
    min_val_atlas_scientific_temperature: f32,

    /// maximum allowed value for conductivity used to assess if the conductivity sensor reading is valid
    #[allow(unused)]
    max_val_atlas_scientific_conductivity: f32,

    /// minimum allowed value for conductivity used to assess if the conductivity sensor reading is valid
    #[allow(unused)]
    min_val_atlas_scientific_conductivity: f32,
}

/// error codes for evaluation of sensor unit response
#[allow(dead_code)]
#[derive(Debug, PartialEq)]
pub enum AtlasScientificError {
    /// The first character in the message sent from the sensor does not match the required magic number.
    FirstCharacterNotMagic,

    /// The last character in the message sent from the sensor does not match the required magic number.
    LastCharacterNotMagic,

    /// The message sent from the sensor unit contains non-numeric characters in the data section
    ContainsInvalidCharacter,

    /// The error is masked by other checks. Still, the signature of the function to convert from u8 array slice to String throws an error.
    InvalidUtf8Sequence,

    /// Conversion from String to f32 failed.
    ConversionFailure,
}

/// communication error codes
#[derive(Debug, PartialEq)]
pub enum I2cError {
    /// the message could not be sent
    #[allow(unused)]
    WriteFailure,

    /// command could not be sent completely to the sensor unit
    #[allow(unused)]
    InsufficientWrite,

    /// response from sensor unit could not be received completely
    #[allow(unused)]
    InsufficientRead,

    /// wrapper for AtlasScientificError
    #[allow(unused)]
    ResponseEvaluationFailure(AtlasScientificError),

    /// Represents a failure to set the address (encoded in `.0`) of the I2C sensor that would later have data requested from it.
    #[allow(unused)]
    SetAddressError(u16),

    /// This crate cannot provide the requested signal.
    #[allow(unused)]
    InvalidSignalRequest(AquariumSignal),
}

#[allow(dead_code)]
/// numeric values used in communication with Atlas Scientific sensor units
mod atlas_scientific_constants {
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

#[allow(dead_code)]
/// Contains the configuration and the implementation for communication with **Atlas Scientific sensors via I2C**.
/// Alternatively, TCP communication is used when configured to run with the simulator.
/// The struct holds attributes for the results and for error flags indicating unsuccessful communication.
/// Sensor data is read periodically and stored.
/// Requests from data logger, ventilation control, and heating control are answered with last measured data.
pub struct I2cInterface {
    /// I2C interface of rppal library
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    i2c: I2c,

    /// sleep duration for waiting response of Atlas Scientific conductivity sensor
    sleep_duration_conductivity: Duration,

    /// sleep duration for waiting response of Atlas Scientific ph sensor
    sleep_duration_ph: Duration,

    /// sleep duration for waiting response of Atlas Scientific temperature sensor
    sleep_duration_temperature: Duration,

    /// configuration for I2C interface    
    config: I2cInterfaceConfig,

    /// spin sleeper for waiting response of sensor unit
    spin_sleeper: SpinSleeper,
}

impl I2cInterface {
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    const LOG_TARGET_I2C_INTERFACE: &'static str = "i2c_interface";

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Creates a new **`I2cInterface`** instance, establishing and configuring the I2C bus for sensor communication.
    ///
    /// This constructor initializes the I2C interface on target hardware. It opens the I2C bus,
    /// sets a global transaction timeout, and pre-calculates specific sleep durations for each Atlas Scientific
    /// sensor type (pH, conductivity, temperature) to optimize response waiting times.
    ///
    /// # Arguments
    /// * `config` - **Configuration data** for the I2C interface, including sensor I2C addresses,
    ///   transaction timeouts, and per-sensor sleep times.
    ///
    /// # Returns
    /// A new **`I2cInterface` struct**, ready to communicate with Atlas Scientific sensors.
    ///
    /// # Panics
    /// This function will **panic** if:
    /// - It **fails to open the I2C bus** (e.g., I2C is not enabled, permissions issues).
    /// - It **fails to set the global transaction timeout** for the I2C bus.
    pub fn new(config: I2cInterfaceConfig) -> I2cInterface {
        I2cInterface {
            i2c: {
                let i2c = match I2c::new() {
                    Ok(c) => c,
                    Err(e) => {
                        panic!(
                            "{}: initialization of I2C interface failed ({e:?})",
                            Self::LOG_TARGET_I2C_INTERFACE
                        );
                    }
                };
                match i2c.set_timeout(config.timeout_transaction) {
                    Ok(_) => i2c,
                    Err(_) => {
                        panic!(
                            "{}: could not set timeout for transactions.",
                            Self::LOG_TARGET_I2C_INTERFACE
                        );
                    }
                }
            },
            // setup durations for each sensor individually
            sleep_duration_conductivity: Duration::from_millis(
                config.sleep_time_atlas_scientific_conductivity,
            ),
            sleep_duration_ph: Duration::from_millis(config.sleep_time_atlas_scientific_ph),

            sleep_duration_temperature: Duration::from_millis(
                config.sleep_time_atlas_scientific_temperature,
            ),

            spin_sleeper: SpinSleeper::default(),

            config,
        }
    }

    #[allow(dead_code)]
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
    fn is_numeric(character: u8, negative_allowed: bool) -> bool {
        match character {
            atlas_scientific_constants::ASCII_VAL_ZERO
                ..=atlas_scientific_constants::ASCII_VAL_NINE => true,
            atlas_scientific_constants::DECIMAL_POINT => true,
            atlas_scientific_constants::NEGATIVE_SIGN => negative_allowed,
            _ => false,
        }
    }

    #[allow(dead_code)]
    /// Evaluates a raw byte array response from an Atlas Scientific sensor unit, validating its format and converting it to `f32`.
    ///
    /// This private helper function performs several critical steps to parse sensor data:
    /// 1.  **Magic Number Check**: Verifies the protocol's start and end "magic" bytes.
    /// 2.  **Numeric Content Validation**: Ensures all characters within the data section are valid numeric components (digits, decimal point, and optionally negative sign).
    /// 3.  **UTF-8 Conversion**: Converts the validated numeric byte sequence into a `String`.
    /// 4.  **Float Parsing**: Attempts to parse the `String` into an `f32` floating-point value.
    ///
    /// The `is_ph` flag specifically controls whether a negative sign is allowed, as pH values are non-negative by definition.
    ///
    /// # Arguments
    /// * `buffer` - A fixed-size array of `u8` bytes representing the complete message received from the Atlas Scientific sensor.
    /// * `is_ph` - A `bool` flag indicating whether the response is expected from a pH sensor (`true`) or another type (`false`).
    ///
    /// # Returns
    /// A `Result<f32, AtlasScientificError>`:
    /// - `Ok(f32)`: The successfully parsed floating-point sensor reading.
    /// - `Err(AtlasScientificError::FirstCharacterNotMagic)`: If the message's first byte is incorrect.
    /// - `Err(AtlasScientificError::LastCharacterNotMagic)`: If the message's last byte is incorrect.
    /// - `Err(AtlasScientificError::ContainsInvalidCharacter)`: If the data section contains non-numeric characters (or an unexpected negative sign for pH).
    /// - `Err(AtlasScientificError::InvalidUtf8Sequence)`: If the numeric byte sequence is not valid UTF-8.
    /// - `Err(AtlasScientificError::ConversionFailure)`: If the valid numeric string cannot be parsed into an `f32`.
    fn check_response(
        buffer: [u8; atlas_scientific_constants::MESSAGE_SIZE],
        is_ph: bool,
    ) -> Result<f32, AtlasScientificError> {
        // first check: first character not magic
        if buffer[atlas_scientific_constants::INDEX_FIRST_CHARACTER]
            != atlas_scientific_constants::MESSAGE_START_MAGIC
        {
            return Err(AtlasScientificError::FirstCharacterNotMagic);
        }
        // second check: last character not magic
        if buffer[atlas_scientific_constants::INDEX_LAST_CHARACTER]
            != atlas_scientific_constants::MESSAGE_FINISH_MAGIC
        {
            return Err(AtlasScientificError::LastCharacterNotMagic);
        }

        // get a slice of the array
        let numeric_content = &buffer[atlas_scientific_constants::INDEX_CONTENT_START
            ..atlas_scientific_constants::INDEX_PENULTIMATE_CHARACTER];

        // third check: all characters in between belong to a number
        for character in numeric_content {
            if !Self::is_numeric(*character, !is_ph) {
                return Err(AtlasScientificError::ContainsInvalidCharacter);
            }
        }

        // conversion to String
        let numeric_string = match str::from_utf8(numeric_content) {
            Ok(c) => c,
            Err(_) => return Err(AtlasScientificError::InvalidUtf8Sequence),
        };

        // conversion to f32
        match &numeric_string.parse::<f32>() {
            Ok(c) => Ok(*c),
            Err(_) => Err(AtlasScientificError::ConversionFailure),
        }
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Reads a specific sensor signal from an Atlas Scientific sensor unit via I2C.
    ///
    /// This function performs a complete I2C communication cycle to retrieve a sensor reading:
    /// 1.  Selects the correct I2C address based on the `AquariumSignal` type.
    /// 2.  Sends a "read" command to the sensor unit.
    /// 3.  Pauses for a signal-specific duration to allow the sensor to process the command.
    /// 4.  Reads the response byte array from the sensor.
    /// 5.  Validates and parses the raw response using `check_response` to convert it into an `f32` value.
    ///
    /// Errors encountered at any stage (e.g., I2C communication issues, invalid signal requests,
    /// or malformed sensor responses) are logged and returned as `I2cError` variants.
    ///
    /// # Arguments
    /// * `signal` - An `AquariumSignal` enum value (e.g., `pH`, `Conductivity`, `WaterTemperature`)
    ///   describing which sensor reading is requested.
    ///
    /// # Returns
    /// A `Result<f32, I2cError>`:
    /// - `Ok(f32)`: The successfully read and parsed sensor value.
    /// - `Err(I2cError::InvalidSignalRequest)`: If the `signal` type is not supported by this interface.
    /// - `Err(I2cError::SetAddressError)`: If setting the I2C slave address fails.
    /// - `Err(I2cError::WriteFailure)`: If the command cannot be written to the sensor.
    /// - `Err(I2cError::InsufficientWrite)`: If the command write is incomplete.
    /// - `Err(I2cError::InsufficientRead)`: If the full response cannot be read from the sensor.
    /// - `Err(I2cError::ResponseEvaluationFailure)`: If the sensor's response message is malformed or invalid.
    pub fn get_response_from_i2c(&mut self, signal: AquariumSignal) -> Result<f32, I2cError> {
        let i2c_address = match signal {
            AquariumSignal::pH => self.config.address_atlas_scientific_ph,
            AquariumSignal::Conductivity => self.config.address_atlas_scientific_conductivity,
            AquariumSignal::WaterTemperature => self.config.address_atlas_scientific_temperature,
            _ => {
                warn!(
                    target: Self::LOG_TARGET_I2C_INTERFACE,
                    "ignoring invalid signal request ({})",
                    signal
                );
                return Err(I2cError::InvalidSignalRequest(signal));
            }
        };

        match self.i2c.set_slave_address(i2c_address) {
            Ok(_) => { /* do nothing */ }
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_I2C_INTERFACE,
                    "could not set {} as address of device ({e:?})",
                    i2c_address
                );
                return Err(I2cError::SetAddressError(i2c_address));
            }
        }

        let send_buf: [u8; 1] = [atlas_scientific_constants::COMMAND_READ; 1];

        match self.i2c.write(&send_buf) {
            Ok(c) => {
                if c != 1 {
                    error!(
                        target: Self::LOG_TARGET_I2C_INTERFACE,
                        "could not write command to sensor unit for {}",
                        signal
                    );
                    return Err(I2cError::InsufficientWrite);
                }
            }
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_I2C_INTERFACE,
                    "writing to device {} failed {e:?}",
                    signal
                );
                return Err(I2cError::WriteFailure);
            }
        }

        let sleep_duration = match signal {
            AquariumSignal::pH => self.sleep_duration_ph,
            AquariumSignal::Conductivity => self.sleep_duration_conductivity,
            AquariumSignal::WaterTemperature => self.sleep_duration_temperature,
            _ => {
                warn!(
                    target: Self::LOG_TARGET_I2C_INTERFACE,
                    "sleep duration for signal {} not defined - using arbitrary sleep duration of {}",
                    signal,
                    self.sleep_duration_temperature.as_millis());
                self.sleep_duration_temperature
            }
        };

        self.spin_sleeper.sleep(sleep_duration);

        let mut read_buffer: [u8; atlas_scientific_constants::MESSAGE_SIZE] =
            [0; atlas_scientific_constants::MESSAGE_SIZE];

        match self.i2c.read(&mut read_buffer) {
            Ok(_) => { /* do nothing */ }
            Err(_) => {
                error!(
                    target: Self::LOG_TARGET_I2C_INTERFACE,
                    "did not receive full response from sensor unit for {}",
                    signal
                );
                return Err(I2cError::InsufficientRead);
            }
        }

        let is_ph = matches!(signal, AquariumSignal::pH);

        match Self::check_response(read_buffer, is_ph) {
            Ok(c) => Ok(c),
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_I2C_INTERFACE,
                    "error in sensor unit response for {}",
                    signal
                );
                Err(I2cError::ResponseEvaluationFailure(e))
            }
        }
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Retrieves all three sensor readings (water temperature, pH, and conductivity) sequentially from Atlas Scientific sensors.
    ///
    /// This private helper function calls `get_response_from_i2c` for each of the three
    /// sensor types in order. If any individual sensor reading fails (returns an `Err`),
    /// this function immediately propagates that error, stopping further measurements.
    /// If all three measurements are successful, their values are returned in a tuple.
    ///
    /// # Returns
    /// A `Result<(f32, f32, f32), I2cError>`:
    /// - `Ok((f32, f32, f32))`: A tuple containing the measured water temperature, pH, and conductivity (in that order).
    /// - `Err(I2cError)`: If any of the sequential sensor readings fails. The specific error from the failed reading will be returned.
    fn get_all_data(&mut self) -> Result<(f32, f32, f32), I2cError> {
        let measured_water_temperature =
            match self.get_response_from_i2c(AquariumSignal::WaterTemperature) {
                Ok(c) => c,
                Err(e) => {
                    return Err(e);
                }
            };
        let measured_ph = match self.get_response_from_i2c(AquariumSignal::pH) {
            Ok(c) => c,
            Err(e) => {
                return Err(e);
            }
        };
        let measured_conductivity = match self.get_response_from_i2c(AquariumSignal::Conductivity) {
            Ok(c) => c,
            Err(e) => {
                return Err(e);
            }
        };
        Ok((
            measured_water_temperature,
            measured_ph,
            measured_conductivity,
        ))
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Executes the main loop for the I2C interface thread, processing incoming requests and sending sensor data responses.
    ///
    /// This function runs indefinitely, acting as a dedicated service for communicating with
    /// Atlas Scientific I2C sensors. It continuously waits for `InternalCommand` messages
    /// from other modules (like `AtlasScientific` or the `SignalHandler`).
    ///
    /// When a `RequestSignal` command for water temperature, pH, or conductivity is received,
    /// it triggers a full sensor measurement cycle using `self.get_all_data()`. The `Result`
    /// of this measurement (containing all three sensor values or an error) is then sent back
    /// to the requesting module.
    ///
    /// The loop also handles `Quit` commands for graceful shutdown and logs errors or warnings
    /// for invalid commands or channel communication issues, using internal lock flags to prevent log flooding.
    ///
    /// # Arguments
    /// * `tx_to_atlas_scientific` - The sender channel for sending `Result<(f32, f32, f32), I2cError>`
    ///   measurement data back to the `AtlasScientific` module.
    /// * `rx_from_atlas_scientific_or_signal_handler` - The receiver channel for getting `InternalCommand`s
    ///   (e.g., `RequestSignal`, `Quit`) from the `AtlasScientific` module or the `SignalHandler`.
    pub fn execute(
        &mut self,
        tx_to_atlas_scientific: Sender<Result<(f32, f32, f32), I2cError>>,
        rx_from_atlas_scientific_or_signal_handler: Receiver<InternalCommand>,
    ) {
        let mut lock_error_invalid_command: bool = false;
        let mut lock_error_invalid_signal: bool = false;
        let mut lock_error_receive: bool = false;

        loop {
            match rx_from_atlas_scientific_or_signal_handler.recv() {
                Ok(c) => {
                    lock_error_receive = false;
                    match c {
                        InternalCommand::RequestSignal(signal_request) => {
                            lock_error_invalid_command = false;
                            // Ambient requests either temperature or humidity
                            match signal_request {
                                AquariumSignal::WaterTemperature
                                | AquariumSignal::pH
                                | AquariumSignal::Conductivity => {
                                    let measurement = self.get_all_data();
                                    _ = tx_to_atlas_scientific.send(measurement);
                                }
                                _ => {
                                    if !lock_error_invalid_signal {
                                        error!(
                                            target: Self::LOG_TARGET_I2C_INTERFACE,
                                            "Received request for invalid signal."
                                        );
                                        lock_error_invalid_signal = true;
                                    }
                                }
                            }
                        }
                        InternalCommand::Quit => {
                            #[cfg(feature = "debug_i2c_interface")]
                            debug!(
                                target: Self::LOG_TARGET_I2C_INTERFACE,
                                "Received QUIT command"
                            );

                            break;
                        }
                        _ => {
                            if !lock_error_invalid_command {
                                error!(
                                    target: Self::LOG_TARGET_I2C_INTERFACE,
                                    "Received invalid command."
                                );
                                lock_error_invalid_command = true;
                            }
                        }
                    };
                }
                Err(e) => {
                    if !lock_error_receive {
                        error!(
                            target: Self::LOG_TARGET_I2C_INTERFACE,
                            "Error in trying to receive command from Ambient or Signal handler: {e:?}"
                        );
                        lock_error_receive = true;
                    }
                }
            };
        }
    }
}

#[cfg(test)]
pub mod tests {
    use assert_float_eq::*;

    use crate::sensors::i2c_interface::{
        atlas_scientific_constants, AtlasScientificError, I2cInterface,
    };

    // feature-specific imports
    cfg_if::cfg_if! {
        if #[cfg(all(feature = "target_hw", target_os = "linux"))] {
            use crate::utilities::config::{read_config_file, ConfigData};
            use crate::utilities::channel_content::AquariumSignal;
            use crate::sensors::i2c_interface::I2cError;
        }
    }

    // happy case for evaluating response from pH sensor unit
    #[test]
    pub fn test_i2c_interface_check_response_valid_ph() {
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '8' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let result = I2cInterface::check_response(stimuli, true);
        match result {
            Ok(c) => {
                assert_float_absolute_eq!(c, 8.0, 0.001);
            }
            Err(e) => {
                panic!("i2c_interface_check_response_valid_pH returned error: {e:?}");
            }
        }
    }

    // evaluating response from pH sensor unit
    // checking if negative values are correctly identified
    #[test]
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    pub fn test_i2c_interface_check_response_invalid_ph_negative() {
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '-' as u8,
            '8' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        assert_eq!(
            I2cInterface::check_response(stimuli, true),
            Err(AtlasScientificError::ContainsInvalidCharacter)
        );
    }

    // happy case for evaluating response from conductivity sensor
    #[test]
    pub fn test_i2c_interface_check_response_valid_conductivity() {
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '5' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let result = I2cInterface::check_response(stimuli, false);
        match result {
            Ok(c) => {
                assert_float_absolute_eq!(c, 50000.0, 0.001);
            }
            Err(e) => {
                panic!("test_i2c_interface_check_response_valid_conductivity return error: {e:?}");
            }
        }
    }

    // happy case for evaluating the response from the temperature sensor unit
    #[test]
    pub fn test_i2c_interface_check_response_valid_temperature() {
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '2' as u8,
            '5' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let result = I2cInterface::check_response(stimuli, false);
        match result {
            Ok(c) => {
                assert_float_absolute_eq!(c, 25.0, 0.001);
            }
            Err(e) => {
                panic!("test_i2c_interface_check_response_valid_temperature return error: {e:?}");
            }
        }
    }

    // checking if an incorrect magic number in the front of the message is identified correctly
    #[test]
    pub fn test_i2c_interface_check_response_invalid_magic_front() {
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC - 1,
            '2' as u8,
            '5' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        assert_eq!(
            I2cInterface::check_response(stimuli, false),
            Err(AtlasScientificError::FirstCharacterNotMagic)
        );
    }

    // checking if an incorrect magic number in the rear part of the message is identified correctly
    #[test]
    pub fn test_i2c_interface_check_response_invalid_magic_rear() {
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '2' as u8,
            '5' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC + 1,
        ];
        assert_eq!(
            I2cInterface::check_response(stimuli, false),
            Err(AtlasScientificError::LastCharacterNotMagic)
        );
    }

    // checking if an incorrect sequence of numeric characters is identified correctly
    #[test]
    pub fn test_i2c_interface_check_response_invalid_conversion() {
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '2' as u8,
            '5' as u8,
            atlas_scientific_constants::NEGATIVE_SIGN,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        assert_eq!(
            I2cInterface::check_response(stimuli, false),
            Err(AtlasScientificError::ConversionFailure)
        );
    }

    // read from real Atlas Scientific hardware combined in one test case to avoid race conditions:
    // - pH sensor
    // - conductivity sensor
    // - temperature
    #[test]
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub fn test_i2c_interface_read_sensors() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        let mut i2c_interface = I2cInterface::new(config.i2c_interface);

        let result = i2c_interface.get_response_from_i2c(AquariumSignal::pH);
        match result {
            Ok(c) => {
                println!("i2c_interface: Read value from sensor. pH = {}", c);
                assert_float_absolute_eq!(c, 8.0, 3.0);
            }
            Err(e) => {
                panic!("test_i2c_interface_read_ph return error: {e:?}");
            }
        }

        let result = i2c_interface.get_response_from_i2c(AquariumSignal::Conductivity);
        match result {
            Ok(c) => {
                println!(
                    "i2c_interface: Read value from sensor. Conductivity = {} microS/cm",
                    c
                );
                assert_float_absolute_eq!(c, 50000.0, 20000.0);
            }
            Err(e) => {
                panic!("test_i2c_interface_read_temperature returned error: {e:?}");
            }
        }

        let result = i2c_interface.get_response_from_i2c(AquariumSignal::WaterTemperature);
        match result {
            Ok(c) => {
                println!(
                    "i2c_interface: Read value from sensor. Temperature = {} °C",
                    c
                );
                assert_float_absolute_eq!(c, 25.0, 10.0);
            }
            Err(e) => {
                panic!("test_i2c_interface_read_temperature returned error: {e:?}");
            }
        }
    }

    // Test the error response of get_response_from_i2c when requesting an invalid signal.
    // This test case only runs on the target hardware because it requires initialization of the I2C subsystem.
    #[test]
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub fn test_i2c_error_response_to_invalid_signal_request() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_i2c_interface.toml".to_string());
        let mut i2c_interface = I2cInterface::new(config.i2c_interface);

        let result = i2c_interface.get_response_from_i2c(AquariumSignal::MockInvalidSignal);

        assert_eq!(
            result,
            Err(I2cError::InvalidSignalRequest(
                AquariumSignal::MockInvalidSignal
            ))
        );
    }
}
