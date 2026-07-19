/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::test_server::test_server_channel_content::{
    BallingFluidInjection, TestServerInternalCommand,
};
use crate::utilities::channel_content::AquariumDevice;
use serde_derive::Deserialize;
use std::thread;
use std::time::Duration;

/// This struct holds the configuration data for the Controllino device simulation.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct ControllinoEmulatorConfig {
    /// Relay id of the protein skimmer
    /// Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub skimmer_id: u16,

    /// Relay id of main pump #1
    /// Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub main_pump1_id: u16,

    /// Relay id of main pump #2
    /// Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub main_pump2_id: u16,

    /// Relay id of auxiliary pump #1
    /// Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub aux_pump1_id: u16,

    /// Relay id of auxiliary pump #2
    /// Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub aux_pump2_id: u16,

    /// Relay id of refill pump
    pub refill_pump_id: u16,

    /// Relay id of heater
    pub heater_id: u16,

    /// Relay id of ventilation
    pub ventilation_id: u16,

    /// Relay id feeder
    pub feeder_id: u16,

    /// Relay id of peristaltic pump #1
    pub peristaltic_pump1_id: u16,

    /// Relay id of peristaltic pump #2
    pub peristaltic_pump2_id: u16,

    /// Relay id of peristaltic pump #3
    pub peristaltic_pump3_id: u16,

    /// Relay id of peristaltic pump #4
    pub peristaltic_pump4_id: u16,
}

/// This struct provides states and functionalities to simulate the Controllino device.
pub struct ControllinoEmulator<'a> {
    /// The state of the relays is stored in this vector. Caution: relay state (true or false) and device state (On or Off) may have reverse relationship.
    pub relays: Vec<bool>,

    /// The configuration data read from the .toml file.
    pub config: &'a ControllinoEmulatorConfig,
}

