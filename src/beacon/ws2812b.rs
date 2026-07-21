/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
//! Provides a driver for controlling WS2812B RGB LEDs via SPI on Linux.
//!
//! This module serves as a high-level wrapper around the `ws2818-rgb-led-spi-driver`
//! crate, simplifying the process of controlling a strip of WS2812B LEDs. It is
//! specifically designed for hardware interaction and is only compiled when the
//! `target_os` is "linux" and the `target_hw` feature is enabled.
//!
//! ## Key Features
//!
//! - **Simple API**: Offers a straightforward `new()` constructor and a `set_color()`
//!   method to control the entire LED strip.
//! - **Configuration Driven**: Uses a `Ws2812BConfig` struct, deserialized
//!   from a configuration file, to specify the SPI device and the number of LEDs.
//! - **Correct Color Encoding**: Automatically handles the common Green-Red-Blue (GRB)
//!   byte order expected by WS2812B LEDs.
//! - **Error Handling**: Provides a dedicated `Ws2812BError` enum for clear and
//!   actionable error reporting during setup or operation.
cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
        use serde_derive::Deserialize;
        use ws2818_rgb_led_spi_driver::adapter_gen::WS28xxAdapter;
        use ws2818_rgb_led_spi_driver::adapter_spi::WS28xxSpiAdapter;
        use ws2818_rgb_led_spi_driver::encoding::encode_rgb;
        use thiserror::Error;
        use std::fmt;
    }
}

/// Represents the primary colors for an RGB LED.
#[cfg(all(target_os = "linux", feature = "target_hw"))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RgbLedColor {
    /// The LEDs are turned off.
    Off,
    /// The color Red.
    Red,
    /// The color Green.
    Green,
    /// The color Blue.
    Blue,
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
impl RgbLedColor {
    /// Maps an `RgbLedColor` enum to its corresponding GRB byte representation for a WS2812B LED.
    ///
    /// WS2812B LEDs typically expect color data in Green-Red-Blue (GRB) order. This function
    /// provides the correct byte array for each primary color.
    ///
    /// # Returns
    /// A static reference to a 3-byte array `[G, R, B]`.
    pub fn get_ws2812b_color_bytes(&self) -> &'static [u8; 3] {
        match self {
            // WS2812B is commonly GRB: Green, Red, Blue
            // [G, R, B]
            RgbLedColor::Off => &[0, 0, 0],
            RgbLedColor::Red => &[0, 255, 0], // Green=0, Red=255, Blue=0
            RgbLedColor::Green => &[255, 0, 0], // Green=255, Red=0, Blue=0
            RgbLedColor::Blue => &[0, 0, 255], // Green=0, Red=0, Blue=255
        }
    }
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
impl fmt::Display for RgbLedColor {
    /// Formats the value using the given formatter.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RgbLedColor::Off => write!(f, "Off"),
            RgbLedColor::Red => write!(f, "Red"),
            RgbLedColor::Green => write!(f, "Green"),
            RgbLedColor::Blue => write!(f, "Blue"),
        }
    }
}

/// Holds the configuration for the WS2812B RGB LED driver.
#[derive(Deserialize)]
#[cfg(all(target_os = "linux", feature = "target_hw"))]
pub struct Ws2812BConfig {
    /// The name of the SPI device file (e.g., "/dev/spidev0.0").
    pub device_name: String,

    /// The total number of LEDs on the connected strip.
    pub number_leds: usize,

    /// A flag to enable or disable the functionality of this module.
    pub active: bool,
}

/// Defines errors that can occur within the `Ws2812B` module.
#[cfg(all(target_os = "linux", feature = "target_hw"))]
#[derive(Error, Debug)]
pub enum Ws2812BError {
    /// Failed to set up the underlying SPI adapter for the Ws2812B driver.
    #[error("[{0}] Failed to setup adapter for Ws2812B: {1}")]
    AdapterSetupFailure(String, String),

    /// Failed to set the color of the Ws2812B LEDs.
    #[error("[{0}] Failed to set color of Ws2812B to {1}: {2}")]
    AdapterSetColorFailure(String, RgbLedColor, String),
}

