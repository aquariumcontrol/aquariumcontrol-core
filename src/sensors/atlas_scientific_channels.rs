/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A central container for all inter-thread communication channels used by the `AtlasScientific` thread.
//!
//! This module defines the `AtlasScientificChannels` struct, which acts as a dedicated
//! "switchboard". Its primary purpose is to aggregate all the `mpsc::channel` senders
//! and receivers that the `AtlasScientific` thread needs to communicate with its
//! dependencies: the `I2cInterface` thread for hardware access and the main
//! `SignalHandler` for graceful shutdown.
//!
//! ## Key Components
//!
//! - **`AtlasScientificChannels` Struct**: A simple container struct that explicitly
//!   declares the full set of communication dependencies for the `AtlasScientific` thread.
//!
//! - **Helper Methods**: Provides convenient wrapper methods, such as
//!   `send_to_i2c_interface()` and `receive_from_i2c_interface()`, to offer a
//!   clean, consistent API for channel operations.
//!
//! ## Design and Architecture
//!
//! The `AtlasScientificChannels` struct is a key part of the application's inter-thread
//! communication strategy, promoting clean and decoupled code.
//!
//! - **Centralization**: By gathering all necessary channels into a single struct, it
//!   provides a clear and explicit declaration of the `AtlasScientific` thread's
//!   communication dependencies.
//!
//! - **Decoupling**: The main `AtlasScientific` thread logic doesn't need to manage
//!   individual channels. It just needs this struct, which it can use to send and
//!   receive messages. This separates the core measurement logic from the communication
//!   infrastructure.
//!
//! - **Testability**: The struct can be easily constructed and used with mock functions during
//!   unit testing, allowing for isolated testing of the `AtlasScientific` thread's
//!   logic without needing to run the entire application.

use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::sensors::i2c_interface::{I2cRequest, I2cResult};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

/// Struct collects the channels for the communication between AtlasScientific and other components.
pub struct AtlasScientificChannels {
    /// Sender part of the channel for communication to I2CInterface
    #[allow(unused)] // used in conditionally compiled code
    pub tx_atlas_scientific_to_i2c_interface: AquaSender<I2cRequest>,

    /// Receiver part of the channel for communication from I2CInterface
    #[allow(unused)] // used in conditionally compiled code
    pub rx_atlas_scientific_from_i2c_interface: AquaReceiver<I2cResult>,

    /// Sender part of the channel for communication to the signal handler
    #[allow(unused)] // used in conditionally compiled code
    pub tx_atlas_scientific_to_signal_handler: AquaSender<bool>,

    /// Receiver part of the channel for communication from the signal handler
    #[allow(unused)] // used in conditionally compiled code
    pub rx_atlas_scientific_from_signal_handler: AquaReceiver<InternalCommand>,
}

impl AtlasScientificChannels {
    /// Sends a request to the I2C interface.
    #[allow(unused)] // used in conditionally compiled code
    pub fn send_to_i2c_interface(&mut self, request: I2cRequest) -> Result<(), AquaChannelError> {
        self.tx_atlas_scientific_to_i2c_interface.send(request)
    }

    /// Receives a result from the I2C interface.
    #[allow(unused)] // used in conditionally compiled code
    pub fn receive_from_i2c_interface(&mut self) -> Result<I2cResult, AquaChannelError> {
        self.rx_atlas_scientific_from_i2c_interface.try_recv()
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for AtlasScientificChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== AtlasScientificChannels ===")?;
        writeln!(
            f,
            "tx_atlas_scientific_to_i2c_interface: {}",
            self.tx_atlas_scientific_to_i2c_interface.count
        )?;
        writeln!(
            f,
            "rx_atlas_scientific_from_i2c_interface: {}",
            self.rx_atlas_scientific_from_i2c_interface.count
        )?;
        writeln!(
            f,
            "tx_atlas_scientific_to_signal_handler: {}",
            self.tx_atlas_scientific_to_signal_handler.count
        )?;
        write!(
            f,
            "rx_atlas_scientific_from_signal_handler: {}",
            self.rx_atlas_scientific_from_signal_handler.count
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for AtlasScientificChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
