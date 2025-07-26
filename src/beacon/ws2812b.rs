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
        use ws2818_rgb_led_spi_driver::adapter_spi::WS28xxSpiAdapter;
        use ws2818_rgb_led_spi_driver::encoding::encode_rgb;
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

#[cfg(all(target_os = "linux", feature = "target_hw"))]
pub struct Ws2812B {
    /// SPI-based driver interface
    adapter: WS28xxSpiAdapter,

    /// holds configuration data
    config: Ws2812BConfig,
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
impl Ws2812B {
    pub fn new(config: Ws2812BConfig) -> Ws2812B {
        cfg_if::cfg_if! {
            let adapter = match WS28xxSpiAdapter::new(&config.device_name) {
                Ok(adapter) => adapter,
                Err(e) => panic!(
                    "{}: Could not instantiate adapter for RGB LED {e:?}",
                    module_path!()
                ),
            };
            Ws2812B { adapter, config }
        }
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub fn set_color(&mut self, color: RgbLedColor) {
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

        match self.adapter.write_encoded_rgb(&spi_encoded_rgb_bits) {
            Ok(_) => {}
            Err(e) => {
                warn!(target: module_path!(), "Could not set color {e:?}");
            }
        }
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
