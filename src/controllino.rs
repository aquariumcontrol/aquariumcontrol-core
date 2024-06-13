/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;
use serialport::SerialPort;
use std::fmt;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use crate::channel_content::{AquariumDevice, InternalCommand};

/// Contains constants for communication with Controllino
mod controllino_constants {
    /// the size of ControllinoMessage is delibereately set here because checksums are added to content of struct
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
}

/// Container for message content sent to Controllino
pub struct ControllinoMessageContent {
    /// Set by control application with command sent to Controllino
    pub command: char,

    /// Set by control application with id (e.g. for relay) of an element sent to Controllino
    pub id: u8,

    /// Unused byte
    pub reserved1: u8,

    /// Set by Controllino with data sent back to control application
    pub signal_from_controllino: i32,

    /// Unused byte
    pub reserved2: char,
}

impl ControllinoMessageContent {
    /// provides a struct of type ControllinoMessage
    /// # Arguments
    /// * `command` - character representing command
    /// * `id` - identifier for relay or digital out in case status change is requested
    pub fn new(command: char, id: u8) -> ControllinoMessageContent {
        ControllinoMessageContent {
            command: command,
            id: id,
            reserved1: 0,
            signal_from_controllino: 0,
            reserved2: '0',
        }
    }
}

/// Data structure for message to be sent to Controllino
pub struct ControllinoMessage {
    pub data: [u8; controllino_constants::MESSAGE_SIZE],
}

impl ControllinoMessage {
    pub fn new(content: ControllinoMessageContent) -> ControllinoMessage {
        // transfer content into data structure and calculate checksum
        let mut data: [u8; controllino_constants::MESSAGE_SIZE] = [0; controllino_constants::MESSAGE_SIZE];
        data[0] = content.command as u8;
        data[1] = content.id;
        data[2] = content.reserved1;
        data[3] = data[0] ^ data[1] ^ data[2];
        data[4] = ((content.signal_from_controllino & 0xFF00) >> 8) as u8;
        data[5] = (content.signal_from_controllino & 0x00FF) as u8;
        data[6] = content.reserved2 as u8;
        data[7] = data[4] ^ data[5] ^ data[6];

        ControllinoMessage { data: data }
    }

    pub fn check(&self) -> bool {
        // calculate the checksums and compare with data received
        (self.data[3] == self.data[0] ^ self.data[1] ^ self.data[2])
            && (self.data[7] == self.data[4] ^ self.data[5] ^ self.data[6])
    }
}

/// Enum codifies the errors which the program may encounter in preparation or execution of communication with Controllino
pub enum ControllinoError {
    IrrelevantCommand(InternalCommand),
}

impl fmt::Debug for ControllinoError {
    /// Provides output of the enum values for debugging purposes.
    /// # Arguments
    /// * `f` - configuration for formatting
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ControllinoError::IrrelevantCommand(c) => write!(f, "Irrelevant command {}", c),
        }
    }
}

/// Holds the configuration data for the Controllino interface.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct ControllinoConfig {
    /// Relay id of the protein skimmer
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub skimmer_id: u8,

    /// Relay id of main pump #1
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub main_pump1_id: u8,

    /// Relay id of main pump #2
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub main_pump2_id: u8,

    /// Relay id of auxiliary pump #1
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub aux_pump1_id: u8,

    /// Relay id of auxiliary pump #2
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub aux_pump2_id: u8,

    /// Relay id of refill pump
    pub refill_pump_id: u8,

    /// Relay id of heater
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

    /// Flag to determine if application shall communicate with hardware directly or via TCP with a simulator
    pub use_simulator: bool,

    /// Baud rate for communication via serial port
    pub baud_rate: u32,

    /// Port name for communication via serial port
    pub port_name: String,

    /// timeout in milliseconds for serial port communication
    pub timeout_millis: u64,
}

/// Contains the configuration and the implementation of the Controllino interface
pub struct Controllino {
    /// configuration data for Controllino
    pub config: ControllinoConfig,

    pub serial_port_opt: Option<Box<dyn SerialPort>>,
}

