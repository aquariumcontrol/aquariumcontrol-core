/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(target_os = "linux")]
use crate::utilities::channel_content::InternalCommand;
#[cfg(target_os = "linux")]
use std::fmt;
#[cfg(target_os = "linux")]
use std::sync::mpsc::{Receiver, Sender};

#[cfg(target_os = "linux")]
pub struct MessagingChannels {
    pub tx_to_signal_handler: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to the signal handler
    pub cnt_tx_messaging_to_signal_handler: u64,

    pub rx_from_signal_handler: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from the signal handler
    pub cnt_rx_messaging_from_signal_handler: u64,

    pub tx_to_refill: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to refill
    pub cnt_tx_messaging_to_refill: u64,

    pub tx_to_ventilation: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to ventilation
    pub cnt_tx_messaging_to_ventilation: u64,

    pub tx_to_heating: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to heating
    pub cnt_tx_messaging_to_heating: u64,

    pub tx_to_feed: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to feed
    pub cnt_tx_messaging_to_feed: u64,

    pub tx_to_balling: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to balling
    pub cnt_tx_messaging_to_balling: u64,

    pub tx_to_monitors: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to monitors
    pub cnt_tx_messaging_to_monitors: u64,

    pub tx_to_watchdog: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to watchdog
    pub cnt_tx_messaging_to_watchdog: u64,
}

#[cfg(all(target_os = "linux", feature = "debug_channels"))]
impl fmt::Display for MessagingChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "")?;
        writeln!(
            f,
            "cnt_tx_to_signal_handler: {}",
            self.cnt_tx_messaging_to_signal_handler
        )?;
        writeln!(
            f,
            "cnt_rx_from_signal_handler: {}",
            self.cnt_rx_messaging_from_signal_handler
        )?;
        writeln!(f, "cnt_tx_to_refill: {}", self.cnt_tx_messaging_to_refill)?;
        writeln!(
            f,
            "cnt_tx_to_ventilation: {}",
            self.cnt_tx_messaging_to_ventilation
        )?;
        writeln!(f, "cnt_tx_to_heating: {}", self.cnt_tx_messaging_to_heating)?;
        writeln!(f, "cnt_tx_to_feed: {}", self.cnt_tx_messaging_to_feed)?;
        writeln!(f, "cnt_tx_to_balling: {}", self.cnt_tx_messaging_to_balling)?;
        writeln!(
            f,
            "cnt_tx_to_monitors: {}",
            self.cnt_tx_messaging_to_monitors
        )?;
        write!(
            f,
            "cnt_tx_to_watchdog: {}",
            self.cnt_tx_messaging_to_watchdog
        )
    }
}

#[cfg(all(target_os = "linux", not(feature = "debug_channels")))]
impl fmt::Display for MessagingChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
