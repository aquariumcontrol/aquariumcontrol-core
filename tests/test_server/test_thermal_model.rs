/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use assert_float_eq::*;
use chrono::Duration;
use chrono::NaiveTime;

use aquarium_control::test_server::ambient_temperature_time_profile::AmbientTemperatureTimeProfile;
use aquarium_control::test_server::ambient_temperature_time_profile::AmbientTemperatureTimeProfileConfig;
use aquarium_control::test_server::evaporation::Evaporation;
use aquarium_control::test_server::heat_flow::HeatFlow;
use aquarium_control::test_server::light_intensity_time_profile::LightIntensityTimeProfile;
use aquarium_control::test_server::tank_geometry::TankGeometry;
use aquarium_control::test_server::test_server_config::read_test_server_config_file;
use aquarium_control::test_server::thermal_model::TankTemperatureModel;

#[test]
fn test_temperature_stabilization_with_heater_switched_on() {
    let config = read_test_server_config_file(None);

    let light_intensity = LightIntensityTimeProfile {
        config: config.light_intensity_time_profile,
    };

    let mut tank_geometry = TankGeometry::new(config.tank_geometry);

    let evaporation = Evaporation {
        config: config.evaporation,
    };

    let mut tank_temperature_model = TankTemperatureModel {
        heat_flow: HeatFlow::new(config.heat_flow, 1.0),
        temperature_water: 25.0,
        light_intensity_time_profile: light_intensity,
        ambient_temperature_time_profile: AmbientTemperatureTimeProfile {
            config: config.ambient_temperature_time_profile,
        },
        evaporation_model: evaporation,
        config: config.tank_temperature_model,
        heater_is_on: true,
        surface_ventilation_is_on: false,
    };

    let time_of_day_opt = NaiveTime::from_hms_opt(13, 0, 0);

    if let Some(mut time_of_day) = time_of_day_opt {
        println!(
            "Temperature at start of simulation: {}°C",
            tank_temperature_model.temperature_water
        );
        for _ in 0..3600 * 24 {
            tank_temperature_model.temperature_iteration(&mut tank_geometry, time_of_day, 1.0);
            time_of_day += Duration::try_seconds(1).unwrap();
            tank_temperature_model.temperature_iteration(&mut tank_geometry, time_of_day, 1.0);
        }
        println!(
            "Temperature at finish of simulation: {}°C",
            tank_temperature_model.temperature_water
        );
        assert_float_absolute_eq!(tank_temperature_model.temperature_water, 30.348, 0.002);
    } else {
        panic!("Could not construct time profile for light.");
    }
}

/// Performs the `create_warmer_ambient_temperature_time_profile` operation.
///
/// # Arguments
/// * `temperature_low` - A parameter representing `temperature_low`.
/// * `temperature_high` - A parameter representing `temperature_high`.
/// * `ambient_temperature_time_profile_config` - A parameter representing `ambient_temperature_time_profile_config`.
fn create_warmer_ambient_temperature_time_profile(
    temperature_low: f32,
    temperature_high: f32,
    ambient_temperature_time_profile_config: &AmbientTemperatureTimeProfileConfig,
) -> AmbientTemperatureTimeProfile {
    let warmer_config = AmbientTemperatureTimeProfileConfig {
        time_ramp_up_start: ambient_temperature_time_profile_config.time_ramp_up_start,
        time_ramp_up_finish: ambient_temperature_time_profile_config.time_ramp_up_finish,
        time_ramp_down_start: ambient_temperature_time_profile_config.time_ramp_down_start,
        time_ramp_down_finish: ambient_temperature_time_profile_config.time_ramp_down_finish,
        ambient_temperature_low: temperature_low,
        ambient_temperature_high: temperature_high,
    };
    AmbientTemperatureTimeProfile {
        config: warmer_config,
    }
}

#[test]
fn test_temperature_stabilization_with_ventilation_switched_on() {
    let config = read_test_server_config_file(None);

    let light_intensity = LightIntensityTimeProfile {
        config: config.light_intensity_time_profile,
    };

    let mut tank_geometry = TankGeometry::new(config.tank_geometry);

    let evaporation = Evaporation {
        config: config.evaporation,
    };

    let warmer_ambient_temperature_time_profile = create_warmer_ambient_temperature_time_profile(
        25.5,
        27.0,
        &config.ambient_temperature_time_profile,
    );

    let mut tank_temperature_model = TankTemperatureModel {
        heat_flow: HeatFlow::new(config.heat_flow, 1.0),
        temperature_water: 25.0,
        light_intensity_time_profile: light_intensity,
        ambient_temperature_time_profile: warmer_ambient_temperature_time_profile,
        evaporation_model: evaporation,
        config: config.tank_temperature_model,
        heater_is_on: false,
        surface_ventilation_is_on: true,
    };

    let time_of_day_opt = NaiveTime::from_hms_opt(13, 0, 0);

    if let Some(mut time_of_day) = time_of_day_opt {
        println!(
            "Temperature at start of simulation: {}°C",
            tank_temperature_model.temperature_water
        );
        for _ in 0..3600 * 24 {
            tank_temperature_model.temperature_iteration(&mut tank_geometry, time_of_day, 1.0);
            time_of_day += Duration::try_seconds(1).unwrap();
            tank_temperature_model.temperature_iteration(&mut tank_geometry, time_of_day, 1.0);
        }
        println!(
            "Temperature at finish of simulation: {}°C",
            tank_temperature_model.temperature_water
        );
        assert_float_absolute_eq!(tank_temperature_model.temperature_water, 29.060, 0.002);
    } else {
        panic!("Could not construct time profile for light.");
    }
}
