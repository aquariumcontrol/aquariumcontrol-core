/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(target_os = "linux")]
use posixmq::PosixMq;
use serde_derive::Deserialize;
use std::fs;
use std::io::ErrorKind;
use std::process;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use crate::InternalCommand;

/// Contains constants of all domains addressed in IPC messages
pub mod messaging_domains {
    pub const REFILL: i32 = 1;
    pub const VENTILATION: i32 = 3;
    pub const FEED: i32 = 8;
    pub const HEATING: i32 = 9;
    pub const BALLING: i32 = 11;
    pub const MONITORS: i32 = 12;
}

#[derive(PartialEq)]
pub enum MessagingDomain {
    Refill,
    Ventilation,
    Feed,
    Heating,
    Balling,
    Monitors,
    Unknown,
}

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

pub struct Message {
    domain: i32,
    command: i32,
    command_param1: i32,
    command_param2: i32,
}

impl Message {
    /// Provides empty message initialized with zeros
    pub fn new() -> Message {
        Message {
            domain: 0,
            command: 0,
            command_param1: 0,
            command_param2: 0,
        }
    }
}

/// Holds the configuration data for the messaging.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct MessagingConfig {
    /// filename used for publishing the PID
    pid_filename: String,

    /// filename used for generating the messsage queue identifier
    mq_filename: String,

    /// timeout for receiving messages in milliseconds
    /// after each timeout, the application checks if a shutdown has been requested
    timeout_millis: u64,
}

/// Checks if there are new messages in the channel.
/// if the message contains the Terminate command, then the function returns true, otherwise false.
/// # Arguments
/// * `rx_from_signal_handler` - receiver part of channel for communication from signal handler
pub fn check_pending_external_request(rx_from_signal_handler: &Receiver<InternalCommand>) -> bool {
    match rx_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Terminate => true,
            _ => {
                println!("Messaging: ignoring inapplicable command from signal handler.");
                false
            }
        },
        Err(e) => {
            match e {
                TryRecvError::Empty => { /* empty buffer - no action required */ }
                TryRecvError::Disconnected => {
                    println!(
                        "Messaging: Error in trying to receive command from signal handler: {e:?}"
                    );
                }
            }
            false
        }
    }
}

pub struct Messaging {
    config: MessagingConfig,

    // Message queue
    #[cfg(target_os = "linux")]
    mq: PosixMq,
}

impl Messaging {
    /// Checks if PID file already contains published PID.
    /// If yes, then application will panic.
    /// If no, then function publishes PID provides a struct of type Messaging.
    /// If publishing does not work, then the application will panic.
    /// # Arguments
    /// * `config` - configuration data for Balling dosing control
    pub fn new(config: MessagingConfig) -> Messaging {
        let file_data = fs::read_to_string(config.pid_filename.clone())
            .expect("Messaging: Unable to read PID file.");
        let file_data_trimmed = file_data.trim();

        if !file_data_trimmed.is_empty() {
            let process_id_read: u32 = match file_data_trimmed.parse() {
                Ok(c) => c,
                Err(e) => {
                    panic!(
                        "Messaging: PID file ({}) contains invalid content: {e:?}",
                        config.pid_filename
                    );
                }
            };
            if process_id_read > 0 {
                panic!(
                    "Messaging: PID file ({}) already contains published PID.",
                    config.pid_filename
                );
            }
        }
        let process_id_numeric: u32 = process::id();
        let process_id_string: String = process_id_numeric.to_string() + "\n";
        fs::write(config.pid_filename.clone(), process_id_string)
            .expect("Messaging: Could not publish PID.");

        #[cfg(target_os = "linux")]
        let mq = match PosixMq::open(&config.mq_filename) {
            Ok(c) => {
                println!("Messaging: Could open message queue.");
                c
            }
            Err(e) => {
                panic!(
                    "Messaging: failure to open message queue using {}, error={e:?}",
                    &config.mq_filename
                );
            }
        };

        Messaging {
            config: config,

            #[cfg(target_os = "linux")]
            mq: mq,
        }
    }

