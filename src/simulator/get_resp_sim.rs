/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::simulator::tcp_communication_error::TcpCommunicationError;
use crate::utilities::channel_content::InternalCommand;
use log::warn;
use std::sync::mpsc::{Receiver, Sender};

/// A trait that provides a generic method for communicating with a simulator.
pub trait GetResponseFromSimulatorTrait {
    /// Sends a request to a simulator via a channel and blocks until a response is received.
    ///
    /// This function standardizes the request/response pattern for fetching simulated
    /// sensor data. It sends an `InternalCommand` to a simulator thread (typically
    /// the `TcpCommunication` module) and waits for a floating-point value in return.
    ///
    /// # Arguments
    /// * `requester` - A `String` identifying the calling module, used for logging and error context.
    /// * `tx_to_tcp` - The sender end of the channel to the simulator thread.
    /// * `rx_from_tcp` - The receiver end of the channel from the simulator thread.
    /// * `internal_command` - The command to send, which should be a `RequestSignal` variant.
    ///
    /// # Returns
    /// A `Result` containing the simulated sensor value:
    /// - `Ok(f32)`: The floating-point value received from the simulator.
    /// - `Ok(0.0)`: If a non-`RequestSignal` command is passed, which is logged and ignored.
    ///
    /// # Errors
    /// Returns a `TcpCommunicationError` if the communication fails at any stage:
    /// - `TcpCommunicationError::SendingToTCPThreadFailed`: If sending the request fails,
    ///   which typically means the simulator thread has panicked or shut down and the
    ///   channel is disconnected.
    /// - `TcpCommunicationError::ReceivingFromTCPThreadFailed`: If receiving the response
    ///   fails, which also indicates the simulator thread is no longer available.
    /// - It will also propagate any `TcpCommunicationError` that the simulator
    ///   thread itself sends back within its `Result`.
    fn get_response_from_simulator(
        requester: String,
        tx_to_tcp: &Sender<InternalCommand>,
        rx_from_tcp: &Receiver<Result<f32, TcpCommunicationError>>,
        internal_command: InternalCommand,
    ) -> Result<f32, TcpCommunicationError> {
        match internal_command {
            InternalCommand::RequestSignal(_) => {
                // 1. Try to send the command, mapping the error if it fails.
                tx_to_tcp.send(internal_command).map_err(|e| {
                    TcpCommunicationError::SendingToTCPThreadFailed {
                        location: module_path!().to_string(),
                        requester: requester.clone(), // Clone for this error path
                        source: e,
                    }
                })?;

                // 2. Block and wait for the response, mapping the error if it fails.
                // The inner `Result` is the actual payload from the simulator.
                rx_from_tcp.recv().map_err(|e| {
                    TcpCommunicationError::ReceivingFromTCPThreadFailed {
                        location: module_path!().to_string(),
                        requester, // Move the original requester string here
                        source: e,
                    }
                })? // This '?' unwraps the outer Result<_, RecvError>
            }
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
