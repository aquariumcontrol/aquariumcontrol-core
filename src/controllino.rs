/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::error;
use serde_derive::Deserialize;
use serialport::ClearBuffer;
use serialport::SerialPort;
use spin_sleep::SpinSleeper;
use std::fmt;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use crate::channel_content::{AquariumDevice, InternalCommand};
use crate::proc_ext_req::ProcessExternalRequestTrait;

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
        &mut self,
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
#[derive(Clone, Deserialize, Debug)]
pub struct ControllinoConfig {
    /// Switch for testing purposes
    pub active: bool,

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
    baud_rate: u32,

    /// Port name for communication via serial port
    port_name: String,

    /// timeout in milliseconds for serial port communication
    timeout_millis: u64,

    /// time in milliseconds to wait for response
    controllino_processing_millis: u64,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation of the Controllino interface
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     controllino[Controllino] --> tcp_communication[TCP communication]
///     controllino --> refill[Refill control]
///     refill --> controllino
///     controllino --> heating[Heating control]
///     heating --> controllino
///     controllino --> balling[Balling dosing control]
///     balling --> controllino
///     controllino --> feed[Feed control]
///     feed --> controllino
///     controllino --> signal_handler[Signal handler]
///     signal_handler --> controllino
///     controllino --> ventilation[Ventilation control]
///     ventilation --> controllino
/// ```

pub struct Controllino {
    /// configuration data for Controllino
    config: ControllinoConfig,

    /// Object is used frequently to wait for a determined period of time, so storing it instead of recreating when needed.
    /// Attribute is public becaused access from within test cases.
    pub spin_sleeper_opt: Option<SpinSleeper>,

    /// Object is used frequently to wait for a determined period of time, so storing it instead of recreating when needed.
    /// Attribute is public becaused access from within test cases.
    pub controllino_processing_duration_opt: Option<Duration>,

    /// Interface to serial port for communication via USB
    /// Attribute is public becaused access from within test cases.
    pub serial_port_opt: Option<Box<dyn SerialPort>>,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to actuate refill pump
    lock_error_actuate_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to actuate ventilation
    lock_error_actuate_ventilation: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to actuate heating
    lock_error_actuate_heating: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to actuate feed
    lock_error_actuate_feed: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to actuate Balling mineral dosing
    lock_error_actuate_balling: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to send to refill control via channel
    lock_error_channel_send_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to receive from refill control via channel
    lock_error_channel_receive_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to send to feed control via channel
    lock_error_channel_send_feed: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to receive from feed control via channel
    lock_error_channel_receive_feed: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to send to ventilation control via channel
    lock_error_channel_send_ventilation: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to receive from ventilation control via channel
    lock_error_channel_receive_ventilation: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to send to heating control via channel
    lock_error_channel_send_heating: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to receive from heating control via channel
    lock_error_channel_receive_heating: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to send to balling minderal dosing via channel
    lock_error_channel_send_balling: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to receive from balling mineral dosing via channel
    lock_error_channel_receive_balling: bool,
}

impl Controllino {
    const LOG_TARGET_CONTROLLINO: &'static str = "controllino";
    const SW_LOG_SENDER: bool = false;

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

