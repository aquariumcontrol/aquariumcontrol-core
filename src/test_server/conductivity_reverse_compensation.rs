/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;

/// This struct holds the configuration data of the conductivity reverse compensation read from .toml file.
#[derive(Deserialize)]
pub struct ConductivityReverseCompensationConfig {
    /// base (undistorted) conductivity of seawater in microS/cm
    pub conductivity_base: f32,

    /// distortion coefficient
    pub conductivity_reverse_compensation_coefficient: f32,

    /// reference temperature for distortion calculation in °C
    pub conductivity_reverse_compensation_reference_temperature: f32,
}

/// This struct contains the configuration and the implementation of the conductivity reverse compensation.
pub struct ConductivityReverseCompensation {
    pub config: ConductivityReverseCompensationConfig,
}

impl ConductivityReverseCompensation {
    /// The function returns a variable of type ConductivityReverseCompensation.
    ///
    /// # Arguments:
    ///
    /// * `config_conductivity_reverse_compensation` - configuration data read from .toml file
    pub fn new(
        config_conductivity_reverse_compensation: ConductivityReverseCompensationConfig,
    ) -> Self {
        Self {
            config: config_conductivity_reverse_compensation,
        }
    }

    /// The function returns a distorted conductivity value which simulates the impact of temperature variation on the conductivity measurement of seawater.
    /// # Arguments:
    ///
    /// * `conductivity_base` - undistorted conductivity value
    /// * `temperature_water` - water temperature in °C
    pub fn conductivity_reverse_compensated(&self, temperature_water: f32) -> f32 {
        self.config.conductivity_base
            * (1.0
                - (self
                    .config
                    .conductivity_reverse_compensation_reference_temperature
                    - temperature_water)
                    * self.config.conductivity_reverse_compensation_coefficient
                    / 100.0)
    }
}
