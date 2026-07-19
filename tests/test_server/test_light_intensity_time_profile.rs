/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use aquarium_control::test_server::light_intensity_time_profile::LightIntensityTimeProfile;
use aquarium_control::test_server::test_server_config::read_test_server_config_file;
use chrono::NaiveTime;

fn create_light_intensity_profile() -> LightIntensityTimeProfile {
    let config = read_test_server_config_file(None);

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
