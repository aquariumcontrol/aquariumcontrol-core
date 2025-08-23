/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(all(not(test), target_os = "linux"))]
use log::info;
use log::{debug, error, warn};

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::fmt;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::{
    io::{prelude::*, BufReader},
    net::TcpStream,
    time::Duration,
};

pub(crate) use crate::simulator::tcp_communication_error::TcpCommunicationError;
use crate::simulator::tcp_data::*;
use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
#[cfg(all(not(test), target_os = "linux"))]
use nix::unistd::gettid;

/// Holds the configuration data for the TCP connection.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct TcpConnectionConfig {
    /// host address (either IP address or DNS entry)
    ip_address: String,

    /// numeric port number
    port: u16,
}

/// Struct collects the channels for communication with other threads.
pub struct TcpCommunicationChannels {
    /// Receiver part of the channel for communication from the relay manager wrapped in Option type
    pub rx_tcp_communication_from_relay_manager_opt: Option<Receiver<InternalCommand>>,

    /// Receiver part of the channel for communication from the sensor manager wrapped in Option type
    pub rx_tcp_communication_from_sensor_manager_opt: Option<Receiver<InternalCommand>>,

    /// Sender part of the channel for communication to sensor manager wrapped in Option type
    pub tx_tcp_communication_to_sensor_manager_opt:
        Option<Sender<Result<f32, TcpCommunicationError>>>,

    /// Receiver part of the channel for communication from tank level switch wrapped in Option type
    pub rx_tcp_communication_from_tank_level_switch_opt: Option<Receiver<InternalCommand>>,

    /// Sender part of the channel for communication to tank level switch wrapped in Option type       
    pub tx_tcp_communication_to_tank_level_switch_opt:
        Option<Sender<Result<f32, TcpCommunicationError>>>,

    /// Sender part of the channel for communication to the signal handler       
    pub tx_tcp_communication_to_signal_handler: Sender<bool>,

    /// Receiver part of the channel for communication from the signal handler       
    pub rx_tcp_communication_from_signal_handler: Receiver<InternalCommand>,
}

impl fmt::Display for TcpCommunicationChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "TcpCommunicationChannels Status:")?;

        writeln!(
            f,
            "- rx_tcp_communication_from_relay_manager_opt: {}",
            if self.rx_tcp_communication_from_relay_manager_opt.is_some() {
                "Some"
            } else {
                "None"
            }
        )?;

        writeln!(
            f,
            "- rx_tcp_communication_from_sensor_manager_opt: {}",
            if self.rx_tcp_communication_from_sensor_manager_opt.is_some() {
                "Some"
            } else {
                "None"
            }
        )?;

        writeln!(
            f,
            "- tx_tcp_communication_to_sensor_manager_opt: {}",
            if self.tx_tcp_communication_to_sensor_manager_opt.is_some() {
                "Some"
            } else {
                "None"
            }
        )?;

        writeln!(
            f,
            "- rx_tcp_communication_from_tank_level_switch_opt: {}",
            if self
                .rx_tcp_communication_from_tank_level_switch_opt
                .is_some()
            {
                "Some"
            } else {
                "None"
            }
        )?;

        writeln!(
            f,
            "- tx_tcp_communication_to_tank_level_switch_opt: {}",
            if self.tx_tcp_communication_to_tank_level_switch_opt.is_some() {
                "Some"
            } else {
                "None"
            }
        )?;

        writeln!(f, "- tx_tcp_communication_to_signal_handler: Present")?;

        writeln!(f, "- rx_tcp_communication_from_signal_handler: Present")
    }
}

