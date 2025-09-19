/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the DS18B20 1-Wire sensor driver.
//!
//! This module contains the `Ds18b20Config` struct, which is used with `serde`
//! to deserialize the `[ds18b20]` section from the application's TOML configuration
//! file. It holds all the parameters required to locate and read temperature data
//! from DS18B20 sensors, which are exposed as files in the system's `/sys/bus/w1/devices/`
//! directory by the 1-Wire kernel driver.

use serde_derive::Deserialize;

/// Holds the configuration data for DS18B20 sensor communication.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct Ds18b20Config {
    /// used primarily for testing purposes
    pub active: bool,

    /// indicates if thread shall be started or not
    pub execute: bool,

    /// pause duration between two measurements in milliseconds
    pub measurement_pause_duration_millis: u64,

    /// system driver base path
    pub system_driver_base_path: String,

    /// system driver sensor-specific path
    pub system_driver_sensor_path_prefix: String,

    /// system driver file name
    pub system_driver_file_name: String,

    #[allow(unused)]
    /// max. number of retries
    pub max_retries: u32,

    #[allow(unused)]
    /// duration between retries in milliseconds
    pub retry_pause_duration_millis: u64,

    /// unique 64-bit ID of water temperature sensor
    pub water_temperature_sensor_id: String,

    /// unique 64-bit ID of ambient temperature sensor
    pub ambient_temperature_sensor_id: String,
}
