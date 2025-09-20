/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the central GPIO (General Purpose Input/Output) handler.
//!
//! This module contains the `GpioHandlerConfig` struct, which is used with `serde`
//! to deserialize the `[gpio_handler]` section from the application's TOML configuration
//! file. It holds all the pin assignments for every hardware device controlled
//! directly via GPIO.
//!
//! This configuration is validated at application startup by the `GpioHandler` to ensure
//! that all pins are unique and within the hardware's valid range, preventing
//! runtime conflicts and errors.

use serde_derive::Deserialize;

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
