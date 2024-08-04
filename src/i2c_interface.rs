/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#![allow(dead_code)]

use spin_sleep::SpinSleeper;
use std::str;
use std::time::Duration;
#[cfg(target_os = "linux")]
use {
    rppal::i2c::I2c, // required for communication with AtlasScientific sensors
};

use crate::channel_content::AquariumSignal;
use crate::gpio_handler::I2cInterfaceConfig;

/// error codes for evaluation of sensor unit response
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
    WriteFailure,

    /// command could not be sent completely to sensor unit
    InsufficientWrite,

    /// response from sensor unit could not be received completely
    InsufficientRead,

    /// wrapper for AtlasScientificError
    ResponseEvaluationFailure(AtlasScientificError),

    /// Represents a failure to set the address (encoded in `.0`) of the I2C sensor that would later have data requested from it.
    SetAddressError(u16),
}

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

/// Contains the configuration and the implementation for the GPIO handler.
pub struct I2cInterface {
    /// I2C interface of rppal library
    #[cfg(target_os = "linux")]
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
    /// Provides a struct of type I2cInterface
    /// # Arguments    
    /// * `config` - configuration for I2C interface
    pub fn new(config: I2cInterfaceConfig) -> I2cInterface {
        I2cInterface {
            #[cfg(target_os = "linux")]
            i2c: {
                let i2c = match I2c::new() {
                    Ok(c) => c,
                    Err(e) => {
                        panic!("i2c_interface: Initialization of I2C interface failed: {e:?}");
                    }
                };
                match i2c.set_timeout(config.timeout_transaction) {
                    Ok(_) => i2c,
                    Err(_) => {
                        panic!("i2c_interface: Could not set timeout for transactions.");
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
    #[cfg(target_os = "linux")]
    pub fn get_response_from_i2c(&mut self, signal: AquariumSignal) -> Result<f32, I2cError> {
        let i2c_address = match signal {
            AquariumSignal::pH => self.config.address_atlas_scientific_ph,
            AquariumSignal::Conductivity => self.config.address_atlas_scientific_conductivity,
            AquariumSignal::WaterTemperature => self.config.address_atlas_scientific_temperature,
            _ => {
                panic!("i2c_interface: invalid signal request");
            }
        };

        match self.i2c.set_slave_address(i2c_address) {
            Ok(_) => { /* do nothing */ }
            Err(_) => {
                println!("i2c_interface: could not set address of device");
                return Err(I2cError::SetAddressError(i2c_address));
            }
        }

        let send_buf: [u8; 1] = [atlas_scientific_constants::COMMAND_READ; 1];

        match self.i2c.write(&send_buf) {
            Ok(c) => {
                if c != 1 {
                    println!(
                        "i2c_interface: could not write command to sensor unit for {}",
                        signal
                    );
                    return Err(I2cError::InsufficientWrite);
                }
            }
            Err(e) => {
                println!("i2c_interface: error in writing to device {} {e:?}", signal);
                return Err(I2cError::WriteFailure);
            }
        }

        let sleep_duration = match signal {
            AquariumSignal::pH => self.sleep_duration_ph,
            AquariumSignal::Conductivity => self.sleep_duration_conductivity,
            AquariumSignal::WaterTemperature => self.sleep_duration_temperature,
            _ => {
                println!("i2c_interface: invalid signal request - using arbitrary duration.");
                self.sleep_duration_temperature
            }
        };

        self.spin_sleeper.sleep(sleep_duration);

        let mut read_buffer: [u8; atlas_scientific_constants::MESSAGE_SIZE] =
            [0; atlas_scientific_constants::MESSAGE_SIZE];

        match self.i2c.read(&mut read_buffer) {
            Ok(_) => { /* do nothing */ }
            Err(_) => {
                println!(
                    "i2c_interface: did not receive full response from sensor unit for {}",
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
                println!("i2c_interface: error in sensor unit response");
                Err(I2cError::ResponseEvaluationFailure(e))
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use assert_float_eq::*;

    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::i2c_interface::AquariumSignal;
    use crate::i2c_interface::atlas_scientific_constants;
    use crate::i2c_interface::AtlasScientificError;
    use crate::i2c_interface::I2cInterface;

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
    #[cfg(target_os = "linux")]
    pub fn test_i2c_interface_read_sensors() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_i2c_interface.toml".to_string());
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
}