/// A type alias for a sender/receiver pair used in TCP communication.
///
/// This channel is used to send sensor values (`f32`) or communication errors
/// between a module and the TCP simulator thread.
pub type TcpCommunicationChannelPair = (
    Sender<Result<f32, TcpCommunicationError>>,
    Receiver<Result<f32, TcpCommunicationError>>,
);

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
        tcp_connection: &TcpConnectionConfig,
    ) -> Result<TcpCommunication, TcpCommunicationError> {
        let stream = TcpStream::connect(
            tcp_connection.ip_address.clone() + ":" + &tcp_connection.port.to_string(),
        )
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
        })
    }

    /// Extracts the last word from a given string slice, using a space as the delimiter.
    ///
    /// This private helper function is useful for parsing responses where the desired
    /// data (e.g., a numeric value) is consistently the final token in a space-separated sequence.
    ///
    /// # Arguments
    /// * `sequence` - The string slice from which to extract the last word.
    ///
    /// # Returns
    /// An `Option<&str>`:
    /// - `Some(&str)` containing a reference to the last word if the string contains words.
    /// - `None` if the input string is empty or contains only whitespace.
    fn get_last_word(sequence: &str) -> Option<&str> {
        let words = sequence.split(" ").collect::<Vec<&str>>();
        words.last().copied()
    }

    /// Trims a single trailing newline (`\n`) and an optional preceding carriage return (`\r`) from a `String`.
    ///
    /// This private helper function modifies the input string in place, removing common
    /// line ending characters. It is particularly useful when reading lines from streams
    /// where `\n` or `\r\n` terminators might be present and need to be cleaned up.
    ///
    /// The implementation is adapted from a common pattern:
    /// Code taken over from <https://stackoverflow.com/questions/37888042/remove-single-trailing-newline-from-string-without-cloning>
    ///
    /// # Arguments
    /// * `s` - A mutable reference to the `String` from which trailing newline characters should be removed.
    fn trim_newline(s: &mut String) {
        if s.ends_with('\n') {
            s.pop();
            if s.ends_with('\r') {
                s.pop();
            }
        }
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
        match self.write_stream.write(request.as_bytes()) {
            Ok(amount_of_bytes_written) => {
                if amount_of_bytes_written != request.len() {
                    warn!(
                        "{}: Writing to TCP stream failed - (amount of bytes written ({}) differs from expected ({}))",
                        module_path!(),
                        amount_of_bytes_written,
                        request.len(),
                    );
                }
            }
            Err(e) => {
                return Err(TcpCommunicationError::WritingToStreamFailed {
                    location: module_path!().to_string(),
                    source: e,
                });
            }
        };
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
                if last_word.is_empty() {
                    Err(TcpCommunicationError::LastWordOfResponseEmpty(
                        module_path!().to_string(),
                    ))
                } else {
                    let mut last_word_string = last_word.to_string();
                    Self::trim_newline(&mut last_word_string);
                    last_word_string.parse::<f32>().map_err(|e| {
                        TcpCommunicationError::ResponseConversionError {
                            location: module_path!().to_string(),
                            last_word: last_word_string,
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
    fn receive_and_answer(
        &mut self,
        rx: &Receiver<InternalCommand>,
        tx: &Sender<Result<f32, TcpCommunicationError>>,
    ) -> Result<(), TcpCommunicationError> {
        let response_to_calling_thread: Result<f32, TcpCommunicationError>;
        match rx.try_recv() {
            Ok(c) => {
                match c {
                    InternalCommand::RequestSignal(s) => {
                        let simulator_response = self.get_signal(s);
                        if simulator_response.is_err() {
                            response_to_calling_thread =
                                Err(TcpCommunicationError::InvalidResponseFromSimulator(
                                    module_path!().to_string(),
                                ));
                        } else {
                            response_to_calling_thread = simulator_response;
                        }
                        match tx.send(response_to_calling_thread) {
                            Ok(()) => Ok(()),
                            Err(_) => Err(TcpCommunicationError::SendingResponseViaChannelFailure(
                                module_path!().to_string(),
                            )),
                        }
                    }
                    _ => {
                        Ok(()) /* Do nothing for all other commands */
                    }
                }
            }
            Err(e) => match e {
                TryRecvError::Empty => Ok(()),
                TryRecvError::Disconnected => Err(TcpCommunicationError::ChannelDisconnected(
                    module_path!().to_string(),
                )),
            },
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
    /// * `tcp_communication_channels` - Channels for communication with other threads,
    ///   including senders/receivers for Ambient, Atlas Scientific, Tank Level Switch,
    ///   Relay Manager, and the Signal Handler.
    pub fn execute(&mut self, tcp_communication_channels: TcpCommunicationChannels) {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let spin_sleeper = SpinSleeper::default();
        let sleep_interval_millis = 10;
        let sleep_duration_ten_milli_sec = Duration::from_millis(sleep_interval_millis);
        let mut lock_relay_manager_channel_disconnected = false;

        println!("{tcp_communication_channels}");

        loop {
            let (quit_command_received, _, _) = self.process_external_request(
                &tcp_communication_channels.rx_tcp_communication_from_signal_handler,
                None,
            );
            if quit_command_received {
                break;
            }

            // *** communication with Ambient ***
            if let Some(ref rx_tcp_communication_from_ambient) =
                tcp_communication_channels.rx_tcp_communication_from_sensor_manager_opt
            {
                if let Some(ref tx_tcp_communication_to_ambient) =
                    tcp_communication_channels.tx_tcp_communication_to_sensor_manager_opt
                {
                    if let Err(e) = self.receive_and_answer(
                        rx_tcp_communication_from_ambient,
                        tx_tcp_communication_to_ambient,
                    ) {
                        log_error_chain(
                            module_path!(),
                            "Error in communication with simulator for ambient signals",
                            e,
                        );
                    }
                }
            }

            // *** communication with Tank level switch ***
            if let Some(ref rx_tcp_communication_from_tank_level_switch) =
                tcp_communication_channels.rx_tcp_communication_from_tank_level_switch_opt
            {
                if let Some(ref tx_tcp_communication_to_tank_level_switch) =
                    tcp_communication_channels.tx_tcp_communication_to_tank_level_switch_opt
                {
                    if let Err(e) = self.receive_and_answer(
                        rx_tcp_communication_from_tank_level_switch,
                        tx_tcp_communication_to_tank_level_switch,
                    ) {
                        log_error_chain(
                            module_path!(),
                            "Error in communication with simulator for ambient signals",
                            e,
                        );
                    }
                }
            }

            // *** communication with relay manager - no response required ***
            if !lock_relay_manager_channel_disconnected {
                match tcp_communication_channels.rx_tcp_communication_from_relay_manager_opt {
                    Some(ref rx_tcp_communication_from_relay_manager) => {
                        match rx_tcp_communication_from_relay_manager.try_recv() {
                            Ok(c) => match c {
                                InternalCommand::SetRelay(c) => {
                                    let response = self.change_relay(c, true);
                                    if let Err(response_err) = response {
                                        log_error_chain(
                                            module_path!(),
                                            "Error in communication with relay manager",
                                            response_err,
                                        );
                                    }
                                }
                                InternalCommand::UnsetRelay(c) => {
                                    let response = self.change_relay(c, false);
                                    if let Err(response_err) = response {
                                        log_error_chain(
                                            module_path!(),
                                            "Error in communication with relay manager",
                                            response_err,
                                        );
                                    }
                                }
                                _ => { /* ignore all other commands */ }
                            },
                            Err(e) => {
                                match e {
                                    TryRecvError::Empty => { /* empty buffer - no action required */
                                    }
                                    TryRecvError::Disconnected => {
                                        lock_relay_manager_channel_disconnected = true;
                                        error!(
                                            target: module_path!(),
                                            "Error in trying to receive command from relay manager: {e:?}"
                                        );
                                    }
                                }
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

        // The application received request to terminate. That is why the loop was left.
        // Answer to the thread which sent the request for termination, so that shutdown can proceed further.
        match tcp_communication_channels
            .tx_tcp_communication_to_signal_handler
            .send(true)
        {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: module_path!(),
                    "sending confirmation back to signal handler failed ({e:?})"
                );
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn test_tcp_channels_display_all_some() {
        let (_, rx1) = channel();
        let (_, rx2) = channel();
        let (tx3, _) = channel();
        let (_, rx4) = channel();
        let (tx5, _) = channel();
        let (tx6, _) = channel();
        let (_, rx7) = channel();

        let channels = TcpCommunicationChannels {
            rx_tcp_communication_from_relay_manager_opt: Some(rx1),
            rx_tcp_communication_from_sensor_manager_opt: Some(rx2),
            tx_tcp_communication_to_sensor_manager_opt: Some(tx3),
            rx_tcp_communication_from_tank_level_switch_opt: Some(rx4),
            tx_tcp_communication_to_tank_level_switch_opt: Some(tx5),
            tx_tcp_communication_to_signal_handler: tx6,
            rx_tcp_communication_from_signal_handler: rx7,
        };

        println!("Output for TcpCommunicationChannels: {}", channels);

        let expected_output = "\
TcpCommunicationChannels Status:
- rx_tcp_communication_from_relay_manager_opt: Some
- rx_tcp_communication_from_sensor_manager_opt: Some
- tx_tcp_communication_to_sensor_manager_opt: Some
- rx_tcp_communication_from_tank_level_switch_opt: Some
- tx_tcp_communication_to_tank_level_switch_opt: Some
- tx_tcp_communication_to_signal_handler: Present
- rx_tcp_communication_from_signal_handler: Present
";
        assert_eq!(format!("{}", channels), expected_output);
    }

    #[test]
    fn test_tcp_channels_display_some_none_mix() {
        let (_, rx1) = channel();
        let (tx6, _) = channel();
        let (_, rx7) = channel();

        let channels = TcpCommunicationChannels {
            rx_tcp_communication_from_relay_manager_opt: None, // None
            rx_tcp_communication_from_sensor_manager_opt: Some(rx1), // Some
            tx_tcp_communication_to_sensor_manager_opt: None,  // None
            rx_tcp_communication_from_tank_level_switch_opt: Some(channel().1), // Some (new channel)
            tx_tcp_communication_to_tank_level_switch_opt: None,                // None
            tx_tcp_communication_to_signal_handler: tx6,
            rx_tcp_communication_from_signal_handler: rx7,
        };

        println!("Output for TcpCommunicationChannels: {}", channels);

        let expected_output = "\
TcpCommunicationChannels Status:
- rx_tcp_communication_from_relay_manager_opt: None
- rx_tcp_communication_from_sensor_manager_opt: Some
- tx_tcp_communication_to_sensor_manager_opt: None
- rx_tcp_communication_from_tank_level_switch_opt: Some
- tx_tcp_communication_to_tank_level_switch_opt: None
- tx_tcp_communication_to_signal_handler: Present
- rx_tcp_communication_from_signal_handler: Present
";
        assert_eq!(format!("{}", channels), expected_output);
    }
}
