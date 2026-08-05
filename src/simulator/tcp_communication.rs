/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A dedicated thread that acts as a proxy between the application and an external TCP simulator.
//!
//! This module implements the `TcpCommunication` struct, which runs as a long-running
//! thread. Its sole purpose is to manage all communication with a TCP-based hardware
//! simulator. It listens for commands from various application threads (like `SensorManager`,
//! `RelayManager`, etc.) via `mpsc` channels, translates them into a specific text-based
//! protocol, sends them to the simulator, and then parses the responses to send back
//! to the original requester.
//!
//! This architecture is fundamental to the project's testability, as it allows the entire
//! application to run and be tested on a developer's machine without any physical hardware.
//!
//! ## Key Components
//!
//! - **`TcpCommunication` Struct**: The core state machine for the thread. It holds the
//!   TCP stream for reading and writing, and configuration details.
//!
//! - **`new()` Method**: Establishes the initial TCP connection to the simulator's IP
//!   address and port, returning an error if the simulator is not available.
//!
//! - **`execute()` Method**: The main thread loop. It continuously polls multiple
//!   `mpsc` channels for incoming requests from other threads. It also listens for a
//!   graceful shutdown command from the `SignalHandler`.
//!
//! - **`handle_simulated_sensor()`**: A helper function that abstracts the non-blocking,
//!   request-response cycle for sensor-like modules (`SensorManager`, `TankLevelSwitch`).
//!
//! - **`get_signal()` & `change_relay()`**: Private methods that implement the specific
//!   text-based protocol for querying a sensor value or changing a relay state, respectively.
//!
//! ## Design and Architecture
//!
//! - **Centralized Simulator Proxy**: All simulator-bound traffic is funneled through this
//!   single thread. This decouples the core logic of other modules from the details of
//!   TCP communication and the simulator's specific protocol. Client modules only need
//!   to know how to send a command on a channel.
//!
//! - **Asynchronous Service Model**: The `execute()` loop uses non-blocking `try_recv()`
//!   calls to poll each client channel. This allows it to service multiple clients
//!   concurrently in a single-threaded, event-driven manner without getting stuck waiting
//!   for a single client.
//!
//! - **Text-Based Protocol Implementation**: This module is the single source of truth for
//!   the simulator's communication protocol. If the simulator's protocol changes, only this
//!   module needs to be updated.

#[cfg(all(not(test), target_os = "linux"))] // only required in real-life execution
use log::info;
use log::{debug, error};

use spin_sleep::SpinSleeper;
use std::{
    io::{prelude::*, BufReader},
    net::TcpStream,
    time::Duration,
};

use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::launch::execution_config::ExecutionConfig;
use crate::simulator::tcp_communication_channels::TcpCommunicationChannels;
use crate::simulator::tcp_communication_config::TcpCommunicationConfig;
pub(crate) use crate::simulator::tcp_communication_error::TcpCommunicationError;
use crate::simulator::tcp_data::*;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
#[cfg(all(not(test), not(feature = "test-utils")))]
#[cfg(not(tarpaulin_include))]
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
#[cfg(all(not(test), target_os = "linux"))] // only required in real-life execution
use nix::unistd::gettid;

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains configuration of implementation for opening, reading, and closing TCP channel.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     relay_manager[Relay Manager] --> tcp_communication[TCP Communication]
///     sensor_manager[Sensor Manager] --> tcp_communication
///     tcp_communication --> sensor_manager
///     tank_level_switch[Tank Level Switch] --> tcp_communication
///     tcp_communication --> tank_level_switch
///     tcp_communication --> signal_handler[Signal Handler]
///     signal_handler --> tcp_communication
/// ```
pub struct TcpCommunication {
    /// the raw TCP stream
    write_stream: TcpStream,

    /// buffered reader for TCP stream
    buf_reader: BufReader<TcpStream>,

    /// information about which threads are started
    execution_config: ExecutionConfig,
}