/// A driver for controlling a strip of WS2812B RGB LEDs via an SPI interface.
#[cfg(all(target_os = "linux", feature = "target_hw"))]
pub struct Ws2812B {
    /// The underlying SPI adapter from the `ws2818-rgb-led-spi-driver` crate.
    adapter: WS28xxSpiAdapter,

    /// The configuration for this driver instance.
    config: Ws2812BConfig,
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
impl Ws2812B {
    /// Creates a new `Ws2812B` driver instance.
    ///
    /// This constructor initializes the connection to the SPI device specified in the
    /// configuration.
    ///
    /// # Arguments
    /// * `config` - The `Ws2812BConfig` containing the device name and number of LEDs.
    ///
    /// # Returns
    /// A `Result` containing a new `Ws2812B` instance on success.
    ///
    /// # Errors
    /// This function will return an `Err(Ws2812BError::AdapterSetupFailure)` if the
    /// underlying SPI adapter from the `ws2818-rgb-led-spi-driver` crate fails
    /// to initialize. This can happen if the specified SPI device file does not
    /// exist or if there are permission issues.
    pub fn new(config: Ws2812BConfig) -> Result<Self, Ws2812BError> {
        let adapter = WS28xxSpiAdapter::new(&config.device_name)
            .map_err(|e| Ws2812BError::AdapterSetupFailure(module_path!().to_string(), e))?;
        Ok(Ws2812B { adapter, config })
    }

    /// Sets the color of all LEDs on the strip.
    ///
    /// This function encodes the desired `RgbLedColor` into the correct byte format
    /// for the entire LED strip and writes the data to the SPI device.
    ///
    /// # Arguments
    /// * `color` - The `RgbLedColor` to set for all LEDs.
    ///
    /// # Returns
    /// An empty `Result` on successful transmission of the color data.
    ///
    /// # Errors
    /// This function will return an `Err(Ws2812BError::AdapterSetColorFailure)` if
    /// writing the encoded color data to the SPI device fails. This could be due
    /// to an I/O error with the underlying hardware.
    pub fn set_color(&mut self, color: RgbLedColor) -> Result<(), Ws2812BError> {
        let raw_color_bytes = color.get_ws2812b_color_bytes(); // This is [G, R, B]

        // Create a vector to hold the encoded data for all LEDs
        // Each LED needs 3 bytes (GRB) in input, which gets expanded by the driver
        let mut spi_encoded_rgb_bits = Vec::with_capacity(self.config.number_leds * 3);

        // Repeat the color for each LED on the strip
        for _i in 0..self.config.number_leds {
            spi_encoded_rgb_bits.extend_from_slice(&encode_rgb(
                raw_color_bytes[1], // Red component
                raw_color_bytes[0], // Green component
                raw_color_bytes[2], // Blue component
            ));
        }

        self.adapter
            .write_encoded_rgb(&spi_encoded_rgb_bits)
            .map_err(|e| {
                Ws2812BError::AdapterSetColorFailure(module_path!().to_string(), color, e)
            })?;

        Ok(())
    }
}

#[cfg(test)]
#[cfg(all(target_os = "linux", feature = "target_hw"))]
pub mod tests {
    use crate::beacon::ws2812b::{RgbLedColor, Ws2812B};
    use crate::utilities::config::{read_config_file, ConfigData};
    use spin_sleep::SpinSleeper;
    use std::time::Duration;

    #[test]
    #[ignore]
    // Verifies that ws2821b set colors.
    pub fn test_ws2821b_set_colors() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string())
                .expect("Failed to read config in test");
        let spin_sleeper = SpinSleeper::default();
        let sleep_duration = Duration::from_millis(500);
        let mut ws2812b = Ws2812B::new(config.ws2812b).expect("Failed to create Ws2812B in test");

        ws2812b
            .set_color(RgbLedColor::Green)
            .expect("Failed to set color to Green");
        spin_sleeper.sleep(sleep_duration);

        ws2812b
            .set_color(RgbLedColor::Blue)
            .expect("Failed to set color to Blue");
        spin_sleeper.sleep(sleep_duration);

        ws2812b
            .set_color(RgbLedColor::Red)
            .expect("Failed to set color to Red");
    }
}
