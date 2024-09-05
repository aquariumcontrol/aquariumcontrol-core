/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(target_os = "linux")]
use log::{error, info, warn};
#[cfg(target_os = "linux")]
use posixmq::PosixMq;
use serde_derive::Deserialize;
use std::fmt;
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
    /// value represents the refill control as domain
    pub const REFILL: i32 = 1;

    /// value represents the ventilation control as domain
    pub const VENTILATION: i32 = 3;

    /// value represents the feed control as domain
    pub const FEED: i32 = 8;

    /// value represents the heating control as domain
    pub const HEATING: i32 = 9;

    /// value represents the Balling dosing control as domain
    pub const BALLING: i32 = 11;

    /// value represents the monitoring as domain
    pub const MONITORS: i32 = 12;
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

    /// in case numeric value in message could not be assigned to one of the above
    Unknown,
}

impl fmt::Display for MessagingDomain {
    /// Provides output of the enum values for debugging purposes.
    /// # Arguments
    /// * `f` - configuration for formatting
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MessagingDomain::Refill => write!(f, "Refill"),
            MessagingDomain::Ventilation => write!(f, "Ventilation"),
            MessagingDomain::Feed => write!(f, "Feed"),
            MessagingDomain::Heating => write!(f, "Heating"),
            MessagingDomain::Balling => write!(f, "Balling"),
            MessagingDomain::Monitors => write!(f, "Monitors"),
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

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Struct provides data structure and implementation for IPC message queue communication.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     signal_handler[Signal handler] --> messaging[Messaging]
///     messaging --> refill[Refill control]
///     messaging --> ventilation[Ventilation control]
///     messaging --> heating[Heating control]
///     messaging --> feed[Feed control]
///     messaging --> balling[Balling dosing control]
///     messaging --> monitors[Monitors]
/// ```

pub struct Messaging {
    config: MessagingConfig,

    // Message queue
    #[cfg(target_os = "linux")]
    mq: PosixMq,
    mq_max_msg_len: usize,
}

impl Messaging {
    const LOG_TARGET_MESSAGING: &'static str = "messaging";

    /// Opens the message queue
    /// # Arguments
    /// * `config` - configuration data for Messaging
    pub fn new(config: MessagingConfig) -> Messaging {
        #[cfg(target_os = "linux")]
        let mq = match PosixMq::open(&config.mq_filename) {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "{}: failure to open message queue using {} ({e:?})",
                    Self::LOG_TARGET_MESSAGING,
                    &config.mq_filename
                );
            }
        };

        // identify maximum length of message queue
        let mq_max_msg_len = match self.mq.attributes() {
            Ok(c) => c.max_msg_len,
            Err(e) => {
                panic!(
                    "{}: Failed to open attributes ({e:?})",
                    Self::LOG_TARGET_MESSAGING,
                );
            }
        };

