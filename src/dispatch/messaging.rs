/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(feature = "debug_messaging")]
use log::debug;

cfg_if::cfg_if! {
    if #[cfg(target_os = "linux")] {
        #[cfg(not(test))]
        use log::info;

        use log::{error, warn};
        use posixmq::PosixMq;
        use std::sync::mpsc::{Receiver, Sender};
        use std::time::Duration;
        use spin_sleep::SpinSleeper;
    }
}

use serde_derive::Deserialize;
use std::fmt;
use std::io::ErrorKind;

use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::InternalCommand;

const LOG_TARGET_MESSAGING: &str = "messaging";

#[cfg(target_os = "linux")]
/// Contains constants of all domains addressed in IPC messages
pub mod messaging_domains {
    /// value represents the refill control as a domain
    pub const REFILL: i32 = 1;

    /// value represents the ventilation control as a domain
    pub const VENTILATION: i32 = 3;

    /// value represents the feed control as a domain
    pub const FEED: i32 = 8;

    /// value represents the heating control as a domain
    pub const HEATING: i32 = 9;

    /// value represents the Balling dosing control as a domain
    pub const BALLING: i32 = 11;

    /// value represents the monitoring as a domain
    pub const MONITORS: i32 = 12;

    /// value represents the watchdog communication as a domain
    pub const WATCHDOG: i32 = 13;
}

#[cfg(target_os = "linux")]
#[derive(PartialEq)]
/// Identifies the addressee of the message
pub enum MessagingDomain {
    /// refill control
    Refill,

    /// ventilation control
    Ventilation,

    /// feed control
    Feed,

    /// heating control
    Heating,

    /// Balling dosing control
    Balling,

    /// monitors
    Monitors,

    /// watchdog communication
    Watchdog,

    /// in case the numeric value in the message could not be assigned to one of the above
    Unknown,
}

impl fmt::Display for MessagingDomain {
    /// Formats the `MessagingDomain` enum into its human-readable string representation.
    ///
    /// This implementation allows `MessagingDomain` variants to be displayed directly
    /// using macros like `println!` or `format!`, returning the name of the domain.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to the formatter, as required by the `fmt::Display` trait.
    ///
    /// # Returns
    /// A `fmt::Result` indicating whether the formatting operation was successful.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MessagingDomain::Refill => write!(f, "Refill"),
            MessagingDomain::Ventilation => write!(f, "Ventilation"),
            MessagingDomain::Feed => write!(f, "Feed"),
            MessagingDomain::Heating => write!(f, "Heating"),
            MessagingDomain::Balling => write!(f, "Balling"),
            MessagingDomain::Monitors => write!(f, "Monitors"),
            MessagingDomain::Watchdog => write!(f, "Watchdog"),
            MessagingDomain::Unknown => write!(f, "Unknown"),
        }
    }
}

#[cfg(target_os = "linux")]
/// Contains constants of all commands provided in IPC messages
pub mod messaging_commands {

    /// used to reset errors in controllers (refill, monitors)
    pub const RESET: i32 = 1;

    /// used to stop controller (feed, balling, heating, ventilation, monitors)
    pub const STOP: i32 = 3;

    /// used to (re-) start controllers (feed, balling, heating, ventilation, monitors)
    pub const START: i32 = 4;

    /// used for triggering execution of feed profiles
    pub const EXEC: i32 = 10;
}

/// Stores the content of the message and provides functionality for testing.
pub struct Message {
    domain: i32,
    command: i32,
    command_param1: i32,
    command_param2: i32,
}

impl Message {
    /// Creates a new `Message` instance with all its fields initialized to zero.
    ///
    /// This constructor provides a default, empty message state, which is
    /// populated with specific domain, command, and parameter values.
    ///
    /// # Returns
    /// A new `Message` struct with `domain`, `command`, `command_param1`,
    /// and `command_param2` all set to `0`.    
    pub fn new() -> Message {
        Message {
            domain: 0,
            command: 0,
            command_param1: 0,
            command_param2: 0,
        }
    }

    #[cfg(test)]
    /// create a message represented as a raw byte array for testing
    pub fn create_message_buf(&self) -> Vec<u8> {
        let mut buf = vec![0; 20];
        let domain_buf = self.domain.to_le_bytes();
        let command_buf = self.command.to_le_bytes();
        let command_param1_buf = self.command_param1.to_le_bytes();
        let command_param2_buf = self.command_param2.to_le_bytes();

        // transfer partial arrays to array for a complete message
        buf[4] = domain_buf[0];
        buf[5] = domain_buf[1];
        buf[6] = domain_buf[2];
        buf[7] = domain_buf[3];
        buf[8] = command_buf[0];
        buf[9] = command_buf[1];
        buf[10] = command_buf[2];
        buf[11] = command_buf[3];
        buf[12] = command_param1_buf[0];
        buf[13] = command_param1_buf[1];
        buf[14] = command_param1_buf[2];
        buf[15] = command_param1_buf[3];
        buf[16] = command_param2_buf[0];
        buf[17] = command_param2_buf[1];
        buf[18] = command_param2_buf[2];
        buf[19] = command_param2_buf[3];

        buf
    }
}

