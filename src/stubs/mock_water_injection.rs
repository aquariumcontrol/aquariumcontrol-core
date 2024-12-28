/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use std::sync::mpsc::{Receiver, Sender};
    use std::time::SystemTime;

    use crate::database::sql_interface_refill::DatabaseInterfaceRefillTrait;
    use crate::stubs::test_command::tests::TestCommand;
    use crate::utilities::channel_content::InternalCommand;
    use crate::water::refill_errors::RefillErrors;
    use crate::water::water_injection::WaterInjectionTrait;

    pub struct MockWaterInjection {
        tx_water_injection_to_test_environment_opt: Option<Sender<TestCommand>>,

        pub injection_recorder: Vec<SystemTime>,
    }

    impl MockWaterInjection {
        pub fn new(
            tx_water_injection_to_test_environment_opt: Option<Sender<TestCommand>>,
        ) -> MockWaterInjection {
            MockWaterInjection {
                tx_water_injection_to_test_environment_opt:
                    tx_water_injection_to_test_environment_opt,
                injection_recorder: Vec::new(),
            }
        }
    }

    impl WaterInjectionTrait for MockWaterInjection {
        fn inject_water(
            &mut self,
            _tx_to_controllino: &Sender<InternalCommand>,
            _rx_from_controllino: &Receiver<bool>,
            _tx_to_tank_level_switch: &Sender<InternalCommand>,
            _rx_from_tank_level_switch: &Receiver<bool>,
            _rx_from_signal_handler: &Receiver<InternalCommand>,
            _tx_to_data_logger: &Sender<bool>,
            _rx_from_data_logger: &Receiver<InternalCommand>,
            _refill_errors: &mut RefillErrors,
            _sql_interface_refill: &mut Box<dyn DatabaseInterfaceRefillTrait + Sync + Send>,
        ) -> bool {
            if self.tx_water_injection_to_test_environment_opt.is_some() {
                let tx_water_injection_to_test_environment = self
                    .tx_water_injection_to_test_environment_opt
                    .clone()
                    .unwrap();
                let _ = tx_water_injection_to_test_environment
                    .send(TestCommand::InformWaterInjectionEvent);
            }
            self.injection_recorder.push(SystemTime::now());
            false // simulate that no Quit command has been received
        }
    }
}
