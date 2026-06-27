/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;

use crate::utilities::iir_filter::{IIRFilter, IIRFilterConfig};

/// This struct holds the configuration data for the heat flow calculation.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct HeatFlowConfig {
    pub coefficient_lights: f32,
    pub coefficient_environment: f32,
    pub coefficient_evaporation: f32,
    pub heater_power: f32,
    pub heater_filter: IIRFilterConfig,
}

/// This struct implements the calculation of heat flows.
pub struct HeatFlow {
    /// Configuration data read from .toml file
    pub config: HeatFlowConfig,

    /// IIRFilter variable
    filter: IIRFilter,
}

impl HeatFlow {
    /// The function initializes the filter for the heat flow from the heater and returns a variable of type HeatFlow.
    ///
    /// # Arguments:
    ///
    /// * `config_heat_flow` - parameter of type HeatFlowConfig that holds the configuration data of the Controllino device.
    /// * `time_step` - delta time used in simulation: if faster than real time, then this value is used to correct the filter factor
    pub fn new(config_heat_flow: HeatFlowConfig, _time_step: f32) -> Self {
        Self {
            filter: IIRFilter::new(
                &config_heat_flow.heater_filter,
                Some(config_heat_flow.heater_power),
                #[cfg(not(test))] // only required in real-life execution
                "heat_flow",
            ),
            config: config_heat_flow,
        }
    }

    /// The function sums up all heat flows and returns one float value representing the heat flow in Watt.
    /// # Arguments:
    ///
    /// * `light_intensity` - intensity of lights at the current time of day in %, value from 0 to 100
    /// * `temperature_water` - temperature of water in main tank in °C
    /// * `temperature_ambient` - temperature of ambient air in °C
    /// * `evaporation_flow` - water evaporation from the main tank to the environment in liters per day
    /// * `heater_is_on` - status of heater        
    pub fn heat_flow_total(
        &mut self,
        light_intensity: f32,
        temperature_water: f32,
        temperature_ambient: f32,
        evaporation_flow: f32,
        heater_is_on: bool,
    ) -> f32 {
        self.heat_flow_lights(light_intensity)
            + self.heat_flow_environment(temperature_water, temperature_ambient)
            + self.heat_flow_heater(heater_is_on)
            + self.heat_flow_evaporation(evaporation_flow, temperature_water)
    }

    /// The function calculates the heat flow from the lamps to the water and returns one float value representing the heat flow in Watt.
    /// # Arguments:
    ///
    /// * `light_intensity` - intensity of lights at the current time of day in %, value from 0 to 100
    pub fn heat_flow_lights(&self, light_intensity: f32) -> f32 {
        if light_intensity > 0.0 {
            light_intensity * self.config.coefficient_lights
        } else {
            0.0
        }
    }

    /// Function calculates the base heat flow from the water to the environment and returns one float value representing the heat flow in Watt.
    /// This heat flow shall represent the radiation either from the environment to the water or from the water to the environment.
    /// Heat may flow in both directions.
    /// # Arguments:
    ///
    /// * `temperature_water` - temperature of the water in the main tank in °C
    /// * `temperature_ambient` - temperature of the ambient air in °C
    pub fn heat_flow_environment(&self, temperature_water: f32, temperature_ambient: f32) -> f32 {
        (temperature_ambient - temperature_water) * self.config.coefficient_environment
    }

    /// Function calculates the heat flow from the heater to the water and returns one float value representing the heat flow in Watt.
    /// # Arguments:
    ///
    /// * `heater_is_on` - status of heater        
    pub fn heat_flow_heater(&mut self, heater_is_on: bool) -> f32 {
        let unfiltered = if heater_is_on {
            self.config.heater_power
        } else {
            0.0
        };

        // apply filtering to the heater
        self.filter.get_filtered_value(unfiltered)
    }

