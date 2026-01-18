/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use std::env;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::scope;

use aquarium_control::generate_sender_logger_group;
use aquarium_control::test_server::controllino_emulator::ControllinoEmulator;
use aquarium_control::test_server::model_environment::ModelEnvironment;
use aquarium_control::test_server::tcp_connection::tcp_connection;
use aquarium_control::test_server::test_server_channel_content::TestServerInternalCommand;
use aquarium_control::test_server::test_server_config::read_test_server_config_file;
use chrono::Utc;

generate_sender_logger_group!(SenderLoggerTuple, A, B);

/// The main function reads the configuration file, initializes the model environment and the Controllino device, opens the TCP port.
fn main() {
    let args: Vec<String> = env::args().collect();
    let config_path = if args.len() > 1 {
        Some(args[1].clone())
    } else {
        None
    };

    let config = read_test_server_config_file(config_path);

    let mut model_environment = ModelEnvironment::new(
        config.tank_geometry,
        config.sump,
        config.tank_level_switch,
        config.pH_time_profile,
        config.tank_temperature_model,
        config.heat_flow,
        config.light_intensity_time_profile,
        config.ambient_temperature_time_profile,
        config.evaporation,
        config.conductivity_reverse_compensation,
        Utc::now().naive_utc().time(),
        1.0, // time step
    );

    let mut controllino = ControllinoEmulator::new(&config.controllino);

    let (tx_to_model_environment, rx_by_model_environment): (
        Sender<TestServerInternalCommand>,
        Receiver<TestServerInternalCommand>,
    ) = mpsc::channel();
    let (tx_to_parser, rx_by_parser): (Sender<f32>, Receiver<f32>) = mpsc::channel();

    let sender_loggers =
        SenderLoggerTuple::new("main.log", tx_to_model_environment, tx_to_parser).unwrap();
    let (tx_logger_to_model_environment, tx_logger_to_parser) = sender_loggers.sender_loggers();

    scope(|scope| {
        scope.spawn(move || {
            let connection_error = tcp_connection(
                &config.tcp_connection,
                tx_logger_to_model_environment.clone(),
                rx_by_parser,
                &mut controllino,
            );

            match connection_error {
                Ok(_) => println!("TCP connection handling terminated without error."),
                Err(e) => println!(
                    "Could not establish TCP connection or TCP connection was closed: {e:?}. Aborting."
                ),
            }
            tx_logger_to_model_environment.send_and_log(TestServerInternalCommand::Quit).unwrap();
        });

        scope.spawn(move || {
            model_environment.execute_models(
                None,
                rx_by_model_environment,
                tx_logger_to_parser,
                Utc::now().naive_utc().time(),
            );
        });
    });
}
