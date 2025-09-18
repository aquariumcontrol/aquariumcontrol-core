/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Implements hardware actuation logic using direct GPIO control on a Linux system.
//!
//! This entire module is conditionally compiled and is only active when the application
//! is built for a `target_os = "linux"` with the `target_hw` feature enabled.
//!
//! It provides the `ActuateGpio` struct, a concrete implementation of the
//! `RelayActuationTrait`. Its purpose is to serve as an alternative to the
//! `ActuateControllino` backend, allowing the `RelayManager` to control relays
//! connected directly to the GPIO pins of a single-board computer like a Raspberry Pi.
//!
//! ## Key Components
//!
//! - **`ActuateGpioConfig`**: A simple configuration struct that holds the mapping
//!   of each logical `AquariumDevice` to a physical GPIO pin.
//!
//! - **`ActuateGpio`**: The main struct that holds the initialized `rppal::gpio::OutputPin`
//!   for each device. It is responsible for translating high-level commands into
//!   low-level GPIO state changes.
//!
//! - **`actuate()` Method**: The core of the implementation. It takes a `SwitchOn` or
//!   `SwitchOff` command, determines the correct GPIO pin to use, and sets its
//!   state to `high` or `low` based on the device's specific logic (e.g., whether
//!   it uses an inverted "active-low" relay).
//!
//! ## Design and Architecture
//!
//! This module is designed as a pluggable hardware abstraction layer.
//!
//! - **Hardware Abstraction**: By implementing the `RelayActuationTrait`, this module
//!   allows the `RelayManager` to remain completely agnostic about the underlying
//!   hardware. The `RelayManager` sends the same `InternalCommand`, whether it's
//!   being handled by a Controllino over serial or by direct GPIO pins.
//!
//! - **Startup Validation**: The `ActuateGpio::new()` constructor attempts to acquire
//!   and configure every required GPIO pin at application startup. If any pin is
//!   invalid, already in use, or inaccessible, due to permissions, the constructor
//!   will fail with a `RelayError`. This "fail-fast" approach prevents the
//!   application from running in a state where it cannot control its hardware.
//!
//! - **Stateless Operation**: Unlike serial communication, direct GPIO control is
//!   stateless. As a result, the `heartbeat()` and `flush_buffer()` methods of the
//!   trait are implemented as no-ops, and `Pulse` commands are not supported.

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
        use log::warn;
        use crate::relays::device_relay_logic::get_relay_command_gpio;
        use crate::relays::relay_error::RelayError;
        use crate::relays::relay_manager::RelayActuationTrait;
        use crate::utilities::channel_content::InternalCommand;
        use crate::sensors::gpio_handler_config::GpioHandlerConfig;
        use rppal::gpio::{Gpio, OutputPin};
        use crate::utilities::channel_content::AquariumDevice;
    }
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
/// Contains configuration for actuation of relays via GPIO
pub struct ActuateGpioConfig {
    pub gpio_skimmer: u8,
    pub gpio_main_pump1: u8,
    pub gpio_main_pump2: u8,
    pub gpio_aux_pump1: u8,
    pub gpio_aux_pump2: u8,
    pub gpio_feeder: u8,
    pub gpio_refill_pump: u8,
    pub gpio_heater: u8,
    pub gpio_ventilation: u8,
    pub gpio_peristaltic_pump1: u8,
    pub gpio_peristaltic_pump2: u8,
    pub gpio_peristaltic_pump3: u8,
    pub gpio_peristaltic_pump4: u8,
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
impl ActuateGpioConfig {
    /// Creates a new `ActuateGpioConfig` by mapping GPIO pins from a `GpioHandlerConfig`.
    ///
    /// This constructor is used to extract and organize the specific GPIO pin assignments
    /// for each aquarium device from a more general GPIO configuration structure.
    /// It's used during the application's initialization exclusively on Linux systems
    /// with the target hardware.
    ///
    /// # Arguments
    /// * `gpio_handler_config` - A reference to the `GpioHandlerConfig` struct, which contains
    ///   the raw GPIO pins for various devices.
    ///
    /// # Returns
    /// A new `ActuateGpioConfig` struct with device-specific GPIO pin assignments.
    pub fn new(gpio_handler_config: &GpioHandlerConfig) -> ActuateGpioConfig {
        ActuateGpioConfig {
            gpio_skimmer: gpio_handler_config.skimmer,
            gpio_main_pump1: gpio_handler_config.main_pump1,
            gpio_main_pump2: gpio_handler_config.main_pump2,
            gpio_aux_pump1: gpio_handler_config.aux_pump1,
            gpio_aux_pump2: gpio_handler_config.aux_pump2,
            gpio_feeder: gpio_handler_config.feeder,
            gpio_refill_pump: gpio_handler_config.refill_pump,
            gpio_heater: gpio_handler_config.heater,
            gpio_ventilation: gpio_handler_config.ventilation,
            gpio_peristaltic_pump1: gpio_handler_config.peristaltic_pump1,
            gpio_peristaltic_pump2: gpio_handler_config.peristaltic_pump2,
            gpio_peristaltic_pump3: gpio_handler_config.peristaltic_pump3,
            gpio_peristaltic_pump4: gpio_handler_config.peristaltic_pump4,
        }
    }
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
/// Contains trait implementation for actuation of relays via GPIO
pub struct ActuateGpio {
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO of the protein skimmer
    pub output_pin_skimmer: OutputPin,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO of the main pump #1
    pub output_pin_main_pump1: OutputPin,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO of the main pump #2
    pub output_pin_main_pump2: OutputPin,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO of the auxiliary pump #1
    pub output_pin_aux_pump1: OutputPin,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO of the auxiliary pump #2
    pub output_pin_aux_pump2: OutputPin,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO of the feeder
    pub output_pin_feeder: OutputPin,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO of the refill pump
    pub output_pin_refill_pump: OutputPin,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO of the heater
    pub output_pin_heater: OutputPin,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO of the surface ventilation
    pub output_pin_ventilation: OutputPin,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO of the peristaltic pump #1
    pub output_pin_peristaltic_pump1: OutputPin,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO of the peristaltic pump #2
    pub output_pin_peristaltic_pump2: OutputPin,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO of the peristaltic pump #3
    pub output_pin_peristaltic_pump3: OutputPin,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// GPIO of the peristaltic pump #4
    pub output_pin_peristaltic_pump4: OutputPin,
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
/// Acquires and configures a specific GPIO pin as an output pin.
///
/// This private helper function attempts to get access to a GPIO pin by its number
/// and then sets its mode to output. This is a critical step for controlling
/// hardware devices connected to the GPIOs.
///
/// # Arguments
/// * `gpio` - A reference to the `Gpio` instance, which provides access to the GPIO interface.
/// * `gpio_number` - The physical GPIO pin to configure.
/// * `device_name` - A descriptive name of the device connected to this pin (used for error logging).
///
/// # Returns
/// A `Result` containing a new `OutputPin` instance on success, ready to be used for setting the pin's state.
///
/// # Errors
/// Returns a `RelayError::GpioGetPinFailure` if the underlying `rppal` library fails
/// to access the pin. This can happen if the pin number is invalid for the hardware,
/// if the pin is already in use by another process, or due to insufficient permissions.
pub fn get_output_pin(
    gpio: &Gpio,
    gpio_number: u8,
    device_name: &str,
) -> Result<OutputPin, RelayError> {
    gpio.get(gpio_number)
        .map(|pin| pin.into_output())
        .map_err(|e| RelayError::GpioGetPinFailure {
            location: module_path!().to_string(),
            pin_number: gpio_number,
            device: device_name.to_string(),
            source: e,
        })
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
impl ActuateGpio {
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Creates a new `ActuateGpio` instance by configuring and acquiring all necessary GPIO output pins.
    ///
    /// This constructor is responsible for initializing direct GPIO hardware control.
    /// It iterates through the `ActuateGpioConfig` to get each device's assigned GPIO pin,
    /// then acquires and sets up each pin as an `OutputPin` using the `rppal` library.
    ///
    /// # Arguments
    /// * `gpio` - A `Gpio` instance, which is the entry point for accessing the GPIO interface
    ///   on a Linux system (e.g., Raspberry Pi).
    /// * `config` - An `ActuateGpioConfig` struct, containing the mapping of aquarium devices to their
    ///   specific GPIO pins.
    ///
    /// # Returns
    /// A `Result` containing a new `ActuateGpio` struct on success, holding all the configured `OutputPin`s.
    ///
    /// # Errors
    /// Returns a `RelayError` if any of the underlying calls to `get_output_pin` fail.
    /// This indicates a critical hardware or configuration error that prevents the
    /// application from controlling the hardware correctly (e.g., invalid pin number,
    /// permission issues).
    pub fn new(gpio: Gpio, config: ActuateGpioConfig) -> Result<ActuateGpio, RelayError> {
        let output_pin_skimmer = get_output_pin(&gpio, config.gpio_skimmer, "skimmer")?;
        let output_pin_main_pump1 = get_output_pin(&gpio, config.gpio_main_pump1, "main pump 1")?;
        let output_pin_main_pump2 = get_output_pin(&gpio, config.gpio_main_pump2, "main pump 2")?;
        let output_pin_aux_pump1 = get_output_pin(&gpio, config.gpio_aux_pump1, "aux pump 1")?;
        let output_pin_aux_pump2 = get_output_pin(&gpio, config.gpio_aux_pump2, "aux pump 2")?;
        let output_pin_feeder = get_output_pin(&gpio, config.gpio_feeder, "feeder")?;
        let output_pin_refill_pump = get_output_pin(&gpio, config.gpio_refill_pump, "refill pump")?;
        let output_pin_heater = get_output_pin(&gpio, config.gpio_heater, "heater")?;
        let output_pin_ventilation = get_output_pin(&gpio, config.gpio_ventilation, "ventilation")?;
        let output_pin_peristaltic_pump1 =
            get_output_pin(&gpio, config.gpio_peristaltic_pump1, "peristaltic pump 1")?;
        let output_pin_peristaltic_pump2 =
            get_output_pin(&gpio, config.gpio_peristaltic_pump2, "peristaltic pump 2")?;
        let output_pin_peristaltic_pump3 =
            get_output_pin(&gpio, config.gpio_peristaltic_pump3, "peristaltic pump 3")?;
        let output_pin_peristaltic_pump4 =
            get_output_pin(&gpio, config.gpio_peristaltic_pump4, "peristaltic pump 4")?;

        Ok(ActuateGpio {
            output_pin_skimmer,
            output_pin_main_pump1,
            output_pin_main_pump2,
            output_pin_aux_pump1,
            output_pin_aux_pump2,
            output_pin_feeder,
            output_pin_refill_pump,
            output_pin_heater,
            output_pin_ventilation,
            output_pin_peristaltic_pump1,
            output_pin_peristaltic_pump2,
            output_pin_peristaltic_pump3,
            output_pin_peristaltic_pump4,
        })
    }
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
impl RelayActuationTrait for ActuateGpio {
    /// Actuates a relay by setting the state of its corresponding GPIO pin.
    ///
    /// This function translates `SwitchOn` and `SwitchOff` commands for specific aquarium devices
    /// into direct GPIO pin manipulations (`set_low()` or `set_high()`). It applies the correct
    /// relay logic (e.g., whether `set_low()` means "on" or "off" for a given device)
    /// by calling `get_relay_command_gpio`.
    ///
    /// This implementation is active only on Linux systems when the `target_hw` feature is enabled.
    ///
    /// # Arguments
    /// * `internal_command` - The `InternalCommand` specifying the `AquariumDevice` and the desired state (`SwitchOn` or `SwitchOff`).
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the GPIO pin state was successfully set.
    ///
    /// # Errors
    /// This function will return a `RelayError` if the command is not applicable:
    /// - `RelayError::IrrelevantCommand`: If the `internal_command` is not `SwitchOn` or `SwitchOff` (e.g., `Pulse`, `RequestSignal`).
    /// - `RelayError::PulseNotAllowedForDevice`: If a `Pulse` command is attempted, as this GPIO implementation does not support it.
    fn actuate(&mut self, internal_command: &InternalCommand) -> Result<(), RelayError> {
        match internal_command {
            InternalCommand::SwitchOn(ref aquarium_device)
            | InternalCommand::SwitchOff(ref aquarium_device) => {
                match get_relay_command_gpio(internal_command, aquarium_device.clone()) {
                    Ok(gpio_target_state) => match gpio_target_state {
                        true => {
                            #[cfg(all(target_os = "linux", feature = "target_hw"))]
                            match aquarium_device {
                                AquariumDevice::Skimmer => {
                                    self.output_pin_skimmer.set_low();
                                }
                                AquariumDevice::MainPump1 => {
                                    self.output_pin_main_pump1.set_low();
                                }
                                AquariumDevice::MainPump2 => {
                                    self.output_pin_main_pump2.set_low();
                                }
                                AquariumDevice::AuxPump1 => {
                                    self.output_pin_aux_pump1.set_low();
                                }
                                AquariumDevice::AuxPump2 => {
                                    self.output_pin_aux_pump2.set_low();
                                }
                                AquariumDevice::Feeder => {
                                    self.output_pin_feeder.set_low();
                                }
                                AquariumDevice::RefillPump => {
                                    self.output_pin_refill_pump.set_low();
                                }
                                AquariumDevice::Heater => {
                                    self.output_pin_heater.set_low();
                                }
                                AquariumDevice::Ventilation => {
                                    self.output_pin_ventilation.set_low();
                                }
                                AquariumDevice::PeristalticPump1 => {
                                    self.output_pin_peristaltic_pump1.set_low();
                                }
                                AquariumDevice::PeristalticPump2 => {
                                    self.output_pin_peristaltic_pump2.set_low();
                                }
                                AquariumDevice::PeristalticPump3 => {
                                    self.output_pin_peristaltic_pump3.set_low();
                                }
                                AquariumDevice::PeristalticPump4 => {
                                    self.output_pin_peristaltic_pump4.set_low();
                                }
                            }
                            Ok(())
                        }
                        false => {
                            #[cfg(all(target_os = "linux", feature = "target_hw"))]
                            match aquarium_device {
                                AquariumDevice::Skimmer => {
                                    self.output_pin_skimmer.set_high();
                                }
                                AquariumDevice::MainPump1 => {
                                    self.output_pin_main_pump1.set_high();
                                }
                                AquariumDevice::MainPump2 => {
                                    self.output_pin_main_pump2.set_high();
                                }
                                AquariumDevice::AuxPump1 => {
                                    self.output_pin_aux_pump1.set_high();
                                }
                                AquariumDevice::AuxPump2 => {
                                    self.output_pin_aux_pump2.set_high();
                                }
                                AquariumDevice::Feeder => {
                                    self.output_pin_feeder.set_high();
                                }
                                AquariumDevice::RefillPump => {
                                    self.output_pin_refill_pump.set_high();
                                }
                                AquariumDevice::Heater => {
                                    self.output_pin_heater.set_high();
                                }
                                AquariumDevice::Ventilation => {
                                    self.output_pin_ventilation.set_high();
                                }
                                AquariumDevice::PeristalticPump1 => {
                                    self.output_pin_peristaltic_pump1.set_high();
                                }
                                AquariumDevice::PeristalticPump2 => {
                                    self.output_pin_peristaltic_pump2.set_high();
                                }
                                AquariumDevice::PeristalticPump3 => {
                                    self.output_pin_peristaltic_pump3.set_high();
                                }
                                AquariumDevice::PeristalticPump4 => {
                                    self.output_pin_peristaltic_pump4.set_high();
                                }
                            }
                            Ok(())
                        }
                    },
                    Err(e) => Err(e),
                }
            }
            _ => {
                warn!(
                    target: module_path!(),
                    "ignoring irrelevant internal command {internal_command}."
                );
                Err(RelayError::IrrelevantCommand(
                    module_path!().to_string(),
                    internal_command.clone(),
                ))
            }
        }
    }

    /// Returns the heartbeat interval, which is not applicable for GPIO actuation.
    ///
    /// GPIO-based relays do not require a keep-alive signal, so this implementation
    /// returns `None`.
    ///
    /// # Returns
    /// Always returns `None`.
    fn get_heartbeat_interval_seconds(&self) -> Option<u64> {
        None
    }

    /// Performs a heartbeat action, which is not applicable for GPIO actuation.
    ///
    /// This is a no-op for the GPIO implementation as there is no persistent
    /// communication channel that needs to be kept alive.
    ///
    /// # Returns
    /// Always returns `Ok(())`.
    ///
    /// # Errors
    /// This function will never return an error.
    fn heartbeat(&mut self) -> Result<(), RelayError> {
        // do nothing
        Ok(())
    }

    /// Flushes a communication buffer, which is not applicable for GPIO actuation.
    ///
    /// This is a no-op for the GPIO implementation as there is no serial communication
    /// buffer to flush.
    ///
    /// # Returns
    /// Always returns `Ok(())`.
    ///
    /// # Errors
    /// This function will never return an error.
    fn flush_buffer(&mut self) -> Result<(), RelayError> {
        // do nothing
        Ok(())
    }
}
