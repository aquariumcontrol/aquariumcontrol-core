/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::NaiveTime;
use serde_derive::Deserialize;

use crate::test_server::ambient_temperature_time_profile::{
    AmbientTemperatureTimeProfile, AmbientTemperatureTimeProfileConfig,
};
use crate::test_server::evaporation::{Evaporation, EvaporationConfig};
use crate::test_server::heat_flow::{HeatFlow, HeatFlowConfig};
use crate::test_server::light_intensity_time_profile::{
    LightIntensityTimeProfile, LightIntensityTimeProfileConfig,
};
use crate::test_server::tank_geometry::TankGeometry;

/// This struct holds the configuration data read from .toml file.
#[derive(Deserialize)]
pub struct TankTemperatureModelConfig {
    /// heat capacity of water in J/(kg °K)
    pub heat_capacity: f32,

    /// density of sea water in kg/l
    pub density: f32,

    /// initial water temperature in °C at start of simulation
    pub initial_temperature: f32,
}

/// This struct contains model configuration, states and implements functionality for calculating the main tank water temperature based on data from underlying models.
pub struct TankTemperatureModel {
    /// heat flow model
    pub heat_flow: HeatFlow,

    /// temperature of the sea water in main tank in °C
    pub temperature_water: f32,

    /// light intensity time profile providing intensity in %
    pub light_intensity_time_profile: LightIntensityTimeProfile,

    /// ambient temperature time profile providing ambient air temperature in °C
    pub ambient_temperature_time_profile: AmbientTemperatureTimeProfile,

    /// evaporation model providing evaporated water in liters/day
    pub evaporation_model: Evaporation,

    /// configuration data for tank temperature model
    pub config: TankTemperatureModelConfig,

    /// state of electrical heater located in sump
    pub heater_is_on: bool,

    /// state of electrical surface ventilation
    pub surface_ventilation_is_on: bool,
}

impl TankTemperatureModel {
    /// Function returns a variable of type TankTemperatureModel after having initialized all underlying models.
    /// # Arguments:
    ///
    /// * `config_tank_temperature_model` - configuration of tank temperature model
    /// * `config_heat_flow` - configuration of heat flow model
    /// * `config_light_intensity_time_profile` - configuration of light intensity time profile
    /// * `config_ambient_temperature_time_profile` - configuration of ambient air temperature time profile
    /// * `config_evaporation` - configuration of water evaporation model
    /// * `time_step` - time step in seconds (in case simulation shall run faster than real time)
    pub fn new(
        config_tank_temperature_model: TankTemperatureModelConfig,
        config_heat_flow: HeatFlowConfig,
        config_light_intensity_time_profile: LightIntensityTimeProfileConfig,
        config_ambient_temperature_time_profile: AmbientTemperatureTimeProfileConfig,
        config_evaporation: EvaporationConfig,
        time_step: f32,
    ) -> TankTemperatureModel {
        let evaporation_model = Evaporation {
            config: config_evaporation,
        };

        TankTemperatureModel {
            heat_flow: HeatFlow::new(config_heat_flow, time_step),
            temperature_water: config_tank_temperature_model.initial_temperature,
            light_intensity_time_profile: LightIntensityTimeProfile {
                config: config_light_intensity_time_profile,
            },
            ambient_temperature_time_profile: AmbientTemperatureTimeProfile {
                config: config_ambient_temperature_time_profile,
            },
            evaporation_model,
            config: config_tank_temperature_model,
            heater_is_on: false,
            surface_ventilation_is_on: false,
        }
    }

