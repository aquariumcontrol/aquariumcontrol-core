/* Copyright 2026 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A central container for all inter-thread communication channels used by the `TemperatureGradient` thread.
//!
//! This module defines the `TemperatureGradientChannels` struct, which acts as a dedicated
//! "switchboard". Its primary purpose is to aggregate all the `mpsc::channel` senders
//! and receivers that the `TemperatureGradient` control thread needs to interact with other parts
//! of the application, such as the `SignalHandler`.
//!
//! By bundling all communication endpoints into a single, clearly defined struct, it
//! makes the `TemperatureGradient` thread's dependencies explicit and simplifies its initialization.
//!
//! ## Key Components
//!
//! - **`TemperatureGradientChannels` Struct**: A container that explicitly declares the full
//!   set of communication dependencies for the `TemperatureGradient` thread.
//!
//! ## Design and Architecture
//!
//! The `TemperatureGradientChannels` struct is a key part of the application's inter-thread
//! communication strategy, promoting clean and decoupled code.
//!
//! - **Centralization**: By gathering all necessary channels into a single struct, it
//!   provides a clear and explicit declaration of the `TemperatureGradient` thread's
//!   communication dependencies. This makes the overall application architecture easier
//!   to reason about.
//!
//! - **Decoupling**: The main `TemperatureGradient` thread logic doesn't need to manage
//!   individual channels or know how they are created. It just needs this struct.
//!   This separates the core control logic from the application's internal "wiring".
//!
//! - **Testability**: The struct can be easily constructed for usage with mock functions during
//!   unit testing, allowing for isolated testing of the `TemperatureGradient` thread's
//!   logic without needing to run the entire application.

use crate::launch::channels::{AquaReceiver, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

/// Struct collects the channels for communication between TemperatureGradient and other components.
pub struct TemperatureGradientChannels {
    /// Sender part of the channel for communication to the signal handler
    pub tx_temperature_gradient_to_signal_handler: AquaSender<bool>,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_temperature_gradient_from_signal_handler: AquaReceiver<InternalCommand>,
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for VentilationChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== TemperatureGradientChannels ===")?;
        writeln!(
            f,
            "tx_temperature_gradient_to_signal_handler: {}",
            self.tx_temperature_gradient_to_signal_handler.count
        )?;
        writeln!(
            f,
            "rx_temperature_gradient_from_signal_handler: {}",
            self.rx_temperature_gradient_from_signal_handler.count
        )?;
        Ok(())
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for TemperatureGradientChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