    /// Function calculates the additional heat flow from the water to the environment and returns one float value representing the heat flow in Watt.
    /// Heat flows only from the water to the environment and not vice versa.
    ///
    /// # Arguments:
    ///
    /// * `evaporation_flow_liter_per_day` - current evaporation in liters per day
    /// * `temperature_water` - temperature of the water in the main tank in °C        
    pub fn heat_flow_evaporation(
        &self,
        evaporation_flow_liter_per_day: f32,
        temperature_water: f32,
    ) -> f32 {
        // calculate the heat value only for positive evaporation values. Otherwise, there is no heat flow.
        if evaporation_flow_liter_per_day > 0.0 {
            let density_sea_water = 1.025;

            // calculate the mass of the water currently evaporated in liters per day
            let evaporation_flow_kg_per_day = evaporation_flow_liter_per_day / density_sea_water;

            // calculate the mass of water currently evaporated in liters per second
            let evaporation_flow_kg_per_second = evaporation_flow_kg_per_day / (24.0 * 3600.0);

            // calculate the temperature in °K
            let temperature_kelvin = temperature_water + 273.15;

            // molar mass of the water evaporated from the main tank in g/mol
            // Note: pure water is evaporated. The salt remains in the main tank.
            let molar_mass_water = 18.02;

            // calculate the enthalpy of the water evaporated in kJ/g
            let enthalpy_per_kilogram = 50.09
                - 0.9298 * (temperature_kelvin / 1000.0)
                - 65.19 * (temperature_kelvin / 1000.0).powf(2.0) / molar_mass_water;

            // calculate the evaporation heat flow from
            // * coefficient_evaporation (parameter configured at 1 for normal operation, deviating value used for testing)
            // * enthalpy_per_kilogram
            let heat_flow_evaporation_watt = self.config.coefficient_evaporation
                * enthalpy_per_kilogram
                * evaporation_flow_kg_per_second
                * 1000.0;

            -heat_flow_evaporation_watt // negative, because evaporation is cooling
        } else {
            0.0
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::test_server::heat_flow::HeatFlow;
    use crate::test_server::test_server_config::read_test_server_config_file;
    use assert_float_eq::*;

    fn create_heat_flow_model() -> HeatFlow {
        let config_heat_flow = read_test_server_config_file(None).heat_flow;

        HeatFlow::new(config_heat_flow, 1.0)
    }

    #[test]
    fn test_heat_flow_to_environment() {
        let heat_flow_model = create_heat_flow_model();
        assert_float_absolute_eq!(
            heat_flow_model.heat_flow_environment(25.0, 24.0),
            -50.0,
            0.001
        )
    }

    #[test]
    fn test_heat_flow_from_environment() {
        let heat_flow_model = create_heat_flow_model();
        assert_float_absolute_eq!(
            heat_flow_model.heat_flow_environment(24.0, 25.0),
            50.0,
            0.001
        )
    }

    #[test]
    fn test_no_heat_flow_to_or_from_environment() {
        let heat_flow_model = create_heat_flow_model();
        assert_float_absolute_eq!(
            heat_flow_model.heat_flow_environment(25.0, 25.0),
            0.0,
            0.001
        )
    }

    #[test]
    fn test_heat_flow_from_heater_when_heater_is_on() {
        let mut heat_flow_model = create_heat_flow_model();

        assert_float_absolute_eq!(heat_flow_model.heat_flow_heater(true), 220.0, 0.001);
    }

    #[test]
    fn test_heat_flow_from_heater_when_heater_is_off() {
        let mut heat_flow_model = create_heat_flow_model();

        assert_float_absolute_eq!(heat_flow_model.heat_flow_heater(false), 110.0, 0.001);
    }

    #[test]
    fn test_heat_flow_evaporation() {
        let heat_flow_model = create_heat_flow_model();
        assert_float_absolute_eq!(
            heat_flow_model.heat_flow_evaporation(1.0, 25.0),
            -0.5588436,
            0.00001
        )
    }

    #[test]
    fn test_no_heat_flow_evaporation() {
        let heat_flow_model = create_heat_flow_model();
        assert_float_absolute_eq!(heat_flow_model.heat_flow_evaporation(0.0, 25.0), 0.0, 0.001)
    }

    #[test]
    fn test_no_heat_flow_evaporation_with_negative_evaporation() {
        let heat_flow_model = create_heat_flow_model();
        assert_float_absolute_eq!(
            heat_flow_model.heat_flow_evaporation(-1.0, 25.0),
            0.0,
            0.001
        )
    }

    #[test]
    fn test_heat_flow_lights() {
        let heat_flow_model = create_heat_flow_model();
        assert_float_absolute_eq!(heat_flow_model.heat_flow_lights(1.0), 1.8, 0.001)
    }

    #[test]
    fn test_no_heat_flow_lights() {
        let heat_flow_model = create_heat_flow_model();
        assert_float_absolute_eq!(heat_flow_model.heat_flow_lights(0.0), 0.0, 0.001)
    }

    #[test]
    fn test_no_heat_flow_lights_with_negative_intensity() {
        let heat_flow_model = create_heat_flow_model();
        assert_float_absolute_eq!(heat_flow_model.heat_flow_lights(-1.0), 0.0, 0.001)
    }

    #[test]
    fn test_heat_flow_total() {
        let mut heat_flow_model = create_heat_flow_model();

        assert_float_absolute_eq!(
            heat_flow_model.heat_flow_total(100.0, 25.5, 22.0, 1.0, true),
            224.441,
            0.001
        );
    }
}
