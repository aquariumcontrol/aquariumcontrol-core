/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the central data structure for a single snapshot of the aquarium's state.
//!
//! This module contains the `RecorderDataFrame` struct, which acts as a comprehensive
//! data container. Its primary purpose is to aggregate all sensor readings and device
//! statuses collected during a single logging cycle by the `DataLogger` thread.

use chrono::NaiveDateTime;
use std::fmt;

#[derive(Debug, PartialEq, Clone)]
/// Contains the data to be logged wrapped into Option.
/// For each signal, where the data could not be acquired, the None value is used.
/// The None value is represented as NULL in the SQL database.
/// The struct only contains minimal implementation for development purposes.
#[derive(Default)]
pub struct RecorderDataFrame {
    // timestamp when the frame was captured
    pub timestamp: NaiveDateTime,

    // measured water temperature in °C
    pub water_temperature: Option<f32>,

    // filtered water temperature in °C
    pub water_temperature_filtered: Option<f32>,

    // measured pH value of water
    pub ph: Option<f32>,

    // filtered pH value of water
    pub ph_filtered: Option<f32>,

    // measured conductivity of water in uS/cm
    pub conductivity: Option<f32>,

    // filtered conductivity of water in uS/cm
    pub conductivity_filtered: Option<f32>,

    // status bit indicating that fresh water refill is ongoing
    pub refill_in_progress: Option<bool>,

    // measured tank level switch position (true=high, false=low)
    pub tank_level_switch_position: Option<bool>,

    // calculated tank level switch stabilized position (true=high, false=low)
    pub tank_level_switch_position_stabilized: Option<bool>,

    // tank level switch validity bit (true=invalid, false=valid)
    pub tank_level_switch_invalid: Option<bool>,

    // status bit indicating that ventilation is switched on
    pub surface_ventilation_status: Option<bool>,

    // measured ambient temperature in °C
    pub ambient_temperature: Option<f32>,

    // measured ambient humidity in %
    pub ambient_humidity: Option<f32>,

    // status bit indicating that the heater is switched on
    pub heater_status: Option<bool>,
}

impl fmt::Display for RecorderDataFrame {
    /// Provides a multi-line, human-readable output of the struct's values for debugging purposes.
    ///
    /// This implementation formats all fields of the `RecorderDataFrame` into a detailed
    /// string representation. `Option` fields are displayed as their contained value or
    /// the string "None" if empty. Boolean `Option` fields are converted to descriptive
    /// words like "yes"/"no", "high"/"low", etc. This is particularly useful for
    /// inspecting the contents of data frames during development and logging.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to the formatter, as required by the `fmt::Display` trait.
    ///
    /// # Returns
    /// A `fmt::Result` indicating whether the formatting operation was successful.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Helper to format any Option<T> where T implements Display.
        fn format_opt<T: fmt::Display>(val: Option<T>) -> String {
            val.map_or_else(|| "None".to_string(), |v| v.to_string())
        }

        // Helper to format an Option<bool> with custom "true" and "false" text.
        fn format_bool_opt(val: Option<bool>, true_text: &str, false_text: &str) -> String {
            val.map_or_else(
                || "None".to_string(),
                |v| {
                    if v {
                        true_text.to_string()
                    } else {
                        false_text.to_string()
                    }
                },
            )
        }
        writeln!(f, "\n**************************************")?;
        writeln!(f, "timestamp: {}", self.timestamp)?;
        writeln!(
            f,
            "water_temperature: {}",
            format_opt(self.water_temperature)
        )?;
        writeln!(
            f,
            "water_temperature_filtered: {}",
            format_opt(self.water_temperature_filtered)
        )?;
        writeln!(f, "ph: {}", format_opt(self.ph))?;
        writeln!(f, "ph_filtered: {}", format_opt(self.ph_filtered))?;
        writeln!(f, "conductivity: {}", format_opt(self.conductivity))?;
        writeln!(
            f,
            "conductivity_filtered: {}",
            format_opt(self.conductivity_filtered)
        )?;
        writeln!(
            f,
            "refill_in_progress: {}",
            format_bool_opt(self.refill_in_progress, "yes", "no")
        )?;
        writeln!(
            f,
            "tank_level_switch_position: {}",
            format_bool_opt(self.tank_level_switch_position, "high", "low")
        )?;
        writeln!(
            f,
            "tank_level_switch_position_stabilized: {}",
            format_bool_opt(self.tank_level_switch_position_stabilized, "high", "low")
        )?;
        writeln!(
            f,
            "tank_level_switch_invalid: {}",
            format_bool_opt(self.tank_level_switch_invalid, "invalid", "valid")
        )?;
        writeln!(
            f,
            "surface_ventilation_status: {}",
            format_bool_opt(self.surface_ventilation_status, "on", "off")
        )?;
        writeln!(
            f,
            "ambient_temperature: {}",
            format_opt(self.ambient_temperature)
        )?;
        writeln!(f, "ambient_humidity: {}", format_opt(self.ambient_humidity))?;
        writeln!(
            f,
            "heater_status: {}",
            format_bool_opt(self.heater_status, "on", "off")
        )?;
        write!(f, "**************************************")
    }
}
