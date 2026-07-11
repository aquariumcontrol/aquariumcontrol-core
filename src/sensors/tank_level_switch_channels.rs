/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A central container for all inter-thread communication channels used by the `TankLevelSwitch` thread.
//!
//! This module defines the `TankLevelSwitchChannels` struct, which acts as a dedicated
//! "switchboard". Its primary purpose is to aggregate all the `mpsc::channel` senders
//! and receivers that the `TankLevelSwitch` thread needs to communicate with other parts
//! of the application.
//!
//! This includes two main communication pathways:
//! 1.  **Simulator Communication**: Optional channels (`..._to_tcp_opt`) for sending
//!     requests to and receiving data from a `TcpCommunication` thread when the
//!     application is running in simulator mode.
//! 2.  **Signal Handler Communication**: Channels for receiving graceful shutdown
//!     commands from and sending acknowledgments to the main `SignalHandler`.
//!
//! ## Key Components
//!
//! - **`TankLevelSwitchChannels` Struct**: A container that explicitly declares the full
//!   set of communication dependencies for the `TankLevelSwitch` thread. The use of
//!   `Option` for the TCP channels makes the simulator-specific communication path
//!   explicit.
//!
//! - **Helper Methods**: Provides convenient wrapper methods, such as `send_to_tcp()`
//!   and `receive_from_signal_handler()`, to offer a clean, consistent API for
//!   channel operations.
//!
//! ## Design and Architecture
//!
//! The `TankLevelSwitchChannels` struct is a key part of the application's inter-thread
//! communication strategy, promoting clean and decoupled code.
//!
//! - **Centralization**: By gathering all necessary channels into a single struct, it
//!   provides a clear and explicit declaration of the `TankLevelSwitch` thread's
//!   communication dependencies. This makes the overall application architecture easier
//!   to reason about.
//!
//! - **Decoupling**: The main `TankLevelSwitch` thread logic doesn't need to manage
//!   individual channels. It just needs this struct. This separates the core sensor
//!   stabilization logic from the communication infrastructure.
//!
//! - **Mode-Specific Communication**: The use of `Option` for the TCP channels is a
//!   core design element. The channels are only `Some(...)` when the application is
//!   configured to run in simulator mode, making the hardware vs. simulator logic
//!   paths type-safe and explicit.
//!
//! - **Testability**: The struct can be easily constructed for usage with mock functions during
//!   unit testing, allowing for isolated testing of the `TankLevelSwitch` thread's
//!   logic without needing to run the entire application.

use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::simulator::tcp_communication::TcpCommunicationError;
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

/// Container for channels used by TankLevelSwitch
pub struct TankLevelSwitchChannels {
    /// Sender part of the channel for communication via TCP wrapped as optional
    pub tx_tank_level_switch_to_tcp_opt: Option<AquaSender<InternalCommand>>,

    /// Receiver part of the channel for communication via TCP wrapped as optional
    pub rx_tank_level_switch_from_tcp_opt: Option<AquaReceiver<Result<f32, TcpCommunicationError>>>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_tank_level_switch_to_signal_handler: AquaSender<bool>,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_tank_level_switch_from_signal_handler: AquaReceiver<InternalCommand>,
}

impl TankLevelSwitchChannels {
    /// Sends a command to the TCP simulator if the channel exists.
    #[allow(unused)]
    pub fn send_to_tcp(
        &mut self,
        command: InternalCommand,
    ) -> Option<Result<(), AquaChannelError>> {
        self.tx_tank_level_switch_to_tcp_opt
            .as_mut()
            .map(|tx| tx.send(command))
    }

    /// Receives a result from the TCP simulator if the channel exists.
    #[allow(unused)]
    pub async fn receive_from_tcp(
        &mut self,
    ) -> Option<Result<Result<f32, TcpCommunicationError>, AquaChannelError>> {
        match &mut self.rx_tank_level_switch_from_tcp_opt {
            Some(rx) => Some(rx.recv().await),
            None => None,
        }
    }

    /// Sends an acknowledgment to the signal handler.
    ///
    /// # Arguments
    /// * `ack` - A parameter representing `ack`.
    #[allow(unused)]
    pub fn send_to_signal_handler(&mut self, ack: bool) -> Result<(), AquaChannelError> {
        self.tx_tank_level_switch_to_signal_handler.send(ack)
    }

    /// Receives a command from the signal handler.
    #[allow(unused)]
    pub async fn receive_from_signal_handler(
        &mut self,
    ) -> Result<InternalCommand, AquaChannelError> {
        self.rx_tank_level_switch_from_signal_handler.recv().await
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for TankLevelSwitchChannels {
    /// Formats the value using the given formatter.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== TankLevelSwitchChannels ===")?;
        if let Some(tx_tank_level_switch_to_tcp) = self.tx_tank_level_switch_to_tcp_opt.as_ref() {
            writeln!(
                f,
                "tx_tank_level_switch_to_tcp_opt: {}",
                tx_tank_level_switch_to_tcp.count
            )?;
        }
        writeln!(
            f,
            "rx_tank_level_switch_from_tcp_opt: {}",
            self.rx_tank_level_switch_from_tcp_opt
                .as_ref()
                .unwrap()
                .count
        )?;
        writeln!(
            f,
            "tx_tank_level_switch_to_signal_handler: {}",
            self.tx_tank_level_switch_to_signal_handler.count
        )?;
        write!(
            f,
            "rx_tank_level_switch_from_signal_handler: {}",
            self.rx_tank_level_switch_from_signal_handler.count
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for TankLevelSwitchChannels {
    /// Formats the value using the given formatter.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
