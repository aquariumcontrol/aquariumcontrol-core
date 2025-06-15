/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(not(test))]
use log::error;
use std::collections::HashSet;

use serde_derive::Deserialize;

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
            use rppal::gpio::{Gpio};
    }
}

const LOG_TARGET_GPIO_HANDLER: &str = "gpio_handler";

// create a stub version of the type to allow build on non-target platforms
#[cfg(not(all(target_os = "linux", feature = "target_hw")))]
pub type Gpio = u64;

/// Holds the configuration data for the GPIO handler.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct GpioHandlerConfig {
    /// Flag to determine if the application shall communicate with hardware directly or via TCP with a simulator
    pub use_simulator: bool,

    /// GPIO for the tank level sensor
    pub tank_level_switch: u8,

    /// GPIO for communication with the DHT22 sensor
    pub dht22_io: u8,

    /// GPIO for providing power to the DHT22 sensor
    pub dht22_vcc: u8,

    /// GPIO of the protein skimmer
    pub skimmer: u8,

    /// GPIO of the main pump #1
    pub main_pump1: u8,

    /// GPIO of the main pump #2
    pub main_pump2: u8,

    /// GPIO of the auxiliary pump #1
    pub aux_pump1: u8,

    /// GPIO of the auxiliary pump #2
    pub aux_pump2: u8,

    /// GPIO of the feeder
    pub feeder: u8,

    /// GPIO of the refill pump
    pub refill_pump: u8,

    /// GPIO of the heater
    pub heater: u8,

    /// GPIO of the surface ventilation
    pub ventilation: u8,

    /// GPIO of the peristaltic pump #1
    pub peristaltic_pump1: u8,

    /// GPIO of the peristaltic pump #2
    pub peristaltic_pump2: u8,

    /// GPIO of the peristaltic pump #3
    pub peristaltic_pump3: u8,

    /// GPIO of the peristaltic pump #4
    pub peristaltic_pump4: u8,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the GPIO handler.
pub struct GpioHandler {
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO interface wrapped in Option
    pub gpio_lib_handle_opt: Option<Gpio>,
}

