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

use crate::dispatch::messaging_channels::MessagingChannels;
use crate::dispatch::messaging_config::MessagingConfig;
use crate::dispatch::messaging_error::MessagingError;
use crate::launch::execution_config::ExecutionConfig;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::channel_content::InternalCommand;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;
use std::time::Duration;

cfg_if::cfg_if! {
    if #[cfg(target_os = "linux")] {
        #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
        use log::info;

        use log::{error, warn};
        use posixmq::PosixMq;
        #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
        use nix::unistd::gettid;

        use crate::dispatch::messaging_domain::MessagingDomain;
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
    pub mq: Option<PosixMq>,

    // maximum length of messages in the queue
    pub mq_max_msg_len: usize,

    /// an inhibition flag to avoid flooding the log file with repeated messages about having received an inapplicable command
    pub lock_warn_inapplicable_command_signal_handler: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    pub lock_error_channel_receive_termination: bool,

    /// configuration data indicating which threads are started
    pub execution_config: ExecutionConfig,
}
#[cfg(not(target_os = "linux"))]
pub struct Messaging {
    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    pub lock_error_channel_receive_termination: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about having received an inapplicable command
    pub lock_warn_inapplicable_command_signal_handler: bool,
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
    ///
    /// [impl->swreq~dispatch_process_posix_messages~1]
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

        mq.set_nonblocking(true)
            .map_err(|e| MessagingError::PosixMessageQueueAttributesError {
                location: module_path!().to_string(),
                source: e,
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
        #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let async_mq_opt: Option<tokio::io::unix::AsyncFd<PosixMq>>;

        async_mq_opt = match self.mq.take() {
            Some(mq) => match tokio::io::unix::AsyncFd::new(mq) {
                Ok(async_mq) => Some(async_mq),
                Err(e) => {
                    error!(
                        target: module_path!(),
                        "Failed to create AsyncFd for accessing message queue ({e:?})"
                    );
                    None
                }
            },
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
                                                    #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
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
                                                        #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
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
        )
        .await;
    }
}
#[cfg(not(target_os = "linux"))]
impl Messaging {
    /// Mock constructor for `Messaging` on non-Linux platforms.
    ///
    /// This is a stub implementation used during testing or development on operating
    /// systems other than Linux (e.g., macOS, Windows) where POSIX message queues are
    /// not supported or used. It initializes the struct with default diagnostic flags.
    pub fn new(
        _config: MessagingConfig,
        _execution_config: ExecutionConfig,
    ) -> Result<Messaging, MessagingError> {
        Ok(Messaging {
            lock_error_channel_receive_termination: false,
            lock_warn_inapplicable_command_signal_handler: false,
        })
    }

    /// Mock execution loop for `Messaging` on non-Linux platforms.
    ///
    /// This is a stub implementation that simulates the loop without accessing a physical
    /// POSIX message queue. It sends `NoOp` commands to other tasks, and then loops
    /// periodically to check for termination requests from the signal handler.
    pub async fn execute(&mut self, messaging_channels: &mut MessagingChannels) {
        messaging_channels
            .tx_messaging_to_monitors
            .send(InternalCommand::NoOp)
            .unwrap();
        messaging_channels
            .tx_messaging_to_feed
            .send(InternalCommand::NoOp)
            .unwrap();
        messaging_channels
            .tx_messaging_to_balling
            .send(InternalCommand::NoOp)
            .unwrap();
        messaging_channels
            .tx_messaging_to_heating
            .send(InternalCommand::NoOp)
            .unwrap();

        let sleep_duration = Duration::from_millis(10);

        loop {
            _ = tokio::time::sleep(Duration::from_millis(100)).await;
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
        )
        .await;
    }
}
