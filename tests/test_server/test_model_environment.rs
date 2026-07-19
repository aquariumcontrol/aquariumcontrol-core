/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#![allow(non_snake_case)]

use aquarium_control::generate_sender_logger_group;
use aquarium_control::test_server::model_environment::{ModelEnvironment, ModelEnvironmentConfig};
use aquarium_control::test_server::test_server_config::read_test_server_config_file;
use assert_float_eq::*;
use chrono::NaiveTime;
use std::sync::mpsc;

generate_sender_logger_group!(SenderLoggerTuple, A, B);

#[test]
fn test_model_environment_execution() {
    let config = read_test_server_config_file(None);
    let time_of_day = NaiveTime::from_hms_opt(9, 0, 0).unwrap();

    let model_config = ModelEnvironmentConfig {
        tank_geometry: config.tank_geometry,
        sump: config.sump,
        tank_level_switch: config.tank_level_switch,
        ph_time_profile: config.pH_time_profile,
        tank_temperature_model: config.tank_temperature_model,
        heat_flow: config.heat_flow,
        light_intensity_time_profile: config.light_intensity_time_profile,
        ambient_temperature_time_profile: config.ambient_temperature_time_profile,
        evaporation: config.evaporation,
        conductivity_reverse_compensation: config.conductivity_reverse_compensation,
        initial_time: time_of_day,
        time_step: 1.0,
    };
    let mut model_environment = ModelEnvironment::new(model_config);

    let (_command_tx, command_rx) = mpsc::channel();
    let (model_tx, _model_rx) = mpsc::channel();

    let sender_loggers = SenderLoggerTuple::new(
        "/tmp/aquarium_test_server__tests__model_environment__test_model_environment_execution.log",
        _command_tx,
        model_tx,
    )
    .unwrap();
    let (_command_tx, model_tx) = sender_loggers.sender_loggers();

    model_environment.execute_models(Some(3), command_rx, model_tx, time_of_day); // execute 3 iterations

    assert_float_absolute_eq!(
        model_environment.tank_temperature_model.temperature_water,
        25.000,
        0.001
    );
}
