/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the DHT22 sensor driver.
//!
//! This module contains the `DhtConfig` struct, which is used with `serde`
//! to deserialize the `[dht]` section from the application's TOML configuration
//! file. It holds all the critical, low-level timing parameters required to
//! successfully communicate with a DHT22 temperature and humidity sensor using its
//! custom one-wire protocol.

use serde_derive::Deserialize;

/// Holds the configuration data for interacting with GPIO
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct DhtConfig {
    /// allows deactivation of the data acquisition
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub active: bool,

    /// indicates if the thread shall be started or not
    #[cfg(feature = "target_hw")]
    pub execute: bool,

    /// duration to keep pin down during start signal in microseconds
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub sensor_init_pin_down_duration_micros: u64,

    /// duration to keep pin up during start signal in microseconds
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub sensor_init_pin_up_duration_micros: u64,

    /// maximum permissible duration for the sensor to respond in microseconds
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub timeout_duration_micros: u64,

    /// duration in milliseconds the sensor is switched off before requesting transmission
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub sensor_reset_duration_millis: u64,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub sensor_startup_duration_millis: u64,

    /// duration in milliseconds the sensor is given to pause between transmissions
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub sensor_pause_duration_millis: u64,

    /// duration in milliseconds between two measurements
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub measurement_interval_millis: u64,
}
