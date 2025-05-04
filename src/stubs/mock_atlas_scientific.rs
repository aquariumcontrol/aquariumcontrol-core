/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[allow(unused)]
#[cfg(test)]
pub mod tests {
    const LOG_TARGET_MOCK_ATLAS_SCIENTIFIC: &str = "mock_atlas_scientific";

    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::time::Duration;

    use crate::stubs::mock_signal::tests::MockSignal;
    use crate::stubs::test_command::tests::TestCommand;
    use crate::utilities::channel_content::{AquariumSignal, InternalCommand};

    pub fn mock_atlas_scientific(
        tx_atlas_scientific_to_test_object: &Sender<f32>,
        rx_atlas_scientific_from_test_object: &Receiver<InternalCommand>,
        rx_atlas_scientific_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        initial_stimuli_water_temperature: f32,
        initial_stimuli_conductivity: f32,
        initial_stimuli_ph: f32,
    ) {
        let mut stimuli_conductivity = initial_stimuli_conductivity;
        let mut stimuli_water_temperature = initial_stimuli_water_temperature;
        let mut stimuli_ph = initial_stimuli_ph;

        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            match rx_atlas_scientific_from_test_object.try_recv() {
                Ok(c) => {
                    let response_value = match c {
                        InternalCommand::RequestSignal(d) => match d {
                            AquariumSignal::WaterTemperature => stimuli_water_temperature,
                            AquariumSignal::Conductivity => stimuli_conductivity,
                            AquariumSignal::pH => stimuli_ph,
                            _ => {
                                panic!(
                                    "{}: inapplicable signal request {}",
                                    LOG_TARGET_MOCK_ATLAS_SCIENTIFIC, d
                                );
                            }
                        },
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            panic!(
                                "{}: Received invalid command.",
                                LOG_TARGET_MOCK_ATLAS_SCIENTIFIC
                            );
                        }
                    };
                    match tx_atlas_scientific_to_test_object.send(response_value) {
                        Ok(()) => {}
                        Err(e) => {
                            panic!(
                                "{}: Error in trying to send response back data logger: {e:?}",
                                LOG_TARGET_MOCK_ATLAS_SCIENTIFIC
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
                                LOG_TARGET_MOCK_ATLAS_SCIENTIFIC
                            );
                        }
                    }
                }
            }

            match rx_atlas_scientific_from_test_environment_opt {
                Some(rx_atlas_scientific_from_test_environment) => {
                    match rx_atlas_scientific_from_test_environment.try_recv() {
                        Ok(c) => {
                            let test_command = c.clone();
                            match c {
                                TestCommand::UpdateSignal(d) => {
                                    let test_signal = d.clone();
                                    match d {
                                        MockSignal::WaterTemperature(c) => {
                                            stimuli_water_temperature = c
                                        }
                                        MockSignal::Conductivity(c) => stimuli_conductivity = c,
                                        MockSignal::pH(c) => stimuli_ph = c,
                                        _ => {
                                            panic!(
                                                "{}: received test command for invalid signal {}",
                                                LOG_TARGET_MOCK_ATLAS_SCIENTIFIC, test_signal
                                            );
                                        }
                                    }
                                }
                                _ => {
                                    panic!(
                                        "{}: received invalid command {}.",
                                        LOG_TARGET_MOCK_ATLAS_SCIENTIFIC, test_command
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
                                        LOG_TARGET_MOCK_ATLAS_SCIENTIFIC
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
