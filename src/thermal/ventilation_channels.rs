/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

/// Struct collects the channels for communication between Ventilation and other components.
pub struct VentilationChannels {
    /// Sender part of the channel for communication to relay manager
    pub tx_ventilation_to_relay_manager: AquaSender<InternalCommand>,

    /// Receiver part of the channel for communication from the relay manager
    pub rx_ventilation_from_relay_manager: AquaReceiver<bool>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_ventilation_to_signal_handler: AquaSender<bool>,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_ventilation_from_signal_handler: AquaReceiver<InternalCommand>,

    /// Sender part of the channel for communication to the schedule checker
    pub tx_ventilation_to_schedule_check: AquaSender<InternalCommand>,

    /// Receiver part of the channel for communication from the schedule checker
    pub rx_ventilation_from_schedule_check: AquaReceiver<bool>,

    /// Receiver part of the channel for communication from the messaging
    pub rx_ventilation_from_messaging_opt: Option<AquaReceiver<InternalCommand>>,
}

impl VentilationChannels {
    /// Sends a command to the relay manager.
    pub fn send_to_relay_manager(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_ventilation_to_relay_manager.send(command)
    }

    /// Receives a response from the relay manager.
    pub fn receive_from_relay_manager(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_ventilation_from_relay_manager.recv()
    }

    /// Sends a command to the schedule checker.
    pub fn send_to_schedule_check(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_ventilation_to_schedule_check.send(command)
    }

    /// Receives a response from the schedule checker.
    pub fn receive_from_schedule_check(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_ventilation_from_schedule_check.recv()
    }

    /// Sends a command to the schedule checker.
    pub fn send_to_signal_handler(&mut self, status: bool) -> Result<(), AquaChannelError> {
        self.tx_ventilation_to_signal_handler.send(status)
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for VentilationChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== VentilationChannels ===")?;
        writeln!(
            f,
            "tx_ventilation_to_relay_manager: {}",
            self.tx_ventilation_to_relay_manager.count
        )?;
        writeln!(
            f,
            "rx_ventilation_from_relay_manager: {}",
            self.rx_ventilation_from_relay_manager.count
        )?;
        writeln!(
            f,
            "tx_ventilation_to_signal_handler: {}",
            self.tx_ventilation_to_signal_handler.count
        )?;
        writeln!(
            f,
            "rx_ventilation_from_signal_handler: {}",
            self.rx_ventilation_from_signal_handler.count
        )?;
        writeln!(
            f,
            "tx_ventilation_to_schedule_check: {}",
            self.tx_ventilation_to_schedule_check.count
        )?;
        writeln!(
            f,
            "rx_ventilation_from_schedule_check: {}",
            self.rx_ventilation_from_schedule_check.count
        )?;
        write!(
            f,
            "rx_ventilation_from_messaging_opt: {}",
            self.rx_ventilation_from_messaging_opt
                .as_ref()
                .unwrap()
                .count
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for VentilationChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
