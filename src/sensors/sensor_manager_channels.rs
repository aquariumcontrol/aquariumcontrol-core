/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::simulator::tcp_communication::TcpCommunicationError;
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

pub struct SensorManagerChannels {
    /// Sender part of the channel for communication via TCP wrapped as optional
    pub tx_sensor_manager_to_tcp_opt: Option<AquaSender<InternalCommand>>,

    /// Receiver part of the channel for communication via TCP wrapped as optional
    pub rx_sensor_manager_from_tcp_opt: Option<AquaReceiver<Result<f32, TcpCommunicationError>>>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_sensor_manager_to_signal_handler: AquaSender<bool>,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_sensor_manager_from_signal_handler: AquaReceiver<InternalCommand>,
}

impl SensorManagerChannels {
    /// Sends a command to the TCP simulator if the channel exists.
    #[allow(unused)]
    pub fn send_to_tcp(
        &mut self,
        command: InternalCommand,
    ) -> Option<Result<(), AquaChannelError>> {
        self.tx_sensor_manager_to_tcp_opt
            .as_mut()
            .map(|tx| tx.send(command))
    }

    /// Receives a result from the TCP simulator if the channel exists.
    #[allow(unused)]
    pub fn receive_from_tcp(
        &mut self,
    ) -> Option<Result<Result<f32, TcpCommunicationError>, AquaChannelError>> {
        self.rx_sensor_manager_from_tcp_opt
            .as_mut()
            .map(|rx| rx.recv())
    }

    /// Sends an acknowledgment to the signal handler.
    #[allow(unused)]
    pub fn send_to_signal_handler(&mut self, ack: bool) -> Result<(), AquaChannelError> {
        self.tx_sensor_manager_to_signal_handler.send(ack)
    }

    /// Receives a command from the signal handler.
    #[allow(unused)]
    pub fn receive_from_signal_handler(&mut self) -> Result<InternalCommand, AquaChannelError> {
        self.rx_sensor_manager_from_signal_handler.recv()
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for SensorManagerChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== SensorManagerChannels ===")?;
        if self.tx_sensor_manager_to_tcp_opt.is_some() {
            writeln!(
                f,
                "tx_sensor_manager_to_tcp_opt: {}",
                self.tx_sensor_manager_to_tcp_opt.as_ref().unwrap().count
            )?;
        }
        if self.rx_sensor_manager_from_tcp_opt.is_some() {
            writeln!(
                f,
                "rx_sensor_manager_from_tcp_opt: {}",
                self.rx_sensor_manager_from_tcp_opt.as_ref().unwrap().count
            )?;
        }
        writeln!(
            f,
            "tx_sensor_manager_to_signal_handler: {}",
            self.tx_sensor_manager_to_signal_handler.count
        )?;
        write!(
            f,
            "rx_sensor_manager_from_signal_handler: {}",
            self.rx_sensor_manager_from_signal_handler.count
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