impl TcpCommunication {
    /// Creates a new `TcpCommunication` instance and establishes a TCP connection to a simulator.
    ///
    /// This constructor attempts to connect to a TCP server (acting as a simulator)
    /// at the specified IP address and port. Upon a successful connection, it prepares
    /// both a `TcpStream` for writing and a `BufReader` for efficient reading from the stream.
    ///
    /// # Arguments
    /// * `tcp_connection` - Configuration data for the TCP connection, including the
    ///   target `ip_address` and `port` of the simulator.
    /// * `execution_config` - Information about which threads are started
    ///
    /// # Returns
    /// A `Result` containing a new `TcpCommunication` struct with an established
    /// TCP connection on success.
    ///
    /// # Errors
    /// Returns a `TcpCommunicationError` if the connection or setup fails:
    /// - `TcpCommunicationError::ConnectionFailed`: If the initial TCP connection to the
    ///   specified address and port cannot be established. This could be due to the
    ///   simulator not running, a firewall blocking the connection, or an incorrect address/port.
    /// - `TcpCommunicationError::StreamCloneFailed`: If the underlying `TcpStream` cannot
    ///   be cloned, which is a necessary step for creating separate read and write handles.
    pub fn new(
        tcp_connection: &TcpCommunicationConfig,
        execution_config: ExecutionConfig,
    ) -> Result<TcpCommunication, TcpCommunicationError> {
        let stream = TcpStream::connect((tcp_connection.ip_address.as_str(), tcp_connection.port))
            .map_err(|e| TcpCommunicationError::ConnectionFailed {
                location: module_path!().to_string(),
                ip_address: tcp_connection.ip_address.clone(),
                port: tcp_connection.port,
                source: e,
            })?;

        let write_stream =
            stream
                .try_clone()
                .map_err(|e| TcpCommunicationError::StreamCloneFailed {
                    location: module_path!().to_string(),
                    source: e,
                })?;

        Ok(TcpCommunication {
            write_stream,
            buf_reader: BufReader::new(stream),
            execution_config,
        })
    }

    /// Extracts the last word from a given string slice, using whitespace as the delimiter.
    ///
    /// This private helper function is useful for parsing responses where the desired
    /// data (e.g., a numeric value) is consistently the final token in a space-separated sequence.
    /// This implementation avoids heap allocation by using a reverse iterator.
    ///
    /// # Arguments
    /// * `sequence` - The string slice from which to extract the last word.
    ///
    /// # Returns
    /// An `Option<&str>`:
    /// - `Some(&str)` containing a reference to the last word if the string contains words.
    /// - `None` if the input string is empty or contains only whitespace.
    fn get_last_word(sequence: &str) -> Option<&str> {
        sequence.split_whitespace().last()
    }

    /// Writes a string request to the TCP stream and handles potential errors.
    ///
    /// This private helper function sends a `request` string as bytes over the TCP stream.
    /// It verifies that the entire request was written successfully and flushes the stream
    /// to ensure the data is sent immediately.
    ///
    /// # Arguments
    /// * `request` - The `String` containing the data to be written to the TCP stream.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) on a successful write-operation and flush.
    ///
    /// # Errors
    /// Returns a `TcpCommunicationError` if the write or flush operation fails:
    /// - `TcpCommunicationError::WritingToStreamFailed`: If the underlying OS returns an
    ///   error while writing to the TCP socket. This usually indicates a connection issue.
    /// - `TcpCommunicationError::FlushFailed`: If the OS cannot flush its write buffer
    ///   to the socket, also indicating a connection problem.
    fn write_stream_check_for_error(
        &mut self,
        request: String,
    ) -> Result<(), TcpCommunicationError> {
        self.write_stream
            .write_all(request.as_bytes())
            .map_err(|e| TcpCommunicationError::WritingToStreamFailed {
                location: module_path!().to_string(),
                source: e,
            })?;
        self.write_stream
            .flush()
            .map_err(|e| TcpCommunicationError::FlushFailed {
                location: module_path!().to_string(),
                source: e,
            })
    }

