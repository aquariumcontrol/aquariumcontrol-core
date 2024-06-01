/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;
use std::str;
use {
    rppal::gpio::Gpio,
    rppal::i2c::I2c, // required for communication with AtlasScientific sensors
};

use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;

/// error codes for evaluation of sensor unit response
enum AtlasScientificError {
    /// first character in message sent from sensor unit does not match required magic number
    FirstCharacterNotMagic,

    /// last character in message sent from sensor unit does not match required magic number    
    LastCharacterNotMagic,

    /// message sent from sensor unit contains non-numeric characters in data section
    ContainsInvalidCharacter,

    /// negative pH values do not make sense. Hence, pH sensor unit response is restricted a bit more
    IllegalNegativeSign,

    /// error is masked by other checks. However signature of function to convert from u8 array slice to String throws an error.
    InvalidUtf8Sequence,

    /// conversion from String to f32 failed
    ConversionFailure,
}

/// communication error codes
enum I2cError {
    /// message could not be sent
    WriteFailure,

    /// command could not be sent completely to sensor unit
    InsufficientWrite,

    /// response from sensor unit could not be received completely
    InsufficientRead,

    /// wrapper for AtlasScientificError
    ResponseEvaluationFailure(AtlasScientificError),
}

mod atlas_scientific_constants {
    /// command sent to sensor unit for initiating response
    pub const command_read: u8 = 'R';

    /// length of message received from sensor unit (number of byte)
    pub const message_size: u16 = 8;

    /// location of first character to be checked inside message from sensor unit
    pub const index_first_character: u16 = 0;

    /// start of data inside message from sensor unit
    pub const index_content_start: u16 = 1;

    /// finish of data inside message from sensor unit
    pub const index_penultimate_character: u16 = 6;

    /// location of last character to be checked inside message from sensor unit
    pub const index_last_character: u16 = 7;

    /// ASCII value for representation of zero figure
    pub const ascii_val_zero: u16 = 48;

    /// ASCII value for representation of figure nine
    pub const ascii_val_nine: u16 = 57;

    /// decimal point
    pub const decimal_point: u8 = '.';

    /// negative sign (note: positive sign does not occurr in message from sensor)
    pub const negative_sign: u8 = '-';

    /// magic number at front position inside message which identifies valid response from sensor unit
    pub const message_start_magic: u16 = 1;

    /// magic number at finish position inside message which identifies valid response from sensor unit
    pub const message_finish_magic: u16 = 0;
}

/// Contains the configuration and the implementation for the GPIO handler.
pub struct I2cInterface {
    /// I2C interface of rppal library
    #[cfg(target_os = "linux")]
    i2c: I2c,

    /// sleep duration for waiting response of Atlas Scientific conductivity sensor
    sleep_duration_conductivity: SpinSleeper,

    /// sleep duration for waiting response of Atlas Scientific ph sensor
    sleep_duration_ph: SpinSleeper,

    /// sleep duration for waiting response of Atlas Scientific temperature sensor
    sleep_duration_temperature: SpinSleeper,

    /// configuration for I2C interface    
    config: I2cInterfaceConfig,

    /// spin sleeper for waiting response of sensor unit
    spin_sleep: SpinSleeper,
}

impl I2cInterface {
    /// Provides a struct of type I2cInterface
    /// # Arguments    
    /// * `config` - configuration for I2C interface
    pub fn new(config: I2cInterfaceConfig) -> I2cInterface {
        I2cInterface {
            i2c: {
                let i2c = match I2c::new() {
                    Ok(c) => c,
                    Err(e) => {
                        panic!("i2c_interface: Initialization of I2C interface failed.");
                    }
                };
                match i2c.set_timeout(config.timeout_transaction) {
                    Ok => { /* do nothing */ }
                    Err => {
                        panic!("i2c_interface: Could not set timeout for transactions.");
                    }
                }

                // setup durations for each sensor individually
                sleep_duration_conductivity =
                    Duration::from_millis(config.sleep_time_atlas_scientific_conductivity);
                sleep_duration_ph = Duration::from_millis(config.sleep_time_atlas_scientific_ph);
                sleep_duration_temperature =
                    Duration::from_millis(config.sleep_time_atlas_scientific_temperature);
                self.spin_sleeper = SpinSleeper::default();
            },
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
            ascii_val_zero..ascii_val_nine => true,
            decimal_point => true,
            negative_sign => negative_allowed,
            _ => false,
        }
    }

