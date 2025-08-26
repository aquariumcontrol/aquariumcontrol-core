/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::utilities::channel_content::InternalCommand;
use std::fmt;
use std::sync::mpsc::{Receiver, TryRecvError};

pub struct Ds18b20Channels {
    pub rx_ds18b20_from_signal_handler: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_ds18b20_from_signal_handler: u64,
}

impl Ds18b20Channels {
    /// Receives a command from the signal handler (non-blocking).
    ///
    /// This wrapper function performs a non-blocking receive-operation for a command and increments
    /// the associated debug counter if the `debug_channels` feature is enabled and a
    /// message is successfully received.
    #[allow(unused)]
    pub fn receive_from_signal_handler(&mut self) -> Result<InternalCommand, TryRecvError> {
        let result = self.rx_ds18b20_from_signal_handler.try_recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_ds18b20_from_signal_handler += 1;
        }
        result
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for Ds18b20Channels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "\ncnt_rx_ds18b20_from_signal_handler: {}",
            self.cnt_rx_ds18b20_from_signal_handler
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for Ds18b20Channels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}