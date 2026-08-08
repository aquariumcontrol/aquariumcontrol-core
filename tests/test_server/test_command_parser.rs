/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#![allow(non_snake_case)]
use aquarium_control::generate_sender_logger_group;
use aquarium_control::test_server::command_parser::{parse_sequence, ParseErr};
use aquarium_control::test_server::controllino_emulator::ControllinoEmulator;
use aquarium_control::test_server::model_environment::{ModelEnvironment, ModelEnvironmentConfig};
use aquarium_control::test_server::test_server_channel_content::TestServerInternalCommand;
use aquarium_control::test_server::test_server_config::read_test_server_config_file;
use assert_float_eq::*;
use chrono::NaiveTime;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::thread::scope;

generate_sender_logger_group!(SenderLoggerTuple, A, B);

fn match_model_response(response: Result<Option<String>, ParseErr>) -> f32 {
    match response {
        Ok(response_sequence_opt) => {
            let response_sequence = response_sequence_opt.unwrap();
            let response_sequence_split = response_sequence.split(" ");
            let words = response_sequence_split.collect::<Vec<&str>>();
            words[3].parse::<f32>().unwrap()
        }
        Err(_err) => {
            panic!("Invalid response from model.");
        }
    }
}

#[test]
// [itest->swreq~simulator_sensor_signals~1]
// Verifies that request signals.
pub fn test_request_signals() {
    let time_of_day = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
    let input_sequence_conductivity = "ATLSSCNTFC READ conductivity".to_string();
    let input_sequence_pH = "ATLSSCNTFC READ pH".to_string();
    let input_sequence_temperature = "ATLSSCNTFC READ temperature".to_string();
    let input_sequence_ambient_humidity = "DHT22 READ AmbHum".to_string();
    let input_sequence_ambient_temperature = "DHT22 READ AmbTemp".to_string();
    let input_sequence_tank_level_switch_position = "TNKLVLSWTCH READ TnkLvlSwtch".to_string();
    let input_sequence_tank_level_switch_qualifier =
        "TNKLVLSWTCH READ TnkLvlSwtchInvld".to_string();
    let config = read_test_server_config_file(None);

    let (tx_to_model_environment, rx_by_model_environment): (
        Sender<TestServerInternalCommand>,
        Receiver<TestServerInternalCommand>,
    ) = mpsc::channel();
    let (tx_to_parser, rx_by_parser): (Sender<f32>, Receiver<f32>) = mpsc::channel();

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

    let mut controllino = ControllinoEmulator::new(&config.controllino);
    let sender_loggers = SenderLoggerTuple::new(
        "/tmp/aquarium_test_server__tests__command_parser__test_request_signals.log",
        tx_to_model_environment,
        tx_to_parser,
    )
    .unwrap();
    let (tx_logger_to_model_environment, tx_logger_to_parser) = sender_loggers.sender_loggers();

    scope(|scope| {
        scope.spawn(move || {
            model_environment.execute_models(
                None,
                rx_by_model_environment,
                tx_logger_to_parser,
                time_of_day,
            );
        });
        scope.spawn(move || {
            let model_response_ambient_temperature = match_model_response(parse_sequence(
                input_sequence_ambient_temperature,
                &tx_logger_to_model_environment,
                &rx_by_parser,
                &mut controllino,
            ));
            let model_response_ambient_humidity = match_model_response(parse_sequence(
                input_sequence_ambient_humidity,
                &tx_logger_to_model_environment,
                &rx_by_parser,
                &mut controllino,
            ));
            let model_response_conductivity = match_model_response(parse_sequence(
                input_sequence_conductivity,
                &tx_logger_to_model_environment,
                &rx_by_parser,
                &mut controllino,
            ));
            let model_response_pH = match_model_response(parse_sequence(
                input_sequence_pH,
                &tx_logger_to_model_environment,
                &rx_by_parser,
                &mut controllino,
            ));
            let model_response_temperature = match_model_response(parse_sequence(
                input_sequence_temperature,
                &tx_logger_to_model_environment,
                &rx_by_parser,
                &mut controllino,
            ));
            let model_response_tank_level_switch_position = match_model_response(parse_sequence(
                input_sequence_tank_level_switch_position,
                &tx_logger_to_model_environment,
                &rx_by_parser,
                &mut controllino,
            ));
            let model_response_tank_level_switch_qualifier = match_model_response(parse_sequence(
                input_sequence_tank_level_switch_qualifier,
                &tx_logger_to_model_environment,
                &rx_by_parser,
                &mut controllino,
            ));
            let _ = tx_logger_to_model_environment.send_and_log(TestServerInternalCommand::Quit);
            assert_float_absolute_eq!(model_response_ambient_temperature, 23.0, 0.001);
            assert_float_absolute_eq!(model_response_ambient_humidity, 60.0, 0.001);
            assert_float_absolute_eq!(model_response_conductivity, 50000.0, 0.2);
            assert_float_absolute_eq!(model_response_pH, 8.455, 0.001);
            assert_float_absolute_eq!(model_response_temperature, 25.0, 0.001);
            assert_float_absolute_eq!(model_response_tank_level_switch_position, 0.0, 0.001);
            assert_float_absolute_eq!(model_response_tank_level_switch_qualifier, 0.0, 0.001);
        });
    });
}
