/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::launch::channels::{AquaReceiver, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use spin_sleep::SpinSleeper;
use std::time::Duration;

/// Helper function for test cases to simulate the signal handler.
///
/// The function waits for a determined period and then sends the Quit and the Terminate
/// commands to the test object.
///
/// # Arguments
/// * `tx_signal_handler_to_feed` - Sender part of the channel for communication to the test object
/// * `rx_signal_handler_from_feed` - Receiver part of the channel for communication from the test object
/// * `spin_sleeper` - SpinSleeper instance used for waiting before sending Quit command to the test object
/// * `sleep_duration` - sleep duration used for waiting before sending Quit command to the test object
pub fn create_mock_signal_handler(
    tx_signal_handler_to_feed: &mut AquaSender<InternalCommand>,
    rx_signal_handler_from_feed: &mut AquaReceiver<bool>,
    spin_sleeper: SpinSleeper,
    sleep_duration: Duration,
) {
    spin_sleeper.sleep(sleep_duration);
    let _ = tx_signal_handler_to_feed.send(InternalCommand::Quit);
    rx_signal_handler_from_feed.blocking_recv().unwrap();
    let _ = tx_signal_handler_to_feed.send(InternalCommand::Terminate);
}
