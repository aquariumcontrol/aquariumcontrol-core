/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#![allow(non_snake_case)]
use crate::simulator::tcp_data::{category, command, signal};
use crate::test_server::controllino_emulator::ControllinoEmulator;
use crate::test_server::logger::SenderLogger;
use crate::test_server::test_server_channel_content::TestServerInternalCommand;
use crate::utilities::channel_content::AquariumSignal;
use std::sync::mpsc::Receiver;

#[derive(Debug)]
/// Enum codifies the errors which the program may encounter during evaluation of the command string.
pub enum ParseErr {
    WrongArgumentCount(i32),
    ParseIntErr(String),
    InvalidCategory(String),
    InvalidCommand(String),
    InvalidSignal(String),
    InvalidRelayId(u16),
    InvalidPulseTime(u32),
    SendFailure,
}

/// Helper function to evaluate command for switching on/switching off relay.
/// It returns a result type where the Ok result is always None because relay actuation does not produce any data.
///
/// # Arguments:
///
/// * `relay_id` - id of the relay to be actuated
/// * `target_state` - parameter indicating whether the relay shall be turned on (true) or off (false)
/// * `tx_to_model` - channel to communicate to model environment
/// * `controllino` - reference to Controllino device simulation
fn modify_relay_state(
    relay_id: u16,
    target_state: bool,
    tx_to_model: &SenderLogger<TestServerInternalCommand>,
    controllino: &mut ControllinoEmulator,
) -> Result<Option<String>, ParseErr> {
    // check id and actuate device
    let result: Option<TestServerInternalCommand> = if target_state {
        controllino.set_relay(relay_id)
    } else {
        controllino.unset_relay(relay_id)
    };
    match result {
        Some(internal_command) => match tx_to_model.send_and_log(internal_command) {
            Ok(_) => Ok(None),
            Err(_) => Err(ParseErr::SendFailure),
        },
        None => Err(ParseErr::InvalidRelayId(relay_id)),
    }
}

/// Helper function to pulse a relay.
/// It returns a result type where the Ok result is always None because relay actuation does not produce any data.
///
/// # Arguments:
///
/// * `relay_id` - id of the relay to be actuated
/// * `duration_ms` - pulse time in milliseconds
/// * `tx_to_model` - channel to communicate to model environment
/// * `controllino` - reference to Controllino device simulation
fn pulse_relay(
    relay_id: u16,
    duration_ms: u32,
    tx_to_model: &SenderLogger<TestServerInternalCommand>,
    controllino: &mut ControllinoEmulator,
) -> Result<Option<String>, ParseErr> {
    // check id and actuate device
    // @todo: external configuration of minimum pulse duration
    let result: Option<TestServerInternalCommand> = if duration_ms > 0 {
        controllino.pulse(relay_id, duration_ms)
    } else {
        return Err(ParseErr::InvalidPulseTime(duration_ms));
    };
    match result {
        Some(internal_command) => match tx_to_model.send_and_log(internal_command) {
            Ok(_) => Ok(None),
            Err(_) => Err(ParseErr::SendFailure),
        },
        None => Err(ParseErr::InvalidRelayId(relay_id)),
    }
}

