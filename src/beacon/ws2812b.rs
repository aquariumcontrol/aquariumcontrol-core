/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
        use serde_derive::Deserialize;
        use log::warn;
        use ws2818_rgb_led_spi_driver::adapter_gen::WS28xxAdapter;
        use ws2818_rgb_led_spi_driver::adapter_spi::{WS28xxSpiAdapter, AdapterError};
        use ws2818_rgb_led_spi_driver::encoding::encode_rgb;
        use thiserror::Error;
    }
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
pub enum RgbLedColor {
    #[allow(unused)]
    Red,

    #[allow(unused)]
    Green,

    #[allow(unused)]
    Blue,
}

/// Maps an `RgbLedColor` enum to its corresponding GRB byte representation for a WS2812B LED.
///
/// WS2812B LEDs typically expect color data in Green-Red-Blue (GRB) order. This function
/// provides the correct byte array for each primary color.
///
/// # Arguments
/// * `color` - The `RgbLedColor` to convert.
///
/// # Returns
/// A static reference to a 3-byte array `[G, R, B]`.
#[cfg(all(target_os = "linux", feature = "target_hw"))]
pub fn get_ws2812b_color_bytes(color: RgbLedColor) -> &'static [u8; 3] {
    match color {
        // WS2812B is commonly GRB: Green, Red, Blue
        // [G, R, B]
        RgbLedColor::Red => &[0, 255, 0], // Green=0, Red=255, Blue=0
        RgbLedColor::Green => &[255, 0, 0], // Green=255, Red=0, Blue=0
        RgbLedColor::Blue => &[0, 0, 255], // Green=0, Red=0, Blue=255
    }
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
impl fmt::Display for RgbLedColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RgbLedColor::Red => write!(f, "Red"),
            RgbLedColor::Green => write!(f, "Green"),
            RgbLedColor::Blue => write!(f, "Blue"),
        }
    }
}

/// Contains configuration data for Ws2812B
#[derive(Deserialize)]
#[cfg(all(target_os = "linux", feature = "target_hw"))]
pub struct Ws2812BConfig {
    /// SPI device name
    pub device_name: String,

    /// number of LEDs to be switched on/off
    pub number_leds: usize,

    /// switch for enabling functionality
    pub active: bool,
}

/// Defines errors that can occur within the `Ws2812B` module.
#[cfg(all(target_os = "linux", feature = "target_hw"))]
#[derive(Error, Debug)]
pub enum Ws2812BError {
    /// Failed to set up the adapter for Ws2812B.
    #[error("Failed to setup adapter for Ws2812B.")]
    AdapterSetupFailure {
        #[source]
        source: AdapterError,
    },

    /// Failed to set color of Ws2812B
    #[error("Failed to set color of Ws2812B to {color}.")]
    AdapterSetColorFailure {
        color: RgbLedColor,

        #[source]
        source: AdapterError,
    }
}

/// A driver for controlling a strip of WS2812B RGB LEDs via an SPI interface.
#[cfg(all(target_os = "linux", feature = "target_hw"))]#[cfg(all(target_os = "linux", feature = "target_hw"))]
pub struct Ws2812B {
    /// SPI-based driver interface
    adapter: WS28xxSpiAdapter,

    /// holds configuration data
    config: Ws2812BConfig,
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
impl Ws2812B {
    /// Creates a new `Ws2812B` driver instance.
    ///
    /// This constructor initializes the connection to the SPI device specified in the
    /// configuration. It will fail if the SPI device cannot be opened or configured.
    ///
    /// # Arguments
    /// * `config` - The `Ws2812BConfig` containing the device name and number of LEDs.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(Ws2812B)`: A new driver instance on successful initialization.
    /// - `Err(Ws2812BError::AdapterSetupFailure)`: If the SPI adapter cannot be created.
    pub fn new(config: Ws2812BConfig) -> Result<Ws2812B, Ws2812BError> {
        let adapter = WS28xxSpiAdapter::new(&config.device_name).map_err(
            |e| Ws2812BError::AdapterSetupFailure { source: e })?;
        Ws2812B { adapter, config }
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
    /// A `Result` which is:
    /// - `Ok(())`: If the color was successfully written to the SPI device.
    /// - `Err(Ws2812BError::AdapterSetColorFailure)`: If writing to the SPI device fails.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub fn set_color(&mut self, color: RgbLedColor) -> Result<(), Ws2812BError> {
        let raw_color_bytes = get_ws2812b_color_bytes(color); // This is [G, R, B]

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

        self.adapter.write_encoded_rgb(&spi_encoded_rgb_bits).map_err(
            |e| Ws2812BError::AdapterSetColorFailure { color, source: e })?;

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
    pub fn test_ws2821b_set_colors() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        let spin_sleeper = SpinSleeper::default();
        let sleep_duration = Duration::from_millis(500);
        let mut ws2812b = Ws2812B::new(config.ws2812b);

        ws2812b.set_color(RgbLedColor::Green);
        spin_sleeper.sleep(sleep_duration);
        ws2812b.set_color(RgbLedColor::Blue);
        spin_sleeper.sleep(sleep_duration);
        ws2812b.set_color(RgbLedColor::Red);
    }
}
