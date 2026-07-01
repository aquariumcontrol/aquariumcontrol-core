/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A central container for all inter-thread communication channels used by the `RelayManager`.
//!
//! This module defines the `RelayManagerChannels` struct, which acts as a dedicated
//! "switchboard" for the `RelayManager` thread. Its primary purpose is to aggregate all
//! the `mpsc::channel` senders and receivers that the `RelayManager` needs to communicate
//! with its various "client" modules (e.g., `Refill`, `Heating`, `Ventilation`) and the
//! main `SignalHandler`.
//!
//! ## Key Components
//!
//! - **`RelayManagerChannels` Struct**: A simple container struct that explicitly declares
//!   the full set of communication dependencies for the `RelayManager` thread.
//!
//! - **Helper Methods**: Provides a large set of convenient wrapper methods, such as
//!   `send_to_refill()` and `receive_from_refill()`. These offer a clean, consistent API
//!   for channel operations, although in practice the `RelayManager`'s main loop may
//!   access the fields directly.
//!
//! ## Design and Architecture
//!
//! The `RelayManagerChannels` struct is a key part of the application's inter-thread
//! communication strategy, promoting clean and decoupled code.
//!
//! - **Centralization**: By gathering all necessary channels into a single struct, it
//!   provides a clear and explicit declaration of the `RelayManager` thread's communication
//!   dependencies. This makes the overall application architecture easier to reason about.
//!
//! - **Decoupling**: The main `RelayManager` thread logic doesn't need to manage
//!   individual channels. It just needs this struct, which it can use to send and
//!   receive messages. This separates the core dispatching logic from the communication
//!   infrastructure.
//!
//! - **Testability**: The struct can be easily constructed with mock channels during
//!   unit testing, allowing for isolated testing of the `RelayManager` thread's logic without
//!   needing to run the entire application.

use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

/// Container for channels used by relay manager
pub struct RelayManagerChannels {
    /// sender part of the channel for communication to the refill thread
    pub tx_relay_manager_to_refill: AquaSender<bool>,
    /// receiver part of the channel for communication from the refill thread
    pub rx_relay_manager_from_refill: AquaReceiver<InternalCommand>,

    /// sender part of the channel for communication to the heating thread
    pub tx_relay_manager_to_heating: AquaSender<bool>,
    /// receiver part of the channel for communication from the heating thread
    pub rx_relay_manager_from_heating: AquaReceiver<InternalCommand>,

    /// sender part of the channel for communication to the ventilation thread
    pub tx_relay_manager_to_ventilation: AquaSender<bool>,
    /// receiver part of the channel for communication from the ventilation thread
    pub rx_relay_manager_from_ventilation: AquaReceiver<InternalCommand>,

    /// sender part of the channel for communication to the Balling dosing thread
    pub tx_relay_manager_to_balling: AquaSender<bool>,
    /// receiver part of the channel for communication from the Balling dosing thread
    pub rx_relay_manager_from_balling: AquaReceiver<InternalCommand>,

    /// sender part of the channel for communication to the feed thread
    pub tx_relay_manager_to_feed: AquaSender<bool>,
    /// receiver part of the channel for communication from the feed thread
    pub rx_relay_manager_from_feed: AquaReceiver<InternalCommand>,

    /// sender part of the channel for communication to the signal handler
    pub tx_relay_manager_to_signal_handler: AquaSender<bool>,
    /// receiver part of the channel for communication from the signal handler
    pub rx_relay_manager_from_signal_handler: AquaReceiver<InternalCommand>,
}

impl RelayManagerChannels {
    /// Sends a response to the refill module.
    ///
    /// # Arguments
    /// * `ack` - A parameter representing `ack`.
    #[allow(unused)]
    pub fn send_to_refill(&mut self, ack: bool) -> Result<(), AquaChannelError> {
        self.tx_relay_manager_to_refill.send(ack)
    }

    /// Receives a command from the refill module (non-blocking).
    #[allow(unused)]
    pub fn receive_from_refill(&mut self) -> Result<InternalCommand, AquaChannelError> {
        self.rx_relay_manager_from_refill.try_recv()
    }

    /// Sends a response to the heating module.
    ///
    /// # Arguments
    /// * `ack` - A parameter representing `ack`.
    #[allow(unused)]
    pub fn send_to_heating(&mut self, ack: bool) -> Result<(), AquaChannelError> {
        self.tx_relay_manager_to_heating.send(ack)
    }