    /// Evaluates response from Atlas Scientific sensor unit and provides wrapped in Result type either
    /// - result
    /// - error code
    /// # Arguments
    /// * `buf` - array of u8
    /// * `is_ph` - flag indicating if sensor unit is pH unit
    fn check_response(buffer: [u8], is_ph: bool) -> Result<f32, AtlasScientificError> {
        // first check: first character not magic
        if buffer[atlas_scientific_constants::index_first_character] != message_start_magic {
            return Err(AtlasScientificError::FirstCharacterNotMagic);
        }
        // second check: last character not magic
        if buffer[atlas_scientific_constants::index_last_character] != message_finish_magic {
            return Err(AtlasScientificError::LastCharacterNotMagic);
        }

        // get slice of array
        let numeric_content = &buffer[atlas_scientific_constants::index_content_start
            ..atlas_scientific_constants::index_penultimate_character];

        // third check: all characters inbetween belong to a number
        for character in numeric_content {
            if !is_numeric(character, !is_ph) {
                return Err(AtlasScientificError::ContainsInvalidCharacter);
            }
        }

        // conversion to String
        let numeric_string = match str::from_utf8(numeric_content) {
            Ok(c) => Some(c),
            Err(_) => Err(AtlasScientificError::InvalidUtf8Sequence),
        };

        // conversion to f32
        match &numeric_string.parse::<f32>() {
            Ok(c) => Some(c),
            Err(_) => Err(AtlasScientificError::ConversionFailure),
        }
    }

