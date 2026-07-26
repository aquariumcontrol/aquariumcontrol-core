/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the channel aggregation struct for the `Refill` thread.
//!
//! This module provides the `RefillChannels` struct, which acts as a "switchboard"
//! by consolidating all the `mpsc` channel endpoints that the `Refill` thread
//! needs to communicate with other application components.
//!
//! ## Purpose and Design
//!
//! The primary purpose of this struct is to provide a single, organized container
//! for the communication infrastructure required by the `Refill` thread.
//!
//! - **Centralization**: By gathering all channel senders (`AquaSender`) and receivers
//!   (`AquaReceiver`) into one place, it simplifies the process of passing the
//!   necessary communication lines to the `Refill` thread at startup.
//!
//! - **Data Flow**: It defines the pathways for:
//!   - Sending `CheckSchedule` commands to the `ScheduleCheck` module and receiving `bool` responses.
//!   - Sending `Activate`/`Deactivate` commands to the `RelayManager` and receiving `bool` acknowledgements.
//!   - Sending `RefillMonitorView` data to the `Monitors` thread.
//!   - Receiving `Start`/`Stop` commands from the `Messaging` module.
//!   - Receiving `Quit`/`Terminate` commands from the `signal_handler` and sending back acknowledgements.
//!
//! - **Optionality**: The channel for receiving commands from `Messaging` is wrapped
//!   in an `Option`, allowing the application to be compiled with or without the
//!   messaging feature.

use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use crate::water::refill_monitor_view::RefillMonitorView;
use std::fmt;

/// Struct collects the channels for the communication between the refill control and the other components.
/// Additionally, the struct contains counters for debug purposes.
/// The implementation of the struct contains functions for those channels which are frequently used.
/// The functions increment the usage counters.
pub struct RefillChannels {
    /// Sender part of the channel for communication to relay manager
    pub tx_refill_to_relay_manager: AquaSender<InternalCommand>,

    /// Receiver part of the channel for communication from the relay manager
    pub rx_refill_from_relay_manager: AquaReceiver<bool>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_refill_to_signal_handler: AquaSender<bool>,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_refill_from_signal_handler: AquaReceiver<InternalCommand>,

    /// Sender part of the channel for communication to the schedule checker
    pub tx_refill_to_schedule_check: AquaSender<InternalCommand>,

    /// Receiver part of the channel for communication from the schedule checker
    pub rx_refill_from_schedule_check: AquaReceiver<bool>,

    /// Receiver part of the channel for communication from the messaging
    pub rx_refill_from_messaging: AquaReceiver<InternalCommand>,

    /// Sender part of the channel for communication to the monitors
    pub tx_refill_to_monitors: AquaSender<RefillMonitorView>,
}

impl RefillChannels {
    /// Performs the `send_to_relay_manager` operation.
    ///
    /// # Arguments
    /// * `command` - The command string to verify.
    /// [impl->swreq~water_channel_communication~1]
    pub fn send_to_relay_manager(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_refill_to_relay_manager.send(command)
    }

    /// Performs the `receive_from_relay_manager` operation.
    pub async fn receive_from_relay_manager(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_refill_from_relay_manager.recv().await
    }

    /// Performs the `send_to_schedule_check` operation.
    ///
    /// # Arguments
    /// * `command` - The command string to verify.
    pub fn send_to_schedule_check(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_refill_to_schedule_check.send(command)
    }

    /// Performs the `receive_from_schedule_check` operation.
    pub async fn receive_from_schedule_check(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_refill_from_schedule_check.recv().await
    }

    /// Performs the `send_to_monitor` operation.
    ///
    /// # Arguments
    /// * `view` - A parameter representing `view`.
    pub fn send_to_monitor(&mut self, view: RefillMonitorView) -> Result<(), AquaChannelError> {
        self.tx_refill_to_monitors.send(view)
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for RefillChannels {
    /// Formats the value using the given formatter.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== RefillChannels ===")?;
        writeln!(
            f,
            "tx_refill_to_relay_manager: {}",
            self.tx_refill_to_relay_manager.count
        )?;
        writeln!(
            f,
            "rx_refill_from_relay_manager: {}",
            self.rx_refill_from_relay_manager.count
        )?;
        writeln!(
            f,
            "tx_refill_to_signal_handler: {}",
            self.tx_refill_to_signal_handler.count
        )?;
        writeln!(
            f,
            "rx_refill_from_signal_handler: {}",
            self.rx_refill_from_signal_handler.count
        )?;
        writeln!(
            f,
            "tx_refill_to_schedule_check: {}",
            self.tx_refill_to_schedule_check.count
        )?;
        writeln!(
            f,
            "rx_refill_from_schedule_check: {}",
            self.rx_refill_from_schedule_check.count
        )?;
        if let Some(rx_refill_from_messaging) = self.rx_refill_from_messaging_opt.as_ref() {
            writeln!(
                f,
                "rx_refill_from_messaging_opt: {}",
                rx_refill_from_messaging.count
            )?;
        }
        write!(
            f,
            "tx_refill_to_monitors: {}",
            self.tx_refill_to_monitors.count
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for RefillChannels {
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
