/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(not(test))]
use log::error;

use serde_derive::Deserialize;

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
            use rppal::gpio::{Gpio, InputPin};
            use crate::dht::DhtConfig;
            use crate::dht::Dht;
    }
}

const LOG_TARGET_GPIO_HANDLER: &'static str = "gpio_handler";

/// Holds the configuration data for the GPIO handler.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct GpioHandlerConfig {
    /// Flag to determine if application shall communicate with hardware directly or via TCP with a simulator
    pub use_simulator: bool,

    /// GPIO pin number for tank level sensor
    pub gpio_pin_number_tank_level: u8,

    /// GPIO pin number for communication with DHT22 sensor
    pub gpio_pin_number_dht22_io: u8,

    /// GPIO pin number for providing power to DHT22 sensor
    pub gpio_pin_number_dht22_vcc: u8,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the GPIO handler.

pub struct GpioHandler {}

impl GpioHandler {
    /// checks if the GPIO pin configuration valid:
    /// - pin numbers shall be within permissible range
    /// - pin numbers shall be distinct
    /// # Arguments
    /// * `config` - configuration of GPIO handler
    pub fn check_valid_gpio_pin_config(config: &GpioHandlerConfig) -> bool {
        // check if pin ids are in valid range
        if config.gpio_pin_number_tank_level > 3
            && config.gpio_pin_number_tank_level < 28
            && config.gpio_pin_number_dht22_vcc > 3
            && config.gpio_pin_number_dht22_vcc < 28
            && config.gpio_pin_number_dht22_io > 3
            && config.gpio_pin_number_dht22_io < 28
        {
            // check if pin ids are distinct
            if config.gpio_pin_number_tank_level != config.gpio_pin_number_dht22_vcc
                && config.gpio_pin_number_dht22_vcc != config.gpio_pin_number_dht22_io
                && config.gpio_pin_number_dht22_io != config.gpio_pin_number_tank_level
            {
                true // valid GPIO pin number
            } else {
                #[cfg(not(test))]
                error!(
                    target: LOG_TARGET_GPIO_HANDLER,
                    "GPIO pins are not distinct."
                );
                false // invalid GPIO pin numbers because not distinct
            }
        } else {
            #[cfg(not(test))]
            error!(
                target: LOG_TARGET_GPIO_HANDLER,
                "GPIO pins are not inside permissible range."
            );
            false // invalid GPIO pin number because out of range
        }
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// This function returns a struct of type GpioHandler.
    /// If real hardware is used, then the function also opens interface to GPIO and I2C.
    /// # Arguments
    /// * `config` - configuration data for GPIO handler
    /// * `dht_config` - configuration data for GPIO handler
    /// * `i2c_config` - configuration data for I2C handler
    pub fn new(
        config: GpioHandlerConfig,
        dht_config: DhtConfig,
    ) -> (GpioHandler, Option<InputPin>, Option<Dht>) {
        let use_simulator = config.use_simulator; // create copy because struct is moved

        // check if GPIO pin configuration is valid
        if !Self::check_valid_gpio_pin_config(&config) {
            panic!(
                "{}: GPIO pin configuration is invalid",
                LOG_TARGET_GPIO_HANDLER,
            );
        }

        if !use_simulator {
            // open interface to GPIO library
            let gpio = match Gpio::new() {
                Ok(c) => c,
                Err(e) => {
                    panic!(
                        "{}: could not open interface to GPIO ({e:?})",
                        LOG_TARGET_GPIO_HANDLER,
                    );
                }
            };

            // get pin for tank level monitoring
            let tank_level_pin = match gpio.get(config.gpio_pin_number_tank_level) {
                Ok(c) => c,
                Err(e) => {
                    panic!(
                    "{}: could not get pin {} from GPIO interface for tank level monitoring ({e:?})",
                    LOG_TARGET_GPIO_HANDLER,
                    config.gpio_pin_number_tank_level
                );
                }
            };

            // clone the values before creating struct
            let gpio_pin_number_dht22_io = config.gpio_pin_number_dht22_io;
            let gpio_pin_number_dht22_vcc = config.gpio_pin_number_dht22_vcc;

            (
                GpioHandler {},
                Some(tank_level_pin.into_input_pullup()),
                Some(Dht::new(
                    dht_config,
                    gpio.clone(),
                    gpio_pin_number_dht22_io,
                    gpio_pin_number_dht22_vcc,
                )),
            )
        } else {
            (GpioHandler {}, None, None)
        }
    }

    #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
    /// This function returns a struct of type GpioHandler for development purposes.
    /// # Arguments
    /// * `config` - configuration data for GPIO handler
    pub fn new(config: GpioHandlerConfig) -> GpioHandler {
        if !config.use_simulator {
            panic!(
                "{}: Usage of hardware not supported on this platform.",
                LOG_TARGET_GPIO_HANDLER
            );
        }

        if !Self::check_valid_gpio_pin_config(&config) {
            panic!(
                "{}: GPIO pin configuration is invalid",
                LOG_TARGET_GPIO_HANDLER,
            );
        }
        GpioHandler {}
    }
}

pub mod tests {
    cfg_if::cfg_if! {
        if #[cfg(all(feature = "target_hw", target_os = "linux", test))] {
            use crate::logger::setup_logger;
            use crate::config::read_config_file;
            use crate::config::ConfigData;
            use crate::gpio_handler::GpioHandler;
        }
    }

    #[test]
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    pub fn test_valid_gpio_config() {
        setup_logger();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        let _gpio_handler = GpioHandler::new(config.gpio_handler, config.dht);
    }

    #[test]
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    #[should_panic]
    pub fn test_invalid_gpio_config_low() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());

        config.gpio.gpio_pin_number_tank_level = 40;

        let _gpio_handler = GpioHandler::new(config.gpio_handler, config.dht);
    }

    #[test]
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    #[should_panic]
    pub fn test_invalid_gpio_config_high() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());

        config.gpio.gpio_pin_number_tank_level = 40;

        let _gpio_handler = GpioHandler::new(config.gpio_handler, config.dht);
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
            let config: ConfigData = read_config_file("/config/aquarium_control.toml".to_string());
            let (_, tank_level_switch_input_pin_opt, _dht_opt) =
                GpioHandler::new(config.gpio_handler, config.dht);

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
}
