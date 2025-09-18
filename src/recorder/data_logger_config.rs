/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the central data logging system.
//!
//! This module contains the `DataLoggerConfig` struct, which is used with `serde` to
//! deserialize the `[data_logger]` section from the application's TOML configuration file.
//! It holds all the parameters that control the behavior of the `DataLogger` thread,
//! from its activation status and logging frequency to the specific file paths for
//! real-time data output and the coefficients for signal filtering.

#![allow(non_snake_case)]
use crate::utilities::iir_filter::IIRFilterConfig;
use serde_derive::Deserialize;

/// Holds the configuration data for the regular data logging to SQL database.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct DataLoggerConfig {
    /// data logging will only be active when the flag is set to true
    pub active: bool,

    /// indicates if the thread shall be started or not
    pub execute: bool,

    /// switch indicating if a self-generated timestamp or the database-generated timestamp shall be used
    pub use_sql_timestamp: bool,

    /// interval of the data logger in seconds
    pub logging_interval: u64,

    /// filter coefficient for the filtered version of the water temperature signal
    pub filter_coefficient_water_temperature: IIRFilterConfig<()>,

    /// filter coefficient for the filtered version of the pH signal
    pub filter_coefficient_pH: IIRFilterConfig<()>,

    /// filter coefficient for the filtered version of the conductivity signal
    pub filter_coefficient_conductivity: IIRFilterConfig<()>,

    /// switch if output to file (in ramdisk) shall be written
    pub output_to_disk: bool,

    /// output file name (in ramdisk) for the current timestamp
    pub output_file_name_timestamp: String,

    /// output file name (in ramdisk) for current value of water temperature
    pub output_file_name_water_temperature: String,

    /// output file name (in ramdisk) for current value of water temperature filtered
    pub output_file_name_water_temperature_filtered: String,

    /// output file name (in ramdisk) for the current value of pH
    pub output_file_name_pH: String,

    /// output file name (in ramdisk) for the current value of pH filtered
    pub output_file_name_pH_filtered: String,

    /// output file name (in ramdisk) for the current value of conductivity
    pub output_file_name_conductivity: String,

    /// output file name (in ramdisk) for the current value of conductivity filtered
    pub output_file_name_conductivity_filtered: String,

    /// output file name (in ramdisk) for the current value of tank level switch position
    pub output_file_name_tank_level_switch_position: String,

    /// output file name (in ramdisk) for the current value of tank level switch invalid
    pub output_file_name_tank_level_switch_invalid: String,

    /// output file name (in ramdisk) for the current value of tank level switch position stabilized
    pub output_file_name_tank_level_switch_position_stabilized: String,

    /// output file name (in ramdisk) for the current value of ventilation
    pub output_file_name_ventilation_is_on: String,

    /// output file name (in ramdisk) for the current value of heating
    pub output_file_name_heating_is_on: String,

    /// output file name (in ramdisk) for the current value of ambient air temperature
    pub output_file_name_ambient_temperature: String,

    /// output file name (in ramdisk) for the current value of air humidity
    pub output_file_name_humidity: String,

    /// output file name (in ramdisk) for the current refill pump status
    pub output_file_name_refill_in_progress: String,

    /// sleep time in milliseconds before starting the data recording
    pub initial_sleep_time_millis: u64,
}
