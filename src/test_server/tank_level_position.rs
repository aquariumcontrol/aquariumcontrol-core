/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::test_server::sump::Sump;
use crate::test_server::tank_geometry::TankGeometry;
use serde_derive::Deserialize;

/// This struct holds the configuration data read from .toml file.
#[derive(Deserialize)]
pub struct TankLevelPositionConfig {
    pub water_level_high: f32,
    pub water_level_low: f32,
}

/// This struct contains configuration data, state and model and implements functionality to simulate the tank level switch.
/// The tank level switch is located in the sump.
/// Below a certain water level, the switch will indicate low water level.
/// Above a certain water level, the switch will indicate high water level.
/// The water level for indicating high position shall be higher than the low position.
pub struct TankLevelPosition {
    /// configuration data read from .toml file
    pub config: TankLevelPositionConfig,

    /// tank level switch position where true represents a high position and false represents a low position
    switch_position: bool,

    /// variable of type Sump which models the aquarium sump
    pub sump: Sump,
}

impl TankLevelPosition {
    /// Function returns a variable of type TankLevelPosition.
    /// # Arguments:
    ///
    /// * `tank_level_switch_config` - configuration data read from .toml file
    /// * `sump` - variable of type Sump
    pub fn new(tank_level_switch_config: TankLevelPositionConfig, sump: Sump) -> TankLevelPosition {
        TankLevelPosition {
            config: tank_level_switch_config,
            switch_position: false,
            sump,
        }
    }

    /// Function calculates the tank level switch position and returns it as bool where true represents a high position and false represents a low position.
    /// With distinct values for high and low level, a hysteresis is implemented.
    /// * `water_level_in_tank` - reference to tank geometry model
    pub fn update_and_get_switch_position(&mut self, water_level_in_tank: &TankGeometry) -> bool {
        let water_level_in_sump = self.sump.get_water_level(water_level_in_tank);
        if water_level_in_sump > self.config.water_level_high {
            if !self.switch_position {
                println!(
                    "TankLevelPosition: setting switch from off to on because water level is high."
                );
            }
            self.switch_position = true;
        } else if water_level_in_sump < self.config.water_level_low {
            if self.switch_position {
                println!(
                    "TankLevelPosition: setting switch from on to off because water level is low."
                );
            }
            self.switch_position = false;
        }
        // else: return previous position
        self.switch_position
    }

    /// Function returns a signal qualifier for the tank level switch as bool where true represents an invalid signal and false represents a valid signal.
    /// Background: Control application checks validity of GPIO pins
    /// Currently, this function only returns false, but future test case implementation might utilize this function.
    pub fn tank_level_switch_invalid(&self) -> bool {
        false
    }
}

#[cfg(test)]
pub mod tests {
    use crate::test_server::sump::Pump;
    use crate::test_server::sump::PumpStatus;
    use crate::test_server::sump::Sump;
    use crate::test_server::tank_geometry::TankGeometry;
    use crate::test_server::tank_geometry::TankGeometryConfig;
    use crate::test_server::tank_level_position::{TankLevelPosition, TankLevelPositionConfig};
    use crate::test_server::test_server_config::read_test_server_config_file;
    use crate::utilities::iir_filter::IIRFilter;
    use crate::utilities::iir_filter::IIRFilterConfig;

    #[test]
    fn test_change_low_to_high_to_low() {
        let config = read_test_server_config_file(None);

        let tank_geometry_config = TankGeometryConfig {
            width: 1.0,
            length: 1.0,
            water_volume_min: 700.0,
            water_volume_max: 900.0,
            initial_water_volume: 800.0,
        };

        let mut tank = TankGeometry::new(tank_geometry_config);

        let pump1 = Pump {
            status: PumpStatus::Off.into(),
            // delta_pressure: 0.01,
        };

        let pump2 = Pump {
            status: PumpStatus::Off.into(),
            // delta_pressure: 0.01,
        };

        let refill_pump = Pump {
            status: PumpStatus::Off.into(),
        };

        let sump = Sump {
            main_pump_1: pump1,
            main_pump_2: pump2,
            refill_pump,
            level_filter: IIRFilter::new(
                &IIRFilterConfig {
                    filter_coefficient: 1.0,
                    fallback_initial_value: 0.0,
                },
                Some(1.0),
                #[cfg(not(test))]
                "sump level",
            ),
            config: config.sump,
        };

        let tank_level_switch_config = TankLevelPositionConfig {
            water_level_high: 0.850,
            water_level_low: 0.825,
        };

        let mut tank_level_switch = TankLevelPosition::new(tank_level_switch_config, sump);

        assert_eq!(
            tank_level_switch.update_and_get_switch_position(&tank),
            false
        );

        tank.increase_water_volume(75.0);

        assert_eq!(
            tank_level_switch.update_and_get_switch_position(&tank),
            true
        );

        tank.reduce_water_volume(100.0);

        assert_eq!(
            tank_level_switch.update_and_get_switch_position(&tank),
            false
        );
    }
}
