/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use crate::launch::channels::{AquaChannelError, AquaReceiver};
    use crate::mocks::mock_signal::tests::MockSignal;
    use crate::mocks::test_command::tests::TestCommand;
    use crate::simulator::tcp_communication_channels::TcpCommunicationChannels;
    use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
    use spin_sleep::SpinSleeper;
    use std::time::Duration;

    /// Performs the `mock_tcp_for_tank_level_switch` operation.
    ///
    /// # Arguments
    /// * `tcp_communication_channels` - A parameter representing `tcp_communication_channels`.
    /// * `rx_tcp_from_test_environment_opt` - A parameter representing `rx_tcp_from_test_environment_opt`.
    /// * `initial_mock_value_position` - A parameter representing `initial_mock_value_position`.
    /// * `initial_mock_value_invalid` - A parameter representing `initial_mock_value_invalid`.
    pub fn mock_tcp_for_tank_level_switch(
        tcp_communication_channels: TcpCommunicationChannels,
        mut rx_tcp_from_test_environment_opt: Option<&mut AquaReceiver<TestCommand>>,
        initial_mock_value_position: bool,
        initial_mock_value_invalid: bool,
    ) {
        let sleep_duration = Duration::from_millis(10);
        let spin_sleeper = SpinSleeper::default();
        let mut mock_value_position = initial_mock_value_position;
        let mut mock_value_invalid = initial_mock_value_invalid;

        let mut command_received: Option<InternalCommand>;
        let mut signal_received: Option<AquariumSignal>;

        let mut tx_tcp_to_test_object = tcp_communication_channels
            .tx_tcp_communication_to_tank_level_switch_opt
            .unwrap();
        let mut rx_tcp_from_test_object = tcp_communication_channels
            .rx_tcp_communication_from_tank_level_switch_opt
            .unwrap();

        loop {
            match rx_tcp_from_test_object.try_recv() {
                Ok(command) => {
                    command_received = Some(command.clone());
                    let response_value: f32 = match command {
                        InternalCommand::RequestSignal(signal) => {
                            signal_received = Some(signal.clone());
                            match signal {
                                AquariumSignal::TankLevelSwitchPosition => {
                                    mock_value_position.into()
                                }
                                AquariumSignal::TankLevelSwitchInvalid => mock_value_invalid.into(),
                                _ => {
                                    panic!(
                                        "{}: unknown signal request to mock TCP",
                                        module_path!()
                                    );
                                }
                            }
                        }
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            panic!("{}: invalid command to mock TCP handler", module_path!());
                        }
                    };
                    match tx_tcp_to_test_object.send(Ok(response_value)) {
                        Ok(()) => {
                            if let (Some(cmd), Some(sig)) = (command_received, signal_received) {
                                println!(
                                    "mock_tcp_for_tank_level_switch: test object sent command {}, \
                                    requesting signal {}, \
                                    sent mock data {} to test object, \
                                    mock position = {}, mock invalid = {}",
                                    cmd,
                                    sig,
                                    response_value,
                                    mock_value_position,
                                    mock_value_invalid
                                );
                            }
                        }
                        Err(e) => {
                            panic!(
                                "{}: sending mock data from TCP to test object failed ({e:?})",
                                module_path!()
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        AquaChannelError::Empty => { /* empty buffer - no action required */ }
                        AquaChannelError::Disconnected => {
                            panic!(
                                "{}: receiving command from test object/test environment failed ({e:?})",
                                module_path!()
                            );
                        }
                    }
                }
            };
            match rx_tcp_from_test_environment_opt {
                Some(ref mut rx_tcp_from_test_environment) => match rx_tcp_from_test_environment
                    .try_recv()
                {
                    Ok(c) => {
                        match c {
                            TestCommand::UpdateSignal(d) => match d {
                                MockSignal::TankLevelSwitchPosition(c) => mock_value_position = c,
                                MockSignal::TankLevelSwitchInvalid(c) => mock_value_invalid = c,
                                MockSignal::TankLevelSwitchPositionStabilized(_) => {
                                    panic!(
                                        "{}: received invalid test command {}",
                                        module_path!(),
                                        d
                                    );
                                }
                                _ => {
                                    panic!(
                                        "{}: Invalid test command {} received.",
                                        module_path!(),
                                        d
                                    );
                                }
                            },
                            _ => {
                                panic!("{}: received invalid command {}", module_path!(), c);
                            }
                        };
                    }
                    Err(e) => {
                        match e {
                            AquaChannelError::Empty => { /* empty buffer - no action required */ }
                            AquaChannelError::Disconnected => {
                                panic!(
                                    "{}: receiving command from test environment failed ({e:?})",
                                    module_path!()
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
