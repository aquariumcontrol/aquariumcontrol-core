/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A central container for all inter-thread communication channels used by the `Feed` module.
//!
//! This module defines the `FeedChannels` struct, which acts as a dedicated "switchboard"
//! for the `Feed` thread. Its primary purpose is to aggregate all the `mpsc::channel`
//! senders and receivers that the `Feed` thread needs to communicate with other concurrent
//! parts of the application, such as the `RelayManager`, `SignalHandler`, and the IPC
//! `Messaging` thread.
//!
//! ## Key Components
//!
//! - **`FeedChannels` Struct**: A simple container struct that explicitly declares all the
//!   communication dependencies of the `Feed` thread.
//!
//! - **Helper Methods**: Provides convenient wrapper methods like `send_to_relay_manager()`
//!   and `receive_from_relay_manager()`. These methods offer a clean, consistent API for
//!   channel operations and abstract away the direct use of the sender/receiver fields.
//!
//! ## Design and Architecture
//!
//! The `FeedChannels` struct is a key part of the application's inter-thread
//! communication strategy, promoting clean and decoupled code.
//!
//! - **Centralization**: By gathering all necessary channels into a single struct, it
//!   provides a clear and explicit declaration of the `Feed` thread's communication
//!   dependencies. This makes the overall architecture easier to reason about.
//!
//! - **Decoupling**: The main `Feed` thread logic doesn't need to manage a list of
//!   individual channels. It just needs this struct, which it can use to send and
//!   receive messages. This separates the core control logic from the communication
//!   infrastructure.
//!
//! - **Testability**: The struct's channels can be used by mock implementation during
//!   unit testing, allowing for isolated testing of the `Feed` thread's logic without
//!   needing to run the entire application.

use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

/// Struct collects the channels for communication between the feed control and the other components.
pub struct FeedChannels {
    /// Sender part of the channel for communication to relay manager
    pub tx_feed_to_relay_manager: AquaSender<InternalCommand>,

    /// Receiver part of the channel for communication from the relay manager
    pub rx_feed_from_relay_manager: AquaReceiver<bool>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_feed_to_signal_handler: AquaSender<bool>,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_feed_from_signal_handler: AquaReceiver<InternalCommand>,

    /// Receiver part of the channel wrapped in Option for communication from the messaging
    pub rx_feed_from_messaging_opt: Option<AquaReceiver<InternalCommand>>,
}

impl FeedChannels {
    /// Sends a command to the relay manager.
    pub fn send_to_relay_manager(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_feed_to_relay_manager.send(command)
    }

    /// Receives a response from the relay manager.
    pub async fn receive_from_relay_manager(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_feed_from_relay_manager.recv().await
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for FeedChannels {
    /// Formats the value using the given formatter.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== FeedChannels ===")?;
        writeln!(
            f,
            "tx_feed_to_relay_manager: {}",
            self.tx_feed_to_relay_manager.count
        )?;
        writeln!(
            f,
            "rx_feed_from_relay_manager: {}",
            self.rx_feed_from_relay_manager.count
        )?;
        writeln!(
            f,
            "tx_feed_to_signal_handler: {}",
            self.tx_feed_to_signal_handler.count
        )?;
        writeln!(
            f,
            "rx_feed_from_signal_handler: {}",
            self.rx_feed_from_signal_handler.count
        )?;
        if let Some(rx_feed_from_messaging) = &self.rx_feed_from_messaging_opt {
            write!(
                f,
                "rx_feed_from_messaging_opt: {}",
                rx_feed_from_messaging.count
            )?;
        }
        Ok(())
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for FeedChannels {
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
