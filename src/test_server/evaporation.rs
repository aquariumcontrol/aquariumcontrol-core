/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;

#[derive(Deserialize)]
/// configuration for the evaporation crate read from the the .toml configuration file
pub struct EvaporationConfig {
    /// base value for evaporation in L/day
    pub constant1: f32,

    /// temperature in °C from which on evaporation starts
    pub constant2: f32,

    /// temperature-proportional value in L/day/°C
    pub constant3: f32,

    /// additional base evaporation in L/d when surface ventilation is switched on in L/day
    pub constant4: f32,

    /// starting point for additional evaporation from surface ventilation in °C
    pub constant5: f32,

    // additional temperature-difference dependend evaporation in L/d/°C when surface ventilation is switched on
    pub constant6: f32,
}

/// Calculation of water evaporation in L/day
pub struct Evaporation {
    pub config: EvaporationConfig,
}

impl Evaporation {
    /// Function calculates an always positive temperature difference value between ambient temperature and a configurable value
    /// Rationale: evaporation goes from tank to the environment, not vice-versa
    fn temperature_delta_positive(ambient_temperature: f32, temperature_reference: f32) -> f32 {
        if ambient_temperature > temperature_reference {
            ambient_temperature - temperature_reference
        } else {
            0.0
        }
    }

    /// Calculates the water evaporation in liters/day.
    ///
    /// # Arguments
    ///
    /// * `ambient_temperature` - Ambient temperature in °C
    /// * `surface_ventilation` - surface ventilation status: True indicates the surface ventilation is active
    ///   
    pub fn evaporation_flow(&self, ambient_temperature: f32, surface_ventilation: bool) -> f32 {
        let delta_temp_base =
            Self::temperature_delta_positive(ambient_temperature, self.config.constant2);

        let delta_temp_surface_ventilation =
            Self::temperature_delta_positive(ambient_temperature, self.config.constant5);

        let base_evaporation = self.config.constant1 + delta_temp_base * self.config.constant3;

        if surface_ventilation {
            base_evaporation
                + self.config.constant4
                + delta_temp_surface_ventilation * self.config.constant6
        } else {
            base_evaporation
        }
    }
}

#[cfg(test)]
pub mod tests {
    use assert_float_eq::*;

    use crate::test_server::evaporation::{Evaporation, EvaporationConfig};

    #[test]
    fn test_evaporation_without_ventilation() {
        let evaporation_config = EvaporationConfig {
            constant1: 1.0,
            constant2: 1.0,
            constant3: 1.0,
            constant4: 1.0,
            constant5: 1.0,
            constant6: 1.0,
        };

        let evaporation = Evaporation {
            config: evaporation_config,
        };

        assert_float_absolute_eq!(evaporation.evaporation_flow(22.0, false), 22.0, 0.001);
    }

    #[test]
    fn test_evaporation_with_ventilation() {
        let evaporation_config = EvaporationConfig {
            constant1: 1.0,
            constant2: 1.0,
            constant3: 1.0,
            constant4: 1.0,
            constant5: 1.0,
            constant6: 1.0,
        };

        let evaporation = Evaporation {
            config: evaporation_config,
        };

        assert_float_absolute_eq!(evaporation.evaporation_flow(22.0, true), 44.0, 0.001);
    }
}
