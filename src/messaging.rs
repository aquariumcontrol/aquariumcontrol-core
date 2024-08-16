/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(target_os = "linux")]
use posixmq::PosixMq;
use serde_derive::Deserialize;
use std::io::ErrorKind;
#[cfg(target_os = "linux")]
use std::sync::mpsc::{Receiver, Sender};
#[cfg(target_os = "linux")]
use std::time::Duration;

use crate::proc_ext_req::ProcessExternalRequestTrait;
use crate::InternalCommand;

#[cfg(target_os = "linux")]
/// Contains constants of all domains addressed in IPC messages
pub mod messaging_domains {
    pub const REFILL: i32 = 1;
    pub const VENTILATION: i32 = 3;
    pub const FEED: i32 = 8;
    pub const HEATING: i32 = 9;
    pub const BALLING: i32 = 11;
    pub const MONITORS: i32 = 12;
}

#[cfg(target_os = "linux")]
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

    #[cfg(test)]
    /// create message represented as raw byte array for testing
    pub fn create_message_buf(&self) -> Vec<u8> {
        let mut buf = vec![0; 20];
        let domain_buf = self.domain.to_le_bytes();
        let command_buf = self.command.to_le_bytes();
        let command_param1_buf = self.command_param1.to_le_bytes();
        let command_param2_buf = self.command_param2.to_le_bytes();

        // transfer partial arrays to array for complete message
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
    /// filename used for generating the messsage queue identifier
    mq_filename: String,

    /// timeout for receiving messages in milliseconds
    /// after each timeout, the application checks if a shutdown has been requested
    timeout_millis: u64,
}

pub struct Messaging {
    config: MessagingConfig,

    // Message queue
    #[cfg(target_os = "linux")]
    mq: PosixMq,
}

