/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::relays::controllino_message::{ControllinoMessage, ControllinoMessageContent};
use crate::relays::relay_manager::{RelayActuationTrait, RelayError};
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
use log::{error, warn};
use serde_derive::Deserialize;
use serialport::{ClearBuffer, SerialPort};
use spin_sleep::SpinSleeper;
use std::collections::HashSet;
use std::time::Duration;

const LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE: &str = "controllino_actuate_hardware";

const CONTROLLINO_MAX_RELAY_ID: u8 = 64;
const CONTROLLINO_MIN_RELAY_ID: u8 = 1;

/// Contains constants for communication with Controllino
#[allow(unused)]
pub mod controllino_constants {
    /// the size of ControllinoMessage is deliberately set here because checksums are added to content of struct
    pub const MESSAGE_SIZE: usize = 8;

    /// command for setting a single relay
    pub const COMMAND_SET_RELAY: char = 'S';

    /// command for unsetting a single relay
    pub const COMMAND_UNSET_RELAY: char = 'U';

    /// command for pulsing a relay
    pub const COMMAND_PULSE_RELAY: char = 'P';

    /// command for getting relay status
    pub const COMMAND_GET_RELAY_STATUS: char = 'G';

    /// command for sending a heart beat from control application to Controllino
    pub const COMMAND_HEARTBEAT: char = 'H';

    /// command for setting a digital output    
    pub const COMMAND_SET_DIGITAL_OUT: char = 'D';

    /// command for unsetting a digital output    
    pub const COMMAND_UNSET_DIGITAL_OUT: char = 'J';

    /// command for getting digital IO status
    pub const COMMAND_GET_DIGITAL_OUT_STATUS: char = 'G';

    /// initial wait period after resetting DTR
    pub const SLEEP_AFTER_DTR_RESET_MILLIS: u64 = 1000;

    /// initial wait period after clearing buffer
    pub const SLEEP_AFTER_CLEAR_BUFFER_MILLIS: u64 = 2000;
}

/// Calculates the checksums of the byte sequence received from Controllino
/// and compares it with the checksums received
/// # Arguments
/// * `data` - array of u8
fn check_controllino_checksums(data: &[u8]) -> bool {
    let first_checksum = data[0] ^ data[1] ^ data[2];
    let second_checksum = data[4] ^ data[5] ^ data[6];
    let ok_result = (data[3] == first_checksum) && (data[7] == second_checksum);
    if !ok_result {
        warn!(target: LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE,
            "data used for checksum calculation: \n\
            [0]={} [1]={} [2]={} [3]={} [4]={} [5]={} [6]={} [7]={}\n\
            first checksum ([3]) should have been {}\n\
            second checksum ([7]) should have been {}",
            data[0], data[1], data[2], data[3],
            data[4], data[5], data[6], data[7],
            first_checksum,
            second_checksum,
        );
    }
    ok_result
}

#[derive(Clone, Deserialize, Debug)]
pub struct ActuateControllinoConfig {
    /// Baud rate for communication via serial port
    baud_rate: u32,

    /// Port name for communication via serial port
    port_name: String,

    /// timeout in milliseconds for serial port communication
    timeout_millis: u64,

    /// time in milliseconds to wait for response
    controllino_processing_millis: u64,

    /// Relay id of the protein skimmer
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub skimmer_id: u8,

    /// Relay id of the main pump #1
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub main_pump1_id: u8,

    /// Relay id of the main pump #2
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub main_pump2_id: u8,

    /// Relay id of the auxiliary pump #1
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub aux_pump1_id: u8,

    /// Relay id of the auxiliary pump #2
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub aux_pump2_id: u8,

    /// Relay id of the refill pump
    pub refill_pump_id: u8,

    /// Relay id of the heater
    pub heater_id: u8,

    /// Relay id of ventilation
    pub ventilation_id: u8,

    /// Relay id feeder
    pub feeder_id: u8,

    /// Relay id of peristaltic pump #1
    pub peristaltic_pump1_id: u8,

