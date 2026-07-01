/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use crate::database::sql_interface_refill::DatabaseInterfaceRefillTrait;
    use crate::launch::channels::AquaSender;
    use crate::mocks::test_command::tests::TestCommand;
    use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
    use crate::water::refill_channels::RefillChannels;
    use crate::water::refill_error_states::RefillErrorStates;
    use crate::water::water_injection::WaterInjectionTrait;

    pub struct MockWaterInjection {
        tx_water_injection_to_test_environment_opt: Option<AquaSender<TestCommand>>,

        pub injection_recorder: Vec<Instant>,

        stimuli_refill_errors: RefillErrorStates,
    }

    impl MockWaterInjection {
        /// Creates a new instance.
        ///
        /// # Arguments
        /// * `tx_water_injection_to_test_environment_opt` - A parameter representing `tx_water_injection_to_test_environment_opt`.
        /// * `refill_errors` - A parameter representing `refill_errors`.
        pub fn new(
            tx_water_injection_to_test_environment_opt: Option<AquaSender<TestCommand>>,
            refill_errors: RefillErrorStates,
        ) -> MockWaterInjection {
            MockWaterInjection {
                tx_water_injection_to_test_environment_opt,
                injection_recorder: Vec::new(),
                stimuli_refill_errors: refill_errors,
            }
        }
    }

    impl WaterInjectionTrait for MockWaterInjection {
        /// Performs the `inject_water` operation.
        ///
        /// # Arguments
        /// * `refill_channels` - A mutable reference to the `RefillChannels` communication interface.
        /// * `refill_errors` - A parameter representing `refill_errors`.
        /// * `sql_interface_refill` - Database interface connection for refill operations.
        /// * `mutex_tank_level_switch` - Shared mutex for the tank level switch signals.
        /// * `mutex_device_scheduler_refill` - Mutex coordinating access to the device scheduler for refill.
        fn inject_water(
            &mut self,
            _refill_channels: &mut RefillChannels,
            refill_errors: &mut RefillErrorStates,
            _sql_interface_refill: &mut Box<dyn DatabaseInterfaceRefillTrait + Sync + Send>,
            _mutex_tank_level_switch: &Arc<Mutex<TankLevelSwitchSignals>>,
            _mutex_device_scheduler_refill: &Arc<Mutex<i32>>,
        ) -> bool {
            if self.tx_water_injection_to_test_environment_opt.is_some() {
                let mut tx_water_injection_to_test_environment = self
                    .tx_water_injection_to_test_environment_opt
                    .clone()
                    .unwrap();

                // copy refill errors from stimuli to mutable reference given as a parameter
                refill_errors.error_switch_stuck_high =
                    self.stimuli_refill_errors.error_switch_stuck_high;
                refill_errors.error_switch_stuck_low =
                    self.stimuli_refill_errors.error_switch_stuck_low;
                refill_errors.error_receive_tank_level_switch_position = self
                    .stimuli_refill_errors
                    .error_receive_tank_level_switch_position;
                refill_errors.error_sql_update_failed =
                    self.stimuli_refill_errors.error_sql_update_failed;
                refill_errors.error_sql_get_historic_data_failed = self
                    .stimuli_refill_errors
                    .error_sql_get_historic_data_failed;

                let _ = tx_water_injection_to_test_environment
                    .send(TestCommand::InformWaterInjectionEvent);
            }
            self.injection_recorder.push(Instant::now());
            false // simulate that no Quit command has been received
        }
    }
}
