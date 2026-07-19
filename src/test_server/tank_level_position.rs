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
