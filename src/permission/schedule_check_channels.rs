/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A central container for all inter-thread communication channels used by the `ScheduleCheck` module.
//!
//! This module defines the `ScheduleCheckChannels` struct, which acts as a dedicated "switchboard"
//! for the `ScheduleCheck` thread. Its primary purpose is to aggregate all the `mpsc::channel`
//! senders and receivers that the `ScheduleCheck` thread needs to communicate with its client
//! modules (`Ventilation`, `Heating`, `Refill`, `Balling`) and the `SignalHandler`.
//!
//! ## Design and Architecture
//!
//! The `ScheduleCheckChannels` struct is a key part of the application's inter-thread
//! communication strategy, promoting clean and decoupled code.
//!
//! - **Centralization**: By gathering all necessary channels into a single struct, it
//!   provides a clear and explicit declaration of the `ScheduleCheck` thread's communication
//!   dependencies. This makes the overall architecture easier to reason about.
//!
//! - **Decoupling**: The main `ScheduleCheck` thread logic doesn't need to manage a list of
//!   individual channels. It just needs this struct, which it can use to send and
//!   receive messages. This separates the core permission logic from the communication
//!   infrastructure.
//!
//! - **Testability**: The struct can be easily constructed with mock channels during
//!   unit testing, allowing for isolated testing of the `ScheduleCheck` thread's logic without
//!   needing to run the entire application.

use crate::launch::channels::{AquaReceiver, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

/// Struct collects all channels for communication between ScheduleCheck and other components.
pub struct ScheduleCheckChannels {
    /// Sender part of the channel for communication to ventilation
    pub tx_schedule_check_to_ventilation: AquaSender<bool>,

    /// Receiver part of the channel for communication from ventilation
    pub rx_schedule_check_from_ventilation: AquaReceiver<InternalCommand>,

    /// Sender part of the channel for communication to heating
    pub tx_schedule_check_to_heating: AquaSender<bool>,

    /// Receiver part of the channel for communication from heating
    pub rx_schedule_check_from_heating: AquaReceiver<InternalCommand>,

    /// Sender part of the channel for communication to refill
    pub tx_schedule_check_to_refill: AquaSender<bool>,

    /// Receiver part of the channel for communication from refill
    pub rx_schedule_check_from_refill: AquaReceiver<InternalCommand>,

    /// Sender part of the channel for communication to the Balling dosing control
    pub tx_schedule_check_to_balling: AquaSender<bool>,

    /// Receiver part of the channel for communication from the Balling dosing control
    pub rx_schedule_check_from_balling: AquaReceiver<InternalCommand>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_schedule_check_to_signal_handler: AquaSender<bool>,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_schedule_check_from_signal_handler: AquaReceiver<InternalCommand>,
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for ScheduleCheckChannels {
    /// Formats the value using the given formatter.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== ScheduleCheckChannels ===")?;
        writeln!(
            f,
            "tx_schedule_check_to_ventilation: {}",
            self.tx_schedule_check_to_ventilation.count
        )?;
        writeln!(
            f,
            "rx_schedule_check_from_ventilation: {}",
            self.rx_schedule_check_from_ventilation.count
        )?;
        writeln!(
            f,
            "tx_schedule_check_to_heating: {}",
            self.tx_schedule_check_to_heating.count
        )?;
        writeln!(
            f,
            "rx_schedule_check_from_heating: {}",
            self.rx_schedule_check_from_heating.count
        )?;
        writeln!(
            f,
            "tx_schedule_check_to_refill: {}",
            self.tx_schedule_check_to_refill.count
        )?;
        writeln!(
            f,
            "rx_schedule_check_from_refill: {}",
            self.rx_schedule_check_from_refill.count
        )?;
        writeln!(
            f,
            "tx_schedule_check_to_balling: {}",
            self.tx_schedule_check_to_balling.count
        )?;
        writeln!(
            f,
            "rx_schedule_check_from_balling: {}",
            self.rx_schedule_check_from_balling.count
        )?;
        writeln!(
            f,
            "tx_schedule_check_to_signal_handler: {}",
            self.tx_schedule_check_to_signal_handler.count
        )?;
        write!(
            f,
            "rx_schedule_check_from_signal_handler: {}",
            self.rx_schedule_check_from_signal_handler.count
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for ScheduleCheckChannels {
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
