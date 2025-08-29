/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::utilities::channel_content::InternalCommand;
use std::fmt;
use std::sync::mpsc::{Receiver, SendError, Sender, TryRecvError};

/// Container for channels used by relay manager
pub struct RelayManagerChannels {
    /// sender part of the channel for communication to the refill thread
    pub tx_relay_manager_to_refill: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_relay_manager_to_refill: u64,

    /// receiver part of the channel for communication from the refill thread
    pub rx_relay_manager_from_refill: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_relay_manager_from_refill: u64,

    /// sender part of the channel for communication to the heating thread
    pub tx_relay_manager_to_heating: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_relay_manager_to_heating: u64,

    /// receiver part of the channel for communication from the heating thread
    pub rx_relay_manager_from_heating: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_relay_manager_from_heating: u64,

    /// sender part of the channel for communication to the ventilation thread
    pub tx_relay_manager_to_ventilation: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_relay_manager_to_ventilation: u64,

    /// receiver part of the channel for communication from the ventilation thread
    pub rx_relay_manager_from_ventilation: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_relay_manager_from_ventilation: u64,

    /// sender part of the channel for communication to the Balling dosing thread
    pub tx_relay_manager_to_balling: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_relay_manager_to_balling: u64,

    /// receiver part of the channel for communication from the Balling dosing thread
    pub rx_relay_manager_from_balling: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_relay_manager_from_balling: u64,

    /// sender part of the channel for communication to the feed thread
    pub tx_relay_manager_to_feed: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_relay_manager_to_feed: u64,

    /// receiver part of the channel for communication from the feed thread
    pub rx_relay_manager_from_feed: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_relay_manager_from_feed: u64,

    /// sender part of the channel for communication to the signal handler
    pub tx_relay_manager_to_signal_handler: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_relay_manager_to_signal_handler: u64,

    /// receiver part of the channel for communication from the signal handler
    pub rx_relay_manager_from_signal_handler: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_relay_manager_from_signal_handler: u64,
}