impl<'a> ControllinoEmulator<'a> {
    /// The function returns a variable of type Controllino.
    ///
    /// # Arguments:
    ///
    /// * `config_controllino` - parameter of type ControllinoEmulatorConfig that holds the configuration data of the Controllino device.    
    pub fn new(config_controllino: &'a ControllinoEmulatorConfig) -> ControllinoEmulator<'a> {
        ControllinoEmulator {
            relays: vec![false; 16],
            config: config_controllino,
        }
    }

    /// The function implements the device actuation when switching on certain relays.
    /// It returns optionally an internal command which needs to be sent to the control application.
    /// # Arguments:
    ///
    /// * `id` - id of the relay to be switched on.    
    pub fn set_relay(&mut self, id: u16) -> Option<TestServerInternalCommand> {
        if (id as usize) < self.relays.len() {
            self.relays[id as usize] = true;
            if id == self.config.skimmer_id {
                return Some(TestServerInternalCommand::SwitchOff(
                    AquariumDevice::Skimmer,
                ));
                // note: reverse logic of relay (NO)
            }
            if id == self.config.main_pump1_id {
                return Some(TestServerInternalCommand::SwitchOff(
                    AquariumDevice::MainPump1,
                ));
                // note: reverse logic of relay (NO)
            }
            if id == self.config.main_pump2_id {
                return Some(TestServerInternalCommand::SwitchOff(
                    AquariumDevice::MainPump2,
                ));
                // note: reverse logic of relay (NO)
            }
            if id == self.config.aux_pump1_id {
                return Some(TestServerInternalCommand::SwitchOff(
                    AquariumDevice::AuxPump1,
                ));
                // note: reverse logic of relay (NO)
            }
            if id == self.config.aux_pump2_id {
                return Some(TestServerInternalCommand::SwitchOff(
                    AquariumDevice::AuxPump2,
                ));
                // note: reverse logic of relay (NO)
            }
            if id == self.config.refill_pump_id {
                return Some(TestServerInternalCommand::SwitchOn(
                    AquariumDevice::RefillPump,
                ));
            }
            if id == self.config.heater_id {
                return Some(TestServerInternalCommand::SwitchOn(AquariumDevice::Heater));
            }
            if id == self.config.ventilation_id {
                return Some(TestServerInternalCommand::SwitchOn(
                    AquariumDevice::Ventilation,
                ));
            }
            if id == self.config.feeder_id {
                return Some(TestServerInternalCommand::SwitchOn(AquariumDevice::Feeder));
            }
            if id == self.config.peristaltic_pump1_id {
                return Some(TestServerInternalCommand::SwitchOn(
                    AquariumDevice::PeristalticPump1,
                ));
            }
            if id == self.config.peristaltic_pump2_id {
                return Some(TestServerInternalCommand::SwitchOn(
                    AquariumDevice::PeristalticPump2,
                ));
            }
            if id == self.config.peristaltic_pump3_id {
                return Some(TestServerInternalCommand::SwitchOn(
                    AquariumDevice::PeristalticPump3,
                ));
            }
            if id == self.config.peristaltic_pump4_id {
                return Some(TestServerInternalCommand::SwitchOn(
                    AquariumDevice::PeristalticPump4,
                ));
            }
        }
        None
    }

    /// The function implements the device actuation when switching off certain relays.
    /// It returns optionally an internal command which needs to be sent to the control application.
    /// # Arguments:
    ///
    /// * `id` - id of the relay to be switched off.    
    pub fn unset_relay(&mut self, id: u16) -> Option<TestServerInternalCommand> {
        if (id as usize) < self.relays.len() {
            self.relays[id as usize] = false;
            if id == self.config.skimmer_id {
                return Some(TestServerInternalCommand::SwitchOn(AquariumDevice::Skimmer));
                // note: reverse logic of relay (NO)
            }
            if id == self.config.main_pump1_id {
                return Some(TestServerInternalCommand::SwitchOn(
                    AquariumDevice::MainPump1,
                ));
                // note: reverse logic of relay (NO)
            }
            if id == self.config.main_pump2_id {
                return Some(TestServerInternalCommand::SwitchOn(
                    AquariumDevice::MainPump2,
                ));
                // note: reverse logic of relay (NO)
            }
            if id == self.config.aux_pump1_id {
                return Some(TestServerInternalCommand::SwitchOn(
                    AquariumDevice::AuxPump1,
                ));
                // note: reverse logic of relay (NO)
            }
            if id == self.config.aux_pump2_id {
                return Some(TestServerInternalCommand::SwitchOn(
                    AquariumDevice::AuxPump2,
                ));
                // note: reverse logic of relay (NO)
            }
            if id == self.config.refill_pump_id {
                return Some(TestServerInternalCommand::SwitchOff(
                    AquariumDevice::RefillPump,
                ));
                // note: reverse logic of relay (NO)
            }
            if id == self.config.heater_id {
                return Some(TestServerInternalCommand::SwitchOff(AquariumDevice::Heater));
            }
            if id == self.config.ventilation_id {
                return Some(TestServerInternalCommand::SwitchOff(
                    AquariumDevice::Ventilation,
                ));
            }
            if id == self.config.feeder_id {
                return Some(TestServerInternalCommand::SwitchOff(AquariumDevice::Feeder));
            }
            if id == self.config.peristaltic_pump1_id {
                return Some(TestServerInternalCommand::SwitchOff(
                    AquariumDevice::PeristalticPump1,
                ));
            }
            if id == self.config.peristaltic_pump2_id {
                return Some(TestServerInternalCommand::SwitchOff(
                    AquariumDevice::PeristalticPump2,
                ));
            }
            if id == self.config.peristaltic_pump3_id {
                return Some(TestServerInternalCommand::SwitchOff(
                    AquariumDevice::PeristalticPump3,
                ));
            }
            if id == self.config.peristaltic_pump4_id {
                return Some(TestServerInternalCommand::SwitchOff(
                    AquariumDevice::PeristalticPump4,
                ));
            }
        }
        None
    }

    /// Function returns an unsigned int where the bits represent the relay states.
    pub fn get_status(&self) -> u32 {
        let mut result: u32 = 0;

        for id in 0..self.relays.len() {
            result += 2_u32.pow(id as u32) * self.relays[id] as u32;
        }
        result
    }

    /// Function is implementing pulsing a relay: Switching on, waiting for a short time period and switching the relay off
    /// Function is implemented only for the peristaltic pumps.
    /// # Arguments:
    ///
    /// * `id` - id of the relay to be pulsed.        
    /// * `duration_ms` - id of the relay to be pulsed.
    pub fn pulse(&mut self, id: u16, duration_ms: u32) -> Option<TestServerInternalCommand> {
        if id > 0 && (id as usize) < self.relays.len() {
            self.relays[id as usize] = true;

            // This function is blocking, and should not be used in async functions.
            let duration = Duration::from_millis(duration_ms.into());

            thread::sleep(duration);

            self.relays[id as usize] = false;

            // @todo: external mapping for peristaltic pump id to Balling fluid type
            if id == self.config.peristaltic_pump1_id {
                return Some(TestServerInternalCommand::Inject(
                    BallingFluidInjection::Calcium(duration_ms),
                ));
            }
            if id == self.config.peristaltic_pump2_id {
                return Some(TestServerInternalCommand::Inject(
                    BallingFluidInjection::Magnesium(duration_ms),
                ));
            }
            if id == self.config.peristaltic_pump3_id {
                return Some(TestServerInternalCommand::Inject(
                    BallingFluidInjection::KH(duration_ms),
                ));
            }
            if id == self.config.peristaltic_pump4_id {
                return Some(TestServerInternalCommand::Inject(
                    BallingFluidInjection::NotConnected,
                ));
            }
        }
        None
    }

    /// Placeholder for sending an alive signal.
    pub fn heartbeat(&mut self) {
        // todo: watchdog implementation
    }
}
