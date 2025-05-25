/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::error;

#[cfg(feature = "debug_relay_manager")]
use log::debug;

use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use serde_derive::Deserialize;
use std::fmt;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

const LOG_TARGET_RELAY_MANAGER: &str = "relay_manager";

/// Trait for the execution of relay actuation on either simulator or real hardware.
/// This trait allows running the main control with a mock implementation for testing.
pub trait RelayActuationTrait {
    fn actuate(&mut self, internal_command: InternalCommand) -> Result<(), RelayError>;
}

/// Enum codifies the errors which the program may encounter in preparation or execution of communication with the relay manager
#[derive(PartialEq)]
pub enum RelayError {
    IrrelevantCommand(InternalCommand),
    FailureCommandMessageCreation,
    WriteError,
    ReadError,
    IncorrectChecksum,
    PulseNotAllowedForDevice(AquariumDevice),
}

impl fmt::Debug for RelayError {
    /// Formats the `RelayError` enum into a human-readable string representation for debugging.
    ///
    /// This implementation allows `RelayError` variants to be displayed directly using
    /// `println!` or `format!`, providing concise and informative error messages.
    /// For variants that contain associated data (like `IrrelevantCommand` or `PulseNotAllowedForDevice`),
    /// that data is included in the output for better diagnostic context.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to the formatter, as required by the `fmt::Display` trait.
    ///
    /// # Returns
    /// A `fmt::Result` indicating whether the formatting operation was successful.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RelayError::IrrelevantCommand(c) => write!(f, "Irrelevant command {}", c),
            RelayError::FailureCommandMessageCreation => {
                write!(f, "Failure command message creation")
            }
            RelayError::WriteError => write!(f, "Write Error"),
            RelayError::ReadError => write!(f, "Read Error"),
            RelayError::IncorrectChecksum => write!(f, "Incorrect Checksum"),
            RelayError::PulseNotAllowedForDevice(c) => {
                write!(f, "Pulse command not allowed for device {}", c)
            }
        }
    }
}
/// Holds the configuration data for the relay manager.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Clone, Deserialize, Debug)]
pub struct RelayManagerConfig {
    /// Switch for testing purposes
    pub active: bool,

    /// Flag to determine if the application shall communicate with hardware directly or via TCP with a simulator
    pub use_simulator: bool,

    /// Flag to determine if the application shall use GPIO or Controllino
    pub use_gpio: bool,
}

/// Container for channels used by relay manager
pub struct RelayManagerChannels {
    /// sender part of the channel for communication to the refill thread
    pub tx_relay_manager_to_refill: Sender<bool>,

    /// receiver part of the channel for communication from the refill thread
    pub rx_relay_manager_from_refill: Receiver<InternalCommand>,

    /// sender part of the channel for communication to the heating thread
    pub tx_relay_manager_to_heating: Sender<bool>,

    /// receiver part of the channel for communication from the heating thread
    pub rx_relay_manager_from_heating: Receiver<InternalCommand>,

    /// sender part of the channel for communication to the ventilation thread
    pub tx_relay_manager_to_ventilation: Sender<bool>,

    /// receiver part of the channel for communication from the ventilation thread
    pub rx_relay_manager_from_ventilation: Receiver<InternalCommand>,

    /// sender part of the channel for communication to the Balling dosing thread
    pub tx_relay_manager_to_balling: Sender<bool>,

    /// receiver part of the channel for communication from the Balling dosing thread
    pub rx_relay_manager_from_balling: Receiver<InternalCommand>,

    /// sender part of the channel for communication to the feed thread
    pub tx_relay_manager_to_feed: Sender<bool>,

    /// receiver part of the channel for communication from the feed thread
    pub rx_relay_manager_from_feed: Receiver<InternalCommand>,

    /// sender part of the channel for communication to the signal handler
    pub tx_relay_manager_to_signal_handler: Sender<bool>,