impl RelayManagerChannels {
    /// Sends a response to the refill module.
    #[allow(unused)]
    pub fn send_to_refill(&mut self, ack: bool) -> Result<(), SendError<bool>> {
        let result = self.tx_relay_manager_to_refill.send(ack);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_relay_manager_to_refill += 1;
        }
        result
    }

    /// Receives a command from the refill module (non-blocking).
    #[allow(unused)]
    pub fn receive_from_refill(&mut self) -> Result<InternalCommand, TryRecvError> {
        let result = self.rx_relay_manager_from_refill.try_recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_relay_manager_from_refill += 1;
        }
        result
    }

    /// Sends a response to the heating module.
    #[allow(unused)]
    pub fn send_to_heating(&mut self, ack: bool) -> Result<(), SendError<bool>> {
        let result = self.tx_relay_manager_to_heating.send(ack);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_relay_manager_to_heating += 1;
        }
        result
    }

    /// Receives a command from the heating module (non-blocking).
    #[allow(unused)]
    pub fn receive_from_heating(&mut self) -> Result<InternalCommand, TryRecvError> {
        let result = self.rx_relay_manager_from_heating.try_recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_relay_manager_from_heating += 1;
        }
        result
    }

    /// Sends a response to the ventilation module.
    #[allow(unused)]
    pub fn send_to_ventilation(&mut self, ack: bool) -> Result<(), SendError<bool>> {
        let result = self.tx_relay_manager_to_ventilation.send(ack);
        #[cfg(feature = "debug_channels")]
        {
            self.cnt_tx_relay_manager_to_ventilation += 1;
        }
        result
    }

    /// Receives a command from the ventilation module (non-blocking).
    #[allow(unused)]
    pub fn receive_from_ventilation(&mut self) -> Result<InternalCommand, TryRecvError> {
        let result = self.rx_relay_manager_from_ventilation.try_recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_relay_manager_from_ventilation += 1;
        }
        result
    }

    /// Sends a response to the balling module.
    #[allow(unused)]
    pub fn send_to_balling(&mut self, ack: bool) -> Result<(), SendError<bool>> {
        let result = self.tx_relay_manager_to_balling.send(ack);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_relay_manager_to_balling += 1;
        }
        result
    }

    /// Receives a command from the balling module (non-blocking).
    #[allow(unused)]
    pub fn receive_from_balling(&mut self) -> Result<InternalCommand, TryRecvError> {
        let result = self.rx_relay_manager_from_balling.try_recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_relay_manager_from_balling += 1;
        }
        result
    }

    /// Sends a response to the feed module.
    #[allow(unused)]
    pub fn send_to_feed(&mut self, ack: bool) -> Result<(), SendError<bool>> {
        let result = self.tx_relay_manager_to_feed.send(ack);
        #[cfg(feature = "debug_channels")]
        {
            self.cnt_tx_relay_manager_to_feed += 1;
        }
        result
    }

    /// Receives a command from the feed module (non-blocking).
    #[allow(unused)]
    pub fn receive_from_feed(&mut self) -> Result<InternalCommand, TryRecvError> {
        let result = self.rx_relay_manager_from_feed.try_recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_relay_manager_from_feed += 1;
        }
        result
    }

    /// Sends a response to the signal handler.
    #[allow(unused)]
    pub fn send_to_signal_handler(&mut self, ack: bool) -> Result<(), SendError<bool>> {
        let result = self.tx_relay_manager_to_signal_handler.send(ack);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_relay_manager_to_signal_handler += 1;
        }
        result
    }

    /// Receives a command from the signal handler (non-blocking).
    #[allow(unused)]
    pub fn receive_from_signal_handler(&mut self) -> Result<InternalCommand, TryRecvError> {
        let result = self.rx_relay_manager_from_signal_handler.try_recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_relay_manager_from_signal_handler += 1;
        }
        result
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for RelayManagerChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== RelayManagerChannels ===")?;
        writeln!(
            f,
            "cnt_tx_relay_manager_to_refill: {}",
            self.cnt_tx_relay_manager_to_refill
        )?;
        writeln!(
            f,
            "cnt_rx_relay_manager_from_refill: {}",
            self.cnt_rx_relay_manager_from_refill
        )?;
        writeln!(
            f,
            "cnt_tx_relay_manager_to_heating: {}",
            self.cnt_tx_relay_manager_to_heating
        )?;
        writeln!(
            f,
            "cnt_rx_relay_manager_from_heating: {}",
            self.cnt_rx_relay_manager_from_heating
        )?;
        writeln!(
            f,
            "cnt_tx_relay_manager_to_ventilation: {}",
            self.cnt_tx_relay_manager_to_ventilation
        )?;
        writeln!(
            f,
            "cnt_rx_relay_manager_from_ventilation: {}",
            self.cnt_rx_relay_manager_from_ventilation
        )?;
        writeln!(
            f,
            "cnt_tx_relay_manager_to_balling: {}",
            self.cnt_tx_relay_manager_to_balling
        )?;
        writeln!(
            f,
            "cnt_rx_relay_manager_from_balling: {}",
            self.cnt_rx_relay_manager_from_balling
        )?;
        writeln!(
            f,
            "cnt_tx_relay_manager_to_feed: {}",
            self.cnt_tx_relay_manager_to_feed
        )?;
        writeln!(
            f,
            "cnt_rx_relay_manager_from_feed: {}",
            self.cnt_rx_relay_manager_from_feed
        )?;
        writeln!(
            f,
            "cnt_tx_relay_manager_to_signal_handler: {}",
            self.cnt_tx_relay_manager_to_signal_handler
        )?;
        write!(
            f,
            "cnt_rx_relay_manager_from_signal_handler: {}",
            self.cnt_rx_relay_manager_from_signal_handler
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for RelayManagerChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
