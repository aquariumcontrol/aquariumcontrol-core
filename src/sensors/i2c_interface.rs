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

    /// sleep time in milliseconds before after writing before reading from pH sensor unit
    #[allow(unused)]
    sleep_time_atlas_scientific_ph: u64,

    /// sleep time in milliseconds before after writing before reading from conductivity sensor unit
    #[allow(unused)]
    sleep_time_atlas_scientific_conductivity: u64,

    /// sleep time in milliseconds before after writing before reading from temperature sensor unit
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

    /// maximum allowed value for conducitivity used to assess if the conductivity sensor reading is valid
    #[allow(unused)]
    max_val_atlas_scientific_conductivity: f32,

    /// minimum allowed value for conducitivity used to assess if the conductivity sensor reading is valid
    #[allow(unused)]
    min_val_atlas_scientific_conductivity: f32,
}

/// error codes for evaluation of sensor unit response
#[allow(dead_code)]
#[derive(Debug, PartialEq)]
pub enum AtlasScientificError {
    /// first character in message sent from sensor unit does not match required magic number
    FirstCharacterNotMagic,

    /// last character in message sent from sensor unit does not match required magic number    
    LastCharacterNotMagic,

    /// message sent from sensor unit contains non-numeric characters in data section
    ContainsInvalidCharacter,

    /// error is masked by other checks. However signature of function to convert from u8 array slice to String throws an error.
    InvalidUtf8Sequence,

    /// conversion from String to f32 failed
    ConversionFailure,
}

/// communication error codes
#[derive(Debug, PartialEq)]
pub enum I2cError {
    /// message could not be sent
    #[allow(unused)]
    WriteFailure,

    /// command could not be sent completely to sensor unit
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

    /// Signal requested cannot be provided by this crate
    #[allow(unused)]
    InvalidSignalRequest(AquariumSignal),
}

#[allow(dead_code)]
/// numeric values used in communication with Atlas Scientific sensor units
mod atlas_scientific_constants {
    /// command sent to sensor unit for initiating response
    pub const COMMAND_READ: u8 = b'R';

    /// length of message received from sensor unit (number of byte)
    pub const MESSAGE_SIZE: usize = 8;

    /// location of first character to be checked inside message from sensor unit
    pub const INDEX_FIRST_CHARACTER: usize = 0;

    /// start of data inside message from sensor unit
    pub const INDEX_CONTENT_START: usize = 1;

    /// finish of data inside message from sensor unit
    pub const INDEX_PENULTIMATE_CHARACTER: usize = 6;

    /// location of last character to be checked inside message from sensor unit
    pub const INDEX_LAST_CHARACTER: usize = 7;

    /// ASCII value for representation of zero figure
    pub const ASCII_VAL_ZERO: u8 = 48;

    /// ASCII value for representation of figure nine
    pub const ASCII_VAL_NINE: u8 = 57;

    /// decimal point
    pub const DECIMAL_POINT: u8 = b'.';

    /// negative sign (note: positive sign does not occurr in message from sensor)
    pub const NEGATIVE_SIGN: u8 = b'-';

    /// magic number at front position inside message which identifies valid response from sensor unit
    pub const MESSAGE_START_MAGIC: u8 = 1;

    /// magic number at finish position inside message which identifies valid response from sensor unit
    pub const MESSAGE_FINISH_MAGIC: u8 = 0;
}

#[allow(dead_code)]
/// Contains the configuration and the implementation for the GPIO handler.
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

    /// Provides a struct of type I2cInterface
    /// # Arguments    
    /// * `config` - configuration for I2C interface
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
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

            config: config,
        }
    }

    /// Checks if character is part of a numeric description:
    /// - returns true if character is either a figureor a decimal point
    /// - returns true/fale depending on parameter is character is a negative sign
    /// - returns false elsewise
    /// # Arguments    
    /// * `character` - character to be checked
    /// * `negative_allowed` - flag indicating if a negative sign is allowed    
    #[allow(dead_code)]
    fn is_numeric(character: u8, negative_allowed: bool) -> bool {
        match character {
            atlas_scientific_constants::ASCII_VAL_ZERO
                ..=atlas_scientific_constants::ASCII_VAL_NINE => true,
            atlas_scientific_constants::DECIMAL_POINT => true,
            atlas_scientific_constants::NEGATIVE_SIGN => negative_allowed,
            _ => false,
        }
    }

    /// Evaluates response from Atlas Scientific sensor unit and provides wrapped in Result type either
    /// - result
    /// - error code
    /// # Arguments
    /// * `buf` - array of u8
    /// * `is_ph` - flag indicating if sensor unit is pH unit
    #[allow(dead_code)]
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

        // get slice of array
        let numeric_content = &buffer[atlas_scientific_constants::INDEX_CONTENT_START
            ..atlas_scientific_constants::INDEX_PENULTIMATE_CHARACTER];

        // third check: all characters inbetween belong to a number
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

    /// Reads and checks response from Atlas Scientific sensor.
    /// Provide wrapped into Result type either
    /// - result
    /// - error code
    /// # Arguments
    /// * `signal` - enum value describing which sensor shall be read
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
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

        let is_ph = match signal {
            AquariumSignal::pH => true,
            _ => false,
        };

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
    // measures all three signals sequentially
    // In case one measurement yields an error, the function return this error.
    // In case all measurements yield success, the function returns a tuple of
    // all three signals wrapped in Ok.
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
    /// receives and response to requests via channel        
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
                panic!("i2c_interfcae_check_response_valid_pH returned error: {e:?}");
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

    // happy case for evaluating response from conductivity sensor unit
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

    // happy case for evaluating response from temperature sensor unit
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

    // checking if incorrect magic number in the front of the message is identified correctly
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

    // checking if incorrect magic number in the rear part of the message is identified correctly
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

    // checking if incorrect sequence of numeric characters is identified correctly
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

    // test the error response of get_response_from_i2c when requesting an invalid signal.
    // This test case only runs on the target hardware, because it requires initialization of the I2C subsystem.
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
