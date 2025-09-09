/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
use crate::launch::channels::AquaChannelError;

use crate::launch::channels::AquaSender;
use crate::sensors::i2c_interface::{I2cRequest, I2cResult};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;
use std::sync::mpsc::Receiver;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use std::sync::mpsc::TryRecvError;

/// Container for channels used by I2cInterface
pub struct I2cInterfaceChannels {
    #[allow(unused)] // used in conditionally compiled code
    pub tx_i2c_interface_to_atlas_scientific: AquaSender<I2cResult>,

    #[allow(unused)] // used in conditionally compiled code
    pub rx_i2c_interface_from_atlas_scientific: Receiver<I2cRequest>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_i2c_interface_from_atlas_scientific: u64,

    #[allow(unused)] // used in conditionally compiled code
    pub rx_i2c_interface_from_signal_handler: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_i2c_interface_from_signal_handler: u64,
}

impl I2cInterfaceChannels {
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// Sends a result to the Atlas Scientific module.
    pub fn send_to_atlas_scientific(&mut self, result: I2cResult) -> Result<(), AquaChannelError> {
        self.tx_i2c_interface_to_atlas_scientific.send(result)
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
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

#[cfg(feature = "debug_channels")]
impl fmt::Display for I2cInterfaceChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== I2cInterfaceChannels ===")?;
        writeln!(
            f,
            "tx_i2c_interface_to_atlas_scientific: {}",
            self.tx_i2c_interface_to_atlas_scientific.count
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

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for I2cInterfaceChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