    /// Communicates with the simulator to change the state of a specific relay.
    ///
    /// This private helper function constructs a TCP message to command a relay
    /// to either switch `ON` or `OFF`. It then writes this command to the TCP stream
    /// and waits for a response from the simulator to confirm the command was received.
    ///
    /// # Arguments
    /// * `relay_id` - The numeric ID of the relay whose state is to be changed.
    /// * `state` - A boolean value indicating the desired state: `true` for ON, `false` for OFF.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the command was sent and a response was received successfully.
    ///
    /// # Errors
    /// Returns a `TcpCommunicationError` if any part of the process fails:
    /// - `TcpCommunicationError::ReadingFromStreamFailed`: If a response line cannot be
    ///   read from the simulator, which likely indicates the connection was dropped.
    /// - It will also propagate any errors from `write_stream_check_for_error`.
    ///
    /// [impl->swreq~simulator_relay_actuation~1]
    fn change_relay(&mut self, relay_id: u16, state: bool) -> Result<(), TcpCommunicationError> {
        let command = if state {
            debug!(target: module_path!(), "switching on relay #{relay_id}");
            command::SETSINGLE
        } else {
            debug!(target: module_path!(), "switching off relay #{relay_id}");
            command::UNSETSINGLE
        };

        let request = format!("{} {} {}\n", category::CONTROLLINO, command, relay_id);

        self.write_stream_check_for_error(request)?;

        let mut response = String::new();
        self.buf_reader.read_line(&mut response).map_err(|e| {
            TcpCommunicationError::ReadingFromStreamFailed {
                location: module_path!().to_string(),
                source: e,
            }
        })?;
        Ok(())
    }

    /// Communicates with the simulator to request and retrieve a sensor signal value.
    ///
    /// This private helper function constructs a TCP message to ask the simulator
    /// for the current reading of a specific `AquariumSignal`. It then writes this
    /// request to the TCP stream, waits for a response, and parses the received
    /// string into an `f32` value.
    ///
    /// # Arguments
    /// * `aquarium_signal` - The `AquariumSignal` enum variant describing the sensor
    ///   signal (e.g., water temperature, pH) for which the value is requested.
    ///
    /// # Returns
    /// A `Result` containing the retrieved sensor value as an `f32` on success.
    ///
    /// # Errors
    /// Returns a `TcpCommunicationError` if any part of the process fails:
    /// - `TcpCommunicationError::IllegalSignalRequestToSimulator`: If an unsupported
    ///   `AquariumSignal` is requested.
    /// - `TcpCommunicationError::ReadingFromStreamFailed`: If a response cannot be read
    ///   from the TCP stream.
    /// - `TcpCommunicationError::ResponseContainsNoWords`: If the simulator's response
    ///   is empty or contains no parsable words.
    /// - `TcpCommunicationError::LastWordOfResponseEmpty`: If the last word of the response
    ///   is an empty string.
    /// - `TcpCommunicationError::ResponseConversionError`: If the last word of the response
    ///   cannot be parsed into an `f32`.
    /// - It will also propagate any errors from `write_stream_check_for_error`.
    ///
    /// [impl->swreq~simulator_sensor_signals~1]
    fn get_signal(
        &mut self,
        aquarium_signal: AquariumSignal,
    ) -> Result<f32, TcpCommunicationError> {
        let (category, tcp_signal_identifier) = match aquarium_signal {
            AquariumSignal::WaterTemperature => (
                category::ATLASSCIENTIFIC,
                signal::ATLASSCIENTIFICTEMPERATURE,
            ),
            AquariumSignal::pH => (category::ATLASSCIENTIFIC, signal::ATLASSCIENTIFICPH),
            AquariumSignal::Conductivity => (
                category::ATLASSCIENTIFIC,
                signal::ATLASSCIENTIFICCONDUCTIVITY,
            ),
            AquariumSignal::AmbientTemperature => (category::DHT22, signal::AMBIENTTEMPERATURE),
            AquariumSignal::AmbientHumidity => (category::DHT22, signal::AMBIENTHUMIDITY),
            AquariumSignal::TankLevelSwitchPosition => {
                (category::TANKLEVELSWITCH, signal::TANKLEVELSWITCHPOSITION)
            }
            AquariumSignal::TankLevelSwitchInvalid => {
                (category::TANKLEVELSWITCH, signal::TANKLEVELSWITCHINVALID)
            }
            _ => {
                return Err(TcpCommunicationError::IllegalSignalRequestToSimulator(
                    module_path!().to_string(),
                    aquarium_signal,
                ));
            }
        };

        let request = format!("{} {} {}\n", category, command::READ, tcp_signal_identifier);
        self.write_stream_check_for_error(request)?;

        let mut response = String::new();
        self.buf_reader.read_line(&mut response).map_err(|e| {
            TcpCommunicationError::ReadingFromStreamFailed {
                location: module_path!().to_string(),
                source: e,
            }
        })?;

        // Flattened and more robust parsing logic
        Self::get_last_word(&response)
            .ok_or_else(|| {
                TcpCommunicationError::ResponseContainsNoWords(module_path!().to_string())
            })
            .and_then(|last_word| {
                let trimmed_word = last_word.trim();
                if trimmed_word.is_empty() {
                    Err(TcpCommunicationError::LastWordOfResponseEmpty(
                        module_path!().to_string(),
                    ))
                } else {
                    // Parse directly from the trimmed slice.
                    trimmed_word.parse::<f32>().map_err(|e| {
                        TcpCommunicationError::ResponseConversionError {
                            location: module_path!().to_string(),
                            last_word: trimmed_word.to_string(), // Only allocate string for the error case.
                            source: e,
                        }
                    })
                }
            })
    }

