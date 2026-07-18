/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

pub mod tests {
    use crate::launch::channels::AquaSender;
    use crate::recorder::{
        data_injection::DataInjectionTrait, recorder_data_frame::RecorderDataFrame,
    };
    use crate::utilities::channel_content::InternalCommand;
    use mysql::PooledConn;
    use std::sync::{Arc, Mutex};

    /// A mock for the DataInjectionTrait that captures the data frame and then
    /// immediately sends a Quit signal to terminate the data_logger thread.
    /// This allows for testing one full cycle of the `execute` loop without
    /// needing to wait for timers.
    pub struct MockInjectorWithQuit {
        pub injected_data: Arc<Mutex<Option<RecorderDataFrame>>>,
        pub tx_signal_handler_to_data_logger: AquaSender<InternalCommand>,
    }

    impl DataInjectionTrait for MockInjectorWithQuit {
        /// Performs the `inject_data` operation.
        ///
        /// # Arguments
        /// * `data_frame` - A parameter representing `data_frame`.
        /// * `conn` - An active pooled database connection.
        fn inject_data(&mut self, data_frame: RecorderDataFrame, conn: PooledConn) -> PooledConn {
            // Capture the first data frame that gets injected.
            let mut data = self.injected_data.lock().unwrap();
            if data.is_none() {
                *data = Some(data_frame);
            }

            let mut tx_signal_handler_to_data_logger_clone =
                self.tx_signal_handler_to_data_logger.clone();

            let _ = std::thread::spawn(move || {
                // Immediately send Quit to stop the loop after one successful injection.
                // Then wait for acknowledgement and send terminate command.
                let _ = tx_signal_handler_to_data_logger_clone.send(InternalCommand::Quit);
                println!("{}: sent quit command", module_path!());
            });
            conn
        }
    }
}
