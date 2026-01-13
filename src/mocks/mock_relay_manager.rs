/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use crate::launch::channels::{AquaReceiver, AquaSender};
    use crate::mocks::mock_actuation_event::tests::MockActuationEvent;
    use crate::utilities::channel_content::{ActuatorState, AquariumDevice, InternalCommand};
    use spin_sleep::SpinSleeper;
    use std::time::Duration;

    pub struct MockActuatorStates {
        pub peristaltic_pump1_actuation_state: ActuatorState,
        pub peristaltic_pump2_actuation_state: ActuatorState,
        pub peristaltic_pump3_actuation_state: ActuatorState,
        pub peristaltic_pump4_actuation_state: ActuatorState,
        pub skimmer_actuation_state: ActuatorState,
        pub main_pump1_actuation_state: ActuatorState,
        pub main_pump2_actuation_state: ActuatorState,
        pub aux_pump1_actuation_state: ActuatorState,
        pub aux_pump2_actuation_state: ActuatorState,
        pub feeder_actuation_state: ActuatorState,
        pub refill_pump_actuation_state: ActuatorState,
        pub heater_actuation_state: ActuatorState,
        pub ventilation_actuation_state: ActuatorState,
    }

    impl MockActuatorStates {
        pub fn new() -> MockActuatorStates {
            MockActuatorStates {
                peristaltic_pump1_actuation_state: ActuatorState::Undefined,
                peristaltic_pump2_actuation_state: ActuatorState::Undefined,
                peristaltic_pump3_actuation_state: ActuatorState::Undefined,
                peristaltic_pump4_actuation_state: ActuatorState::Undefined,
                skimmer_actuation_state: ActuatorState::Undefined,
                main_pump1_actuation_state: ActuatorState::Undefined,
                main_pump2_actuation_state: ActuatorState::Undefined,
                aux_pump1_actuation_state: ActuatorState::Undefined,
                aux_pump2_actuation_state: ActuatorState::Undefined,
                feeder_actuation_state: ActuatorState::Undefined,
                refill_pump_actuation_state: ActuatorState::Undefined,
                heater_actuation_state: ActuatorState::Undefined,
                ventilation_actuation_state: ActuatorState::Undefined,
            }
        }

        pub fn check_terminal_condition_refill(&self) {
            assert_eq!(self.refill_pump_actuation_state, ActuatorState::Off);
            assert_eq!(self.skimmer_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.main_pump1_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.main_pump2_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.aux_pump1_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.aux_pump2_actuation_state, ActuatorState::Undefined);
            assert_eq!(
                self.peristaltic_pump1_actuation_state,
                ActuatorState::Undefined
            );
            assert_eq!(
                self.peristaltic_pump2_actuation_state,
                ActuatorState::Undefined
            );
            assert_eq!(
                self.peristaltic_pump3_actuation_state,
                ActuatorState::Undefined
            );
            assert_eq!(
                self.peristaltic_pump4_actuation_state,
                ActuatorState::Undefined
            );
            assert_eq!(self.feeder_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.heater_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.ventilation_actuation_state, ActuatorState::Undefined);
        }

        pub fn check_terminal_condition_ventilation(&self) {
            assert_eq!(self.refill_pump_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.skimmer_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.main_pump1_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.main_pump2_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.aux_pump1_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.aux_pump2_actuation_state, ActuatorState::Undefined);
            assert_eq!(
                self.peristaltic_pump1_actuation_state,
                ActuatorState::Undefined
            );
            assert_eq!(
                self.peristaltic_pump2_actuation_state,
                ActuatorState::Undefined
            );
            assert_eq!(
                self.peristaltic_pump3_actuation_state,
                ActuatorState::Undefined
            );
            assert_eq!(
                self.peristaltic_pump4_actuation_state,
                ActuatorState::Undefined
            );
            assert_eq!(self.feeder_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.heater_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.ventilation_actuation_state, ActuatorState::On);
        }

        pub fn check_terminal_condition_heating(&self) {
            assert_eq!(self.refill_pump_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.skimmer_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.main_pump1_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.main_pump2_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.aux_pump1_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.aux_pump2_actuation_state, ActuatorState::Undefined);
            assert_eq!(
                self.peristaltic_pump1_actuation_state,
                ActuatorState::Undefined
            );
            assert_eq!(
                self.peristaltic_pump2_actuation_state,
                ActuatorState::Undefined
            );
            assert_eq!(
                self.peristaltic_pump3_actuation_state,
                ActuatorState::Undefined
            );
            assert_eq!(
                self.peristaltic_pump4_actuation_state,
                ActuatorState::Undefined
            );
            assert_eq!(self.feeder_actuation_state, ActuatorState::Undefined);
            assert_eq!(self.heater_actuation_state, ActuatorState::Off);
            assert_eq!(self.ventilation_actuation_state, ActuatorState::Undefined);
        }
    }

    // Helper function to record the actuation state of the devices.
    fn update_actuator_states(
        command: InternalCommand,
        mock_actuator_states: &mut MockActuatorStates,
    ) {
        // First, determine the new state and the target device from the command.
        let (device, new_state) = match command {
            InternalCommand::SwitchOn(device) => (device, ActuatorState::On),
            InternalCommand::SwitchOff(device) => (device, ActuatorState::Off),
            // If it's any other command, we don't need to do anything.
            _ => return,
        };

        // Now, update the state of the correct pump in a single match block.
        match device {
            AquariumDevice::PeristalticPump1 => {
                mock_actuator_states.peristaltic_pump1_actuation_state = new_state;
            }
            AquariumDevice::PeristalticPump2 => {
                mock_actuator_states.peristaltic_pump2_actuation_state = new_state;
            }
            AquariumDevice::PeristalticPump3 => {
                mock_actuator_states.peristaltic_pump3_actuation_state = new_state;
            }
            AquariumDevice::PeristalticPump4 => {
                mock_actuator_states.peristaltic_pump4_actuation_state = new_state;
            }
            AquariumDevice::Skimmer => {
                mock_actuator_states.skimmer_actuation_state = new_state;
            }
            AquariumDevice::MainPump1 => {
                mock_actuator_states.main_pump1_actuation_state = new_state;
            }
            AquariumDevice::MainPump2 => {
                mock_actuator_states.main_pump2_actuation_state = new_state;
            }
            AquariumDevice::AuxPump1 => {
                mock_actuator_states.aux_pump1_actuation_state = new_state;
            }
            AquariumDevice::AuxPump2 => {
                mock_actuator_states.aux_pump2_actuation_state = new_state;
            }
            AquariumDevice::Feeder => {
                mock_actuator_states.feeder_actuation_state = new_state;
            }
            AquariumDevice::Heater => {
                mock_actuator_states.heater_actuation_state = new_state;
            }
            AquariumDevice::Ventilation => {
                mock_actuator_states.ventilation_actuation_state = new_state;
            }
            AquariumDevice::RefillPump => {
                mock_actuator_states.refill_pump_actuation_state = new_state;
            }
            AquariumDevice::Beacon => {
                // Do nothing
            }
        }
    }

    pub fn mock_relay_manager(
        tx_relay_manager_to_test_object: &mut AquaSender<bool>,
        rx_relay_manager_from_test_object: &mut AquaReceiver<InternalCommand>,
    ) -> (Vec<MockActuationEvent>, MockActuatorStates) {
        let mut result: Vec<MockActuationEvent> = Vec::new();
        let mut mock_actuator_states = MockActuatorStates::new();

        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            // receive command from the test object
            match rx_relay_manager_from_test_object.recv() {
                Ok(command) => {
                    match command {
                        InternalCommand::SwitchOn(_) => {
                            update_actuator_states(command.clone(), &mut mock_actuator_states);
                            result.push(MockActuationEvent::new(command));
                        }
                        InternalCommand::SwitchOff(_) => {
                            update_actuator_states(command.clone(), &mut mock_actuator_states);
                            result.push(MockActuationEvent::new(command));
                        }
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            // println!("mock_relay_manager: Received command {}", command);
                            result.push(MockActuationEvent::new(command));
                        }
                    }
                }
                Err(e) => {
                    panic!(
                        "{}: Error when receiving actuation command from test object: {e:?}",
                        module_path!()
                    );
                }
            }

            // send confirmation back
            match tx_relay_manager_to_test_object.send(true) {
                Ok(_) => {}
                Err(e) => {
                    panic!(
                        "{}: Error when sending back actuation command confirmation to test object: {e:?}",
                        module_path!()
                    );
                }
            }

            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
        println!("mock_relay_manager: done");
        (result, mock_actuator_states)
    }
}