/// Holds the configuration data for the messaging.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct MessagingConfig {
    /// filename used for generating the message queue identifier
    mq_filename: String,

    /// timeout for receiving messages in milliseconds
    /// after each timeout, the application checks if a shutdown has been requested
    timeout_millis: u64,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Struct provides data structure and implementation for IPC message queue communication.
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     signal_handler[Signal handler] --> messaging[Messaging]
///     messaging --> refill[Refill control]
///     messaging --> ventilation[Ventilation control]
///     messaging --> heating[Heating control]
///     messaging --> feed[Feed control]
///     messaging --> balling[Balling dosing control]
///     messaging --> monitors[Monitors]
///     messaging --> watchdog[Watchdog]
/// ```

pub struct Messaging {
    config: MessagingConfig,

    // Message queue
    #[cfg(target_os = "linux")]
    mq: PosixMq,
    mq_max_msg_len: usize,
}

impl Messaging {
    /// Opens and initializes the POSIX message queue for inter-process communication.
    ///
    /// This constructor attempts to open the message queue specified in the configuration.
    /// It then retrieves the maximum message length supported by this queue.
    /// This function is specifically enabled for Linux operating systems because
    /// the messaging subsystem is only supported by this operating system and not by
    /// other operating systems (e.g., macOS).
    ///
    /// # Arguments
    /// * `config` - Configuration data for the messaging module, primarily containing
    ///   the `mq_filename` (the name of the POSIX message queue).
    ///
    /// # Returns
    /// A new `Messaging` struct, initialized with the configuration, the opened
    /// message queue handle, and its maximum message length.
    ///
    /// # Panics
    /// This function will panic in the following scenarios:
    /// - If it fails to open the message queue with the specified `mq_filename`.
    /// - If it fails to retrieve the attributes (like max message length) of the opened message queue.
    pub fn new(config: MessagingConfig) -> Messaging {
        #[cfg(target_os = "linux")]
        let mq = match PosixMq::open(&config.mq_filename) {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "{}: failure to open message queue using {} ({e:?})",
                    LOG_TARGET_MESSAGING, &config.mq_filename
                );
            }
        };

        // identify maximum length of message queue
        let mq_max_msg_len = match mq.attributes() {
            Ok(c) => c.max_msg_len,
            Err(e) => {
                panic!(
                    "{}: Failed to open attributes ({e:?})",
                    LOG_TARGET_MESSAGING,
                );
            }
        };

        Messaging {
            config,

            #[cfg(target_os = "linux")]
            mq: mq,
            mq_max_msg_len,
        }
    }

    #[cfg(target_os = "linux")]
    /// Continuously monitors the POSIX message queue for incoming commands and forwards them to the relevant modules.
    ///
    /// This function serves as the central message dispatching unit for the application on Linux systems.
    /// It operates in a loop, attempting to receive messages from a configured POSIX message queue (`mq_filename`).
    /// Upon receiving a message, it parses the message's content (domain, command, parameters) and
    /// dispatches it as an `InternalCommand` to the appropriate module's channel.
    ///
    /// It also periodically checks for a `Quit` command from the signal handler to initiate a graceful shutdown
    /// and then waits for a `Terminate` command before exiting.
    ///
    /// # Arguments
    /// * `tx_to_signal_handler` - The sender part of the channel used to acknowledge commands received by the signal handler.
    /// * `rx_from_signal_handler` - The receiver part of the channel used to receive commands (like `Quit` or `Terminate`) from the signal handler.
    /// * `tx_to_refill` - Sender channel for commands to the refill control module.
    /// * `tx_to_ventilation` - Sender channel for commands to the ventilation control module.
    /// * `tx_to_heating` - Sender channel for commands to the heating control module.
    /// * `tx_to_feed` - Sender channel for commands to the feed control module.
    /// * `tx_to_balling` - Sender channel for commands to the Balling dosing control module.
    /// * `tx_to_monitors` - Sender channel for commands to the monitors module.
    /// * `tx_to_watchdog` - Sender channel for commands to the watchdog module.
    ///
    /// # Panics
    /// This function will panic if:
    /// - It encounters a fatal error while receiving from the POSIX message queue (other than a timeout).
    /// - It fails to receive the `Terminate` command from the signal handler during shutdown.
    pub fn execute(
        &mut self,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
        tx_to_refill: Sender<InternalCommand>,
        tx_to_ventilation: Sender<InternalCommand>,
        tx_to_heating: Sender<InternalCommand>,
        tx_to_feed: Sender<InternalCommand>,
        tx_to_balling: Sender<InternalCommand>,
        tx_to_monitors: Sender<InternalCommand>,
        tx_to_watchdog: Sender<InternalCommand>,
    ) {
        let duration_timeout = Duration::from_millis(self.config.timeout_millis);

        let mut msgbuf = vec![0; self.mq_max_msg_len];
        let mut msg = Message::new();

        let sleep_duration = Duration::from_millis(10);
        let spin_sleeper = SpinSleeper::default();

        loop {
            match self.mq.recv_timeout(&mut msgbuf, duration_timeout) {
                Ok((_r1, _r2)) => {
                    if msgbuf.len() > 0 {
                        let mut domain_buf: [u8; 4] = [0; 4];
                        domain_buf[0] = msgbuf[4];
                        domain_buf[1] = msgbuf[5];
                        domain_buf[2] = msgbuf[6];
                        domain_buf[3] = msgbuf[7];
                        msg.domain = i32::from_le_bytes(domain_buf);

                        let mut command_buf: [u8; 4] = [0; 4];
                        command_buf[0] = msgbuf[8];
                        command_buf[1] = msgbuf[9];
                        command_buf[2] = msgbuf[10];
                        command_buf[3] = msgbuf[11];
                        msg.command = i32::from_le_bytes(command_buf);

                        let mut command_param1_buf: [u8; 4] = [0; 4];
                        command_param1_buf[0] = msgbuf[12];
                        command_param1_buf[1] = msgbuf[13];
                        command_param1_buf[2] = msgbuf[14];
                        command_param1_buf[3] = msgbuf[15];
                        msg.command_param1 = i32::from_le_bytes(command_param1_buf);

                        let mut command_param2_buf: [u8; 4] = [0; 4];
                        command_param2_buf[0] = msgbuf[16];
                        command_param2_buf[1] = msgbuf[17];
                        command_param2_buf[2] = msgbuf[18];
                        command_param2_buf[3] = msgbuf[19];
                        msg.command_param2 = i32::from_le_bytes(command_param2_buf);

                        #[cfg(not(test))]
                        info!(
                            target: LOG_TARGET_MESSAGING,
                            "received message: domain={}, cmd={}, param1={}, param2={}",
                            msg.domain,
                            msg.command,
                            msg.command_param1,
                            msg.command_param2
                        );

                        // interpret the domain in a message
                        let domain: MessagingDomain = match msg.domain {
                            messaging_domains::REFILL => MessagingDomain::Refill,
                            messaging_domains::VENTILATION => MessagingDomain::Ventilation,
                            messaging_domains::FEED => MessagingDomain::Feed,
                            messaging_domains::HEATING => MessagingDomain::Heating,
                            messaging_domains::BALLING => MessagingDomain::Balling,
                            messaging_domains::MONITORS => MessagingDomain::Monitors,
                            messaging_domains::WATCHDOG => MessagingDomain::Watchdog,
                            _ => {
                                warn!(
                                    target: LOG_TARGET_MESSAGING,
                                    "ignoring message for unknown domain {}",
                                    msg.domain
                                );
                                MessagingDomain::Unknown
                            }
                        };

                        // interpret command in a message
                        let command: InternalCommand = match msg.command {
                            messaging_commands::RESET => InternalCommand::ResetAllErrors,
                            messaging_commands::STOP => InternalCommand::Stop,
                            messaging_commands::START => InternalCommand::Start,
                            messaging_commands::EXEC => {
                                InternalCommand::Execute(msg.command_param1)
                            }
                            _ => {
                                warn!(
                                    target: LOG_TARGET_MESSAGING,
                                    "Messaging: ignoring message with unknown command {}",
                                    msg.command
                                );
                                InternalCommand::Unknown
                            }
                        };

                        // prepare and execute communication to domain
                        if (domain != MessagingDomain::Unknown)
                            && (command != InternalCommand::Unknown)
                        {
                            let tx_to_domain = match domain {
                                MessagingDomain::Refill => &tx_to_refill,
                                MessagingDomain::Ventilation => &tx_to_ventilation,
                                MessagingDomain::Feed => &tx_to_feed,
                                MessagingDomain::Heating => &tx_to_heating,
                                MessagingDomain::Balling => &tx_to_balling,
                                MessagingDomain::Monitors => &tx_to_monitors,
                                MessagingDomain::Watchdog => &tx_to_watchdog,
                                MessagingDomain::Unknown => unreachable!(),
                            };

                            #[cfg(not(test))]
                            info!(
                                target: LOG_TARGET_MESSAGING,
                                "forwarding message to domain {}",
                                domain
                            );

                            match tx_to_domain.send(command) {
                                Ok(_) => { /* do nothing */ }
                                Err(e) => {
                                    error!(
                                        target: LOG_TARGET_MESSAGING,
                                        "channel communication to domain {} failed ({e:?})",
                                        domain
                                    );
                                }
                            }
                        }
                    } else {
                        error!(
                            target: LOG_TARGET_MESSAGING,
                            "message has incorrect length ({}). ignoring it.",
                            msgbuf.len()
                        );
                    }
                }
                Err(e) => {
                    match e.kind() {
                        ErrorKind::TimedOut => { /* do nothing */ }
                        _ => {
                            error!(
                                target: LOG_TARGET_MESSAGING,
                                "timeout occurred when reading POSIX message ({e:?})"
                            );
                        }
                    }
                }
            }
            let (quit_command_received, _, _) =
                self.process_external_request(&rx_from_signal_handler, None);
            if quit_command_received {
                #[cfg(feature = "debug_messaging")]
                debug!(
                    target: LOG_TARGET_MESSAGING,
                    "received QUIT command from signal handler"
                );

                match tx_to_signal_handler.send(true) {
                    Ok(_) => { /* do nothing */ }
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_MESSAGING,
                            "Error occurred when responding to Quit command from signal handler ({e:?})"
                        );
                    }
                }
                break;
            }

            spin_sleeper.sleep(sleep_duration);
        }

        // wait for Terminate command
        let mut lock_error_invalid_command = false;

        loop {
            match rx_from_signal_handler.recv() {
                Ok(command) => match command {
                    InternalCommand::Terminate => {
                        break;
                    }
                    _ => {
                        if !lock_error_invalid_command {
                            error!(
                                target: LOG_TARGET_MESSAGING,
                                "Received invalid command {} when waiting for Terminate command",
                                command
                            );
                            lock_error_invalid_command = true;
                        }
                    }
                },
                Err(e) => {
                    panic!(
                        "{}: Receiving Terminate command from signal handler failed ({e:?}).",
                        LOG_TARGET_MESSAGING,
                    );
                }
            }
            spin_sleeper.sleep(sleep_duration);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use posixmq::PosixMq;
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender};
    use std::{sync::mpsc, thread, time::Duration};

    use crate::dispatch::messaging::{
        messaging_commands, messaging_domains, Message, Messaging, LOG_TARGET_MESSAGING,
    };
    use crate::stubs::mock_message_command_receiver::tests::mock_message_command_receiver;
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::config::{read_config_file, ConfigData};

    #[cfg(target_os = "linux")]
    // Opens a POSIX message queue for test purposes.
    //
    // This helper function is exclusively used in test environments on Linux.
    // It attempts to open a POSIX message queue specified by its `filename`.
    // If the message queue cannot be opened, the function will panic,
    // indicating a critical issue for the test setup.
    //
    // # Arguments
    // * `filename` - The name of the POSIX message queue to open for testing.
    //
    // # Returns
    // An opened `PosixMq` instance.
    //
    // # Panics
    // This function will panic if it fails to open the specified POSIX message queue.
    fn open_posixmq_test_file(filename: &str) -> PosixMq {
        match PosixMq::open(filename) {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "{}: failure to open message queue for test using {}, error={e:?}",
                    LOG_TARGET_MESSAGING, filename
                );
            }
        }
    }

    #[cfg(target_os = "linux")]
    // Creates a new thread to run the `Messaging` module's `execute` loop.
    //
    // This helper function is used in test environments on Linux to simulate the `Messaging`
    // module running in its own dedicated thread. It sets up the necessary channel
    // connections for communication with various components (like `Refill`, `Heating`, `Watchdog`, etc.).
    //
    // # Arguments
    // * `messaging` - The `Messaging` instance to be executed in the new thread. It is moved into the thread.
    // * `tx_messaging_to_signal_handler` - The sender channel for `Messaging` to send acknowledgments to the signal handler.
    // * `rx_messaging_from_signal_handler` - The receiver channel for `Messaging` to receive commands from the signal handler.
    // * `tx_messaging_to_refill` - The sender channel for `Messaging` to send commands to the refill control.
    // * `tx_messaging_to_ventilation` - The sender channel for `Messaging` to send commands to the ventilation control.
    // * `tx_messaging_to_heating` - The sender channel for `Messaging` to send commands to the heating control.
    // * `tx_messaging_to_feed` - The sender channel for `Messaging` to send commands to the feed control.
    // * `tx_messaging_to_balling` - The sender channel for `Messaging` to send commands to the Balling dosing control.
    // * `tx_messaging_to_monitors` - The sender channel for `Messaging` to send commands to the monitors module.
    // * `tx_messaging_to_watchdog` - The sender channel for `Messaging` to send commands to the watchdog.
    //
    // # Returns
    // A `thread::JoinHandle<()>` which can be used to wait for the spawned thread to complete.
    //
    // # Panics
    // This function will panic if the new thread cannot be spawned.
    fn create_test_object(
        mut messaging: Messaging,
        tx_messaging_to_signal_handler: Sender<bool>,
        rx_messaging_from_signal_handler: Receiver<InternalCommand>,
        tx_messaging_to_refill: Sender<InternalCommand>,
        tx_messaging_to_ventilation: Sender<InternalCommand>,
        tx_messaging_to_heating: Sender<InternalCommand>,
        tx_messaging_to_feed: Sender<InternalCommand>,
        tx_messaging_to_balling: Sender<InternalCommand>,
        tx_messaging_to_monitors: Sender<InternalCommand>,
        tx_messaging_to_watchdog: Sender<InternalCommand>,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                messaging.execute(
                    tx_messaging_to_signal_handler,
                    rx_messaging_from_signal_handler,
                    tx_messaging_to_refill,
                    tx_messaging_to_ventilation,
                    tx_messaging_to_heating,
                    tx_messaging_to_feed,
                    tx_messaging_to_balling,
                    tx_messaging_to_monitors,
                    tx_messaging_to_watchdog,
                );
            })
            .unwrap()
    }

    #[cfg(target_os = "linux")]
    // Creates a new thread to simulate external stimuli and commands for the `Messaging` module.
    //
    // This helper function is used in test environments on Linux. It simulates an
    // external process sending commands to the `Messaging` module via a POSIX message queue
    // and also sends shutdown commands to the `Messaging` module and a mock control object
    // through internal channels.
    //
    // # Arguments
    // * `msg_commands` - A vector of `Message` structs to be sent as stimuli to the `Messaging` module.
    // * `tx_environment_to_object` - The sender channel for this test environment to send commands to a mock control object.
    // * `tx_signal_handler_to_messaging` - The sender channel for this test environment to send commands to the `Messaging` module, simulating the signal handler.
    // * `rx_signal_handler_from_messaging` - The receiver channel for this test environment to receive acknowledgments from the `Messaging` module.
    // * `mq_filename` - The name of the POSIX message queue used for sending stimuli.
    //
    // # Returns
    // A `thread::JoinHandle<()>` which can be used to wait for the spawned thread to complete.
    //
    // # Panics
    // This function will panic if:
    // - The new thread cannot be spawned.
    // - It fails to open the specified POSIX message queue for sending stimuli.
    fn create_test_environment(
        msg_commands: Vec<Message>,
        tx_environment_to_object: Sender<InternalCommand>,
        tx_signal_handler_to_messaging: Sender<InternalCommand>,
        rx_signal_handler_from_messaging: Receiver<bool>,
        mq_filename: String,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_100_millis = Duration::from_millis(100);
                let spin_sleeper = SpinSleeper::default();

                spin_sleeper.sleep(sleep_duration_100_millis);

                let mq_stimuli = open_posixmq_test_file(&mq_filename);

                for msg_command in msg_commands {
                    // sending an IPC message requesting the command to the (sub-)test object
                    let _ = mq_stimuli.send(0, &msg_command.create_message_buf());

                    spin_sleeper.sleep(sleep_duration_100_millis);
                }

                // requesting mock control to quit
                let _ = tx_environment_to_object.send(InternalCommand::Quit);

                // requesting messaging to quit
                let _ = tx_signal_handler_to_messaging.send(InternalCommand::Quit);

                // waiting for confirmation from messaging
                let _ = rx_signal_handler_from_messaging.recv();

                let _ = tx_signal_handler_to_messaging.send(InternalCommand::Terminate);
            })
            .unwrap()
    }

    #[cfg(target_os = "linux")]
    // Asserts that a specific sequence of "Stop" then "Start" commands was received.
    //
    // This helper function is used in test environments on Linux to validate the order
    // and content of commands received by a test mock. It specifically checks that:
    // 1. Exactly two commands were received.
    // 2. The first command received was `InternalCommand::Stop`.
    // 3. The second command received was `InternalCommand::Start`.
    //
    // # Arguments
    // * `commands_received` - A `Vec<InternalCommand>` containing the commands that were
    //   captured by a mock object in the order they were received. This vector is consumed.
    //
    // # Panics
    // This function will panic if:
    // - The `commands_received` vector does not contain exactly two elements.
    // - The commands are not `InternalCommand::Stop` followed by `InternalCommand::Start` in that order.
    fn assert_stop_start_sequence(mut commands_received: Vec<InternalCommand>) {
        // assert the number of commands sent/received are identical
        assert_eq!(2, commands_received.len());

        // assert content of last message sent from test environment:
        assert_eq!(InternalCommand::Start, commands_received.pop().unwrap());

        // assert the content of the first message sent from the test environment:
        assert_eq!(InternalCommand::Stop, commands_received.pop().unwrap());
    }

    #[cfg(target_os = "linux")]
    // Test case checks if messages for ventilation control are triggering the corresponding channel communication
    #[test]
    pub fn test_messaging_ventilation() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.messaging.mq_filename = "/aquarium_control.5UXp".to_string();
        let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

        let mut msg_start = Message::new();
        msg_start.domain = messaging_domains::VENTILATION;
        msg_start.command = messaging_commands::START;

        let mut msg_stop = Message::new();
        msg_stop.domain = messaging_domains::VENTILATION;
        msg_stop.command = messaging_commands::STOP;

        let messaging = Messaging::new(config.messaging);

        let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler) = mpsc::channel();
        let (tx_messaging_to_refill, _rx_refill_from_messaging) = mpsc::channel();
        let (tx_messaging_to_ventilation, rx_ventilation_from_messaging) = mpsc::channel();
        let (tx_messaging_to_heating, _rx_heating_from_messaging) = mpsc::channel();
        let (tx_messaging_to_feed, _rx_feed_from_messaging) = mpsc::channel();
        let (tx_messaging_to_balling, _rx_balling_from_messaging) = mpsc::channel();
        let (tx_messaging_to_monitors, _rx_monitors_from_messaging) = mpsc::channel();
        let (tx_messaging_to_watchdog, _rx_monitors_from_watchdog) = mpsc::channel();
        let (tx_messaging_to_signal_handler, rx_signal_handler_from_messaging) = mpsc::channel();

        let tx_test_environment_to_ventilation = tx_messaging_to_ventilation.clone();

        let join_handle_mock_ventilation = thread::Builder::new()
            .name("mock_ventilation".to_string())
            .spawn(move || {
                assert_stop_start_sequence(mock_message_command_receiver(
                    rx_ventilation_from_messaging,
                ));
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            vec![msg_stop, msg_start], // first stop, then start ventilation
            tx_test_environment_to_ventilation,
            tx_signal_handler_to_messaging,
            rx_signal_handler_from_messaging,
            mq_filename_for_test_environment,
        );

        // thread for the test object
        let join_handle_test_object = create_test_object(
            messaging,
            tx_messaging_to_signal_handler,
            rx_messaging_from_signal_handler,
            tx_messaging_to_refill,
            tx_messaging_to_ventilation,
            tx_messaging_to_heating,
            tx_messaging_to_feed,
            tx_messaging_to_balling,
            tx_messaging_to_monitors,
            tx_messaging_to_watchdog,
        );

        join_handle_mock_ventilation
            .join()
            .expect("Mock ventilation thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[cfg(target_os = "linux")]
    // Test case checks if messages for heating control are triggering the corresponding channel communication
    #[test]
    pub fn test_messaging_heating() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.messaging.mq_filename = "/aquarium_control.6UXp".to_string();
        let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

        let mut msg_start = Message::new();
        msg_start.domain = messaging_domains::HEATING;
        msg_start.command = messaging_commands::START;

        let mut msg_stop = Message::new();
        msg_stop.domain = messaging_domains::HEATING;
        msg_stop.command = messaging_commands::STOP;

        let messaging = Messaging::new(config.messaging);

        let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler) = mpsc::channel();
        let (tx_messaging_to_refill, _rx_refill_from_messaging) = mpsc::channel();
        let (tx_messaging_to_ventilation, _rx_ventilation_from_messaging) = mpsc::channel();
        let (tx_messaging_to_heating, rx_heating_from_messaging) = mpsc::channel();
        let (tx_messaging_to_feed, _rx_feed_from_messaging) = mpsc::channel();
        let (tx_messaging_to_balling, _rx_balling_from_messaging) = mpsc::channel();
        let (tx_messaging_to_monitors, _rx_monitors_from_messaging) = mpsc::channel();
        let (tx_messaging_to_watchdog, _rx_monitors_from_watchdog) = mpsc::channel();
        let (tx_messaging_to_signal_handler, rx_signal_handler_from_messaging) = mpsc::channel();

        let tx_test_environment_to_heating = tx_messaging_to_heating.clone();

        let join_handle_mock_heating = thread::Builder::new()
            .name("mock_heating".to_string())
            .spawn(move || {
                assert_stop_start_sequence(mock_message_command_receiver(
                    rx_heating_from_messaging,
                ));
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            vec![msg_stop, msg_start], // first stop, then start heating
            tx_test_environment_to_heating,
            tx_signal_handler_to_messaging,
            rx_signal_handler_from_messaging,
            mq_filename_for_test_environment,
        );

        // thread for the test object
        let join_handle_test_object = create_test_object(
            messaging,
            tx_messaging_to_signal_handler,
            rx_messaging_from_signal_handler,
            tx_messaging_to_refill,
            tx_messaging_to_ventilation,
            tx_messaging_to_heating,
            tx_messaging_to_feed,
            tx_messaging_to_balling,
            tx_messaging_to_monitors,
            tx_messaging_to_watchdog,
        );

        join_handle_mock_heating
            .join()
            .expect("Mock heating thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[cfg(target_os = "linux")]
    // Test case checks if messages for refill control are triggering the corresponding channel communication
    #[test]
    pub fn test_messaging_refill() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.messaging.mq_filename = "/aquarium_control.7UXp".to_string();
        let mq_file_name_for_test_environment = config.messaging.mq_filename.clone();

        let mut msg_reset = Message::new();
        msg_reset.domain = messaging_domains::REFILL;
        msg_reset.command = messaging_commands::RESET;

        let mut msg_start = Message::new();
        msg_start.domain = messaging_domains::REFILL;
        msg_start.command = messaging_commands::START;

        let mut msg_stop = Message::new();
        msg_stop.domain = messaging_domains::REFILL;
        msg_stop.command = messaging_commands::STOP;

        let messaging = Messaging::new(config.messaging);

        let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler) = mpsc::channel();
        let (tx_messaging_to_refill, rx_refill_from_messaging) = mpsc::channel();
        let (tx_messaging_to_ventilation, _rx_ventilation_from_messaging) = mpsc::channel();
        let (tx_messaging_to_heating, _rx_heating_from_messaging) = mpsc::channel();
        let (tx_messaging_to_feed, _rx_feed_from_messaging) = mpsc::channel();
        let (tx_messaging_to_balling, _rx_balling_from_messaging) = mpsc::channel();
        let (tx_messaging_to_monitors, _rx_monitors_from_messaging) = mpsc::channel();
        let (tx_messaging_to_watchdog, _rx_monitors_from_watchdog) = mpsc::channel();
        let (tx_messaging_to_signal_handler, rx_signal_handler_from_messaging) = mpsc::channel();

        let tx_test_environment_to_refill = tx_messaging_to_refill.clone();

        let join_handle_mock_refill = thread::Builder::new()
            .name("mock_refill".to_string())
            .spawn(move || {
                let mut commands_received = mock_message_command_receiver(rx_refill_from_messaging);

                // assert the number of commands sent/received are identical
                assert_eq!(3, commands_received.len());

                // assert the content of last message sent from test environment:
                assert_eq!(InternalCommand::Start, commands_received.pop().unwrap());

                // assert the content of the second message sent from the test environment:
                assert_eq!(InternalCommand::Stop, commands_received.pop().unwrap());

                // assert the content of the first message sent from the test environment:
                assert_eq!(
                    InternalCommand::ResetAllErrors,
                    commands_received.pop().unwrap()
                );
            })
            .unwrap();

        // thread for controlling duration of test run
        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            vec![msg_reset, msg_stop, msg_start], // first reset errors, then stop refill, then start refill
            tx_test_environment_to_refill,
            tx_signal_handler_to_messaging,
            rx_signal_handler_from_messaging,
            mq_file_name_for_test_environment,
        );

        // thread for the test object
        let join_handle_test_object = create_test_object(
            messaging,
            tx_messaging_to_signal_handler,
            rx_messaging_from_signal_handler,
            tx_messaging_to_refill,
            tx_messaging_to_ventilation,
            tx_messaging_to_heating,
            tx_messaging_to_feed,
            tx_messaging_to_balling,
            tx_messaging_to_monitors,
            tx_messaging_to_watchdog,
        );

        join_handle_mock_refill
            .join()
            .expect("Mock refill thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[cfg(target_os = "linux")]
    // Test case checks if messages for feed control are triggering the corresponding channel communication
    #[test]
    pub fn test_messaging_feed() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.messaging.mq_filename = "/aquarium_control.8UXp".to_string();
        let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

        let mut msg_exec = Message::new();
        msg_exec.domain = messaging_domains::FEED;
        msg_exec.command = messaging_commands::EXEC;
        msg_exec.command_param1 = 13;

        let mut msg_start = Message::new();
        msg_start.domain = messaging_domains::FEED;
        msg_start.command = messaging_commands::START;

        let mut msg_stop = Message::new();
        msg_stop.domain = messaging_domains::FEED;
        msg_stop.command = messaging_commands::STOP;

        let messaging = Messaging::new(config.messaging);

        let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler) = mpsc::channel();
        let (tx_messaging_to_refill, _rx_refill_from_messaging) = mpsc::channel();
        let (tx_messaging_to_ventilation, _rx_ventilation_from_messaging) = mpsc::channel();
        let (tx_messaging_to_heating, _rx_heating_from_messaging) = mpsc::channel();
        let (tx_messaging_to_feed, rx_feed_from_messaging) = mpsc::channel();
        let (tx_messaging_to_balling, _rx_balling_from_messaging) = mpsc::channel();
        let (tx_messaging_to_monitors, _rx_monitors_from_messaging) = mpsc::channel();
        let (tx_messaging_to_watchdog, _rx_monitors_from_watchdog) = mpsc::channel();
        let (tx_messaging_to_signal_handler, rx_signal_handler_from_messaging) = mpsc::channel();

        let tx_test_environment_to_feed = tx_messaging_to_feed.clone();

        let join_handle_mock_feed = thread::Builder::new()
            .name("mock_feed".to_string())
            .spawn(move || {
                let mut commands_received = mock_message_command_receiver(rx_feed_from_messaging);

                // assert the number of commands sent/received are identical
                assert_eq!(3, commands_received.len());

                // assert content of last message sent from test environment:
                assert_eq!(InternalCommand::Start, commands_received.pop().unwrap());

                // assert the content of the second message sent from the test environment:
                assert_eq!(InternalCommand::Stop, commands_received.pop().unwrap());

                // assert the content of the first message sent from the test environment:
                assert_eq!(
                    InternalCommand::Execute(13),
                    commands_received.pop().unwrap()
                );
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            vec![msg_exec, msg_stop, msg_start], // first execute a feed profile, then stop feed, then start feed
            tx_test_environment_to_feed,
            tx_signal_handler_to_messaging,
            rx_signal_handler_from_messaging,
            mq_filename_for_test_environment,
        );

        // thread for the test object
        let join_handle_test_object = create_test_object(
            messaging,
            tx_messaging_to_signal_handler,
            rx_messaging_from_signal_handler,
            tx_messaging_to_refill,
            tx_messaging_to_ventilation,
            tx_messaging_to_heating,
            tx_messaging_to_feed,
            tx_messaging_to_balling,
            tx_messaging_to_monitors,
            tx_messaging_to_watchdog,
        );

        join_handle_mock_feed
            .join()
            .expect("Mock feed thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[cfg(target_os = "linux")]
    // Test case checks if messages for Balling control are triggering the corresponding channel communication
    #[test]
    pub fn test_messaging_balling() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.messaging.mq_filename = "/aquarium_control.9UXp".to_string();
        let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

        open_posixmq_test_file(&config.messaging.mq_filename);

        let mut msg_start = Message::new();
        msg_start.domain = messaging_domains::BALLING;
        msg_start.command = messaging_commands::START;

        let mut msg_stop = Message::new();
        msg_stop.domain = messaging_domains::BALLING;
        msg_stop.command = messaging_commands::STOP;

        let messaging = Messaging::new(config.messaging);

        let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler) = mpsc::channel();
        let (tx_messaging_to_refill, _rx_refill_from_messaging) = mpsc::channel();
        let (tx_messaging_to_ventilation, _rx_ventilation_from_messaging) = mpsc::channel();
        let (tx_messaging_to_heating, _rx_heating_from_messaging) = mpsc::channel();
        let (tx_messaging_to_feed, _rx_feed_from_messaging) = mpsc::channel();
        let (tx_messaging_to_balling, rx_balling_from_messaging) = mpsc::channel();
        let (tx_messaging_to_monitors, _rx_monitors_from_messaging) = mpsc::channel();
        let (tx_messaging_to_watchdog, _rx_monitors_from_watchdog) = mpsc::channel();
        let (tx_messaging_to_signal_handler, rx_signal_handler_from_messaging) = mpsc::channel();

        let tx_test_environment_to_balling = tx_messaging_to_balling.clone();

        let join_handle_mock_balling = thread::Builder::new()
            .name("mock_balling".to_string())
            .spawn(move || {
                assert_stop_start_sequence(mock_message_command_receiver(
                    rx_balling_from_messaging,
                ));
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            vec![msg_stop, msg_start], // first stop, then start Balling dosing control
            tx_test_environment_to_balling,
            tx_signal_handler_to_messaging,
            rx_signal_handler_from_messaging,
            mq_filename_for_test_environment,
        );

        // thread for the test object
        let join_handle_test_object = create_test_object(
            messaging,
            tx_messaging_to_signal_handler,
            rx_messaging_from_signal_handler,
            tx_messaging_to_refill,
            tx_messaging_to_ventilation,
            tx_messaging_to_heating,
            tx_messaging_to_feed,
            tx_messaging_to_balling,
            tx_messaging_to_monitors,
            tx_messaging_to_watchdog,
        );

        join_handle_mock_balling
            .join()
            .expect("Mock Balling thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[cfg(target_os = "linux")]
    // Test case checks if messages for Watchdog control are triggering the corresponding channel communication
    #[test]
    pub fn test_messaging_watchdog() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.messaging.mq_filename = "/aquarium_control.9UXp".to_string();
        let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

        let mut msg_start = Message::new();
        msg_start.domain = messaging_domains::WATCHDOG;
        msg_start.command = messaging_commands::START;

        let mut msg_stop = Message::new();
        msg_stop.domain = messaging_domains::WATCHDOG;
        msg_stop.command = messaging_commands::STOP;

        let messaging = Messaging::new(config.messaging);

        let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler) = mpsc::channel();
        let (tx_messaging_to_refill, _rx_refill_from_messaging) = mpsc::channel();
        let (tx_messaging_to_ventilation, _rx_ventilation_from_messaging) = mpsc::channel();
        let (tx_messaging_to_heating, _rx_heating_from_messaging) = mpsc::channel();
        let (tx_messaging_to_feed, _rx_feed_from_messaging) = mpsc::channel();
        let (tx_messaging_to_balling, _rx_balling_from_messaging) = mpsc::channel();
        let (tx_messaging_to_monitors, _rx_monitors_from_messaging) = mpsc::channel();
        let (tx_messaging_to_signal_handler, rx_signal_handler_from_messaging) = mpsc::channel();
        let (tx_messaging_to_watchdog, rx_watchdog_from_messaging) = mpsc::channel();

        let tx_test_environment_to_watchdog = tx_messaging_to_watchdog.clone();

        let join_handle_mock_watchdog = thread::Builder::new()
            .name("mock_watchdog".to_string())
            .spawn(move || {
                assert_stop_start_sequence(mock_message_command_receiver(
                    rx_watchdog_from_messaging,
                ));
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            vec![msg_stop, msg_start], // first stop, then start Watchdog
            tx_test_environment_to_watchdog,
            tx_signal_handler_to_messaging,
            rx_signal_handler_from_messaging,
            mq_filename_for_test_environment,
        );

        // thread for the test object
        let join_handle_test_object = create_test_object(
            messaging,
            tx_messaging_to_signal_handler,
            rx_messaging_from_signal_handler,
            tx_messaging_to_refill,
            tx_messaging_to_ventilation,
            tx_messaging_to_heating,
            tx_messaging_to_feed,
            tx_messaging_to_balling,
            tx_messaging_to_monitors,
            tx_messaging_to_watchdog,
        );

        join_handle_mock_watchdog
            .join()
            .expect("Mock Balling thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }
}
