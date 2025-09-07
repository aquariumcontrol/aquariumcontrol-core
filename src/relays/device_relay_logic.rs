/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use crate::relays::relay_error::RelayError;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};

#[cfg(all(target_os = "linux", feature = "target_hw"))]
/// An enum to represent the physical logic of a relay.
enum RelayLogic {
    NormallyOpen,
    NormallyClosed,
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
/// Determines the target GPIO state based on a command and the relay's physical logic.
///
/// # Arguments
/// * `command` - The `InternalCommand` to interpret.
/// * `logic` - The `RelayLogic` of the device being controlled.
///
/// # Returns
/// A `Result` containing the target GPIO state (`true` for HIGH, `false` for LOW).
///
/// # Errors
/// Propagates errors for irrelevant or unsupported commands.
fn get_gpio_state(command: &InternalCommand, logic: RelayLogic) -> Result<bool, RelayError> {
    match command {
        InternalCommand::SwitchOn(_) => match logic {
            RelayLogic::NormallyOpen => Ok(true),    // ON is HIGH
            RelayLogic::NormallyClosed => Ok(false), // ON is LOW
        },
        InternalCommand::SwitchOff(_) => match logic {
            RelayLogic::NormallyOpen => Ok(false),  // OFF is LOW
            RelayLogic::NormallyClosed => Ok(true), // OFF is HIGH
        },
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

#[cfg(all(target_os = "linux", feature = "target_hw"))]
/// Determines the appropriate GPIO pin state (`true` for low, `false` for high)
/// required to actuate a specific aquarium device based on its configured relay logic.
///
/// This function serves as the central mapping for GPIO control. It takes a high-level
/// `InternalCommand` (like `SwitchOn` or `SwitchOff`) and an `AquariumDevice`
/// and determines the correct boolean state for the GPIO pin, accounting for whether
/// the connected relay is normally open or normally closed.
///
/// # Arguments
/// * `internal_command` - The `InternalCommand` (e.g., `SwitchOn(device)`)
///   that indicates the desired action.
/// * `device` - The `AquariumDevice` for which the GPIO state is to be determined.
///
/// # Returns
/// A `Result` containing a boolean that represents the target GPIO pin state.
/// - `Ok(true)`: The GPIO pin should be set to `high`.
/// - `Ok(false)`: The GPIO pin should be set to `low`.
///
/// # Errors
/// This function will return a `RelayError` if the command is not a valid actuation command:
/// - `RelayError::PulseNotAllowedForDevice`: If a `Pulse` command is given, as GPIO-controlled
///   devices in this design do not support timed pulses.
/// - `RelayError::IrrelevantCommand`: For any other `InternalCommand` that is not
///   a recognized actuation command (e.g., `RequestSignal`).
pub fn get_relay_command_gpio(
    internal_command: &InternalCommand,
    device: AquariumDevice,
) -> Result<bool, RelayError> {
    // First, determine the relay logic for the device. This separates the
    // "what is it" from the "how to control it".
    let logic = match device {
        AquariumDevice::Skimmer
        | AquariumDevice::MainPump1
        | AquariumDevice::MainPump2
        | AquariumDevice::AuxPump1
        | AquariumDevice::AuxPump2
        | AquariumDevice::Ventilation => RelayLogic::NormallyClosed,

        AquariumDevice::Heater
        | AquariumDevice::RefillPump
        | AquariumDevice::Feeder
        | AquariumDevice::PeristalticPump1
        | AquariumDevice::PeristalticPump2
        | AquariumDevice::PeristalticPump3
        | AquariumDevice::PeristalticPump4 => RelayLogic::NormallyOpen,
    };

    // Then, get the GPIO state based on that single, unified logic function.
    get_gpio_state(internal_command, logic)
}
