/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::{error, warn};
use serialport::SerialPort;
use spin_sleep::SpinSleeper;
use std::fmt;
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::channel_content::InternalCommand;
use crate::controllino::controllino_constants;
use crate::controllino::ControllinoConfig;
use crate::controllino::ControllinoError;
use crate::controllino::RelayActuationTrait;
use crate::device_relay_logic::get_relay_command;

const LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE: &str = "controllino_actuate_hardware";

/// Container for message content sent to Controllino
pub struct ControllinoMessageContent {
    /// Set by control application with command sent to Controllino
    pub command: char,

    /// Set by control application with id (e.g. for relay) of an element sent to Controllino
    pub id: u8,

    /// Unused byte
    pub reserved1: u8,

    /// Either used to communicate pulse duration in milliseconds to Controllino or
    /// set by Controllino with data to be sent back to control application
    pub payload: u16,

    /// Unused byte
    pub reserved2: char,
}

impl ControllinoMessageContent {
    /// Provides a struct of type ControllinoMessage.
    /// The struct serves as container for the data to be sent to Controllino.
    /// It is used as input to generate a struct of type ControllinoMessage.
    /// # Arguments
    /// * `command` - character representing command
    /// * `id` - identifier for relay or digital out in case status change is requested
    pub fn new(command: char, id: u8, duration_millis: u16) -> ControllinoMessageContent {
        ControllinoMessageContent {
            command: command,
            id: id,
            reserved1: 0,
            payload: duration_millis,
            reserved2: '0',
        }
    }
}

/// Data structure for message to be sent to Controllino
#[derive(Debug, PartialEq)]
pub struct ControllinoMessage {
    /// represents the raw message transferred via serial port
    pub data: [u8; controllino_constants::MESSAGE_SIZE],
}

impl ControllinoMessage {
    /// Provides a struct of type ControllinoMessage.
    /// This function transfers the content into an array of u8 and
    /// calculates the checksums. The array is then sent to Controllino.
    /// # Arguments
    /// * `content` - struct of type ControllinoMessageContent
    pub fn new(content: ControllinoMessageContent) -> ControllinoMessage {
        // transfer content into data structure and calculate checksum
        let mut data: [u8; controllino_constants::MESSAGE_SIZE] =
            [0; controllino_constants::MESSAGE_SIZE];
        data[0] = content.command as u8;
        data[1] = content.id;
        data[2] = content.reserved1;
        data[3] = data[0] ^ data[1] ^ data[2];
        data[4] = ((content.payload & 0xFF00) >> 8) as u8;
        data[5] = (content.payload & 0x00FF) as u8;
        data[6] = content.reserved2 as u8;
        data[7] = data[4] ^ data[5] ^ data[6];

        ControllinoMessage { data: data }
    }
}

impl fmt::Display for ControllinoMessage {
    /// Provides output of the struct data for debugging purposes.
    /// # Arguments
    /// * `f` - configuration for formatting
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ControllinoMessage: [0]={} [1]={} [2]={} [3]={} [4]={} [5]={} [6]={} [7]={}",
            self.data[0],
            self.data[1],
            self.data[2],
            self.data[3],
            self.data[4],
            self.data[5],
            self.data[6],
            self.data[7]
        )
    }
}

/// Calculates the checksums of the byte sequence received from Controllino
/// and compares it with the checksums received
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

/// Creates the message to be sent to Controllino
/// # Arguments
/// * `internal_command` - enum value used for identifying if relay shall be switched on or switched off
fn create_command_message(
    internal_command: InternalCommand,
    config: &ControllinoConfig,
) -> Result<ControllinoMessage, ControllinoError> {
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
                    return Err(ControllinoError::IrrelevantCommand(internal_command));
                }
            };
            let controllino_message_content =
                ControllinoMessageContent::new(relay_command_char, relay_id, duration_millis);
            let controllino_message = ControllinoMessage::new(controllino_message_content);
            Ok(controllino_message)
        }
        _ => Err(ControllinoError::IrrelevantCommand(internal_command)),
    }
}

/// Contains trait implementation for real-life operation.
pub struct ControllinoActuateHardware;

