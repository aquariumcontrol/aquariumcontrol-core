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
use std::io::{self, BufRead, BufReader, Write};
use std::net::{AddrParseError, Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::str::FromStr;
use std::sync::mpsc::Receiver;
use crate::test_server::tcp_connection_config::TcpConnectionConfig;

pub const QUIT_COMMAND: &str = "QUIT";

#[derive(Debug)]
pub enum TcpErr {
    BadAddr(AddrParseError),
    Parse(ParseErr),
    ConnectionFailed(io::Error),
    ReadMessageFailed(io::Error),
    WriteMessageFailed(io::Error),
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
    // Clone the stream for writing.
    let mut write_stream = stream.try_clone().map_err(TcpErr::ConnectionFailed)?;
    // Use the original stream for reading via BufReader.
    let mut buf_reader = BufReader::new(stream);
    let mut next_line = String::new();

    loop {
        next_line.clear();
        match buf_reader.read_line(&mut next_line) {
            Ok(0) => return Ok(false), // EOF
            Ok(_) => {}
            Err(e) => return Err(TcpErr::ReadMessageFailed(e)),
        }

        // Clean up string
        let request_string = next_line
            .trim()
            .trim_start_matches(char::from_u32(0).unwrap_or_default())
            .to_string();

        if request_string.is_empty() {
            // Just an empty line or whitespace
            continue;
        }

        if request_string == QUIT_COMMAND {
            return Ok(true);
        }

        println!("Received request: {}", request_string);

        match parse_sequence(request_string.clone(), tx, rx, controllino) {
            Ok(response) => {
                let response_string = match response {
                    Some(resp) => resp + "\n",
                    None => request_string + "\n",
                };

                write_stream
                    .write_all(response_string.as_bytes())
                    .map_err(TcpErr::WriteMessageFailed)?;
                write_stream.flush().map_err(TcpErr::WriteMessageFailed)?;
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
    let ipv4 = Ipv4Addr::from_str(&config_tcp_connection.ip_address).map_err(TcpErr::BadAddr)?;
    let address = SocketAddrV4::new(ipv4, config_tcp_connection.port);

    let listener = TcpListener::bind(address).map_err(TcpErr::ConnectionFailed)?;

    println!("Server listening on {}", address);

    // accept connections and process them serially
    let mut connection_count = 0;
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                connection_count += 1;
                println!("*** Accepting incoming connection #{}", connection_count);
                let res = handle_client(stream, &tx, &rx, controllino);
                match res {
                    Ok(should_quit) => {
                        println!(
                            "connection closed gracefully; {}quitting",
                            if should_quit { "" } else { "not " }
                        );
                        if should_quit {
                            return Ok(())
                        }
                    }
                    Err(e) => {
                        println!("Connection error: {:?}", e);
                        return Err(e);
                    }
                }
            }
            Err(e) => {
                return Err(TcpErr::ConnectionFailed(e));
            }
        }
    }
    Ok(())
}