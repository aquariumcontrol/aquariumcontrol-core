/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::utilities::channel_content::InternalCommand;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::Receiver;
use std::time::Duration;

const LOG_TARGET_WATCHDOG: &str = "watchdog";

/// Holds the configuration data for the watchdog communication.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct WatchdogConfig {
    /// communication will only be active when the flag is set to true
    active: bool,

    /// filename for sending heartbeat to system watchdog
    watchdog_filename: String,

    /// content to be written to watchdog file
    watchdog_heartbeat: String,

    /// duration between two hearbeats in milliseconds
    heartbeat_duration_millis: u64,
}

/// Contains the configuration and the implementation for the Watchdog communication
pub struct Watchdog {
    config: WatchdogConfig,
}

impl Watchdog {
    /// Provides a struct of type Watchdog.
    /// # Arguments
    /// * `config` - configuration data for Watchdog communication
    pub fn new(config: WatchdogConfig) -> Watchdog {
        if config.heartbeat_duration_millis == 0 {
            panic!(
                "{}: duration between two watchdog heartbeats is zero - please assign a value > 0",
                LOG_TARGET_WATCHDOG
            );
        }
        if config.watchdog_filename.is_empty() {
            panic!(
                "{}: watchdog filename is empty - please assign a value",
                LOG_TARGET_WATCHDOG
            );
        }
        if config.watchdog_heartbeat.is_empty() {
            panic!(
                "{}: watchdog heartbeat message is empty - please assign a value",
                LOG_TARGET_WATCHDOG
            );
        }

        Watchdog { config }
    }

    /// Executes the watchdog communication in a loop until signal handler commands termination.
    /// # Arguments
    /// * `rx_from_signal_handler` - receiver part of channel for communication from signal handler                
    /// * `rx_from_messaging_opt` - receiver part of channel communication wrapped in Option from Messaging
    pub fn execute(
        &mut self,
        rx_from_signal_handler: Receiver<InternalCommand>,
        rx_from_messaging_opt: Option<Receiver<InternalCommand>>,
    ) {
        let spin_sleeper = SpinSleeper::default();
        let sleep_duration = Duration::from_millis(self.config.heartbeat_duration_millis);
        let mut quit_command_received: bool; // request to end the application has been received
        let mut stop_command_received: bool; // request to (temporarily) stop watchdog communication has been received
        let mut start_command_received: bool; // request to (temporarily) stop watchdog communication has been received
        let mut inhibited: bool = false;

        loop {
            if self.config.active && !inhibited {
                let _ = std::fs::write(
                    &self.config.watchdog_filename,
                    &self.config.watchdog_heartbeat,
                );
            }
            spin_sleeper.sleep(sleep_duration);
            (
                quit_command_received,
                start_command_received,
                stop_command_received,
            ) = self
                .process_external_request(&rx_from_signal_handler, rx_from_messaging_opt.as_ref());

            if quit_command_received {
                break;
            }
            if start_command_received {
                inhibited = false;
            }
            if stop_command_received {
                inhibited = true;
            }
        }
    }
}
