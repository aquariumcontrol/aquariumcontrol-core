/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the channel aggregation struct for the `Watchdog` thread.
//!
//! This module provides the `WatchDogChannels` struct, which acts as a "switchboard"
//! by consolidating all the `mpsc` channel endpoints that the `Watchdog` thread
//! needs to communicate with other application components.
//!
//! ## Purpose and Design
//!
//! The primary purpose of this struct is to provide a single, organized container
//! for the communication infrastructure required by the `Watchdog` thread.
//!
//! - **Centralization**: By gathering all channel senders (`AquaSender`) and receivers
//!   (`AquaReceiver`) into one place, it simplifies the process of passing the
//!   necessary communication lines to the `Watchdog` thread at startup.
//!
//! - **Data Flow**: It defines the pathways for:
//!   - Receiving `Start`/`Stop` commands from the `Messaging` module to inhibit petting.
//!   - Receiving `Quit`/`Terminate` commands from the `signal_handler` for shutdown.
//!   - Sending shutdown acknowledgements back to the `signal_handler`.
//!
//! - **Optionality**: The channel for receiving commands from `Messaging` is wrapped
//!   in an `Option`, allowing the application to be compiled with or without the
//!   messaging feature.
//!
//! ## Usage
//!
//! An instance of `WatchDogChannels` is constructed in the main `launch` module,
//! where all the channels are created. This single struct is then moved into the
//! `Watchdog` thread, giving it all the handles it needs to operate.

use crate::launch::channels::{AquaReceiver, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

pub struct WatchDogChannels {
    /// Sender part of channel to signal handler
    pub tx_watchdog_to_signal_handler: AquaSender<bool>,

    /// Receiver part of channel from signal handler
    pub rx_watchdog_from_signal_handler: AquaReceiver<InternalCommand>,

    /// Receiver part of channel from messaging
    pub rx_watchdog_from_messaging_opt: Option<AquaReceiver<InternalCommand>>,
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
        if let Some(rx_watchdog_from_messaging) = self.rx_watchdog_from_messaging_opt.as_ref() {
            write!(
                f,
                "rx_watchdog_from_messaging_opt: {}",
                rx_watchdog_from_messaging.count
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
