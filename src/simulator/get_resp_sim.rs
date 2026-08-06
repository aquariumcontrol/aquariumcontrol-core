/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Provides a standardized trait for fetching data from a simulator.
//!
//! This module defines the `GetResponseFromSimulatorTrait`, which offers a single,
//! consistent method for any part of the application to request and receive data
//! from a simulator thread (typically the `TcpCommunication` module).
//!
//! By providing this functionality as a trait with a default implementation, it
//! ensures that all client modules (like `SensorManager`, `TankLevelSwitch`, etc.)
//! use the exact same logic for the request/response cycle, reducing code
//! duplication and centralizing error handling.
//!
//! ## Key Components
//!
//! - **`GetResponseFromSimulatorTrait`**: A trait that can be implemented by any
//!   struct that needs to communicate with the simulator.
//!
//! - **`get_response_from_simulator()`**: The default method provided by the trait.
//!   It encapsulates the entire synchronous, blocking communication pattern:
//!   1.  Sends an `InternalCommand` to the simulator thread.
//!   2.  Blocks and waits for a `Result<f32, ...>` to be sent back.
//!   3.  Handles all channel-related errors, mapping them to the application's
//!       `TcpCommunicationError` type for consistent error handling.
//!
//! ## Design and Architecture
//!
//! - **Standardization via Trait**: The primary design goal is to enforce a single,
//!   robust communication pattern. Instead of each sensor module implementing its
//!   own version of a send/receive loop, they simply implement this empty trait
//!   to gain access to the pre-built `get_response_from_simulator` method.
//!
//! - **Centralized Logic**: All logic for sending, receiving, and handling channel
//!   disconnection errors is located in one place. If the communication protocol
//!   needs to be updated (e.g., to add timeouts or retries), it only needs to be
//!   changed here.
//!
//! - **Synchronous Communication**: The method uses a blocking `recv()` call. This
//!   simplifies the logic for the client threads, as they can treat the call as a
//!   standard function that returns a value, without needing to manage asynchronous
//!   callbacks or futures for this specific interaction.