    /// Relay id of peristaltic pump #2
    pub peristaltic_pump2_id: u8,

    /// Relay id of peristaltic pump #3
    pub peristaltic_pump3_id: u8,

    /// Relay id of peristaltic pump #4
    pub peristaltic_pump4_id: u8,
}

/// Contains relay configuration and trait implementation for actuation using Controllino hardware
pub struct ActuateControllino {
    config: ActuateControllinoConfig,

    /// Object is used frequently to wait for a determined period of time, so storing it instead of recreating when needed.
    /// Attribute is public because access from within test cases.
    pub spin_sleeper: SpinSleeper,

    /// Object is used frequently to wait for a determined period of time, so storing it instead of recreating when needed.
    /// Attribute is public because access from within test cases.
    pub controllino_processing_duration: Duration,

    /// Interface to serial port for communication via USB
    /// Attribute is public because access from within test cases.
    pub serial_port: Box<dyn SerialPort>,
}

/// Creates the message to be sent to Controllino
/// # Arguments
/// * `internal_command` - enum value used for identifying if relay shall be switched on or switched off
/// * `config` - configuration of Controllino
fn create_command_message(
    internal_command: InternalCommand,
    config: &ActuateControllinoConfig,
) -> Result<ControllinoMessage, RelayError> {
    match internal_command {
        InternalCommand::SwitchOn(ref c)
        | InternalCommand::SwitchOff(ref c)
        | InternalCommand::Pulse(ref c, _) => {
            let relay_command = match get_relay_command(&internal_command, c.clone(), config) {
                Ok(c) => c,
                Err(e) => {
                    return Err(e);
                }
            };
            let (relay_command_char, relay_id, duration_millis) = match relay_command {
                InternalCommand::SetRelay(relay_id) => {
                    // relay id is set inside this crate, so cast is safe
                    (controllino_constants::COMMAND_SET_RELAY, relay_id as u8, 0)
                }
                InternalCommand::UnsetRelay(relay_id) => {
                    // relay id is set inside this crate, so cast is safe
                    (
                        controllino_constants::COMMAND_UNSET_RELAY,
                        relay_id as u8,
                        0,
                    )
                }
                InternalCommand::PulseRelay(relay_id, duration_millis) => {
                    // relay id is set inside this crate, so cast is safe
                    (
                        controllino_constants::COMMAND_PULSE_RELAY,
                        relay_id as u8,
                        duration_millis,
                    )
                }
                _ => {
                    return Err(RelayError::IrrelevantCommand(internal_command));
                }
            };
            let controllino_message_content =
                ControllinoMessageContent::new(relay_command_char, relay_id, duration_millis);
            let controllino_message = ControllinoMessage::new(controllino_message_content);
            Ok(controllino_message)
        }
        _ => Err(RelayError::IrrelevantCommand(internal_command)),
    }
}

