/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the central `SensorManager` thread.
//!
//! This module contains the `SensorManagerConfig` struct, which is used with `serde`
//! to deserialize the `[sensor_manager]` section from the application's TOML
//! configuration file. It holds all the parameters that control how the
//! `SensorManager` aggregates data, including which physical sensors to use for
//! logical measurements and what fallback values to use in case of a sensor failure.

use serde_derive::Deserialize;

#[derive(Deserialize, Debug)]
/// Holds the configuration data for the measurement of ambient signals from DHT sensor
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
pub struct SensorManagerConfig {
    /// flag to determine if the application shall communicate with hardware directly or via TCP with a simulator
    pub use_simulator: bool,

    /// indicates if the thread shall be started or not
    pub execute: bool,

    /// replacement value for ambient temperature in case reading from DHT22 sensor fails
    pub replacement_value_ambient_temperature: f32,

    /// replacement value for ambient humidity in case reading from DHT22 sensor fails
    pub replacement_value_ambient_humidity: f32,

    /// replacement value for water temperature in case reading from I2C fails
    pub replacement_value_water_temperature: f32,

    /// replacement value for pH in case reading from I2C fails
    pub replacement_value_ph: f32,

    /// replacement value for conductivity in case reading from I2C fails
    pub replacement_value_conductivity: f32,

    /// definition of the source for the water temperature - valid values are "ds18b20" or "atlas_scientific".
    pub source_water_temperature: String,

    /// definition of the source for the ambient temperature - valid values are "ds18b20" or "dht".
    pub source_ambient_temperature: String,
}
