/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::simulator::tcp_communication::TcpCommunicationError;
use crate::utilities::channel_content::InternalCommand;
use std::fmt;
use std::sync::mpsc::{Receiver, RecvError, SendError, Sender};

pub struct SensorManagerChannels {
    /// Sender part of the channel for communication via TCP wrapped as optional
    pub tx_sensor_manager_to_tcp_opt: Option<Sender<InternalCommand>>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_sensor_manager_to_tcp_opt: u64,

    /// Receiver part of the channel for communication via TCP wrapped as optional
    pub rx_sensor_manager_from_tcp_opt: Option<Receiver<Result<f32, TcpCommunicationError>>>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_sensor_manager_from_tcp_opt: u64,

    /// Sender part of the channel for communication to the signal handler
    pub tx_sensor_manager_to_signal_handler: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_sensor_manager_to_signal_handler: u64,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_sensor_manager_from_signal_handler: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_sensor_manager_from_signal_handler: u64,
}

impl SensorManagerChannels {
    /// Sends a command to the TCP simulator if the channel exists.
    #[allow(unused)]
    pub fn send_to_tcp(
        &mut self,
        command: InternalCommand,
    ) -> Option<Result<(), SendError<InternalCommand>>> {
        self.tx_sensor_manager_to_tcp_opt.as_ref().map(|tx| {
            let result = tx.send(command);
            #[cfg(feature = "debug_channels")]
            if result.is_ok() {
                self.cnt_tx_sensor_manager_to_tcp_opt += 1;
            }
            result
        })
    }

    /// Receives a result from the TCP simulator if the channel exists.
    #[allow(unused)]
    pub fn receive_from_tcp(
        &mut self,
    ) -> Option<Result<Result<f32, TcpCommunicationError>, RecvError>> {
        self.rx_sensor_manager_from_tcp_opt.as_ref().map(|rx| {
            let result = rx.recv();
            #[cfg(feature = "debug_channels")]
            if result.is_ok() {
                self.cnt_rx_sensor_manager_from_tcp_opt += 1;
            }
            result
        })
    }

    /// Sends an acknowledgment to the signal handler.
    #[allow(unused)]
    pub fn send_to_signal_handler(&mut self, ack: bool) -> Result<(), SendError<bool>> {
        let result = self.tx_sensor_manager_to_signal_handler.send(ack);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_sensor_manager_to_signal_handler += 1;
        }
        result
    }

    /// Receives a command from the signal handler.
    #[allow(unused)]
    pub fn receive_from_signal_handler(&mut self) -> Result<InternalCommand, RecvError> {
        let result = self.rx_sensor_manager_from_signal_handler.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_sensor_manager_from_signal_handler += 1;
        }
        result
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for SensorManagerChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== SensorManagerChannels ===")?;
        writeln!(
            f,
            "cnt_tx_sensor_manager_to_tcp_opt: {}",
            self.cnt_tx_sensor_manager_to_tcp_opt
        )?;
        writeln!(
            f,
            "cnt_rx_sensor_manager_from_tcp_opt: {}",
            self.cnt_rx_sensor_manager_from_tcp_opt
        )?;
        writeln!(
            f,
            "cnt_tx_sensor_manager_to_signal_handler: {}",
            self.cnt_tx_sensor_manager_to_signal_handler
        )?;
        write!(
            f,
            "cnt_rx_sensor_manager_from_signal_handler: {}",
            self.cnt_rx_sensor_manager_from_signal_handler
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for SensorManagerChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
