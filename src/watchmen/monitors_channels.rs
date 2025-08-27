/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::utilities::channel_content::InternalCommand;
use crate::water::refill_monitor_view::RefillMonitorView;
use std::fmt;
use std::sync::mpsc::{Receiver, SendError, Sender, TryRecvError};

/// Struct collects the channels for communication between Monitors and other components.
pub struct MonitorsChannels {
    /// Receiver part of the channel for communication from Refill
    pub rx_monitors_from_refill: Receiver<RefillMonitorView>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from Refill
    pub cnt_rx_monitors_from_refill: u64,

    /// Sender part of the channel for communication to the signal handler
    pub tx_monitors_to_signal_handler: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to the signal handler
    pub cnt_tx_monitors_to_signal_handler: u64,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_monitors_from_signal_handler: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from the signal handler
    pub cnt_rx_monitors_from_signal_handler: u64,

    /// Receiver part of the channel for communication from the messaging
    pub rx_monitors_from_messaging_opt: Option<Receiver<InternalCommand>>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from messaging
    pub cnt_rx_monitors_from_messaging_opt: u64,
}

impl MonitorsChannels {
    /// Receives a monitor view from the refill module.
    ///
    /// This wrapper performs a non-blocking receive-operation and increments the debug counter on success.
    #[allow(unused)]
    pub fn receive_from_refill(&mut self) -> Result<RefillMonitorView, TryRecvError> {
        let result = self.rx_monitors_from_refill.try_recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_monitors_from_refill += 1;
        }
        result
    }

    /// Sends an acknowledgment to the signal handler.
    ///
    /// This wrapper sends a boolean acknowledgment and increments the debug counter.
    #[allow(unused)]
    pub fn send_to_signal_handler(&mut self, ack: bool) -> Result<(), SendError<bool>> {
        let result = self.tx_monitors_to_signal_handler.send(ack);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_monitors_to_signal_handler += 1;
        }
        result
    }

    /// Receives a command from the signal handler.
    ///
    /// This wrapper performs a non-blocking receive-operation and increments the debug counter on success.
    #[allow(unused)]
    pub fn receive_from_signal_handler(&mut self) -> Result<InternalCommand, TryRecvError> {
        let result = self.rx_monitors_from_signal_handler.try_recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_monitors_from_signal_handler += 1;
        }
        result
    }

    /// Receives a command from messaging if the channel exists.
    ///
    /// This wrapper performs a non-blocking receive-operation and increments the debug counter on success.
    #[allow(unused)]
    pub fn receive_from_messaging(&mut self) -> Option<Result<InternalCommand, TryRecvError>> {
        self.rx_monitors_from_messaging_opt.as_ref().map(|rx| {
            let result = rx.try_recv();
            #[cfg(feature = "debug_channels")]
            if result.is_ok() {
                self.cnt_rx_monitors_from_messaging_opt += 1;
            }
            result
        })
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for MonitorsChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "\ncnt_rx_monitors_from_refill: {}",
            self.cnt_rx_monitors_from_refill
        )?;
        writeln!(
            f,
            "cnt_tx_monitors_to_signal_handler: {}",
            self.cnt_tx_monitors_to_signal_handler
        )?;
        writeln!(
            f,
            "cnt_rx_monitors_from_signal_handler: {}",
            self.cnt_rx_monitors_from_signal_handler
        )?;
        write!(
            f,
            "cnt_rx_monitors_from_messaging_opt: {}",
            self.cnt_rx_monitors_from_messaging_opt
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for MonitorsChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
