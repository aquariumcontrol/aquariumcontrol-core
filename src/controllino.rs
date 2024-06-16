/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;
use serialport::ClearBuffer;
use serialport::SerialPort;
use spin_sleep::SpinSleeper;
use std::fmt;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use crate::channel_content::{AquariumDevice, InternalCommand};

/// Contains constants for communication with Controllino
#[allow(unused)]
pub mod controllino_constants {
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

    /// initial wait period after resetting DTR
    pub const SLEEP_AFTER_DTR_RESET_MILLIS: u64 = 1000;

    /// initial wait periof after clearing buffer
    pub const SLEEP_AFTER_CLEAR_BUFFER_MILLIS: u64 = 2000;
}

/// Trait for the execution of relay actuation on either simulator or real hardware.
/// This trait allows running the main control with a mock implementation for testing.
pub trait RelayActuationTrait {
    fn actuate(
        &self,
        internal_command: InternalCommand,
        tx_to_tcp_opt: Option<&Sender<InternalCommand>>,
        serial_port_opt: &mut Option<Box<dyn SerialPort>>,
        spin_sleeper_opt: &Option<SpinSleeper>,
        controllino_processing_duration_opt: &Option<Duration>,
        config: &ControllinoConfig,
    ) -> Result<(), ControllinoError>;
}

/// Enum codifies the errors which the program may encounter in preparation or execution of communication with Controllino
#[derive(PartialEq)]
pub enum ControllinoError {
    IrrelevantCommand(InternalCommand),
    FailureCommandMessageCreation,
    WriteError,
    ReadError,
    IncorrectChecksum,
    PulseNotAllowedForDevice(AquariumDevice),
}

impl fmt::Debug for ControllinoError {
    /// Provides output of the enum values for debugging purposes.
    /// # Arguments
    /// * `f` - configuration for formatting
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ControllinoError::IrrelevantCommand(c) => write!(f, "Irrelevant command {}", c),
            ControllinoError::FailureCommandMessageCreation => {
                write!(f, "Failure command message creation")
            }
            ControllinoError::WriteError => write!(f, "Write Error"),
            ControllinoError::ReadError => write!(f, "Read Error"),
            ControllinoError::IncorrectChecksum => write!(f, "Incorrect Checksum"),
            ControllinoError::PulseNotAllowedForDevice(c) => {
                write!(f, "Pulse command not allowed for device {}", c)
            }
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

    /// time in milliseconds to wait for response
    pub controllino_processing_millis: u64,
}

/// Contains the configuration and the implementation of the Controllino interface
pub struct Controllino {
    /// configuration data for Controllino
    pub config: ControllinoConfig,

    /// Object is used frequently to wait for a determined period of time, so storing it instead of recreating when needed.
    pub spin_sleeper_opt: Option<SpinSleeper>,

    /// Object is used frequently to wait for a determined period of time, so storing it instead of recreating when needed.
    pub controllino_processing_duration_opt: Option<Duration>,

    /// Interface to serial port for communication via USB
    pub serial_port_opt: Option<Box<dyn SerialPort>>,
}

