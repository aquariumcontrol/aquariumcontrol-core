/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    const LOG_TARGET_MOCK_DATA_LOGGER: &str = "mock_data_logger";

    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::time::Duration;

    use crate::utilities::channel_content::InternalCommand;
    
    pub fn mock_data_logger(
        _tx_data_logger_to_test_object: &Sender<InternalCommand>,
        rx_data_logger_from_test_object: &Receiver<bool>,
        rx_data_logger_from_signal_handler: &Receiver<InternalCommand>,
    ) {
        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        loop {
            // receive information for data logger
            match rx_data_logger_from_test_object.try_recv() {
                Ok(_) => { /* do nothing */ }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            // for this mock thread, this case occurrs - test case execution has concluded
                            break;
                        }
                    }
                }
            }
            // receive quit signal
            match rx_data_logger_from_signal_handler.try_recv() {
                Ok(c) => {
                    match c {
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            panic!("{}: Received invalid command.", LOG_TARGET_MOCK_DATA_LOGGER);
                        }
                    };
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            // for this mock thread, this case occurrs - test case execution has concluded
                            break;
                        }
                    }
                }
            }
            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
    }
}