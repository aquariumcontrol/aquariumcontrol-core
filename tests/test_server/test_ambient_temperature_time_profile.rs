/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Test the configuration process of the ambient temperature time profile.

use aquarium_control::test_server::ambient_temperature_time_profile::AmbientTemperatureTimeProfile;
use aquarium_control::test_server::test_server_config::read_test_server_config_file;
use chrono::NaiveTime;

fn create_profile() -> AmbientTemperatureTimeProfile {
    let config = read_test_server_config_file(None);

    AmbientTemperatureTimeProfile {
        config: config.ambient_temperature_time_profile,
    }
}

#[test]
fn test_ambient_temperature_before_ramp_up() {
    let profile = create_profile();
    let time_before_ramp_up = NaiveTime::from_hms_opt(5, 0, 0).unwrap();
    assert_eq!(profile.ambient_temperature(time_before_ramp_up), 22.0);
}

#[test]
fn test_ambient_temperature_ramp_up() {
    let profile = create_profile();
    let time_ramp_up = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
    assert_eq!(profile.ambient_temperature(time_ramp_up), 23.0);
}

#[test]
fn test_ambient_temperature_constant_high() {
    let profile = create_profile();
    let time_constant_high = NaiveTime::from_hms_opt(12, 30, 0).unwrap();
    assert_eq!(profile.ambient_temperature(time_constant_high), 24.0);
}

#[test]
fn test_ambient_temperature_ramp_down() {
    let profile = create_profile();
    let time_ramp_down = NaiveTime::from_hms_opt(15, 30, 0).unwrap();
    assert_eq!(profile.ambient_temperature(time_ramp_down), 23.0);
}

#[test]
fn test_ambient_temperature_after_ramp_down() {
    let profile = create_profile();
    let time_after_ramp_down = NaiveTime::from_hms_opt(20, 00, 0).unwrap();
    assert_eq!(profile.ambient_temperature(time_after_ramp_down), 22.0);
}
