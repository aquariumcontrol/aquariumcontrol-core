/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::relays::relay_manager::RelayError;
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};

fn get_relay_command_normally_open(command: &InternalCommand) -> Result<bool, RelayError> {
    match command {
        InternalCommand::SwitchOn(_) => Ok(true),
        InternalCommand::SwitchOff(_) => Ok(false),
        InternalCommand::Pulse(d, _) => Err(RelayError::PulseNotAllowedForDevice(d.clone())),
        _ => Err(RelayError::IrrelevantCommand(command.clone())),
    }
}

fn get_relay_command_normally_closed(command: &InternalCommand) -> Result<bool, RelayError> {
    match command {
        InternalCommand::SwitchOn(_) => Ok(false),
        InternalCommand::SwitchOff(_) => Ok(true),
        InternalCommand::Pulse(d, _) => Err(RelayError::PulseNotAllowedForDevice(d.clone())),
        _ => Err(RelayError::IrrelevantCommand(command.clone())),
    }
}

/// Provides boolean value for switching the relay according to relay logic when using GPIO.
/// # Arguments
/// * `internal_command` - enum value which is interpreted
/// * `device` - enum value for which the function shall provide the relay id
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