    /// Reads and checks response from Atlas Scientific sensor.
    /// Provide wrapped into Result type either
    /// - result
    /// - error code
    /// # Arguments
    /// * `signal` - enum value describing which sensor shall be read
    pub fn get_response_from_i2c(&self, signal: AquariumSignal) -> Result<f32, I2cError> {
        let i2c_address = match signal {
            AquariumSignal::pH => self.config.address_atlas_scientific_ph,
            AquariumSignal::Conductivity => self.config.address_atlas_scientific_conductivity,
            AquariumSignal::WaterTemperature => self.config.address_atlas_scientific_temperature,
        };
        self.i2c.set_slave_address(i2c_address);

        let send_buf: [u8] = [atlas_scientific_constants::command_read; 1];
        match self.i2c.write(send_buf) {
            Ok(c) => {
                if c != 1 {
                    println!(
                        "i2c_interface: could not write command to sensor unit for {}",
                        signal
                    );
                    return (I2cError::I2cInsufficientWrite);
                }
            }
            Err(e) => {
                println!("i2c_interface: error in writing to device {} {e:?}", signal);
                return (I2cError::I2cWriteFailure);
            }
        }

        let sleep_duration = match signal {
            AquariumSignal::pH => self.sleep_duration_ph,
            AquariumSignal::Conductivity => self.sleep_duration_conductivity,
            AquariumSignal::WaterTemperature => self.sleep_duration_temperature,
        };
        self.spin_sleeper(sleep_duration);

        let mut read_buffer: [u8] = [0; atlas_scientific_constants::message_size];
        match self.i2c.read(&mut read_buffer) {
            atlas_scientific_constants::message_size => { /* do nothing */ }
            _ => {
                println!(
                    "i2c_interface: did not receive full response from sensor unit for {}",
                    signal
                );
                return (I2cError::I2cInsufficientRead);
            }
        }

        let is_ph = match signal {
            AquariumSignal::pH => true,
            _ => false,
        };

        match check_response(read_buffer, is_ph) {
            Ok(c) => Ok(c),
            Err(e) => {
                println!("i2c_interface: error in sensor unit response");
                I2cError::ResponseEvaluationFailure(e)
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::channel_content::AquariumSignal;
    use crate::gpio_handler::I2cInterfaceConfig;
    use crate::i2c_interface::I2cInterface;
    use assert_float_eq::*;

    // happy case for evaluating response from pH sensor unit
    #[test]
    pub fn test_i2c_interface_check_response_valid_pH() {
        let stimuli: [u8] = [
            atlas_scientific_constants::message_start_magic,
            '8' as u8,
            atlas_scientific_constants::decimal_point,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::message_finish_magic,
        ];
        assert_float_absolute_eq!(check_response(stimuli, true), 8.0, 0.001);
    }

    // evaluating response from pH sensor unit
    // checking if negative values are correctly identified
    pub fn test_i2c_interface_check_response_invalid_pH_negative() {
        let stimuli: [u8] = [
            atlas_scientific_constants::message_start_magic,
            '-' as u8,
            '8' as u8,
            atlas_scientific_constants::decimal_point,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::message_finish_magic,
        ];
        assert_eq!(
            check_response(stimuli, true),
            Err(AtlasScientificError::IllegalNegativeSign)
        );
    }

    // happy case for evaluating response from conductivity sensor unit
    #[test]
    pub fn test_i2c_interface_check_response_valid_conductivity() {
        let stimuli: [u8] = [
            atlas_scientific_constants::message_start_magic,
            '5' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::message_finish_magic,
        ];
        assert_float_absolute_eq!(check_response(stimuli, false), 50000.0, 0.001);
    }

    // happy case for evaluating response from temperature sensor unit
    #[test]
    pub fn test_i2c_interface_check_response_valid_temperature() {
        let stimuli: [u8] = [
            atlas_scientific_constants::message_start_magic,
            '2' as u8,
            '5' as u8,
            atlas_scientific_constants::decimal_point,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::message_finish_magic,
        ];
        assert_float_absolute_eq!(check_response(stimuli, false), 25.0, 0.001);
    }

    // checking if incorrect magic number in the front of the message is identified correctly
    #[test]
    pub fn test_i2c_interface_check_response_invalid_magic_front() {
        let stimuli: [u8] = [
            atlas_scientific_constants::message_start_magic - 1,
            '2' as u8,
            '5' as u8,
            atlas_scientific_constants::decimal_point,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::message_finish_magic,
        ];
        assert_eq!(
            check_response(stimuli, false),
            Err(AtlasScientificError::FirstCharacterNotMagic)
        );
    }

    // checking if incorrect magic number in the rear part of the message is identified correctly
    #[test]
    pub fn test_i2c_interface_check_response_invalid_magic_rear() {
        let stimuli: [u8] = [
            atlas_scientific_constants::message_start_magic,
            '2' as u8,
            '5' as u8,
            atlas_scientific_constants::decimal_point,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::message_finish_magic - 1,
        ];
        assert_eq!(
            check_response(stimuli, false),
            Err(AtlasScientificError::LastCharacterNotMagic)
        );
    }

    // checking if incorrect sequence of numeric characters is identified correctly
    #[test]
    pub fn test_i2c_interface_check_response_invalid_conversion() {
        let stimuli: [u8] = [
            atlas_scientific_constants::message_start_magic,
            '2' as u8,
            '5' as u8,
            atlas_scientific_constants::negative_sign,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::message_finish_magic - 1,
        ];
        assert_eq!(
            check_response(stimuli, false),
            Err(AtlasScientificError::ConversionFailure)
        );
    }
    
    // read from real hardware: Atlas Scientific pH sensor
    #[test]
    pub fn test_i2c_interface_read_ph() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_i2c_interface.toml".to_string());
        i2c_interface = I2cInterface::new(config.i2c_interface);
        assert_float_absolute_eq!(
            get_response_from_i2c(AquariumSignal::pH),
            8.0,
            3.0
        );
    }
    
    // read from real hardware: Atlas Scientific conductivity sensor
    #[test]
    pub fn test_i2c_interface_read_temperature() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_i2c_interface.toml".to_string());
        i2c_interface = I2cInterface::new(config.i2c_interface);
        assert_float_absolute_eq!(
            get_response_from_i2c(AquariumSignal::Conductivity),
            50000.0,
            10000.0
        );
    }
    
    // read from real hardware: Atlas Scientific temperature sensor
    #[test]
    pub fn test_i2c_interface_read_temperature() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_i2c_interface.toml".to_string());
        i2c_interface = I2cInterface::new(config.i2c_interface);
        assert_float_absolute_eq!(
            get_response_from_i2c(AquariumSignal::WaterTemperature),
            25.0,
            10.0
        );
    }
    
}
