/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    const LOG_TARGET_MOCK_HEATING: &str = "mock_heating";

    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::time::Duration;

    use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
    use crate::stubs::test_command::tests::TestCommand;
    use crate::stubs::mock_signal::tests::MockSignal;
    
    pub fn mock_heating(
        tx_heating_to_tank_level_switch_opt: Option<&Sender<InternalCommand>>,
        rx_heating_from_tank_level_switch_opt: Option<&Receiver<bool>>,
        rx_heating_from_signal_handler: &Receiver<InternalCommand>,
        tx_heating_to_data_logger_opt: Option<&Sender<bool>>,
        rx_heating_from_data_logger_opt: Option<&Receiver<InternalCommand>>,
        rx_heating_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        initial_stimuli: bool,
    ) {
        let sleep_duration = Duration::from_millis(10);
        let spin_sleeper = SpinSleeper::default();
        let mut stimuli = initial_stimuli;

        loop {
            match tx_heating_to_tank_level_switch_opt {
                Some(tx_heating_to_tank_level_switch) => {
                    match tx_heating_to_tank_level_switch.send(InternalCommand::RequestSignal(
                        AquariumSignal::TankLevelSwitchPosition,
                    )) {
                        Ok(_) => {
                            match rx_heating_from_tank_level_switch_opt {
                                Some(rx_heating_from_tank_level_switch) => {
                                    match rx_heating_from_tank_level_switch.try_recv() {
                                        Ok(_) => {
                                            // do nothing
                                        }
                                        Err(e) => {
                                            match e {
                                                TryRecvError::Empty => { /* empty buffer - no action required */
                                                }
                                                TryRecvError::Disconnected => {
                                                    panic!(
                                                        "{}: Error in trying to receive signal from test object: {e:?}",
                                                        LOG_TARGET_MOCK_HEATING
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                None => {
                                    panic!(
                                        "{}: invoked with valid sender but with empty receiver part for channel communication to tank level switch.",
                                        LOG_TARGET_MOCK_HEATING
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            panic!(
                                "{}: Could not send signal request to test object: {e:?}",
                                LOG_TARGET_MOCK_HEATING
                            );
                        }
                    }
                }
                None => { /* do nothing */ }
            };

            match rx_heating_from_signal_handler.try_recv() {
                Ok(c) => {
                    match c {
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            panic!("{}: Received invalid command.", LOG_TARGET_MOCK_HEATING);
                        }
                    };
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!(
                                "{}: Error in trying to receive command from signal handler: {e:?}",
                                LOG_TARGET_MOCK_HEATING
                            );
                        }
                    }
                }
            };

            // communicate with data logger
            match rx_heating_from_data_logger_opt {
                Some(rx_heating_from_data_logger) => {
                    match rx_heating_from_data_logger.try_recv() {
                        Ok(data_logger_request) => {
                            match data_logger_request {
                                InternalCommand::RequestSignal(data_logger_signal_request) => {
                                    match data_logger_signal_request {
                                        AquariumSignal::HeatingControlStatus => {
                                            match tx_heating_to_data_logger_opt {
                                                Some(tx_heating_to_data_logger) => {
                                                    match tx_heating_to_data_logger.send(stimuli) {
                                                        Ok(_) => { /* do nothing */ }
                                                        Err(e) => {
                                                            panic!(
                                                                "{}: Could not send signal request to data logger ({e:?})",
                                                                LOG_TARGET_MOCK_HEATING
                                                            );
                                                        }
                                                    };
                                                }
                                                None => {
                                                    panic!(
                                                        "{}: invoked without Sender for channel communication to data logger",
                                                        LOG_TARGET_MOCK_HEATING
                                                    );
                                                }
                                            };
                                        }
                                        _ => {
                                            panic!(
                                                "{}: Invalid signal {} requested from data logger",
                                                LOG_TARGET_MOCK_HEATING, data_logger_signal_request
                                            );
                                        }
                                    }
                                }
                                _ => {
                                    panic!(
                                        "{}: Received invalid command {} from data logger",
                                        LOG_TARGET_MOCK_HEATING, data_logger_request
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!(
                                        "{}: Error in trying to receive command from data logger: {e:?}",
                                        LOG_TARGET_MOCK_HEATING
                                    );
                                }
                            }
                        }
                    }
                }
                None => { /* do nothing */ }
            }

            match rx_heating_from_test_environment_opt {
                Some(rx_heating_from_test_environment) => {
                    match rx_heating_from_test_environment.try_recv() {
                        Ok(c) => {
                            let test_command = c.clone();
                            match c {
                                TestCommand::UpdateSignal(d) => {
                                    let test_signal = d.clone();
                                    match d {
                                        MockSignal::HeatingControlStatus(c) => stimuli = c,
                                        _ => {
                                            panic!(
                                                "{}: received test command for invalid signal {}",
                                                LOG_TARGET_MOCK_HEATING, test_signal
                                            );
                                        }
                                    }
                                }
                                _ => {
                                    panic!(
                                        "{}: received invalid command {}.",
                                        LOG_TARGET_MOCK_HEATING, test_command
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!(
                                        "{}: Error in trying to receive command from test environment: {e:?}",
                                        LOG_TARGET_MOCK_HEATING
                                    );
                                }
                            }
                        }
                    }
                }
                None => { /* do nothing */ }
            };

            spin_sleeper.sleep(sleep_duration);
        }
    }
}
