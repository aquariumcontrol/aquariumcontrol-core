/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use crate::stubs::mock_common::mock_common_receive_from_signal_handler;
    use crate::utilities::channel_content::InternalCommand;
    use crate::water::refill::RefillMonitorView;
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    const LOG_TARGET_MOCK_MONITOR: &str = "mock_monitors";

    pub fn mock_monitors(
        rx_from_refill: Receiver<RefillMonitorView>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        loop {
            if mock_common_receive_from_signal_handler(
                &rx_from_signal_handler,
                LOG_TARGET_MOCK_MONITOR,
            ) {
                break;
            }

            _ = rx_from_refill.try_recv();

            spin_sleeper.sleep(sleep_duration_hundred_millis);
        }
    }
}
