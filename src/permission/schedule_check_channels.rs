/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::utilities::channel_content::InternalCommand;
use std::fmt;
use std::sync::mpsc::{Receiver, SendError, Sender};

/// Struct collects all channels for communication between ScheduleCheck and other components.
pub struct ScheduleCheckChannels {
    /// Sender part of the channel for communication to ventilation
    pub tx_schedule_check_to_ventilation: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to ventilation
    pub cnt_tx_schedule_check_to_ventilation: u64,

    /// Receiver part of the channel for communication from ventilation
    pub rx_schedule_check_from_ventilation: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from ventilation
    pub cnt_rx_schedule_check_from_ventilation: u64,

    /// Sender part of the channel for communication to heating
    pub tx_schedule_check_to_heating: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to heating
    pub cnt_tx_schedule_check_to_heating: u64,

    /// Receiver part of the channel for communication from heating
    pub rx_schedule_check_from_heating: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from heating
    pub cnt_rx_schedule_check_from_heating: u64,

    /// Sender part of the channel for communication to refill
    pub tx_schedule_check_to_refill: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to refill
    pub cnt_tx_schedule_check_to_refill: u64,

    /// Receiver part of the channel for communication from refill
    pub rx_schedule_check_from_refill: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from refill
    pub cnt_rx_schedule_check_from_refill: u64,

    /// Sender part of the channel for communication to the Balling dosing control
    pub tx_schedule_check_to_balling: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to balling
    pub cnt_tx_schedule_check_to_balling: u64,

    /// Receiver part of the channel for communication from the Balling dosing control
    pub rx_schedule_check_from_balling: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from balling
    pub cnt_rx_schedule_check_from_balling: u64,

    /// Sender part of the channel for communication to the signal handler
    pub tx_schedule_check_to_signal_handler: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to the signal handler
    pub cnt_tx_schedule_check_to_signal_handler: u64,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_schedule_check_from_signal_handler: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from the signal handler
    pub cnt_rx_schedule_check_from_signal_handler: u64,
}

impl ScheduleCheckChannels {

    /// Sends an acknowledgment to the signal handler.
    pub fn send_to_signal_handler(&mut self, ack: bool) -> Result<(), SendError<bool>> {
        #[cfg(feature = "debug_channels")]
        {
            self.cnt_tx_schedule_check_to_signal_handler += 1;
        }
        self.tx_schedule_check_to_signal_handler.send(ack)
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for ScheduleCheckChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "")?;
        writeln!(
            f,
            "cnt_tx_schedule_check_to_ventilation: {}",
            self.cnt_tx_schedule_check_to_ventilation
        )?;
        writeln!(
            f,
            "cnt_rx_schedule_check_from_ventilation: {}",
            self.cnt_rx_schedule_check_from_ventilation
        )?;
        writeln!(
            f,
            "cnt_tx_schedule_check_to_heating: {}",
            self.cnt_tx_schedule_check_to_heating
        )?;
        writeln!(
            f,
            "cnt_rx_schedule_check_from_heating: {}",
            self.cnt_rx_schedule_check_from_heating
        )?;
        writeln!(
            f,
            "cnt_tx_schedule_check_to_refill: {}",
            self.cnt_tx_schedule_check_to_refill
        )?;
        writeln!(
            f,
            "cnt_rx_schedule_check_from_refill: {}",
            self.cnt_rx_schedule_check_from_refill
        )?;
        writeln!(
            f,
            "cnt_tx_schedule_check_to_balling: {}",
            self.cnt_tx_schedule_check_to_balling
        )?;
        writeln!(
            f,
            "cnt_rx_schedule_check_from_balling: {}",
            self.cnt_rx_schedule_check_from_balling
        )?;
        writeln!(
            f,
            "cnt_tx_schedule_check_to_signal_handler: {}",
            self.cnt_tx_schedule_check_to_signal_handler
        )?;
        write!(
            f,
            "cnt_rx_schedule_check_from_signal_handler: {}",
            self.cnt_rx_schedule_check_from_signal_handler
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for ScheduleCheckChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}