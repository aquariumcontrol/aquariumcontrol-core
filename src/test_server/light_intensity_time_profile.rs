/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::NaiveTime;
use serde_derive::Deserialize;

/// This struct contains the configuration data for the light intensity time profile.
#[derive(Deserialize)]
pub struct LightIntensityTimeProfileConfig {
    /// represents the time of day when the lights are starting to ramp up linearly
    pub time_sunrise: NaiveTime,

    /// time_full_start represents the time of day when the lights are fully ramped up
    pub time_full_start: NaiveTime,

    /// time_full_finish represents the time of day when the lights are starting to ramp down linearly
    pub time_full_finish: NaiveTime,

    /// time_sunset represents the time of day when the lights are fully ramped down
    pub time_sunset: NaiveTime,
}

/// This struct calculates the intensity of the lights depending on time of day.
pub struct LightIntensityTimeProfile {
    /// configuration data read from the .toml file
    pub config: LightIntensityTimeProfileConfig,
}

impl LightIntensityTimeProfile {
    /// returns the light intensity at a given time of day in %
    /// # Arguments:
    ///
    /// * `time_of_day` - parameter of type NaiveTime that holds the time of day for which to calculate the light intensity
    pub fn intensity(&self, time_of_day: NaiveTime) -> f32 {
        // calculates light intensity at a given time of day using profile parameters
        let full_amplitude = 100.0;
        if time_of_day <= self.config.time_sunrise {
            // Before sunrise.
            0.0
        } else if (time_of_day > self.config.time_sunrise)
            && (time_of_day <= self.config.time_full_start)
        {
            // Sun is rising.
            let delta_time = time_of_day - self.config.time_sunrise;
            let period_length = self.config.time_full_start - self.config.time_sunrise;
            let position_in_period: f32 =
                delta_time.num_seconds() as f32 / period_length.num_seconds() as f32;
            full_amplitude * position_in_period
        } else if (time_of_day > self.config.time_full_start)
            && (time_of_day <= self.config.time_full_finish)
        {
            // Sun has reached full intensity.
            full_amplitude
        } else if (time_of_day > self.config.time_full_finish)
            && (time_of_day <= self.config.time_sunset)
        {
            // Sun is setting.
            let delta_time = time_of_day - self.config.time_full_finish;
            let period_length = self.config.time_sunset - self.config.time_full_finish;
            let position_in_period: f32 =
                delta_time.num_seconds() as f32 / period_length.num_seconds() as f32;
            full_amplitude * (1.0 - position_in_period)
        } else
        /* time_of_day > time_sunset */
        {
            // After sunset.
            0.0
        }
    }
}
