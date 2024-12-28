/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::{debug, error};
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::io;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::{
    io::{prelude::*, BufReader},
    net::TcpStream,
    time::Duration,
};

use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::simulator::tcp_data::{*};

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

/// Contains the error values for unsuccessful attempt of opening TCP channel.
#[derive(Debug)]
pub enum TcpCommunicationError {
    /// value indicates that error has occurred during TCP communication
    #[allow(unused)]
    ConnectionFailed(io::Error),
    //ReadMessageFailed(io::Error),
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains configuration of implementation for opening, reading and closing TCP channel.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     controllino[Controllino] --> tcp_communication[TCP communication]
///     gpio_handler[GPIO handler] --> tcp_communication
///     tcp_communication --> gpio_handler
///     tcp_communication --> signal_handler[Signal handler]
///     signal_handler --> tcp_communication
/// ```

pub struct TcpCommunication {
    /// the raw TCP stream
    write_stream: TcpStream,

    /// buffered reader for TCP stream
    buf_reader: BufReader<TcpStream>,
}

impl TcpCommunication {
    const LOG_TARGET_TCP_COMMUNICATION: &'static str = "tcp_communication";

    /// Provides wrapped into Result type either
    /// - a struct of type TcpCommunication after a successful attempt to open the TCP channel
    /// - or an error value from enum TcpCommunicationError
    /// # Arguments
    /// * `tcp_connection` - configuration data for TCP connection    
    pub fn new(
        tcp_connection: &TcpConnectionConfig,
    ) -> Result<TcpCommunication, TcpCommunicationError> {
        let stream = match TcpStream::connect(
            tcp_connection.ip_address.clone() + ":" + &tcp_connection.port.to_string(),
        ) {
            Ok(c) => c,
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_TCP_COMMUNICATION,
                    "connection to {} via port {} failed ({e:?})",
                    tcp_connection.ip_address, tcp_connection.port
                );
                return Err(TcpCommunicationError::ConnectionFailed(e));
            }
        };
        Ok(TcpCommunication {
            write_stream: stream.try_clone().unwrap(),
            buf_reader: BufReader::new(stream),
        })
    }

    /// Splits a string into multiple words using space as delimiter and returns the last word as type &str wrapped into type Option.
    /// # Arguments
    /// * `sequence` - reference to String which contains the sequence of words to be split
    fn get_last_word(sequence: &String) -> Option<&str> {
        let words = sequence.split(" ").collect::<Vec<&str>>();
        words.last().copied()
    }

    /// Removes trailing newline or carriage return.
    /// Code taken over from <https://stackoverflow.com/questions/37888042/remove-single-trailing-newline-from-string-without-cloning>
    /// # Arguments
    /// * `s` - reference to String which contains newline or carriage return.
    fn trim_newline(s: &mut String) {
        if s.ends_with('\n') {
            s.pop();
            if s.ends_with('\r') {
                s.pop();
            }
        }
    }

    /// Communicates with simulator to change relay state.
    /// # Arguments
    /// * `relay_id` - numeric id of relay for which the state shall be changed
    /// * `state` - value to be set: true represents On, false represents Off    
    fn change_relay(&mut self, relay_id: u16, state: bool) {
        let request: String;
        let relay_id_string = relay_id.to_string();
        if state {
            debug!(
                target: Self::LOG_TARGET_TCP_COMMUNICATION,
                "switching on relay #{}",
                relay_id
            );
            request = category::CONTROLLINO.to_string()
                + " "
                + command::SETSINGLE
                + " "
                + &relay_id_string
                + "\n";
        } else {
            debug!(
                target: Self::LOG_TARGET_TCP_COMMUNICATION,
                "switching off relay #{}",
                relay_id
            );
            request = category::CONTROLLINO.to_string()
                + " "
                + command::UNSETSINGLE
                + " "
                + &relay_id_string
                + "\n";
        }
        match self.write_stream.write(request.as_bytes()) {
            Ok(_) => {}
            Err(e) => {
                panic!(
                    "{}: Writing to TCP stream failed ({e:?})",
                    Self::LOG_TARGET_TCP_COMMUNICATION,
                );
            }
        };
        let _ = self.write_stream.flush();

        let mut response = String::new();
        response.clear();
        match self.buf_reader.read_line(&mut response) {
            Ok(_) => {}
            Err(e) => {
                panic!(
                    "{}: Reading from TCP stream failed ({e:?})",
                    Self::LOG_TARGET_TCP_COMMUNICATION,
                );
            }
        };
    }

    /// Communicates with simulator to get signal value.
    /// It returns the value as type f32.
    /// # Arguments
    /// * `aquarium_signal` - enum value of type AquariumSignal which describes the signal for which the value shall be retrieved.
    fn get_signal(&mut self, aquarium_signal: AquariumSignal) -> f32 {
        let category = match aquarium_signal {
            AquariumSignal::WaterTemperature => category::ATLASSCIENTIFIC,
            AquariumSignal::pH => category::ATLASSCIENTIFIC,
            AquariumSignal::Conductivity => category::ATLASSCIENTIFIC,
            AquariumSignal::AmbientTemperature => category::DHT22,
            AquariumSignal::AmbientHumidity => category::DHT22,
            AquariumSignal::TankLevelSwitchPosition => category::TANKLEVELSWITCH,
            AquariumSignal::TankLevelSwitchInvalid => category::TANKLEVELSWITCH,
            _ => {
                panic!(
                    "{}: Internal error - illegal signal request {} to simulator",
                    Self::LOG_TARGET_TCP_COMMUNICATION,
                    aquarium_signal
                );
            }
        };
        let tcp_signal_identifier = match aquarium_signal {
            AquariumSignal::WaterTemperature => signal::ATLASSCIENTIFICTEMPERATURE,
            AquariumSignal::pH => signal::ATLASSCIENTIFICPH,
            AquariumSignal::Conductivity => signal::ATLASSCIENTIFICCONDUCTIVITY,
            AquariumSignal::AmbientTemperature => signal::AMBIENTTEMPERATURE,
            AquariumSignal::AmbientHumidity => signal::AMBIENTHUMIDITY,
            AquariumSignal::TankLevelSwitchPosition => signal::TANKLEVELSWITCHPOSITION,
            AquariumSignal::TankLevelSwitchInvalid => signal::TANKLEVELSWITCHINVALID,
            _ => {
                panic!(
                    "{}: Internal error - illegal signal request {} to simulator",
                    Self::LOG_TARGET_TCP_COMMUNICATION,
                    aquarium_signal
                );
            }
        };
        let request =
            category.to_string() + " " + command::READ + " " + tcp_signal_identifier + "\n";

        match self.write_stream.write(request.as_bytes()) {
            Ok(_) => {}
            Err(e) => {
                panic!(
                    "{}: Writing to TCP stream failed ({e:?})",
                    Self::LOG_TARGET_TCP_COMMUNICATION,
                );
            }
        };
        let _ = self.write_stream.flush();

        let mut response = String::new();
        response.clear();
        match self.buf_reader.read_line(&mut response) {
            Ok(_) => {}
            Err(e) => {
                panic!(
                    "{}: Reading from TCP stream failed ({e:?})",
                    Self::LOG_TARGET_TCP_COMMUNICATION
                );
            }
        };
        match Self::get_last_word(&response) {
            Some(last_word) => {
                if !last_word.is_empty() {
                    let mut last_word_string = last_word.to_string();
                    Self::trim_newline(&mut last_word_string);
                    match last_word_string.parse::<f32>() {
                        Ok(c) => c,
                        Err(e) => {
                            panic!(
                                "{}: Failed to convert response ({last_word}) to f32: ({e:?})",
                                Self::LOG_TARGET_TCP_COMMUNICATION
                            );
                        }
                    }
                } else {
                    panic!(
                        "{}: Last word of response is empty.",
                        Self::LOG_TARGET_TCP_COMMUNICATION
                    );
                }
            }
            None => {
                panic!(
                    "{}: Response does not contain any words.",
                    Self::LOG_TARGET_TCP_COMMUNICATION
                );
            }
        }
    }

    fn receive_and_answer(&mut self, rx: &Receiver<InternalCommand>, tx: &Sender<f32>) {
        match rx.try_recv() {
            Ok(c) => {
                match c {
                    InternalCommand::RequestSignal(s) => {
                        let response_value = self.get_signal(s);
                        match tx.send(response_value) {
                            Ok(()) => { /* do nothing */ }
                            Err(e) => {
                                panic!(
                                    "{}: sending response value via channel failed ({e:?})",
                                    Self::LOG_TARGET_TCP_COMMUNICATION,
                                );
                            }
                        }
                    }
                    _ => { /* Do nothing for all other commands */ }
                }
            }
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        panic!(
                            "{}: Error in trying to receive command from data logger: {e:?}",
                            Self::LOG_TARGET_TCP_COMMUNICATION
                        );
                    }
                }
            }
        }
    }

    /// Executes the main control in a loop until signal handler commands quitting.
    /// # Arguments
    /// * `rx_from_controllino_opt` - Receiver part of channel for communication from Controllino wrapped in Option type
    /// * `rx_from_gpio_opt` - Receiver part of channel for communication from Controllino wrapped in Option type
    /// * `tx_to_gpio_opt` - Sender part of channel for communication to signal handler wrapped in Option type
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler  
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler       
    pub fn execute(
        &mut self,
        rx_from_controllino_opt: Option<Receiver<InternalCommand>>,
        rx_from_ambient_opt: Option<Receiver<InternalCommand>>,
        tx_to_ambient_opt: Option<Sender<f32>>,
        rx_from_atlas_scientific_opt: Option<Receiver<InternalCommand>>,
        tx_to_atlas_scientific_opt: Option<Sender<f32>>,
        rx_from_tank_level_switch_opt: Option<Receiver<InternalCommand>>,
        tx_to_tank_level_switch_opt: Option<Sender<f32>>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        let spin_sleeper = SpinSleeper::default();
        let sleep_interval_millis = 10;
        let sleep_duration_ten_milli_sec = Duration::from_millis(sleep_interval_millis);
        loop {
            let (quit_command_received, _, _) =
                self.process_external_request(&rx_from_signal_handler, None);
            if quit_command_received {
                break;
            }

            // *** communication with Ambient ***
            match rx_from_ambient_opt {
                Some(ref rx_from_ambient) => match tx_to_ambient_opt {
                    Some(ref tx_to_ambient) => {
                        self.receive_and_answer(&rx_from_ambient, &tx_to_ambient);
                    }
                    None => {}
                },
                None => {}
            }

            // *** communication with Atlas Scientific ***
            match rx_from_atlas_scientific_opt {
                Some(ref rx_from_atlas_scientific) => match tx_to_atlas_scientific_opt {
                    Some(ref tx_to_atlas_scientific) => {
                        self.receive_and_answer(&rx_from_atlas_scientific, &tx_to_atlas_scientific);
                    }
                    None => {}
                },
                None => {}
            }

            // *** communication with Tank level switch ***
            match rx_from_tank_level_switch_opt {
                Some(ref rx_from_tank_level_switch) => match tx_to_tank_level_switch_opt {
                    Some(ref tx_to_tank_level_switch) => {
                        self.receive_and_answer(
                            &rx_from_tank_level_switch,
                            &tx_to_tank_level_switch,
                        );
                    }
                    None => {}
                },
                None => {}
            }

            // *** communication with Controllino - no response required ***
            match rx_from_controllino_opt {
                Some(ref rx_from_controllino) => {
                    match rx_from_controllino.try_recv() {
                        Ok(c) => match c {
                            InternalCommand::SetRelay(c) => self.change_relay(c, true),
                            InternalCommand::UnsetRelay(c) => self.change_relay(c, false),
                            _ => { /* ignore all other commands */ }
                        },
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!(
                                        "TcpCommunication: Error in trying to receive command from data logger: {e:?}"
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
            // **********************************
            spin_sleeper.sleep(sleep_duration_ten_milli_sec);
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_TCP_COMMUNICATION,
                    "sending confirmation back to signal handler faild ({e:?})"
                );
            }
        };
    }
}
