/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use aquarium_control::test_server::heat_flow::HeatFlow;
use aquarium_control::test_server::test_server_config::read_test_server_config_file;
use assert_float_eq::*;

/// Performs the `create_heat_flow_model` operation.
fn create_heat_flow_model() -> HeatFlow {
    let config_heat_flow = read_test_server_config_file(None).heat_flow;

    HeatFlow::new(config_heat_flow, 1.0)
}

#[test]
// Verifies that heat flow to environment.
fn test_heat_flow_to_environment() {
    let heat_flow_model = create_heat_flow_model();
    assert_float_absolute_eq!(
        heat_flow_model.heat_flow_environment(25.0, 24.0),
        -50.0,
        0.001
    )
}

#[test]
// Verifies that heat flow from environment.
fn test_heat_flow_from_environment() {
    let heat_flow_model = create_heat_flow_model();
    assert_float_absolute_eq!(
        heat_flow_model.heat_flow_environment(24.0, 25.0),
        50.0,
        0.001
    )
}

#[test]
// Verifies that no heat flow to or from environment.
fn test_no_heat_flow_to_or_from_environment() {
    let heat_flow_model = create_heat_flow_model();
    assert_float_absolute_eq!(
        heat_flow_model.heat_flow_environment(25.0, 25.0),
        0.0,
        0.001
    )
}

#[test]
// Verifies that heat flow from heater when heater is on.
fn test_heat_flow_from_heater_when_heater_is_on() {
    let mut heat_flow_model = create_heat_flow_model();

    assert_float_absolute_eq!(heat_flow_model.heat_flow_heater(true), 220.0, 0.001);
}

#[test]
// Verifies that heat flow from heater when heater is off.
fn test_heat_flow_from_heater_when_heater_is_off() {
    let mut heat_flow_model = create_heat_flow_model();

    assert_float_absolute_eq!(heat_flow_model.heat_flow_heater(false), 110.0, 0.001);
}

#[test]
// Verifies that heat flow evaporation.
fn test_heat_flow_evaporation() {
    let heat_flow_model = create_heat_flow_model();
    assert_float_absolute_eq!(
        heat_flow_model.heat_flow_evaporation(1.0, 25.0),
        -0.5588436,
        0.00001
    )
}

#[test]
// Verifies that no heat flow evaporation.
fn test_no_heat_flow_evaporation() {
    let heat_flow_model = create_heat_flow_model();
    assert_float_absolute_eq!(heat_flow_model.heat_flow_evaporation(0.0, 25.0), 0.0, 0.001)
}

#[test]
// Verifies that no heat flow evaporation with negative evaporation.
fn test_no_heat_flow_evaporation_with_negative_evaporation() {
    let heat_flow_model = create_heat_flow_model();
    assert_float_absolute_eq!(
        heat_flow_model.heat_flow_evaporation(-1.0, 25.0),
        0.0,
        0.001
    )
}

#[test]
// Verifies that heat flow lights.
fn test_heat_flow_lights() {
    let heat_flow_model = create_heat_flow_model();
    assert_float_absolute_eq!(heat_flow_model.heat_flow_lights(1.0), 1.8, 0.001)
}

#[test]
// Verifies that no heat flow lights.
fn test_no_heat_flow_lights() {
    let heat_flow_model = create_heat_flow_model();
    assert_float_absolute_eq!(heat_flow_model.heat_flow_lights(0.0), 0.0, 0.001)
}

#[test]
// Verifies that no heat flow lights with negative intensity.
fn test_no_heat_flow_lights_with_negative_intensity() {
    let heat_flow_model = create_heat_flow_model();
    assert_float_absolute_eq!(heat_flow_model.heat_flow_lights(-1.0), 0.0, 0.001)
}

#[test]
// Verifies that heat flow total.
fn test_heat_flow_total() {
    let mut heat_flow_model = create_heat_flow_model();

    assert_float_absolute_eq!(
        heat_flow_model.heat_flow_total(100.0, 25.5, 22.0, 1.0, true),
        224.441,
        0.001
    );
}
