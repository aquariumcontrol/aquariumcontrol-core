/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[allow(unused)]
#[cfg(test)]
pub mod tests {
    use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
    use crate::mocks::mock_signal::tests::MockSignal;
    use crate::mocks::test_command::tests::TestCommand;
    use crate::sensors::tank_level_switch::TankLevelSwitchError;
    use crate::simulator::tcp_communication::TcpCommunicationError;
    use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
    use spin_sleep::SpinSleeper;
    use std::time::Duration;

    /// Listens to channel once and sends TankLevelSwitchError::SimulatorChannelNotProvided
    ///
    /// # Arguments
    /// * `tx_simulator_to_test_object` - Channel for communication to the test object
    /// * `rx_simulator_from_test_object` - Channel for receiving from test object
    pub async fn mock_simulator(
        tx_simulator_to_test_object: &mut AquaSender<Result<f32, TcpCommunicationError>>,
        rx_simulator_from_test_object: &mut AquaReceiver<InternalCommand>,
        first_resp_opt: Option<f32>,
    ) {
        // If caller provides valid argument, then return it on first communication
        if let Some(first_resp) = first_resp_opt {
            let request = rx_simulator_from_test_object.recv().await;
            tx_simulator_to_test_object.send(Ok(first_resp));
        }

        // Intentionally return error
        _ = rx_simulator_from_test_object.recv().await;
        tx_simulator_to_test_object.send(Err(TcpCommunicationError::MockError {
            location: module_path!().to_string(),
        }));
    }
}