impl Controllino {
    /// Provides a struct of type Controllino
    /// # Arguments
    /// * `config` - configuration data for Controllino
    pub fn new(config: ControllinoConfig) -> Controllino {
        let spin_sleeper_opt: Option<SpinSleeper>;
        let controllino_processing_duration_opt: Option<Duration>;
        let serial_port_opt: Option<Box<dyn SerialPort>>;

        if config.use_simulator {
            spin_sleeper_opt = None;
            controllino_processing_duration_opt = None;
            serial_port_opt = None;
        } else {
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper_opt = Some(spin_sleeper);

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

            // write to Data Terminal Ready pin: clear the signal
            match serial_port.write_data_terminal_ready(false) {
                Ok(_) => {
                    println!("Controllino: could reset DTR");
                }
                Err(e) => {
                    panic!("Controllino: error when clearing DTR: {e:?}");
                }
            }

            // wait for device
            let sleep_duration_after_dtr_reset =
                Duration::from_millis(controllino_constants::SLEEP_AFTER_DTR_RESET_MILLIS);
            spin_sleeper.sleep(sleep_duration_after_dtr_reset);

            // flush the buffer
            match serial_port.clear(ClearBuffer::All) {
                Ok(_) => {
                    println!("Controllino: could clear buffer.");
                }
                Err(e) => {
                    panic!("Controllino: error when clearing buffer: {e:?}");
                }
            }

            // wait for device
            let sleep_duration_after_buffer_clear =
                Duration::from_millis(controllino_constants::SLEEP_AFTER_CLEAR_BUFFER_MILLIS);
            spin_sleeper.sleep(sleep_duration_after_buffer_clear);

            let controllino_processing_millis = config.controllino_processing_millis;
            let controllino_processing_duration =
                Duration::from_millis(controllino_processing_millis);
            controllino_processing_duration_opt = Some(controllino_processing_duration);

            serial_port_opt = Some(serial_port);
        }

        Controllino {
            config: config,
            spin_sleeper_opt: spin_sleeper_opt,
            controllino_processing_duration_opt: controllino_processing_duration_opt,
            serial_port_opt: serial_port_opt,
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
        actuator: &mut impl RelayActuationTrait,
    ) {
        loop {
            if self.check_pending_external_request(&rx_from_signal_handler) {
                break;
            }

            // *** communication with refill control ***
            match rx_from_refill.try_recv() {
                Ok(c) => {
                    println!("Controllino: received request from Refill thread.");
                    match actuator.actuate(
                        c,
                        tx_to_tcp_opt.as_ref(),
                        &mut self.serial_port_opt,
                        &self.spin_sleeper_opt,
                        &self.controllino_processing_duration_opt,
                        &self.config,
                    ) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in actuation of refill pump: {e:?}"
                            );
                        }                        
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
                    match actuator.actuate(
                        c,
                        tx_to_tcp_opt.as_ref(),
                        &mut self.serial_port_opt,
                        &self.spin_sleeper_opt,
                        &self.controllino_processing_duration_opt,
                        &self.config,
                    ) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in actuation of heater: {e:?}"
                            );
                        }                        
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
                    match actuator.actuate(
                        c,
                        tx_to_tcp_opt.as_ref(),
                        &mut self.serial_port_opt,
                        &self.spin_sleeper_opt,
                        &self.controllino_processing_duration_opt,
                        &self.config,
                    ) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in actuation of ventilation: {e:?}"
                            );
                        }                        
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
                    match actuator.actuate(
                        c,
                        tx_to_tcp_opt.as_ref(),
                        &mut self.serial_port_opt,
                        &self.spin_sleeper_opt,
                        &self.controllino_processing_duration_opt,
                        &self.config,
                    ) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in actuation of Balling dosing: {e:?}"
                            );
                        }                        
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
                    match actuator.actuate(
                        c,
                        tx_to_tcp_opt.as_ref(),
                        &mut self.serial_port_opt,
                        &self.spin_sleeper_opt,
                        &self.controllino_processing_duration_opt,
                        &self.config,
                    ) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in actuation of feeder: {e:?}"
                            );
                        }
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

#[cfg(test)]
pub mod tests {
    use serialport::SerialPort;
    use spin_sleep::SpinSleeper;
    use std::time::Duration;

    use crate::channel_content::AquariumDevice::AuxPump1;
    use crate::channel_content::AquariumDevice::AuxPump2;
    use crate::channel_content::AquariumDevice::Feeder;
    use crate::channel_content::AquariumDevice::Heater;
    use crate::channel_content::AquariumDevice::MainPump1;
    use crate::channel_content::AquariumDevice::MainPump2;
    use crate::channel_content::AquariumDevice::PeristalticPump1;
    use crate::channel_content::AquariumDevice::PeristalticPump2;
    use crate::channel_content::AquariumDevice::PeristalticPump3;
    use crate::channel_content::AquariumDevice::PeristalticPump4;
    use crate::channel_content::AquariumDevice::Skimmer;
    use crate::channel_content::AquariumDevice::Ventilation;

    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::controllino::controllino_constants;
    use crate::controllino::Controllino;
    use crate::controllino::ControllinoError;

    // Request Controllino to pulse all relays for peristaltic pumps
    #[test]
    pub fn test_pulse_all_peristaltic_pumps() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_controllino_macos.toml".to_string());
        let controllino = Controllino::new(config.controllino);

        let mut serial_port: Box<dyn SerialPort>;
        serial_port = match controllino.serial_port_opt.as_ref().unwrap().try_clone() {
            Ok(c) => c,
            Err(_) => {
                panic!("Controllino: Error occured when trying to clone serial port.");
            }
        };