    /// Receives a `RequestSignal` command without blocking from a channel and sends back the corresponding sensor value via TCP.
    ///
    /// This private helper function is used to service requests from various modules
    /// that are configured to run in simulator mode. It checks the provided receiver channel (`rx`) for an
    /// `InternalCommand::RequestSignal`. If such a request is found, it uses `self.get_signal()`
    /// to fetch the simulated sensor value and sends it back through the provided sender channel (`tx`).
    /// Other command types are ignored.
    ///
    /// # Arguments
    /// * `rx` - A reference to the receiver channel from which `InternalCommand` requests are received.
    /// * `tx` - A reference to the sender channel through which the `f32` sensor response is sent back.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(f32)`: In the case of success
    /// - `Err(TcpCommunicationError:)`: in case of failure
    ///
    /// [impl->swreq~simulator_sensor_signals~1]
    fn receive_and_answer(
        &mut self,
        rx: &mut AquaReceiver<InternalCommand>,
        tx: &mut AquaSender<Result<f32, TcpCommunicationError>>,
    ) -> Result<(), TcpCommunicationError> {
        match rx.try_recv() {
            Ok(InternalCommand::RequestSignal(s)) => {
                // A signal was requested. Get it from the simulator.
                let response = self.get_signal(s);
                // Send the response (Ok or Err) back to the caller.
                tx.send(response).map_err(|_| {
                    TcpCommunicationError::SendingResponseViaChannelFailure(
                        module_path!().to_string(),
                    )
                })
            }
            Ok(_) => {
                // Any other command was received, which we can safely ignore.
                Ok(())
            }
            #[cfg(feature = "debug_channels")]
            Err(AquaChannelError::Full) => {
                // Not applicable. Do nothing.
                Ok(())
            }
            Err(AquaChannelError::Empty) => {
                // No command in the channel, which is a normal, non-error state.
                Ok(())
            }
            Err(AquaChannelError::Disconnected) => {
                // The channel is broken, which is a critical error.
                Err(TcpCommunicationError::ChannelDisconnected(
                    module_path!().to_string(),
                ))
            }
        }
    }