/// Provides one of two enum values of type InternalCommand wrapping the relay number.
/// Alternative relay logic can be considered here by choosing either SetRelay or UnsetRelay.
/// # Arguments
/// * `internal_command` - enum value which is interpreted
/// * `device` - enum value for which the function shall provide the relay id
/// * `config` - Controllino configuration read from .toml file
pub fn get_relay_command(
    internal_command: &InternalCommand,
    device: AquariumDevice,
    config: &ActuateControllinoConfig,
) -> Result<InternalCommand, RelayError> {
    match device {
        AquariumDevice::Skimmer => match internal_command {
            InternalCommand::SwitchOn(_) => {
                Ok(InternalCommand::UnsetRelay(config.skimmer_id as u16))
            }
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::SetRelay(config.skimmer_id as u16))
            }
            InternalCommand::Pulse(d, _) => Err(RelayError::PulseNotAllowedForDevice(d.clone())),
            _ => Err(RelayError::IrrelevantCommand(internal_command.clone())),
        },
        AquariumDevice::MainPump1 => match internal_command {
            InternalCommand::SwitchOn(_) => {
                Ok(InternalCommand::UnsetRelay(config.main_pump1_id as u16))
            }
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::SetRelay(config.main_pump1_id as u16))
            }
            InternalCommand::Pulse(d, _) => Err(RelayError::PulseNotAllowedForDevice(d.clone())),
            _ => Err(RelayError::IrrelevantCommand(internal_command.clone())),
        },
        AquariumDevice::MainPump2 => match internal_command {
            InternalCommand::SwitchOn(_) => {
                Ok(InternalCommand::UnsetRelay(config.main_pump2_id as u16))
            }
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::SetRelay(config.main_pump2_id as u16))
            }
            InternalCommand::Pulse(d, _) => Err(RelayError::PulseNotAllowedForDevice(d.clone())),
            _ => Err(RelayError::IrrelevantCommand(internal_command.clone())),
        },
        AquariumDevice::AuxPump1 => match internal_command {
            InternalCommand::SwitchOn(_) => {
                Ok(InternalCommand::UnsetRelay(config.aux_pump1_id as u16))
            }
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::SetRelay(config.aux_pump1_id as u16))
            }
            InternalCommand::Pulse(d, _) => Err(RelayError::PulseNotAllowedForDevice(d.clone())),
            _ => Err(RelayError::IrrelevantCommand(internal_command.clone())),
        },
        AquariumDevice::AuxPump2 => match internal_command {
            InternalCommand::SwitchOn(_) => {
                Ok(InternalCommand::UnsetRelay(config.aux_pump2_id as u16))
            }
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::SetRelay(config.aux_pump2_id as u16))
            }
            InternalCommand::Pulse(d, _) => Err(RelayError::PulseNotAllowedForDevice(d.clone())),
            _ => Err(RelayError::IrrelevantCommand(internal_command.clone())),
        },
        AquariumDevice::Heater => match internal_command {
            InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(config.heater_id as u16)),
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::UnsetRelay(config.heater_id as u16))
            }
            InternalCommand::Pulse(d, _) => Err(RelayError::PulseNotAllowedForDevice(d.clone())),
            _ => Err(RelayError::IrrelevantCommand(internal_command.clone())),
        },
        AquariumDevice::Ventilation => match internal_command {
            InternalCommand::SwitchOn(_) => {
                Ok(InternalCommand::SetRelay(config.ventilation_id as u16))
            }
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::UnsetRelay(config.ventilation_id as u16))
            }
            InternalCommand::Pulse(d, _) => Err(RelayError::PulseNotAllowedForDevice(d.clone())),
            _ => Err(RelayError::IrrelevantCommand(internal_command.clone())),
        },
        AquariumDevice::RefillPump => match internal_command {
            InternalCommand::SwitchOn(_) => {
                Ok(InternalCommand::SetRelay(config.refill_pump_id as u16))
            }
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::UnsetRelay(config.refill_pump_id as u16))
            }
            InternalCommand::Pulse(d, _) => Err(RelayError::PulseNotAllowedForDevice(d.clone())),
            _ => Err(RelayError::IrrelevantCommand(internal_command.clone())),
        },
        AquariumDevice::Feeder => match internal_command {
            InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(config.feeder_id as u16)),
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::UnsetRelay(config.feeder_id as u16))
            }
            InternalCommand::Pulse(d, _) => Err(RelayError::PulseNotAllowedForDevice(d.clone())),
            _ => Err(RelayError::IrrelevantCommand(internal_command.clone())),
        },
        AquariumDevice::PeristalticPump1 => match internal_command {
            InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(
                config.peristaltic_pump1_id as u16,
            )),
            InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(
                config.peristaltic_pump1_id as u16,
            )),
            InternalCommand::Pulse(_, t) => Ok(InternalCommand::PulseRelay(
                config.peristaltic_pump1_id as u16,
                *t,
            )),
            _ => Err(RelayError::IrrelevantCommand(internal_command.clone())),
        },
        AquariumDevice::PeristalticPump2 => match internal_command {
            InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(
                config.peristaltic_pump2_id as u16,
            )),
            InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(
                config.peristaltic_pump2_id as u16,
            )),
            InternalCommand::Pulse(_, t) => Ok(InternalCommand::PulseRelay(
                config.peristaltic_pump2_id as u16,
                *t,
            )),
            _ => Err(RelayError::IrrelevantCommand(internal_command.clone())),
        },
        AquariumDevice::PeristalticPump3 => match internal_command {
            InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(
                config.peristaltic_pump3_id as u16,
            )),
            InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(
                config.peristaltic_pump3_id as u16,
            )),
            InternalCommand::Pulse(_, t) => Ok(InternalCommand::PulseRelay(
                config.peristaltic_pump3_id as u16,
                *t,
            )),
            _ => Err(RelayError::IrrelevantCommand(internal_command.clone())),
        },
        AquariumDevice::PeristalticPump4 => match internal_command {
            InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(
                config.peristaltic_pump4_id as u16,
            )),
            InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(
                config.peristaltic_pump4_id as u16,
            )),
            InternalCommand::Pulse(_, t) => Ok(InternalCommand::PulseRelay(
                config.peristaltic_pump4_id as u16,
                *t,
            )),
            _ => Err(RelayError::IrrelevantCommand(internal_command.clone())),
        },
    }
}

