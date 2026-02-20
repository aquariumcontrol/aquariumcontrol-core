/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the TCP simulator communication thread.
//!
//! This module contains the `TcpCommunicationConfig` struct, which is used with `serde`
//! to deserialize the `[tcp_communication]` section from the application's TOML
//! configuration file. It holds the network parameters required for the `TcpCommunication`
//! thread to connect to the external hardware simulator.

use serde_derive::Deserialize;

/// Holds the configuration data for the TCP connection.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct TcpCommunicationConfig {
    /// host address (either IP address or DNS entry)
    pub ip_address: String,

    /// numeric port number
    pub port: u16,

    /// indicates if the thread shall be started or not
    pub execute: bool,
}
