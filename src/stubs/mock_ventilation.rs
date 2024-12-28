/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    const LOG_TARGET_MOCK_VENTILATION: &str = "mock_ventilation";

    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::time::Duration;

    use crate::stubs::mock_signal::tests::MockSignal;
    use crate::stubs::test_command::tests::TestCommand;
    use crate::utilities::channel_content::{AquariumSignal, InternalCommand};

    pub fn mock_ventilation(
        tx_ventilation_to_data_logger: &Sender<bool>,
        rx_ventilation_from_data_logger: &Receiver<InternalCommand>,
        rx_ventilation_from_signal_handler: &Receiver<InternalCommand>,
        rx_ventilation_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        initial_stimuli: bool,
    ) {
        let sleep_duration = Duration::from_millis(10);
        let spin_sleeper = SpinSleeper::default();
        let mut stimuli = initial_stimuli;

        loop {
            match rx_ventilation_from_data_logger.try_recv() {
                Ok(data_logger_request) => {
                    match data_logger_request {
                        InternalCommand::RequestSignal(data_logger_signal_request) => {
                            match data_logger_signal_request {
                                AquariumSignal::VentilationControlStatus => {
                                    match tx_ventilation_to_data_logger.send(stimuli) {
                                        Ok(_) => { /* do nothing */ }
                                        Err(e) => {
                                            panic!(
                                                "{}: Could not send signal request to test object: {e:?}",
                                                LOG_TARGET_MOCK_VENTILATION
                                            );
                                        }
                                    };
                                }
                                _ => {
                                    panic!(
                                        "{}: Received invalid signal request for {} from data logger",
                                        LOG_TARGET_MOCK_VENTILATION,
                                        data_logger_signal_request
                                    );
                                }
                            };
                        }
                        _ => {
                            panic!(
                                "{}: Received invalid command {} from data logger.",
                                LOG_TARGET_MOCK_VENTILATION, data_logger_request
                            );
                        }
                    };
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!(
                                "{}: Error in trying to receive command from data logger: {e:?}",
                                LOG_TARGET_MOCK_VENTILATION
                            );
                        }
                    }
                }
            };

            match rx_ventilation_from_signal_handler.try_recv() {
                Ok(c) => {
                    match c {
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            panic!("{}: Received invalid command.", LOG_TARGET_MOCK_VENTILATION);
                        }
                    };
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!(
                                "{}: Error in trying to receive command from signal handler: {e:?}",
                                LOG_TARGET_MOCK_VENTILATION
                            );
                        }
                    }
                }
            };

            match rx_ventilation_from_test_environment_opt {
                Some(rx_ventilation_from_test_environment) => {
                    match rx_ventilation_from_test_environment.try_recv() {
                        Ok(c) => {
                            let test_command = c.clone();
                            match c {
                                TestCommand::UpdateSignal(d) => {
                                    let test_signal = d.clone();
                                    match d {
                                        MockSignal::VentilationControlStatus(c) => stimuli = c,
                                        _ => {
                                            panic!(
                                                "{}: received test command for invalid signal {}",
                                                LOG_TARGET_MOCK_VENTILATION, test_signal
                                            );
                                        }
                                    }
                                }
                                _ => {
                                    panic!(
                                        "{}: received invalid command {}.",
                                        LOG_TARGET_MOCK_VENTILATION, test_command
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
                                        LOG_TARGET_MOCK_VENTILATION
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
