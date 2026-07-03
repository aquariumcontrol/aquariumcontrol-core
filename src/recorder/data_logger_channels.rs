/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A central container for all inter-thread communication channels used by the `DataLogger` module.
//!
//! This module defines the `DataLoggerChannels` struct, which acts as a dedicated "switchboard"
//! for the `DataLogger` thread. Its primary purpose is to aggregate the `mpsc::channel`
//! sender and receiver that the `DataLogger` thread needs to communicate with the `SignalHandler`.
//!
//! ## Key Components
//!
//! - **`DataLoggerChannels` Struct**: A simple container struct that explicitly declares the
//!   communication dependencies of the `DataLogger` thread.
//!
//! - **Helper Methods**: Provides a convenient wrapper method, `send_to_signal_handler()`.
//!   This offers a clean, consistent API for channel operations and abstracts away the
//!   direct use of the sender field.
//!
//! ## Design and Architecture
//!
//! The `DataLoggerChannels` struct is a key part of the application's inter-thread
//! communication strategy, promoting clean and decoupled code.
//!
//! - **Centralization**: By gathering all necessary channels into a single struct, it
//!   provides a clear and explicit declaration of the `DataLogger` thread's communication
//!   dependencies. This makes the overall architecture easier to reason about.
//!
//! - **Decoupling**: The main `DataLogger` thread logic doesn't need to manage
//!   individual channels. It just needs this struct, which it can use to send and
//!   receive messages. This separates the core data logging logic from the communication
//!   infrastructure.
//!
//! - **Testability**: The struct can be easily constructed with mock channels during
//!   unit testing, allowing for isolated testing of the `DataLogger` thread's logic without
//!   needing to run the entire application.

use crate::launch::channels::{AquaReceiver, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

/// Struct acts as a central hub for communication channels used by the DataLogger.
/// It's a way to bundle together all the necessary mpsc (Multi-Producer, Single-Consumer) channel
/// endpoints that the data logger needs to interact with other parts of your application.
pub struct DataLoggerChannels {
    /// Sender part of the channel for communication to the signal handler
    pub tx_data_logger_to_signal_handler: AquaSender<bool>,
    /// Receiver part of the channel for communication from the signal handler                
    pub rx_data_logger_from_signal_handler: AquaReceiver<InternalCommand>,
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for DataLoggerChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== DataLoggerChannels ===")?;
        writeln!(
            f,
            "tx_data_logger_to_signal_handler: {}",
            self.tx_data_logger_to_signal_handler.count
        )?;
        write!(
            f,
            "rx_data_logger_from_signal_handler: {}",
            self.rx_data_logger_from_signal_handler.count
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for DataLoggerChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
