/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A central container for all inter-thread communication channels used by the `Messaging` module.
//!
//! This module defines the `MessagingChannels` struct, which acts as a "switchboard" or
//! router for commands received from an external source. Its primary purpose is to hold all
//! the `mpsc::channel` senders that the main `Messaging` thread needs to dispatch commands
//! to the various application domains (e.g., `Refill`, `Heating`, `Feed`).
//!
//! This entire module is conditionally compiled and is only available on `target_os = "linux"`
//! (or during tests), as it is a core part of the POSIX message queue IPC mechanism.
//!
//! ## Key Components
//!
//! - **`MessagingChannels` Struct**: A simple container struct that aggregates all the
//!   `AquaSender` and `AquaReceiver` endpoints required by the `Messaging` thread.
//!
//! - **`send_command_to_domain()` Method**: The core logic of this module. It takes an
//!   `InternalCommand` and a `MessagingDomain` and uses a `match` statement to select
//!   the correct channel sender, effectively routing the command to the intended thread.
//!
//! ## Design and Architecture
//!
//! The `MessagingChannels` struct is a key part of the application's inter-thread
//! communication strategy, promoting clean and decoupled code.
//!
//! - **Centralization**: By gathering all necessary channels into a single struct, it
//!   provides a clear and explicit declaration of the `Messaging` thread's communication
//!   dependencies. This makes the overall architecture easier to reason about.
//!
//! - **Decoupling**: The `Messaging` thread itself doesn't need to know about every
//!   channel. It just needs this struct, which it can use to dispatch commands.
//!   This separates the dispatch logic from the communication infrastructure.
//!
//! - **Testability**: The struct's channels can be used by mock implementation during
//!   unit testing, allowing for isolated testing of the `Feed` thread's logic without
//!   needing to run the entire application.

#[cfg(any(test, target_os = "linux"))]
use crate::launch::channels::AquaReceiver;
#[allow(unused)]
use crate::launch::channels::AquaSender;
#[cfg(any(test, target_os = "linux"))]
use crate::utilities::channel_content::InternalCommand;

cfg_if::cfg_if! {
    if #[cfg(target_os = "linux")] {
        use std::fmt;
        use crate::dispatch::messaging_domain::MessagingDomain;
        use crate::dispatch::messaging_error::MessagingError;
    }
}

#[cfg(any(test, target_os = "linux"))]
/// Collects all channels used by the `Messaging` module for IPC.
///
/// This struct acts as a central container for all the communication endpoints
/// that the `Messaging` module uses to dispatch commands received via a POSIX
/// message queue to the appropriate application threads (domains). It also
/// includes channels for communicating with the `SignalHandler` for graceful shutdown.
///
/// This entire struct is conditionally compiled and is only available on Linux
/// systems, as POSIX message queues are a platform-specific feature.
pub struct MessagingChannels {
    /// Sender for sending acknowledgments back to the `SignalHandler`.
    pub tx_messaging_to_signal_handler: AquaSender<bool>,

    /// Receiver for commands from the `SignalHandler` (e.g., `Quit`, `Terminate`).
    pub rx_messaging_from_signal_handler: AquaReceiver<InternalCommand>,

    /// Sender for dispatching commands to the `Refill` module.
    pub tx_messaging_to_refill: AquaSender<InternalCommand>,

    /// Sender for dispatching commands to the `Ventilation` module.
    pub tx_messaging_to_ventilation: AquaSender<InternalCommand>,

    /// Sender for dispatching commands to the `Heating` module.
    pub tx_messaging_to_heating: AquaSender<InternalCommand>,

    /// Sender for dispatching commands to the `Feed` module.
    pub tx_messaging_to_feed: AquaSender<InternalCommand>,

    /// Sender for dispatching commands to the `Balling` module.
    pub tx_messaging_to_balling: AquaSender<InternalCommand>,

