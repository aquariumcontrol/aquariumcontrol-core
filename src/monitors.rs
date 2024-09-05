/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::{debug, error, info, warn};
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::channel_content::InternalCommand;
use crate::proc_ext_req::ProcessExternalRequestTrait;

/// Holds the configuration data for the monitors.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct MonitorsConfig {
    /// monitors will only be active when the flag is set to true
    active: bool,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the monitors.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     monitors[Monitors] --> gpio_handler[GPIO handler]
///     gpio_handler --> monitors
///     monitors[Monitors] --> controllino[Controllino]
///     controllino --> monitors
///     monitors --> signal_handler[Signal handler]
///     signal_handler --> monitors
///     messaging[Messaging] --> monitors
/// ```

pub struct Monitors {
    /// configuration data for monitors
    config: MonitorsConfig,
}

impl ProcessExternalRequestTrait for Monitors {}

impl Monitors {
    const LOG_TARGET_MONITORS: &'static str = "monitors";

    /// Provides a struct of type Monitors.
    /// # Arguments
    /// * `config` - configuration data for monitors
    pub fn new(config: MonitorsConfig) -> Monitors {
        Self { config: config }
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `mutex_device_scheduler_monitors` - clone of the mutex for avoiding parallel device actuation.
    /// * `tx_to_gpio` - Sender part of channel for communication to GPIO handler
    /// * `rx_from_gpio` - Receiver part of channel for communication from GPIO handler
    /// * `tx_to_controllino` - Sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Receiver part of channel for communication from Controllino
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    pub fn execute(
        &mut self,
        _mutex_device_scheduler_monitors: Arc<Mutex<i32>>,
        _tx_to_gpio: Sender<InternalCommand>,
        _rx_from_gpio: Receiver<f32>,
        _tx_to_controllino: Sender<InternalCommand>,
        _rx_from_controllino: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
        rx_from_messaging: Receiver<InternalCommand>,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut loop_counter = 0;
        let mut quit_command_received: bool;
        let mut start_command_received: bool;
        let mut stop_command_received: bool;
        let mut monitors_inhibited: bool = false;

        loop {
            if self.config.active && (loop_counter % 100 == 0) {
                if monitors_inhibited {
                    info!(
                        target: Self::LOG_TARGET_MONITORS,
                        "monitors are inhibited after receiving stop command via message.");
                } else {
                    // run monitors every 10 seconds
                    debug!(
                        target: Self::LOG_TARGET_MONITORS,
                        "executing monitors"
                    );
                }
            }
            (
                quit_command_received,
                start_command_received,
                stop_command_received,
            ) = self.process_external_request(&rx_from_signal_handler, Some(&rx_from_messaging));
            if quit_command_received {
                break;
            }
            if stop_command_received {
                info!(
                    target: Self::LOG_TARGET_MONITORS,
                    "received Stop command. Inhibiting monitoring."
                );
                monitors_inhibited = true;
            }
            if start_command_received {
                info!(
                    target: Self::LOG_TARGET_MONITORS,
                    "received Start command. Restarting monitoring."
                );
                monitors_inhibited = false;
            }

            spin_sleeper.sleep(sleep_duration_hundred_millis);

            loop_counter += 1;
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        info!(
            target: Self::LOG_TARGET_MONITORS,
            "sending Quit confirmation to signal handler."
        );

        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_MONITORS,
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // this thread has channel connection to underlying threads (Controllino).
        // These threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by signal_handler crate.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match rx_from_signal_handler.recv() {
                Ok(c) => match c {
                    InternalCommand::Terminate => true,
                    _ => {
                        warn!(
                            target: Self::LOG_TARGET_MONITORS,
                            "ignoring not applicable command {} from signal handler.",
                            c
                        );
                        false
                    }
                },
                Err(e) => {
                    error!(
                        target: Self::LOG_TARGET_MONITORS,
                        "receiving termination signal failed ({e:?})"
                    );
                    false
                }
            };
            spin_sleeper.sleep(sleep_duration_hundred_millis);
        }
    }
}
