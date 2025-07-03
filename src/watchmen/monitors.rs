/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::{error, info};

#[cfg(test)]
use log::debug;

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::TryRecvError;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::utilities::channel_content::InternalCommand;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::water::refill_monitor_view::RefillMonitorView;

/// Holds the configuration data for the monitors.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct MonitorsConfig {
    /// monitors will only be active when the flag is set to true
    active: bool,

    /// Maximum number of monitoring entries for refill
    max_length_refill_monitor_views: usize,
}

/// Struct collects the channels for the communication between the monitors and the other components.
pub struct MonitorsChannels {
    /// sender part of the channel for communication to the relay manager
    #[allow(unused)]
    pub tx_monitors_to_relay_manager: Sender<InternalCommand>,

    /// receiver part of the channel for communication from the relay manager
    #[allow(unused)]
    pub rx_monitors_from_relay_manager: Receiver<bool>,

    /// receiver part of the channel for communication from the refill control
    pub rx_monitors_from_refill: Receiver<RefillMonitorView>,

    /// sender part of the channel for communication to the signal handler
    pub tx_monitors_to_signal_handler: Sender<bool>,

    /// receiver part of the channel for communication from the signal handler
    pub rx_monitors_from_signal_handler: Receiver<InternalCommand>,

    /// receiver part of the channel for communication from the messaging component
    pub rx_monitors_from_messaging_opt: Option<Receiver<InternalCommand>>,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the monitors.
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     monitors[Monitors] --> relay_manager[Relay Manager]
///     relay_manager --> monitors
///     monitors --> signal_handler[Signal handler]
///     signal_handler --> monitors
///     messaging[Messaging] --> monitors
/// ```
pub struct Monitors {
    /// configuration data for monitors
    config: MonitorsConfig,

    /// inhibition flag to avoid flooding the log file with repeated messages about monitors being inhibited
    lock_info_monitors_inhibited: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive from refill
    lock_error_receive_refill: bool,

    /// storage of refill monitor views
    refill_monitor_views: Vec<RefillMonitorView>,
}

impl ProcessExternalRequestTrait for Monitors {}

impl Monitors {
    const LOG_TARGET_MONITORS: &'static str = "monitors";

    /// Creates a new `Monitors` instance.
    ///
    /// This constructor initializes the monitoring module with its specified
    /// configuration. It sets up internal "lock" flags to `false` by default;
    /// these flags are used to prevent repetitive error and warning messages
    /// from flooding the log during operation.
    ///
    /// # Arguments
    /// * `config` - **Configuration data** for the monitors, loaded from a TOML file.
    ///   This includes parameters such as whether the monitors are `active`.
    ///
    /// # Returns
    /// A new **`Monitors` struct**, ready to perform its monitoring tasks.
    pub fn new(config: MonitorsConfig) -> Monitors {
        Self {
            config,
            lock_info_monitors_inhibited: false,
            lock_error_receive_refill: false,
            refill_monitor_views: vec![],
        }
    }
    /// Manages a vector of `RefillMonitorView` structs, ensuring it does not exceed
    /// a specified maximum length.
    ///
    /// If the vector has reached its maximum capacity, the oldest element (at index 0)
    /// is removed before the new `refill_monitor_view` is appended.
    /// Otherwise, the new `refill_monitor_view` is simply appended.
    ///
    /// # Arguments
    /// * `refill_monitor_view` - The new `RefillMonitorView` struct to be added to the vector.
    pub fn append_refill_monitor_view(&mut self, refill_monitor_view: RefillMonitorView) {
        // Check if the vector has reached or exceeded its maximum allowed length.
        if self.refill_monitor_views.len() >= self.config.max_length_refill_monitor_views {
            // If it has, remove the oldest element (at index 0).
            // This shifts all further elements to the left.
            self.refill_monitor_views.remove(0);
        }
        // Append the new RefillMonitorView struct to the end of the vector.
        self.refill_monitor_views.push(refill_monitor_view);
    }

