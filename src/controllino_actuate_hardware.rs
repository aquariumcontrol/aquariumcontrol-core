/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

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
        println!("check_controllino_checksums:");
        println!(
            "data[0]={} data[1]={} data[2]={} data[3]={}",
            data[0], data[1], data[2], data[3]
        );
        println!(
            "data[4]={} data[5]={} data[6]={} data[7]={}",
            data[4], data[5], data[6], data[7]
        );
        println!("first checksum should have been {}", first_checksum);
        println!("second checksum should have been {}", second_checksum);
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

pub struct ControllinoActuateHardware;

impl RelayActuationTrait for ControllinoActuateHardware {
    /// Triggers actuation on real hardware via serial port communication
    /// It translates the commands SwitchOn and SwitchOff to SetRelay and UnsetRelay
    /// considering relay id and relay logic.
    /// # Arguments
    /// * `internal_command` - enum value used for identifying content which shall be sent
    /// * `tx_to_tcp_opt` - channel for communication with simulator - not used in this implementation of the trait
    fn actuate(
        &self,
        internal_command: InternalCommand,
        _tx_to_tcp_opt: Option<&Sender<InternalCommand>>,
        serial_port_opt: &mut Option<Box<dyn SerialPort>>,
        spin_sleeper_opt: &Option<SpinSleeper>,
        controllino_processing_duration_opt: &Option<Duration>,
        config: &ControllinoConfig,
    ) -> Result<(), ControllinoError> {
        // if unwrap panics, panic shall happen
        let spin_sleeper = spin_sleeper_opt.unwrap();
        let controllino_processing_duration = controllino_processing_duration_opt.unwrap();
        let mut serial_port = serial_port_opt.as_mut().unwrap();

        match internal_command {
            InternalCommand::SwitchOn(ref _c)
            | InternalCommand::SwitchOff(ref _c)
            | InternalCommand::Pulse(ref _c, _) => {
                // create message to be sent to Controllino
                let message = match create_command_message(internal_command.clone(), config) {
                    Ok(c) => c,
                    Err(e) => {
                        println!(
                            "Controllino: encountered error in preparation of communication: {e:?}"
                        );
                        return Err(ControllinoError::FailureCommandMessageCreation);
                    }
                };

                println!("{}", message);

                // write message to Controllino
                match serial_port.write(&message.data) {
                    Ok(_) => { /* do nothing */ }
                    Err(e) => {
                        println!("Controllino: Error occurred when sending command to Controllino: {e:?}");
                        return Err(ControllinoError::WriteError);
                    }
                }

                // give Controllino some time to process the command
                spin_sleeper.sleep(controllino_processing_duration);

                // for pulse command, also wait the pulse duration
                match internal_command {
                    InternalCommand::Pulse(_, t) => {
                        let pulse_duration = Duration::from_millis(t as u64);
                        spin_sleeper.sleep(pulse_duration);
                    }
                    _ => { /* do nothing */ }
                }

                // read response from Controllino
                let mut serial_buf: Vec<u8> = vec![0; controllino_constants::MESSAGE_SIZE];
                match serial_port.read_exact(serial_buf.as_mut_slice()) {
                    Ok(()) => {
                        //println!("message_length={}", message_length);
                        if !check_controllino_checksums(serial_buf.as_slice()) {
                            println!("Controllino: Received response from device with incorrect checksums");
                            return Err(ControllinoError::IncorrectChecksum);
                        } else {
                            Ok(())
                        }
                    }
                    Err(_) => {
                        println!("Controllino: Error when trying to receive response from device.");
                        return Err(ControllinoError::ReadError);
                    }
                }
            }
            _ => {
                println!(
                    "Controllino: ignoring irrelevant internal command {}.",
                    internal_command.to_string()
                );
                return Err(ControllinoError::IrrelevantCommand(internal_command));
            }
        }
    }
}
