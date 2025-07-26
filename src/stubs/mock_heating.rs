/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface_error::SqlInterfaceError;
    use crate::database::sql_interface_heating_setvals::HeatingSetValueUpdaterTrait;
    use crate::stubs::mock_common::mock_common_receive_from_signal_handler;
    use crate::stubs::mock_signal::tests::MockSignal;
    use crate::stubs::test_command::tests::TestCommand;
    use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    pub fn mock_heating(
        tx_heating_to_tank_level_switch_opt: Option<&Sender<InternalCommand>>,
        rx_heating_from_tank_level_switch_opt: Option<&Receiver<bool>>,
        rx_heating_from_signal_handler: &Receiver<InternalCommand>,
        rx_heating_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        mutex_heating_status: Arc<Mutex<bool>>,
    ) {
        let sleep_duration = Duration::from_millis(10);
        let spin_sleeper = SpinSleeper::default();

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
                                                        module_path!()
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                None => {
                                    panic!(
                                        "{}: invoked with valid sender but with empty receiver part for channel communication to tank level switch.",
                                        module_path!()
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            panic!(
                                "{}: Could not send signal request to test object: {e:?}",
                                module_path!()
                            );
                        }
                    }
                }
                None => { /* do nothing */ }
            };

            if mock_common_receive_from_signal_handler(
                rx_heating_from_signal_handler,
                module_path!(),
            ) {
                break;
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
                                        MockSignal::HeatingControlStatus(c) => {
                                            // Write signal to mutex
                                            {
                                                match mutex_heating_status.lock() {
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
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
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

    pub struct MockSqlInterfaceHeatingSetVals {
        active: bool,
        new_set_value_heating_switch_off_temperature: Option<f32>,
        new_set_value_heating_switch_on_temperature: Option<f32>,
        delay_duration: Option<Duration>,
        start_time: Instant,
    }

    impl MockSqlInterfaceHeatingSetVals {
        // Creates a new `MockSqlInterfaceHeatingSetVals` instance which will either actively
        // manipulate test data or behave neutrally depending on the parameters.
        //
        // # Arguments
        // * `active` - determine if test data shall be manipulated with the call to update_set_value
        // * `new_set_value_heating_switch_off_temperature` - new set value for switch off temperature
        //   to be applied to the mutable reference provided in update_set_value
        // * `new_set_value_heating_switch_on_temperature` - new set value for switch on temperature
        //   to be applied to the mutable reference provided in update_set_value
        // * `delay_duration` - duration of time which shall pass before manipulating test data
        //
        // # Returns
        // A new `HeatingSetValueMockUpdater` struct.
        pub fn new(
            active: bool,
            new_set_value_heating_switch_off_temperature: Option<f32>,
            new_set_value_heating_switch_on_temperature: Option<f32>,
            delay_duration: Option<Duration>,
        ) -> MockSqlInterfaceHeatingSetVals {
            MockSqlInterfaceHeatingSetVals {
                active,
                new_set_value_heating_switch_off_temperature,
                new_set_value_heating_switch_on_temperature,
                delay_duration,
                start_time: Instant::now(),
            }
        }
    }
    impl HeatingSetValueUpdaterTrait for MockSqlInterfaceHeatingSetVals {
        fn update_set_value(
            &mut self,
            heating_switch_off_temperature: &mut f32,
            heating_switch_on_temperature: &mut f32,
        ) -> Result<(), SqlInterfaceError> {
            if self.active
                && self.delay_duration.is_some()
                && self.new_set_value_heating_switch_off_temperature.is_some()
                && self.new_set_value_heating_switch_on_temperature.is_some()
            {
                // manipulate the test data when enabled and after the delay has passed.
                if Instant::now().duration_since(self.start_time) > self.delay_duration.unwrap() {
                    *heating_switch_off_temperature =
                        self.new_set_value_heating_switch_off_temperature.unwrap();
                    *heating_switch_on_temperature =
                        self.new_set_value_heating_switch_on_temperature.unwrap();
                }
            }
            Ok(())
        }
    }
}
