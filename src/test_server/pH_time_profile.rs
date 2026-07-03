/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#![allow(non_camel_case_types)]
use chrono::NaiveTime;
use serde_derive::Deserialize;

use crate::utilities::iir_filter::{IIRFilter, IIRFilterConfig};

/// This struct holds the configuration data of the pH time profile read from .toml file.
#[derive(Deserialize)]
pub struct pHTimeProfileConfig {
    /// represents the time of day when the pH starts to ramp up linearly
    pub time_low: NaiveTime,

    /// represents the time of day when the pH starts to ramp down linearly.
    pub time_high: NaiveTime,

    /// represents the time of day when the day starts (configured as midnight)
    pub time_start_of_day: NaiveTime,

    /// represents the time of day when the day end (configured as one minute before midnight)
    pub time_end_of_day: NaiveTime,

    /// lowest pH value throughout the day occurring at the corresponding time of day
    pub pH_low: f32,

    /// highest pH throughout the day occurring at the corresponding time of day
    pub pH_high: f32,

    /// filter configuration of the filter
    pub filter: IIRFilterConfig,
}

/// This struct contains the configuration and the implementation of the pH time profile.
pub struct pHTimeProfile {
    /// configuration data read from .toml file
    pub config: pHTimeProfileConfig,

    /// IIRFilter variable
    filter: IIRFilter,
}

impl pHTimeProfile {
    /// The function returns a variable of type pHTimeProfile.
    ///
    /// # Arguments:
    ///
    /// * `pHTimeProfileConfig` - configuration data read from .toml file
    /// * `time_of_day` - time of day used for calculation of the unfiltered pH value which is used for initialization of the filter
    /// * `time_step` - time step in seconds (in case simulation shall run faster than real time)
    pub fn new(config: pHTimeProfileConfig, time_of_day: NaiveTime, _time_step: f32) -> Self {
        Self {
            filter: IIRFilter::new(
                &config.filter,
                Some(Self::pH_unfiltered(&config, time_of_day)),
                #[cfg(not(test))]
                "pH",
            ),
            config,
        }
    }

    // The function calculates unfiltered pH at a given time of day using profile parameters
    fn pH_unfiltered(config: &pHTimeProfileConfig, time_of_day: NaiveTime) -> f32 {
        // distinguish two different configurations
        if config.time_low < config.time_high {
            // first comes the low point in the day, then the high point ("low before high")
            if time_of_day < config.time_low {
                // pH is ramping down
                let delta_time = (config.time_end_of_day - config.time_high)
                    + (time_of_day - config.time_start_of_day);
                let period_length = (config.time_end_of_day - config.time_start_of_day)
                    - (config.time_high - config.time_low);
                let position_in_period: f32 =
                    delta_time.num_seconds() as f32 / period_length.num_seconds() as f32;
                config.pH_high - (config.pH_high - config.pH_low) * position_in_period
            } else if (time_of_day >= config.time_low) && (time_of_day < config.time_high) {
                // pH is ramping up
                let delta_time = time_of_day - config.time_low;
                let period_length = config.time_high - config.time_low;
                let position_in_period: f32 =
                    delta_time.num_seconds() as f32 / period_length.num_seconds() as f32;
                config.pH_low + (config.pH_high - config.pH_low) * position_in_period
            } else {
                // pH is ramping down again
                let delta_time = time_of_day - config.time_high;
                let period_length = (config.time_end_of_day - config.time_start_of_day)
                    - (config.time_high - config.time_low);
                let position_in_period: f32 =
                    delta_time.num_seconds() as f32 / period_length.num_seconds() as f32;
                config.pH_high - (config.pH_high - config.pH_low) * position_in_period
            }
        } else {
            // first comes the high point in the day, then the low point ("high before low")
            if time_of_day < config.time_high {
                // pH is ramping up
                let delta_time = (config.time_end_of_day - config.time_start_of_day)
                    - (config.time_low - config.time_start_of_day)
                    + (time_of_day - config.time_start_of_day);
                let period_length = (config.time_end_of_day - config.time_start_of_day)
                    - (config.time_low - config.time_high);
                let position_in_period: f32 =
                    delta_time.num_seconds() as f32 / period_length.num_seconds() as f32;
                config.pH_low + (config.pH_high - config.pH_low) * position_in_period
            } else if (time_of_day >= config.time_high) && (time_of_day < config.time_low) {
                // pH is ramping down
                let delta_time = time_of_day - config.time_high;
                let period_length = config.time_low - config.time_high;
                let position_in_period: f32 =
                    delta_time.num_seconds() as f32 / period_length.num_seconds() as f32;
                config.pH_high - (config.pH_high - config.pH_low) * position_in_period
            } else {
                // pH is ramping up again
                let delta_time = time_of_day - config.time_low;
                let period_length = (config.time_end_of_day - config.time_start_of_day)
                    - (config.time_low - config.time_high);
                let position_in_period: f32 =
                    delta_time.num_seconds() as f32 / period_length.num_seconds() as f32;
                config.pH_low + (config.pH_high - config.pH_low) * position_in_period
            }
        }
    }

