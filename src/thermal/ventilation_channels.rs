/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::utilities::channel_content::InternalCommand;
use std::fmt;
use std::sync::mpsc::{Receiver, RecvError, SendError, Sender};

/// Struct collects the channels for communication between Ventilation and other components.
pub struct VentilationChannels {
    /// Sender part of the channel for communication to relay manager
    pub tx_ventilation_to_relay_manager: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to the relay manager
    pub cnt_tx_ventilation_to_relay_manager: u64,

    /// Receiver part of the channel for communication from the relay manager
    pub rx_ventilation_from_relay_manager: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from the relay manager
    pub cnt_rx_ventilation_from_relay_manager: u64,

    /// Sender part of the channel for communication to the signal handler
    pub tx_ventilation_to_signal_handler: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to the signal handler
    pub cnt_tx_ventilation_to_signal_handler: u64,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_ventilation_from_signal_handler: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from the signal handler
    pub cnt_rx_ventilation_from_signal_handler: u64,

    /// Sender part of the channel for communication to the schedule checker
    pub tx_ventilation_to_schedule_check: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to the schedule checker
    pub cnt_tx_ventilation_to_schedule_check: u64,

    /// Receiver part of the channel for communication from the schedule checker
    pub rx_ventilation_from_schedule_check: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from the schedule checker
    pub cnt_rx_ventilation_from_schedule_check: u64,

    /// Receiver part of the channel for communication from the messaging
    pub rx_ventilation_from_messaging_opt: Option<Receiver<InternalCommand>>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from messaging
    pub cnt_rx_ventilation_from_messaging_opt: u64,
}

impl VentilationChannels {
    /// Sends a command to the relay manager.
    pub fn send_to_relay_manager(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_ventilation_to_relay_manager.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_ventilation_to_relay_manager += 1;
        }
        result
    }

    /// Receives a response from the relay manager.
    pub fn receive_from_relay_manager(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_ventilation_from_relay_manager.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_ventilation_from_relay_manager += 1;
        }
        result
    }

    /// Sends a command to the schedule checker.
    pub fn send_to_schedule_check(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_ventilation_to_schedule_check.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_ventilation_to_schedule_check += 1;
        }
        result
    }

    /// Receives a response from the schedule checker.
    pub fn receive_from_schedule_check(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_ventilation_from_schedule_check.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_ventilation_from_schedule_check += 1;
        }
        result
    }

    /// Sends a command to the schedule checker.
    pub fn send_to_signal_handler(&mut self, status: bool) -> Result<(), SendError<bool>> {
        let result = self.tx_ventilation_to_signal_handler.send(status);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_ventilation_to_schedule_check += 1;
        }
        result
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for VentilationChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== VentilationChannels ===")?;
        writeln!(
            f,
            "cnt_tx_ventilation_to_relay_manager: {}",
            self.cnt_tx_ventilation_to_relay_manager
        )?;
        writeln!(
            f,
            "cnt_rx_ventilation_from_relay_manager: {}",
            self.cnt_rx_ventilation_from_relay_manager
        )?;
        writeln!(
            f,
            "cnt_tx_ventilation_to_signal_handler: {}",
            self.cnt_tx_ventilation_to_signal_handler
        )?;
        writeln!(
            f,
            "cnt_rx_ventilation_from_signal_handler: {}",
            self.cnt_rx_ventilation_from_signal_handler
        )?;
        writeln!(
            f,
            "cnt_tx_ventilation_to_schedule_check: {}",
            self.cnt_tx_ventilation_to_schedule_check
        )?;
        writeln!(
            f,
            "cnt_rx_ventilation_from_schedule_check: {}",
            self.cnt_rx_ventilation_from_schedule_check
        )?;
        write!(
            f,
            "cnt_rx_ventilation_from_messaging_opt: {}",
            self.cnt_rx_ventilation_from_messaging_opt
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