            if config.active {
                let mut serial_port =
                    match serialport::new(config.port_name.clone(), config.baud_rate).open() {
                        Ok(c) => c,
                        Err(e) => {
                            panic!(
                                "{}: failed to open serial port ({e:?})",
                                Self::LOG_TARGET_CONTROLLINO
                            );
                        }
                    };

                let timeout_duration = Duration::from_millis(config.timeout_millis);
                match serial_port.set_timeout(timeout_duration) {
                    Ok(()) => { /* do nothing */ }
                    Err(_) => {
                        panic!(
                            "{}: could not set timeout of {} milliseconds.",
                            Self::LOG_TARGET_CONTROLLINO,
                            config.timeout_millis
                        );
                    }
                }

                // write to Data Terminal Ready pin: clear the signal
                match serial_port.write_data_terminal_ready(false) {
                    Ok(_) => { /* could reset DTR - do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when clearing DTR ({e:?})",
                            Self::LOG_TARGET_CONTROLLINO
                        );
                    }
                }

                // wait for device
                let sleep_duration_after_dtr_reset =
                    Duration::from_millis(controllino_constants::SLEEP_AFTER_DTR_RESET_MILLIS);
                spin_sleeper.sleep(sleep_duration_after_dtr_reset);

                // flush the buffer
                match serial_port.clear(ClearBuffer::All) {
                    Ok(_) => { /* could clear buffer - do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when clearing buffer ({e:?})",
                            Self::LOG_TARGET_CONTROLLINO
                        );
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
            } else {
                controllino_processing_duration_opt = None;
                serial_port_opt = None;
            }
        }

        Controllino {
            config: config,
            spin_sleeper_opt: spin_sleeper_opt,
            controllino_processing_duration_opt: controllino_processing_duration_opt,
            serial_port_opt: serial_port_opt,
            lock_error_actuate_refill: false,
            lock_error_actuate_ventilation: false,
            lock_error_actuate_heating: false,
            lock_error_actuate_feed: false,
            lock_error_actuate_balling: false,
            lock_error_channel_send_refill: false,
            lock_error_channel_receive_refill: false,
            lock_error_channel_send_heating: false,
            lock_error_channel_receive_heating: false,
            lock_error_channel_send_ventilation: false,
            lock_error_channel_receive_ventilation: false,
            lock_error_channel_send_balling: false,
            lock_error_channel_receive_balling: false,
            lock_error_channel_send_feed: false,
            lock_error_channel_receive_feed: false,
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
            let (quit_command_received, _, _) =
                self.process_external_request(&rx_from_signal_handler, None);
            if quit_command_received {
                break;
            }

            // *** communication with refill control ***
            match rx_from_refill.try_recv() {
                Ok(c) => {
                    if Self::SW_LOG_SENDER {
                        println!(
                            "{}: received command from Refill: {}",
                            Self::LOG_TARGET_CONTROLLINO,
                            c
                        );
                    }
                    self.lock_error_channel_receive_refill = false;
                    match actuator.actuate(
                        c,
                        tx_to_tcp_opt.as_ref(),
                        &mut self.serial_port_opt,
                        &self.spin_sleeper_opt,
                        &self.controllino_processing_duration_opt,
                        &self.config,
                    ) {
                        Ok(()) => {
                            self.lock_error_actuate_refill = false;
                        }
                        Err(e) => {
                            if !self.lock_error_actuate_refill {
                                error!(
                                    target: Self::LOG_TARGET_CONTROLLINO,
                                    "actuation of refill pump. ({e:?})"
                                );
                                self.lock_error_actuate_refill = true;
                            }
                        }
                    }
                    match tx_to_refill.send(true) {
                        Ok(()) => {
                            self.lock_error_channel_send_refill = false;
                        }
                        Err(e) => {
                            if !self.lock_error_channel_send_refill {
                                error!(
                                    target: Self::LOG_TARGET_CONTROLLINO,
                                    "responding to refill control ({e:?})"
                                );
                                self.lock_error_channel_send_refill = true;
                            }
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => {
                            /* empty buffer - no action required */
                            self.lock_error_channel_receive_refill = false;
                        }
                        TryRecvError::Disconnected => {
                            if !self.lock_error_channel_receive_refill {
                                error!(
                                    "{}: error when trying to receive command from refill control ({e:?})",
                                    Self::LOG_TARGET_CONTROLLINO
                                );
                                self.lock_error_channel_receive_refill = true;
                            }
                        }
                    }
                }
            }
            // **********************************

