/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::test_server::command_parser::{parse_sequence, ParseErr};
use crate::test_server::controllino_emulator::ControllinoEmulator;
use crate::test_server::logger::SenderLogger;

use crate::test_server::test_server_channel_content::TestServerInternalCommand;
use serde_derive::Deserialize;
use std::io;
use std::net::{AddrParseError, Ipv4Addr, SocketAddrV4};
use std::str::FromStr;
use std::{
    io::{prelude::*, BufReader},
    net::{TcpListener, TcpStream},
    sync::mpsc::Receiver,
};

pub const QUIT_COMMAND: &str = "QUIT";

#[derive(Deserialize)]
pub struct TcpConnectionConfig {
    pub ip_address: String,
    pub port: u16,
}

#[derive(Debug)]
pub enum TcpErr {
    BadAddr(AddrParseError),
    Parse(ParseErr),
    ConnectionFailed(io::Error),
    ReadMessageFailed(io::Error),
}

/// Function reads content of stream and forwards String data to parser. It also sends back String data to the stream if parser returns it.
/// Function returns a Result type with
/// - a bool as wrapped type for Ok case: Value is true when quit command is received.
/// - TcpErr as wrapped type for the error case.
///
/// # Arguments:
///
/// * `stream` - variable of type TcpStream
/// * `tx` - reference to channel to communicate to model environment
/// * `rx` - reference to channel to receive communication back from model environment
/// * `controllino` - reference to device model
fn handle_client(
    stream: TcpStream,
    tx: &SenderLogger<TestServerInternalCommand>,
    rx: &Receiver<f32>,
    controllino: &mut ControllinoEmulator,
) -> Result<bool, TcpErr> {
    let mut buf_reader = BufReader::new(&stream);
    let mut next_line = String::new();
    loop {
        next_line.clear();
        match buf_reader.read_line(&mut next_line) {
            Ok(_) => {}
            Err(e) => return Err(TcpErr::ReadMessageFailed(e)),
        }
        if next_line.is_empty() {
            return Ok(false);
        }
        if next_line.trim() == QUIT_COMMAND {
            return Ok(true);
        }

        let request_string = next_line
            .trim()
            .trim_start_matches(char::from_u32(0).unwrap())
            .to_string();
        let response_string_with_newline: String;

        println!("Received request: {}", request_string.clone());
        match parse_sequence(request_string.clone(), tx, rx, controllino) {
            Ok(response) => {
                // response is of type Option<String>
                match response {
                    None => {
                        response_string_with_newline = request_string.clone() + "\n";
                    }
                    Some(response_string) => {
                        response_string_with_newline = response_string + "\n";
                    }
                }
                let mut write_stream = stream.try_clone().unwrap();
                let _ = write_stream.write(response_string_with_newline.as_bytes());
                let _ = write_stream.flush();
            }
            Err(e) => {
                return Err(TcpErr::Parse(e));
            }
        }
    }
}

/// Function checks address configuration data, creates a TCP binding to listen on a dedicated port for incoming connection and processes them by forwarding the streams to another function.
/// Function returns a Result type with TcpErr as wrapped type for the error case.
///
/// # Arguments:
///
/// * `config_tcp_connection` - reference to configuration data read from .toml file
/// * `tx` - channel to communicate to model environment
/// * `rx` - channel to receive communication back from model environment
/// * `controllino` - reference to device model
pub fn tcp_connection(
    config_tcp_connection: &TcpConnectionConfig,
    tx: SenderLogger<TestServerInternalCommand>,
    rx: Receiver<f32>,
    controllino: &mut ControllinoEmulator,
) -> Result<(), TcpErr> {
    let ipv4 = match Ipv4Addr::from_str(&config_tcp_connection.ip_address) {
        Ok(v) => v,
        Err(e) => return Err(TcpErr::BadAddr(e)),
    };
    let address = SocketAddrV4::new(ipv4, config_tcp_connection.port);

    let listener = match TcpListener::bind(address) {
        Ok(l) => l,
        Err(_) => {
            panic!("TCP binding failed. address={}", address);
        }
    };

    // accept connections and process them serially
    let mut connection_count = 0;
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                connection_count += 1;
                println!("*** Accepting incoming connection #{}", connection_count);
                let res = handle_client(stream, &tx, &rx, controllino);
                if let Err(e) = res {
                    return Err(e);
                }
                let should_quit = res?;
                println!(
                    "connection closed gracefully; {}quitting",
                    if should_quit { "" } else { "not " }
                );
                if should_quit {
                    return Ok(());
                }
            }
            Err(e) => {
                return Err(TcpErr::ConnectionFailed(e));
            }
        }
    }
    Ok(())
}
