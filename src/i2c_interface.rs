/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;
//#[cfg(target_os = "linux")]
use {
    rppal::gpio::Gpio,
    rppal::i2c::I2c, // required for communication with AtlasScientific sensors
};

use crate::channel_content::InternalCommand;
use crate::channel_content::AquariumSignal;

enum I2cError {
    I2cGenericError,
}

/// Contains the configuration and the implementation for the GPIO handler.
pub struct I2cInterface {    
    /// I2C interface of rppal library
    #[cfg(target_os = "linux")]
    i2c: I2c,
}

impl I2cInterface {
    pub fn new() -> I2cInterface {
        match I2c::new() {
            Ok(c) => c,
            Err(e) => {
                panic!("GpioHandler: Initialization of I2C interface failed.");
            }
        }
    }
    
    pub fn get_response_from_i2c(signal: AquariumSignal) -> Result<f32, I2cError> {
        Err(I2cError::I2cGenericError)
    }
}