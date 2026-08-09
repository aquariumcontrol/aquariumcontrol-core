/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages the initialization and validation of the system's GPIO pins.
//!
//! This module acts as a central gatekeeper for all GPIO (General Purpose Input/Output)
//! operations. Its primary responsibility is to be instantiated once at application
//! startup to perform two critical functions:
//!
//! 1.  **Validate Configuration**: It checks the content of the `[gpio_handler]` section of the
//!     configuration file and rigorously checks that all assigned GPIO pins
//!     are within a valid hardware range and are unique, preventing conflicts.
//! 2.  **Initialize Hardware**: On a supported hardware platform (`linux` with the
//!     `target_hw` feature), it acquires a handle to the system's GPIO interface
//!     (using the `rppal` crate).
//!
//! This module intentionally does not perform any I/O operations itself. Instead, it
//! provides the validated configuration and the hardware handle to other modules
//! (like actuators and sensors) that are responsible for the actual reading and
//! writing of pin states.

use std::collections::HashSet;

use crate::sensors::gpio_handler_config::GpioHandlerConfig;
use thiserror::Error;

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
            use rppal::gpio::{Gpio};
    }
}

// create a stub version of the type to allow build on non-target platforms
#[cfg(not(all(target_os = "linux", feature = "target_hw")))]
pub type Gpio = u64;

/// Minimum permissible GPIO value. The configuration is checked against this value.
pub const GPIO_MIN: u8 = 3;

/// Maximum permissible GPIO value. The configuration is checked against this value.
pub const GPIO_MAX: u8 = 28;

/// Contains error definition for GpioHandler
#[derive(Error, Debug)]
pub enum GpioHandlerError {
    /// Configuration contains invalid GPIO pin mapping.
    #[error("Configuration contains invalid GPIO pin mapping: {0} is out of range")]
    PinOutsideRange(u8),

    /// Found duplicate GPIO values in configuration.
    #[error("Found duplicate GPIO values in configuration: {0} is not unique.")]
    PinDuplicateValue(u8),

    /// Could not open interface to GPIO.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("Could not open interface to GPIO.")]
    OpenInterfaceFailed {
        #[source]
        source: rppal::gpio::Error,
    },

    /// This error is triggered, When running either on different OS or without target hardware.
    /// Defensive programming: This prevents the startup of the application.
    #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
    #[error("Usage of hardware not supported on this platform.")]
    PlatformNotSupported,
}

/// Contains the configuration and the implementation for the GPIO handler.
pub struct GpioHandler {
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO interface wrapped in Option
    pub gpio_lib_handle_opt: Option<Gpio>,
}

impl GpioHandler {
    /// Checks if the GPIO pin configuration is valid and free from conflicts.
    ///
    /// This function performs two essential validation checks on the configured GPIO pins:
    /// 1.  **Range Check**: Ensures all pins fall within the permissible operating
    ///     range defined by `GPIO_MIN` and `GPIO_MAX`.
    /// 2.  **Uniqueness Check**: Verifies that all assigned pins are distinct,
    ///     preventing multiple devices from being configured to the same physical pin.
    ///
    /// # Arguments
    /// * `config` - The `GpioHandlerConfig` struct containing the GPIO pin assignments.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if all GPIO pins are valid.
    ///
    /// # Errors
    /// Returns a `GpioHandlerError` if any validation check fails:
    /// - `GpioHandlerError::PinOutsideRange`: If any configured GPIO pin is
    ///   outside the valid range (e.g., less than or equal to 3, or greater than
    ///   or equal to 28).
    /// - `GpioHandlerError::PinDuplicateValue`: If the same GPIO pin is assigned
    ///   to more than one device.
    pub fn check_valid_gpio_pin_config(config: &GpioHandlerConfig) -> Result<(), GpioHandlerError> {
        // check if pin ids are in valid range and if they are distinct
        const VALID_GPIO_RANGE: std::ops::Range<u8> = GPIO_MIN..(GPIO_MAX + 1);

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
            if !VALID_GPIO_RANGE.contains(&value) {
                return Err(GpioHandlerError::PinOutsideRange(value));
            }
            if !seen_values.insert(value) {
                return Err(GpioHandlerError::PinDuplicateValue(value));
            }
        }

