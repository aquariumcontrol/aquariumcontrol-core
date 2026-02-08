use std::sync::{Arc, Mutex};
use crate::thermal::heating_channels::HeatingChannels;
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
use crate::utilities::logger::log_error_chain;

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
fn actuate_heater(
    command: InternalCommand,
    mutex_device_scheduler_heating: &Arc<Mutex<i32>>,
    mutex_blocked_during_actuation: &mut bool,
    heating_channels: &mut HeatingChannels,
) {
    // reduce the probability of wrong warning
    *mutex_blocked_during_actuation |= mutex_device_scheduler_heating.try_lock().is_err();

    // avoid electrical overloads through parallel device actuation
    let mut mutex_data = mutex_device_scheduler_heating.lock().unwrap();

    // Use map_err for more concise error handling on the send operation.
    if let Err(e) = heating_channels.send_to_relay_manager(command.clone()) {
        let error_message = format!("Channel communication to relay manager for {command} failed.");
        log_error_chain(module_path!(), &error_message, e);
    } else {
        // If send was successful, wait for the response.
        if let Err(e) = heating_channels.receive_from_relay_manager() {
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
pub fn switch_on_heater(
    mutex_device_scheduler_heating: &Arc<Mutex<i32>>,
    mutex_blocked_during_actuation: &mut bool,
    heating_channels: &mut HeatingChannels,
) {
    actuate_heater(
        InternalCommand::SwitchOn(AquariumDevice::Heater),
        mutex_device_scheduler_heating,
        mutex_blocked_during_actuation,
        heating_channels,
    );
}

/// Commands the relay manager to switch off the heater.
///
/// This function is a specific wrapper around `actuate_heater` that sends the `SwitchOff` command.
///
/// # Arguments
/// * `mutex_device_scheduler_heating` - A reference to the shared actuation mutex.
/// * `mutex_blocked_during_actuation` - A mutable flag to track if the actuation mutex was blocked.
/// * `heating_channels` - A mutable reference to the struct containing the channels.
pub fn switch_off_heater(
    mutex_device_scheduler_heating: &Arc<Mutex<i32>>,
    mutex_blocked_during_actuation: &mut bool,
    heating_channels: &mut HeatingChannels,
) {
    actuate_heater(
        InternalCommand::SwitchOff(AquariumDevice::Heater),
        mutex_device_scheduler_heating,
        mutex_blocked_during_actuation,
        heating_channels,
    );
}