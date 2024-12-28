/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    const LOG_TARGET_MOCK_TANK_LEVEL_SWITCH: &str = "mock_tank_level_switch";

    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::time::Duration;

    use crate::stubs::mock_signal::tests::MockSignal;
    use crate::stubs::test_command::tests::TestCommand;
    use crate::utilities::channel_content::{AquariumSignal, InternalCommand};

    pub fn mock_tank_level_switch(
        tx_tank_level_switch_to_test_object: &Sender<bool>,
        rx_tank_level_switch_from_test_object_or_signal_handler: &Receiver<InternalCommand>,
        rx_tank_level_switch_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        tx_tank_level_switch_to_data_logger_opt: Option<&Sender<bool>>,
        rx_tank_level_switch_from_data_logger_opt: Option<&Receiver<InternalCommand>>,
        initial_stimuli_position: bool,
        initial_stimuli_invalid: bool,
        initial_stimuli_position_stabilized: bool,
    ) {
        let mut stimuli_position: bool = initial_stimuli_position;
        let mut stimuli_invalid: bool = initial_stimuli_invalid;
        let mut stimuli_position_stabilized: bool = initial_stimuli_position_stabilized;

        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        loop {
            match rx_tank_level_switch_from_test_object_or_signal_handler.try_recv() {
                Ok(c) => {
                    let response_value = match c {
                        InternalCommand::RequestSignal(d) => match d {
                            AquariumSignal::TankLevelSwitchPosition => stimuli_position,
                            AquariumSignal::TankLevelSwitchInvalid => stimuli_invalid,
                            AquariumSignal::TankLevelSwitchPositionStabilized => {
                                stimuli_position_stabilized
                            }
                            _ => {
                                panic!(
                                    "{}: inapplicable signal request {}",
                                    LOG_TARGET_MOCK_TANK_LEVEL_SWITCH, d
                                );
                            }
                        },
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            panic!(
                                "{}: Received invalid command.",
                                LOG_TARGET_MOCK_TANK_LEVEL_SWITCH
                            );
                        }
                    };
                    match tx_tank_level_switch_to_test_object.send(response_value) {
                        Ok(()) => {}
                        Err(e) => {
                            panic!(
                                "{}: Error in trying to send response back data logger: {e:?}",
                                LOG_TARGET_MOCK_TANK_LEVEL_SWITCH
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!(
                                "{}: Error in trying to receive command from test object or test environment: {e:?}",
                                LOG_TARGET_MOCK_TANK_LEVEL_SWITCH
                            );
                        }
                    }
                }
            }

            // communicate with data logger
            match rx_tank_level_switch_from_data_logger_opt {
                Some(rx_tank_level_switch_from_data_logger) => {
                    match rx_tank_level_switch_from_data_logger.try_recv() {
                        Ok(data_logger_request) => {
                            match data_logger_request {
                                InternalCommand::RequestSignal(data_logger_signal_request) => {
                                    let data_logger_signal_response =
                                        match data_logger_signal_request {
                                            AquariumSignal::TankLevelSwitchPosition => {
                                                stimuli_position
                                            }
                                            AquariumSignal::TankLevelSwitchInvalid => {
                                                stimuli_invalid
                                            }
                                            AquariumSignal::TankLevelSwitchPositionStabilized => {
                                                stimuli_position_stabilized
                                            }
                                            _ => {
                                                panic!(
                                                    "{}: Invalid signal {} requested from DataLogger",
                                                    LOG_TARGET_MOCK_TANK_LEVEL_SWITCH,
                                                    data_logger_signal_request
                                                );
                                            }
                                        };
                                    match tx_tank_level_switch_to_data_logger_opt {
                                        Some(tx_tank_level_switch_to_data_logger) => {
                                            match tx_tank_level_switch_to_data_logger
                                                .send(data_logger_signal_response)
                                            {
                                                Ok(_) => { /* do nothing */ }
                                                Err(e) => {
                                                    panic!(
                                                        "{}: Could not send signal request to DataLogger ({e:?})",
                                                        LOG_TARGET_MOCK_TANK_LEVEL_SWITCH
                                                    );
                                                }
                                            };
                                        }
                                        None => {
                                            panic!(
                                                "{}: invoked without Sender for channel communication to data logger",
                                                LOG_TARGET_MOCK_TANK_LEVEL_SWITCH
                                            );
                                        }
                                    };
                                }
                                _ => {
                                    panic!(
                                        "{}: Received invalid command {} from DataLogger",
                                        LOG_TARGET_MOCK_TANK_LEVEL_SWITCH, data_logger_request
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!(
                                        "{}: Error in trying to receive command from DataLogger: {e:?}",
                                        LOG_TARGET_MOCK_TANK_LEVEL_SWITCH
                                    );
                                }
                            }
                        }
                    }
                }
                None => { /* do nothing */ }
            }

            match rx_tank_level_switch_from_test_environment_opt {
                Some(rx_tank_level_switch_from_test_environment) => {
                    match rx_tank_level_switch_from_test_environment.try_recv() {
                        Ok(c) => match c {
                            TestCommand::UpdateSignal(d) => {
                                println!("Mock TankLevelSwitch: received {}", d);
                                match d {
                                    MockSignal::TankLevelSwitchPosition(c) => stimuli_position = c,
                                    MockSignal::TankLevelSwitchInvalid(c) => stimuli_invalid = c,
                                    MockSignal::TankLevelSwitchPositionStabilized(c) => {
                                        stimuli_position_stabilized = c
                                    }
                                    _ => {
                                        panic!(
                                            "{}: Invalid test command {} received.",
                                            LOG_TARGET_MOCK_TANK_LEVEL_SWITCH, d
                                        );
                                    }
                                }
                            }
                            _ => {
                                panic!(
                                    "{}: received invalid command {}",
                                    LOG_TARGET_MOCK_TANK_LEVEL_SWITCH, c
                                );
                            }
                        },
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!(
                                        "{}: Error in trying to receive command from test environment: {e:?}",
                                        LOG_TARGET_MOCK_TANK_LEVEL_SWITCH
                                    );
                                }
                            }
                        }
                    }
                }
                None => { /* do nothing */ }
            }
            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
    }
}