        Messaging {
            config: config,

            #[cfg(target_os = "linux")]
            mq: mq,
            mq_max_msg_len: mq_max_msg_len,
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

        let mut msgbuf = vec![0; self.mq_max_msg_len];
        let mut msg = Message::new();

        loop {
            match self.mq.recv_timeout(&mut msgbuf, duration_timeout) {
                Ok((r1, r2)) => {
                    //println!(
                    //    "{}: POSIX message received with r1={}, r2={}, length={}.",
                    //    Self::LOG_TARGET_MESSAGING,
                    //    r1, r2, msgbuf.len()
                    //);
                    //let zero_char: u8 = 0;
                    //for (position, non_zero_char) in msgbuf.iter().enumerate() {
                    //    if *non_zero_char != zero_char {
                    //        println!("[{}] {}", position, non_zero_char);
                    //    }
                    //}
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

                        info!(
                            target: Self::LOG_TARGET_MESSAGING,
                            "received message: domain={}, cmd={}, param1={}, param2={}",
                            msg.domain,
                            msg.command,
                            msg.command_param1,
                            msg.command_param2
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
                                warn!(
                                    target: Self::LOG_TARGET_MESSAGING,
                                    "ignoring message for unknown domain {}",
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
                                warn!(
                                    target: Self::LOG_TARGET_MESSAGING,
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

                            info!(
                                target: Self::LOG_TARGET_MESSAGING,
                                "forwarding message to domain {}",
                                domain
                            );

                            match tx_to_domain.send(command) {
                                Ok(_) => { /* do nothing */ }
                                Err(e) => {
                                    error!(
                                        target: Self::LOG_TARGET_MESSAGING,
                                        "channel communication to domain {} failed ({e:?})",
                                        domain
                                    );
                                }
                            }
                        }
                    } else {
                        error!(
                            target: Self::LOG_TARGET_MESSAGING,
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
                                target: Self::LOG_TARGET_MESSAGING,
                                "timeout occurred when reading POSIX message ({e:?})"
                            );
                        }
                    }
                }
            }
            let (quit_command_received, _, _) =
                self.process_external_request(&rx_from_signal_handler, None);
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
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::Duration;

    use crate::balling::tests::MockMineralInjection;
    use crate::balling::Balling;
    use crate::channel_content::AquariumDevice;
    use crate::channel_content::AquariumSignal;
    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::feed::tests::MockFoodInjection;
    use crate::feed::Feed;
    use crate::heating::tests::prepare_heating_tests_mock_midnight_calculator;
    use crate::heating::tests::HeatingStatsMockDataTransfer;
    use crate::messaging::messaging_commands;
    use crate::messaging::messaging_domains;
    use crate::messaging::Message;
    use crate::messaging::Messaging;
    use crate::refill::tests::RefillTestExecutionVoid;
    use crate::refill::Refill;
    use crate::refill::RefillConfig;
    use crate::refill::RefillErrors;
    use crate::refill::TankLevelSwitchSignals;
    use crate::sql_interface::SqlInterface;
    use crate::sql_interface_balling::SqlInterfaceBalling;
    use crate::sql_interface_feed::SqlInterfaceFeed;
    use crate::sql_interface_refill::SqlInterfaceRefill;
    use crate::ventilation::tests::mock_schedule_check;
    use crate::ventilation::tests::run_main_cycle_with_stimuli;
    use crate::ventilation::tests::test_receive_actuation_command_send_confirmation;
    use crate::ventilation::tests::test_receive_information_for_datalogger;
    use crate::ventilation::Ventilation;
    use crate::water_injection::WaterInjectionTrait;

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
                let command1 = test_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
                );
                assert_eq!(
                    command1,
                    Some(InternalCommand::SwitchOn(AquariumDevice::Ventilation))
                );
                let state1 =
                    test_receive_information_for_datalogger(&rx_datalogger_from_ventilation);
                assert_eq!(state1, true);

                let command2 = test_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
                );
                assert_eq!(
                    command2,
                    Some(InternalCommand::SwitchOff(AquariumDevice::Ventilation))
                );
                let state2 =
                    test_receive_information_for_datalogger(&rx_datalogger_from_ventilation);
                assert_eq!(state2, false);

