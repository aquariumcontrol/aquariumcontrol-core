/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;

/// This struct holds the configuration data read from .toml file.
#[derive(Deserialize)]
pub struct TankGeometryConfig {
    /// width of the tank in m
    pub width: f32,

    /// length of the tank in m
    pub length: f32,

    /// operational lower limit of tank     
    pub water_volume_min: f32,

    /// operational upper limit of tank
    pub water_volume_max: f32,

    /// water volume at start of simulation
    pub initial_water_volume: f32,
}

/// This struct contains the configuration, the state and the implementation of functionality for the main tank static water level calculation.
pub struct TankGeometry {
    /// configuration data read from .toml file
    pub config: TankGeometryConfig,

    /// water volume in L calculated during simulation
    water_volume: f32,
}

impl TankGeometry {
    /// Function returns a variable of type TankGeometry after having initialized the underlying model contained in the same.
    /// # Arguments:
    ///
    /// * `config_tank_geometry` - configuration of tank geometry read from .toml file
    pub fn new(config_tank_geometry: TankGeometryConfig) -> Self {
        Self {
            water_volume: config_tank_geometry.initial_water_volume,
            config: config_tank_geometry,
        }
    }

    /// Function calculates the static water level based on water volume in the tank and dimensions of the tank and returns it as float value.
    pub fn get_water_level(&self) -> f32 {
        (self.water_volume / 1000.0) / (self.config.width * self.config.length)
    }

    /// Getter-function for the water volume
    pub fn get_water_volume(&self) -> f32 {
        self.water_volume
    }

    /// Function reduces the water volume in the tank observing the minimum limitation.
    /// It does not return any value.
    /// # Arguments:
    ///
    /// * `delta_water_volume` - amount in liters which is subtracted from the water volume    
    pub fn reduce_water_volume(&mut self, delta_water_volume: f32) {
        if self.water_volume - delta_water_volume > self.config.water_volume_min {
            self.water_volume -= delta_water_volume;
        } else {
            self.water_volume = self.config.water_volume_min;
        }
    }

    /// Function increases the water volume in the tank observing the maximum limitation.
    /// It does not return any value.
    /// # Arguments:
    ///
    /// * `delta_water_volume` - amount in liters which is added to the water volume    
    pub fn increase_water_volume(&mut self, delta_water_volume: f32) {
        if self.water_volume + delta_water_volume < self.config.water_volume_max {
            self.water_volume += delta_water_volume;
        } else {
            self.water_volume = self.config.water_volume_max;
        }
    }

    /// Sets the water volume.
    ///
    /// # Arguments
    /// * `water_volume` - A parameter representing `water_volume`.
    #[cfg(test)]
    pub fn set_water_volume(&mut self, water_volume: f32) {
        if water_volume < self.config.water_volume_min
            || water_volume > self.config.water_volume_max
        {
            panic!("Invalid water volume supplied—not within constraints of tank geometry");
        }

        self.water_volume = water_volume;
    }
}

#[cfg(test)]
pub mod tests {
    use crate::test_server::tank_geometry::TankGeometry;
    use crate::test_server::tank_geometry::TankGeometryConfig;
    use assert_float_eq::*;

    #[test]
    // Verifies that create tank geometry.
    fn test_create_tank_geometry() {
        let geometry_config = TankGeometryConfig {
            width: 1.0,
            length: 1.0,
            water_volume_max: 900.0,
            water_volume_min: 700.0,
            initial_water_volume: 800.0,
        };

        let geometry = TankGeometry::new(geometry_config);
        assert_float_absolute_eq!(geometry.get_water_level(), 0.8, 0.001);
    }

    #[test]
    // Verifies that tank geometry set water volume.
    fn test_tank_geometry_set_water_volume() {
        let geometry_config = TankGeometryConfig {
            width: 1.0,
            length: 1.0,
            water_volume_max: 900.0,
            water_volume_min: 700.0,
            initial_water_volume: 800.0,
        };

        let mut geometry = TankGeometry::new(geometry_config);

        geometry.set_water_volume(900.0);
        assert_float_absolute_eq!(geometry.get_water_level(), 0.9, 0.001);
    }
}
