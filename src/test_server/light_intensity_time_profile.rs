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

#[cfg(test)]
pub mod tests {
    use chrono::NaiveTime;
    use crate::test_server::light_intensity_time_profile::LightIntensityTimeProfile;
    use crate::test_server::test_server_config::read_test_server_config_file;

    fn create_light_intensity_profile() -> LightIntensityTimeProfile {
        let config = read_test_server_config_file("/config-test/aquarium_control_test_server.toml".to_string());

        LightIntensityTimeProfile {
            config: config.light_intensity_time_profile,
        }
    }

    #[test]
    fn test_light_intensity_before_sunrise() {
        let profile = create_light_intensity_profile();
        let time_before_sunrise = NaiveTime::from_hms_opt(5, 0, 0).unwrap();
        assert_eq!(profile.intensity(time_before_sunrise), 0.0);
    }

    #[test]
    fn test_light_intensity_rising_sun() {
        let profile = create_light_intensity_profile();
        let time_rising_sun = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
        assert_eq!(profile.intensity(time_rising_sun), 50.0);
    }

    #[test]
    fn test_light_intensity_full_sun() {
        let profile = create_light_intensity_profile();
        let time_full_sun = NaiveTime::from_hms_opt(12, 30, 0).unwrap();
        assert_eq!(profile.intensity(time_full_sun), 100.0);
    }

    #[test]
    fn test_light_intensity_setting_sun() {
        let profile = create_light_intensity_profile();
        let time_setting_sun = NaiveTime::from_hms_opt(15, 30, 0).unwrap();
        assert_eq!(profile.intensity(time_setting_sun), 50.0);
    }

    #[test]
    fn test_light_intensity_after_sunset() {
        let profile = create_light_intensity_profile();
        let time_after_sunset = NaiveTime::from_hms_opt(20, 00, 0).unwrap();
        assert_eq!(profile.intensity(time_after_sunset), 0.0);
    }
}