                let command3 = test_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
                );
                assert_eq!(
                    command3,
                    Some(InternalCommand::SwitchOn(AquariumDevice::Ventilation))
                );
                let state3 =
                    test_receive_information_for_datalogger(&rx_datalogger_from_ventilation);
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

                // check amount of actuations (after quitting)
                assert_eq!(actuation_count, 3);
            });
            println!(
                "* [Messaging] checking if messaging can stop and start ventilation succeeded."
            );
        });
    }

    #[cfg(target_os = "linux")]
    // Test case runs Balling dosing control and triggers inhibition by sending stop message.
    // After verification that Balling dosing is stopped,
    // test case sends start message and verifies if Balling dosing control
    // resumes operation.
    #[test]
    pub fn test_messaging_stops_starts_balling_dosing() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let config: ConfigData = read_config_file(
            "/config/aquarium_control_test_balling_multiple_dosings.toml".to_string(),
        );
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

        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_balling = SqlInterfaceBalling::new(sql_interface.get_connection());

        let mut msg_start = Message::new();
        msg_start.domain = messaging_domains::BALLING;
        msg_start.command = messaging_commands::START;

        let mut msg_stop = Message::new();
        msg_stop.domain = messaging_domains::BALLING;
        msg_stop.command = messaging_commands::STOP;

        // empty the Balling dosing log table
        match SqlInterface::truncate_table(&mut sql_interface, "ballingdosinglog".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        // create test objects
        let mut balling = Balling::new(config.balling, sql_interface_balling);
        let mut messaging = Messaging::new(config.messaging);

        let (tx_balling_to_controllino, _rx_controllino_from_balling) = mpsc::channel();
        let (_tx_controllino_to_balling, rx_balling_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_balling, rx_balling_from_signal_handler) = mpsc::channel();
        let (tx_balling_to_signal_handler, rx_signal_handler_from_balling) = mpsc::channel();
        let (tx_schedule_check_to_balling, rx_balling_from_schedule_check) = mpsc::channel();
        let (tx_balling_to_schedule_check, rx_schedule_check_from_balling) = mpsc::channel();
        let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler) = mpsc::channel();
        let (tx_messaging_to_refill, _rx_refill_from_messaging) = mpsc::channel();
        let (tx_messaging_to_ventilation, _rx_ventilation_from_messaging) = mpsc::channel();
        let (tx_messaging_to_heating, _rx_heating_from_messaging) = mpsc::channel();
        let (tx_messaging_to_feed, _rx_feed_from_messaging) = mpsc::channel();
        let (tx_messaging_to_balling, rx_balling_from_messaging) = mpsc::channel();
        let (tx_messaging_to_monitors, _rx_monitors_from_messaging) = mpsc::channel();

        let tx_balling_to_schedule_check_for_test_case_finish =
            tx_balling_to_schedule_check.clone();

        let mut mock_mineral_injection = MockMineralInjection::new();

        scope(|scope| {
            // thread for mock schedule check: runs until it receives Quit command
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_balling,
                    &rx_schedule_check_from_balling,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            let mutex_device_scheduler_balling_test_object = Arc::new(Mutex::new(0));
            let mutex_device_scheduler_balling_test_environment =
                mutex_device_scheduler_balling_test_object.clone();

            // thread IPC messaging - this scope controls execution of the test case
            scope.spawn(move || {
                // wait for 1 second to make sure test object can receive message
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millisec);
                }

                // sending IPC message requesting stop of Balling dosing control
                let _ = mq_stimuli.send(0, &msg_stop.create_message_buf());

                // wait for 10 seconds. If test object does not follow stop request,
                // then dosings will happen in this period.
                for _ in 0..100 {
                    spin_sleeper.sleep(sleep_duration_100_millisec);
                }

                let actuation_count_1 = *mutex_device_scheduler_balling_test_environment
                    .lock()
                    .unwrap();

                // check amount of actuations (after stop request)
                assert_eq!(actuation_count_1, 4);

                // sending IPC message requesting restart of Balling dosing control
                let _ = mq_stimuli.send(0, &msg_start.create_message_buf());

                // wait for 10 seconds. If test object follows start request,
                // then dosings will happen in this period.
                for _ in 0..100 {
                    spin_sleeper.sleep(sleep_duration_100_millisec);
                }

                let actuation_count_2 = *mutex_device_scheduler_balling_test_environment
                    .lock()
                    .unwrap();

                // check amount of actuations (after start request)
                assert_eq!(actuation_count_2, 9);

                // requesting Balling dosing control to quit
                let _ = tx_signal_handler_to_balling.send(InternalCommand::Quit);
                rx_signal_handler_from_balling.recv().unwrap();
                let _ = tx_signal_handler_to_balling.send(InternalCommand::Terminate);

                // requesting messaging to quit
                let _ = tx_signal_handler_to_messaging.send(InternalCommand::Quit);
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
                balling.execute(
                    mutex_device_scheduler_balling_test_object.clone(),
                    tx_balling_to_controllino,
                    rx_balling_from_controllino,
                    tx_balling_to_signal_handler,
                    rx_balling_from_signal_handler,
                    tx_balling_to_schedule_check,
                    rx_balling_from_schedule_check,
                    rx_balling_from_messaging,
                    &mut mock_mineral_injection,
                );
                println!("MockMineralInjection:\n{}", mock_mineral_injection);

                let _ =
                    tx_balling_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            });

            println!(
                "* [Messaging] checking if messaging can stop and start balling dosing succeeded."
            );
        });
    }

    #[cfg(target_os = "linux")]
    // Test case runs heating control and triggers inhibition by sending stop message.
    // After verification that heating is stopped,
    // test case sends start message and verifies if heating control
    // resumes operation.
    #[test]
    pub fn test_messaging_stops_starts_heating() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (config, mut heating, mut sql_interface) =
            prepare_heating_tests_mock_midnight_calculator(3600);

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
        msg_start.domain = messaging_domains::HEATING;
        msg_start.command = messaging_commands::START;

        let mut msg_stop = Message::new();
        msg_stop.domain = messaging_domains::HEATING;
        msg_stop.command = messaging_commands::STOP;

        let mut messaging = Messaging::new(config.messaging);

        let (tx_heating_to_gpio, rx_gpio_from_heating) = mpsc::channel();
        let (tx_gpio_to_heating, rx_heating_from_gpio) = mpsc::channel();
        let (tx_heating_to_datalogger, rx_datalogger_from_heating) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler) = mpsc::channel();
        let (tx_messaging_to_refill, _rx_refill_from_messaging) = mpsc::channel();
        let (tx_messaging_to_ventilation, _rx_ventilation_from_messaging) = mpsc::channel();
        let (tx_messaging_to_heating, rx_heating_from_messaging) = mpsc::channel();
        let (tx_messaging_to_feed, _rx_feed_from_messaging) = mpsc::channel();
        let (tx_messaging_to_balling, _rx_balling_from_messaging) = mpsc::channel();
        let (tx_messaging_to_monitors, _rx_monitors_from_messaging) = mpsc::channel();

        let tx_heating_to_schedule_check_for_test_case_finish =
            tx_heating_to_schedule_check.clone();

        let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();

        scope(|scope| {
            // thread for mock schedule check: runs until it receives Quit command
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

            // thread for mock GPIO and IPC messaging - this scope controls execution of the test case
            scope.spawn(move || {
                run_main_cycle_with_stimuli(16, &rx_gpio_from_heating, &tx_gpio_to_heating, 20.0);

                // sending IPC message requesting stop of heating control
                let _ = mq_stimuli.send(0, &msg_stop.create_message_buf());

                run_main_cycle_with_stimuli(16, &rx_gpio_from_heating, &tx_gpio_to_heating, 20.0);

                // sending IPC message requesting stop of heating control
                let _ = mq_stimuli.send(0, &msg_start.create_message_buf());

                run_main_cycle_with_stimuli(16, &rx_gpio_from_heating, &tx_gpio_to_heating, 20.0);

                // requesting heating control to quit
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);

                // requesting messaging to quit
                let _ = tx_signal_handler_to_messaging.send(InternalCommand::Quit);
            });

            // thread for mock Controllino, DataLogger
            scope.spawn(move || {
                let command1 = test_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_heating,
                    &tx_controllino_to_heating,
                );
                assert_eq!(
                    command1,
                    Some(InternalCommand::SwitchOn(AquariumDevice::Heater))
                );
                let state1 = test_receive_information_for_datalogger(&rx_datalogger_from_heating);
                assert_eq!(state1, true);

                let command2 = test_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_heating,
                    &tx_controllino_to_heating,
                );
                assert_eq!(
                    command2,
                    Some(InternalCommand::SwitchOff(AquariumDevice::Heater))
                );
                let state2 = test_receive_information_for_datalogger(&rx_datalogger_from_heating);
                assert_eq!(state2, false);

                let command3 = test_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_heating,
                    &tx_controllino_to_heating,
                );
                assert_eq!(
                    command3,
                    Some(InternalCommand::SwitchOn(AquariumDevice::Heater))
                );
                let state3 = test_receive_information_for_datalogger(&rx_datalogger_from_heating);
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
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_gpio,
                    rx_heating_from_gpio,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_datalogger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging,
                    &mut sql_interface,
                    &mut heating_stats_transfer,
                );

                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

                let actuation_count = *mutex_device_scheduler_heating.lock().unwrap();

                // check amount of actuations (after quitting)
                assert_eq!(actuation_count, 4);
            });
            println!("* [Messaging] checking if messaging can stop and start heating succeeded.");
        });
    }

    #[cfg(target_os = "linux")]
    // Test case runs feed control and triggers inhibition by sending stop message.
    // After verification that feed is stopped,
    // test case sends start message and verifies if feed control
    // resumes operation.
    // Afterwards test case triggers execution of a specific feed profile
    // by sending "execute" message.
    #[test]
    pub fn test_messaging_stops_starts_execute_feed() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_feed.toml".to_string());
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

        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let mut sql_interface_feed = SqlInterfaceFeed::new(sql_interface.get_connection());

        let mut msg_start = Message::new();
        msg_start.domain = messaging_domains::FEED;
        msg_start.command = messaging_commands::START;

        let mut msg_stop = Message::new();
        msg_stop.domain = messaging_domains::FEED;
        msg_stop.command = messaging_commands::STOP;

        let mut msg_execute = Message::new();
        msg_execute.domain = messaging_domains::FEED;
        msg_execute.command = messaging_commands::EXEC;
        msg_execute.command_param1 = 1;

        // empty the feed schedule
        match SqlInterface::truncate_table(&mut sql_interface, "feedschedule".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let time_stamp_feed_schedule_entry =
            match SqlInterface::get_current_timestamp_offset_seconds(&mut sql_interface.conn, 4) {
                Ok(c) => c,
                Err(e) => {
                    panic!(
                        "test_feed_schedule_execution: Could not calculate timestamp for stimuli: {e:?}"
                    );
                }
            };

        // manipulation of database: inserting feed schedule entries with future timestamp
        match sql_interface_feed.insert_feed_schedule_entry(
            time_stamp_feed_schedule_entry,
            1,
            "testOneTime".to_string(),
            false,
            false,
        ) {
            Ok(()) => { /* do nothing */ }
            Err(e) => {
                panic!(
                    "test_feed_schedule_execution: Could not insert stimuli into database: {e:?}"
                );
            }
        };

        // create test objects
        let mut feed = Feed::new(config.feed, sql_interface_feed);
        let mut messaging = Messaging::new(config.messaging);

        let (tx_feed_to_controllino, _rx_controllino_from_feed) = mpsc::channel();
        let (_tx_controllino_to_feed, rx_feed_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_feed, rx_feed_from_signal_handler) = mpsc::channel();
        let (tx_feed_to_signal_handler, rx_signal_handler_from_feed) = mpsc::channel();
        let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler) = mpsc::channel();
        let (tx_messaging_to_refill, _rx_refill_from_messaging) = mpsc::channel();
        let (tx_messaging_to_ventilation, _rx_ventilation_from_messaging) = mpsc::channel();
        let (tx_messaging_to_heating, _rx_heating_from_messaging) = mpsc::channel();
        let (tx_messaging_to_feed, rx_feed_from_messaging) = mpsc::channel();
        let (tx_messaging_to_balling, _rx_balling_from_messaging) = mpsc::channel();
        let (tx_messaging_to_monitors, _rx_monitors_from_messaging) = mpsc::channel();

        let mut mock_food_injection = MockFoodInjection::new();

        scope(|scope| {
            let mutex_device_scheduler_feed_test_object = Arc::new(Mutex::new(0));
            let mutex_device_scheduler_feed_test_environment =
                mutex_device_scheduler_feed_test_object.clone();

            // thread IPC messaging - this scope controls execution of the test case
            scope.spawn(move || {
                // wait for 1 second to make sure test object can receive message
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millisec);
                }

                // sending IPC message requesting stop of feed control
                let _ = mq_stimuli.send(0, &msg_stop.create_message_buf());

                // wait for 10 seconds. If test object does not follow stop request,
                // then food injection will happen in this period.
                for _ in 0..100 {
                    spin_sleeper.sleep(sleep_duration_100_millisec);
                }

                let actuation_count_1 =
                    *mutex_device_scheduler_feed_test_environment.lock().unwrap();

                // check amount of actuations (after stop request)
                assert_eq!(actuation_count_1, 0);

                // sending IPC message requesting restart of feed control
                let _ = mq_stimuli.send(0, &msg_start.create_message_buf());

                // wait for 10 seconds. If test object follows start request,
                // then food injections will happen in this period.
                for _ in 0..100 {
                    spin_sleeper.sleep(sleep_duration_100_millisec);
                }

                // get count of feed profiles executed
                let actuation_count_2 =
                    *mutex_device_scheduler_feed_test_environment.lock().unwrap();

                // check amount of actuations (after start request)
                assert_eq!(actuation_count_2, 1);

                // sending IPC message requesting execution of feed profile
                let _ = mq_stimuli.send(0, &msg_execute.create_message_buf());

                // wait for 2 seconds. If test object follows execute request,
                // then food injection will happen in this period.
                for _ in 0..20 {
                    spin_sleeper.sleep(sleep_duration_100_millisec);
                }

                // get count of feed profiles executed
                let actuation_count_3 =
                    *mutex_device_scheduler_feed_test_environment.lock().unwrap();

                // check amount of actuations (after execute request)
                assert_eq!(actuation_count_3, 2);

                // requesting feed control to quit
                let _ = tx_signal_handler_to_feed.send(InternalCommand::Quit);
                rx_signal_handler_from_feed.recv().unwrap();
                let _ = tx_signal_handler_to_feed.send(InternalCommand::Terminate);

                // requesting messaging to quit
                let _ = tx_signal_handler_to_messaging.send(InternalCommand::Quit);
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
                feed.execute(
                    mutex_device_scheduler_feed_test_object.clone(),
                    tx_feed_to_controllino,
                    rx_feed_from_controllino,
                    tx_feed_to_signal_handler,
                    rx_feed_from_signal_handler,
                    rx_feed_from_messaging,
                    &mut mock_food_injection,
                );
                println!("MockFoodInjection:\n{}", mock_food_injection);

                // check if the right feed profiles have been executed
                assert_eq!(mock_food_injection.profile_ids_executed.len(), 2);
                assert_eq!(mock_food_injection.profile_ids_executed[0], 1);
                assert_eq!(mock_food_injection.profile_ids_executed[1], 1);
            });

            println!("* [Messaging] checking if messaging can stop, start feed control and execute feed profiles succeeded.");
        });
    }

    pub fn run_main_cycle_with_stimuli_refill(
        iterations: u32,
        rx_gpio_from_refill: &Receiver<InternalCommand>,
        tx_gpio_to_refill: &Sender<f32>,
        tank_level_position_stimuli: f32,
        tank_level_invalid_stimuli: f32,
    ) {
        for i in 0..iterations {
            println!(
                "run_main_cycle_with_stimuli_refill: running loop iteration #{}",
                i
            );
            for _ in 0..2 {
                match rx_gpio_from_refill.recv() {
                    Ok(c) => match c {
                        InternalCommand::RequestSignal(AquariumSignal::TankLevelSwitchPosition) => {
                            println!("run_main_cycle_with_stimuli_refill: Answering request for tank level switch position with {}", tank_level_position_stimuli);
                            let _ = tx_gpio_to_refill.send(tank_level_position_stimuli);
                        }
                        InternalCommand::RequestSignal(AquariumSignal::TankLevelSwitchInvalid) => {
                            println!("run_main_cycle_with_stimuli_refill: Answering request for tank level switch invalid with {}", tank_level_invalid_stimuli);
                            let _ = tx_gpio_to_refill.send(tank_level_invalid_stimuli);
                        }
                        _ => {
                            println!("run_main_cycle_with_stimuli_refill: Ignoring inapplicable request {}", c);
                        }
                    },
                    Err(e) => {
                        panic!("run_main_cycle_with_stimuli_refill: Could not read from channel: {e:?}");
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    // Test case runs refill control and triggers inhibition by sending stop message.
    // After verification that refill is stopped,
    // test case sends start message and verifies if feed control
    // resumes operation.
    #[test]
    pub fn test_messaging_stops_starts_refill() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_refill_quaternary.toml".to_string());
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

        let sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_refill = SqlInterfaceRefill::new(sql_interface.get_connection());

        let mut msg_start = Message::new();
        msg_start.domain = messaging_domains::REFILL;
        msg_start.command = messaging_commands::START;

        let mut msg_stop = Message::new();
        msg_stop.domain = messaging_domains::REFILL;
        msg_stop.command = messaging_commands::STOP;

        // create test objects
        let mut refill = Refill::new(config.refill, sql_interface_refill);
        let mut messaging = Messaging::new(config.messaging);

        let (tx_refill_to_controllino, _rx_controllino_from_refill) = mpsc::channel();
        let (_tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (tx_refill_to_datalogger, _rx_datalogger_from_refill) = mpsc::channel();
        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) = mpsc::channel();
        let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler) = mpsc::channel();
        let (tx_messaging_to_refill, rx_refill_from_messaging) = mpsc::channel();
        let (tx_messaging_to_ventilation, _rx_ventilation_from_messaging) = mpsc::channel();
        let (tx_messaging_to_heating, _rx_heating_from_messaging) = mpsc::channel();
        let (tx_messaging_to_feed, _rx_feed_from_messaging) = mpsc::channel();
        let (tx_messaging_to_balling, _rx_balling_from_messaging) = mpsc::channel();
        let (tx_messaging_to_monitors, _rx_monitors_from_messaging) = mpsc::channel();

        let tx_refill_to_schedule_check_for_test_case_finish = tx_refill_to_schedule_check.clone();

        let mut mock_water_injection = RefillTestExecutionVoid {};

        scope(|scope| {
            let mutex_device_scheduler_refill_test_object = Arc::new(Mutex::new(0));
            let mutex_device_scheduler_refill_test_environment =
                mutex_device_scheduler_refill_test_object.clone();

            // thread for mock schedule check: runs until it receives Quit command
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            // thread for IPC messaging - this scope controls execution of the test case
            scope.spawn(move || {
                // Phase 1: Normal refill cycle, sending stop command afterwards
                // stimulate tank level switch high and valid
                run_main_cycle_with_stimuli_refill(
                    4,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    1.0,
                    0.0,
                );
                // stimulate tank level switch low and valid
                run_main_cycle_with_stimuli_refill(
                    6,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    0.0,
                    0.0,
                );
                run_main_cycle_with_stimuli_refill(
                    1,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    1.0,
                    0.0,
                );
                let actuation_count_1 = *mutex_device_scheduler_refill_test_environment
                    .lock()
                    .unwrap();
                println!(
                    "test_messaging_stops_starts_refill: actuation_count_1 = {}",
                    actuation_count_1
                );
                // amount of actuations (before stop request): one refill operation executed
                assert_eq!(actuation_count_1, 1);

                // sending IPC message requesting stop of refill control
                let _ = mq_stimuli.send(0, &msg_stop.create_message_buf());
                // stimulate tank level switch high and valid
                run_main_cycle_with_stimuli_refill(
                    4,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    1.0,
                    0.0,
                );

                // Phase 2: Stimulating low level and checking if inhibition works
                // stimulate tank level switch low and valid
                run_main_cycle_with_stimuli_refill(
                    6,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    0.0,
                    0.0,
                );
                let actuation_count_2 = *mutex_device_scheduler_refill_test_environment
                    .lock()
                    .unwrap();
                // amount of actuations (after stop request): no further refill operation executed
                println!(
                    "test_messaging_stops_starts_refill: actuation_count_2 = {}",
                    actuation_count_2
                );
                assert_eq!(actuation_count_2, 1);

                // stimulate tank level switch high and valid
                run_main_cycle_with_stimuli_refill(
                    1,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    1.0,
                    0.0,
                );
                // sending IPC message requesting (re-)start of refill control
                let _ = mq_stimuli.send(0, &msg_start.create_message_buf());
                // stimulate tank level switch high and valid
                run_main_cycle_with_stimuli_refill(
                    2,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    1.0,
                    0.0,
                );

                // Phase 3: Normal refill cycle after having sent restart command
                // stimulate tank level switch low and valid
                run_main_cycle_with_stimuli_refill(
                    6,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    0.0,
                    0.0,
                );
                run_main_cycle_with_stimuli_refill(
                    2,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    1.0,
                    0.0,
                );
                let actuation_count_3 = *mutex_device_scheduler_refill_test_environment
                    .lock()
                    .unwrap();
                // amount of actuations (after restart request): one more refill operation executed
                println!(
                    "test_messaging_stops_starts_refill: actuation_count_3 = {}",
                    actuation_count_3
                );
                assert_eq!(actuation_count_3, 2);

                // requesting refill control to quit
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);

                // requesting messaging to quit
                let _ = tx_signal_handler_to_messaging.send(InternalCommand::Quit);
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
                refill.execute(
                    mutex_device_scheduler_refill_test_object.clone(),
                    tx_refill_to_gpio,
                    rx_refill_from_gpio,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_datalogger,
                    rx_refill_from_messaging,
                    &mut mock_water_injection,
                );

                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            });

            println!(
                "* [Messaging] checking if messaging can stop and start refill control succeeded."
            );
        });
    }

    pub struct MockWaterInjectionAlwaysErr;

    impl WaterInjectionTrait for MockWaterInjectionAlwaysErr {
        fn inject_water(
            &mut self,
            _tx_to_controllino: &Sender<InternalCommand>,
            _rx_from_controllino: &Receiver<bool>,
            _tx_to_gpio: &Sender<InternalCommand>,
            _rx_from_gpio: &Receiver<f32>,
            _rx_from_signal_handler: &Receiver<InternalCommand>,
            _tx_to_datalogger: &Sender<bool>,
            _tank_level_switch_signals: &mut TankLevelSwitchSignals,
            refill_errors: &mut RefillErrors,
            _config: &RefillConfig,
            _sql_interface_refill: &mut SqlInterfaceRefill,
        ) -> bool {
            // do nothing
            println!("Refill: execute_refill *MockWaterInjectionAlwaysErr* has been called.");
            // trigger error condition for allowing test case to reset it
            refill_errors.error_switch_stuck_low = true;
            false // simulate that no Quit command has been received
        }
    }

    #[cfg(target_os = "linux")]
    // Test case runs refill control and triggers reset of error by sending "ResetAllErrors" message.
    #[test]
    pub fn test_messaging_resets_refill_errors() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_refill_quaternary.toml".to_string());
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

        let sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_refill = SqlInterfaceRefill::new(sql_interface.get_connection());

        let mut msg_reset = Message::new();
        msg_reset.domain = messaging_domains::REFILL;
        msg_reset.command = messaging_commands::RESET;

        // create test objects
        let mut refill = Refill::new(config.refill, sql_interface_refill);
        let mut messaging = Messaging::new(config.messaging);

        let (tx_refill_to_controllino, _rx_controllino_from_refill) = mpsc::channel();
        let (_tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (tx_refill_to_datalogger, _rx_datalogger_from_refill) = mpsc::channel();
        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) = mpsc::channel();
        let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler) = mpsc::channel();
        let (tx_messaging_to_refill, rx_refill_from_messaging) = mpsc::channel();
        let (tx_messaging_to_ventilation, _rx_ventilation_from_messaging) = mpsc::channel();
        let (tx_messaging_to_heating, _rx_heating_from_messaging) = mpsc::channel();
        let (tx_messaging_to_feed, _rx_feed_from_messaging) = mpsc::channel();
        let (tx_messaging_to_balling, _rx_balling_from_messaging) = mpsc::channel();
        let (tx_messaging_to_monitors, _rx_monitors_from_messaging) = mpsc::channel();

        let tx_refill_to_schedule_check_for_test_case_finish = tx_refill_to_schedule_check.clone();

        let mut mock_water_injection = MockWaterInjectionAlwaysErr {};

        scope(|scope| {
            let mutex_device_scheduler_refill_test_object = Arc::new(Mutex::new(0));
            let mutex_device_scheduler_refill_test_environment =
                mutex_device_scheduler_refill_test_object.clone();

            // thread for mock schedule check: runs until it receives Quit command
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            // thread for IPC messaging - this scope controls execution of the test case
            scope.spawn(move || {
                // Phase 1: Normal refill cycle, sending stop command afterwards
                // stimulate tank level switch high and valid
                run_main_cycle_with_stimuli_refill(
                    4,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    1.0,
                    0.0,
                );
                // stimulate tank level switch low and valid
                run_main_cycle_with_stimuli_refill(
                    6,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    0.0,
                    0.0,
                );
                run_main_cycle_with_stimuli_refill(
                    1,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    1.0,
                    0.0,
                );
                let actuation_count_1 = *mutex_device_scheduler_refill_test_environment
                    .lock()
                    .unwrap();
                println!(
                    "test_messaging_stops_starts_refill: actuation_count_1 = {}",
                    actuation_count_1
                );
                // amount of actuations (before stop request): one refill operation executed
                assert_eq!(actuation_count_1, 1);

                // sending IPC message requesting error reset of refill control
                let _ = mq_stimuli.send(0, &msg_reset.create_message_buf());
                // stimulate tank level switch high and valid
                run_main_cycle_with_stimuli_refill(
                    4,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    1.0,
                    0.0,
                );

                // Phase 2: Stimulating low level and checking if inhibition works
                // stimulate tank level switch low and valid
                run_main_cycle_with_stimuli_refill(
                    6,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    0.0,
                    0.0,
                );
                run_main_cycle_with_stimuli_refill(
                    1,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    1.0,
                    0.0,
                );

                let actuation_count_2 = *mutex_device_scheduler_refill_test_environment
                    .lock()
                    .unwrap();
                // amount of actuations (after stop request): no further refill operation executed
                println!(
                    "test_messaging_stops_starts_refill: actuation_count_2 = {}",
                    actuation_count_2
                );
                assert_eq!(actuation_count_2, 2);

                // requesting refill control to quit
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);

                // requesting messaging to quit
                let _ = tx_signal_handler_to_messaging.send(InternalCommand::Quit);
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
                refill.execute(
                    mutex_device_scheduler_refill_test_object.clone(),
                    tx_refill_to_gpio,
                    rx_refill_from_gpio,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_datalogger,
                    rx_refill_from_messaging,
                    &mut mock_water_injection,
                );

                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            });

            println!(
                "* [Messaging] checking if messaging can reset refill control error succeeded."
            );
        });
    }
}