        Ok(())
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Creates a new `GpioHandler` instance for the target hardware platform (Linux).
    ///
    /// This constructor initializes the GPIO handler. It first validates the pin
    /// configuration using `check_valid_gpio_pin_config`. If `use_simulator` is
    /// false, it then attempts to acquire a handle to the system's GPIO interface
    /// using `rppal`.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the GPIO handler, specifying the pins and
    ///   simulation mode.
    ///
    /// # Returns
    /// A `Result` containing an `Option<GpioHandler>` on success:
    /// - `Ok(Some(GpioHandler))` with an active `Gpio` handle if `use_simulator` is `false`.
    /// - `Ok(Some(GpioHandler))` with `None` for the handle if `use_simulator` is `true`.
    ///
    /// # Errors
    /// Returns a `GpioHandlerError` if initialization fails:
    /// - `GpioHandlerError::PinOutsideRange` or `PinDuplicateValue`: If the pin
    ///   configuration is invalid (propagated from `check_valid_gpio_pin_config`).
    /// - `GpioHandlerError::OpenInterfaceFailed`: If `use_simulator` is `false` and
    ///   the function fails to open the underlying GPIO interface (e.g., due to
    ///   permission issues or hardware problems).
    pub fn new(config: GpioHandlerConfig) -> Result<Option<GpioHandler>, GpioHandlerError> {
        let use_simulator = config.use_simulator; // create copy because the struct is moved

        // check if GPIO pin configuration is valid
        Self::check_valid_gpio_pin_config(&config)?;

        if !use_simulator {
            // open interface to the GPIO library
            let gpio_lib_handle =
                Gpio::new().map_err(|e| GpioHandlerError::OpenInterfaceFailed { source: e })?;

            Ok(Some(GpioHandler {
                gpio_lib_handle_opt: Some(gpio_lib_handle),
            }))
        } else {
            Ok(Some(GpioHandler {
                gpio_lib_handle_opt: None,
            }))
        }
    }

    #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
    #[allow(unused)]
    /// Creates a new `GpioHandler` instance for development or non-target platforms.
    ///
    /// This constructor is activated when compiling for a non-Linux OS or when the
    /// `target_hw` feature is disabled. It allows the application to run in a
    /// simulated mode without requiring real GPIO hardware.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the GPIO handler.
    ///
    /// # Returns
    /// A `Result` containing `Ok(None)` if the configuration is valid and `use_simulator`
    /// is `true`, indicating that no hardware handle is needed.
    ///
    /// # Errors
    /// Returns a `GpioHandlerError` if initialization fails:
    /// - `GpioHandlerError::PlatformNotSupported`: If `use_simulator` is `false`,
    ///   as hardware access is not possible on the current platform.
    /// - `GpioHandlerError::PinOutsideRange` or `PinDuplicateValue`: If the pin
    ///   configuration is invalid (propagated from `check_valid_gpio_pin_config`)
    pub fn new(config: GpioHandlerConfig) -> Result<Option<GpioHandler>, GpioHandlerError> {
        if !config.use_simulator {
            return Err(GpioHandlerError::PlatformNotSupported);
        }

        Self::check_valid_gpio_pin_config(&config)?;

        Ok(None)
    }
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub mod tests {
    use crate::sensors::gpio_handler::{GpioHandler, GpioHandlerError};
    use crate::sensors::gpio_handler_config::GpioHandlerConfig;

    #[test]
    // [utest->swreq~sensors_read_data~1]
    // Verifies that GpioHandler::new returns PlatformNotSupported when use_simulator is false on non-target platforms.
    #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
    fn test_new_platform_not_supported() {
        let config = GpioHandlerConfig {
            use_simulator: false,
            tank_level_switch: 3,
            dht22_io: 4,
            dht22_vcc: 5,
            skimmer: 6,
            main_pump1: 7,
            main_pump2: 8,
            aux_pump1: 9,
            aux_pump2: 10,
            feeder: 11,
            refill_pump: 12,
            heater: 13,
            ventilation: 14,
            peristaltic_pump1: 15,
            peristaltic_pump2: 16,
            peristaltic_pump3: 17,
            peristaltic_pump4: 18,
        };

        let result = GpioHandler::new(config);
        assert!(matches!(
            result,
            Err(GpioHandlerError::PlatformNotSupported)
        ));
    }
}
