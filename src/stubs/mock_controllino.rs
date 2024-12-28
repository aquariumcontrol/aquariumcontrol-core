/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    const LOG_TARGET_MOCK_CONTROLLINO: &str = "mock_controllino";

    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender};
    use std::time::{Duration, SystemTime};

    use crate::stubs::actuation_event::tests::ActuationEvent;
    use crate::utilities::channel_content::InternalCommand;

    pub fn mock_controllino(
        tx_controllino_to_test_object: &Sender<bool>,
        rx_controllino_from_test_object: &Receiver<InternalCommand>,
    ) -> Vec<ActuationEvent> {
        let mut result: Vec<ActuationEvent> = Vec::new();

        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            // receive command from test object
            match rx_controllino_from_test_object.recv() {
                Ok(command) => {
                    match command {
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            // println!("mock_controllino: Received command {}", command);
                            // store tuple of SystemTime and command in vector
                            result.push(ActuationEvent::new(SystemTime::now(), command));
                        }
                    }
                }
                Err(e) => {
                    panic!(
                        "{}: Error when receiving actuation command from test object: {e:?}",
                        LOG_TARGET_MOCK_CONTROLLINO
                    );
                }
            }

            // send confirmation back
            match tx_controllino_to_test_object.send(true) {
                Ok(_) => {}
                Err(e) => {
                    panic!(
                        "{}: Error when sending back actuation command confirmation to test object: {e:?}",
                        LOG_TARGET_MOCK_CONTROLLINO
                    );
                }
            }

            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
        result
    }
}
