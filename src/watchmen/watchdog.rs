/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages the system's hardware or software watchdog timer.
//!
//! This module provides the `Watchdog` struct, which runs as a dedicated thread
//! responsible for periodically "petting" a system watchdog (e.g., `/dev/watchdog`).
//! Its primary purpose is to ensure the application's liveness. If this
//! thread fails to send its regular heartbeat, the watchdog will trigger a system
//! reset, preventing the application from freezing indefinitely.
//!
//! ## Core Functionality
//!
//! - **Periodic Heartbeats**: The thread runs in a loop, sending a configured
//!   heartbeat message to the watchdog file at a regular interval. This action is
//!   often called "petting the dog."
//!
//! - **Graceful Deactivation**: During a controlled shutdown (initiated by the
//!   `signal_handler`), the thread sends a final deactivation message to the
//!   watchdog. This informs the system that the application is shutting down
//!   intentionally and prevents an unintended reset.
//!
//! - **Runtime Control**: The watchdog's petting can be temporarily inhibited at
//!   runtime by sending `Stop` and `Start` commands via the `Messaging` system.
//!   This is useful for debugging or testing scenarios where a system reset is
//!   desired.
//!
//! - **Robust Validation**: Before starting, the `Watchdog::new()` constructor
//!   performs several critical validation checks on the configuration to prevent
//!   common errors, such as empty messages or a zero-duration heartbeat.
//!
//! ## Platform-Specific Behavior
//!
//! While the code is written to be platform-agnostic and will compile on any
//! target, the actual watchdog functionality is only effective on systems that
//! provide a watchdog interface, such as Linux via `/dev/watchdog`.
//!
//! ## Key Components
//!
//! - **`WatchdogError`**: A `thiserror`-based enum defining all possible
//!   configuration validation failures.
//!
//! - **`Watchdog`**: The main struct that holds the configuration and runs the
//!   main execution loop.
//!
//! - **`new()`**: The constructor that validates the `WatchdogConfig` and returns
//!   a `Result<Watchdog, WatchdogError>`.
//!
//! - **`execute()`**: The entry point and main loop for the thread. It orchestrates
//!   the periodic petting, command processing, and graceful shutdown.
//!
//! - **`PettingTrait`**: An abstraction for the I/O operation of writing to the
//!   watchdog file. This allows for easy mocking and testing of the core logic
//!   without actual filesystem interaction.

use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::watchmen::petting::PettingTrait;
use crate::watchmen::watchdog_channels::WatchDogChannels;
use std::time::Duration;

#[cfg(all(target_os = "linux", not(test), not(feature = "test-utils")))]
// only required in real-life execution
use log::info;

use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::watchmen::watchdog_config::WatchdogConfig;
#[cfg(all(target_os = "linux", not(test), not(feature = "test-utils")))]
// only required in real-life execution
use nix::unistd::gettid;
use thiserror::Error;

/// Contains the error definition for Watchdog
#[derive(Error, Debug)]
pub enum WatchdogError {
    /// Duration between two heartbeats is zero.
    #[error("[{0}] Duration between two watchdog heartbeats is zero - please assign a value > 0")]
    DurationBetweenTwoHeartbeatsIsZero(String),

    /// Watchdog filename is empty.
    #[error("[{0}] Watchdog filename is empty - please assign a value")]
    FilenameIsEmpty(String),

    /// Watchdog heartbeat message is empty.
    #[error("[{0}] Watchdog heartbeat message is empty - please assign a value")]
    HeartbeatMessageIsEmpty(String),

    /// Watchdog deactivation message is empty.
    #[error("[{0}] Watchdog deactivation message is empty - please assign a value")]
    DeactivationMessageIsEmpty(String),

    /// Watchdog heartbeat and deactivation messages are identical.
    #[error("[{0}] watchdog heartbeat and deactivation message are identical ({1}) - please assign different values")]
    HeartbeatAndDeactivationMessagesAreIdentical(String, String),
}

/// Represents the Watchdog responsible for activating and petting the watchdog.
///
/// This struct encapsulates the configuration, interface details, and internal state
/// required to communicate with the operating system.
///
/// Thread Communication:
/// This module is designed to run in its own dedicated thread.
/// It reads commands provided by the messaging system and acts accordingly.
/// It responds to `Quit` commands for graceful shutdown.
///
/// Platform-Specific Behavior:
/// The watchdog logic will only work on Linux systems but compile for any other system as well.
///
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     messaging[Messaging] --> ds18b20[Ds18b20]
///     signal_handler[SignalHandler] --> ds18b20
/// ```
pub struct Watchdog {
    config: WatchdogConfig,
}

