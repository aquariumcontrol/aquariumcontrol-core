/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use log::warn;
use serde_derive::Deserialize;
use ws2818_rgb_led_spi_driver::adapter_gen::WS28xxAdapter;
use ws2818_rgb_led_spi_driver::adapter_spi::WS28xxSpiAdapter;

const LOG_TAG_WS2812B: &str = "WS2812B";

pub enum RgbLedColor {
    Red,
    Green,
    Blue,
}

pub fn get_ws2812b_color_bytes(color: RgbLedColor) -> &'static [u8; 3] {
    match color {
        // WS2812B is commonly GRB: Green, Red, Blue
        // [G, R, B]
        RgbLedColor::Red => &[0, 255, 0],   // Green=0, Red=255, Blue=0
        RgbLedColor::Green => &[255, 0, 0], // Green=255, Red=0, Blue=0
        RgbLedColor::Blue => &[0, 0, 255],  // Green=0, Red=0, Blue=255
    }
}

#[derive(Deserialize)]
pub struct Ws2812BConfig {
    pub device_name: String,
}
pub struct Ws2812B {
    adapter: WS28xxSpiAdapter,
}

impl Ws2812B {
    pub fn new(config: Ws2812BConfig) -> Ws2812B {
        let adapter = match WS28xxSpiAdapter::new(&config.device_name) {
            Ok(adapter) => adapter,
            Err(e) => panic!("{}: Could not instantiate adapter for RGB LED {e:?}", LOG_TAG_WS2812B),
        };
        
        Ws2812B {
            adapter
        }
    }

    pub fn set_color(&mut self, color: RgbLedColor) {
        let color_code = get_ws2812b_color_bytes(color);

        match self.adapter.write_encoded_rgb(color_code) {
            Ok(_) => {},
            Err(e) => {
                warn!("{}: Could not set color {e:?}", LOG_TAG_WS2812B);
            }
        }
    }
}