impl RelayActuationTrait for ActuateControllino {
    /// Triggers actuation on Controllino hardware via serial port communication
    /// It translates the commands SwitchOn and SwitchOff to SetRelay and UnsetRelay
    /// considering relay id and relay logic.
    /// # Arguments
    /// * `internal_command` - enum value used for identifying content which shall be sent
    fn actuate(&mut self, internal_command: InternalCommand) -> Result<(), RelayError> {
        match internal_command {
            InternalCommand::SwitchOn(ref _c)
            | InternalCommand::SwitchOff(ref _c)
            | InternalCommand::Pulse(ref _c, _) => {
                // create a message to be sent to Controllino
                let message = match create_command_message(internal_command.clone(), &self.config) {
                    Ok(c) => c,
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE,
                            "preparation of communication failed ({e:?})"
                        );
                        return Err(RelayError::FailureCommandMessageCreation);
                    }
                };

                // write a message to Controllino
                match self.serial_port.write(&message.data) {
                    Ok(amount_byte_written) => {
                        if amount_byte_written != message.data.len() {
                            warn!(
                                    "{}: Writing to Controllino device failed. Amount of bytes written: {}. Expected: {}",
                                    LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE,
                                    amount_byte_written,
                                    message.data.len()
                                );
                        }
                    }
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE,
                            "sending command to Controllino failed ({e:?})"
                        );
                        return Err(RelayError::WriteError);
                    }
                }

                // give Controllino some time to process the command
                self.spin_sleeper
                    .sleep(self.controllino_processing_duration);

                // for pulse command, also wait the pulse duration
                match internal_command {
                    InternalCommand::Pulse(_, t) => {
                        let pulse_duration = Duration::from_millis(t as u64);
                        self.spin_sleeper.sleep(pulse_duration);
                    }
                    _ => { /* do nothing */ }
                }

                // read response from Controllino
                let mut serial_buf: Vec<u8> = vec![0; controllino_constants::MESSAGE_SIZE];
                match self.serial_port.read_exact(serial_buf.as_mut_slice()) {
                    Ok(()) => {
                        if !check_controllino_checksums(serial_buf.as_slice()) {
                            warn!(
                                target: LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE,
                                "Received response from device with incorrect checksums"
                            );
                            Err(RelayError::IncorrectChecksum)
                        } else {
                            Ok(())
                        }
                    }
                    Err(_) => {
                        error!(
                            target: LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE,
                            "receiving response from device failed."
                        );
                        Err(RelayError::ReadError)
                    }
                }
            }
            _ => {
                warn!(
                    target: LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE,
                    "ignoring irrelevant internal command {}.",
                    internal_command
                );
                Err(RelayError::IrrelevantCommand(internal_command))
            }
        }
    }
}

