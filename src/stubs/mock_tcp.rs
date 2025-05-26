/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    const LOG_TARGET_MOCK_TCP: &str = "mock_tcp";

    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::time::Duration;

    use crate::stubs::mock_signal::tests::MockSignal;
    use crate::stubs::test_command::tests::TestCommand;
    use crate::utilities::channel_content::{AquariumSignal, InternalCommand};

    pub fn mock_tcp_for_tank_level_switch(
        tx_tcp_to_test_object: &Sender<f32>,
        rx_tcp_from_test_object: &Receiver<InternalCommand>,
        rx_tcp_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        initial_mock_value_position: bool,
        initial_mock_value_invalid: bool,
    ) {
        let sleep_duration = Duration::from_millis(10);
        let spin_sleeper = SpinSleeper::default();
        let mut mock_value_position = initial_mock_value_position;
        let mut mock_value_invalid = initial_mock_value_invalid;
        
        let mut command_received: Option<InternalCommand>;
        let mut signal_received: Option<AquariumSignal>;
        
        loop {
            match rx_tcp_from_test_object.try_recv() {
                Ok(command) => {
                    command_received = Some(command.clone());
                    let response_value: f32 = match command {
                        InternalCommand::RequestSignal(signal) => {
                            signal_received = Some(signal.clone());
                            match signal {
                                AquariumSignal::TankLevelSwitchPosition => mock_value_position.into(),
                                AquariumSignal::TankLevelSwitchInvalid => mock_value_invalid.into(),
                                _ => {
                                    panic!(
                                        "{}: unknown signal request to mock TCP",
                                        LOG_TARGET_MOCK_TCP
                                    );
                                }
                            }
                        },
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            panic!(
                                "{}: invalid command to mock TCP handler",
                                LOG_TARGET_MOCK_TCP
                            );
                        }
                    };
                    match tx_tcp_to_test_object.send(response_value) {
                        Ok(()) => {
                            if command_received.is_some() && signal_received.is_some() {
                                println!(
                                    "mock_tcp_for_tank_level_switch: test object sent command {}, \
                                    requesting signal {}, \
                                    sent mock data {} to test object, \
                                    mock position = {}, mock invalid = {}",
                                    command_received.unwrap(),
                                    signal_received.unwrap(),
                                    response_value,
                                    mock_value_position,
                                    mock_value_invalid
                                );
                            }
                        }
                        Err(e) => {
                            panic!(
                                "{}: sending mock data from TCP to test object failed ({e:?})",
                                LOG_TARGET_MOCK_TCP
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!(
                                "{}: receiving command from test object/test environment failed ({e:?})",
                                LOG_TARGET_MOCK_TCP
                            );
                        }
                    }
                }
            };
            match rx_tcp_from_test_environment_opt {
                Some(rx_tcp_from_test_environment) => match rx_tcp_from_test_environment.try_recv()
                {
                    Ok(c) => {
                        match c {
                            TestCommand::UpdateSignal(d) => match d {
                                MockSignal::TankLevelSwitchPosition(c) => mock_value_position = c,
                                MockSignal::TankLevelSwitchInvalid(c) => mock_value_invalid = c,
                                MockSignal::TankLevelSwitchPositionStabilized(_) => {
                                    panic!(
                                        "{}: received invalid test command {}",
                                        LOG_TARGET_MOCK_TCP, d
                                    );
                                }
                                _ => {
                                    panic!(
                                        "{}: Invalid test command {} received.",
                                        LOG_TARGET_MOCK_TCP, d
                                    );
                                }
                            },
                            _ => {
                                panic!("{}: received invalid command {}", LOG_TARGET_MOCK_TCP, c);
                            }
                        };
                    }
                    Err(e) => {
                        match e {
                            TryRecvError::Empty => { /* empty buffer - no action required */ }
                            TryRecvError::Disconnected => {
                                panic!(
                                    "{}: receiving command from test environment failed ({e:?})",
                                    LOG_TARGET_MOCK_TCP
                                );
                            }
                        }
                    }
                },
                None => { /* do nothing */ }
            }
            spin_sleeper.sleep(sleep_duration);
        }
    }
}