    /// Executes the main control loop for the monitor module.
    ///
    /// This function runs continuously, performing periodic monitoring tasks when active.
    /// It responds to external `Start` and `Stop` commands to enable or inhibit monitoring
    /// activities. The loop also continuously checks for `Quit` and `Terminate` commands
    /// from the signal handler to ensure a graceful application shutdown.
    ///
    /// Monitoring activities (represented by debug logs) are performed at a rate of
    /// approximately every 10 seconds, controlled by the `loop_counter` and `sleep_duration`.
    ///
    /// # Arguments
    /// * `_mutex_device_scheduler_monitors` - (Ignored) A clone of a mutex for device scheduling.
    ///   This argument is part of a common signature but is not used by this module's logic.
    /// * `_tx_to_relay_manager` - (Ignored) The sender channel for communicating with the relay manager.
    ///   This argument is present for signature consistency but not used by this function.
    /// * `_rx_from_relay_manager` - (Ignored) The receiver channel for communication from the relay manager.
    ///   This argument is present for signature consistency but not used by this function.
    /// * `tx_to_signal_handler` - The sender channel for this module to send acknowledgment
    ///   back to the main signal handler during shutdown.
    /// * `rx_from_signal_handler` - The receiver channel for this module to receive commands
    ///   (e.g., `Quit`, `Terminate`) from the main signal handler.
    /// * `rx_from_messaging_opt` - An `Option` containing the receiver channel for commands
    ///   from the messaging system (e.g., `Start`, `Stop`).
    pub fn execute(
        &mut self,
        _mutex_device_scheduler_monitors: Arc<Mutex<i32>>,
        monitors_channels: MonitorsChannels,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut quit_command_received: bool;
        let mut start_command_received: bool;
        let mut stop_command_received: bool;
        let mut monitors_inhibited: bool = false;
        let mut old_refill_view_opt: Option<RefillMonitorView> = None;

        loop {
            if self.config.active {
                if monitors_inhibited && !self.lock_info_monitors_inhibited {
                    info!(
                        target: Self::LOG_TARGET_MONITORS,
                        "monitors are inhibited after receiving stop command via message.");
                    self.lock_info_monitors_inhibited = true;
                } else {
                    self.lock_info_monitors_inhibited = false;
                    #[cfg(test)]
                    debug!(
                        target: Self::LOG_TARGET_MONITORS,
                        "executing monitors"
                    );
                }

                (
                    quit_command_received,
                    start_command_received,
                    stop_command_received,
                ) = self.process_external_request(
                    &monitors_channels.rx_monitors_from_signal_handler,
                    monitors_channels.rx_monitors_from_messaging_opt.as_ref(),
                );
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

                match monitors_channels.rx_monitors_from_refill.try_recv() {
                    Ok(new_refill_view) => match old_refill_view_opt {
                        Some(ref old_refill_view) => {
                            if &new_refill_view != old_refill_view {
                                self.append_refill_monitor_view(new_refill_view.clone());
                                old_refill_view_opt = Some(new_refill_view);
                            }
                        }
                        None => {
                            self.append_refill_monitor_view(new_refill_view.clone());
                            old_refill_view_opt = Some(new_refill_view);
                        }
                    },
                    Err(e) => {
                        match e {
                            TryRecvError::Empty => { /* empty buffer - no action required */ }
                            TryRecvError::Disconnected => {
                                if !self.lock_error_receive_refill {
                                    error!(target: Self::LOG_TARGET_MONITORS, "Error when trying to receive from refill: {}", e);
                                    self.lock_error_receive_refill = true;
                                }
                            }
                        }
                    }
                };
                spin_sleeper.sleep(sleep_duration_hundred_millis);
            }

            // The application received request to terminate. That is why the loop was left.
            // Answer to the thread which sent the request for termination, so that shutdown can proceed further.
            match monitors_channels.tx_monitors_to_signal_handler.send(true) {
                Ok(()) => {}
                Err(e) => {
                    error!(
                        target: Self::LOG_TARGET_MONITORS,
                        "sending quit confirmation back to signal handler failed ({e:?})"
                    );
                }
            };
        }
    }
}
