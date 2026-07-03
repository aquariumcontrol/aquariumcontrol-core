/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::test_server::tank_geometry::TankGeometry;
use crate::utilities::iir_filter::{IIRFilter, IIRFilterConfig};
use serde_derive::Deserialize;

/// This enumeration describes the status of a pump.
#[derive(PartialEq, Clone, Copy, Deserialize)]
pub enum PumpStatus {
    #[serde(alias = "on", alias = "On", alias = "ON")]
    On,
    #[serde(alias = "off", alias = "Off", alias = "OFF")]
    Off,
}

/// This struct is used to describe the status of a pump.
pub struct Pump {
    /// Status of the pump (enum)
    pub status: PumpStatus,
}

/// This struct holds the configuration data read from .toml file.
#[derive(Deserialize)]
pub struct SumpConfig {
    /// density of sea water in kg/m^3
    pub rho_sea_water: f32,

    /// gravitation at location of aquarium in m/s^2
    pub gravitation: f32,

    /// the delta pressure which one pump creates in bar          
    pub main_pump_delta_pressure: f32,

    /// flow rate of the refill pump in L/s
    pub refill_pump_flow_rate: f32,

    /// filter configuration for the main pumps pressure increase
    pub level_filter: IIRFilterConfig,

    /// initial state of main pump #1
    pub main_pump_1_initial_state: PumpStatus,

    /// initial state of main pump #2
    pub main_pump_2_initial_state: PumpStatus,

    /// initial state of main pump #2
    pub refill_pump_initial_state: PumpStatus,
}

/// This struct contains the configuration and the implementation of functionality for the sump of the aquarium.
/// The sump is a separated compartment of the fish tank.
/// It contains a part of the overall water volume.
/// Inside the sump, there are pumps which pump from the sump to the main tank.
/// The operation of the pump results in lowering the water level in the sump and increasing water level in the main tank.
/// This is relevant, because the tank level switch resides in the sump.
/// This module calculates the decrease of water level when switching on the pumps.
pub struct Sump {
    /// Status of main pump #1
    pub main_pump_1: Pump,

    /// Status of main pump #2
    pub main_pump_2: Pump,

    /// Status of the refill pump
    pub refill_pump: Pump,

    /// configuration of the sump read from .toml file
    pub config: SumpConfig,

    /// filter variable for the sump level
    pub level_filter: IIRFilter,
}

impl Sump {
    /// The function returns a variable of type Sump.
    ///
    /// # Arguments:
    ///
    /// * `sump_config` - configuration data read from .toml file
    /// * `time_step` - delta time used in simulation: if faster than real time, then this value is used to correct the filter factor
    pub fn new(sump_config: SumpConfig, _time_step: f32) -> Sump {
        Sump {
            main_pump_1: Pump {
                status: sump_config.main_pump_1_initial_state,
            },
            main_pump_2: Pump {
                status: sump_config.main_pump_2_initial_state,
            },
            refill_pump: Pump {
                status: sump_config.refill_pump_initial_state,
            },
            level_filter: IIRFilter::new(
                &sump_config.level_filter,
                None,
                #[cfg(not(test))]
                "sump level",
            ),
            config: sump_config,
        }
    }

    /// Function calculates the decrease of water level in the sump induced by one main pump and returns it as float value.
    pub fn calc_main_pump_delta_level(&self) -> f32 {
        self.config.main_pump_delta_pressure * 100000.0 /* conversion from bar to N/^m2 */  / (self.config.rho_sea_water * self.config.gravitation)
    }

    /// The function calculates the unfiltered water level in the sump and returns it as float value.
    ///
    /// # Arguments:
    ///
    /// * `water_volume_in_tank` - model of tank geometry of type TankGeometry
    pub fn get_water_level_unfiltered(&self, water_volume_in_tank: &TankGeometry) -> f32 {
        let pump_1_level_reduction: f32 = if self.main_pump_1.status == PumpStatus::On {
            // reducing sump water level because pump 1 is on
            self.calc_main_pump_delta_level()
        } else {
            // not reducing sump water level because pump 1 is off
            0.0
        };
        let pump_2_level_reduction: f32 = if self.main_pump_2.status == PumpStatus::On {
            // reducing sump water level because pump 2 is on;
            self.calc_main_pump_delta_level()
        } else {
            // not reducing sump water level because pump 2 is off
            0.0
        };
        let water_level_sum = water_volume_in_tank.get_water_level()
            - pump_1_level_reduction
            - pump_2_level_reduction;
        water_level_sum.max(0.0)
    }