    /// The function calculates the filtered pH value and returns it as float value.
    pub fn pH(&mut self, time_of_day: NaiveTime) -> f32 {
        self.filter
            .get_filtered_value(Self::pH_unfiltered(&self.config, time_of_day))
    }
}

#[cfg(test)]
pub mod tests {
    use crate::test_server::pH_time_profile::pHTimeProfile;
    use crate::test_server::pH_time_profile::pHTimeProfileConfig;
    use crate::utilities::iir_filter::IIRFilterConfig;
    use assert_float_eq::*;
    use chrono::NaiveTime;

    fn create_profile<'a>(
        high_before_low: bool,
        pH_low: f32,
        pH_high: f32,
        current_time: NaiveTime,
    ) -> pHTimeProfile {
        let time_high_opt: Option<NaiveTime>;
        let time_low_opt: Option<NaiveTime>;
        if high_before_low {
            time_high_opt = NaiveTime::from_hms_opt(5, 0, 0);
            time_low_opt = NaiveTime::from_hms_opt(14, 0, 0);
        } else {
            time_low_opt = NaiveTime::from_hms_opt(5, 0, 0);
            time_high_opt = NaiveTime::from_hms_opt(14, 0, 0);
        }
        let time_start_of_day_opt = NaiveTime::from_hms_opt(0, 0, 0);
        let time_end_of_day_opt = NaiveTime::from_hms_opt(23, 59, 59);
        if let (Some(time_low), Some(time_high), Some(time_start_of_day), Some(time_end_of_day)) = (
            time_low_opt,
            time_high_opt,
            time_start_of_day_opt,
            time_end_of_day_opt,
        ) {
            let pH_time_profile_config = pHTimeProfileConfig {
                time_low,
                time_high,
                time_start_of_day,
                time_end_of_day,
                pH_low,
                pH_high,
                filter: IIRFilterConfig {
                    filter_coefficient: 0.5,
                    fallback_initial_value: 0.0,
                },
            };
            pHTimeProfile::new(pH_time_profile_config, current_time, 1.0)
        } else {
            panic!("Could not construct time profile for pH.");
        }
    }

    #[test]
    fn test_pH_low_before_high_ramp_down_1() {
        let time_ramp_down_1 = NaiveTime::from_hms_opt(3, 0, 0).unwrap();
        let mut profile = create_profile(false, 0.0, 1.0, time_ramp_down_1);
        assert_float_absolute_eq!(profile.pH(time_ramp_down_1), 0.15, 0.02);
    }

    #[test]
    fn test_pH_low_before_high_ramp_up() {
        let time_ramp_up = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
        let mut profile = create_profile(false, 0.0, 1.0, time_ramp_up);
        assert_float_absolute_eq!(profile.pH(time_ramp_up), 0.44, 0.02);
    }

    #[test]
    fn test_pH_low_before_high_ramp_down_2() {
        let time_ramp_down_2 = NaiveTime::from_hms_opt(18, 0, 0).unwrap();
        let mut profile = create_profile(false, 0.0, 1.0, time_ramp_down_2);
        assert_float_absolute_eq!(profile.pH(time_ramp_down_2), 0.75, 0.02);
    }

    #[test]
    fn test_pH_high_before_low_ramp_up_1() {
        let time_ramp_up_1 = NaiveTime::from_hms_opt(3, 0, 0).unwrap();
        let mut profile = create_profile(true, 0.0, 1.0, time_ramp_up_1);
        assert_float_absolute_eq!(profile.pH(time_ramp_up_1), 0.85, 0.02);
    }

    #[test]
    fn test_pH_high_before_low_ramp_down() {
        let time_ramp_down = NaiveTime::from_hms_opt(10, 0, 0).unwrap();
        let mut profile = create_profile(true, 0.0, 1.0, time_ramp_down);
        assert_float_absolute_eq!(profile.pH(time_ramp_down), 0.45, 0.02);
    }

    #[test]
    fn test_pH_high_before_low_ramp_up_2() {
        let time_ramp_up_2 = NaiveTime::from_hms_opt(20, 0, 0).unwrap();
        let mut profile = create_profile(true, 0.0, 1.0, time_ramp_up_2);
        assert_float_absolute_eq!(profile.pH(time_ramp_up_2), 0.4, 0.02);
    }
}
