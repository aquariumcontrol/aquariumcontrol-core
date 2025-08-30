/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
cfg_if::cfg_if! {
    if #[cfg(target_os = "linux")] {
        use crate::dispatch::messaging::MessagingDomain;
        use crate::dispatch::messaging_error::MessagingError;
        use crate::utilities::channel_content::InternalCommand;
        use std::fmt;
        use std::sync::mpsc::{Receiver, Sender};
    }
}

#[cfg(target_os = "linux")]
/// Collects all `mpsc` channels used by the `Messaging` module for IPC.
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
    pub tx_messaging_to_signal_handler: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to the signal handler.
    pub cnt_tx_messaging_to_signal_handler: u64,

    /// Receiver for commands from the `SignalHandler` (e.g., `Quit`, `Terminate`).
    pub rx_messaging_from_signal_handler: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages received from the signal handler.
    pub cnt_rx_messaging_from_signal_handler: u64,

    /// Sender for dispatching commands to the `Refill` module.
    pub tx_messaging_to_refill: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to refill.
    pub cnt_tx_messaging_to_refill: u64,

    /// Sender for dispatching commands to the `Ventilation` module.
    pub tx_messaging_to_ventilation: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to ventilation.
    pub cnt_tx_messaging_to_ventilation: u64,

    /// Sender for dispatching commands to the `Heating` module.
    pub tx_messaging_to_heating: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to heating.
    pub cnt_tx_messaging_to_heating: u64,

    /// Sender for dispatching commands to the `Feed` module.
    pub tx_messaging_to_feed: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to feed.
    pub cnt_tx_messaging_to_feed: u64,

    /// Sender for dispatching commands to the `Balling` module.
    pub tx_messaging_to_balling: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to balling.
    pub cnt_tx_messaging_to_balling: u64,

    /// Sender for dispatching commands to the `Monitors` module.
    pub tx_messaging_to_monitors: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to monitors.
    pub cnt_tx_messaging_to_monitors: u64,

    /// Sender for dispatching commands to the `Watchdog` module.
    pub tx_messaging_to_watchdog: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    /// Counter for messages sent to watchdog.
    pub cnt_tx_messaging_to_watchdog: u64,
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
            MessagingDomain::Refill => &self.tx_messaging_to_refill,
            MessagingDomain::Ventilation => &self.tx_messaging_to_ventilation,
            MessagingDomain::Feed => &self.tx_messaging_to_feed,
            MessagingDomain::Heating => &self.tx_messaging_to_heating,
            MessagingDomain::Balling => &self.tx_messaging_to_balling,
            MessagingDomain::Monitors => &self.tx_messaging_to_monitors,
            MessagingDomain::Watchdog => &self.tx_messaging_to_watchdog,
            _ => {
                return Err(MessagingError::UnknownDomain(
                    module_path!().to_string(),
                    domain.to_string(),
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

        // Conditionally increment the counter for the corresponding channel on success.
        #[cfg(feature = "debug_channels")]
        {
            let counter = match domain {
                MessagingDomain::Refill => &mut self.cnt_tx_messaging_to_refill,
                MessagingDomain::Ventilation => &mut self.cnt_tx_messaging_to_ventilation,
                MessagingDomain::Feed => &mut self.cnt_tx_messaging_to_feed,
                MessagingDomain::Heating => &mut self.cnt_tx_messaging_to_heating,
                MessagingDomain::Balling => &mut self.cnt_tx_messaging_to_balling,
                MessagingDomain::Monitors => &mut self.cnt_tx_messaging_to_monitors,
                MessagingDomain::Watchdog => &mut self.cnt_tx_messaging_to_watchdog,
                _ => {
                    return Err(MessagingError::UnknownDomain(
                        module_path!().to_string(),
                        domain.to_string(),
                    ));
                }
            };
            *counter += 1;
        }

        Ok(())
    }
}

#[cfg(all(target_os = "linux", feature = "debug_channels"))]
impl fmt::Display for MessagingChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== MessagingChannels ===")?;
        writeln!(
            f,
            "cnt_tx_messaging_to_signal_handler: {}",
            self.cnt_tx_messaging_to_signal_handler
        )?;
        writeln!(
            f,
            "cnt_rx_messaging_from_signal_handler: {}",
            self.cnt_rx_messaging_from_signal_handler
        )?;
        writeln!(
            f,
            "cnt_tx_messaging_to_refill: {}",
            self.cnt_tx_messaging_to_refill
        )?;
        writeln!(
            f,
            "cnt_tx_messaging_to_ventilation: {}",
            self.cnt_tx_messaging_to_ventilation
        )?;
        writeln!(
            f,
            "cnt_tx_messaging_to_heating: {}",
            self.cnt_tx_messaging_to_heating
        )?;
        writeln!(
            f,
            "cnt_tx_messaging_to_feed: {}",
            self.cnt_tx_messaging_to_feed
        )?;
        writeln!(
            f,
            "cnt_tx_messaging_to_balling: {}",
            self.cnt_tx_messaging_to_balling
        )?;
        writeln!(
            f,
            "cnt_tx_messaging_to_monitors: {}",
            self.cnt_tx_messaging_to_monitors
        )?;
        write!(
            f,
            "cnt_tx_messaging_to_watchdog: {}",
            self.cnt_tx_messaging_to_watchdog
        )
    }
}

#[cfg(all(target_os = "linux", not(feature = "debug_channels")))]
impl fmt::Display for MessagingChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