use crate::launch::channels::{AquaReceiver, AquaSender};
use crate::simulator::tcp_communication_error::TcpCommunicationError;
use crate::utilities::channel_content::InternalCommand;
use log::warn;

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
    #[allow(async_fn_in_trait)]
    async fn get_response_from_simulator(
        requester: String,
        tx_to_tcp: &mut AquaSender<InternalCommand>,
        rx_from_tcp: &mut AquaReceiver<Result<f32, TcpCommunicationError>>,
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
                rx_from_tcp.recv().await.map_err(|e| {
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

    fn get_response_from_simulator_blocking(
        requester: String,
        tx_to_tcp: &mut AquaSender<InternalCommand>,
        rx_from_tcp: &mut AquaReceiver<Result<f32, TcpCommunicationError>>,
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
                rx_from_tcp.blocking_recv().map_err(|e| {
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

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub mod tests {
    use super::*;
    use crate::launch::channels::channel;
    use crate::utilities::channel_content::AquariumSignal;

    struct TestRequester;
    impl GetResponseFromSimulatorTrait for TestRequester {}

    #[tokio::test]
    // Verifies get_response_from_simulator async returns sensor value for RequestSignal and 0.0 for other commands.
    // [utest->swreq~simulator_sensor_signals~1]
    async fn test_get_response_from_simulator_async_happy_path_and_fallback() {
        let (mut tx_cmd, mut rx_cmd) = channel::<InternalCommand>(10, false);
        let (mut tx_resp, mut rx_resp) = channel::<Result<f32, TcpCommunicationError>>(10, false);

        // Fallback branch: non-RequestSignal returns Ok(0.0)
        let res_fallback = TestRequester::get_response_from_simulator(
            "test_loc".to_string(),
            &mut tx_cmd,
            &mut rx_resp,
            InternalCommand::Quit,
        )
        .await;
        assert_eq!(res_fallback.unwrap(), 0.0);

        // RequestSignal branch: sends signal request and receives response
        let handle = tokio::spawn(async move {
            let cmd = rx_cmd.recv().await.unwrap();
            assert!(matches!(
                cmd,
                InternalCommand::RequestSignal(AquariumSignal::WaterTemperature)
            ));
            tx_resp.send(Ok(25.5)).unwrap();
        });

        let res = TestRequester::get_response_from_simulator(
            "test_loc".to_string(),
            &mut tx_cmd,
            &mut rx_resp,
            InternalCommand::RequestSignal(AquariumSignal::WaterTemperature),
        )
        .await;

        assert_eq!(res.unwrap(), 25.5);
        handle.await.unwrap();
    }

    #[tokio::test]
    // Verifies get_response_from_simulator async returns SendingToTCPThreadFailed and ReceivingFromTCPThreadFailed on channel disconnections.
    // [utest->swreq~simulator_sensor_signals~1]
    async fn test_get_response_from_simulator_async_channel_errors() {
        // 1. Send error: rx_cmd is dropped
        let (mut tx_cmd, rx_cmd) = channel::<InternalCommand>(10, false);
        let (_tx_resp, mut rx_resp) = channel::<Result<f32, TcpCommunicationError>>(10, false);
        drop(rx_cmd);

        let res_send_err = TestRequester::get_response_from_simulator(
            "test_loc".to_string(),
            &mut tx_cmd,
            &mut rx_resp,
            InternalCommand::RequestSignal(AquariumSignal::WaterTemperature),
        )
        .await;
        assert!(matches!(
            res_send_err,
            Err(TcpCommunicationError::SendingToTCPThreadFailed { .. })
        ));

        // 2. Recv error: tx_resp is dropped after receiving command
        let (mut tx_cmd, mut rx_cmd) = channel::<InternalCommand>(10, false);
        let (tx_resp, mut rx_resp) = channel::<Result<f32, TcpCommunicationError>>(10, false);

        let handle = tokio::spawn(async move {
            let _ = rx_cmd.recv().await;
            drop(tx_resp);
        });

        let res_recv_err = TestRequester::get_response_from_simulator(
            "test_loc".to_string(),
            &mut tx_cmd,
            &mut rx_resp,
            InternalCommand::RequestSignal(AquariumSignal::WaterTemperature),
        )
        .await;
        assert!(matches!(
            res_recv_err,
            Err(TcpCommunicationError::ReceivingFromTCPThreadFailed { .. })
        ));

        handle.await.unwrap();
    }

    #[test]
    // Verifies get_response_from_simulator_blocking returns sensor value for RequestSignal and 0.0 for other commands.
    // [utest->swreq~simulator_sensor_signals~1]
    fn test_get_response_from_simulator_blocking_happy_path_and_fallback() {
        let (mut tx_cmd, mut rx_cmd) = channel::<InternalCommand>(10, false);
        let (mut tx_resp, mut rx_resp) = channel::<Result<f32, TcpCommunicationError>>(10, false);

        // Fallback branch: non-RequestSignal returns Ok(0.0)
        let res_fallback = TestRequester::get_response_from_simulator_blocking(
            "test_loc".to_string(),
            &mut tx_cmd,
            &mut rx_resp,
            InternalCommand::Quit,
        );
        assert_eq!(res_fallback.unwrap(), 0.0);

        // RequestSignal branch: sends signal request and receives response
        let handle = std::thread::spawn(move || {
            let cmd = rx_cmd.blocking_recv().unwrap();
            assert!(matches!(
                cmd,
                InternalCommand::RequestSignal(AquariumSignal::pH)
            ));
            tx_resp.send(Ok(7.2)).unwrap();
        });

        let res = TestRequester::get_response_from_simulator_blocking(
            "test_loc".to_string(),
            &mut tx_cmd,
            &mut rx_resp,
            InternalCommand::RequestSignal(AquariumSignal::pH),
        );

        assert_eq!(res.unwrap(), 7.2);
        handle.join().unwrap();
    }

    #[test]
    // Verifies get_response_from_simulator_blocking handles channel send/recv errors and propagates simulator payload errors.
    // [utest->swreq~simulator_sensor_signals~1]
    fn test_get_response_from_simulator_blocking_channel_and_inner_errors() {
        // 1. Send error: rx_cmd is dropped
        let (mut tx_cmd, rx_cmd) = channel::<InternalCommand>(10, false);
        let (_tx_resp, mut rx_resp) = channel::<Result<f32, TcpCommunicationError>>(10, false);
        drop(rx_cmd);

        let res_send_err = TestRequester::get_response_from_simulator_blocking(
            "test_loc".to_string(),
            &mut tx_cmd,
            &mut rx_resp,
            InternalCommand::RequestSignal(AquariumSignal::WaterTemperature),
        );
        assert!(matches!(
            res_send_err,
            Err(TcpCommunicationError::SendingToTCPThreadFailed { .. })
        ));

        // 2. Recv error: tx_resp is dropped
        let (mut tx_cmd, mut rx_cmd) = channel::<InternalCommand>(10, false);
        let (tx_resp, mut rx_resp) = channel::<Result<f32, TcpCommunicationError>>(10, false);

        let handle = std::thread::spawn(move || {
            let _ = rx_cmd.blocking_recv();
            drop(tx_resp);
        });

        let res_recv_err = TestRequester::get_response_from_simulator_blocking(
            "test_loc".to_string(),
            &mut tx_cmd,
            &mut rx_resp,
            InternalCommand::RequestSignal(AquariumSignal::WaterTemperature),
        );
        assert!(matches!(
            res_recv_err,
            Err(TcpCommunicationError::ReceivingFromTCPThreadFailed { .. })
        ));
        handle.join().unwrap();

        // 3. Inner payload error propagation
        let (mut tx_cmd, mut rx_cmd) = channel::<InternalCommand>(10, false);
        let (mut tx_resp, mut rx_resp) = channel::<Result<f32, TcpCommunicationError>>(10, false);

        let handle = std::thread::spawn(move || {
            let _ = rx_cmd.blocking_recv();
            tx_resp
                .send(Err(TcpCommunicationError::ResponseContainsNoWords(
                    "mock_loc".to_string(),
                )))
                .unwrap();
        });

        let res_payload_err = TestRequester::get_response_from_simulator_blocking(
            "test_loc".to_string(),
            &mut tx_cmd,
            &mut rx_resp,
            InternalCommand::RequestSignal(AquariumSignal::WaterTemperature),
        );
        assert!(matches!(
            res_payload_err,
            Err(TcpCommunicationError::ResponseContainsNoWords(_))
        ));
        handle.join().unwrap();
    }
}
