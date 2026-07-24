/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages the central monitoring thread for aggregating and storing system state.
//!
//! This module provides the `Monitors` struct, which runs as a dedicated thread
//! responsible for collecting real-time data from other application modules. Its
//! primary function is to maintain a historical record of specific system states,
//! such as the data provided by the `Refill` module.
//!
//! ## Core Functionality
//!
//! The `Monitors` thread operates in a continuous loop, performing the following tasks:
//!
//! 1.  **Data Aggregation**: It listens for `RefillMonitorView` updates sent from the
//!     `Refill` thread. To conserve memory and reduce redundant data, it only stores
//!     a new view if its content has changed from the previously received one.
//!
//! 2.  **Historical Window**: It maintains a rolling window of the most recent
//!     `RefillMonitorView` structs. The maximum size of this history is configurable,
//!     preventing unbounded memory growth. When the limit is reached, the oldest
//!     entry is discarded to make room for the new one.
//!
//! 3.  **Runtime Control**: The monitoring activity can be dynamically enabled or
//!     disabled at runtime via `Start` and `Stop` commands received from the
//!     `Messaging` system. This allows for flexible control without restarting the
//!     application.
//!
//! 4.  **Graceful Shutdown**: The thread continuously listens for a `Quit` command
//!     from the `signal_handler`. Upon receipt, it breaks its main loop and sends
//!     an acknowledgment, ensuring it participates correctly in the application's
//!     phased shutdown sequence.
//!
//! ## Key Components
//!
//! - **`Monitors`**: The main struct that holds the configuration, state (like the
//!   vector of views), and log-suppression flags. It uses the default implementation
//!   of the `ProcessExternalRequestTrait` to handle `Start`, `Stop`, and `Quit` commands.
//!
//! - **`execute()`**: The entry point and main loop for the thread. It orchestrates
//!   data reception, state management, and command processing.
//!
//! - **`append_refill_monitor_view()`**: A helper method that manages the `Vec<RefillMonitorView>`,
//!   enforcing the configured size limit.
//!
//! - **`MonitorsChannels`**: (Defined in `monitors_channels.rs`) A struct that
//!   encapsulates all the `mpsc` channels required for the `Monitors` thread to
//!   communicate with other parts of the application.

use log::error;

#[cfg(all(not(test), not(feature = "test-utils")))]
use log::info;

#[cfg(feature = "debug_channels")]
use log::debug;

#[cfg(all(not(feature = "test-utils"), not(test), target_os = "linux"))] // only required in real-life execution
use nix::unistd::gettid;

use crate::launch::channels::AquaChannelError;
use crate::sensors::atlas_scientific_monitor_view::AtlasScientificMonitorView;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::watchmen::monitors_channels::MonitorsChannels;
use crate::watchmen::monitors_config::MonitorsConfig;
use crate::water::refill_monitor_view::RefillMonitorView;
use std::sync::Arc;
use std::time::Duration;

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

    /// storage of atlas scientific monitor views
    atlas_scientific_monitor_views: Vec<AtlasScientificMonitorView>,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive from atlas scientific
    lock_error_receive_atlas_scientific: bool,
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
            atlas_scientific_monitor_views: vec![],
            lock_error_receive_atlas_scientific: false,
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

    /// Manages a vector of `AtlasScientificMonitorView` structs, ensuring it does not exceed
    /// a specified maximum length.
    ///
    /// If the vector has reached its maximum capacity, the oldest element (at index 0)
    /// is removed before the new `atlas_scientific_monitor_view` is appended.
    /// Otherwise, the new `atlas_scientific_monitor_view` is simply appended.
    ///
    /// # Arguments
    /// * `atlas_scientific_monitor_view` - The new `AtlasScientificMonitorView` struct to be added to the vector.
    pub fn append_atlas_scientific_monitor_view(
        &mut self,
        atlas_scientific_monitor_view: AtlasScientificMonitorView,
    ) {
        // Check if the vector has reached or exceeded its maximum allowed length.
        if self.atlas_scientific_monitor_views.len()
            >= self.config.max_length_atlas_scientific_monitor_views
        {
            #[cfg(feature = "debug_channels")]
            debug!(target: module_path!(), "CAUTION: Reached size limit for Atlas Scientific monitor views.");

            // If it has, remove the oldest element (at index 0).
            // This shifts all further elements to the left.
            self.atlas_scientific_monitor_views.remove(0);
        }
        // Append the new AtlasScientificMonitorView struct to the end of the vector.
        self.atlas_scientific_monitor_views
            .push(atlas_scientific_monitor_view);
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
    /// * `mutex_device_scheduler_monitors` - A parameter representing `mutex_device_scheduler_monitors`.
    pub async fn execute(
        &mut self,
        _mutex_device_scheduler_monitors: Arc<tokio::sync::Mutex<i32>>,
        monitors_channels: &mut MonitorsChannels,
    ) {
        #[cfg(all(target_os = "linux", not(feature = "test-utils"), not(test)))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let mut quit_command_received: bool;
        let mut start_command_received: bool;
        let mut stop_command_received: bool;
        let mut monitors_inhibited: bool = false;
        let mut old_refill_view_opt: Option<RefillMonitorView> = None;

        loop {
            if self.config.active {
                #[allow(clippy::needless_bool_assign)]
                if monitors_inhibited && !self.lock_info_monitors_inhibited {
                    #[cfg(all(not(test), not(feature = "test-utils")))]
                    info!(
                        target: module_path!(),
                        "monitors are inhibited after receiving stop command via message.");
                    self.lock_info_monitors_inhibited = true;
                } else {
                    self.lock_info_monitors_inhibited = false;
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
                            #[cfg(feature = "debug_channels")]
                            AquaChannelError::Full => { /* only applicable to send - no action required */
                            }
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

                match monitors_channels
                    .rx_monitors_from_atlas_scientific
                    .try_recv()
                {
                    Ok(new_view) => {
                        // Deduplicate against last stored view
                        if let Some(last_view) = self.atlas_scientific_monitor_views.last() {
                            if &new_view != last_view {
                                self.append_atlas_scientific_monitor_view(new_view);
                            }
                        } else {
                            self.append_atlas_scientific_monitor_view(new_view);
                        }
                    }
                    Err(e) => {
                        match e {
                            #[cfg(feature = "debug_channels")]
                            AquaChannelError::Full => { /* only applicable to send */ }
                            AquaChannelError::Empty => { /* empty buffer - no action required */ }
                            AquaChannelError::Disconnected => {
                                if !self.lock_error_receive_atlas_scientific {
                                    error!(target: module_path!(), "Error when trying to receive from atlas_scientific: {e}");
                                    self.lock_error_receive_atlas_scientific = true;
                                }
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(sleep_duration_hundred_millis).await;

            (
                quit_command_received,
                start_command_received,
                stop_command_received,
            ) = self.process_external_request(
                &mut monitors_channels.rx_monitors_from_signal_handler,
                Some(&mut monitors_channels.rx_monitors_from_messaging),
            );
            if quit_command_received {
                break;
            }
            if stop_command_received {
                #[cfg(all(not(test), not(feature = "test-utils")))]
                info!(
                    target: module_path!(),
                    "received Stop command. Inhibiting monitoring."
                );
                monitors_inhibited = true;
            }
            if start_command_received {
                #[cfg(all(not(test), not(feature = "test-utils")))]
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
