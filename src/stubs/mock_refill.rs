/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    const LOG_TARGET_MOCK_REFILL: &str = "mock_refill";

    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::time::Duration;
    
    use crate::utilities::channel_content::{InternalCommand, AquariumSignal};
    use crate::stubs::mock_signal::tests::MockSignal;
    use crate::stubs::test_command::tests::TestCommand;

    pub fn mock_refill(
        tx_refill_to_tank_level_switch_opt: Option<&Sender<InternalCommand>>,
        rx_refill_from_tank_level_switch_opt: Option<&Receiver<bool>>,
        rx_refill_from_signal_handler: &Receiver<InternalCommand>,
        tx_refill_to_data_logger_opt: Option<&Sender<bool>>,
        rx_refill_from_data_logger_opt: Option<&Receiver<InternalCommand>>,
        rx_refill_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        initial_stimuli: bool,
    ) -> Vec<bool> {
        let mut result: Vec<bool> = Vec::new();

        let sleep_duration = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        let mut stimuli = initial_stimuli;

        loop {
            match tx_refill_to_tank_level_switch_opt {
                Some(tx_refill_to_tank_level_switch) => {
                    match tx_refill_to_tank_level_switch.send(InternalCommand::RequestSignal(
                        AquariumSignal::TankLevelSwitchPositionStabilized,
                    )) {
                        Ok(_) => {
                            match rx_refill_from_tank_level_switch_opt {
                                Some(rx_refill_from_tank_level_switch) => {
                                    match rx_refill_from_tank_level_switch.try_recv() {
                                        Ok(d) => {
                                            result.push(d);
                                        }
                                        Err(e) => {
                                            match e {
                                                TryRecvError::Empty => { /* empty buffer - no action required */
                                                }
                                                TryRecvError::Disconnected => {
                                                    panic!(
                                                            "{}: Error in trying to receive command from TankLevelSwitch: {e:?}",
                                                            LOG_TARGET_MOCK_REFILL
                                                        );
                                                }
                                            }
                                        }
                                    }
                                }
                                None => {
                                    panic!(
                                            "{}: invoked with valid sender but without valid receiver for channel communication with tank level switch",
                                            LOG_TARGET_MOCK_REFILL
                                        );
                                }
                            }
                        }
                        Err(e) => {
                            panic!(
                                "{}: Could not send signal request to TankLevelSwitch ({e:?})",
                                LOG_TARGET_MOCK_REFILL
                            );
                        }
                    }
                }
                None => { /* do nothing */ }
            };

            match rx_refill_from_signal_handler.try_recv() {
                Ok(c) => {
                    match c {
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            panic!("{}: Received invalid command.", LOG_TARGET_MOCK_REFILL);
                        }
                    };
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!(
                                "{}: Error in trying to receive command from signal handler: {e:?}",
                                LOG_TARGET_MOCK_REFILL
                            );
                        }
                    }
                }
            };

            // communicate with data logger
            match rx_refill_from_data_logger_opt {
                Some(rx_refill_from_data_logger) => {
                    match rx_refill_from_data_logger.try_recv() {
                        Ok(data_logger_request) => {
                            match data_logger_request {
                                InternalCommand::RequestSignal(data_logger_signal_request) => {
                                    match data_logger_signal_request {
                                        AquariumSignal::RefillPumpStatus => {
                                            match tx_refill_to_data_logger_opt {
                                                Some(tx_refill_to_data_logger) => {
                                                    match tx_refill_to_data_logger.send(stimuli) {
                                                        Ok(_) => { /* do nothing */ }
                                                        Err(e) => {
                                                            panic!(
                                                                "{}: Could not send signal request to DataLogger ({e:?})",
                                                                LOG_TARGET_MOCK_REFILL
                                                            );
                                                        }
                                                    };
                                                }
                                                None => {
                                                    panic!(
                                                        "{}: switch invoked without Sender for channel communication to data logger",
                                                        LOG_TARGET_MOCK_REFILL
                                                    );
                                                }
                                            }
                                        }
                                        _ => {
                                            panic!(
                                                "{}: switch received request for invalid signal {}",
                                                LOG_TARGET_MOCK_REFILL, data_logger_signal_request
                                            );
                                        }
                                    };
                                }
                                _ => {
                                    panic!(
                                        "{}: Received invalid command {} from DataLogger",
                                        LOG_TARGET_MOCK_REFILL, data_logger_request
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
                                        LOG_TARGET_MOCK_REFILL
                                    );
                                }
                            }
                        }
                    }
                }
                None => { /* do nothing */ }
            }

            match rx_refill_from_test_environment_opt {
                Some(rx_refill_from_test_environment) => {
                    match rx_refill_from_test_environment.try_recv() {
                        Ok(c) => {
                            let test_command = c.clone();
                            match c {
                                TestCommand::UpdateSignal(d) => {
                                    let test_signal = d.clone();
                                    match d {
                                        MockSignal::RefillPumpStatus(c) => stimuli = c,
                                        _ => {
                                            panic!(
                                                "{}: received test command for invalid signal {}",
                                                LOG_TARGET_MOCK_REFILL, test_signal
                                            );
                                        }
                                    }
                                }
                                _ => {
                                    panic!(
                                        "{}: received invalid command {}.",
                                        LOG_TARGET_MOCK_REFILL, test_command
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
                                        LOG_TARGET_MOCK_REFILL
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
        result
    }
}