impl RelayActuationTrait for ControllinoActuateHardware {
    /// Triggers actuation on real hardware via serial port communication
    /// It translates the commands SwitchOn and SwitchOff to SetRelay and UnsetRelay
    /// considering relay id and relay logic.
    /// # Arguments
    /// * `internal_command` - enum value used for identifying content which shall be sent
    /// * `_tx_to_tcp_opt` - channel for communication with simulator wrapped in Option - not used in this implementation of the trait
    /// * `serial_port_opt` - serial port interface for communication via USB wrapped in Option
    /// * `spin_sleeper_opt` - spin sleeper object for precise waiting wrapped in Option
    /// * `controllino_processing_duration_opt` - spin sleeper object for precise waiting wrapped in Option
    /// * `config` - Controllino configuration    
    fn actuate(
        &mut self,
        internal_command: InternalCommand,
        _tx_to_tcp_opt: Option<&Sender<InternalCommand>>,
        serial_port_opt: &mut Option<Box<dyn SerialPort>>,
        spin_sleeper_opt: &Option<SpinSleeper>,
        controllino_processing_duration_opt: &Option<Duration>,
        config: &ControllinoConfig,
    ) -> Result<(), ControllinoError> {
        // if unwrap panics, panic shall happen
        let spin_sleeper = spin_sleeper_opt.unwrap();

        match internal_command {
            InternalCommand::SwitchOn(ref _c)
            | InternalCommand::SwitchOff(ref _c)
            | InternalCommand::Pulse(ref _c, _) => {
                // create message to be sent to Controllino
                let message = match create_command_message(internal_command.clone(), config) {
                    Ok(c) => c,
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE,
                            "preparation of communication failed ({e:?})"
                        );
                        return Err(ControllinoError::FailureCommandMessageCreation);
                    }
                };

                if config.active {
                    // write message to Controllino
                    let serial_port = serial_port_opt.as_mut().unwrap();
                    match serial_port.write(&message.data) {
                        Ok(_) => { /* do nothing */ }
                        Err(e) => {
                            error!(
                                target: LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE,
                                "sending command to Controllino failed ({e:?})"
                            );
                            return Err(ControllinoError::WriteError);
                        }
                    }
                }

                // give Controllino some time to process the command
                if config.active {
                    let controllino_processing_duration =
                        controllino_processing_duration_opt.unwrap();
                    spin_sleeper.sleep(controllino_processing_duration);
                }

                // for pulse command, also wait the pulse duration
                match internal_command {
                    InternalCommand::Pulse(_, t) => {
                        let pulse_duration = Duration::from_millis(t as u64);
                        spin_sleeper.sleep(pulse_duration);
                    }
                    _ => { /* do nothing */ }
                }

                if config.active {
                    // read response from Controllino
                    let mut serial_buf: Vec<u8> = vec![0; controllino_constants::MESSAGE_SIZE];
                    let serial_port = serial_port_opt.as_mut().unwrap();
                    match serial_port.read_exact(serial_buf.as_mut_slice()) {
                        Ok(()) => {
                            if !check_controllino_checksums(serial_buf.as_slice()) {
                                warn!(
                                    target: LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE,
                                    "Received response from device with incorrect checksums"
                                );
                                return Err(ControllinoError::IncorrectChecksum);
                            } else {
                                Ok(())
                            }
                        }
                        Err(_) => {
                            error!(
                                target: LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE,
                                "receiving response from device failed."
                            );
                            return Err(ControllinoError::ReadError);
                        }
                    }
                } else {
                    Ok(())
                }
            }
            _ => {
                warn!(
                    target: LOG_TARGET_CONTROLLINO_ACTUATE_HARDWARE,
                    "ignoring irrelevant internal command {}.",
                    internal_command.to_string()
                );
                return Err(ControllinoError::IrrelevantCommand(internal_command));
            }
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
    
