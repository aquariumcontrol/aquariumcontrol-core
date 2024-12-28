/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    const LOG_TARGET_MOCK_SCHEDULE_CHECK: &str = "mock_schedule_check";

    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::time::Duration;

    use crate::utilities::channel_content::InternalCommand;
    use crate::stubs::mock_signal::tests::MockSignal;
    use crate::stubs::test_command::tests::TestCommand;

    // send via channel a value to the test object to indicate if actuation is permitted.
    pub fn mock_schedule_check(
        tx_schedule_check_to_test_object: &Sender<bool>,
        rx_schedule_check_from_test_object: &Receiver<InternalCommand>,
        rx_schedule_check_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        initial_stimuli: bool,
    ) {
        let mut stimuli = initial_stimuli;

        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        loop {
            match rx_schedule_check_from_test_object.try_recv() {
                Ok(c) => {
                    match c {
                        InternalCommand::ScheduleCheck => {
                            // allow control to actuate
                            let _ = tx_schedule_check_to_test_object.send(stimuli);
                        }
                        InternalCommand::Quit => {
                            // test case has finished
                            break;
                        }
                        _ => {
                            panic!("{}: test object sent invalid command.", LOG_TARGET_MOCK_SCHEDULE_CHECK);
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!(
                                "{}: Error in trying to receive command from test object: {e:?}",
                                LOG_TARGET_MOCK_SCHEDULE_CHECK
                            );
                        }
                    }
                }
            }
            match rx_schedule_check_from_test_environment_opt {
                Some(rx_schedule_check_from_test_environment) => {
                    match rx_schedule_check_from_test_environment.try_recv() {
                        Ok(c) => {
                            let test_command = c.clone();
                            match c {
                                TestCommand::UpdateSignal(d) => {
                                    let test_signal = d.clone();
                                    match d {
                                        MockSignal::ScheduleAllows(c) => stimuli = c,
                                        _ => {
                                            panic!(
                                                "{}: received test command for invalid signal {}",
                                                LOG_TARGET_MOCK_SCHEDULE_CHECK, test_signal
                                            );
                                        }
                                    }
                                }
                                _ => {
                                    panic!(
                                        "{}: received invalid command {}.",
                                        LOG_TARGET_MOCK_SCHEDULE_CHECK, test_command
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
                                        LOG_TARGET_MOCK_SCHEDULE_CHECK
                                    );
                                }
                            }
                        }
                    }
                }
                None => { /* do nothing */ }
            };
            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
    }
}