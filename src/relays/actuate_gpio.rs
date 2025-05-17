/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::warn;

use crate::relays::device_relay_logic::get_relay_command_gpio;
use crate::relays::relay_manager::{RelayActuationTrait, RelayError};
use crate::utilities::channel_content::InternalCommand;

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
        use crate::sensors::gpio_handler::GpioHandlerConfig;
        use rppal::gpio::{Gpio, OutputPin};
        use crate::utilities::channel_content::AquariumDevice;
    }
}

const LOG_TARGET_RELAY_MANAGER_ACTUATE_GPIO: &str = "relay_manager_actuate_gpio";

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
fn get_output_pin(gpio: &Gpio, gpio_number: u8, device_name: &str) -> OutputPin {
    match gpio.get(gpio_number) {
        Ok(c) => c.into_output(),
        Err(e) => {
            panic!(
                "{}: could not get pin {} from GPIO interface for {} ({e:?})",
                LOG_TARGET_RELAY_MANAGER_ACTUATE_GPIO, gpio_number, device_name
            );
        }
    }
}

impl ActuateGpio {
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub fn new(gpio: Gpio, config: ActuateGpioConfig) -> ActuateGpio {
        let output_pin_skimmer = get_output_pin(&gpio, config.gpio_skimmer, "skimmer");
        let output_pin_main_pump1 = get_output_pin(&gpio, config.gpio_main_pump1, "main pump 1");
        let output_pin_main_pump2 = get_output_pin(&gpio, config.gpio_main_pump2, "main pump 2");
        let output_pin_aux_pump1 = get_output_pin(&gpio, config.gpio_aux_pump1, "aux pump 1");
        let output_pin_aux_pump2 = get_output_pin(&gpio, config.gpio_aux_pump2, "aux pump 2");
        let output_pin_feeder = get_output_pin(&gpio, config.gpio_feeder, "feeder");
        let output_pin_refill_pump = get_output_pin(&gpio, config.gpio_refill_pump, "refill pump");
        let output_pin_heater = get_output_pin(&gpio, config.gpio_heater, "heater");
        let output_pin_ventilation = get_output_pin(&gpio, config.gpio_ventilation, "ventilation");
        let output_pin_peristaltic_pump1 =
            get_output_pin(&gpio, config.gpio_peristaltic_pump1, "peristaltic pump 1");
        let output_pin_peristaltic_pump2 =
            get_output_pin(&gpio, config.gpio_peristaltic_pump2, "peristaltic pump 2");
        let output_pin_peristaltic_pump3 =
            get_output_pin(&gpio, config.gpio_peristaltic_pump3, "peristaltic pump 3");
        let output_pin_peristaltic_pump4 =
            get_output_pin(&gpio, config.gpio_peristaltic_pump4, "peristaltic pump 4");

        ActuateGpio {
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
        }
    }
}

impl RelayActuationTrait for ActuateGpio {
    /// Triggers actuation of relays via GPIO
    /// It translates the commands SwitchOn and SwitchOff to SetRelay and UnsetRelay
    /// considering relay id and relay logic.
    /// # Arguments
    /// * `internal_command` - enum value used for identifying content which shall be sent
    fn actuate(&mut self, internal_command: InternalCommand) -> Result<(), RelayError> {
        match internal_command {
            InternalCommand::SwitchOn(ref aquarium_device)
            | InternalCommand::SwitchOff(ref aquarium_device) => {
                match get_relay_command_gpio(&internal_command, aquarium_device.clone()) {
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
                            self.output_pin_skimmer.set_high();
                            Ok(())
                        }
                    },
                    Err(e) => Err(e),
                }
            }
            _ => {
                warn!(
                    target: LOG_TARGET_RELAY_MANAGER_ACTUATE_GPIO,
                    "ignoring irrelevant internal command {}.",
                    internal_command
                );
                Err(RelayError::IrrelevantCommand(internal_command))
            }
        }
    }
}
