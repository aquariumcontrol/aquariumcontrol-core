/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A central container for all inter-thread communication channels used by the `TcpCommunication` thread.
//!
//! This module defines the `TcpCommunicationChannels` struct, which acts as a dedicated
//! "switchboard". Its primary purpose is to aggregate all the `mpsc::channel` senders
//! and receivers that the `TcpCommunication` thread needs to act as a proxy for the
//! external hardware simulator.
//!
//! It collects channels from all potential client threads (like `RelayManager`, `SensorManager`,
//! etc.) and the main `SignalHandler`.
//!
//! ## Key Components
//!
//! - **`TcpCommunicationChannels` Struct**: A container that explicitly declares the full
//!   set of communication dependencies for the `TcpCommunication` thread. The use of
//!   `Option` for the client channels makes the simulator-specific communication paths
//!   explicit and type-safe.
//!
//! - **Helper Methods**: Provides a suite of wrapper methods, such as `send_to_sensor_manager()`
//!   and `receive_from_relay_manager()`, to offer a clean API for channel operations.
//!
//! ## Design and Architecture
//!
//! The `TcpCommunicationChannels` struct is a key part of the application's inter-thread
//! communication strategy, promoting clean and decoupled code.
//!
//! - **Centralization**: By gathering all necessary channels into a single struct, it
//!   provides a clear and explicit declaration of the `TcpCommunication` thread's
//!   communication dependencies. This makes the overall application architecture easier
//!   to reason about when in simulator mode.
//!
//! - **Decoupling**: The main `TcpCommunication` thread logic doesn't need to manage
//!   individual channels. It just needs this struct. This separates the core TCP
//!   protocol logic from the application's internal communication infrastructure.
//!
//! - **Conditional Communication**: The use of `Option` for the client channels is a
//!   core design element. The channels are only `Some(...)` when the application is
//!   configured to run in simulator mode *and* the corresponding client thread is
//!   active. This prevents runtime errors from trying to communicate on a non-existent channel.
//!
//! - **Testability**: The struct can be easily constructed for usage with mock functions during
//!   unit testing, allowing for isolated testing of the `TcpCommunication` thread's
//!   logic without needing to run the entire application.

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
    #[cfg(all(not(test), feature = "debug_channels"))]
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
