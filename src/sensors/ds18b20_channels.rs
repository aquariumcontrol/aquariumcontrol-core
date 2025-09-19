/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A central container for the communication channel used by the `Ds18b20` thread.
//!
//! This module defines the `Ds18b20Channels` struct, which acts as a dedicated
//! "switchboard". Its primary purpose is to aggregate the `mpsc::channel`
//! receiver that the `Ds18b20` thread needs to communicate with the main
//! `SignalHandler` for graceful shutdown.
//!
//! ## Key Components
//!
//! - **`Ds18b20Channels` Struct**: A simple container struct that explicitly declares
//!   the communication dependencies for the `Ds18b20` thread. In this case, it's
//!   a single receiver from the `SignalHandler`.
//!
//! - **Helper Methods**: Provides a convenient wrapper method,
//!   `receive_from_signal_handler()`, to offer a clean, consistent API for
//!   channel operations.
//!
//! ## Design and Architecture
//!
//! The `Ds18b20Channels` struct is a key part of the application's inter-thread
//! communication strategy, promoting clean and decoupled code.
//!
//! - **Centralization**: By gathering all necessary channels into a single struct, it
//!   provides a clear and explicit declaration of the `Ds18b20` thread's
//!   communication dependencies. This makes the overall application architecture easier
//!   to reason about.
//!
//! - **Decoupling**: The main `Ds18b20` thread logic doesn't need to manage
//!   individual channels. It just needs this struct, which it can use to
//!   receive messages. This separates the core sensor reading logic from the
//!   communication infrastructure.
//!
//! - **Testability**: The struct can be easily constructed for use in mock functions for
//!   unit testing, allowing for isolated testing of the `Ds18b20` thread's
//!   shutdown logic without needing to run the entire application.

use crate::launch::channels::{AquaChannelError, AquaReceiver};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

pub struct Ds18b20Channels {
    pub rx_ds18b20_from_signal_handler: AquaReceiver<InternalCommand>,
}

impl Ds18b20Channels {
    /// Receives a command from the signal handler (non-blocking).
    ///
    /// This wrapper function performs a non-blocking receive-operation for a command and increments
    /// the associated debug counter if the `debug_channels` feature is enabled and a
    /// message is successfully received.
    #[allow(unused)]
    pub fn receive_from_signal_handler(&mut self) -> Result<InternalCommand, AquaChannelError> {
        self.rx_ds18b20_from_signal_handler.try_recv()
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for Ds18b20Channels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== Ds18b20Channels ===")?;
        write!(
            f,
            "\ncnt_rx_ds18b20_from_signal_handler: {}",
            self.rx_ds18b20_from_signal_handler.count
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for Ds18b20Channels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
