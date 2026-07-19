/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::NaiveTime;
use serde_derive::Deserialize;

/// This struct holds the configuration data for the ambient temperature time profile.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct AmbientTemperatureTimeProfileConfig {
    /// time of day when the temperature starts to ramp up linearly
    pub time_ramp_up_start: NaiveTime,

    /// time of day when the temperature is fully ramped up
    pub time_ramp_up_finish: NaiveTime,

    /// ime of day when the temperature is starting to ramp down linearly
    pub time_ramp_down_start: NaiveTime,

    /// time of day when the temperature is fully ramped down
    pub time_ramp_down_finish: NaiveTime,

    /// ambient temperature in °C during the low phase (night)
    pub ambient_temperature_low: f32,

    /// ambient temperature in °C during the high phase (day)
    pub ambient_temperature_high: f32,
}

/// This struct contains the configuration and the implementation of the ambient temperature time profile.
pub struct AmbientTemperatureTimeProfile {
    /// The configuration data read from the .toml file.
    pub config: AmbientTemperatureTimeProfileConfig,
}

impl AmbientTemperatureTimeProfile {
    /// calculates ambient temperature in °C at a given time of day using profile parameters provided in the configuration
    ///
    /// # Arguments:
    ///
    /// * `time_of_day` - parameter of type NaiveTime that holds the time of day for which to calculate the ambient temperature
    pub fn ambient_temperature(&self, time_of_day: NaiveTime) -> f32 {
        if time_of_day <= self.config.time_ramp_up_start {
            // Before ramp up
            self.config.ambient_temperature_low
        } else if (time_of_day > self.config.time_ramp_up_start)
            && (time_of_day <= self.config.time_ramp_up_finish)
        {
            // Temperature is ramping up
            let delta_time = time_of_day - self.config.time_ramp_up_start;
            let period_length = self.config.time_ramp_up_finish - self.config.time_ramp_up_start;
            let position_in_period: f32 =
                delta_time.num_seconds() as f32 / period_length.num_seconds() as f32;
            self.config.ambient_temperature_low
                + (self.config.ambient_temperature_high - self.config.ambient_temperature_low)
                    * position_in_period
        } else if (time_of_day > self.config.time_ramp_up_finish)
            && (time_of_day <= self.config.time_ramp_down_start)
        {
            // Temperature is at constant high level
            self.config.ambient_temperature_high
        } else if (time_of_day > self.config.time_ramp_down_start)
            && (time_of_day <= self.config.time_ramp_down_finish)
        {
            // Temperature is ramping down
            let delta_time = time_of_day - self.config.time_ramp_down_start;
            let period_length =
                self.config.time_ramp_down_finish - self.config.time_ramp_down_start;
            let position_in_period: f32 =
                delta_time.num_seconds() as f32 / period_length.num_seconds() as f32;
            self.config.ambient_temperature_low
                + (self.config.ambient_temperature_high - self.config.ambient_temperature_low)
                    * (1.0 - position_in_period)
        } else
        /* time_of_day > ramp_down_finish */
        {
            // After ramp down
            self.config.ambient_temperature_low
        }
    }
}
