/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

cfg_if::cfg_if! {
    if #[cfg(feature = "test-utils")] {
        use aquarium_control::test_server::sump::Pump;
        use aquarium_control::test_server::sump::PumpStatus;
        use aquarium_control::test_server::sump::Sump;
        use aquarium_control::test_server::tank_geometry::TankGeometry;
        use aquarium_control::test_server::tank_geometry::TankGeometryConfig;
        use aquarium_control::test_server::test_server_config::read_test_server_config_file;
        use aquarium_control::utilities::iir_filter::{IIRFilter, IIRFilterConfig};
        use assert_float_eq::*;
    }
}

#[cfg(feature = "test-utils")]
#[test]
// [itest->swreq~simulator_sensor_signals~1]
// Verifies that static sump water level.
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
        status: PumpStatus::On,
        // delta_pressure: 0.01,
    };

    let pump2 = Pump {
        status: PumpStatus::On,
        // delta_pressure: 0.01,
    };

    let refill_pump = Pump {
        status: PumpStatus::Off,
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
            #[cfg(all(not(test), not(feature = "test-utils")))]
            // only required in real-life execution
            "sump level",
        ),
        config: config.sump,
    };

    assert_float_absolute_eq!(sump.get_water_level(&tank), 0.780, 0.001);
}

#[cfg(feature = "test-utils")]
#[test]
// [itest->swreq~simulator_sensor_signals~1]
// Verifies that dynamic sump water level.
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
        status: PumpStatus::Off,
        // delta_pressure: 0.01,
    };

    let pump2 = Pump {
        status: PumpStatus::Off,
        // delta_pressure: 0.01,
    };

    let refill_pump = Pump {
        status: PumpStatus::Off,
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
            #[cfg(all(not(test), not(feature = "test-utils")))]
            // only required in real-life execution
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