    /// receiver part of the channel for communication from the signal handler
    pub rx_relay_manager_from_signal_handler: Receiver<InternalCommand>,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation of the relay manager
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     relay_manager[Relay manager] --> tcp_communication[TCP communication]
///     relay_manager --> refill[Refill control]
///     refill --> relay_manager
///     relay_manager --> heating[Heating control]
///     heating --> relay_manager
///     relay_manager --> balling[Balling dosing control]
///     balling --> relay_manager
///     relay_manager --> feed[Feed control]
///     feed --> relay_manager
///     relay_manager --> signal_handler[Signal handler]
///     signal_handler --> relay_manager
///     relay_manager --> ventilation[Ventilation control]
///     ventilation --> relay_manager
/// ```
pub struct RelayManager {
    /// configuration data for relay manager
    config: RelayManagerConfig,

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

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to send to refill control via the channel
    lock_error_channel_send_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to receive from refill control via the channel
    lock_error_channel_receive_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to send to feed control via the channel
    lock_error_channel_send_feed: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to receive from feed control via the channel
    lock_error_channel_receive_feed: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to send to ventilation control via the channel
    lock_error_channel_send_ventilation: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to receive from ventilation control via the channel
    lock_error_channel_receive_ventilation: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to send to heating control via the channel
    lock_error_channel_send_heating: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to receive from heating control via the channel
    lock_error_channel_receive_heating: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to send to balling mineral dosing via the channel
    lock_error_channel_send_balling: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages of failure to receive from balling mineral dosing via the channel
    lock_error_channel_receive_balling: bool,
}

impl RelayManager {
    /// Creates a new `RelayManager` instance.
    ///
    /// This constructor initializes the relay management module with its configuration.
    /// It sets up various internal lock flags to `false`, which are used to prevent
    /// repetitive error and warning messages from flooding the log file during operation,
    /// especially concerning channel communication and actuation errors.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the relay manager, loaded from a TOML file.
    ///
    /// # Returns
    /// A new `RelayManager` struct, ready to handle relay actuation commands
    /// and communicate with other modules.    
    pub fn new(config: RelayManagerConfig) -> RelayManager {
        RelayManager {
            config,
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

    /// Executes the main control loop for the Relay Manager.
    ///
    /// This function runs continuously, acting as the central dispatcher for actuation commands
    /// received from various control modules. It forwards these commands either to real hardware
    /// (e.g., Controllino via serial port) or to a simulator, depending on the `RelayActuationTrait`
    /// implementation it receives.
    ///
    /// The loop continually checks for new commands from different sources (Refill, Heating,
    /// Ventilation, Balling, Feed) and from the global signal handler. It remains active
    /// until a `Quit` command is received from the signal handler, at which point it initiates
    /// a graceful shutdown.
    ///
    /// # Arguments
    /// * `relay_manager_channels` - A struct containing all `mpsc` channels necessary for
    ///   communication with other control threads and the main signal handler.
    /// * `actuator` - A mutable reference to an object implementing the `RelayActuationTrait`,
    ///   which handles the actual hardware interaction or simulation.
    pub fn execute(
        &mut self,
        relay_manager_channels: RelayManagerChannels,
        actuator: &mut impl RelayActuationTrait,
    ) {
        loop {
            let (quit_command_received, _, _) = self.process_external_request(
                &relay_manager_channels.rx_relay_manager_from_signal_handler,
                None,
            );
            if quit_command_received {
                #[cfg(feature = "debug_relay_manager")]
                debug!(
                    target: LOG_TARGET_RELAY_MANAGER,
                    "received QUIT command"
                );

                break;
            }

            // *** communication with refill control ***
            match relay_manager_channels
                .rx_relay_manager_from_refill
                .try_recv()
            {
                Ok(c) => {
                    self.lock_error_channel_receive_refill = false;
                    if self.config.active {
                        match actuator.actuate(c) {
                            Ok(()) => {
                                self.lock_error_actuate_refill = false;
                            }
                            Err(e) => {
                                if !self.lock_error_actuate_refill {
                                    error!(
                                        target: LOG_TARGET_RELAY_MANAGER,
                                        "actuation of refill pump. ({e:?})"
                                    );
                                    self.lock_error_actuate_refill = true;
                                }
                            }
                        }
                    }
                    match &relay_manager_channels.tx_relay_manager_to_refill.send(true) {
                        Ok(()) => {
                            self.lock_error_channel_send_refill = false;
                        }
                        Err(e) => {
                            if !self.lock_error_channel_send_refill {
                                error!(
                                    target: LOG_TARGET_RELAY_MANAGER,
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
                                    LOG_TARGET_RELAY_MANAGER
                                );
                                self.lock_error_channel_receive_refill = true;
                            }
                        }
                    }
                }
            }
            // **********************************

            // *** communication with heating control ***
            match relay_manager_channels
                .rx_relay_manager_from_heating
                .try_recv()
            {
                Ok(c) => {
                    self.lock_error_channel_receive_heating = false;
                    match actuator.actuate(c) {
                        Ok(()) => {
                            self.lock_error_actuate_heating = false;
                        }
                        Err(e) => {
                            if !self.lock_error_actuate_heating {
                                error!(
                                    target: LOG_TARGET_RELAY_MANAGER,
                                    "actuation of heater ({e:?})"
                                );
                                self.lock_error_actuate_heating = true;
                            }
                        }
                    }
                    match &relay_manager_channels
                        .tx_relay_manager_to_heating
                        .send(true)
                    {
                        Ok(()) => {
                            self.lock_error_channel_send_heating = false;
                        }
                        Err(e) => {
                            if !self.lock_error_channel_send_heating {
                                error!(
                                    target: LOG_TARGET_RELAY_MANAGER,
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
                                    LOG_TARGET_RELAY_MANAGER
                                );
                                self.lock_error_channel_receive_heating = true;
                            }
                        }
                    }
                }
            }
            // **********************************

            // *** communication with ventilation control ***
            match relay_manager_channels
                .rx_relay_manager_from_ventilation
                .try_recv()
            {
                Ok(c) => {
                    self.lock_error_channel_receive_ventilation = false;
                    match actuator.actuate(c) {
                        Ok(()) => {
                            self.lock_error_actuate_ventilation = false;
                        }
                        Err(e) => {
                            if !self.lock_error_actuate_ventilation {
                                error!(
                                    target: LOG_TARGET_RELAY_MANAGER,
                                    "actuation of ventilation ({e:?})"
                                );
                                self.lock_error_actuate_ventilation = true;
                            }
                        }
                    }
                    match &relay_manager_channels
                        .tx_relay_manager_to_ventilation
                        .send(true)
                    {
                        Ok(()) => {
                            self.lock_error_channel_send_ventilation = false;
                        }
                        Err(e) => {
                            if !self.lock_error_channel_send_ventilation {
                                error!(
                                    target: LOG_TARGET_RELAY_MANAGER,
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
                                    LOG_TARGET_RELAY_MANAGER
                                );
                                self.lock_error_channel_receive_ventilation = true;
                            }
                        }
                    }
                }
            }
            // **********************************

            // *** communication with Balling dosing control ***
            match relay_manager_channels
                .rx_relay_manager_from_balling
                .try_recv()
            {
                Ok(c) => {
                    self.lock_error_channel_receive_balling = false;
                    match actuator.actuate(c) {
                        Ok(()) => {
                            self.lock_error_actuate_balling = false;
                        }
                        Err(e) => {
                            if !self.lock_error_actuate_balling {
                                error!(
                                    target: LOG_TARGET_RELAY_MANAGER,
                                    "actuation of Balling dosing ({e:?})"
                                );
                                self.lock_error_actuate_balling = true;
                            }
                        }
                    }
                    match &relay_manager_channels
                        .tx_relay_manager_to_balling
                        .send(true)
                    {
                        Ok(()) => {
                            self.lock_error_channel_send_balling = false;
                        }
                        Err(e) => {
                            if !self.lock_error_channel_send_balling {
                                error!(
                                    target: LOG_TARGET_RELAY_MANAGER,
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
                                    LOG_TARGET_RELAY_MANAGER
                                );
                                self.lock_error_channel_receive_balling = true;
                            }
                        }
                    }
                }
            }
            // **********************************

            // *** communication with feed control ***
            match relay_manager_channels.rx_relay_manager_from_feed.try_recv() {
                Ok(c) => {
                    self.lock_error_channel_receive_feed = false;
                    match actuator.actuate(c) {
                        Ok(()) => {
                            self.lock_error_actuate_feed = false;
                        }
                        Err(e) => {
                            if !self.lock_error_actuate_feed {
                                error!(
                                    target: LOG_TARGET_RELAY_MANAGER,
                                    "actuation of feeder ({e:?})"
                                );
                                self.lock_error_actuate_feed = true;
                            }
                        }
                    }
                    match &relay_manager_channels.tx_relay_manager_to_feed.send(true) {
                        Ok(()) => {
                            self.lock_error_channel_send_feed = false;
                        }
                        Err(e) => {
                            if !self.lock_error_channel_send_feed {
                                error!(
                                    target: LOG_TARGET_RELAY_MANAGER,
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
                                    LOG_TARGET_RELAY_MANAGER
                                );
                                self.lock_error_channel_receive_feed = true;
                            }
                        }
                    }
                }
            }
            // **********************************
        }

        #[cfg(feature = "debug_relay_manager")]
        debug!(
            target: LOG_TARGET_RELAY_MANAGER,
            "exited loop, sending back confirmation to signal handler"
        );

        // application received request to terminate, that is why the loop was left.
        // answer to the thread which sent the request for termination, so that shutdown can proceed further.
        match &relay_manager_channels
            .tx_relay_manager_to_signal_handler
            .send(true)
        {
            Ok(()) => {
                #[cfg(feature = "debug_relay_manager")]
                debug!(
                    target: LOG_TARGET_RELAY_MANAGER,
                    "sent back confirmation to signal handler"
                );
            }
            Err(e) => {
                error!(target: LOG_TARGET_RELAY_MANAGER,
                    "sending confirmation to signal handler ({e:?})"
                );
            }
        };
    }
}

#[cfg(test)]
pub mod tests {
    use std::sync::mpsc;
    use std::thread::scope;

    use crate::relays::relay_manager::{
        RelayActuationTrait, RelayError, RelayManager, RelayManagerChannels,
    };
    use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
    use crate::utilities::config::{read_config_file, ConfigData};

    // Contains mock trait implementation for unit testing purposes.
    pub struct ActuateVoid {
        // execution counter for testing purposes
        execution_counter: u32,
    }

    impl RelayActuationTrait for ActuateVoid {
        // Mock function for testing
        fn actuate(&mut self, _internal_command: InternalCommand) -> Result<(), RelayError> {
            self.execution_counter += 1;
            Ok(())
        }
    }

    impl ActuateVoid {
        // provides the number of executions to the test case for assertion
        pub fn get_execution_count(&self) -> u32 {
            self.execution_counter
        }
    }

    #[test]
    // Test case tests the channel communication for the .execute function of relay manager.
    pub fn test_relay_manager_channel_communication() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_simulator.toml".to_string());

        let mut relay_manager = RelayManager::new(config.relay_manager);

        let (tx_relay_manager_to_refill, rx_refill_from_relay_manager) = mpsc::channel();
        let (tx_refill_to_relay_manager, rx_relay_manager_from_refill) = mpsc::channel();
        let (tx_relay_manager_to_heating, rx_heating_from_relay_manager) = mpsc::channel();
        let (tx_heating_to_relay_manager, rx_relay_manager_from_heating) = mpsc::channel();
        let (tx_relay_manager_to_ventilation, rx_ventilation_from_relay_manager) = mpsc::channel();
        let (tx_ventilation_to_relay_manager, rx_relay_manager_from_ventilation) = mpsc::channel();
        let (tx_relay_manager_to_balling, rx_balling_from_relay_manager) = mpsc::channel();
        let (tx_balling_to_relay_manager, rx_relay_manager_from_balling) = mpsc::channel();
        let (tx_relay_manager_to_feed, rx_feed_from_relay_manager) = mpsc::channel();
        let (tx_feed_to_relay_manager, rx_relay_manager_from_feed) = mpsc::channel();
        let (tx_relay_manager_to_signal_handler, rx_signal_handler_from_relay_manager) =
            mpsc::channel();
        let (tx_signal_handler_to_relay_manager, rx_relay_manager_from_signal_handler) =
            mpsc::channel();

        scope(|scope| {
            // thread for test environment
            scope.spawn(move || {
                let _ = tx_refill_to_relay_manager.send(InternalCommand::SwitchOn(AquariumDevice::RefillPump));
                match rx_refill_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for refill thread: {e:?}");
                    }
                }
                let _ = tx_heating_to_relay_manager.send(InternalCommand::SwitchOn(AquariumDevice::Heater));
                match rx_heating_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for heating thread: {e:?}");
                    }
                }
                let _ = tx_ventilation_to_relay_manager.send(InternalCommand::SwitchOn(AquariumDevice::Ventilation));
                match rx_ventilation_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for ventilation thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_relay_manager.send(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump1));
                match rx_balling_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_relay_manager.send(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump2));
                match rx_balling_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_relay_manager.send(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump3));
                match rx_balling_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_relay_manager.send(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump4));
                match rx_balling_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_relay_manager.send(InternalCommand::SwitchOn(AquariumDevice::Feeder));
                match rx_feed_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_relay_manager.send(InternalCommand::SwitchOn(AquariumDevice::MainPump1));
                match rx_feed_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_relay_manager.send(InternalCommand::SwitchOn(AquariumDevice::MainPump2));
                match rx_feed_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_relay_manager.send(InternalCommand::SwitchOn(AquariumDevice::AuxPump1));
                match rx_feed_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_relay_manager.send(InternalCommand::SwitchOn(AquariumDevice::AuxPump2));
                match rx_feed_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_refill_to_relay_manager.send(InternalCommand::SwitchOff(AquariumDevice::RefillPump));
                match rx_refill_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for refill thread: {e:?}");
                    }
                }
                let _ = tx_heating_to_relay_manager.send(InternalCommand::SwitchOff(AquariumDevice::Heater));
                match rx_heating_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for heating thread: {e:?}");
                    }
                }
                let _ = tx_ventilation_to_relay_manager.send(InternalCommand::SwitchOff(AquariumDevice::Ventilation));
                match rx_ventilation_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for ventilation thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_relay_manager.send(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump1));
                match rx_balling_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_relay_manager.send(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump2));
                match rx_balling_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_relay_manager.send(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump3));
                match rx_balling_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_balling_to_relay_manager.send(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump4));
                match rx_balling_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_relay_manager.send(InternalCommand::SwitchOff(AquariumDevice::Feeder));
                match rx_feed_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_relay_manager.send(InternalCommand::SwitchOff(AquariumDevice::MainPump1));
                match rx_feed_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_relay_manager.send(InternalCommand::SwitchOff(AquariumDevice::MainPump2));
                match rx_feed_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_relay_manager.send(InternalCommand::SwitchOff(AquariumDevice::AuxPump1));
                match rx_feed_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_feed_to_relay_manager.send(InternalCommand::SwitchOff(AquariumDevice::AuxPump2));
                match rx_feed_from_relay_manager.recv() {
                    Ok(c) => {
                        assert_eq!(c, true);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
                rx_signal_handler_from_relay_manager.recv().unwrap();
            });

            // thread for the test object
            scope.spawn(move || {
                let mut mock_actuator = ActuateVoid {
                    execution_counter: 0,
                };

                let relay_manager_channels = RelayManagerChannels {
                    tx_relay_manager_to_refill,
                    rx_relay_manager_from_refill,
                    tx_relay_manager_to_heating,
                    rx_relay_manager_from_heating,
                    tx_relay_manager_to_ventilation,
                    rx_relay_manager_from_ventilation,
                    tx_relay_manager_to_balling,
                    rx_relay_manager_from_balling,
                    tx_relay_manager_to_feed,
                    rx_relay_manager_from_feed,
                    tx_relay_manager_to_signal_handler,
                    rx_relay_manager_from_signal_handler,
                };

                relay_manager.execute(relay_manager_channels, &mut mock_actuator);

                // Test object thread has ended. Assertion checks the number of calls to the actuator.
                assert_eq!(mock_actuator.get_execution_count(), 24);
            });
        });
    }
}
