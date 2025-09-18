/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A container for the communication channel used by the `ActuateSimulator`.
//!
//! This module defines the `ActuateSimulatorChannels` struct, which acts as a simple
//! "switchboard" for the `ActuateSimulator`. Its purpose is to hold the `mpsc::channel`
//! sender that the simulator uses to broadcast the low-level relay commands it generates.
//!
//! ## Key Components
//!
//! - **`ActuateSimulatorChannels`**: A simple container struct.
//!   - `tx_actuate_simulator_to_tcp_opt`: The sender is wrapped in an `Option` because
//!     the application can also be configured to run without simulator capabilities.
//!
//! ## Design and Architecture
//!
//! This struct is a key part of the application's hardware-free testing strategy.
//!
//! - **Decoupling**: It decouples the `ActuateSimulator` from the test harness that
//!   receives the commands. The simulator only needs to know about this channel
//!   container, not the specifics of the TCP listener.

use crate::launch::channels::AquaSender;
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

pub struct ActuateSimulatorChannels {
    pub tx_actuate_simulator_to_tcp_opt: Option<AquaSender<InternalCommand>>,
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for ActuateSimulatorChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== ActuateSimulatorChannels ===")?;

        let count_str = self
            .tx_actuate_simulator_to_tcp_opt
            .as_ref()
            .map_or_else(
                || "None".to_string(), // Display "None" if the sender doesn't exist
                |sender| sender.count.to_string(), // Display the count if it exists
            );

        write!(f, "tx_actuate_simulator_to_tcp_opt: {}", count_str)
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
