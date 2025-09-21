/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface_error::SqlInterfaceError;
    use crate::database::thermal_set_value_updater_trait::ThermalSetValueUpdaterTrait;
    use crate::launch::channels::{AquaChannelError, AquaReceiver};
    use crate::mocks::mock_common::mock_common_receive_from_signal_handler;
    use crate::mocks::mock_signal::tests::MockSignal;
    use crate::mocks::test_command::tests::TestCommand;
    use crate::utilities::channel_content::InternalCommand;
    use spin_sleep::SpinSleeper;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    #[allow(unused)]
    pub fn mock_ventilation(
        rx_ventilation_from_signal_handler: &mut AquaReceiver<InternalCommand>,
        mut rx_ventilation_from_test_environment_opt: Option<&mut AquaReceiver<TestCommand>>,
        mutex_ventilation_status: Arc<Mutex<bool>>,
    ) {
        let sleep_duration = Duration::from_millis(10);
        let spin_sleeper = SpinSleeper::default();

        loop {
            if mock_common_receive_from_signal_handler(
                rx_ventilation_from_signal_handler,
                module_path!(),
            ) {
                break;
            }

            match rx_ventilation_from_test_environment_opt {
                Some(ref mut rx_ventilation_from_test_environment) => {
                    match rx_ventilation_from_test_environment.try_recv() {
                        Ok(c) => {
                            let test_command = c.clone();
                            match c {
                                TestCommand::UpdateSignal(d) => {
                                    let test_signal = d.clone();
                                    match d {
                                        MockSignal::VentilationControlStatus(c) => {
                                            // Write signal to mutex
                                            {
                                                match mutex_ventilation_status.lock() {
                                                    Ok(mut d) => *d = c,
                                                    Err(_) => {
                                                        // Do nothing
                                                    }
                                                }
                                            }
                                        }
                                        _ => {
                                            panic!(
                                                "{}: received test command for invalid signal {}",
                                                module_path!(),
                                                test_signal
                                            );
                                        }
                                    }
                                }
                                _ => {
                                    panic!(
                                        "{}: received invalid command {}.",
                                        module_path!(),
                                        test_command
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                AquaChannelError::Empty => { /* empty buffer - no action required */
                                }
                                AquaChannelError::Disconnected => {
                                    panic!(
                                        "{}: Error in trying to receive command from test environment: {e:?}",
                                        module_path!()
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

    pub struct MockSqlInterfaceVentilationSetVals {
        active: bool,
        new_set_value_ventilation_switch_off_temperature: Option<f32>,
        new_set_value_ventilation_switch_on_temperature: Option<f32>,
        delay_duration: Option<Duration>,
        start_time: Instant,
    }

    impl MockSqlInterfaceVentilationSetVals {
        // Creates a new `MockSqlInterfaceVentilationSetVals` instance which will either actively
        // manipulate test data or behave neutrally depending on the parameters.
        //
        // # Arguments
        // * `active` - determine if test data shall be manipulated with the call to update_set_value
        // * `new_set_value_ventilation_switch_off_temperature` - new set value for switch off temperature
        //   to be applied to the mutable reference provided in update_set_value
        // * `new_set_value_ventilation_switch_on_temperature` - new set value for switch on temperature
        //   to be applied to the mutable reference provided in update_set_value
        // * `delay_duration` - duration of time which shall pass before manipulating test data
        //
        // # Returns
        // A new `VentilationSetValueMockUpdater` struct.
        pub fn new(
            active: bool,
            new_set_value_ventilation_switch_off_temperature: Option<f32>,
            new_set_value_ventilation_switch_on_temperature: Option<f32>,
            delay_duration: Option<Duration>,
        ) -> MockSqlInterfaceVentilationSetVals {
            MockSqlInterfaceVentilationSetVals {
                active,
                new_set_value_ventilation_switch_off_temperature,
                new_set_value_ventilation_switch_on_temperature,
                delay_duration,
                start_time: Instant::now(),
            }
        }
    }
    impl ThermalSetValueUpdaterTrait for MockSqlInterfaceVentilationSetVals {
        fn update_set_value(
            &mut self,
            ventilation_switch_off_temperature: &mut f32,
            ventilation_switch_on_temperature: &mut f32,
        ) -> Result<(), SqlInterfaceError> {
            if self.active
                && self.delay_duration.is_some()
                && self
                    .new_set_value_ventilation_switch_off_temperature
                    .is_some()
                && self
                    .new_set_value_ventilation_switch_on_temperature
                    .is_some()
            {
                // manipulate the test data when enabled and after the delay has passed.
                if Instant::now().duration_since(self.start_time) > self.delay_duration.unwrap() {
                    *ventilation_switch_off_temperature = self
                        .new_set_value_ventilation_switch_off_temperature
                        .unwrap();
                    *ventilation_switch_on_temperature = self
                        .new_set_value_ventilation_switch_on_temperature
                        .unwrap();
                }
            }
            Ok(())
        }
    }
}