impl Messaging {
    /// Opens the message queue
    /// # Arguments
    /// * `config` - configuration data for Messaging
    pub fn new(config: MessagingConfig) -> Messaging {
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
        &mut self,
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
            let (quit_command_received, _, _) =
                self.process_external_request(&rx_from_signal_handler, &rx_from_signal_handler);
            if quit_command_received {
                break;
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use posixmq::PosixMq;
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::Duration;

    use crate::channel_content::AquariumDevice;
    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::messaging::messaging_commands;
    use crate::messaging::messaging_domains;
    use crate::messaging::Message;
    use crate::messaging::Messaging;
    use crate::ventilation::tests::mock_schedule_check;
    use crate::ventilation::tests::run_main_cycle_with_stimuli;
    use crate::ventilation::tests::test_ventilation_receive_actuation_command_send_confirmation;
    use crate::ventilation::tests::test_ventilation_receive_information_for_datalogger;
    use crate::ventilation::Ventilation;

    #[cfg(target_os = "linux")]
    // Test case runs ventilation control and triggers inhibition by sending stop message.
    // After verification that ventilation is stopped,
    // test case sends start message and verifies if ventilation control
    // resumes operation.
    #[test]
    pub fn test_messaging_stops_starts_ventilation() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_ventilation.toml".to_string());
        let mq_stimuli = match PosixMq::open(&config.messaging.mq_filename) {
            Ok(c) => {
                println!("Messaging: Could open message queue.");
                c
            }
            Err(e) => {
                panic!(
                    "Messaging: failure to open message queue for test using {}, error={e:?}",
                    &config.messaging.mq_filename
                );
            }
        };

        let mut msg_start = Message::new();
        msg_start.domain = messaging_domains::VENTILATION;
        msg_start.command = messaging_commands::START;

        let mut msg_stop = Message::new();
        msg_stop.domain = messaging_domains::VENTILATION;
        msg_stop.command = messaging_commands::STOP;

        let mut ventilation = Ventilation::new(config.ventilation);
        let mut messaging = Messaging::new(config.messaging);

        let (tx_ventilation_to_gpio, rx_gpio_from_ventilation) = mpsc::channel();
        let (tx_gpio_to_ventilation, rx_ventilation_from_gpio) = mpsc::channel();
        let (tx_ventilation_to_datalogger, rx_datalogger_from_ventilation) = mpsc::channel();
        let (tx_ventilation_to_controllino, rx_controllino_from_ventilation) = mpsc::channel();
        let (tx_controllino_to_ventilation, rx_ventilation_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_ventilation, rx_ventilation_from_signal_handler) =
            mpsc::channel();
        let (tx_ventilation_to_signal_handler, rx_signal_handler_from_ventilation) =
            mpsc::channel();
        let (tx_schedule_check_to_ventilation, rx_ventilation_from_schedule_check) =
            mpsc::channel();
        let (tx_ventilation_to_schedule_check, rx_schedule_check_from_ventilation) =
            mpsc::channel();
        let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler) = mpsc::channel();
        let (tx_messaging_to_refill, _rx_refill_from_messaging) = mpsc::channel();
        let (tx_messaging_to_ventilation, rx_ventilation_from_messaging) = mpsc::channel();
        let (tx_messaging_to_heating, _rx_heating_from_messaging) = mpsc::channel();
        let (tx_messaging_to_feed, _rx_feed_from_messaging) = mpsc::channel();
        let (tx_messaging_to_balling, _rx_balling_from_messaging) = mpsc::channel();
        let (tx_messaging_to_monitors, _rx_monitors_from_messaging) = mpsc::channel();

        let tx_ventilation_to_schedule_check_for_test_case_finish =
            tx_ventilation_to_schedule_check.clone();

        scope(|scope| {
            // thread for mock schedule check: runs until it receives Quit command
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_ventilation,
                    &rx_schedule_check_from_ventilation,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

            // thread for mock GPIO and IPC messaging - this scope controls execution of the test case
            scope.spawn(move || {
                run_main_cycle_with_stimuli(
                    4,
                    &rx_gpio_from_ventilation,
                    &tx_gpio_to_ventilation,
                    30.0,
                );

                // sending IPC message requesting stop of ventilation control
                let _ = mq_stimuli.send(0, &msg_stop.create_message_buf());

                run_main_cycle_with_stimuli(
                    4,
                    &rx_gpio_from_ventilation,
                    &tx_gpio_to_ventilation,
                    30.0,
                );

                // sending IPC message requesting stop of ventilation control
                let _ = mq_stimuli.send(0, &msg_start.create_message_buf());

                run_main_cycle_with_stimuli(
                    4,
                    &rx_gpio_from_ventilation,
                    &tx_gpio_to_ventilation,
                    30.0,
                );

                // requesting ventilation control to quit
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);

                // requesting messaging to quit
                let _ = tx_signal_handler_to_messaging.send(InternalCommand::Quit);
            });

            // thread for mock Controllino, DataLogger
            scope.spawn(move || {
                let command1 = test_ventilation_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
                );
                assert_eq!(command1, Some(InternalCommand::SwitchOn(AquariumDevice::Ventilation)));
                let state1 = test_ventilation_receive_information_for_datalogger(
                    &rx_datalogger_from_ventilation,
                );
                assert_eq!(state1, true);
                
                let command2 = test_ventilation_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
                );
                assert_eq!(command2, Some(InternalCommand::SwitchOff(AquariumDevice::Ventilation)));                
                let state2 = test_ventilation_receive_information_for_datalogger(
                    &rx_datalogger_from_ventilation,
                );
                assert_eq!(state2, false);
                
                let command3 = test_ventilation_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
                );
                assert_eq!(command3, Some(InternalCommand::SwitchOn(AquariumDevice::Ventilation)));
                let state3 = test_ventilation_receive_information_for_datalogger(
                    &rx_datalogger_from_ventilation,
                );
                assert_eq!(state3, true);
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object #1 - messaging
            scope.spawn(move || {
                messaging.execute(
                    rx_messaging_from_signal_handler,
                    tx_messaging_to_refill,
                    tx_messaging_to_ventilation,
                    tx_messaging_to_heating,
                    tx_messaging_to_feed,
                    tx_messaging_to_balling,
                    tx_messaging_to_monitors,
                );
            });

            // thread for test object #2
            scope.spawn(move || {
                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    tx_ventilation_to_gpio,
                    rx_ventilation_from_gpio,
                    tx_ventilation_to_controllino,
                    rx_ventilation_from_controllino,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_datalogger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    rx_ventilation_from_messaging,
                );

                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);

                let actuation_count = *mutex_device_scheduler_ventilation.lock().unwrap();

                // check if ventilation is only actuated once (when quitting)
                assert_eq!(actuation_count, 3);
            });
            println!("* [Messaging] checking if messaging can stop and start ventilation");
        });
    }

    #[cfg(target_os = "linux")]
    // Test case runs Balling dosing control and triggers inhibition by sending stop message.
    // After verification that Balling dosing is stopped,
    // test case sends start message and verifies if Balling dosing control
    // resumes operation.
    #[test]
    pub fn test_messaging_stops_starts_balling_dosing() {}

    #[cfg(target_os = "linux")]
    // Test case runs heating control and triggers inhibition by sending stop message.
    // After verification that heating is stopped,
    // test case sends start message and verifies if heating control
    // resumes operation.
    #[test]
    pub fn test_messaging_stops_starts_heating() {}

    #[cfg(target_os = "linux")]
    // Test case runs feed control and triggers inhibition by sending stop message.
    // After verification that feed is stopped,
    // test case sends start message and verifies if feed control
    // resumes operation.
    #[test]
    pub fn test_messaging_stops_starts_feed() {}

    #[cfg(target_os = "linux")]
    // Test case runs feed control and triggers execution of a specific feed profile
    // by sending "execute" message.
    #[test]
    pub fn test_messaging_executes_feed_profile() {}

    #[cfg(target_os = "linux")]
    // Test case runs refill control and triggers inhibition by sending stop message.
    // After verification that refill is stopped,
    // test case sends start message and verifies if feed control
    // resumes operation.
    #[test]
    pub fn test_messaging_stops_starts_refill() {}

    #[cfg(target_os = "linux")]
    // Test case runs refill control and triggers reset of error by sending "ResetAllErrors" message.
    #[test]
    pub fn test_messaging_resets_refill_errors() {}
}
