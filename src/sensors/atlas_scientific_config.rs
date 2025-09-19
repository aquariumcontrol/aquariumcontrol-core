/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the Atlas Scientific sensor management system.
//!
//! This module contains the `AtlasScientificConfig` struct, which is used with `serde`
//! to deserialize the `[atlas_scientific]` section from the application's TOML
//! configuration file. It holds all the parameters required to manage the sensor
//! measurement cycle, from I2C addresses and timings to the valid ranges for
//! sensor readings.

use serde_derive::Deserialize;

/// Holds the configuration data for the Atlas Scientific sensor communication.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct AtlasScientificConfig {
    /// allows deactivation of the data acquisition
    #[allow(unused)] // used in conditionally compiled code
    pub active: bool,

    /// indicates if the thread shall be started or not
    pub execute: bool,

    /// I2C address of Atlas Scientific pH sensor unit
    #[allow(unused)]
    pub address_atlas_scientific_ph: u16,

    /// I2C address of Atlas Scientific conductivity sensor unit
    #[allow(unused)]
    pub address_atlas_scientific_conductivity: u16,

    /// I2C address of Atlas Scientific temperature sensor unit    
    #[allow(unused)]
    pub address_atlas_scientific_temperature: u16,

    /// sleep time in milliseconds before after writing before reading from the pH sensor unit
    #[allow(unused)]
    pub sleep_time_millis_ph: u64,

    /// sleep time in milliseconds before after writing before reading from the conductivity sensor unit
    #[allow(unused)]
    pub sleep_time_millis_conductivity: u64,

    /// sleep time in milliseconds before after writing before reading from the temperature sensor unit
    #[allow(unused)]
    pub sleep_time_millis_temperature: u64,

    /// maximum allowed value for pH used to assess if the temperature sensor reading is valid
    #[allow(unused)]
    pub max_val_atlas_scientific_ph: f32,

    /// minimum allowed value for pH used to assess if the temperature sensor reading is valid
    #[allow(unused)]
    pub min_val_atlas_scientific_ph: f32,

    /// maximum allowed value for temperature used to assess if the temperature sensor reading is valid
    #[allow(unused)]
    pub max_val_atlas_scientific_temperature: f32,

    /// minimum allowed value for temperature used to assess if the temperature sensor reading is valid
    #[allow(unused)]
    pub min_val_atlas_scientific_temperature: f32,

    /// maximum allowed value for conductivity used to assess if the conductivity sensor reading is valid
    #[allow(unused)]
    pub max_val_atlas_scientific_conductivity: f32,

    /// minimum allowed value for conductivity used to assess if the conductivity sensor reading is valid
    #[allow(unused)]
    pub min_val_atlas_scientific_conductivity: f32,

    /// measurement interval in milliseconds
    pub measurement_interval_millis: u64,
}