    /// Function calculates temperature change induced by heat flow from water to environment or vice versa and updates water mass in main tank considering the evaporation.
    /// Function does not return any value.
    /// # Arguments:
    ///
    /// * `water_volume` - reference to tank geometry model
    /// * `time_of_day` - time of day for which to execute the calculation
    /// * `time_step` - time step in seconds (in case simulation shall run faster than real time)
    pub fn temperature_iteration(
        &mut self,
        water_volume: &mut TankGeometry,
        time_of_day: NaiveTime,
        time_step: f32,
    ) {
        // calculate ambient temperature at time of day
        let temperature_ambient = self
            .ambient_temperature_time_profile
            .ambient_temperature(time_of_day);

        // calculate light intensity at time of day
        let light_intensity = self.light_intensity_time_profile.intensity(time_of_day);

        // calculate evaporation flow
        let evaporation_flow = self
            .evaporation_model
            .evaporation_flow(temperature_ambient, self.surface_ventilation_is_on);

        // calculate total heat flow using calculated light intensity, ambient temperature
        // and tank temperature from previous iteration
        let heat_flow_total = self.heat_flow.heat_flow_total(
            light_intensity,
            self.temperature_water,
            temperature_ambient,
            evaporation_flow,
            self.heater_is_on,
        );

        // calculate water mass from volume divided by density
        let water_kg = water_volume.get_water_volume() / self.config.density;

        // update temperature considering heat capacity of water (4.184 Joule)
        self.temperature_water +=
            time_step * heat_flow_total / (self.config.heat_capacity /* J / (kg K) */ * water_kg);

        // calculate evaporated water in liters / second
        let water_loss = time_step * evaporation_flow / 3600.0 / 24.0;

        // reduce water in tank by the amount which evaporated
        water_volume.reduce_water_volume(water_loss);
    }
}

#[cfg(test)]
pub mod tests {
    use assert_float_eq::*;
    use chrono::Duration;
    use chrono::NaiveTime;

    use crate::test_server::ambient_temperature_time_profile::AmbientTemperatureTimeProfile;
    use crate::test_server::ambient_temperature_time_profile::AmbientTemperatureTimeProfileConfig;
    use crate::test_server::test_server_config::read_test_server_config_file;
    use crate::test_server::evaporation::Evaporation;
    use crate::test_server::heat_flow::HeatFlow;
    use crate::test_server::light_intensity_time_profile::LightIntensityTimeProfile;
    use crate::test_server::tank_geometry::TankGeometry;
    use crate::test_server::thermal_model::TankTemperatureModel;

    #[test]
    fn test_temperature_stabilization_with_heater_switched_on() {
        let config = read_test_server_config_file("/config-test/aquarium_control_test_server.toml".to_string());

        let light_intensity = LightIntensityTimeProfile {
            config: config.light_intensity_time_profile,
        };

        let mut tank_geometry = TankGeometry::new(config.tank_geometry);

        let evaporation = Evaporation {
            config: config.evaporation,
        };

        let mut tank_temperature_model = TankTemperatureModel {
            heat_flow: HeatFlow::new(config.heat_flow, 1.0),
            temperature_water: 25.0.into(),
            light_intensity_time_profile: light_intensity,
            ambient_temperature_time_profile: AmbientTemperatureTimeProfile {
                config: config.ambient_temperature_time_profile,
            },
            evaporation_model: evaporation,
            config: config.tank_temperature_model,
            heater_is_on: true.into(),
            surface_ventilation_is_on: false.into(),
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
        let config = read_test_server_config_file("/config-test/aquarium_control_test_server.toml".to_string());

        let light_intensity = LightIntensityTimeProfile {
            config: config.light_intensity_time_profile,
        };

        let mut tank_geometry = TankGeometry::new(config.tank_geometry);

        let evaporation = Evaporation {
            config: config.evaporation,
        };

        let warmer_ambient_temperature_time_profile =
            create_warmer_ambient_temperature_time_profile(
                25.5,
                27.0,
                &config.ambient_temperature_time_profile,
            );

        let mut tank_temperature_model = TankTemperatureModel {
            heat_flow: HeatFlow::new(config.heat_flow, 1.0),
            temperature_water: 25.0.into(),
            light_intensity_time_profile: light_intensity,
            ambient_temperature_time_profile: warmer_ambient_temperature_time_profile,
            evaporation_model: evaporation,
            config: config.tank_temperature_model,
            heater_is_on: false.into(),
            surface_ventilation_is_on: true.into(),
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
}
