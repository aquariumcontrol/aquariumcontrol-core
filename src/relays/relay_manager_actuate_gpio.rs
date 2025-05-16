/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::warn;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use rppal::gpio::{Gpio, OutputPin};

use crate::relays::controllino::{ControllinoError, RelayActuationTrait};
use crate::relays::device_relay_logic::get_relay_command_gpio;
use crate::utilities::channel_content::InternalCommand;

const LOG_TARGET_RELAY_MANAGER_ACTUATE_GPIO: &str = "relay_manager_actuate_gpio";

/// Contains trait implementation for actuation of relays via GPIO
pub struct RelayManagerActuateGpio {
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub output_pin_skimmer: OutputPin,
}

impl RelayManagerActuateGpio {
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub fn new(gpio: Gpio, gpio_skimmer: u8) -> RelayManagerActuateGpio {
        // get pin for actuating skimmer
        let output_pin_skimmer = match gpio.get(gpio_skimmer) {
            Ok(c) => c.into_output(),
            Err(e) => {
                panic!(
                    "{}: could not get pin {} from GPIO interface for skimmer relay ({e:?})",
                    LOG_TARGET_RELAY_MANAGER_ACTUATE_GPIO, gpio_skimmer
                );
            }
        };

        RelayManagerActuateGpio {
            #[cfg(all(target_os = "linux", feature = "target_hw"))]
            output_pin_skimmer,
        }
    }
}

impl RelayActuationTrait for RelayManagerActuateGpio {
    /// Triggers actuation of relays via GPIO
    /// It translates the commands SwitchOn and SwitchOff to SetRelay and UnsetRelay
    /// considering relay id and relay logic.
    /// # Arguments
    /// * `internal_command` - enum value used for identifying content which shall be sent
    fn actuate(
        &mut self,
        internal_command: InternalCommand,
    ) -> Result<(), ControllinoError> {
        match internal_command {
            InternalCommand::SwitchOn(ref aquarium_device)
            | InternalCommand::SwitchOff(ref aquarium_device) => {
                match get_relay_command_gpio(&internal_command, aquarium_device.clone()) {
                    Ok(gpio_target_state) => match gpio_target_state {
                        true => {
                            #[cfg(all(target_os = "linux", feature = "target_hw"))]
                            self.output_pin_skimmer.set_low();
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
                Err(ControllinoError::IrrelevantCommand(internal_command))
            }
        }
    }
}
