/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::{error, info};

#[cfg(any(feature = "debug_channels", test))]
use log::debug;

#[cfg(all(not(test), target_os = "linux"))]
use nix::unistd::gettid;

use spin_sleep::SpinSleeper;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use crate::launch::channels::AquaChannelError;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::watchmen::monitors_channels::MonitorsChannels;
use crate::watchmen::monitors_config::MonitorsConfig;
use crate::water::refill_monitor_view::RefillMonitorView;

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
            #[cfg(feature = "debug_channels")]
            debug!(target: module_path!(), "CAUTION: Reached size limit for monitor views.");

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
    /// * `monitors_channels` - A mutable reference to the struct containing the channels.
    pub fn execute(
        &mut self,
        _mutex_device_scheduler_monitors: Arc<Mutex<i32>>,
        monitors_channels: &mut MonitorsChannels,
    ) {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

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
                        target: module_path!(),
                        "monitors are inhibited after receiving stop command via message.");
                    self.lock_info_monitors_inhibited = true;
                } else {
                    self.lock_info_monitors_inhibited = false;
                    #[cfg(test)]
                    debug!(
                        target: module_path!(),
                        "executing monitors"
                    );
                }

                match monitors_channels.receive_from_refill() {
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
                            #[cfg(feature = "debug_channels")]
                            AquaChannelError::Full => { /* only applicable to send - no action required */ }
                            AquaChannelError::Empty => { /* empty buffer - no action required */ }
                            AquaChannelError::Disconnected => {
                                if !self.lock_error_receive_refill {
                                    error!(target: module_path!(), "Error when trying to receive from refill: {e}");
                                    self.lock_error_receive_refill = true;
                                }
                            }
                        }
                    }
                };
            }
            spin_sleeper.sleep(sleep_duration_hundred_millis);

            (
                quit_command_received,
                start_command_received,
                stop_command_received,
            ) = self.process_external_request(
                &mut monitors_channels.rx_monitors_from_signal_handler,
                monitors_channels.rx_monitors_from_messaging_opt.as_mut(),
            );
            if quit_command_received {
                break;
            }
            if stop_command_received {
                info!(
                    target: module_path!(),
                    "received Stop command. Inhibiting monitoring."
                );
                monitors_inhibited = true;
            }
            if start_command_received {
                info!(
                    target: module_path!(),
                    "received Start command. Restarting monitoring."
                );
                monitors_inhibited = false;
            }
        }

        monitors_channels.acknowledge_signal_handler();
    }
}
