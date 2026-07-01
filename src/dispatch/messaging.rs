/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Provides the Inter-Process Communication (IPC) bridge for external control.
//!
//! This module is responsible for listening to a POSIX message queue for commands sent
//! from external processes (e.g., the command-line utility `aquarium_control` or the web server).
//! It acts as the central point of dispatcher, translating these external messages into internal
//! commands and forwarding them to the appropriate application threads
//! (like `Refill`, `Heating`, `Feed`, etc.).
//!
//! This entire module is conditionally compiled and is only available on `target_os = "linux"`.
//!
//! ## Key Components
//!
//! - **`Messaging` Struct**: The core component that holds the POSIX message queue connection
//!   and runs the main dispatch loop.
//!
//! - **`Message` Struct**: A simple data structure representing the raw command received
//!   from the message queue containing a `domain`, `command`, and parameters.
//!
//! - **`execute()` Method**: The main loop of the `Messaging` thread. It performs the following:
//!   1.  Listens for incoming byte messages on the POSIX queue.
//!   2.  Parses the raw bytes into a `Message` struct.
//!   3.  Interprets the message's `domain` (e.g., `Refill`, `Heating`) and `command`.
//!   4.  Sends the corresponding `InternalCommand` to the target thread via its channel.
//!   5.  Listens for `Quit` and `Terminate` signals from the `signal_handler` for graceful shutdown.
//!
//! ## Design and Architecture
//!
//! The `Messaging` module serves as a crucial bridge between the running application and
//! the outside world, allowing for dynamic control without restarting the service.
//!
//! - **Decoupling**: It decouples the internal workings of the application from the
//!   specifics of the external control mechanism.
//! - **Centralized Dispatch**: All external commands flow through this single point,
//!   making the control flow easy to understand and debug.
//! - **Robustness**: It includes logic to handle unknown domains or commands gracefully
//!   by logging a warning and ignoring the message.
//!
//! ### Example Message Flow
//!
//! 1.  An external script sends a byte message to the `/aquarium_control` POSIX message queue.
//!     The message contains bytes representing: `domain=HEATING`, `command=STOP`.
//! 2.  The `Messaging::execute()` loop receives these bytes.
//! 3.  It parses the bytes into a `Message` struct.
//! 4.  It matches the `domain` to `MessagingDomain::Heating` and the `command` to `InternalCommand::Stop`.
//! 5.  It sends `InternalCommand::Stop` to the `Heating` thread's channel.
//! 6.  The `Heating` thread receives the command and stops its operation.
//!
//! The mermaid diagram in the `Messaging` struct documentation provides a visual overview
//! of the communication paths.

#[cfg(feature = "debug_messaging")]
use log::debug;

cfg_if::cfg_if! {
    if #[cfg(target_os = "linux")] {
        #[cfg(not(test))] // only required in real-life execution
        use log::info;

        use log::{error, warn};
        use posixmq::PosixMq;
        use std::time::Duration;
        #[cfg(not(test))] // only required in real-life execution
        use nix::unistd::gettid;

        use crate::utilities::channel_content::InternalCommand;
        use crate::dispatch::messaging_channels::MessagingChannels;
        use crate::dispatch::messaging_error::MessagingError;
        use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
        use crate::utilities::wait_for_termination::WaitForTerminationTrait;
        use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
        use crate::dispatch::messaging_domain::MessagingDomain;
        use crate::dispatch::messaging_config::MessagingConfig;
        use crate::launch::execution_config::ExecutionConfig;
        use crate::dispatch::ipc_message::Message;
        use std::io::ErrorKind;
    }
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
#[cfg(target_os = "linux")]
pub struct Messaging {
    // Message queue
    mq: Option<PosixMq>,

    // maximum length of messages in the queue
    mq_max_msg_len: usize,

    /// an inhibition flag to avoid flooding the log file with repeated messages about having received an inapplicable command
    pub lock_warn_inapplicable_command_signal_handler: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    pub lock_error_channel_receive_termination: bool,

    /// configuration data indicating which threads are started
    execution_config: ExecutionConfig,
}