/// Evaluation of commands received via TCP
/// It returns a result type where the Ok result may contain a string because the control application expects an answer from the test server.
///
/// # Arguments:
///
/// * `sequence` - String with the message received from the control application
/// * `tx_to_model` - channel to communicate to model environment
/// * `rx_from_model` - channel receive answer from model environment
/// * `controllino` - reference to Controllino device simulation
pub fn parse_sequence(
    sequence: String,
    tx_to_model: &SenderLogger<TestServerInternalCommand>,
    rx_from_model: &Receiver<f32>,
    controllino: &mut ControllinoEmulator,
) -> Result<Option<String>, ParseErr> {
    let words = sequence.split(" ").collect::<Vec<&str>>();

    if words.len() >= 2 && words.len() <= 4 {
        let word_category = words[0];
        let word_command = words[1];

        let word_signal = if words.len() >= 3 {
            words[2]
        } else {
            "dummysignal"
        };
        let word_optional = if words.len() == 4 {
            words[3]
        } else {
            "dummyoptional"
        };

        match word_category {
            category::CONTROLLINO => match word_command {
                command::SETSINGLE => {
                    if words.len() == 3 {
                        match word_signal.parse::<u16>() {
                            Ok(relay_id) => {
                                modify_relay_state(relay_id, true, tx_to_model, controllino)
                            }
                            Err(_) => Err(ParseErr::ParseIntErr(word_signal.to_string())),
                        }
                    } else {
                        Err(ParseErr::WrongArgumentCount(words.len() as i32))
                    }
                }
                command::UNSETSINGLE => {
                    if words.len() == 3 {
                        match word_signal.parse::<u16>() {
                            Ok(relay_id) => {
                                modify_relay_state(relay_id, false, tx_to_model, controllino)
                            }
                            Err(_) => Err(ParseErr::ParseIntErr(word_signal.to_string())),
                        }
                    } else {
                        Err(ParseErr::WrongArgumentCount(words.len() as i32))
                    }
                }
                command::GETSTATUS => {
                    if words.len() == 2 {
                        let response = sequence + " " + &controllino.get_status().to_string();
                        Ok(Some(response))
                    } else {
                        Err(ParseErr::WrongArgumentCount(words.len() as i32))
                    }
                }
                command::PULSE => {
                    if words.len() == 4 {
                        match word_signal.parse::<u16>() {
                            Ok(relay_id) => match word_optional.parse::<u32>() {
                                Ok(pulsetime_ms) => {
                                    pulse_relay(relay_id, pulsetime_ms, tx_to_model, controllino)
                                }
                                Err(_) => Err(ParseErr::ParseIntErr(word_optional.to_string())),
                            },
                            Err(_) => Err(ParseErr::ParseIntErr(word_signal.to_string())),
                        }
                    } else {
                        Err(ParseErr::WrongArgumentCount(words.len() as i32))
                    }
                }
                command::HEARTBEAT => {
                    if words.len() == 2 {
                        controllino.heartbeat();
                        Ok(None)
                    } else {
                        Err(ParseErr::WrongArgumentCount(words.len() as i32))
                    }
                }
                _ => Err(ParseErr::InvalidCommand(word_command.to_string())),
            },
            category::DHT22 => match word_command {
                command::READ => {
                    if words.len() == 3 {
                        match word_signal {
                            signal::AMBIENTTEMPERATURE => {
                                tx_to_model
                                    .send_and_log(TestServerInternalCommand::RequestSignal(
                                        AquariumSignal::AmbientTemperature,
                                    ))
                                    .unwrap();
                                let ambientTemperatureResponse = rx_from_model.recv().unwrap();
                                // todo: replace model_environment
                                let response =
                                    sequence + " " + &ambientTemperatureResponse.to_string();
                                // todo: send real response to client
                                Ok(Some(response))
                            }
                            signal::AMBIENTHUMIDITY => {
                                tx_to_model
                                    .send_and_log(TestServerInternalCommand::RequestSignal(
                                        AquariumSignal::AmbientHumidity,
                                    ))
                                    .unwrap();
                                let ambientHumidityResponse = rx_from_model.recv().unwrap();
                                let response =
                                    sequence + " " + &ambientHumidityResponse.to_string();
                                // todo: send response to client
                                Ok(Some(response))
                            }
                            _ => Err(ParseErr::InvalidSignal(word_signal.to_string())),
                        }
                    } else {
                        Err(ParseErr::WrongArgumentCount(words.len() as i32))
                    }
                }
                _ => Err(ParseErr::InvalidCommand(word_command.to_string())),
            },
            category::ATLASSCIENTIFIC => match word_command {
                command::READ => {
                    if words.len() == 3 {
                        match word_signal {
                            signal::ATLASSCIENTIFICTEMPERATURE => {
                                tx_to_model
                                    .send_and_log(TestServerInternalCommand::RequestSignal(
                                        AquariumSignal::WaterTemperature,
                                    ))
                                    .unwrap();
                                let water_temp_response = rx_from_model.recv().unwrap();
                                let response = sequence + " " + &water_temp_response.to_string();
                                Ok(Some(response))
                            }
                            signal::ATLASSCIENTIFICPH => {
                                tx_to_model
                                    .send_and_log(TestServerInternalCommand::RequestSignal(
                                        AquariumSignal::pH,
                                    ))
                                    .unwrap();
                                let pH_response = rx_from_model.recv().unwrap();
                                let response = sequence + " " + &pH_response.to_string();
                                Ok(Some(response))
                            }
                            signal::ATLASSCIENTIFICCONDUCTIVITY => {
                                tx_to_model
                                    .send_and_log(TestServerInternalCommand::RequestSignal(
                                        AquariumSignal::Conductivity,
                                    ))
                                    .unwrap();
                                let conductivity_response = rx_from_model.recv().unwrap();
                                let response = sequence + " " + &conductivity_response.to_string();
                                Ok(Some(response))
                            }
                            _ => Err(ParseErr::InvalidSignal(word_signal.to_string())),
                        }
                    } else {
                        Err(ParseErr::WrongArgumentCount(words.len() as i32))
                    }
                }
                _ => Err(ParseErr::InvalidCommand(word_command.to_string())),
            },
            category::TANKLEVELSWITCH => match word_command {
                command::READ => {
                    if words.len() == 3 {
                        match word_signal {
                            signal::TANKLEVELSWITCHPOSITION => {
                                tx_to_model
                                    .send_and_log(TestServerInternalCommand::RequestSignal(
                                        AquariumSignal::TankLevelSwitchPosition,
                                    ))
                                    .unwrap();
                                let tank_level_switch_response: f32 = rx_from_model.recv().unwrap();
                                let response =
                                    sequence + " " + &tank_level_switch_response.to_string();
                                Ok(Some(response))
                            }
                            signal::TANKLEVELSWITCHINVALID => {
                                tx_to_model
                                    .send_and_log(TestServerInternalCommand::RequestSignal(
                                        AquariumSignal::TankLevelSwitchInvalid,
                                    ))
                                    .unwrap();
                                let tank_level_switch_invalid_response: f32 =
                                    rx_from_model.recv().unwrap();
                                let response = sequence
                                    + " "
                                    + &tank_level_switch_invalid_response.to_string();
                                Ok(Some(response))
                            }
                            _ => Err(ParseErr::InvalidSignal(word_signal.to_string())),
                        }
                    } else {
                        Err(ParseErr::WrongArgumentCount(words.len() as i32))
                    }
                }
                _ => Err(ParseErr::InvalidCommand(word_command.to_string())),
            },
            category::REFILLCONTROL => match word_command {
                command::INITIALIZED => Ok(None),
                _ => Err(ParseErr::InvalidCommand(word_command.to_string())),
            },
            _ => Err(ParseErr::InvalidCategory(word_category.to_string())),
        }
    } else {
        Err(ParseErr::WrongArgumentCount(words.len() as i32))
    }
}