            // *** communication with heating control ***
            match rx_from_heating.try_recv() {
                Ok(c) => {
                    if Self::SW_LOG_SENDER {
                        println!(
                            "{}: received command from heating: {}",
                            Self::LOG_TARGET_CONTROLLINO,
                            c
                        );
                    }
                    self.lock_error_channel_receive_heating = false;
                    match actuator.actuate(
                        c,
                        tx_to_tcp_opt.as_ref(),
                        &mut self.serial_port_opt,
                        &self.spin_sleeper_opt,
                        &self.controllino_processing_duration_opt,
                        &self.config,
                    ) {
                        Ok(()) => {
                            self.lock_error_actuate_heating = false;
                        }
                        Err(e) => {
                            if !self.lock_error_actuate_heating {
                                error!(
                                    target: Self::LOG_TARGET_CONTROLLINO,
                                    "actuation of heater ({e:?})"
                                );
                                self.lock_error_actuate_heating = true;
                            }
                        }
                    }
                    match tx_to_heating.send(true) {
                        Ok(()) => {
                            self.lock_error_channel_send_heating = false;
                        }
                        Err(e) => {
                            if !self.lock_error_channel_send_heating {
                                error!(
                                    target: Self::LOG_TARGET_CONTROLLINO,
                                    "responding to heating control: {e:?}"
                                );
                                self.lock_error_channel_send_heating = true;
                            }
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => {
                            /* empty buffer - no action required */
                            self.lock_error_channel_receive_heating = false;
                        }
                        TryRecvError::Disconnected => {
                            if !self.lock_error_channel_receive_heating {
                                error!(
                                    "{}: Error in trying to receive command from heating control {e:?}",
                                    Self::LOG_TARGET_CONTROLLINO
                                );
                                self.lock_error_channel_receive_heating = true;
                            }
                        }
                    }
                }
            }
            // **********************************

            // *** communication with ventilation control ***
            match rx_from_ventilation.try_recv() {
                Ok(c) => {
                    if Self::SW_LOG_SENDER {
                        println!(
                            "{}: received command from ventilation: {}",
                            Self::LOG_TARGET_CONTROLLINO,
                            c
                        );
                    }
                    self.lock_error_channel_receive_ventilation = false;
                    match actuator.actuate(
                        c,
                        tx_to_tcp_opt.as_ref(),
                        &mut self.serial_port_opt,
                        &self.spin_sleeper_opt,
                        &self.controllino_processing_duration_opt,
                        &self.config,
                    ) {
                        Ok(()) => {
                            self.lock_error_actuate_ventilation = false;
                        }
                        Err(e) => {
                            if !self.lock_error_actuate_ventilation {
                                error!(
                                    target: Self::LOG_TARGET_CONTROLLINO,
                                    "actuation of ventilation ({e:?})"
                                );
                                self.lock_error_actuate_ventilation = true;
                            }
                        }
                    }
                    match tx_to_ventilation.send(true) {
                        Ok(()) => {
                            self.lock_error_channel_send_ventilation = false;
                        }
                        Err(e) => {
                            if !self.lock_error_channel_send_ventilation {
                                error!(
                                    target: Self::LOG_TARGET_CONTROLLINO,
                                    "responding to ventilation control ({e:?})"
                                );
                                self.lock_error_channel_send_ventilation = true;
                            }
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => {
                            /* empty buffer - no action required */
                            self.lock_error_channel_receive_ventilation = false;
                        }
                        TryRecvError::Disconnected => {
                            if !self.lock_error_channel_receive_ventilation {
                                error!(
                                    "{}: Error in trying to receive command from ventilation control {e:?}",
                                    Self::LOG_TARGET_CONTROLLINO
                                );
                                self.lock_error_channel_receive_ventilation = true;
                            }
                        }
                    }
                }
            }
            // **********************************

            // *** communication with Balling dosing control ***
            match rx_from_balling.try_recv() {
                Ok(c) => {
                    if Self::SW_LOG_SENDER {
                        println!(
                            "{}: received command from Balling: {}",
                            Self::LOG_TARGET_CONTROLLINO,
                            c
                        );
                    }
                    self.lock_error_channel_receive_balling = false;
                    match actuator.actuate(
                        c,
                        tx_to_tcp_opt.as_ref(),
                        &mut self.serial_port_opt,
                        &self.spin_sleeper_opt,
                        &self.controllino_processing_duration_opt,
                        &self.config,
                    ) {
                        Ok(()) => {
                            self.lock_error_actuate_balling = false;
                        }
                        Err(e) => {
                            if !self.lock_error_actuate_balling {
                                error!(
                                    target: Self::LOG_TARGET_CONTROLLINO,
                                    "actuation of Balling dosing ({e:?})"
                                );
                                self.lock_error_actuate_balling = true;
                            }
                        }
                    }
                    match tx_to_balling.send(true) {
                        Ok(()) => {
                            self.lock_error_channel_send_balling = false;
                        }
                        Err(e) => {
                            if !self.lock_error_channel_send_balling {
                                error!(
                                    target: Self::LOG_TARGET_CONTROLLINO,
                                    "responding to Balling dosing control ({e:?})"
                                );
                                self.lock_error_channel_send_balling = true;
                            }
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => {
                            /* empty buffer - no action required */
                            self.lock_error_channel_receive_balling = false;
                        }
                        TryRecvError::Disconnected => {
                            if !self.lock_error_channel_receive_balling {
                                error!(
                                    "{}: Error in trying to receive command from Balling dosing control ({e:?})", 
                                    Self::LOG_TARGET_CONTROLLINO
                                );
                                self.lock_error_channel_receive_balling = true;
                            }
                        }
                    }
                }
            }
            // **********************************

            // *** communication with feed control ***
            match rx_from_feed.try_recv() {
                Ok(c) => {
                    if Self::SW_LOG_SENDER {
                        println!(
                            "{}: received command from Feed: {}",
                            Self::LOG_TARGET_CONTROLLINO,
                            c
                        );
                    }
                    self.lock_error_channel_receive_feed = false;
                    match actuator.actuate(
                        c,
                        tx_to_tcp_opt.as_ref(),
                        &mut self.serial_port_opt,
                        &self.spin_sleeper_opt,
                        &self.controllino_processing_duration_opt,
                        &self.config,
                    ) {
                        Ok(()) => {
                            self.lock_error_actuate_feed = false;
                        }
                        Err(e) => {
                            if !self.lock_error_actuate_feed {
                                error!(
                                    target: Self::LOG_TARGET_CONTROLLINO,
                                    "actuation of feeder ({e:?})"
                                );
                                self.lock_error_actuate_feed = true;
                            }
                        }
                    }
                    match tx_to_feed.send(true) {
                        Ok(()) => {
                            self.lock_error_channel_send_feed = false;
                        }
                        Err(e) => {
                            if !self.lock_error_channel_send_feed {
                                error!(
                                    target: Self::LOG_TARGET_CONTROLLINO,
                                    "responding to feed control ({e:?})"
                                );
                                self.lock_error_channel_send_feed = true;
                            }
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => {
                            /* empty buffer - no action required */
                            self.lock_error_channel_receive_feed = false;
                        }
                        TryRecvError::Disconnected => {
                            if !self.lock_error_channel_receive_feed {
                                error!(
                                    "{}: Error in trying to receive command from feed control ({e:?})",
                                    Self::LOG_TARGET_CONTROLLINO
                                );
                                self.lock_error_channel_receive_feed = true;
                            }
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
                error!(target: Self::LOG_TARGET_CONTROLLINO,
                    "sending confirmation to signal handler ({e:?})"
                );
            }
        };
    }
}

#[cfg(test)]
pub mod tests {
    use serialport::SerialPort;
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::sync::mpsc::Sender;
    use std::thread::scope;
    use std::time::Duration;

    use crate::channel_content::AquariumDevice;
    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::controllino::Controllino;
    use crate::controllino::ControllinoConfig;
    use crate::controllino::ControllinoError;
    use crate::controllino::RelayActuationTrait;

    // Contains mock trait implementation for unit testing purposes.
    pub struct ControllinoActuateVoid {
        // execution counter for testing purposes
        execution_counter: u32,
    }

    impl RelayActuationTrait for ControllinoActuateVoid {
        // Mock function for testing
        fn actuate(
            &mut self,
            _internal_command: InternalCommand,
            _tx_to_tcp_opt: Option<&Sender<InternalCommand>>,
            _serial_port_opt: &mut Option<Box<dyn SerialPort>>,
            _spin_sleeper_opt: &Option<SpinSleeper>,
            _controllino_processing_duration_opt: &Option<Duration>,
            _config: &ControllinoConfig,
        ) -> Result<(), ControllinoError> {
            self.execution_counter += 1;
            Ok(())
        }
    }

    impl ControllinoActuateVoid {
        // provides number of execution to test case for assertion
        pub fn get_execution_count(&self) -> u32 {
            self.execution_counter
        }
    }

    #[test]
    // test the channel communication of the .execute function of Controllino
    pub fn test_controllino_channel_communication() {
        let config: ConfigData = read_config_file(
            "/config/aquarium_control_test_controllino_simulator.toml".to_string(),
        );

        let mut controllino = Controllino::new(config.controllino);

        let (tx_controllino_to_tcp, _) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_ventilation, rx_ventilation_from_controllino) = mpsc::channel();
        let (tx_ventilation_to_controllino, rx_controllino_from_ventilation) = mpsc::channel();
        let (tx_controllino_to_balling, rx_balling_from_controllino) = mpsc::channel();
        let (tx_balling_to_controllino, rx_controllino_from_balling) = mpsc::channel();
        let (tx_controllino_to_feed, rx_feed_from_controllino) = mpsc::channel();
        let (tx_feed_to_controllino, rx_controllino_from_feed) = mpsc::channel();
        let (tx_controllino_to_signal_handler, rx_signal_handler_from_controllino) =
            mpsc::channel();
        let (tx_signal_handler_to_controllino, rx_controllino_from_signal_handler) =
            mpsc::channel();

        scope(|scope| {
            // thread for test environment
            scope.spawn(move || {
                let _ = tx_refill_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::RefillPump));
                match rx_refill_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for refill thread: {e:?}");
                    }
                }
                let _ = tx_heating_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::Heater));
                match rx_heating_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for heating thread: {e:?}");
                    }
                }
                let _ = tx_ventilation_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::Ventilation));
                match rx_ventilation_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for ventilation thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump1));
                match rx_balling_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump2));
                match rx_balling_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump3));
                match rx_balling_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump4));
                match rx_balling_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::Feeder));
                match rx_feed_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::MainPump1));
                match rx_feed_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::MainPump2));
                match rx_feed_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::AuxPump1));
                match rx_feed_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::AuxPump2));
                match rx_feed_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_refill_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::RefillPump));
                match rx_refill_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for refill thread: {e:?}");
                    }
                }
                let _ = tx_heating_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::Heater));
                match rx_heating_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for heating thread: {e:?}");
                    }
                }
                let _ = tx_ventilation_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::Ventilation));
                match rx_ventilation_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for ventilation thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump1));
                match rx_balling_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump2));
                match rx_balling_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump3));
                match rx_balling_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump4));
                match rx_balling_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::Feeder));
                match rx_feed_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::MainPump1));
                match rx_feed_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::MainPump2));
                match rx_feed_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::AuxPump1));
                match rx_feed_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::AuxPump2));
                match rx_feed_from_controllino.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_controllino_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_signal_handler_to_controllino.send(InternalCommand::Quit);
                rx_signal_handler_from_controllino.recv().unwrap();
            });

            // thread for test object
            scope.spawn(move || {
                let mut mock_actuator = ControllinoActuateVoid {
                    execution_counter: 0,
                };

                controllino.execute(
                    Some(tx_controllino_to_tcp),
                    tx_controllino_to_refill,
                    rx_controllino_from_refill,
                    tx_controllino_to_heating,
                    rx_controllino_from_heating,
                    tx_controllino_to_ventilation,
                    rx_controllino_from_ventilation,
                    tx_controllino_to_balling,
                    rx_controllino_from_balling,
                    tx_controllino_to_feed,
                    rx_controllino_from_feed,
                    tx_controllino_to_signal_handler,
                    rx_controllino_from_signal_handler,
                    &mut mock_actuator,
                );

                // test object thread has ended. Assertion checks number of calls to actuator.
                assert_eq!(mock_actuator.get_execution_count(), 24);
            });
        });
    }
}