fn controllino_open_serial_port(
    port_name: String,
    baud_rate: u32,
    timeout_millis: u64,
) -> Box<dyn SerialPort> {
    let spin_sleeper = SpinSleeper::default();

    let mut serial_port = match serialport::new(port_name.clone(), baud_rate).open() {
        Ok(c) => c,
        Err(e) => {
            panic!(
                "{}: failed to open serial port ({e:?})",
                LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE
            );
        }
    };

    let timeout_duration = Duration::from_millis(timeout_millis);
    match serial_port.set_timeout(timeout_duration) {
        Ok(()) => { /* do nothing */ }
        Err(_) => {
            panic!(
                "{}: could not set timeout of {} milliseconds.",
                LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE, timeout_millis
            );
        }
    }

    // write to Data Terminal Ready pin: clear the signal
    match serial_port.write_data_terminal_ready(false) {
        Ok(_) => { /* could reset DTR - do nothing */ }
        Err(e) => {
            panic!(
                "{}: error when clearing DTR ({e:?})",
                LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE
            );
        }
    }

    // wait for the device
    let sleep_duration_after_dtr_reset =
        Duration::from_millis(controllino_constants::SLEEP_AFTER_DTR_RESET_MILLIS);
    spin_sleeper.sleep(sleep_duration_after_dtr_reset);

    // flush the buffer
    match serial_port.clear(ClearBuffer::All) {
        Ok(_) => { /* could clear buffer - do nothing */ }
        Err(e) => {
            panic!(
                "{}: error when clearing buffer ({e:?})",
                LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE
            );
        }
    }

    // wait for the device
    let sleep_duration_after_buffer_clear =
        Duration::from_millis(controllino_constants::SLEEP_AFTER_CLEAR_BUFFER_MILLIS);
    spin_sleeper.sleep(sleep_duration_after_buffer_clear);

    serial_port
}

impl ActuateControllino {
    /// Checks if the relay ID configuration is valid:
    /// - IDs shall be within a permissible range.
    /// - IDs shall be distinct.
    /// # Arguments
    /// * `config` - configuration of Controllino relays
    pub fn check_valid_relay_id_config(config: &ActuateControllinoConfig) -> bool {
        // check if relay ids are in valid range and if they are distinct
        let mut seen_values = HashSet::new();

        let relay_id_values = [
            config.aux_pump1_id,
            config.aux_pump2_id,
            config.feeder_id,
            config.heater_id,
            config.main_pump1_id,
            config.main_pump2_id,
            config.peristaltic_pump1_id,
            config.peristaltic_pump2_id,
            config.peristaltic_pump3_id,
            config.peristaltic_pump4_id,
            config.refill_pump_id,
            config.ventilation_id,
        ];

        for &value in &relay_id_values {
            if value < CONTROLLINO_MIN_RELAY_ID || value > CONTROLLINO_MAX_RELAY_ID {
                #[cfg(not(test))]
                error!(
                    target: LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE,
                    "Not all relay IDs are inside permissible range."
                );
                #[cfg(test)]
                println!("Not all relay IDs are inside permissible range.");
                return false; // Value is out of the allowed range
            }
            if !seen_values.insert(value) {
                #[cfg(not(test))]
                error!(
                    target: LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE,
                    "Not all relay IDs are distinct - duplicate values found."
                );
                #[cfg(test)]
                println!("Not all relay IDs are distinct - duplicate values found.");
                return false; // Duplicate value found
            }
        }

        true
    }

    /// Creates a struct of type ActuateControllino which is used from the relay manager.
    /// # Arguments
    /// * `config` - configuration of Controllino relays
    pub fn new(config: ActuateControllinoConfig) -> ActuateControllino {
        let spin_sleeper = SpinSleeper::default();

        // check validity of configuration
        if Self::check_valid_relay_id_config(&config) {
            panic!(
                "{}: Relay ID configuration is invalid.",
                LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE
            );
        }

        if config.port_name.is_empty() {
            panic!(
                "{}: Port name for communication with Controllino is empty.",
                LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE
            );
        }

        let serial_port = controllino_open_serial_port(
            config.port_name.clone(),
            config.baud_rate,
            config.timeout_millis,
        );
        let controllino_processing_millis = config.controllino_processing_millis;
        let controllino_processing_duration = Duration::from_millis(controllino_processing_millis);

        ActuateControllino {
            config,
            spin_sleeper,
            controllino_processing_duration,
            serial_port,
        }
    }
}

