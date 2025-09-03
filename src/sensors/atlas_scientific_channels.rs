/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::sensors::i2c_interface::{I2cRequest, I2cResult};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;
use std::sync::mpsc::{Receiver, SendError, Sender, TryRecvError};

/// Struct collects the channels for the communication between AtlasScientific and other components.
pub struct AtlasScientificChannels {
    /// Sender part of the channel for communication to I2CInterface
    #[allow(unused)] // used in conditionally compiled code
    pub tx_atlas_scientific_to_i2c_interface: Sender<I2cRequest>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to the I2C interface
    pub cnt_tx_atlas_scientific_to_i2c_interface: u64,

    /// Receiver part of the channel for communication from I2CInterface
    #[allow(unused)] // used in conditionally compiled code
    pub rx_atlas_scientific_from_i2c_interface: Receiver<I2cResult>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from the I2C interface
    pub cnt_rx_atlas_scientific_from_i2c_interface: u64,

    /// Sender part of the channel for communication to the signal handler
    #[allow(unused)] // used in conditionally compiled code
    pub tx_atlas_scientific_to_signal_handler: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to the signal handler
    pub cnt_tx_atlas_scientific_to_signal_handler: u64,

    /// Receiver part of the channel for communication from the signal handler
    #[allow(unused)] // used in conditionally compiled code
    pub rx_atlas_scientific_from_signal_handler: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from the signal handler
    pub cnt_rx_atlas_scientific_from_signal_handler: u64,
}

impl AtlasScientificChannels {
    /// Sends a request to the I2C interface.
    #[allow(unused)] // used in conditionally compiled code
    pub fn send_to_i2c_interface(
        &mut self,
        request: I2cRequest,
    ) -> Result<(), SendError<I2cRequest>> {
        let result = self.tx_atlas_scientific_to_i2c_interface.send(request);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_atlas_scientific_to_i2c_interface += 1;
        }
        result
    }

    /// Receives a result from the I2C interface.
    #[allow(unused)] // used in conditionally compiled code
    pub fn receive_from_i2c_interface(&mut self) -> Result<I2cResult, TryRecvError> {
        let result = self.rx_atlas_scientific_from_i2c_interface.try_recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_atlas_scientific_from_i2c_interface += 1;
        }
        result
    }

    /// Sends a request to the I2C interface.
    pub fn send_to_signal_handler(
        &mut self,
        status: bool,
    ) -> Result<(), SendError<bool>> {
        let result = self.tx_atlas_scientific_to_signal_handler.send(status);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_atlas_scientific_to_signal_handler += 1;
        }
        result
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for AtlasScientificChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== AtlasScientificChannels ===")?;
        writeln!(
            f,
            "cnt_tx_atlas_scientific_to_i2c_interface: {}",
            self.cnt_tx_atlas_scientific_to_i2c_interface
        )?;
        writeln!(
            f,
            "cnt_rx_atlas_scientific_from_i2c_interface: {}",
            self.cnt_rx_atlas_scientific_from_i2c_interface
        )?;
        writeln!(
            f,
            "cnt_tx_atlas_scientific_to_signal_handler: {}",
            self.cnt_tx_atlas_scientific_to_signal_handler
        )?;
        write!(
            f,
            "cnt_rx_atlas_scientific_from_signal_handler: {}",
            self.cnt_rx_atlas_scientific_from_signal_handler
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for AtlasScientificChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
