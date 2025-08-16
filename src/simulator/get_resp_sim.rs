/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::simulator::tcp_communication_error::TcpCommunicationError;
use crate::utilities::channel_content::InternalCommand;
use log::warn;
use std::sync::mpsc::{Receiver, Sender};

/// The trait implements communication with the simulator instead of real HW.
pub trait GetResponseFromSimulatorTrait {
    /// Requests a response from the simulator via TCP communication by sending
    /// an internal message via the channel to the TCP thread.
    ///
    /// This function is used **solely for development and testing purposes**.
    /// It sends a `RequestSignal` command to the TCP communication thread (which
    /// acts as a simulator) and then synchronously waits for a response.
    /// Inapplicable commands are logged as warnings and return a default success value.
    ///
    /// # Arguments
    /// * `requester` - For debugging purposes: identify the thread which is requesting the response.
    /// * `tx_to_tcp` - A reference to the sender channel for sending commands to the TCP communication thread.
    /// * `rx_from_tcp` - A reference to the receiver channel for receiving `Result<f32, TcpCommunicationError>` data from the TCP communication thread.
    /// * `internal_command` - The `InternalCommand` to send, expected to be a `RequestSignal` for sensor data.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(f32)`: Containing the simulated sensor value on success.
    /// - `Ok(0.0)`: For inapplicable commands that are ignored.
    /// - `Err(TcpCommunicationError)`: If sending the request or receiving the response fails.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - Sending the `internal_command` to the TCP thread fails (e.g., the channel is disconnected).
    /// - Receiving the response from the TCP thread fails (e.g., the channel is disconnected).
    fn get_response_from_simulator(
        requester: String,
        tx_to_tcp: &Sender<InternalCommand>,
        rx_from_tcp: &Receiver<Result<f32, TcpCommunicationError>>,
        internal_command: InternalCommand,
    ) -> Result<f32, TcpCommunicationError> {
        match internal_command {
            InternalCommand::RequestSignal(_) => match tx_to_tcp.send(internal_command) {
                Ok(()) => rx_from_tcp.recv().unwrap_or_else(|e| {
                    Err(TcpCommunicationError::ReceivingFromTCPThreadFailed {
                        location: module_path!().to_string(),
                        requester,
                        source: e,
                    })
                }),
                Err(e) => Err(TcpCommunicationError::SendingToTCPThreadFailed {
                    location: module_path!().to_string(),
                    requester,
                    source: e,
                }),
            },
            _ => {
                warn!(
                    target: module_path!(),
                    "ignoring not applicable internal command ({internal_command}) from {requester}"
                );
                Ok(0.0)
            }
        }
    }
}