    /// Handles a non-blocking request-response cycle for a single simulated sensor module.
    ///
    /// This private helper function encapsulates the logic for servicing a client thread
    /// (like `SensorManager` or `TankLevelSwitch`) that is running in simulator mode.
    /// It is designed to be called repeatedly from the main `execute` loop for each potential client.
    ///
    /// The function first checks if the client module is configured to be active (`is_active`)
    /// and if its communication channels have been provided. If both are true, it calls
    /// `receive_and_answer` to process one potential request. If any communication error
    /// occurs during this process, it is logged with context, but the function does not
    /// panic or return an error, allowing the main loop to continue servicing other clients.
    ///
    /// # Arguments
    /// * `is_active` - A boolean flag indicating if the client module is configured to be running.
    ///   If `false`, the function does nothing.
    /// * `rx_opt` - A mutable `Option` containing the `AquaReceiver` for incoming `InternalCommand`s
    ///   from the client.
    /// * `tx_opt` - A mutable `Option` containing the `AquaSender` to send the `Result<f32, ...>`
    ///   response back to the client.
    /// * `module_name` - A string slice identifying the client module (e.g., "sensor manager")
    ///   for contextual error logging.
    fn handle_simulated_sensor(
        &mut self,
        is_active: bool,
        rx_opt: &mut Option<AquaReceiver<InternalCommand>>,
        tx_opt: &mut Option<AquaSender<Result<f32, TcpCommunicationError>>>,
        _module_name: &str,
    ) {
        if is_active {
            if let (Some(rx), Some(tx)) = (rx_opt, tx_opt) {
                if let Err(_e) = self.receive_and_answer(rx, tx) {
                    #[cfg(all(not(test), not(feature = "test-utils")))]
                    #[cfg(not(tarpaulin_include))]
                    log_error_chain(
                        // not required during testing
                        module_path!(),
                        &format!("Error in communication with {_module_name}"),
                        _e,
                    );
                }
            }
        }
    }

