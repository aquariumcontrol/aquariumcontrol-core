/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::utilities::channel_content::InternalCommand;
use std::fmt;
use std::sync::mpsc::{SendError, Sender};

pub struct ActuateSimulatorChannels {
    pub tx_actuate_simulator_to_tcp_opt: Option<Sender<InternalCommand>>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_actuate_simulator_to_tcp_opt: u64,
}

impl ActuateSimulatorChannels {
    /// Sends a command to the TCP simulator if the channel exists.
    ///
    /// This wrapper function sends a command and increments the associated debug
    /// counter if the `debug_channels` feature is enabled.
    #[allow(unused)]
    pub fn send_to_tcp(
        &mut self,
        command: InternalCommand,
    ) -> Option<Result<(), SendError<InternalCommand>>> {
        self.tx_actuate_simulator_to_tcp_opt.as_ref().map(|tx| {
            #[cfg(feature = "debug_channels")]
            {
                self.cnt_tx_actuate_simulator_to_tcp_opt += 1;
            }
            tx.send(command)
        })
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for ActuateSimulatorChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== ActuateSimulatorChannels ===")?;
        write!(
            f,
            "cnt_tx_actuate_simulator_to_tcp_opt: {}",
            self.cnt_tx_actuate_simulator_to_tcp_opt
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for ActuateSimulatorChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