    #[cfg(target_os = "linux")]
    pub fn execute(
        &self,
        rx_from_signal_handler: Receiver<InternalCommand>,
        tx_to_refill: Sender<InternalCommand>,
        tx_to_ventilation: Sender<InternalCommand>,
        tx_to_heating: Sender<InternalCommand>,
        tx_to_feed: Sender<InternalCommand>,
        tx_to_balling: Sender<InternalCommand>,
        tx_to_monitors: Sender<InternalCommand>,
    ) {
        let duration_timeout = Duration::from_millis(self.config.timeout_millis);

        // identify maximum length of message queue
        let mq_max_msg_len = match self.mq.attributes() {
            Ok(c) => c.max_msg_len,
            Err(e) => {
                panic!("Messaging: Failed to open attributes ({e:?}");
            }
        };

        let mut msgbuf = vec![0; mq_max_msg_len];
        let mut msg = Message::new();

        loop {
            match self.mq.recv_timeout(&mut msgbuf, duration_timeout) {
                Ok((r1, r2)) => {
                    println!(
                        "Messaging: POSIX message received with r1={}, r2={}.",
                        r1, r2
                    );
                    println!("Messaging: Received message has size {}.", msgbuf.len());
                    let zero_char: u8 = 0;
                    for (position, non_zero_char) in msgbuf.iter().enumerate() {
                        if *non_zero_char != zero_char {
                            println!("[{}] {}", position, non_zero_char);
                        }
                    }
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

                        println!(
                            "Messaging: domain={}, cmd={}, param1={}, param2={}",
                            msg.domain, msg.command, msg.command_param1, msg.command_param2
                        );

                        // interpret domain in message
                        let domain: MessagingDomain = match msg.domain {
                            messaging_domains::REFILL => MessagingDomain::Refill,
                            messaging_domains::VENTILATION => MessagingDomain::Ventilation,
                            messaging_domains::FEED => MessagingDomain::Feed,
                            messaging_domains::HEATING => MessagingDomain::Heating,
                            messaging_domains::BALLING => MessagingDomain::Balling,
                            messaging_domains::MONITORS => MessagingDomain::Monitors,
                            _ => {
                                println!(
                                    "Messaging: ignoring message for unknown domain {}",
                                    msg.domain
                                );
                                MessagingDomain::Unknown
                            }
                        };

                        // interpret command in message
                        let command: InternalCommand = match msg.command {
                            messaging_commands::RESET => InternalCommand::ResetAllErrors,
                            messaging_commands::STOP => InternalCommand::Stop,
                            messaging_commands::START => InternalCommand::Start,
                            messaging_commands::EXEC => {
                                InternalCommand::Execute(msg.command_param1)
                            }
                            _ => {
                                println!(
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
                                MessagingDomain::Unknown => unreachable!(),
                            };

                            match tx_to_domain.send(command) {
                                Ok(_) => { /* do nothing */ }
                                Err(e) => {
                                    println!(
                                        "messaging: Error in channel communication to domain: {e:?}"
                                    );
                                }
                            }
                        }
                    } else {
                        println!(
                            "Messaging: Message has incorrect length ({}). Ignoring it.",
                            msgbuf.len()
                        );
                    }
                }
                Err(e) => {
                    match e.kind() {
                        ErrorKind::TimedOut => { /* do nothing */ }
                        _ => {
                            println!("Messaging: Error occurred when reading POSIX message: {e:?}");
                        }
                    }
                }
            }
            if check_pending_external_request(&rx_from_signal_handler) {
                break;
            }
        }
    }

    /// Erases PID from previously generated file
    pub fn erase_pid(&self) {
        fs::write(self.config.pid_filename.clone(), "0".to_string())
            .expect("Messaging: Could not erase PID.");
    }
}

#[cfg(test)]
pub mod tests {
    use crate::config::ConfigData;
    use crate::messaging::Messaging;
    use crate::read_config_file;
    use std::fs;
    use std::process;

    #[test]
    pub fn test_messaging_publish_pid() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_publish_pid.toml".to_string());
        let pid_filename = config.messaging.pid_filename.clone();
        fs::write(pid_filename.clone(), "0".to_string())
            .expect("test_messaging_publish_PID: Could not erase PID.");

        // create test object and run code to be tested
        _ = Messaging::new(config.messaging);

        let process_id_numeric: u32 = process::id();
        let process_id_string: String = process_id_numeric.to_string() + "\n";

        let file_data = fs::read_to_string(pid_filename)
            .expect("test_messaging_publish_PID: Unable to read file");

        assert_eq!(file_data, process_id_string);
        println!(
            "Data read from file ({}) equals data generated ({}).",
            { file_data.trim() },
            { process_id_string.trim() }
        );
    }

    #[test]
    #[should_panic]
    pub fn test_messaging_non_existent_pid_file() {
        let config: ConfigData = read_config_file(
            "/config/aquarium_control_test_messaging_non_existent_pid_file.toml".to_string(),
        );
        _ = Messaging::new(config.messaging);
    }

    #[test]
    #[should_panic]
    pub fn test_messaging_invalid_content_pid_file() {
        let config: ConfigData = read_config_file(
            "/config/aquarium_control_test_messaging_invalid_content_pid_file.toml".to_string(),
        );
        _ = Messaging::new(config.messaging);
    }
}