        assert_eq!(
            controllino.actuate_in_real_hardware(
                &mut serial_port,
                InternalCommand::Pulse(PeristalticPump1, 500),
            ),
            Ok(())
        );
        assert_eq!(
            controllino.actuate_in_real_hardware(
                &mut serial_port,
                InternalCommand::Pulse(PeristalticPump2, 500),
            ),
            Ok(())
        );
        assert_eq!(
            controllino.actuate_in_real_hardware(
                &mut serial_port,
                InternalCommand::Pulse(PeristalticPump3, 500),
            ),
            Ok(())
        );
        assert_eq!(
            controllino.actuate_in_real_hardware(
                &mut serial_port,
                InternalCommand::Pulse(PeristalticPump4, 500),
            ),
            Ok(())
        );
    }

    // Request Controllino to switch on and off all relays sequentially
    #[test]
    pub fn test_switch_on_off_all_relays() {
        let sleep_duration_500_millisec = Duration::from_millis(500);
        let spin_sleeper = SpinSleeper::default();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_controllino_macos.toml".to_string());
        let controllino = Controllino::new(config.controllino);

        let mut serial_port: Box<dyn SerialPort>;
        serial_port = match controllino.serial_port_opt.as_ref().unwrap().try_clone() {
            Ok(c) => c,
            Err(_) => {
                panic!("Controllino: Error occured when trying to clone serial port.");
            }
        };

        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOn(Skimmer)),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOn(Ventilation)),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOn(Heater)),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOn(MainPump1)),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOn(MainPump2)),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOn(AuxPump1)),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOn(AuxPump2)),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino.actuate_in_real_hardware(
                &mut serial_port,
                InternalCommand::SwitchOn(PeristalticPump1),
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino.actuate_in_real_hardware(
                &mut serial_port,
                InternalCommand::SwitchOn(PeristalticPump2),
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino.actuate_in_real_hardware(
                &mut serial_port,
                InternalCommand::SwitchOn(PeristalticPump3),
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino.actuate_in_real_hardware(
                &mut serial_port,
                InternalCommand::SwitchOn(PeristalticPump4),
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOn(Feeder)),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOff(Skimmer)),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino.actuate_in_real_hardware(
                &mut serial_port,
                InternalCommand::SwitchOff(Ventilation)
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOff(Heater)),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOff(MainPump1)),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOff(MainPump2)),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOff(AuxPump1)),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOff(AuxPump2)),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino.actuate_in_real_hardware(
                &mut serial_port,
                InternalCommand::SwitchOff(PeristalticPump1),
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino.actuate_in_real_hardware(
                &mut serial_port,
                InternalCommand::SwitchOff(PeristalticPump2),
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino.actuate_in_real_hardware(
                &mut serial_port,
                InternalCommand::SwitchOff(PeristalticPump3),
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino.actuate_in_real_hardware(
                &mut serial_port,
                InternalCommand::SwitchOff(PeristalticPump4),
            ),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millisec);
        assert_eq!(
            controllino
                .actuate_in_real_hardware(&mut serial_port, InternalCommand::SwitchOff(Feeder)),
            Ok(())
        );
    }

    #[test]
    pub fn test_create_message_invalid_command() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_controllino_macos.toml".to_string());
        config.controllino.use_simulator = true;
        let controllino = Controllino::new(config.controllino);
        assert_eq!(
            controllino.create_command_message(InternalCommand::ResetAllErrors),
            Err(ControllinoError::IrrelevantCommand(
                InternalCommand::ResetAllErrors
            )),
        );
    }

    #[test]
    pub fn test_create_message_switch_on_with_set_relay_switch_off_with_unset_relay() {
        let mut config_to_controllino: ConfigData =
            read_config_file("/config/aquarium_control_test_controllino_macos.toml".to_string());
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_controllino_macos.toml".to_string());
        config_to_controllino.controllino.use_simulator = true;
        let controllino = Controllino::new(config_to_controllino.controllino);

        // switching on all devices
        let mut message = controllino
            .create_command_message(InternalCommand::SwitchOn(Skimmer))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.skimmer_id as u8);

        let mut message = controllino
            .create_command_message(InternalCommand::SwitchOn(Ventilation))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.ventilation_id as u8);

        message = controllino
            .create_command_message(InternalCommand::SwitchOn(Heater))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.heater_id as u8);

        message = controllino
            .create_command_message(InternalCommand::SwitchOn(MainPump1))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.main_pump1_id as u8);

        message = controllino
            .create_command_message(InternalCommand::SwitchOn(MainPump2))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.main_pump2_id as u8);

        message = controllino
            .create_command_message(InternalCommand::SwitchOn(AuxPump1))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.aux_pump1_id as u8);

        message = controllino
            .create_command_message(InternalCommand::SwitchOn(AuxPump2))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.aux_pump2_id as u8);

        message = controllino
            .create_command_message(InternalCommand::SwitchOn(PeristalticPump1))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump1_id as u8
        );

        message = controllino
            .create_command_message(InternalCommand::SwitchOn(PeristalticPump2))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump2_id as u8
        );

        message = controllino
            .create_command_message(InternalCommand::SwitchOn(PeristalticPump3))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump3_id as u8
        );

        message = controllino
            .create_command_message(InternalCommand::SwitchOn(PeristalticPump4))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump4_id as u8
        );

        message = controllino
            .create_command_message(InternalCommand::SwitchOn(Feeder))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.feeder_id as u8);

        // switching off all devices
        let mut message = controllino
            .create_command_message(InternalCommand::SwitchOff(Skimmer))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.skimmer_id as u8);

        let mut message = controllino
            .create_command_message(InternalCommand::SwitchOff(Ventilation))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.ventilation_id as u8);

        message = controllino
            .create_command_message(InternalCommand::SwitchOff(Heater))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.heater_id as u8);

        message = controllino
            .create_command_message(InternalCommand::SwitchOff(MainPump1))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.main_pump1_id as u8);

        message = controllino
            .create_command_message(InternalCommand::SwitchOff(MainPump2))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.main_pump2_id as u8);

        message = controllino
            .create_command_message(InternalCommand::SwitchOff(AuxPump1))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.aux_pump1_id as u8);

        message = controllino
            .create_command_message(InternalCommand::SwitchOff(AuxPump2))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.aux_pump2_id as u8);

        message = controllino
            .create_command_message(InternalCommand::SwitchOff(PeristalticPump1))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump1_id as u8
        );

        message = controllino
            .create_command_message(InternalCommand::SwitchOff(PeristalticPump2))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump2_id as u8
        );

        message = controllino
            .create_command_message(InternalCommand::SwitchOff(PeristalticPump3))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump3_id as u8
        );

        message = controllino
            .create_command_message(InternalCommand::SwitchOff(PeristalticPump4))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump4_id as u8
        );

        message = controllino
            .create_command_message(InternalCommand::SwitchOff(Feeder))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino.feeder_id as u8);
        println!(
            "message.data[0]={}\nmessage.data[1]={}\nmessage.data[2]={}\nmessage.data[3]={}",
            message.data[0], message.data[1], message.data[2], message.data[3]
        );
        println!(
            "message.data[4]={}\nmessage.data[5]={}\nmessage.data[6]={}\nmessage.data[7]={}",
            message.data[4], message.data[5], message.data[6], message.data[7]
        );
    }

    #[test]
    pub fn test_create_message_pulse() {
        let mut config_to_controllino: ConfigData =
            read_config_file("/config/aquarium_control_test_controllino_macos.toml".to_string());
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_controllino_macos.toml".to_string());
        config_to_controllino.controllino.use_simulator = true;
        let controllino = Controllino::new(config_to_controllino.controllino);

        // try to pulse all devices
        let mut error_message =
            controllino.create_command_message(InternalCommand::Pulse(Skimmer, 1000));
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(Skimmer))
        );

        error_message =
            controllino.create_command_message(InternalCommand::Pulse(Ventilation, 1000));
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(Ventilation))
        );

        error_message = controllino.create_command_message(InternalCommand::Pulse(Heater, 1000));
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(Heater))
        );

        error_message = controllino.create_command_message(InternalCommand::Pulse(MainPump1, 1000));
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(MainPump1))
        );

        error_message = controllino.create_command_message(InternalCommand::Pulse(MainPump2, 1000));
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(MainPump2))
        );

        error_message = controllino.create_command_message(InternalCommand::Pulse(AuxPump1, 1000));
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(AuxPump1))
        );

        error_message = controllino.create_command_message(InternalCommand::Pulse(AuxPump2, 1000));
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(AuxPump2))
        );

        let mut message = controllino
            .create_command_message(InternalCommand::Pulse(PeristalticPump1, 1000))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump1_id as u8
        );

        message = controllino
            .create_command_message(InternalCommand::Pulse(PeristalticPump2, 1000))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump2_id as u8
        );

        message = controllino
            .create_command_message(InternalCommand::Pulse(PeristalticPump3, 1000))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump3_id as u8
        );

        message = controllino
            .create_command_message(InternalCommand::Pulse(PeristalticPump4, 1000))
            .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino.peristaltic_pump4_id as u8
        );

        error_message = controllino.create_command_message(InternalCommand::Pulse(Feeder, 1000));
        assert_eq!(
            error_message,
            Err(ControllinoError::PulseNotAllowedForDevice(Feeder))
        );
    }
}