impl GpioHandler {
    #[allow(unused)]
    /// Checks if the GPIO pin configuration is **valid and free from conflicts**.
    ///
    /// This function performs two essential validation checks on the configured GPIO pins:
    /// 1.  **Range Check**: Ensures all pin IDs fall within a permissible operating range (currently 4 to 27, inclusive).
    /// 2.  **Uniqueness Check**: Verifies that all assigned pin IDs are distinct, preventing multiple devices from being configured to the same physical pin.
    ///
    /// Errors (out-of-range or duplicate IDs) are logged to the console/main log unless in testing mode
    /// and printed to stdout in test builds for immediate visibility.
    ///
    /// # Arguments
    /// * `config` - The `GpioHandlerConfig` struct containing the GPIO pin assignments for various aquarium devices.
    ///
    /// # Returns
    /// `true` if all GPIO pin IDs are valid (within range and distinct); `false` otherwise.
    pub fn check_valid_gpio_pin_config(config: &GpioHandlerConfig) -> bool {
        // check if pin ids are in valid range and if they are distinct
        let mut seen_values = HashSet::new();

        let gpio_values = [
            config.tank_level_switch,
            config.dht22_io,
            config.dht22_vcc,
            config.skimmer,
            config.main_pump1,
            config.main_pump2,
            config.aux_pump1,
            config.aux_pump2,
            config.feeder,
            config.refill_pump,
            config.heater,
            config.ventilation,
            config.peristaltic_pump1,
            config.peristaltic_pump2,
            config.peristaltic_pump3,
            config.peristaltic_pump4,
        ];

        for &value in &gpio_values {
            if value <= 3 || value >= 28 {
                #[cfg(not(test))]
                error!(
                    target: LOG_TARGET_GPIO_HANDLER,
                    "Not all GPIOs are inside permissible range. GPIO {} is not valid.",
                    value
                );
                #[cfg(test)]
                println!("Not all GPIOs are inside permissible range.");
                return false; // Value is out of the allowed range
            }
            if !seen_values.insert(value) {
                #[cfg(not(test))]
                error!(
                    target: LOG_TARGET_GPIO_HANDLER,
                    "Not all GPIOs are distinct - duplicate values found."
                );
                #[cfg(test)]
                println!("Not all GPIOs are distinct - duplicate values found.");
                return false; // Duplicate value found
            }
        }

        true
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// This function returns a struct of type GpioHandler after opening the interface to GPIO.
    ///
    /// This constructor initializes the GPIO handler. If `use_simulator` is false (meaning
    /// hardware is being targeted), it attempts to open the system's GPIO interface via `rppal`.
    /// If `use_simulator` is true, it creates a handler without a real GPIO interface.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the GPIO handler, specifying the pins and simulation mode.
    ///
    /// # Returns
    /// An `Option<GpioHandler>`:
    /// - `Some(GpioHandler)` containing a handle to the GPIO interface if `use_simulator` is false and opening succeeds.
    /// - `Some(GpioHandler)` with `gpio_lib_handle_opt` as `None` if `use_simulator` is true.
    /// - `None` on unsupported platforms/features (handled by the other `new` implementation).
    ///
    /// # Panics
    /// This function will panic if:
    /// - The `config` contains invalid GPIO pin mappings (e.g., out-of-range or duplicate IDs).
    /// - `use_simulator` is false, and it fails to open the underlying GPIO interface.
    pub fn new(config: GpioHandlerConfig) -> Option<GpioHandler> {
        let use_simulator = config.use_simulator; // create copy because the struct is moved

        // check if GPIO pin configuration is valid
        if !Self::check_valid_gpio_pin_config(&config) {
            panic!(
                "{}: GPIO pin configuration is invalid",
                LOG_TARGET_GPIO_HANDLER,
            );
        }

        if !use_simulator {
            // open interface to the GPIO library
            let gpio_lib_handle = match Gpio::new() {
                Ok(c) => c,
                Err(e) => {
                    panic!(
                        "{}: could not open interface to GPIO ({e:?})",
                        LOG_TARGET_GPIO_HANDLER,
                    );
                }
            };

            Some(GpioHandler {
                gpio_lib_handle_opt: Some(gpio_lib_handle),
            })
        } else {
            Some(GpioHandler {
                gpio_lib_handle_opt: None,
            })
        }
    }

    #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
    #[allow(unused)]
    /// Creates a new `GpioHandler` instance for **development platforms**.
    ///
    /// This constructor is activated when the application is compiled for a non-Linux
    /// operating system or when the `target_hw` feature is not enabled. In this mode,
    /// it acts as a placeholder or a mock, primarily to allow compilation and basic testing
    /// without requiring actual GPIO hardware. It always returns `None` as its GPIO
    /// library-handle.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the GPIO handler, including pin assignments and simulation settings.
    ///
    /// # Returns
    /// Always returns `None`, indicating that a live GPIO interface is not available on this platform or configuration.
    ///
    /// # Panics
    /// This function will panic if:
    /// - `use_simulator` in the `config` is `false` (meaning a hardware setup is attempted on an unsupported platform).
    /// - The `config` contains **invalid GPIO pin mappings** (e.g., out-of-range or duplicate IDs).
    pub fn new(config: GpioHandlerConfig) -> Option<GpioHandler> {
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
        None
    }
}

pub mod tests {
    #[allow(unused_imports)]
    use crate::sensors::gpio_handler::GpioHandler;
    #[allow(unused_imports)]
    use crate::utilities::config::{read_config_file, ConfigData};

    #[test]
    pub fn test_valid_gpio_config() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        let _gpio_handler = GpioHandler::new(config.gpio_handler);
    }

    #[test]
    #[should_panic]
    pub fn test_invalid_gpio_config_low() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());

        config.gpio_handler.dht22_io = 3;

        let _gpio_handler = GpioHandler::new(config.gpio_handler);
    }
    #[test]
    #[should_panic]
    pub fn test_invalid_gpio_config_duplicate() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());

        config.gpio_handler.dht22_io = 5;
        config.gpio_handler.tank_level_switch = 5;

        let _gpio_handler = GpioHandler::new(config.gpio_handler);
    }

    #[test]
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    #[should_panic]
    pub fn test_invalid_gpio_config_high() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());

        config.gpio_handler.dht22_io = 40;

        let _gpio_handler = GpioHandler::new(config.gpio_handler);
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
