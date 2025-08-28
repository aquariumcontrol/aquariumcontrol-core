#[cfg(all(target_os = "linux", feature = "target_hw"))]
/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::sensors::i2c_interface::{I2cRequest, I2cResult};
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use crate::utilities::channel_content::InternalCommand;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use std::fmt;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use std::sync::mpsc::{Receiver, SendError, Sender, TryRecvError};

#[cfg(all(target_os = "linux", feature = "target_hw"))]
/// Container for channels used by I2cInterface
pub struct I2cInterfaceChannels {
    pub tx_i2c_interface_to_atlas_scientific: Sender<I2cResult>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_i2c_interface_to_atlas_scientific: u64,

    pub rx_i2c_interface_from_atlas_scientific: Receiver<I2cRequest>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_i2c_interface_from_atlas_scientific: u64,

    pub rx_i2c_interface_from_signal_handler: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_i2c_interface_from_signal_handler: u64,
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
impl I2cInterfaceChannels {
    /// Sends a result to the Atlas Scientific module.
    pub fn send_to_atlas_scientific(
        &mut self,
        result: I2cResult,
    ) -> Result<(), SendError<I2cResult>> {
        let result = self.tx_i2c_interface_to_atlas_scientific.send(result);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_i2c_interface_to_atlas_scientific += 1;
        }
        result
    }

    /// Receives a request from the Atlas Scientific module.
    pub fn receive_from_atlas_scientific(&mut self) -> Result<I2cRequest, TryRecvError> {
        let result = self.rx_i2c_interface_from_atlas_scientific.try_recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_i2c_interface_from_atlas_scientific += 1;
        }
        result
    }
}

#[cfg(all(target_os = "linux", feature = "debug_channels", feature = "target_hw"))]
impl fmt::Display for I2cInterfaceChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "")?;
        writeln!(
            f,
            "cnt_tx_i2c_interface_to_atlas_scientific: {}",
            self.cnt_tx_i2c_interface_to_atlas_scientific
        )?;
        writeln!(
            f,
            "cnt_rx_i2c_interface_from_atlas_scientific: {}",
            self.cnt_rx_i2c_interface_from_atlas_scientific
        )?;
        write!(
            f,
            "cnt_rx_i2c_interface_from_signal_handler: {}",
            self.cnt_rx_i2c_interface_from_signal_handler
        )
    }
}

#[cfg(all(
    not(feature = "debug_channels"),
    target_os = "linux",
    feature = "target_hw"
))]
impl fmt::Display for I2cInterfaceChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