            use crate::controllino::{Controllino, RelayActuationTrait};
            use crate::controllino_actuate_hardware::ControllinoActuateHardware;
        }
    }

    use crate::channel_content::AquariumDevice::{
        AuxPump1, AuxPump2, Feeder, Heater, MainPump1, MainPump2, PeristalticPump1,
        PeristalticPump2, PeristalticPump3, PeristalticPump4, Skimmer, Ventilation,
    };

    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::controllino::controllino_constants;
    use crate::controllino::ControllinoError;
    use crate::controllino_actuate_hardware::create_command_message;

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
    // Actuates on real hardware.
    // Execution of this test case requires serial port connection to Controllino device.
    pub fn test_controllino_switch_on_off_pulse() {
        let sleep_duration_500_millisec = Duration::from_millis(500);
        let spin_sleeper = SpinSleeper::default();

        let config: ConfigData = read_config_file(get_platform_specific_configuration_file());
        let controllino = Controllino::new(config.controllino);

        let config2: ConfigData = read_config_file(get_platform_specific_configuration_file());

        let serial_port: Box<dyn SerialPort>;
        serial_port = match controllino.serial_port_opt.as_ref().unwrap().try_clone() {
            Ok(c) => c,
            Err(_) => {
                panic!("Controllino: Error occured when trying to clone serial port.");
            }
        };

        let mut actuator = ControllinoActuateHardware;

        let expect_info =
            "Controllino test_switch_on_off_pulse failed cloning serial port interface.";

        // Request Controllino to switch on and off all relays sequentially
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOn(Skimmer),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOn(Ventilation),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOn(Heater),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOn(MainPump1),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOn(MainPump2),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOn(AuxPump1),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOn(AuxPump2),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOn(PeristalticPump1),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOn(PeristalticPump2),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOn(PeristalticPump3),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOn(PeristalticPump4),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOn(Feeder),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOff(Skimmer),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOff(Ventilation),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOff(Heater),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOff(MainPump1),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOff(MainPump2),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOff(AuxPump1),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOff(AuxPump2),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOff(PeristalticPump1),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOff(PeristalticPump2),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOff(PeristalticPump3),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOff(PeristalticPump4),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::SwitchOff(Feeder),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);

        // Request Controllino to pulse all relays for peristaltic pumps
        assert_eq!(
            actuator.actuate(
                InternalCommand::Pulse(PeristalticPump1, 500),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::Pulse(PeristalticPump2, 500),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::Pulse(PeristalticPump3, 500),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            actuator.actuate(
                InternalCommand::Pulse(PeristalticPump4, 500),
                None,
                &mut Some(serial_port.try_clone().expect(expect_info)),
                &controllino.spin_sleeper_opt,
                &controllino.controllino_processing_duration_opt,
                &config2.controllino
            ),
            Ok(())
        );
    }

    #[test]
    /// Tests if invalid command is rejected when preparing message for Controllino
    pub fn test_create_message_invalid_command() {
        let config: ConfigData = read_config_file(
            "/config/aquarium_control_test_controllino_simulator.toml".to_string(),
        );
        assert_eq!(
            create_command_message(InternalCommand::ResetAllErrors, &config.controllino),
            Err(ControllinoError::IrrelevantCommand(
                InternalCommand::ResetAllErrors
            )),
        );
    }

    #[test]
    // Tests the implementation of the relay logic.
    // During the creation of the message to Controllino the application considers
    // if relay needs to bet set or unset to switch on/off a device.
    pub fn test_controllino_create_message_switch_on_with_set_relay_switch_off_with_unset_relay() {
        let config: ConfigData = read_config_file(
            "/config/aquarium_control_test_controllino_simulator.toml".to_string(),
        );

        // switching on all devices
        let mut message =
            create_command_message(InternalCommand::SwitchOn(Skimmer), &config.controllino)
                .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.skimmer_id as u8);

        message =
            create_command_message(InternalCommand::SwitchOn(Ventilation), &config.controllino)
                .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.ventilation_id as u8);

        message =
            create_command_message(InternalCommand::SwitchOn(Heater), &config.controllino).unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.heater_id as u8);

        message = create_command_message(InternalCommand::SwitchOn(MainPump1), &config.controllino)
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.main_pump1_id as u8);

        message = create_command_message(InternalCommand::SwitchOn(MainPump2), &config.controllino)
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.main_pump2_id as u8);

        message = create_command_message(InternalCommand::SwitchOn(AuxPump1), &config.controllino)
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.aux_pump1_id as u8);

        message = create_command_message(InternalCommand::SwitchOn(AuxPump2), &config.controllino)
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.aux_pump2_id as u8);

        message = create_command_message(
            InternalCommand::SwitchOn(PeristalticPump1),
            &config.controllino,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump1_id as u8
        );

        message = create_command_message(
            InternalCommand::SwitchOn(PeristalticPump2),
            &config.controllino,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump2_id as u8
        );

        message = create_command_message(
            InternalCommand::SwitchOn(PeristalticPump3),
            &config.controllino,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump3_id as u8
        );

        message = create_command_message(
            InternalCommand::SwitchOn(PeristalticPump4),
            &config.controllino,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump4_id as u8
        );

        message =
            create_command_message(InternalCommand::SwitchOn(Feeder), &config.controllino).unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.feeder_id as u8);

        // switching off all devices
        let message =
            create_command_message(InternalCommand::SwitchOff(Skimmer), &config.controllino)
                .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.skimmer_id as u8);

        let mut message =
            create_command_message(InternalCommand::SwitchOff(Ventilation), &config.controllino)
                .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.ventilation_id as u8);

        message = create_command_message(InternalCommand::SwitchOff(Heater), &config.controllino)
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.heater_id as u8);

        message =
            create_command_message(InternalCommand::SwitchOff(MainPump1), &config.controllino)
                .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.main_pump1_id as u8);

        message =
            create_command_message(InternalCommand::SwitchOff(MainPump2), &config.controllino)
                .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.main_pump2_id as u8);

        message = create_command_message(InternalCommand::SwitchOff(AuxPump1), &config.controllino)
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.aux_pump1_id as u8);

        message = create_command_message(InternalCommand::SwitchOff(AuxPump2), &config.controllino)
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.aux_pump2_id as u8);

        message = create_command_message(
            InternalCommand::SwitchOff(PeristalticPump1),
            &config.controllino,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump1_id as u8
        );

        message = create_command_message(
            InternalCommand::SwitchOff(PeristalticPump2),
            &config.controllino,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump2_id as u8
        );

        message = create_command_message(
            InternalCommand::SwitchOff(PeristalticPump3),
            &config.controllino,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump3_id as u8
        );

        message = create_command_message(
            InternalCommand::SwitchOff(PeristalticPump4),
            &config.controllino,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump4_id as u8
        );

        message = create_command_message(InternalCommand::SwitchOff(Feeder), &config.controllino)
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.feeder_id as u8);
    }

    #[test]
    // Tests the message creation for pulse command.
    // Pulse command is only permitted for a subset of the devices (e.g. peristaltic pumps).
    pub fn test_create_message_pulse() {
        let config: ConfigData = read_config_file(
            "/config/aquarium_control_test_controllino_simulator.toml".to_string(),
        );

        let mut error_message =
            create_command_message(InternalCommand::Pulse(Skimmer, 1000), &config.controllino);
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(Skimmer))
        );

        error_message = create_command_message(
            InternalCommand::Pulse(Ventilation, 1000),
            &config.controllino,
        );
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(Ventilation))
        );

        error_message =
            create_command_message(InternalCommand::Pulse(Heater, 1000), &config.controllino);
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(Heater))
        );

        error_message =
            create_command_message(InternalCommand::Pulse(MainPump1, 1000), &config.controllino);
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(MainPump1))
        );

        error_message =
            create_command_message(InternalCommand::Pulse(MainPump2, 1000), &config.controllino);
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(MainPump2))
        );

        error_message =
            create_command_message(InternalCommand::Pulse(AuxPump1, 1000), &config.controllino);
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(AuxPump1))
        );

        error_message =
            create_command_message(InternalCommand::Pulse(AuxPump2, 1000), &config.controllino);
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(AuxPump2))
        );

        let mut message = create_command_message(
            InternalCommand::Pulse(PeristalticPump1, 1000),
            &config.controllino,
        )
        .unwrap();

        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump1_id as u8
        );

        message = create_command_message(
            InternalCommand::Pulse(PeristalticPump2, 1000),
            &config.controllino,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump2_id as u8
        );

        message = create_command_message(
            InternalCommand::Pulse(PeristalticPump3, 1000),
            &config.controllino,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump3_id as u8
        );

        message = create_command_message(
            InternalCommand::Pulse(PeristalticPump4, 1000),
            &config.controllino,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump4_id as u8
        );

        error_message =
            create_command_message(InternalCommand::Pulse(Feeder, 1000), &config.controllino);
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(Feeder))
        );
    }
}