    /// Executes the main control loop for the TCP communication module.
    ///
    /// This function runs continuously as a server, processing incoming requests
    /// from various client threads (e.g., Ambient, Atlas Scientific, Tank Level Switch, Relay Manager).
    /// It dispatches these requests to the connected TCP simulator and sends back the responses.
    ///
    /// The loop remains active until a `Quit` command is received from the signal handler.
    /// Upon receiving `Quit`, it breaks out of the loop and sends a confirmation back to the signal handler.
    ///
    /// # Arguments
    /// * `tcp_communication_channels` - A mutable reference to a struct containing the
    ///   channels for communication with other threads,
    ///   including senders/receivers for SensorManager, Tank Level Switch,
    ///   Relay Manager, and the Signal Handler.
    ///
    /// [impl->swreq~simulator_sensor_signals~1]
    /// [impl->swreq~simulator_relay_actuation~1]
    pub fn execute(&mut self, tcp_communication_channels: &mut TcpCommunicationChannels) {
        #[cfg(all(target_os = "linux", not(test)))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        #[cfg(all(not(test), feature = "debug_channels"))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        tcp_communication_channels.report_status();

        let spin_sleeper = SpinSleeper::default();
        let sleep_interval_millis = 10;
        let sleep_duration_ten_milli_sec = Duration::from_millis(sleep_interval_millis);
        let mut lock_relay_manager_channel_disconnected = false;

        loop {
            let (quit_command_received, _, _) = self.process_external_request(
                &mut tcp_communication_channels.rx_tcp_communication_from_signal_handler,
                None,
            );
            if quit_command_received {
                break;
            }

            // *** communication with sensor manager ***
            self.handle_simulated_sensor(
                self.execution_config.sensor_manager,
                &mut tcp_communication_channels.rx_tcp_communication_from_sensor_manager_opt,
                &mut tcp_communication_channels.tx_tcp_communication_to_sensor_manager_opt,
                "sensor manager",
            );

            // *** communication with Tank level switch ***
            self.handle_simulated_sensor(
                self.execution_config.tank_level_switch,
                &mut tcp_communication_channels.rx_tcp_communication_from_tank_level_switch_opt,
                &mut tcp_communication_channels.tx_tcp_communication_to_tank_level_switch_opt,
                "tank level switch",
            );

            // *** communication with relay manager - no response required ***
            if !lock_relay_manager_channel_disconnected && self.execution_config.relay_manager {
                match tcp_communication_channels.rx_tcp_communication_from_relay_manager_opt {
                    Some(ref mut rx_tcp_communication_from_relay_manager) => {
                        match rx_tcp_communication_from_relay_manager.try_recv() {
                            Ok(c) => match c {
                                InternalCommand::SetRelay(c) => {
                                    let response = self.change_relay(c, true);
                                    if let Err(_response_err) = response {
                                        #[cfg(all(not(test), not(feature = "test-utils")))]
                                        #[cfg(not(tarpaulin_include))]
                                        log_error_chain(
                                            // not required for testing
                                            module_path!(),
                                            "Error in communication to relay manager",
                                            _response_err,
                                        );
                                    }
                                }
                                InternalCommand::UnsetRelay(c) => {
                                    let response = self.change_relay(c, false);
                                    if let Err(_response_err) = response {
                                        #[cfg(all(not(test), not(feature = "test-utils")))]
                                        #[cfg(not(tarpaulin_include))]
                                        log_error_chain(
                                            // not required for testing
                                            module_path!(),
                                            "Error in communication to relay manager",
                                            _response_err,
                                        );
                                    }
                                }
                                _ => { /* ignore all other commands */ }
                            },
                            #[cfg(feature = "debug_channels")]
                            Err(AquaChannelError::Full) => { /* Not applicable - do nothing */ }
                            Err(AquaChannelError::Empty) => { /* empty buffer - no action required */
                            }
                            Err(AquaChannelError::Disconnected) => {
                                lock_relay_manager_channel_disconnected = true;
                                error!(
                                    target: module_path!(),
                                    "Error in trying to receive command from relay manager"
                                );
                            }
                        }
                    }
                    None => {
                        // no value provided - do nothing
                    }
                }
            }
            // **********************************
            spin_sleeper.sleep(sleep_duration_ten_milli_sec);
        }

        tcp_communication_channels.acknowledge_signal_handler();
    }
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    // Verifies get_last_word extracts final space-separated token or returns None when empty.
    // [utest->swreq~simulator_sensor_signals~1]
    fn test_get_last_word() {
        assert_eq!(TcpCommunication::get_last_word("CONTROLLINO READ 1 25.5\n"), Some("25.5"));
        assert_eq!(TcpCommunication::get_last_word(""), None);
        assert_eq!(TcpCommunication::get_last_word("   \t\n"), None);
    }

    #[test]
    // Verifies TcpCommunication::new returns ConnectionFailed error when connecting to an invalid port.
    // [utest->swreq~simulator_sensor_signals~1]
    fn test_new_connection_failed() {
        let config = TcpCommunicationConfig {
            ip_address: "127.0.0.1".to_string(),
            port: 1, // invalid port
            execute: true,
        };
        let exec_config = ExecutionConfig::default();
        let res = TcpCommunication::new(&config, exec_config);
        assert!(matches!(res, Err(TcpCommunicationError::ConnectionFailed { .. })));
    }

    #[test]
    // Verifies get_signal queries sensor values over TCP and handles illegal signals and response errors.
    // [utest->swreq~simulator_sensor_signals~1]
    fn test_get_signal_success_and_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            // 1. WaterTemperature request
            reader.read_line(&mut line).unwrap();
            stream.write_all(b"ATLASSCIENTIFIC READ 1 25.5\n").unwrap();