#[cfg(target_os = "linux")]
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
    /// * `execution_config` - Configuration data indicating which threads are started.
    ///
    /// # Returns
    /// A `Result` containing a new, initialized `Messaging` struct on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - It fails to open the message queue with the specified `mq_filename`. This will
    ///   be a `MessagingError::PosixMessageQueueOpeningError`.
    /// - It fails to retrieve the attributes (like max message length) of the opened
    ///   message queue. This will be a `MessagingError::PosixMessageQueueAttributesError`.
    pub fn new(
        config: MessagingConfig,
        execution_config: ExecutionConfig,
    ) -> Result<Messaging, MessagingError> {
        let mq_file_name_clone_for_error = config.mq_filename.clone();

        let mq = PosixMq::open(&config.mq_filename).map_err(|e| {
            MessagingError::PosixMessageQueueOpeningError {
                location: module_path!().to_string(),
                mq_name: mq_file_name_clone_for_error,
                source: e,
            }
        })?;

        mq.set_nonblocking(true).map_err(|e| {
            MessagingError::PosixMessageQueueAttributesError {
                location: module_path!().to_string(),
                source: e,
            }
        })?;

        // identify maximum length of message queue
        let mq_max_msg_len = match mq.attributes() {
            Ok(c) => c.max_msg_len,
            Err(e) => {
                return Err(MessagingError::PosixMessageQueueAttributesError {
                    location: module_path!().to_string(),
                    source: e,
                });
            }
        };

        Ok(Messaging {
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
            mq: Some(mq),
            mq_max_msg_len,
            execution_config,
        })
    }

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
    /// * `messaging_channels` - A mutable reference to a struct containing all channels for communication to other threads.
    ///
    /// # Panics
    /// This function will panic if:
    /// - It encounters a fatal error while receiving from the POSIX message queue (other than a timeout).
    /// - It fails to receive the `Terminate` command from the signal handler during shutdown.
    pub async fn execute(&mut self, messaging_channels: &mut MessagingChannels) {
        #[cfg(not(test))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let mut async_mq_opt: Option<tokio::io::unix::AsyncFd<PosixMq>>;

        async_mq_opt = match self.mq.take() {
            Some(mq) => {
                match tokio::io::unix::AsyncFd::new(mq) {
                    Ok(async_mq) => Some(async_mq),
                    Err(e) => {
                        error!(
                            target: module_path!(),
                            "Failed to create AsyncFd for accessing message queue ({e:?})"
                        );
                        None
                    }
                }
            }
            None => {
                error!(
                    target: module_path!(),
                    "Message queue already taken or not initialized"
                );
                None
            }
        };

        let mut msgbuf = vec![0; self.mq_max_msg_len];

        let sleep_duration = Duration::from_millis(10);

        loop {
            match &async_mq_opt {
                Some(async_mq) => {
                    tokio::select! {
                        // 1. Wait for message queue to become readable
                        readable_result = async_mq.readable() => {
                            if let Ok(mut guard) = readable_result {
                                loop {
                                    match async_mq.get_ref().recv(&mut msgbuf) {
                                        Ok((_r1, _r2)) => {
                                            if !msgbuf.is_empty() {
                                                if let Ok(msg) = Message::from_bytes(&msgbuf) {
                                                    #[cfg(not(test))] // only required in real-life execution
                                                    #[cfg(not(tarpaulin_include))]
                                                    info!(
                                                        target: module_path!(),
                                                        "received message: domain={}, cmd={}, param1={}, param2={}",
                                                        msg.domain,
                                                        msg.command,
                                                        msg.command_param1,
                                                        msg.command_param2
                                                    );

                                                    // interpret the domain in a message
                                                    let domain = MessagingDomain::from(msg.domain);
                                                    if domain == MessagingDomain::Unknown {
                                                        warn!(
                                                            target: module_path!(),
                                                            "ignoring message for unknown domain {}",
                                                            msg.domain
                                                        );
                                                    }

                                                    // interpret command in a message
                                                    let command = InternalCommand::from_msg(&msg);
                                                    if command == InternalCommand::Unknown {
                                                        warn!(
                                                            target: module_path!(),
                                                            "Messaging: ignoring message with unknown command {}",
                                                            msg.command
                                                        );
                                                    }

                                                    // prepare and execute communication to domain
                                                    if (domain != MessagingDomain::Unknown)
                                                        && (command != InternalCommand::Unknown)
                                                        && (domain.is_domain_thread_executed(&self.execution_config))
                                                    {
                                                        #[cfg(not(test))] // only required in real-life execution
                                                        #[cfg(not(tarpaulin_include))]
                                                        info!(
                                                            target: module_path!(),
                                                            "forwarding message to domain {}",
                                                            domain
                                                        );

                                                        match messaging_channels
                                                            .send_command_to_domain(command, domain.clone())
                                                        {
                                                            Ok(_) => { /* do nothing */ }
                                                            Err(e) => {
                                                                error!(
                                                                    target: module_path!(),
                                                                    "channel communication to domain {} failed ({e:?})",
                                                                    domain
                                                                );
                                                            }
                                                        }
                                                    }
                                                } else {
                                                    error!(
                                                        target: module_path!(),
                                                        "message has incorrect length ({}). ignoring it.",
                                                        msgbuf.len()
                                                    );
                                                }
                                            } else {
                                                error!(
                                                    target: module_path!(),
                                                    "message has incorrect length ({}). ignoring it.",
                                                    msgbuf.len()
                                                );
                                            }
                                        }
                                        Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                                            guard.clear_ready();
                                            break;
                                        }
                                        Err(e) => {
                                            error!(
                                                target: module_path!(),
                                                "Error occurred when reading POSIX message ({e:?})"
                                            );
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        // 2. Or wake up periodically (every 100ms) to check for signals
                        _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                    }
                }
                None => {
                    // Getting access to message queue was not successful.
                    // Instead of exiting directly, we wait for quit command.
                    // Wake up periodically (every 100ms) to check for signals
                    _ = tokio::time::sleep(Duration::from_millis(100))
                }
            };
            let (quit_command_received, _, _) = self.process_external_request(
                &mut messaging_channels.rx_messaging_from_signal_handler,
                None,
            );
            if quit_command_received {
                #[cfg(feature = "debug_messaging")]
                debug!(
                    target: module_path!(),
                    "received QUIT command from signal handler"
                );

                break;
            }
        }

        messaging_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads.
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination_async(
            &mut messaging_channels.rx_messaging_from_signal_handler,
            sleep_duration,
            module_path!(),
        ).await;
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")]
pub mod tests {
    use posixmq::PosixMq;
    use spin_sleep::SpinSleeper;
    use std::{thread, time::Duration};

    use crate::dispatch::ipc_message::Message;
    use crate::dispatch::messaging::Messaging;
    use crate::dispatch::messaging_channels::MessagingChannels;
    use crate::dispatch::messaging_domain::MessagingDomain;
    use crate::launch::channels::AquaSender;
    use crate::launch::channels::Channels;
    use crate::launch::execution_config::ExecutionConfig;
    use crate::mocks::mock_message_command_receiver::tests::mock_message_command_receiver;
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::config::{read_config_file, ConfigData};
    use crate::utilities::signal_handler_channels::SignalHandlerChannels;

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
    // This function will panic if it fails to open the specified POSIX message queue,
    // as this is considered a critical failure for the test setup.
    fn open_posixmq_test_file(filename: &str) -> PosixMq {
        match PosixMq::open(filename) {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "{}: failure to open message queue for test using {}, error={e:?}",
                    module_path!(),
                    filename
                );
            }
        }
    }

    // Creates a new thread to run the `Messaging` module's `execute` loop.
    //
    // This helper function is used in test environments on Linux to simulate the `Messaging`
    // module running in its own dedicated thread. It sets up the necessary channel
    // connections for communication with various components (like `Refill`, `Heating`, `Watchdog`, etc.).
    //
    // # Arguments
    // * `messaging` - The `Messaging` instance to be executed in the new thread. It is moved into the thread.
    // * `messaging_channels` - Struct containing the channels for messaging
    //
    // # Returns
    // A `thread::JoinHandle<()>` which can be used to wait for the spawned thread to complete.
    //
    // # Panics
    // This function will panic if the new thread cannot be spawned.
    fn create_test_object(
        mut messaging: Messaging,
        mut messaging_channels: MessagingChannels,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async {
                        messaging.execute(&mut messaging_channels).await;
                    });
            })
            .unwrap()
    }

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
    fn create_test_environment(
        msg_commands: Vec<Message>,
        mut tx_environment_to_object: AquaSender<InternalCommand>,
        signal_handler_channels: SignalHandlerChannels,
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
                    let _ = mq_stimuli.send(0, &msg_command.to_bytes());

                    spin_sleeper.sleep(sleep_duration_100_millis);
                }

                // requesting mock control to quit
                let _ = tx_environment_to_object.send(InternalCommand::Quit);

                let mut tx_signal_handler_to_messaging = signal_handler_channels
                    .tx_signal_handler_to_messaging_opt
                    .unwrap();
                let mut rx_signal_handler_from_messaging = signal_handler_channels
                    .rx_signal_handler_from_messaging_opt
                    .unwrap();

                // requesting messaging to quit
                let _ = tx_signal_handler_to_messaging.send(InternalCommand::Quit);

                // waiting for confirmation from messaging
                let _ = rx_signal_handler_from_messaging.recv();

                let _ = tx_signal_handler_to_messaging.send(InternalCommand::Terminate);
            })
            .unwrap()
    }

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
        let spin_sleeper = SpinSleeper::default();
        spin_sleeper.sleep(Duration::from_millis(100));

        for command in &commands_received {
            println!("assert_stop_start_sequence received command: {command:?}");
        }
        // assert the number of commands sent/received are identical
        assert_eq!(2, commands_received.len());

        // assert content of last message sent from test environment:
        assert_eq!(InternalCommand::Start, commands_received.pop().unwrap());

        // assert the content of the first message sent from the test environment:
        assert_eq!(InternalCommand::Stop, commands_received.pop().unwrap());
    }

    // Test case checks if messages for ventilation control are triggering the corresponding channel communication
    #[test]
    pub fn test_messaging_ventilation() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.messaging.mq_filename = "/aquarium_control.5UXp".to_string();
        let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

        let mut msg_start = Message::new();
        msg_start.domain = MessagingDomain::Ventilation.into();
        msg_start.command = InternalCommand::Start.to_numeric();

        let mut msg_stop = Message::new();
        msg_stop.domain = MessagingDomain::Ventilation.into();
        msg_stop.command = InternalCommand::Stop.to_numeric();

        let messaging = Messaging::new(config.messaging, ExecutionConfig::default()).unwrap();

        let channels = Channels::new_for_test();

        let tx_test_environment_to_ventilation =
            channels.messaging.tx_messaging_to_ventilation.clone();

        let join_handle_mock_ventilation = thread::Builder::new()
            .name("mock_ventilation".to_string())
            .spawn(move || {
                assert_stop_start_sequence(mock_message_command_receiver(
                    channels
                        .ventilation
                        .rx_ventilation_from_messaging_opt
                        .unwrap(),
                ));
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            vec![msg_stop, msg_start], // first stop, then start ventilation
            tx_test_environment_to_ventilation,
            channels.signal_handler,
            mq_filename_for_test_environment,
        );

        // thread for the test object
        let join_handle_test_object = create_test_object(messaging, channels.messaging);

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

    // Test case checks if messages for heating control are triggering the corresponding channel communication
    #[test]
    pub fn test_messaging_heating() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.messaging.mq_filename = "/aquarium_control.6UXp".to_string();
        let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

        let mut msg_start = Message::new();
        msg_start.domain = MessagingDomain::Heating.into();
        msg_start.command = InternalCommand::Start.to_numeric();

        let mut msg_stop = Message::new();
        msg_stop.domain = MessagingDomain::Heating.into();
        msg_stop.command = InternalCommand::Stop.to_numeric();

        let messaging = Messaging::new(config.messaging, ExecutionConfig::default()).unwrap();

        let channels = Channels::new_for_test();

        let tx_test_environment_to_heating = channels.messaging.tx_messaging_to_heating.clone();

        let join_handle_mock_heating = thread::Builder::new()
            .name("mock_heating".to_string())
            .spawn(move || {
                assert_stop_start_sequence(mock_message_command_receiver(
                    channels.heating.rx_heating_from_messaging_opt.unwrap(),
                ));
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            vec![msg_stop, msg_start], // first stop, then start heating
            tx_test_environment_to_heating,
            channels.signal_handler,
            mq_filename_for_test_environment,
        );

        // thread for the test object
        let join_handle_test_object = create_test_object(messaging, channels.messaging);

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

    // Test case checks if messages for refill control are triggering the corresponding channel communication
    #[test]
    pub fn test_messaging_refill() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.messaging.mq_filename = "/aquarium_control.7UXp".to_string();
        let mq_file_name_for_test_environment = config.messaging.mq_filename.clone();

        let mut msg_reset = Message::new();
        msg_reset.domain = MessagingDomain::Refill.into();
        msg_reset.command = InternalCommand::ResetAllErrors.to_numeric();

        let mut msg_start = Message::new();
        msg_start.domain = MessagingDomain::Refill.into();
        msg_start.command = InternalCommand::Start.to_numeric();

        let mut msg_stop = Message::new();
        msg_stop.domain = MessagingDomain::Refill.into();
        msg_stop.command = InternalCommand::Stop.to_numeric();

        let messaging = Messaging::new(config.messaging, ExecutionConfig::default()).unwrap();

        let channels = Channels::new_for_test();

        let tx_test_environment_to_refill = channels.messaging.tx_messaging_to_refill.clone();

        let join_handle_mock_refill = thread::Builder::new()
            .name("mock_refill".to_string())
            .spawn(move || {
                let mut commands_received = mock_message_command_receiver(
                    channels.refill.rx_refill_from_messaging_opt.unwrap(),
                );

                // assert the number of commands sent/received are identical
                assert_eq!(3, commands_received.len());

                // assert the content of the last message sent from test environment:
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
            channels.signal_handler,
            mq_file_name_for_test_environment,
        );

        // thread for the test object
        let join_handle_test_object = create_test_object(messaging, channels.messaging);

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

    // Test case checks if messages for feed control are triggering the corresponding channel communication
    #[test]
    pub fn test_messaging_feed() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.messaging.mq_filename = "/aquarium_control.8UXp".to_string();
        let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

        let mut msg_exec = Message::new();
        msg_exec.domain = MessagingDomain::Feed.into();
        msg_exec.command = InternalCommand::Execute(13).to_numeric();
        msg_exec.command_param1 = 13;

        let mut msg_start = Message::new();
        msg_start.domain = MessagingDomain::Feed.into();
        msg_start.command = InternalCommand::Start.to_numeric();

        let mut msg_stop = Message::new();
        msg_stop.domain = MessagingDomain::Feed.into();
        msg_stop.command = InternalCommand::Stop.to_numeric();

        let messaging = Messaging::new(config.messaging, ExecutionConfig::default()).unwrap();

        let channels = Channels::new_for_test();

        let tx_test_environment_to_feed = channels.messaging.tx_messaging_to_feed.clone();

        let join_handle_mock_feed = thread::Builder::new()
            .name("mock_feed".to_string())
            .spawn(move || {
                let mut commands_received = mock_message_command_receiver(
                    channels.feed.rx_feed_from_messaging_opt.unwrap(),
                );

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
            channels.signal_handler,
            mq_filename_for_test_environment,
        );

        // thread for the test object
        let join_handle_test_object = create_test_object(messaging, channels.messaging);

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

    // Test case checks if messages for Balling control are triggering the corresponding channel communication
    #[test]
    pub fn test_messaging_balling() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.messaging.mq_filename = "/aquarium_control.9UXp".to_string();
        let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

        open_posixmq_test_file(&config.messaging.mq_filename);

        let mut msg_start = Message::new();
        msg_start.domain = MessagingDomain::Balling.into();
        msg_start.command = InternalCommand::Start.to_numeric();

        let mut msg_stop = Message::new();
        msg_stop.domain = MessagingDomain::Balling.into();
        msg_stop.command = InternalCommand::Stop.to_numeric();

        let messaging = Messaging::new(config.messaging, ExecutionConfig::default()).unwrap();

        let channels = Channels::new_for_test();

        let tx_test_environment_to_balling = channels.messaging.tx_messaging_to_balling.clone();

        let join_handle_mock_balling = thread::Builder::new()
            .name("mock_balling".to_string())
            .spawn(move || {
                assert_stop_start_sequence(mock_message_command_receiver(
                    channels.balling.rx_balling_from_messaging_opt.unwrap(),
                ));
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            vec![msg_stop, msg_start], // first stop, then start Balling dosing control
            tx_test_environment_to_balling,
            channels.signal_handler,
            mq_filename_for_test_environment,
        );

        // thread for the test object
        let join_handle_test_object = create_test_object(messaging, channels.messaging);

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

    // Test case checks if messages for Watchdog control are triggering the corresponding channel communication
    #[test]
    pub fn test_messaging_watchdog() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.messaging.mq_filename = "/aquarium_control.9UXp".to_string();
        let mq_filename_for_test_environment = config.messaging.mq_filename.clone();

        let mut msg_start = Message::new();
        msg_start.domain = MessagingDomain::Watchdog.into();
        msg_start.command = InternalCommand::Start.to_numeric();

        let mut msg_stop = Message::new();
        msg_stop.domain = MessagingDomain::Watchdog.into();
        msg_stop.command = InternalCommand::Stop.to_numeric();

        let messaging = Messaging::new(config.messaging, ExecutionConfig::default()).unwrap();

        let channels = Channels::new_for_test();

        let tx_test_environment_to_watchdog = channels.messaging.tx_messaging_to_watchdog.clone();

        let join_handle_mock_watchdog = thread::Builder::new()
            .name("mock_watchdog".to_string())
            .spawn(move || {
                assert_stop_start_sequence(mock_message_command_receiver(
                    channels.watchdog.rx_watchdog_from_messaging_opt.unwrap(),
                ));
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            vec![msg_stop, msg_start], // first stop, then start Watchdog
            tx_test_environment_to_watchdog,
            channels.signal_handler,
            mq_filename_for_test_environment,
        );

        // thread for the test object
        let join_handle_test_object = create_test_object(messaging, channels.messaging);

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