impl Controllino {
    /// Provides a struct of type Controllino
    /// # Arguments
    /// * `config` - configuration data for Controllino
    pub fn new(config: ControllinoConfig) -> Controllino {
        let serial_port_opt: Option<Box<dyn SerialPort>>;

        if config.use_simulator {
            let mut serial_port =
                match serialport::new(config.port_name.clone(), config.baud_rate).open() {
                    Ok(c) => c,
                    Err(e) => {
                        panic!("controllino: failed to open serial port: {e:?}");
                    }
                };

            let timeout_duration = Duration::from_millis(config.timeout_millis);
            match serial_port.set_timeout(timeout_duration) {
                Ok(()) => { /* do nothing */ }
                Err(_) => {
                    panic!(
                        "controllino: could not set timeout of {} milliseconds.",
                        config.timeout_millis
                    );
                }
            }

            serial_port_opt = Some(serial_port);
        } else {
            serial_port_opt = None;
        }

        Controllino {
            config: config,
            serial_port_opt: serial_port_opt,
        }
    }

    /// Provides one of two enum values of type InternalCommand wrapping the relay number.
    /// The relay logic is considered here by choosing either SetRelay or UnsetRelay.
    /// # Arguments
    /// * `internal_command` - enum value which is interpreted
    /// * `device` - enum value for which the function shall provide the relay id
    fn get_relay_command(
        &self,
        internal_command: &InternalCommand,
        device: AquariumDevice,
    ) -> Result<InternalCommand, ControllinoError> {
        match device {
            AquariumDevice::Skimmer => match internal_command {
                InternalCommand::SwitchOn(_) => {
                    Ok(InternalCommand::UnsetRelay(self.config.skimmer_id as u16))
                }
                InternalCommand::SwitchOff(_) => {
                    Ok(InternalCommand::SetRelay(self.config.skimmer_id as u16))
                }
                _ => Err(ControllinoError::IrrelevantCommand(
                    internal_command.clone(),
                )),
            },
            AquariumDevice::MainPump1 => match internal_command {
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::UnsetRelay(
                    self.config.main_pump1_id as u16,
                )),
                InternalCommand::SwitchOff(_) => {
                    Ok(InternalCommand::SetRelay(self.config.main_pump1_id as u16))
                }
                _ => Err(ControllinoError::IrrelevantCommand(
                    internal_command.clone(),
                )),
            },
            AquariumDevice::MainPump2 => match internal_command {
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::UnsetRelay(
                    self.config.main_pump2_id as u16,
                )),
                InternalCommand::SwitchOff(_) => {
                    Ok(InternalCommand::SetRelay(self.config.main_pump2_id as u16))
                }
                _ => Err(ControllinoError::IrrelevantCommand(
                    internal_command.clone(),
                )),
            },
            AquariumDevice::AuxPump1 => match internal_command {
                InternalCommand::SwitchOn(_) => {
                    Ok(InternalCommand::UnsetRelay(self.config.aux_pump1_id as u16))
                }
                InternalCommand::SwitchOff(_) => {
                    Ok(InternalCommand::SetRelay(self.config.aux_pump1_id as u16))
                }
                _ => Err(ControllinoError::IrrelevantCommand(
                    internal_command.clone(),
                )),
            },
            AquariumDevice::AuxPump2 => match internal_command {
                InternalCommand::SwitchOn(_) => {
                    Ok(InternalCommand::UnsetRelay(self.config.aux_pump2_id as u16))
                }
                InternalCommand::SwitchOff(_) => {
                    Ok(InternalCommand::SetRelay(self.config.aux_pump2_id as u16))
                }
                _ => Err(ControllinoError::IrrelevantCommand(
                    internal_command.clone(),
                )),
            },
            AquariumDevice::Heater => match internal_command {
                InternalCommand::SwitchOn(_) => {
                    Ok(InternalCommand::SetRelay(self.config.heater_id as u16))
                }
                InternalCommand::SwitchOff(_) => {
                    Ok(InternalCommand::UnsetRelay(self.config.heater_id as u16))
                }
                _ => Err(ControllinoError::IrrelevantCommand(
                    internal_command.clone(),
                )),
            },
            AquariumDevice::Ventilation => match internal_command {
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::UnsetRelay(
                    self.config.ventilation_id as u16,
                )),
                InternalCommand::SwitchOff(_) => {
                    Ok(InternalCommand::SetRelay(self.config.ventilation_id as u16))
                }
                _ => Err(ControllinoError::IrrelevantCommand(
                    internal_command.clone(),
                )),
            },
            AquariumDevice::RefillPump => match internal_command {
                InternalCommand::SwitchOn(_) => {
                    Ok(InternalCommand::SetRelay(self.config.refill_pump_id as u16))
                }
                InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(
                    self.config.refill_pump_id as u16,
                )),
                _ => Err(ControllinoError::IrrelevantCommand(
                    internal_command.clone(),
                )),
            },
            AquariumDevice::Feeder => match internal_command {
                InternalCommand::SwitchOn(_) => {
                    Ok(InternalCommand::SetRelay(self.config.feeder_id as u16))
                }
                InternalCommand::SwitchOff(_) => {
                    Ok(InternalCommand::UnsetRelay(self.config.feeder_id as u16))
                }
                _ => Err(ControllinoError::IrrelevantCommand(
                    internal_command.clone(),
                )),
            },
            AquariumDevice::PeristalticPump1 => match internal_command {
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(
                    self.config.peristaltic_pump1_id as u16,
                )),
                InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(
                    self.config.peristaltic_pump1_id as u16,
                )),
                _ => Err(ControllinoError::IrrelevantCommand(
                    internal_command.clone(),
                )),
            },
            AquariumDevice::PeristalticPump2 => match internal_command {
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(
                    self.config.peristaltic_pump2_id as u16,
                )),
                InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(
                    self.config.peristaltic_pump2_id as u16,
                )),
                _ => Err(ControllinoError::IrrelevantCommand(
                    internal_command.clone(),
                )),
            },
            AquariumDevice::PeristalticPump3 => match internal_command {
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(
                    self.config.peristaltic_pump3_id as u16,
                )),
                InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(
                    self.config.peristaltic_pump3_id as u16,
                )),
                _ => Err(ControllinoError::IrrelevantCommand(
                    internal_command.clone(),
                )),
            },
            AquariumDevice::PeristalticPump4 => match internal_command {
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(
                    self.config.peristaltic_pump4_id as u16,
                )),
                InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(
                    self.config.peristaltic_pump4_id as u16,
                )),
                _ => Err(ControllinoError::IrrelevantCommand(
                    internal_command.clone(),
                )),
            },
        }
    }

    /// Triggers actuation on simulator via TCP communication
    /// This function is used for development purposes only.
    /// It translates the commands SwitchOn and SwitchOff to SetRelay and UnsetRelay
    /// considering relay id and relay logic.
    /// # Arguments
    /// * `tx_to_tcp` - Reference to Sender part of communication channel with TcpCommunication
    /// * `internal_command` - enum value used for identifying content which shall be sent
    fn actuate_in_simulator(
        &self,
        tx_to_tcp: &Sender<InternalCommand>,
        internal_command: InternalCommand,
    ) {
        match internal_command {
            InternalCommand::SwitchOn(ref c) | InternalCommand::SwitchOff(ref c) => {
                let relay_command = match self.get_relay_command(&internal_command, c.clone()) {
                    Ok(c) => c,
                    Err(e) => {
                        panic!("Controllino: Error in preparation of communication to simulator: {e:?}");
                    }
                };
                match tx_to_tcp.send(relay_command.clone()) {
                    Ok(()) => {}
                    Err(e) => {
                        panic!(
                            "Controllino: error when sending relay command {} to TCP thread: {e:?}",
                            relay_command
                        );
                    }
                }
            }
            _ => {
                println!(
                    "Controllino: ignoring irrelevant internal command {}.",
                    internal_command.to_string()
                );
            }
        }
    }

    /// Creates the message to be sent to Controllino
    /// # Arguments
    /// * `internal_command` - enum value used for identifying if relay shall be switched on or switched off
    fn create_command_message(
        &self,
        internal_command: InternalCommand,
    ) -> Result<ControllinoMessage, ControllinoError> {
        match internal_command {
            InternalCommand::SwitchOn(ref c) | InternalCommand::SwitchOff(ref c) => {
                let relay_command = match self.get_relay_command(&internal_command, c.clone()) {
                    Ok(c) => c,
                    Err(e) => {
                        return Err(e);
                    }
                };
                let (relay_command_char, relay_id) = match relay_command {
                    InternalCommand::SetRelay(relay_id) => {
                        // relay id is set inside this crate, so cast is safe
                        (controllino_constants::COMMAND_SET_RELAY, relay_id as u8)
                    }
                    InternalCommand::UnsetRelay(relay_id) => {
                        // relay id is set inside this crate, so cast is safe
                        (controllino_constants::COMMAND_UNSET_RELAY, relay_id as u8)
                    }
                    _ => {
                        return Err(ControllinoError::IrrelevantCommand(internal_command));
                    }
                };
                let controllino_message_content =
                    ControllinoMessageContent::new(relay_command_char, relay_id);
                let controllino_message = ControllinoMessage::new(controllino_message_content);
                Ok(controllino_message)
            }
            _ => Err(ControllinoError::IrrelevantCommand(internal_command)),
        }
    }

    /// Triggers actuation on real hardware via serial port communication
    /// It translates the commands SwitchOn and SwitchOff to SetRelay and UnsetRelay
    /// considering relay id and relay logic.
    /// # Arguments
    /// * `serial_port` - Reference to SerialPort
    /// * `internal_command` - enum value used for identifying content which shall be sent
    fn actuate_in_real_hardware(
        &self,
        serial_port: &mut Box<dyn SerialPort>,
        internal_command: InternalCommand,
    ) {
        match internal_command {
            InternalCommand::SwitchOn(ref _c) | InternalCommand::SwitchOff(ref _c) => {
                let message = match self.create_command_message(internal_command) {
                    Ok(c) => c,
                    Err(e) => {
                        println!(
                            "Controllino: encountered error in preparation of communication: {e:?}"
                        );
                        return;
                    }
                };
                match serial_port.write(&message.data) {
                    Ok(_) => { /* do nothing */ }
                    Err(e) => {
                        println!("Controllino: Error occurred when sending command to Controllino: {e:?}");
                    }
                }

                let mut serial_buf: Vec<u8> = vec![0; controllino_constants::MESSAGE_SIZE];
                match serial_port.read(serial_buf.as_mut_slice()) {
                    Ok(_) => { /* do nothing */ }
                    Err(_) => {
                        println!("Controllino: Error when trying to receive response from device.");
                    }
                }
            }
            _ => {
                println!(
                    "Controllino: ignoring irrelevant internal command {}.",
                    internal_command.to_string()
                );
            }
        }
    }
    /// Function checks if there are new messages in the channel.
    /// if the message contains the quit command, then the function returns true, otherwise false.
    /// # Arguments
    /// * `rc_from_signal_handler` - recevier part of channel for communication with signal handler.
    fn check_pending_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
    ) -> bool {
        match rx_from_signal_handler.try_recv() {
            Ok(c) => match c {
                InternalCommand::Quit => true,
                _ => {
                    println!("Controllino: ignoring not applicable command from signal handler.");
                    false
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        println!(
                            "Controllino: Error in trying to receive command from signal handler: {e:?}"
                        );
                    }
                }
                false
            }
        }
    }

    /// Executes the main control in a loop until signal handler commands quitting.
    /// This function receives request from other threads and either actuates on real HW
    /// or communicates with simulator
    /// # Arguments
    /// * `tx_to_tcp_opt` - receiver part of channel for communication to signal handler
    /// * `tx_to_refill` -  sender part of channel for communication to refill thread
    /// * `rx_from_refill` - receiver part of channel for communication from refill thread
    /// * `tx_to_heating` - sender part of channel for communication to heating thread
    /// * `rx_from_heating` - receiver part of channel for communication from heating thread
    /// * `tx_to_ventilation` - sender part of channel for communication to ventilation thread
    /// * `rx_from_ventilation` - receiver part of channel for communication from ventilation thread
    /// * `tx_to_balling` - sender part of channel for communication to Balling dosing thread
    /// * `rx_from_balling` - receiver part of channel for communication from Balling dosing thread
    /// * `tx_to_feed` - sender part of channel for communication to feed thread
    /// * `rx_from_feed` - receiver part of channel for communication from feed thread
    /// * `tx_to_signal_handler` - sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - receiver part of channel for communication from signal handler
    pub fn execute(
        &mut self,
        tx_to_tcp_opt: Option<Sender<InternalCommand>>,
        tx_to_refill: Sender<bool>,
        rx_from_refill: Receiver<InternalCommand>,
        tx_to_heating: Sender<bool>,
        rx_from_heating: Receiver<InternalCommand>,
        tx_to_ventilation: Sender<bool>,
        rx_from_ventilation: Receiver<InternalCommand>,
        tx_to_balling: Sender<bool>,
        rx_from_balling: Receiver<InternalCommand>,
        tx_to_feed: Sender<bool>,
        rx_from_feed: Receiver<InternalCommand>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        let tx_to_tcp: Sender<InternalCommand>;
        if self.config.use_simulator && tx_to_tcp_opt.is_none() {
            panic!("Controllino configured to run with simulator, but no channel to TCP thread provided.");
        } else {
            tx_to_tcp = tx_to_tcp_opt.unwrap(); // this unwrap is safe because checked above.
        }

        let mut serial_port: Box<dyn SerialPort>;
        if !self.config.use_simulator && self.serial_port_opt.is_none() {
            panic!("Controllino configured to run with real hardware, but no serial port connection provided.");
        } else {
            serial_port = match self.serial_port_opt.as_ref().unwrap().try_clone() {
                // above unwrap is safe because checked further above.
                Ok(c) => c,
                Err(_) => {
                    panic!("Controllino: Error occured when trying to clone serial port.");
                }
            }
        }

        loop {
            if self.check_pending_external_request(&rx_from_signal_handler) {
                break;
            }

            // *** communication with refill control ***
            match rx_from_refill.try_recv() {
                Ok(c) => {
                    println!("Controllino: received request from Refill thread.");
                    if self.config.use_simulator {
                        self.actuate_in_simulator(&tx_to_tcp, c);
                    } else {
                        self.actuate_in_real_hardware(&mut serial_port, c);
                    }

                    match tx_to_refill.send(true) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in trying to send response value back to refill control: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("Controllino: Error in trying to receive command from refill control {e:?}");
                        }
                    }
                }
            }
            // **********************************

            // *** communication with heating control ***
            match rx_from_heating.try_recv() {
                Ok(c) => {
                    println!("Controllino: received request from Heating thread.");
                    if self.config.use_simulator {
                        self.actuate_in_simulator(&tx_to_tcp, c);
                    } else {
                        self.actuate_in_real_hardware(&mut serial_port, c);
                    }

                    match tx_to_heating.send(true) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in trying to send response value back to heating control: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("Controllino: Error in trying to receive command from heating control {e:?}");
                        }
                    }
                }
            }
            // **********************************

            // *** communication with ventilation control ***
            match rx_from_ventilation.try_recv() {
                Ok(c) => {
                    println!("Controllino: received request from Ventilation thread.");
                    if self.config.use_simulator {
                        self.actuate_in_simulator(&tx_to_tcp, c);
                    } else {
                        self.actuate_in_real_hardware(&mut serial_port, c);
                    }

                    match tx_to_ventilation.send(true) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in trying to send response value back to ventilation control: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("Controllino: Error in trying to receive command from ventilation control {e:?}");
                        }
                    }
                }
            }
            // **********************************

            // *** communication with Balling dosing control ***
            match rx_from_balling.try_recv() {
                Ok(c) => {
                    println!("Controllino: received request from Balling thread.");
                    if self.config.use_simulator {
                        self.actuate_in_simulator(&tx_to_tcp, c);
                    } else {
                        self.actuate_in_real_hardware(&mut serial_port, c);
                    }

                    match tx_to_balling.send(true) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in trying to send response value back to Balling dosing control: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("Controllino: Error in trying to receive command from Balling dosing control {e:?}");
                        }
                    }
                }
            }
            // **********************************

            // *** communication with feed control ***
            match rx_from_feed.try_recv() {
                Ok(c) => {
                    println!("Controllino: received request from Feed thread.");
                    if self.config.use_simulator {
                        self.actuate_in_simulator(&tx_to_tcp, c);
                    } else {
                        self.actuate_in_real_hardware(&mut serial_port, c);
                    }

                    match tx_to_feed.send(true) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in trying to send response value back to feed control: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("Controllino: Error in trying to receive command from feed control {e:?}");
                        }
                    }
                }
            }
            // **********************************
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                println!("Controllino: Error in trying to send confirmation back to signal handler: {e:?}");
            }
        };
    }
}