    /// Receives a command from the heating module (non-blocking).
    #[allow(unused)]
    pub fn receive_from_heating(&mut self) -> Result<InternalCommand, AquaChannelError> {
        self.rx_relay_manager_from_heating.try_recv()
    }

    /// Sends a response to the ventilation module.
    ///
    /// # Arguments
    /// * `ack` - A parameter representing `ack`.
    #[allow(unused)]
    pub fn send_to_ventilation(&mut self, ack: bool) -> Result<(), AquaChannelError> {
        self.tx_relay_manager_to_ventilation.send(ack)
    }

    /// Receives a command from the ventilation module (non-blocking).
    #[allow(unused)]
    pub fn receive_from_ventilation(&mut self) -> Result<InternalCommand, AquaChannelError> {
        self.rx_relay_manager_from_ventilation.try_recv()
    }

    /// Sends a response to the balling module.
    ///
    /// # Arguments
    /// * `ack` - A parameter representing `ack`.
    #[allow(unused)]
    pub fn send_to_balling(&mut self, ack: bool) -> Result<(), AquaChannelError> {
        self.tx_relay_manager_to_balling.send(ack)
    }

    /// Receives a command from the balling module (non-blocking).
    #[allow(unused)]
    pub fn receive_from_balling(&mut self) -> Result<InternalCommand, AquaChannelError> {
        self.rx_relay_manager_from_balling.try_recv()
    }

    /// Sends a response to the feed module.
    ///
    /// # Arguments
    /// * `ack` - A parameter representing `ack`.
    #[allow(unused)]
    pub fn send_to_feed(&mut self, ack: bool) -> Result<(), AquaChannelError> {
        self.tx_relay_manager_to_feed.send(ack)
    }

    /// Receives a command from the feed module (non-blocking).
    #[allow(unused)]
    pub fn receive_from_feed(&mut self) -> Result<InternalCommand, AquaChannelError> {
        self.rx_relay_manager_from_feed.try_recv()
    }

    /// Sends a response to the signal handler.
    ///
    /// # Arguments
    /// * `ack` - A parameter representing `ack`.
    #[allow(unused)]
    pub fn send_to_signal_handler(&mut self, ack: bool) -> Result<(), AquaChannelError> {
        self.tx_relay_manager_to_signal_handler.send(ack)
    }

    /// Receives a command from the signal handler (non-blocking).
    #[allow(unused)]
    pub fn receive_from_signal_handler(&mut self) -> Result<InternalCommand, AquaChannelError> {
        self.rx_relay_manager_from_signal_handler.try_recv()
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for RelayManagerChannels {
    /// Formats the value using the given formatter.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== RelayManagerChannels ===")?;
        writeln!(
            f,
            "tx_relay_manager_to_refill: {}",
            self.tx_relay_manager_to_refill.count
        )?;
        writeln!(
            f,
            "rx_relay_manager_from_refill: {}",
            self.rx_relay_manager_from_refill.count
        )?;
        writeln!(
            f,
            "tx_relay_manager_to_heating: {}",
            self.tx_relay_manager_to_heating.count
        )?;
        writeln!(
            f,
            "rx_relay_manager_from_heating: {}",
            self.rx_relay_manager_from_heating.count
        )?;
        writeln!(
            f,
            "tx_relay_manager_to_ventilation: {}",
            self.tx_relay_manager_to_ventilation.count
        )?;
        writeln!(
            f,
            "rx_relay_manager_from_ventilation: {}",
            self.rx_relay_manager_from_ventilation.count
        )?;
        writeln!(
            f,
            "tx_relay_manager_to_balling: {}",
            self.tx_relay_manager_to_balling.count
        )?;
        writeln!(
            f,
            "rx_relay_manager_from_balling: {}",
            self.rx_relay_manager_from_balling.count
        )?;
        writeln!(
            f,
            "tx_relay_manager_to_feed: {}",
            self.tx_relay_manager_to_feed.count
        )?;
        writeln!(
            f,
            "rx_relay_manager_from_feed: {}",
            self.rx_relay_manager_from_feed.count
        )?;
        writeln!(
            f,
            "tx_relay_manager_to_signal_handler: {}",
            self.tx_relay_manager_to_signal_handler.count
        )?;
        write!(
            f,
            "rx_relay_manager_from_signal_handler: {}",
            self.rx_relay_manager_from_signal_handler.count
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for RelayManagerChannels {
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
