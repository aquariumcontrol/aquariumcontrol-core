/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

pub struct WatchDogChannels {
    pub tx_watchdog_to_signal_handler: AquaSender<bool>,

    pub rx_watchdog_from_signal_handler: AquaReceiver<InternalCommand>,

    pub rx_watchdog_from_messaging_opt: Option<AquaReceiver<InternalCommand>>,
}

impl WatchDogChannels {
    /// Sends an acknowledgment to the signal handler.
    ///
    /// This wrapper function sends a boolean acknowledgment and increments the
    /// associated debug counter if the `debug_channels` feature is enabled and
    /// the send operation is successful.
    #[allow(unused)]
    pub fn send_to_signal_handler(&mut self, ack: bool) -> Result<(), AquaChannelError> {
        self.tx_watchdog_to_signal_handler.send(ack)
    }

    /// Receives a command from the signal handler (non-blocking).
    ///
    /// This wrapper function performs a non-blocking receive-operation for a command and increments
    /// the associated debug counter if the `debug_channels` feature is enabled and a
    /// message is successfully received.
    #[allow(unused)]
    pub fn receive_from_signal_handler(&mut self) -> Result<InternalCommand, AquaChannelError> {
        self.rx_watchdog_from_signal_handler.try_recv()
    }

    /// Receives a command from the messaging module, if the channel exists (non-blocking).
    ///
    /// This wrapper function performs a non-blocking receive-operation and increments the
    /// associated debug counter if the `debug_channels` feature is enabled and a
    /// message is successfully received.
    #[allow(unused)]
    pub fn receive_from_messaging(&mut self) -> Option<Result<InternalCommand, AquaChannelError>> {
        self.rx_watchdog_from_messaging_opt
            .as_mut()
            .map(|mut rx| rx.try_recv())
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for WatchDogChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== WatchDogChannels ===")?;
        writeln!(
            f,
            "tx_watchdog_to_signal_handler: {}",
            self.tx_watchdog_to_signal_handler.count
        )?;
        writeln!(
            f,
            "rx_watchdog_from_signal_handler: {}",
            self.rx_watchdog_from_signal_handler.count
        )?;
        if self.rx_watchdog_from_messaging_opt.is_some() {
            write!(
                f,
                "rx_watchdog_from_messaging_opt: {}",
                self.rx_watchdog_from_messaging_opt.as_ref().unwrap().count
            )?;
        }
        Ok(())
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for WatchDogChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