            // 2. Empty response test
            line.clear();
            reader.read_line(&mut line).unwrap();
            stream.write_all(b"\n").unwrap();

            // 3. Unparsable response test
            line.clear();
            reader.read_line(&mut line).unwrap();
            stream.write_all(b"ATLASSCIENTIFIC READ 1 INVALID\n").unwrap();
        });

        let config = TcpCommunicationConfig {
            ip_address: "127.0.0.1".to_string(),
            port,
            execute: true,
        };
        let exec_config = ExecutionConfig::default();
        let mut tcp_comm = TcpCommunication::new(&config, exec_config).unwrap();

        // Test 1: valid signal
        let val = tcp_comm.get_signal(AquariumSignal::WaterTemperature).unwrap();
        assert_eq!(val, 25.5);

        // Test 2: illegal signal
        let err_illegal = tcp_comm.get_signal(AquariumSignal::HeatingControlStatus);
        assert!(matches!(err_illegal, Err(TcpCommunicationError::IllegalSignalRequestToSimulator(..))));

        // Test 3: empty response
        let err_empty = tcp_comm.get_signal(AquariumSignal::pH);
        assert!(matches!(err_empty, Err(TcpCommunicationError::ResponseContainsNoWords(..))));

        // Test 4: unparsable response
        let err_parse = tcp_comm.get_signal(AquariumSignal::Conductivity);
        assert!(matches!(err_parse, Err(TcpCommunicationError::ResponseConversionError { .. })));

        handle.join().unwrap();
    }

    #[test]
    // Verifies change_relay transmits SETSINGLE and UNSETSINGLE commands over TCP.
    // [utest->swreq~simulator_relay_actuation~1]
    fn test_change_relay_success() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();

            // 1. Relay ON
            reader.read_line(&mut line).unwrap();
            assert!(line.contains("SETSINGLE"));
            stream.write_all(b"CONTROLLINO SETSINGLE 1 OK\n").unwrap();

            // 2. Relay OFF
            line.clear();
            reader.read_line(&mut line).unwrap();
            assert!(line.contains("UNSETSINGLE"));
            stream.write_all(b"CONTROLLINO UNSETSINGLE 1 OK\n").unwrap();
        });

        let config = TcpCommunicationConfig {
            ip_address: "127.0.0.1".to_string(),
            port,
            execute: true,
        };
        let exec_config = ExecutionConfig::default();
        let mut tcp_comm = TcpCommunication::new(&config, exec_config).unwrap();

        assert!(tcp_comm.change_relay(1, true).is_ok());
        assert!(tcp_comm.change_relay(1, false).is_ok());

        handle.join().unwrap();
    }

    #[test]
    // Verifies receive_and_answer processes RequestSignal commands and returns sensor responses via channel.
    // [utest->swreq~simulator_sensor_signals~1]
    fn test_receive_and_answer() {
        use crate::launch::channels::channel;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            stream.write_all(b"DHT22 READ 1 22.5\n").unwrap();
        });

        let config = TcpCommunicationConfig {
            ip_address: "127.0.0.1".to_string(),
            port,
            execute: true,
        };
        let exec_config = ExecutionConfig::default();
        let mut tcp_comm = TcpCommunication::new(&config, exec_config).unwrap();

        let (mut tx_req, mut rx_req) = channel::<InternalCommand>(10, false);
        let (mut tx_resp, mut rx_resp) = channel::<Result<f32, TcpCommunicationError>>(10, false);

        tx_req.send(InternalCommand::RequestSignal(AquariumSignal::AmbientTemperature)).unwrap();
        let res = tcp_comm.receive_and_answer(&mut rx_req, &mut tx_resp);
        assert!(res.is_ok());

        let resp_val = rx_resp.try_recv().unwrap().unwrap();
        assert_eq!(resp_val, 22.5);

        handle.join().unwrap();
    }

    #[test]
    // Verifies execute loop services sensor and relay channels and shuts down cleanly on Quit command.
    // [utest->swreq~simulator_sensor_signals~1]
    // [utest->swreq~simulator_relay_actuation~1]
    fn test_execute_loop_success_and_shutdown() {
        use crate::launch::channels::channel;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let mock_server_handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();

            // 1. Sensor manager request: WaterTemperature
            reader.read_line(&mut line).unwrap();
            stream.write_all(b"ATLASSCIENTIFIC READ 1 24.8\n").unwrap();

            // 2. Tank level switch request: TankLevelSwitchPosition
            line.clear();
            reader.read_line(&mut line).unwrap();
            stream.write_all(b"TANKLEVELSWITCH READ 1 1.0\n").unwrap();

            // 3. Relay manager request: SetRelay(1)
            line.clear();
            reader.read_line(&mut line).unwrap();
            stream.write_all(b"CONTROLLINO SETSINGLE 1 OK\n").unwrap();
        });

        let config = TcpCommunicationConfig {
            ip_address: "127.0.0.1".to_string(),
            port,
            execute: true,
        };
        let exec_config = ExecutionConfig {
            sensor_manager: true,
            tank_level_switch: true,
            relay_manager: true,
            ..ExecutionConfig::default()
        };
        let mut tcp_comm = TcpCommunication::new(&config, exec_config).unwrap();

        let (mut tx_sm_req, rx_sm_req) = channel::<InternalCommand>(10, false);
        let (tx_sm_resp, mut rx_sm_resp) = channel::<Result<f32, TcpCommunicationError>>(10, false);

        let (mut tx_tls_req, rx_tls_req) = channel::<InternalCommand>(10, false);
        let (tx_tls_resp, mut rx_tls_resp) = channel::<Result<f32, TcpCommunicationError>>(10, false);

        let (mut tx_rm_req, rx_rm_req) = channel::<InternalCommand>(10, false);

        let (mut tx_sig_cmd, rx_sig_cmd) = channel::<InternalCommand>(10, false);
        let (tx_sig_ack, mut rx_sig_ack) = channel::<bool>(10, false);

        let mut channels = TcpCommunicationChannels {
            tx_tcp_communication_to_sensor_manager_opt: Some(tx_sm_resp),
            rx_tcp_communication_from_sensor_manager_opt: Some(rx_sm_req),
            tx_tcp_communication_to_tank_level_switch_opt: Some(tx_tls_resp),
            rx_tcp_communication_from_tank_level_switch_opt: Some(rx_tls_req),
            rx_tcp_communication_from_relay_manager_opt: Some(rx_rm_req),
            tx_tcp_communication_to_signal_handler: tx_sig_ack,
            rx_tcp_communication_from_signal_handler: rx_sig_cmd,
        };

        // Spawn execute in thread
        let tcp_comm_handle = thread::spawn(move || {
            tcp_comm.execute(&mut channels);
        });

        // Send requests
        tx_sm_req.send(InternalCommand::RequestSignal(AquariumSignal::WaterTemperature)).unwrap();
        tx_tls_req.send(InternalCommand::RequestSignal(AquariumSignal::TankLevelSwitchPosition)).unwrap();
        tx_rm_req.send(InternalCommand::SetRelay(1)).unwrap();

        // Verify sensor responses
        let sm_res = rx_sm_resp.blocking_recv().unwrap().unwrap();
        assert_eq!(sm_res, 24.8);

        let tls_res = rx_tls_resp.blocking_recv().unwrap().unwrap();
        assert_eq!(tls_res, 1.0);

        // Send Quit signal to terminate loop
        tx_sig_cmd.send(InternalCommand::Quit).unwrap();

        tcp_comm_handle.join().unwrap();
        mock_server_handle.join().unwrap();

        // Verify signal handler acknowledgment
        let ack = rx_sig_ack.blocking_recv().unwrap();
        assert!(ack);
    }
}



