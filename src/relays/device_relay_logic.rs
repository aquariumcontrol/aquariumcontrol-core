/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::relays::relay_error::RelayError;
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};

/// Determines the target GPIO state for a normally-open relay based on an `InternalCommand`.
///
/// For a normally-open relay, a `true` (HIGH) signal typically switches it **on**,
/// and a `false` (LOW) signal switches it **off**.
///
/// # Arguments
/// * `command` - The `InternalCommand` (e.g., `SwitchOn`, `SwitchOff`, `Pulse`) to interpret.
///
/// # Returns
/// A `Result` which is:
/// - `Ok(true)` if `command` is `SwitchOn`.
/// - `Ok(false)` if `command` is `SwitchOff`.
/// - `Err(RelayError::PulseNotAllowedForDevice)` if `command` is `Pulse`.
/// - `Err(RelayError::IrrelevantCommand)` for any other command.
fn get_relay_command_normally_open(command: &InternalCommand) -> Result<bool, RelayError> {
    match command {
        InternalCommand::SwitchOn(_) => Ok(true),
        InternalCommand::SwitchOff(_) => Ok(false),
        InternalCommand::Pulse(d, _) => Err(RelayError::PulseNotAllowedForDevice(
            module_path!().to_string(),
            d.clone(),
        )),
        _ => Err(RelayError::IrrelevantCommand(
            module_path!().to_string(),
            command.clone(),
        )),
    }
}

/// Determines the target GPIO state for a normally-closed relay based on an `InternalCommand`.
///
/// For a normally-closed relay, a `false` (LOW) signal typically switches it **on**,
/// and a `true` (HIGH) signal switches it **off**.
///
/// # Arguments
/// * `command` - The `InternalCommand` (e.g., `SwitchOn`, `SwitchOff`, `Pulse`) to interpret.
///
/// # Returns
/// A `Result` which is:
/// - `Ok(false)` if `command` is `SwitchOn`.
/// - `Ok(true)` if `command` is `SwitchOff`.
/// - `Err(RelayError::PulseNotAllowedForDevice)` if `command` is `Pulse`.
/// - `Err(RelayError::IrrelevantCommand)` for any other command.
fn get_relay_command_normally_closed(command: &InternalCommand) -> Result<bool, RelayError> {
    match command {
        InternalCommand::SwitchOn(_) => Ok(false),
        InternalCommand::SwitchOff(_) => Ok(true),
        InternalCommand::Pulse(d, _) => Err(RelayError::PulseNotAllowedForDevice(
            module_path!().to_string(),
            d.clone(),
        )),
        _ => Err(RelayError::IrrelevantCommand(
            module_path!().to_string(),
            command.clone(),
        )),
    }
}

/// Determines the appropriate GPIO pin state (`true` for low, `false` for high)
/// required to actuate a specific aquarium device based on its configured relay logic.
///
/// This function serves as the central mapping for GPIO control. It takes a high-level
/// `InternalCommand` (like `SwitchOn` or `SwitchOff`) and an `AquariumDevice`
/// and, by leveraging `get_relay_command_normally_open` or `get_relay_command_normally_closed`,
/// returns the boolean state the GPIO pin should be set to. This accounts for
/// relays that are normally open or normally closed.
///
/// # Arguments
/// * `internal_command` - The `InternalCommand` (e.g., `SwitchOn(device)`, `SwitchOff(device)`)
///   that indicates the desired action for the device.
/// * `device` - The `AquariumDevice` enum variant for which the GPIO state is to be determined.
///
/// # Returns
/// A `Result<bool, RelayError>`:
/// - `Ok(true)`: The GPIO pin should be set to `low` to achieve the desired device state.
/// - `Ok(false)`: The GPIO pin should be set to `high` to achieve the desired device state.
/// - `Err(RelayError::PulseNotAllowedForDevice)`: If a `Pulse` command is given for a device
///   that doesn't support pulsing (as determined by the underlying helper functions).
/// - `Err(RelayError::IrrelevantCommand)`: For any other `InternalCommand` that is not
///   a recognized actuation command (e.g., `RequestSignal`).
pub fn get_relay_command_gpio(
    internal_command: &InternalCommand,
    device: AquariumDevice,
) -> Result<bool, RelayError> {
    match device {
        AquariumDevice::Skimmer => get_relay_command_normally_closed(internal_command),
        AquariumDevice::MainPump1 => get_relay_command_normally_closed(internal_command),
        AquariumDevice::MainPump2 => get_relay_command_normally_closed(internal_command),
        AquariumDevice::AuxPump1 => get_relay_command_normally_closed(internal_command),
        AquariumDevice::AuxPump2 => get_relay_command_normally_closed(internal_command),
        AquariumDevice::Heater => get_relay_command_normally_open(internal_command),
        AquariumDevice::Ventilation => get_relay_command_normally_closed(internal_command),
        AquariumDevice::RefillPump => get_relay_command_normally_open(internal_command),
        AquariumDevice::Feeder => get_relay_command_normally_open(internal_command),
        AquariumDevice::PeristalticPump1 => get_relay_command_normally_open(internal_command),
        AquariumDevice::PeristalticPump2 => get_relay_command_normally_open(internal_command),
        AquariumDevice::PeristalticPump3 => get_relay_command_normally_open(internal_command),
        AquariumDevice::PeristalticPump4 => get_relay_command_normally_open(internal_command),
    }
}