#[cfg(test)]
pub mod tests {
    // feature-specific imports
    cfg_if::cfg_if! {
        if #[cfg(feature = "controllino_hw")] {
            use serialport::SerialPort;
            use spin_sleep::SpinSleeper;
            use std::time::Duration;

            use crate::relays::controllino::{Controllino, RelayActuationTrait};
            use crate::relays::controllino_actuate_hardware::ControllinoActuateHardware;
        }
    }

    use crate::utilities::channel_content::AquariumDevice::{
        AuxPump1, AuxPump2, Feeder, Heater, MainPump1, MainPump2, PeristalticPump1,
        PeristalticPump2, PeristalticPump3, PeristalticPump4, Skimmer, Ventilation,
    };

    use crate::relays::actuate_controllino::{
        controllino_constants, create_command_message, ActuateControllino,
        CONTROLLINO_MAX_RELAY_ID, CONTROLLINO_MIN_RELAY_ID,
    };
    use crate::relays::relay_manager::RelayError;
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::config::{read_config_file, ConfigData};
    #[test]
    #[should_panic]
    pub fn test_invalid_relay_id_config_low() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());

        config.controllino_relay.main_pump1_id = CONTROLLINO_MIN_RELAY_ID - 1;

        let _actuate_controllino = ActuateControllino::new(config.controllino_relay);
    }
    #[test]
    #[should_panic]
    pub fn test_invalid_relay_id_config_high() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());

        config.controllino_relay.main_pump1_id = CONTROLLINO_MAX_RELAY_ID + 1;

        let _actuate_controllino = ActuateControllino::new(config.controllino_relay);
    }
    #[test]
    #[should_panic]
    pub fn test_invalid_relay_config_duplicate() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());

        config.controllino_relay.main_pump1_id = CONTROLLINO_MAX_RELAY_ID;
        config.controllino_relay.main_pump2_id = CONTROLLINO_MAX_RELAY_ID;

        let _gpio_handler = ActuateControllino::new(config.controllino_relay);
    }

    #[cfg(all(target_os = "macos", feature = "controllino_hw"))]
    fn get_platform_specific_configuration_file() -> String {
        "/config/aquarium_control_test_controllino_macos.toml".to_string()
    }

    #[cfg(all(target_os = "linux", feature = "controllino_hw"))]
    fn get_platform_specific_configuration_file() -> String {
        "/config/aquarium_control_test_controllino_linux.toml".to_string()
    }

    #[cfg(feature = "controllino_hw")]
    #[test]
    // Test case actuates on real hardware.
    // Execution of this test case requires serial port connection to the Controllino device.
    pub fn test_controllino_switch_on_off_pulse() {
        let sleep_duration_500_millis = Duration::from_millis(500);
        let spin_sleeper = SpinSleeper::default();

        let config: ConfigData = read_config_file(get_platform_specific_configuration_file());
        let controllino = Controllino::new(config.relay_manager);

        let config2: ConfigData = read_config_file(get_platform_specific_configuration_file());

        let serial_port: Box<dyn SerialPort>;
        serial_port = match controllino.serial_port_opt.as_ref().unwrap().try_clone() {
            Ok(c) => c,
            Err(_) => {
                panic!("Controllino: Error occurred when trying to clone serial port.");
            }
        };

        let mut actuator = ControllinoActuateHardware;

        let expect_info =
            "Controllino test_switch_on_off_pulse failed cloning serial port interface.";

        // Request Controllino to switch on and off all relays sequentially
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(Skimmer),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(Ventilation),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(actuator.actuate(InternalCommand::SwitchOn(Heater),), Ok(()));
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(MainPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(MainPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(AuxPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(AuxPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(PeristalticPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(PeristalticPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(PeristalticPump3),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(PeristalticPump4),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(actuator.actuate(InternalCommand::SwitchOn(Feeder),), Ok(()));
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(Skimmer),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(Ventilation),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(Heater),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(MainPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(MainPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(AuxPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(AuxPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(PeristalticPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(PeristalticPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(PeristalticPump3),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(PeristalticPump4),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(Feeder),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);

        // Request Controllino to pulse all relays for peristaltic pumps
        assert_eq!(
            actuator.actuate(InternalCommand::Pulse(PeristalticPump1, 500),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::Pulse(PeristalticPump2, 500),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::Pulse(PeristalticPump3, 500),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::Pulse(PeristalticPump4, 500),),
            Ok(())
        );
    }

    #[test]
    /// Test case tests if an invalid command is rejected when preparing the message for Controllino.
    pub fn test_create_message_invalid_command() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_simulator.toml".to_string());
        assert_eq!(
            create_command_message(InternalCommand::ResetAllErrors, &config.controllino_relay),
            Err(RelayError::IrrelevantCommand(
                InternalCommand::ResetAllErrors
            )),
        );
    }

    #[test]
    // Test case tests the implementation of the relay logic.
    // During the creation of the message to Controllino, the application considers
    // if relay needs to bet set or unset to switch on/off a device.
    pub fn test_controllino_create_message_switch_on_with_set_relay_switch_off_with_unset_relay() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_simulator.toml".to_string());

        // switching on all devices
        let mut message = create_command_message(
            InternalCommand::SwitchOn(Skimmer),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.skimmer_id);

        message = create_command_message(
            InternalCommand::SwitchOn(Ventilation),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.ventilation_id);

        message =
            create_command_message(InternalCommand::SwitchOn(Heater), &config.controllino_relay)
                .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.heater_id);

        message = create_command_message(
            InternalCommand::SwitchOn(MainPump1),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.main_pump1_id);

        message = create_command_message(
            InternalCommand::SwitchOn(MainPump2),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.main_pump2_id);

        message = create_command_message(
            InternalCommand::SwitchOn(AuxPump1),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.aux_pump1_id);

        message = create_command_message(
            InternalCommand::SwitchOn(AuxPump2),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.aux_pump2_id);

        message = create_command_message(
            InternalCommand::SwitchOn(PeristalticPump1),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump1_id
        );

        message = create_command_message(
            InternalCommand::SwitchOn(PeristalticPump2),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump2_id
        );

        message = create_command_message(
            InternalCommand::SwitchOn(PeristalticPump3),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump3_id
        );

        message = create_command_message(
            InternalCommand::SwitchOn(PeristalticPump4),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump4_id
        );

        message =
            create_command_message(InternalCommand::SwitchOn(Feeder), &config.controllino_relay)
                .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.feeder_id);

        // switching off all devices
        let message = create_command_message(
            InternalCommand::SwitchOff(Skimmer),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.skimmer_id);

        let mut message = create_command_message(
            InternalCommand::SwitchOff(Ventilation),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.ventilation_id);

        message = create_command_message(
            InternalCommand::SwitchOff(Heater),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.heater_id);

        message = create_command_message(
            InternalCommand::SwitchOff(MainPump1),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.main_pump1_id);

        message = create_command_message(
            InternalCommand::SwitchOff(MainPump2),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.main_pump2_id);

        message = create_command_message(
            InternalCommand::SwitchOff(AuxPump1),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.aux_pump1_id);

        message = create_command_message(
            InternalCommand::SwitchOff(AuxPump2),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.aux_pump2_id);

        message = create_command_message(
            InternalCommand::SwitchOff(PeristalticPump1),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump1_id
        );

        message = create_command_message(
            InternalCommand::SwitchOff(PeristalticPump2),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump2_id
        );

        message = create_command_message(
            InternalCommand::SwitchOff(PeristalticPump3),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump3_id
        );

        message = create_command_message(
            InternalCommand::SwitchOff(PeristalticPump4),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump4_id
        );

        message = create_command_message(
            InternalCommand::SwitchOff(Feeder),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.feeder_id);
    }

    #[test]
    // Tests the message creation for pulse command.
    // Pulse command is only permitted for a subset of the devices (e.g., peristaltic pumps).
    pub fn test_create_message_pulse() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_simulator.toml".to_string());

        let mut error_message = create_command_message(
            InternalCommand::Pulse(Skimmer, 1000),
            &config.controllino_relay,
        );
        assert_eq!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(Skimmer))
        );

        error_message = create_command_message(
            InternalCommand::Pulse(Ventilation, 1000),
            &config.controllino_relay,
        );
        assert_eq!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(Ventilation))
        );

        error_message = create_command_message(
            InternalCommand::Pulse(Heater, 1000),
            &config.controllino_relay,
        );
        assert_eq!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(Heater))
        );

        error_message = create_command_message(
            InternalCommand::Pulse(MainPump1, 1000),
            &config.controllino_relay,
        );
        assert_eq!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(MainPump1))
        );

        error_message = create_command_message(
            InternalCommand::Pulse(MainPump2, 1000),
            &config.controllino_relay,
        );
        assert_eq!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(MainPump2))
        );

        error_message = create_command_message(
            InternalCommand::Pulse(AuxPump1, 1000),
            &config.controllino_relay,
        );
        assert_eq!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(AuxPump1))
        );

        error_message = create_command_message(
            InternalCommand::Pulse(AuxPump2, 1000),
            &config.controllino_relay,
        );
        assert_eq!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(AuxPump2))
        );

        let mut message = create_command_message(
            InternalCommand::Pulse(PeristalticPump1, 1000),
            &config.controllino_relay,
        )
        .unwrap();

        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump1_id
        );

        message = create_command_message(
            InternalCommand::Pulse(PeristalticPump2, 1000),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump2_id
        );

        message = create_command_message(
            InternalCommand::Pulse(PeristalticPump3, 1000),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump3_id
        );

        message = create_command_message(
            InternalCommand::Pulse(PeristalticPump4, 1000),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump4_id
        );

        error_message = create_command_message(
            InternalCommand::Pulse(Feeder, 1000),
            &config.controllino_relay,
        );
        assert_eq!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(Feeder))
        );
    }

    #[cfg(feature = "controllino_hw")]
    #[test]
    // Test case actuates on real hardware.
    // Execution of this test case requires serial port connection to the Controllino device.
    pub fn test_controllino_simulate_feed_profile() {
        let sleep_duration_500_millis = Duration::from_millis(500);
        let sleep_duration_8_secs = Duration::from_secs(8);
        let sleep_duration_6_secs = Duration::from_secs(6);
        let spin_sleeper = SpinSleeper::default();

        let config: ConfigData = read_config_file(get_platform_specific_configuration_file());
        let controllino = Controllino::new(config.relay_manager);

        let config2: ConfigData = read_config_file(get_platform_specific_configuration_file());

        let serial_port: Box<dyn SerialPort>;
        serial_port = match controllino.serial_port_opt.as_ref().unwrap().try_clone() {
            Ok(c) => c,
            Err(_) => {
                panic!("Controllino: Error occurred when trying to clone serial port.");
            }
        };

        let mut actuator = ControllinoActuateHardware;

        let expect_info =
            "Controllino test_switch_on_off_pulse failed cloning serial port interface.";

        // Request Controllino to switch on and off all relays sequentially
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(MainPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(MainPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(AuxPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_8_secs);
        assert_eq!(actuator.actuate(InternalCommand::SwitchOn(Feeder),), Ok(()));
        spin_sleeper.sleep(sleep_duration_6_secs);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(Feeder),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_8_secs);
        assert_eq!(actuator.actuate(InternalCommand::SwitchOn(Feeder),), Ok(()));
        spin_sleeper.sleep(sleep_duration_6_secs);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(Feeder),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_8_secs);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(MainPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(MainPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(AuxPump1),),
            Ok(())
        );
    }
}
