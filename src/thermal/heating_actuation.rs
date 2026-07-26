/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Helper module for actuating the aquarium heater.
//!
//! This module provides a set of functions to control the physical state of the heater
//! by sending commands to the `RelayManager`. It encapsulates the low-level details of:
//!
//! - **Channel Communication**: Sending `SwitchOn` and `SwitchOff` commands.
//! - **Synchronization**: Acquiring the shared `device_scheduler` mutex to ensure exclusive access during actuation.
//! - **Error Handling**: Logging any communication failures that occur during the process.
//!
//! These functions are primarily used by the `Heating` control thread to execute its control decisions.

use crate::thermal::heating_channels::HeatingChannels;
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
use crate::utilities::logger::log_error_chain;
use std::sync::Arc;

/// Sends a command to the relay manager to actuate the heater and waits for a response.
///
/// This private helper function encapsulates the logic for sending a command
/// (either `SwitchOn` or `SwitchOff`) to the relay manager. It handles channel
/// communication, error logging, and updates the shared actuation mutex.
///
/// # Arguments
/// * `command` - The `InternalCommand` to send (`SwitchOn` or `SwitchOff`).
/// * `mutex_device_scheduler_heating` - A reference to the shared actuation mutex.
/// * `mutex_blocked_during_actuation` - A mutable flag to track if the actuation mutex was blocked.
/// * `heating_channels` - A mutable reference to the struct containing the channels.
/// * `is_multi_thread` - A boolean flag indicating if the Tokio runtime is running in multi-threaded mode.
///
/// [impl->swreq~thermal_heater_heating~1]
async fn actuate_heater(
    command: InternalCommand,
    mutex_device_scheduler_heating: &Arc<tokio::sync::Mutex<i32>>,
    mutex_blocked_during_actuation: &mut bool,
    heating_channels: &mut HeatingChannels,
    _is_multi_thread: bool,
) {
    // reduce the probability of wrong warning
    *mutex_blocked_during_actuation |= mutex_device_scheduler_heating.try_lock().is_err();

    let mut mutex_data = mutex_device_scheduler_heating.lock().await;

    // Use map_err for more concise error handling on the send operation.
    if let Err(e) = heating_channels.send_to_relay_manager(command.clone()) {
        let error_message = format!("Channel communication to relay manager for {command} failed.");
        log_error_chain(module_path!(), &error_message, e);
    } else {
        // If send was successful, wait for the response.
        if let Err(e) = heating_channels.receive_from_relay_manager().await {
            let error_message =
                format!("Receiving answer from relay manager for {command} failed.");
            log_error_chain(module_path!(), &error_message, e);
        }
    }

    *mutex_data = mutex_data.saturating_add(1);
}

/// Commands the relay manager to switch on the heater.
///
/// This function is a specific wrapper around `actuate_heater` that sends the `SwitchOn` command.
///
/// # Arguments
/// * `mutex_device_scheduler_heating` - A reference to the shared actuation mutex.
/// * `mutex_blocked_during_actuation` - A mutable flag to track if the actuation mutex was blocked.
/// * `heating_channels` - A mutable reference to the struct containing the channels.
/// * `is_multi_thread` - A boolean flag indicating if the Tokio runtime is running in multithreaded mode.
pub async fn switch_on_heater(
    mutex_device_scheduler_heating: &Arc<tokio::sync::Mutex<i32>>,
    mutex_blocked_during_actuation: &mut bool,
    heating_channels: &mut HeatingChannels,
    is_multi_thread: bool,
) {
    actuate_heater(
        InternalCommand::SwitchOn(AquariumDevice::Heater),
        mutex_device_scheduler_heating,
        mutex_blocked_during_actuation,
        heating_channels,
        is_multi_thread,
    )
    .await;
}

/// Commands the relay manager to switch off the heater.
///
/// This function is a specific wrapper around `actuate_heater` that sends the `SwitchOff` command.
///
/// # Arguments
/// * `mutex_device_scheduler_heating` - A reference to the shared actuation mutex.
/// * `mutex_blocked_during_actuation` - A mutable flag to track if the actuation mutex was blocked.
/// * `heating_channels` - A mutable reference to the struct containing the channels.
/// * `is_multi_thread` - A boolean flag indicating if the Tokio runtime is running in multi-threaded mode.
pub async fn switch_off_heater(
    mutex_device_scheduler_heating: &Arc<tokio::sync::Mutex<i32>>,
    mutex_blocked_during_actuation: &mut bool,
    heating_channels: &mut HeatingChannels,
    is_multi_thread: bool,
) {
    actuate_heater(
        InternalCommand::SwitchOff(AquariumDevice::Heater),
        mutex_device_scheduler_heating,
        mutex_blocked_during_actuation,
        heating_channels,
        is_multi_thread,
    )
    .await;
}
