/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A central container for all inter-thread communication channels used by the `I2cInterface` thread.
//!
//! This module defines the `I2cInterfaceChannels` struct, which acts as a dedicated
//! "switchboard". Its primary purpose is to aggregate all the `mpsc::channel` senders
//! and receivers that the `I2cInterface` thread needs to communicate with its
//! clients (like `AtlasScientific`) and the main `SignalHandler` for graceful shutdown.
//!
//! The entire module is heavily reliant on conditional compilation (`#[cfg]`) to allow the
//! application to be compiled and tested on platforms without I2C hardware.
//!
//! ## Key Components
//!
//! - **`I2cInterfaceChannels` Struct**: A simple container struct that explicitly
//!   declares the full set of communication dependencies for the `I2cInterface` thread.
//!
//! - **Helper Methods**: Provides convenient wrapper methods, such as
//!   `send_to_atlas_scientific()` and `receive_from_atlas_scientific()`, to offer a
//!   clean, consistent API for channel operations on supported platforms.
//!
//! ## Design and Architecture
//!
//! The `I2cInterfaceChannels` struct is a key part of the application's inter-thread
//! communication strategy, promoting clean and decoupled code.
//!
//! - **Centralization**: By gathering all necessary channels into a single struct, it
//!   provides a clear and explicit declaration of the `I2cInterface` thread's
//!   communication dependencies.
//!
//! - **Decoupling**: The main `I2cInterface` thread logic doesn't need to manage
//!   individual channels. It just needs this struct. This separates the core I2C
//!   bus management logic from the communication infrastructure.
//!
//! - **Platform Abstraction**: The use of `#[cfg]` ensures that fields and methods
//!   related to hardware communication only exist when compiling for the target
//!   device, preventing compilation errors on other systems.
//!
//! - **Testability**: The struct can be easily constructed for use with mock functions during
//!   unit testing, allowing for isolated testing of the `I2cInterface` thread's
//!   logic without needing to run the entire application.

#[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
use crate::launch::channels::AquaChannelError;

use crate::launch::channels::{AquaReceiver, AquaSender};
use crate::sensors::i2c_interface::{I2cRequest, I2cResult};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

/// Container for channels used by I2cInterface
pub struct I2cInterfaceChannels {
    #[allow(unused)] // used in conditionally compiled code
    pub tx_i2c_interface_to_atlas_scientific: AquaSender<I2cResult>,

    #[allow(unused)] // used in conditionally compiled code
    pub rx_i2c_interface_from_atlas_scientific: AquaReceiver<I2cRequest>,

    #[allow(unused)] // used in conditionally compiled code
    pub rx_i2c_interface_from_signal_handler: AquaReceiver<InternalCommand>,
}

impl I2cInterfaceChannels {
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// Sends a result to the Atlas Scientific module.
    pub fn send_to_atlas_scientific(&mut self, result: I2cResult) -> Result<(), AquaChannelError> {
        self.tx_i2c_interface_to_atlas_scientific.send(result)
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Receives a request from the Atlas Scientific module.
    pub fn receive_from_atlas_scientific(&mut self) -> Result<I2cRequest, AquaChannelError> {
        self.rx_i2c_interface_from_atlas_scientific.try_recv()
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
            "rx_i2c_interface_from_atlas_scientific: {}",
            self.rx_i2c_interface_from_atlas_scientific.count
        )?;
        write!(
            f,
            "rx_i2c_interface_from_signal_handler: {}",
            self.rx_i2c_interface_from_signal_handler.count
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
