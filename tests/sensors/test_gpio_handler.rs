/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
//! Tests the initialization and validation of the system's GPIO pins.

#[allow(unused_imports)]
use aquarium_control::sensors::gpio_handler::{GpioHandler, GpioHandlerError};
#[allow(unused_imports)]
use aquarium_control::utilities::config::{read_config_file, ConfigData};

#[test]
pub fn test_valid_gpio_config() {
    let config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    // The `new` function should succeed for a valid configuration.
    assert!(GpioHandler::new(config.gpio_handler).is_ok());
}

#[test]
pub fn test_invalid_gpio_config_low() {
    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

    config.gpio_handler.dht22_io = 2; // Value is at the boundary, which is invalid.

    let result = GpioHandler::new(config.gpio_handler);
    assert!(result.is_err());
    // Check that we get the specific error we expect.
    assert!(matches!(result, Err(GpioHandlerError::PinOutsideRange(_))));
}
#[test]
pub fn test_invalid_gpio_config_duplicate() {
    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

    let duplicate_pin = 5;
    config.gpio_handler.dht22_io = duplicate_pin;
    config.gpio_handler.tank_level_switch = duplicate_pin;

    let result = GpioHandler::new(config.gpio_handler);
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(GpioHandlerError::PinDuplicateValue(_))
    ));
}

#[test]
// This check is platform-agnostic, so the cfg attribute isn't needed.
pub fn test_invalid_gpio_config_high() {
    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

    config.gpio_handler.dht22_io = 40; // Value is outside the valid range.

    let result = GpioHandler::new(config.gpio_handler);
    assert!(result.is_err());
    assert!(matches!(result, Err(GpioHandlerError::PinOutsideRange(40))));
}

mod commissioning_prep {
    cfg_if::cfg_if! {
        if #[cfg(all(feature = "commissioning_prep", feature = "target_hw", target_os = "linux", test))] {
            use crate::config::ConfigData;
            use crate::read_config_file;
            use crate::GpioHandler;

            use spin_sleep::SpinSleeper;
            use std::time::Duration;
        }
    }

    #[test]
    #[cfg(all(
        feature = "commissioning_prep",
        feature = "target_hw",
        target_os = "linux",
        test
    ))]
    // for development purposes, visualize the tank level switch pin state with 2 Hz
    pub fn test_show_tank_level_switch() {
        let config: ConfigData = read_config_file("config/aquarium_control.toml".to_string());
        let (_, tank_level_switch_input_pin_opt, _dht_opt) =
            GpioHandler::new(config.gpio_handler, config.dht).unwrap();

        let tank_level_switch_input_pin = match tank_level_switch_input_pin_opt {
            Some(c) => c,
            None => {
                panic!("Could not initialize tank level switch input pin.");
            }
        };

        let sleep_time = Duration::from_millis(500);
        let spin_sleeper = SpinSleeper::default();

        loop {
            println!(
                "Tank Level switch input pin is {}",
                match tank_level_switch_input_pin.is_high() {
                    true => "HIGH",
                    false => "LOW",
                }
            );

            spin_sleeper.sleep(sleep_time);
        }
    }
}
