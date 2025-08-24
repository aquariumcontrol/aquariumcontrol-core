/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::utilities::channel_content::InternalCommand;
use std::fmt;
use std::sync::mpsc::{Receiver, SendError, Sender, TryRecvError};

/// Struct acts as a central hub for communication channels used by the DataLogger.
/// It's a way to bundle together all the necessary mpsc (Multi-Producer, Single-Consumer) channel
/// endpoints that the data logger needs to interact with other parts of your application.
pub struct DataLoggerChannels {
    /// Sender part of the channel for communication to the signal handler
    pub tx_to_signal_handler: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_to_signal_handler: u64,

    /// Receiver part of the channel for communication from the signal handler                
    pub rx_from_signal_handler: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_from_signal_handler: u64,
}

impl DataLoggerChannels {
    /// Sends an acknowledgment to the signal handler.
    ///
    /// This wrapper function sends a boolean acknowledgment and increments the
    /// associated debug counter if the `debug_channels` feature is enabled.
    pub fn send_to_signal_handler(&mut self, ack: bool) -> Result<(), SendError<bool>> {
        let result = self.tx_to_signal_handler.send(ack);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_to_signal_handler += 1;
        }
        result
    }

    /// Receives a command from the signal handler.
    ///
    /// This wrapper function performs a blocking receive for a command and increments
    /// the associated debug counter if the `debug_channels` feature is enabled.
    pub fn receive_from_signal_handler(&mut self) -> Result<InternalCommand, TryRecvError> {
        let result = self.rx_from_signal_handler.try_recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_from_signal_handler += 1;
        }
        result
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for DataLoggerChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "cnt_tx_to_signal_handler: {}",
            self.cnt_tx_to_signal_handler
        )?;
        write!(
            f,
            "cnt_rx_from_signal_handler: {}",
            self.cnt_rx_from_signal_handler
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for DataLoggerChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
