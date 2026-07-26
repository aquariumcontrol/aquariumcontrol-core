/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Translates high-level device commands into low-level GPIO pin states.
//!
//! This entire module is conditionally compiled and is only active when the application
//! is built for a `target_os = "linux"` with the `target_hw` feature enabled.
//!
//! Its sole purpose is to serve as a pure logic layer that abstracts away the physical
//! characteristics of the relays being used. It determines whether a GPIO pin should be
//! set to HIGH or LOW to achieve a desired state (ON or OFF) for a specific device.
//!
//! ## Key Components
//!
//! - **`RelayLogic` Enum**: A simple but powerful abstraction that defines the two
//!   possible physical wiring configurations for a relay:
//!   - `NormallyOpen` (NO): The circuit is open by default. Applying power (HIGH) closes it, turning the device ON.
//!   - `NormallyClosed` (NC): The circuit is closed by default. Applying power (HIGH) opens it, turning the device OFF.
//!
//! - **`get_relay_command_gpio()`**: The public entry point of the module. It acts as a
//!   central mapping, taking a high-level `AquariumDevice` and determining its
//!   `RelayLogic`.
//!
//! - **`get_gpio_state()`**: A private helper function that contains the core state
//!   machine. It takes a command (`SwitchOn`/`SwitchOff`) and a `RelayLogic` and
//!   returns the final boolean state (`true` for HIGH, `false` for LOW) required for the GPIO pin.
//!
//! ## Design and Architecture
//!
//! This module uses **Separation of Concerns** principle.
//!
//! - **Decoupling "What" from "How"**: The `get_relay_command_gpio` function separates the
//!   question "What device are we controlling?" from "How is that device wired?". It
//!   first determines the `RelayLogic` for the given device, then passes that logic to
//!   `get_gpio_state` to get the final pin state.
//!
//! - **Simplifying the Actuator**: By handling this translation, the module allows the
//!   `ActuateGpio` implementation to be extremely simple. The actuator no longer needs
//!   to know about inverted logic or relay types; it just receives a boolean and calls
//!   `pin.set_high()` or `pin.set_low()`.
//!
//! - **Centralized Logic**: All physical relay logic when using GPIO for actuation is
//!   centralized in this one file.

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
///
/// [impl->swreq~relay_control_actuators~1]
fn get_gpio_state(command: &InternalCommand, logic: RelayLogic) -> Result<bool, RelayError> {
    match (command, logic) {
        (InternalCommand::SwitchOn(_), RelayLogic::NormallyOpen) => Ok(true), // ON is HIGH
        (InternalCommand::SwitchOn(_), RelayLogic::NormallyClosed) => Ok(false), // ON is LOW

        (InternalCommand::SwitchOff(_), RelayLogic::NormallyOpen) => Ok(false), // OFF is LOW
        (InternalCommand::SwitchOff(_), RelayLogic::NormallyClosed) => Ok(true), // OFF is HIGH

        // Error cases can be handled here as well, though the refactored
        // public function above already handles them.
        (InternalCommand::Pulse(d, _), _) => Err(RelayError::PulseNotAllowedForDevice(
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

        AquariumDevice::Beacon => {
            return Err(RelayError::DeviceNotConnected(
                module_path!().to_string(),
                AquariumDevice::Beacon,
            ));
        }
    };

    // Then, get the GPIO state based on that single, unified logic function.
    get_gpio_state(internal_command, logic)
}