impl Watchdog {
    /// Creates a new `Watchdog` instance.
    ///
    /// This constructor initializes the watchdog communication module with its specified
    /// configuration. It performs critical **validation checks** on the configuration
    /// parameters to ensure the watchdog can operate correctly and safely.
    ///
    /// **Validation checks performed: **
    /// - `heartbeat_duration_millis` must be greater than zero.
    /// - `watchdog_filename` cannot be an empty string.
    /// - `watchdog_heartbeat` message cannot be an empty string.
    /// - `watchdog_deactivation` message cannot be an empty string.
    /// - `watchdog_heartbeat` and `watchdog_deactivation` messages must be different.
    ///
    /// # Arguments
    /// * `config` - **Configuration data** for the watchdog communication, loaded from a TOML file.
    ///   This includes filename, heartbeat messages, and timing parameters.
    ///
    /// # Returns
    /// A new **`Watchdog` struct**, ready for sending heartbeats or respond to termination requests.
    ///
    /// # Panics
    /// This function will **panic** if any of the configuration validation checks fail,
    /// as an incorrectly configured watchdog can lead to system instability or unintended resets.
    ///
    /// [impl->swreq~watchmen_pet_watchdog~1]
    pub fn new(config: WatchdogConfig) -> Result<Watchdog, WatchdogError> {
        if config.heartbeat_duration_millis == 0 {
            return Err(WatchdogError::DurationBetweenTwoHeartbeatsIsZero(
                module_path!().to_string(),
            ));
        }
        if config.watchdog_filename.is_empty() {
            return Err(WatchdogError::FilenameIsEmpty(module_path!().to_string()));
        }
        if config.watchdog_heartbeat.is_empty() {
            return Err(WatchdogError::HeartbeatMessageIsEmpty(
                module_path!().to_string(),
            ));
        }
        if config.watchdog_deactivation.is_empty() {
            return Err(WatchdogError::DeactivationMessageIsEmpty(
                module_path!().to_string(),
            ));
        }
        if config.watchdog_deactivation == config.watchdog_heartbeat {
            return Err(WatchdogError::HeartbeatAndDeactivationMessagesAreIdentical(
                module_path!().to_string(),
                config.watchdog_deactivation,
            ));
        }

        Ok(Watchdog { config })
    }

    /// Executes the main control loop for the watchdog communication module.
    ///
    /// This function runs continuously, managing the periodic sending of heartbeats
    /// to a system watchdog. It ensures the application's liveness is maintained
    /// and handles various external commands for controlling its operation.
    ///
    /// **Key Operations:**
    /// - **Heartbeat Sending**: If `active` and not `inhibited`, it calls the `petting.pet()`
    ///   method to send a heartbeat message to the configured `watchdog_filename`.
    /// - **Cycle Management**: It sleeps for the configured `heartbeat_duration_millis`
    ///   between each heartbeat.
    /// - **External Control**: It processes `Start` and `Stop` commands received from
    ///   messaging or the signal handler to `inhibit` or re-enable watchdog activity.
    /// - **Graceful Deactivation**: Upon receiving a `Quit` command, it sends a final
    ///   `watchdog_deactivation` message before exiting the loop.
    /// - **Shutdown Confirmation**: After exiting the loop, it sends a confirmation back
    ///   to the signal handler.
    ///
    /// # Arguments
    /// * `watchdog_channels` - A mutable reference to the struct containing the channels.
    /// * `petting` - A mutable reference to an object implementing the `PettingTrait`,
    ///   responsible for the actual writing of heartbeat/deactivation messages to the watchdog file.
    ///
    /// [impl->swreq~watchmen_pet_watchdog~1]
/// [impl->swreq~watchmen_reboot_on_stall~1]
    pub async fn execute(
        &mut self,
        watchdog_channels: &mut WatchDogChannels,
        petting: &mut impl PettingTrait,
    ) {
        #[cfg(all(target_os = "linux", not(test), not(feature = "test-utils")))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let sleep_duration = Duration::from_millis(self.config.heartbeat_duration_millis);
        let mut quit_command_received: bool; // the request to end the application has been received
        let mut stop_command_received: bool; // the request to (temporarily) stop watchdog communication has been received
        let mut start_command_received: bool; // the request to (temporarily) stop watchdog communication has been received
        let mut inhibited: bool = false;

        loop {
            if self.config.active && !inhibited {
                // Send the heartbeat to the watchdog
                petting.pet(
                    &self.config.watchdog_filename,
                    &self.config.watchdog_heartbeat,
                );
            }
            tokio::time::sleep(sleep_duration).await;
            (
                quit_command_received,
                start_command_received,
                stop_command_received,
            ) = self.process_external_request(
                &mut watchdog_channels.rx_watchdog_from_signal_handler,
                Some(&mut watchdog_channels.rx_watchdog_from_messaging),
            );

            if quit_command_received {
                if self.config.active {
                    // Deactivate the watchdog functionality
                    petting.pet(
                        &self.config.watchdog_filename,
                        &self.config.watchdog_deactivation,
                    );
                }
                break;
            }

            // For testing purposes, allow deactivation of the watchdog communication.
            // This will trigger a reset of the system.
            if start_command_received {
                inhibited = false;
            }
            if stop_command_received {
                inhibited = true;
            }
        }

        watchdog_channels.acknowledge_signal_handler();
    }
}
