/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A central container for all inter-thread communication channels used by the `Heating` thread.
//!
//! This module defines the `HeatingChannels` struct, which acts as a dedicated
//! "switchboard". Its primary purpose is to aggregate all the `mpsc::channel` senders
//! and receivers that the `Heating` control thread needs to interact with other parts
//! of the application, such as the `RelayManager`, `SignalHandler`, and `ScheduleCheck`.
//!
//! By bundling all communication endpoints into a single, clearly defined struct, it
//! makes the `Heating` thread's dependencies explicit and simplifies its initialization.
//!
//! ## Key Components
//!
//! - **`HeatingChannels` Struct**: A container that explicitly declares the full
//!   set of communication dependencies for the `Heating` thread. The use of
//!   `Option` for the messaging channel (`rx_heating_from_messaging_opt`) makes
//!   optional communication paths type-safe.
//!
//! - **Helper Methods**: Provides a suite of wrapper methods, such as `send_to_relay_manager()`
//!   and `receive_from_schedule_check()`, to offer a clean, high-level API for channel
//!   operations, encapsulating the raw `send` and `recv` calls.
//!
//! ## Design and Architecture
//!
//! The `HeatingChannels` struct is a key part of the application's inter-thread
//! communication strategy, promoting clean and decoupled code.
//!
//! - **Centralization**: By gathering all necessary channels into a single struct, it
//!   provides a clear and explicit declaration of the `Heating` thread's
//!   communication dependencies. This makes the overall application architecture easier
//!   to reason about.
//!
//! - **Decoupling**: The main `Heating` thread logic doesn't need to manage
//!   individual channels or know how they are created. It just needs this struct.
//!   This separates the core control logic from the application's internal "wiring".
//!
//! - **Testability**: The struct can be easily constructed for usage with mock functions during
//!   unit testing, allowing for isolated testing of the `Heating` thread's
//!   logic without needing to run the entire application.

use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

/// Struct collects the channels for communication to relay manager, signal handler, data logger, schedule check, and messaging.
pub struct HeatingChannels {
    /// Sender part of the channel for communication to the relay manager
    pub tx_heating_to_relay_manager: AquaSender<InternalCommand>,

    /// Receiver part of the channel for communication from the relay manager
    pub rx_heating_from_relay_manager: AquaReceiver<bool>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_heating_to_signal_handler: AquaSender<bool>,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_heating_from_signal_handler: AquaReceiver<InternalCommand>,

    /// Sender part of the channel for communication to the schedule checker
    pub tx_heating_to_schedule_check: AquaSender<InternalCommand>,

    /// Receiver part of the channel for communication from the schedule checker
    pub rx_heating_from_schedule_check: AquaReceiver<bool>,

    /// Receiver part of the channel for communication from the messaging
    pub rx_heating_from_messaging_opt: Option<AquaReceiver<InternalCommand>>,
}

impl HeatingChannels {
    /// Sends a command to the relay manager.
    pub fn send_to_relay_manager(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_heating_to_relay_manager.send(command)
    }

    /// Receives a response from the relay manager.
    pub fn receive_from_relay_manager(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_heating_from_relay_manager.recv()
    }

    /// Sends a command to the schedule checker.
    pub fn send_to_schedule_check(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_heating_to_schedule_check.send(command)
    }

    /// Receives a response from the schedule checker.
    pub fn receive_from_schedule_check(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_heating_from_schedule_check.recv()
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for HeatingChannels {
    /// Formats the value using the given formatter.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== HeatingChannels ===")?;
        writeln!(
            f,
            "tx_heating_to_relay_manager: {}",
            self.tx_heating_to_relay_manager.count
        )?;
        writeln!(
            f,
            "rx_heating_from_relay_manager: {}",
            self.rx_heating_from_relay_manager.count
        )?;
        writeln!(
            f,
            "tx_heating_to_signal_handler: {}",
            self.tx_heating_to_signal_handler.count
        )?;
        writeln!(
            f,
            "rx_heating_from_signal_handler: {}",
            self.rx_heating_from_signal_handler.count
        )?;
        writeln!(
            f,
            "tx_heating_to_schedule_check: {}",
            self.tx_heating_to_schedule_check.count
        )?;
        writeln!(
            f,
            "rx_heating_from_schedule_check: {}",
            self.rx_heating_from_schedule_check.count
        )?;
        if let Some(ref rx_heating_from_messaging) = self.rx_heating_from_messaging_opt {
            write!(
                f,
                "rx_heating_from_messaging_opt: {}",
                rx_heating_from_messaging.count
            )?;
        }

        Ok(())
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for HeatingChannels {
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
