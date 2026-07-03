/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the channel structure for the `Memory` monitoring thread.
//!
//! This module provides the `MemoryChannels` struct, which encapsulates the communication
//! channel required by the `Memory` monitor. The `Memory` monitor is a simple,
//! one-way thread that periodically reports memory usage and only needs to listen
//! for a shutdown command.
//!
//! ## Key Components
//!
//! - **`MemoryChannels`**: The primary struct that holds the receiving end of a channel.
//!   - **`rx_memory_from_signal_handler`**: The `mpsc` receiver that listens for an
//!     `InternalCommand::Quit` message from the central `signal_handler`. This
//!     ensures the thread can be terminated gracefully during an application-wide
//!     shutdown.

use crate::launch::channels::AquaReceiver;
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

/// Struct collects the channels for communication with other threads.
pub struct MemoryChannels {
    /// Receiver part of the channel for communication from the signal handler
    pub rx_memory_from_signal_handler: AquaReceiver<InternalCommand>,
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for MemoryChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== MemoryChannels ===")?;
        write!(
            f,
            "rx_memory_from_signal_handler: {}",
            self.rx_memory_from_signal_handler.count
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for MemoryChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