    /// The function calculates the filtered water level in the sump and returns it as float value.
    ///
    /// # Arguments:
    ///
    /// * `water_volume_in_tank` - model of tank geometry of type TankGeometry
    pub fn get_water_level(&mut self, water_volume_in_tank: &TankGeometry) -> f32 {
        self.level_filter
            .get_filtered_value(self.get_water_level_unfiltered(water_volume_in_tank))
    }

    /// The function checks if refill pump is switched on.
    /// If yes, then the function increases the water volume in tank by fixed amount during each execution.
    /// The function does not return any value.
    /// # Arguments:
    ///
    /// * `water_volume_in_tank` - model of tank geometry of type TankGeometry
    /// * `time_step` - time step in seconds (in case simulation shall run faster than real time)
    pub fn calc_refill(&self, water_volume: &mut TankGeometry, time_step: f32) {
        if self.refill_pump.status == PumpStatus::On {
            println!(
                "Sump: increasing water volume in tank by {}",
                self.config.refill_pump_flow_rate,
            );
            water_volume.increase_water_volume(self.config.refill_pump_flow_rate * time_step);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::test_server::sump::Pump;
    use crate::test_server::sump::PumpStatus;
    use crate::test_server::sump::Sump;
    use crate::test_server::tank_geometry::TankGeometry;
    use crate::test_server::tank_geometry::TankGeometryConfig;
    use crate::test_server::test_server_config::read_test_server_config_file;
    use crate::utilities::iir_filter::{IIRFilter, IIRFilterConfig};
    use assert_float_eq::*;

    #[test]
    fn test_static_sump_water_level() {
        let config = read_test_server_config_file(None);

        let tank_config = TankGeometryConfig {
            width: 1.0,
            length: 1.0,
            water_volume_min: 700.0,
            water_volume_max: 900.0,
            initial_water_volume: 800.0,
        };

        let tank = TankGeometry::new(tank_config);

        let pump1 = Pump {
            status: PumpStatus::On.into(),
            // delta_pressure: 0.01,
        };

        let pump2 = Pump {
            status: PumpStatus::On.into(),
            // delta_pressure: 0.01,
        };

        let refill_pump = Pump {
            status: PumpStatus::Off.into(),
        };

        let mut sump = Sump {
            main_pump_1: pump1,
            main_pump_2: pump2,
            refill_pump,
            level_filter: IIRFilter::new(
                &IIRFilterConfig {
                    filter_coefficient: 1.0,
                    fallback_initial_value: 0.8,
                },
                Some(1.0),
                #[cfg(not(test))]
                "sump level",
            ),
            config: config.sump,
        };

        assert_float_absolute_eq!(sump.get_water_level(&tank), 0.780, 0.001);
    }

    #[test]
    fn test_dynamic_sump_water_level() {
        let config = read_test_server_config_file(None);

        let tank_config = TankGeometryConfig {
            width: 1.0,
            length: 1.0,
            water_volume_min: 700.0,
            water_volume_max: 900.0,
            initial_water_volume: 800.0,
        };

        let tank = TankGeometry::new(tank_config);

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

        let mut sump = Sump {
            main_pump_1: pump1,
            main_pump_2: pump2,
            refill_pump,
            level_filter: IIRFilter::new(
                &IIRFilterConfig {
                    filter_coefficient: 0.5,
                    fallback_initial_value: 0.8,
                },
                Some(0.8),
                #[cfg(not(test))]
                "sump level",
            ),
            config: config.sump,
        };

        let initial_sump_level = sump.get_water_level(&tank);

        assert_float_absolute_eq!(initial_sump_level, 0.8, 0.001);

        // sump.switch_on_pumps();
        sump.main_pump_1.status = PumpStatus::On;
        sump.main_pump_2.status = PumpStatus::On;

        assert_float_absolute_eq!(sump.get_water_level(&tank), 0.790, 0.001);
        assert_float_absolute_eq!(sump.get_water_level(&tank), 0.785, 0.001);
        assert_float_absolute_eq!(sump.get_water_level(&tank), 0.782, 0.001);
    }
}
