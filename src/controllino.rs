/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;
use serialport::SerialPort;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;
use std::fmt;

use crate::channel_content::{AquariumDevice, InternalCommand};

/// This module contains constants representing commands received from control application.
mod command {
    pub const SETSINGLE: &str = "SETSINGLE";
    pub const UNSETSINGLE: &str = "UNSETSINGLE";
    pub const GETSTATUS: &str = "GETSTATUS";
    pub const PULSE: &str = "PULSE";
    pub const HEARTBEAT: &str = "HEARTBEAT";
    pub const READ: &str = "READ";
    pub const INITIALIZED: &str = "INITIALIZED";
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
    pub skimmer_id: u16,

    /// Relay id of main pump #1
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub main_pump1_id: u16,

    /// Relay id of main pump #2
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub main_pump2_id: u16,

    /// Relay id of auxiliary pump #1
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub aux_pump1_id: u16,

    /// Relay id of auxiliary pump #2
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
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
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::UnsetRelay(self.config.skimmer_id)),
                InternalCommand::SwitchOff(_) => Ok(InternalCommand::SetRelay(self.config.skimmer_id)),
                _ =>  Err(ControllinoError::IrrelevantCommand(internal_command.clone())),
            },
            AquariumDevice::MainPump1 => match internal_command {
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::UnsetRelay(self.config.main_pump1_id)),
                InternalCommand::SwitchOff(_) => Ok(InternalCommand::SetRelay(self.config.main_pump1_id)),
                _ =>  Err(ControllinoError::IrrelevantCommand(internal_command.clone())),
            },
            AquariumDevice::MainPump2 => match internal_command {
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::UnsetRelay(self.config.main_pump2_id)),
                InternalCommand::SwitchOff(_) => Ok(InternalCommand::SetRelay(self.config.main_pump2_id)),
                _ =>  Err(ControllinoError::IrrelevantCommand(internal_command.clone())),
            },
            AquariumDevice::AuxPump1 => match internal_command {
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::UnsetRelay(self.config.aux_pump1_id)),
                InternalCommand::SwitchOff(_) => Ok(InternalCommand::SetRelay(self.config.aux_pump1_id)),
                _ =>  Err(ControllinoError::IrrelevantCommand(internal_command.clone())),
            },
            AquariumDevice::AuxPump2 => match internal_command {
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::UnsetRelay(self.config.aux_pump2_id)),
                InternalCommand::SwitchOff(_) => Ok(InternalCommand::SetRelay(self.config.aux_pump2_id)),
                _ =>  Err(ControllinoError::IrrelevantCommand(internal_command.clone())),
            },
            AquariumDevice::Heater => match internal_command {
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(self.config.heater_id)),
                InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(self.config.heater_id)),
                _ =>  Err(ControllinoError::IrrelevantCommand(internal_command.clone())),
            },
            AquariumDevice::Ventilation => match internal_command {
                InternalCommand::SwitchOn(_) => {
                    Ok(InternalCommand::UnsetRelay(self.config.ventilation_id))
                },
                InternalCommand::SwitchOff(_) => Ok(InternalCommand::SetRelay(self.config.ventilation_id)),
                _ =>  Err(ControllinoError::IrrelevantCommand(internal_command.clone())),
            },
            AquariumDevice::RefillPump => match internal_command {
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(self.config.refill_pump_id)),
                InternalCommand::SwitchOff(_) => {
                    Ok(InternalCommand::UnsetRelay(self.config.refill_pump_id))
                }
                _ =>  Err(ControllinoError::IrrelevantCommand(internal_command.clone())),
            },
            AquariumDevice::Feeder => match internal_command {
                InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(self.config.feeder_id)),
                InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(self.config.feeder_id)),
                _ =>  Err(ControllinoError::IrrelevantCommand(internal_command.clone())),
            },
            AquariumDevice::PeristalticPump1 => match internal_command {
                InternalCommand::SwitchOn(_) => {
                    Ok(InternalCommand::SetRelay(self.config.peristaltic_pump1_id))
                },
                InternalCommand::SwitchOff(_) => {
                    Ok(InternalCommand::UnsetRelay(self.config.peristaltic_pump1_id))
                }
                _ =>  Err(ControllinoError::IrrelevantCommand(internal_command.clone())),
            },
            AquariumDevice::PeristalticPump2 => match internal_command {
                InternalCommand::SwitchOn(_) => {
                    Ok(InternalCommand::SetRelay(self.config.peristaltic_pump2_id))
                },
                InternalCommand::SwitchOff(_) => {
                    Ok(InternalCommand::UnsetRelay(self.config.peristaltic_pump2_id))
                }
                _ =>  Err(ControllinoError::IrrelevantCommand(internal_command.clone())),
            },
            AquariumDevice::PeristalticPump3 => match internal_command {
                InternalCommand::SwitchOn(_) => {
                    Ok(InternalCommand::SetRelay(self.config.peristaltic_pump3_id))
                },
                InternalCommand::SwitchOff(_) => {
                    Ok(InternalCommand::UnsetRelay(self.config.peristaltic_pump3_id))
                }
                _ =>  Err(ControllinoError::IrrelevantCommand(internal_command.clone())),
            },
            AquariumDevice::PeristalticPump4 => match internal_command {
                InternalCommand::SwitchOn(_) => {
                    Ok(InternalCommand::SetRelay(self.config.peristaltic_pump4_id))
                },
                InternalCommand::SwitchOff(_) => {
                    Ok(InternalCommand::UnsetRelay(self.config.peristaltic_pump4_id))
                }
                _ =>  Err(ControllinoError::IrrelevantCommand(internal_command.clone())),
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

    /// Creates the command to be sent to Controllino as String
    /// * `internal_command` - enum value used for identifying if relay shall be switched on or switched off
    fn create_command_string(&self, internal_command: InternalCommand) -> Result<String, ControllinoError> {
        match internal_command {
            InternalCommand::SwitchOn(ref c) | InternalCommand::SwitchOff(ref c) => {
                let relay_command = match self.get_relay_command(&internal_command, c.clone()) {
                    Ok(c) => c,
                    Err(e) => {
                        return Err(e);
                    }
                };
                let (relay_command_str, relay_id_string) = match relay_command {
                    InternalCommand::SetRelay(relay_id) => {
                        let relay_id_string = relay_id.to_string();
                        (command::SETSINGLE, relay_id_string)
                    },
                    InternalCommand::UnsetRelay(relay_id) => {
                        let relay_id_string = relay_id.to_string();
                        (command::UNSETSINGLE, relay_id_string)
                    },
                    _ => {
                        return Err(ControllinoError::IrrelevantCommand(internal_command));
                    }
                };
                Ok("CONTROLLINO ".to_string() + relay_command_str + " " + &relay_id_string)
            },
            _ => {
                Err(ControllinoError::IrrelevantCommand(internal_command))
            }
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
                let command_string = match self.create_command_string(internal_command) {
                    Ok(c) => c,
                    Err(e) => {
                        println!("Controllino: encountered error in preparation of communication: {e:?}");
                        return;
                    }
                };
                let output_to_controllino = command_string.as_bytes();
                match serial_port.write(output_to_controllino) {
                    Ok(_) => { /* do nothing */ }
                    Err(e) => {
                        println!("Controllino: Error occurred when sending command ({}) to Controllino: {e:?}", command_string);
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
