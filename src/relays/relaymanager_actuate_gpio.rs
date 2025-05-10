/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::warn;
use serialport::SerialPort;
use spin_sleep::SpinSleeper;
use std::{sync::mpsc::Sender, time::Duration};
#[cfg(feature = "target_hw")]
use rppal::gpio::Gpio;

use crate::relays::controllino::{ControllinoConfig, ControllinoError, RelayActuationTrait};
use crate::relays::device_relay_logic::{get_relay_command, get_relay_command_gpio};
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};

const LOG_TARGET_RELAY_MANAGER_ACTUATE_GPIO: &str = "relay_manager_actuate_gpio";

/// Contains trait implementation for actuation of relays via GPIO
pub struct RelayManagerActuateGpio {
    pub output_pin_skimmer: Pin<Output>
};

impl RelayActuationTrait for RelayManagerActuateGpio {
    /// Triggers actuation of relays via GPIO
    /// It translates the commands SwitchOn and SwitchOff to SetRelay and UnsetRelay
    /// considering relay id and relay logic.
    /// # Arguments
    /// * `internal_command` - enum value used for identifying content which shall be sent
    fn actuate(
        &mut self,
        internal_command: InternalCommand,
        _tx_to_tcp_opt: Option<&Sender<InternalCommand>>,
        _serial_port_opt: &mut Option<Box<dyn SerialPort>>,
        _spin_sleeper_opt: &Option<SpinSleeper>,
        _controllino_processing_duration_opt: &Option<Duration>,
        _config: &ControllinoConfig,
    ) -> Result<(), ControllinoError> {
        match internal_command {
            InternalCommand::SwitchOn(ref c) | InternalCommand::SwitchOff(ref c) => {
                let gpio_target_state = match get_relay_command_gpio(&internal_command, c.clone());
                match c {
                    AquariumDevice::Skimmer => { self.gp },
                    AquariumDevice::MainPump1 => {}
                    AquariumDevice::MainPump2 => {}
                    AquariumDevice::AuxPump1 => {}
                    AquariumDevice::AuxPump2 => {}
                    AquariumDevice::Heater => {}
                    AquariumDevice::Ventilation => {}
                    AquariumDevice::RefillPump => {}
                    AquariumDevice::Feeder => {}
                    AquariumDevice::PeristalticPump1 => {}
                    AquariumDevice::PeristalticPump2 => {}
                    AquariumDevice::PeristalticPump3 => {}
                    AquariumDevice::PeristalticPump4 => {}
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