    #[allow(unused)]
    /// Sender for dispatching commands to the `Monitors` module.
    pub tx_messaging_to_monitors: AquaSender<InternalCommand>,

    #[allow(unused)]
    /// Sender for dispatching commands to the `Watchdog` module.
    pub tx_messaging_to_watchdog: AquaSender<InternalCommand>,
}

#[cfg(target_os = "linux")]
impl MessagingChannels {
    /// Dispatches a command to the appropriate application domain (thread).
    ///
    /// This function acts as a router, taking a generic `InternalCommand` and a
    /// `MessagingDomain` and sending the command to the correct channel based on
    /// the specified domain. It also handles the conditional incrementing of debug
    /// counters when the `debug_channels` feature is enabled.
    ///
    /// # Arguments
    /// * `command` - The `InternalCommand` (e.g., `Start`, `Stop`) to be sent.
    /// * `domain` - The target `MessagingDomain` (e.g., `Refill`, `Heating`) that
    ///   should receive the command.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the command was successfully sent.
    ///
    /// # Errors
    /// Returns a `MessagingError` if:
    /// - The `domain` is `MessagingDomain::Unknown`.
    /// - The target channel is disconnected (i.e., the receiving thread has terminated),
    ///   wrapped in `MessagingError::ChannelSendError`.
    pub fn send_command_to_domain(
        &mut self,
        command: InternalCommand,
        domain: MessagingDomain,
    ) -> Result<(), MessagingError> {
        // Select the correct sender channel based on the domain.
        let tx_messaging_to_domain = match domain {
            MessagingDomain::Refill => &mut self.tx_messaging_to_refill,
            MessagingDomain::Ventilation => &mut self.tx_messaging_to_ventilation,
            MessagingDomain::Feed => &mut self.tx_messaging_to_feed,
            MessagingDomain::Heating => &mut self.tx_messaging_to_heating,
            MessagingDomain::Balling => &mut self.tx_messaging_to_balling,
            MessagingDomain::Monitors => &mut self.tx_messaging_to_monitors,
            MessagingDomain::Watchdog => &mut self.tx_messaging_to_watchdog,
            _ => {
                return Err(MessagingError::UnknownDomain(
                    module_path!().to_string(),
                    domain as i32,
                ));
            }
        };

        // Send the command and capture the result.
        tx_messaging_to_domain
            .send(command)
            .map_err(|e| MessagingError::ChannelSendError {
                location: module_path!().to_string(),
                source: e,
            })?;

        Ok(())
    }
}

#[cfg(all(feature = "debug_channels", target_os = "linux"))]
impl fmt::Display for MessagingChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== MessagingChannels ===")?;
        writeln!(
            f,
            "tx_messaging_to_signal_handler: {}",
            self.tx_messaging_to_signal_handler.count
        )?;
        writeln!(
            f,
            "rx_messaging_from_signal_handler: {}",
            self.rx_messaging_from_signal_handler.count
        )?;
        writeln!(
            f,
            "tx_messaging_to_refill: {}",
            self.tx_messaging_to_refill.count
        )?;
        writeln!(
            f,
            "tx_messaging_to_ventilation: {}",
            self.tx_messaging_to_ventilation.count
        )?;
        writeln!(
            f,
            "tx_messaging_to_heating: {}",
            self.tx_messaging_to_heating.count
        )?;
        writeln!(
            f,
            "tx_messaging_to_feed: {}",
            self.tx_messaging_to_feed.count
        )?;
        writeln!(
            f,
            "tx_messaging_to_balling: {}",
            self.tx_messaging_to_balling.count
        )?;
        writeln!(
            f,
            "tx_messaging_to_monitors: {}",
            self.tx_messaging_to_monitors.count
        )?;
        write!(
            f,
            "tx_messaging_to_watchdog: {}",
            self.tx_messaging_to_watchdog.count
        )
    }
}

#[cfg(all(not(feature = "debug_channels"), target_os = "linux"))]
impl fmt::Display for MessagingChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
