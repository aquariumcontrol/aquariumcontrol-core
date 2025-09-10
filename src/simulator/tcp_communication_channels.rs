/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::simulator::tcp_communication::TcpCommunicationError;
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

/// Struct collects the channels for communication with other threads.
pub struct TcpCommunicationChannels {
    /// Receiver part of the channel for communication from the relay manager wrapped in Option type
    pub rx_tcp_communication_from_relay_manager_opt: Option<AquaReceiver<InternalCommand>>,

    /// Receiver part of the channel for communication from the sensor manager wrapped in Option type
    pub rx_tcp_communication_from_sensor_manager_opt: Option<AquaReceiver<InternalCommand>>,

    /// Sender part of the channel for communication to sensor manager wrapped in Option type
    pub tx_tcp_communication_to_sensor_manager_opt:
        Option<AquaSender<Result<f32, TcpCommunicationError>>>,

    /// Receiver part of the channel for communication from tank level switch wrapped in Option type
    pub rx_tcp_communication_from_tank_level_switch_opt: Option<AquaReceiver<InternalCommand>>,

    /// Sender part of the channel for communication to tank level switch wrapped in Option type
    pub tx_tcp_communication_to_tank_level_switch_opt:
        Option<AquaSender<Result<f32, TcpCommunicationError>>>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_tcp_communication_to_signal_handler: AquaSender<bool>,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_tcp_communication_from_signal_handler: AquaReceiver<InternalCommand>,
}

impl TcpCommunicationChannels {
    /// Receives a command from the relay manager if the channel exists.
    #[allow(unused)]
    pub fn receive_from_relay_manager(
        &mut self,
    ) -> Option<Result<InternalCommand, AquaChannelError>> {
        self.rx_tcp_communication_from_relay_manager_opt
            .as_mut()
            .map(|rx| rx.recv())
    }

    /// Receives a command from the sensor manager if the channel exists.
    #[allow(unused)]
    pub fn receive_from_sensor_manager(
        &mut self,
    ) -> Option<Result<InternalCommand, AquaChannelError>> {
        self.rx_tcp_communication_from_sensor_manager_opt
            .as_mut()
            .map(|rx| rx.recv())
    }

    /// Sends a result to the sensor manager if the channel exists.
    #[allow(unused)]
    pub fn send_to_sensor_manager(
        &mut self,
        data: Result<f32, TcpCommunicationError>,
    ) -> Option<Result<(), AquaChannelError>> {
        self.tx_tcp_communication_to_sensor_manager_opt
            .as_mut()
            .map(|tx| tx.send(data))
    }

    /// Receives a command from the tank level switch if the channel exists.
    #[allow(unused)]
    pub fn receive_from_tank_level_switch(
        &mut self,
    ) -> Option<Result<InternalCommand, AquaChannelError>> {
        self.rx_tcp_communication_from_tank_level_switch_opt
            .as_mut()
            .map(|rx| rx.recv())
    }

    /// Sends a result to the tank level switch if the channel exists.
    #[allow(unused)]
    pub fn send_to_tank_level_switch(
        &mut self,
        data: Result<f32, TcpCommunicationError>,
    ) -> Option<Result<(), AquaChannelError>> {
        self.tx_tcp_communication_to_tank_level_switch_opt
            .as_mut()
            .map(|tx| tx.send(data))
    }

    /// Sends an acknowledgment to the signal handler.
    pub fn send_to_signal_handler(&mut self, ack: bool) -> Result<(), AquaChannelError> {
        self.tx_tcp_communication_to_signal_handler.send(ack)
    }

    /// Receives a command from the signal handler.
    #[allow(unused)]
    pub fn receive_from_signal_handler(&mut self) -> Result<InternalCommand, AquaChannelError> {
        self.rx_tcp_communication_from_signal_handler.try_recv()
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for TcpCommunicationChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== TcpCommunicationChannels ===")?;
        writeln!(
            f,
            "rx_tcp_communication_from_relay_manager_opt: {}",
            self.rx_tcp_communication_from_relay_manager_opt
                .as_ref()
                .unwrap()
                .count
        )?;
        writeln!(
            f,
            "rx_tcp_communication_from_sensor_manager_opt: {}",
            self.rx_tcp_communication_from_sensor_manager_opt
                .as_ref()
                .unwrap()
                .count
        )?;
        writeln!(
            f,
            "tx_tcp_communication_to_sensor_manager_opt: {}",
            self.tx_tcp_communication_to_sensor_manager_opt
                .as_ref()
                .unwrap()
                .count
        )?;
        writeln!(
            f,
            "rx_tcp_communication_from_tank_level_switch_opt: {}",
            self.rx_tcp_communication_from_tank_level_switch_opt
                .as_ref()
                .unwrap()
                .count
        )?;
        writeln!(
            f,
            "tx_tcp_communication_to_tank_level_switch_opt: {}",
            self.tx_tcp_communication_to_tank_level_switch_opt
                .as_ref()
                .unwrap()
                .count
        )?;
        writeln!(
            f,
            "tx_tcp_communication_to_signal_handler: {}",
            self.tx_tcp_communication_to_signal_handler.count
        )?;
        write!(
            f,
            "rx_tcp_communication_from_signal_handler: {}",
            self.rx_tcp_communication_from_signal_handler.count
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for TcpCommunicationChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}

impl TcpCommunicationChannels {
    #[cfg(not(test))]
    pub fn report_status(&self) {
        println!("TcpCommunicationChannels Status:");

        println!(
            "- rx_tcp_communication_from_relay_manager_opt: {}",
            self.rx_tcp_communication_from_relay_manager_opt
                .as_ref()
                .map_or("None", |_| "Some")
        );

        println!(
            "- rx_tcp_communication_from_sensor_manager_opt: {}",
            self.rx_tcp_communication_from_sensor_manager_opt
                .as_ref()
                .map_or("None", |_| "Some")
        );

        println!(
            "- tx_tcp_communication_to_sensor_manager_opt: {}",
            self.tx_tcp_communication_to_sensor_manager_opt
                .as_ref()
                .map_or("None", |_| "Some")
        );

        println!(
            "- rx_tcp_communication_from_tank_level_switch_opt: {}",
            self.rx_tcp_communication_from_tank_level_switch_opt
                .as_ref()
                .map_or("None", |_| "Some")
        );

        println!(
            "- tx_tcp_communication_to_tank_level_switch_opt: {}",
            self.tx_tcp_communication_to_tank_level_switch_opt
                .as_ref()
                .map_or("None", |_| "Some")
        );

        println!("- tx_tcp_communication_to_signal_handler: Present");

        println!("- rx_tcp_communication_from_signal_handler: Present");
    }
}
