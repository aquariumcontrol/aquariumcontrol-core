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
        use aquarium_control::test_server::tank_level_position::{TankLevelPosition, TankLevelPositionConfig};
        use aquarium_control::test_server::test_server_config::read_test_server_config_file;
        use aquarium_control::utilities::iir_filter::IIRFilter;
        use aquarium_control::utilities::iir_filter::IIRFilterConfig;
    }
}

#[cfg(feature = "test-utils")]
#[test]
// [itest->swreq~simulator_sensor_signals~1]
// Verifies that change low to high to low.
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
            #[cfg(all(not(test), not(feature = "test-utils")))]
            // only required in real-life execution
            "sump level",
        ),
        config: config.sump,
    };

    let tank_level_switch_config = TankLevelPositionConfig {
        water_level_high: 0.850,
        water_level_low: 0.825,
    };

    let mut tank_level_switch = TankLevelPosition::new(tank_level_switch_config, sump);

    assert!(!tank_level_switch.update_and_get_switch_position(&tank));

    tank.increase_water_volume(75.0);

    assert!(tank_level_switch.update_and_get_switch_position(&tank));

    tank.reduce_water_volume(100.0);

    assert!(!tank_level_switch.update_and_get_switch_position(&tank));
}
