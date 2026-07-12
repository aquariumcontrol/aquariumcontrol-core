/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Implements the central hardware actuation dispatcher thread.
//!
//! This module provides the `RelayManager` struct, which runs as a dedicated, long-running
//! thread. Its primary responsibility is to act as the sole arbiter of hardware access,
//! receiving high-level actuation commands from various control modules (e.g., `Refill`,
//! `Heating`, `Ventilation`) and translating them into actions performed by a specific
//! hardware backend.
//!
//! ## Key Components
//!
//! - **`RelayManager` Struct**: The core state machine for the thread. It holds the
//!   configuration, a large number of stateful boolean "lock" flags for log
//!   suppression, and the execution configuration.
//!
//! - **`RelayActuationTrait`**: A critical abstraction that decouples the `RelayManager`
//!   from the concrete hardware implementation. This allows the manager to operate
//!   identically whether it's controlling a `Controllino` over a serial port, direct
//!   `GPIO` pins, or a `simulator` during testing.
//!
//! - **`execute()` Method**: The main thread loop. It continuously polls the channels
//!   from all client modules, processes incoming commands, and handles graceful shutdown.
//!
//! - **`actuate_with_retries()` Method**: A key robustness feature. When an actuation
//!   command fails, this function implements a retry mechanism, attempting the command
//!   multiple times before reporting a final failure. It can also trigger a buffer flush
//!   on the hardware if a communication error is detected.
//!
//! ## Design and Architecture
//!
//! This module is designed for robustness, testability, and clear separation of concerns.
//!
//! - **Centralized Dispatcher**: The `RelayManager` is the single point of entry for all
//!   hardware commands. This prevents race conditions and ensures that device actuations
//!   are handled in a serialized, predictable manner.
//!
//! - **Hardware Abstraction**: The use of the `RelayActuationTrait` is a cornerstone of
//!   the application's design. It allows the entire system to be tested without any
//!   physical hardware by simply plugging in a mock `ActuateSimulator` implementation.
//!
//! - **Robust Error Handling (Retries & Log Flood Prevention)**: The module is designed
//!   for long-running, unattended operation.
//!   - The `actuate_with_retries` logic ensures that transient hardware glitches do not
//!     immediately cause a system failure.
//!   - The extensive use of `lock_...` flags prevents a single, persistent issue (like a
//!     disconnected channel) from filling up disk space with thousands of identical
//!     log messages. The error is logged once, and the lock is engaged until the
//!     condition is resolved.
//!
//! - **Heartbeat Management**: For hardware that requires a periodic "keep-alive" signal
//!   (like the Controllino), the `RelayManager` automatically sends a heartbeat at the
//!   configured interval to maintain the connection.

#[cfg(not(test))] // only required in real-life execution
use log::error;
#[cfg(all(not(test), target_os = "linux"))] // only required in real-life execution
use log::info;

#[cfg(not(test))] // only required in real-life execution
use log::warn;

#[cfg(feature = "debug_relay_manager")]
use log::debug;

// reduce terminal output when testing
#[cfg(all(not(test), target_os = "linux"))] // only required in real-life execution
use nix::unistd::gettid;

use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::launch::execution_config::ExecutionConfig;
use crate::relays::relay_error::RelayError;
use crate::relays::relay_manager_channels::RelayManagerChannels;
use crate::relays::relay_manager_config::RelayManagerConfig;
use crate::utilities::channel_content::InternalCommand;
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use std::time::{Duration, Instant};

/// Trait for the execution of relay actuation on either simulator or real hardware.
/// This trait allows running the main control with a mock implementation for testing.
pub trait RelayActuationTrait {
    /// Actuates a device based on the given command.
    ///
    /// This is the primary method for controlling a hardware device. Implementations
    /// should translate the high-level `InternalCommand` into a hardware-specific
    /// action (e.g., sending a serial command, setting a GPIO pin).
    ///
    /// # Arguments
    /// * `internal_command` - The command specifying the device and action to perform.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) on successful actuation.
    ///
    /// # Errors
    /// Returns a `RelayError` variant if the actuation fails. This could be due to
    /// various reasons depending on the implementation, such as
    /// - A failure to write to a serial port (`WriteError`).
    /// - A corrupt response from the hardware (`IncorrectChecksum`).
    /// - An attempt to use an unsupported command for the given hardware.
    fn actuate(&mut self, internal_command: &InternalCommand) -> Result<(), RelayError>;

    /// Gets the required interval for the heartbeat signal, if any.
    ///
    /// Some hardware (like the Controllino) requires a periodic "keep-alive" signal
    /// to ensure the connection is active. This method defines that interval.
    ///
    /// # Returns
    /// - `Some(u64)`: The heartbeat interval in seconds.
    /// - `None`: If no heartbeat is required for this actuator (e.g., for direct
    ///   GPIO control or a simulator).
    fn get_heartbeat_interval_seconds(&self) -> Option<u64>;

    /// Sends a heartbeat signal to the hardware.
    ///
    /// This method should be called periodically according to the interval specified
    /// by `get_heartbeat_interval_seconds` to maintain the hardware connection.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the heartbeat was sent successfully.
    ///
    /// # Errors
    /// Returns a `RelayError` if sending the heartbeat signal fails, for instance,
    /// due to a disconnected serial port.
    fn heartbeat(&mut self) -> Result<(), RelayError>;

    /// Flushes the communication buffer of the hardware interface.
    ///
    /// This method is a recovery mechanism, typically used after a communication
    /// error (like an incorrect checksum) to clear any lingering, corrupt data
    /// from the input/output buffers and resynchronize communication.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the buffer was flushed successfully.
    ///
    /// # Errors
    /// Returns a `RelayError` if the flush operation fails, which could indicate
    /// a deeper issue with the serial port or hardware driver.
    fn flush_buffer(&mut self) -> Result<(), RelayError>;
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation of the relay manager.
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

    /// information about which threads have been started
    execution_config: ExecutionConfig,
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
    /// * `execution_config` - An execution configuration instance.
    pub fn new(config: RelayManagerConfig, execution_config: ExecutionConfig) -> RelayManager {
        RelayManager {
            config,
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
            execution_config,
        }
    }

    /// Attempts to actuate a given command, with retries on failure.
    ///
    /// This helper function encapsulates the retry logic for actuator commands. It
    /// will attempt the actuation at least once, and if it fails, it will retry
    /// up to the number of times specified by `self.config.actuation_retries`.
    /// If a failure is due to an `IncorrectChecksum`, it will attempt to flush the
    /// buffer before retrying.
    ///
    /// # Arguments
    /// * `actuator` - A mutable reference to an object implementing `RelayActuationTrait`.
    /// * `command` - The `InternalCommand` to be executed.
    /// * `actuation_name` - A descriptive name for the action (e.g., "refill pump") for logging.
    /// * `spin_sleeper` - A `SpinSleeper` to pause between retries.
    /// * `sleep_duration` - The `Duration` to wait between retry attempts.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the actuation succeeds, possibly after one or more retries.
    ///
    /// # Errors
    /// Returns `Err(RelayError::ActuationFailed)` if all attempts (the initial one plus all
    /// retries) fail. This error is a wrapper that contains detailed context, including
    /// - The name of the actuation that failed.
    /// - The total number of attempts made.
    /// - The specific error from the *last* attempt, providing the root cause of the final failure.
    /// * `"heater" sleep_duration` - A parameter representing `"heater" sleep_duration`.
    async fn actuate_with_retries(
        &mut self,
        actuator: &mut impl RelayActuationTrait,
        command: &InternalCommand,
        actuation_name: &str, // Name for logging, e.g., "refill pump", "heater"
        sleep_duration: Duration,
    ) -> Result<(), RelayError> {
        let mut attempts = 0;
        let max_attempts = 1 + self.config.actuation_retries; // 1 initial attempt + `actuation_retries` retries
        let mut error_to_be_reported_opt = None;

        while attempts < max_attempts {
            let actuate_result = tokio::task::block_in_place(|| actuator.actuate(command));
            match actuate_result {
                Ok(()) => {
                    if attempts > 0 {
                        #[cfg(not(test))] // only required in real-life execution
                        #[cfg(not(tarpaulin_include))]
                        warn!(
                            target: module_path!(),
                            "{actuation_name}: actuation succeeded after {attempts} retries."
                        );
                        #[cfg(test)] // to avoid warning when compiling for test
                        if actuation_name == "not existing name" {
                            println!("Actuation with strange name: {}", actuation_name);
                        }
                    }
                    return Ok(());
                }
                Err(e) => {
                    if matches!(e, RelayError::IncorrectChecksum(_)) {
                        let flush_result = tokio::task::block_in_place(|| actuator.flush_buffer());
                        if let Err(f) = flush_result {
                            // if we cannot flush the buffer, mission abort
                            error_to_be_reported_opt = Some(f);
                            break;
                        }
                    }

                    // Log error only after the final attempt
                    if attempts == max_attempts - 1 {
                        error_to_be_reported_opt = Some(e);
                        break;
                    } else {
                        // Wait a little before retrying
                        tokio::time::sleep(sleep_duration).await;
                    }
                    attempts += 1;
                }
            }
        }
        if let Some(error_to_be_reported) = error_to_be_reported_opt {
            Err(RelayError::ActuationFailed {
                location: module_path!().to_string(),
                actuation_name: actuation_name.to_string(),
                last_attempt_number: attempts + 1,
                max_attempt_allowed: max_attempts,
                source: Box::new(error_to_be_reported),
            })
        } else {
            Ok(())
        }
    }

    /// Executes the main control loop for the Relay Manager.
    ///
    /// This function runs continuously, acting as the central dispatcher for actuation commands
    /// received from various control modules (Refill, Heating, etc.). It uses a helper function,
    /// `process_actuation_request`, to handle the logic for each module's channel pair,
    /// keeping the main loop clean and readable.
    ///
    /// The loop forwards commands to the provided `actuator` and handles graceful shutdown
    /// when a `Quit` command is received from the signal handler. It also manages to send
    /// periodic heartbeats to the hardware if required.
    ///
    /// # Arguments
    /// * `relay_manager_channels` - A mutable reference to the struct containing all `mpsc` channels necessary for
    ///   communication with other control threads.
    /// * `actuator` - A mutable reference to an object implementing the `RelayActuationTrait`,
    ///   which handles the actual hardware interaction or simulation.
    pub async fn execute(
        &mut self,
        relay_manager_channels: &mut RelayManagerChannels,
        actuator: &mut impl RelayActuationTrait,
    ) {
        #[cfg(all(target_os = "linux", not(test)))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let mut last_heartbeat_time = Instant::now();
        let sleep_duration_between_retries =
            Duration::from_millis(self.config.pause_between_retries_millis);
        let sleep_duration_main_cycle = Duration::from_millis(10);

        loop {
            // Check for quit command first
            if self
                .process_external_request(
                    &mut relay_manager_channels.rx_relay_manager_from_signal_handler,
                    None,
                )
                .0
            {
                #[cfg(feature = "debug_relay_manager")]
                debug!(target: module_path!(), "received QUIT command");
                break;
            }

            if self.execution_config.refill {
                (
                    self.lock_error_channel_receive_refill,
                    self.lock_error_channel_send_refill,
                ) = self
                    .process_actuation_request(
                        (
                            &mut relay_manager_channels.rx_relay_manager_from_refill,
                            &mut relay_manager_channels.tx_relay_manager_to_refill,
                        ),
                        (
                            self.lock_error_channel_receive_refill,
                            self.lock_error_channel_send_refill,
                        ),
                        "refill",
                        actuator,
                        sleep_duration_between_retries,
                    )
                    .await;
            }

            if self.execution_config.heating {
                (
                    self.lock_error_channel_receive_heating,
                    self.lock_error_channel_send_heating,
                ) = self
                    .process_actuation_request(
                        (
                            &mut relay_manager_channels.rx_relay_manager_from_heating,
                            &mut relay_manager_channels.tx_relay_manager_to_heating,
                        ),
                        (
                            self.lock_error_channel_receive_heating,
                            self.lock_error_channel_send_heating,
                        ),
                        "heating",
                        actuator,
                        sleep_duration_between_retries,
                    )
                    .await;
            }

            if self.execution_config.ventilation {
                (
                    self.lock_error_channel_receive_ventilation,
                    self.lock_error_channel_send_ventilation,
                ) = self
                    .process_actuation_request(
                        (
                            &mut relay_manager_channels.rx_relay_manager_from_ventilation,
                            &mut relay_manager_channels.tx_relay_manager_to_ventilation,
                        ),
                        (
                            self.lock_error_channel_receive_ventilation,
                            self.lock_error_channel_send_ventilation,
                        ),
                        "ventilation",
                        actuator,
                        sleep_duration_between_retries,
                    )
                    .await;
            }

            if self.execution_config.balling {
                (
                    self.lock_error_channel_receive_balling,
                    self.lock_error_channel_send_balling,
                ) = self
                    .process_actuation_request(
                        (
                            &mut relay_manager_channels.rx_relay_manager_from_balling,
                            &mut relay_manager_channels.tx_relay_manager_to_balling,
                        ),
                        (
                            self.lock_error_channel_receive_balling,
                            self.lock_error_channel_send_balling,
                        ),
                        "Balling dosing",
                        actuator,
                        sleep_duration_between_retries,
                    )
                    .await;
            }

            if self.execution_config.feed {
                (
                    self.lock_error_channel_receive_feed,
                    self.lock_error_channel_send_feed,
                ) = self
                    .process_actuation_request(
                        (
                            &mut relay_manager_channels.rx_relay_manager_from_feed,
                            &mut relay_manager_channels.tx_relay_manager_to_feed,
                        ),
                        (
                            self.lock_error_channel_receive_feed,
                            self.lock_error_channel_send_feed,
                        ),
                        "feed",
                        actuator,
                        sleep_duration_between_retries,
                    )
                    .await;
            }

            // *** send heartbeat ***
            if let Some(interval_secs) = actuator.get_heartbeat_interval_seconds() {
                if Instant::now().duration_since(last_heartbeat_time).as_secs() >= interval_secs {
                    let hb_result = tokio::task::block_in_place(|| actuator.heartbeat());
                    if let Err(e) = hb_result {
                        log_error_chain(
                            module_path!(),
                            "Execution of heartbeat for relay control failed.",
                            e,
                        );
                    }
                    last_heartbeat_time = Instant::now();
                }
            }

            // have a minimum of sleep time in between checking channels
            tokio::time::sleep(sleep_duration_main_cycle).await;
        }

        #[cfg(feature = "debug_relay_manager")]
        debug!(
            target: module_path!(),
            "exited loop, sending back confirmation to signal handler"
        );

        // Application received request to terminate. That is why the loop was left.
        // Answer to the thread which sent the request for termination, so that shutdown can proceed further.
        if let Err(_e) = relay_manager_channels
            .tx_relay_manager_to_signal_handler
            .send(true)
        {
            #[cfg(not(test))] // only required in real-life execution
            #[cfg(not(tarpaulin_include))]
            error!(target: module_path!(), "Sending confirmation to signal handler failed. ({_e:?})");
        };
    }

    /// Helper function to process an actuation request from a single channel pair.
    ///
    /// This function encapsulates the logic for receiving a command, actuating it with retries,
    /// and sending a confirmation response. It is used to de-duplicate the main `execute` loop.
    ///
    /// It takes the current state of the log-suppression flags (`lock_receive`, `lock_send`)
    /// by value and returns their updated state in a tuple. This pattern is used to work
    /// around Rust's borrowing rules, as the flags are fields on `RelayManager` and cannot
    /// be mutably borrowed multiple times in the `execute` loop.
    ///
    /// # Arguments
    /// * `rx` - The channel for receiving `InternalCommand`s from a control module.
    /// * `tx` - The channel for sending a boolean confirmation back to the control module.
    /// * `lock_receive` - The current state of the receive-error lock flag.
    /// * `lock_send` - The current state of the send error lock flag.
    /// * `actuation_name` - A descriptive name for the action (e.g., "refill") for logging.
    /// * `actuator` - A mutable reference to the hardware actuator.
    /// * `spin_sleeper` - A `SpinSleeper` to pause between retries.
    /// * `sleep_duration_between_retries` - The `Duration` to wait between retry attempts.
    ///
    /// # Returns
    /// A tuple `(bool, bool)` containing the updated state of the `lock_receive` and
    /// `lock_send` flags, respectively.
    /// * `channels` - A parameter representing `channels`.
    async fn process_actuation_request(
        &mut self,
        channels: (&mut AquaReceiver<InternalCommand>, &mut AquaSender<bool>),
        locks: (bool, bool),
        actuation_name: &str,
        actuator: &mut impl RelayActuationTrait,
        sleep_duration_between_retries: Duration,
    ) -> (bool, bool) {
        let (rx, tx) = channels;
        let (mut lock_receive, mut lock_send) = locks;
        match rx.try_recv() {
            Ok(_command) => {
                lock_receive = false;
                if self.config.active {
                    if let Err(_actuation_error) = self
                        .actuate_with_retries(
                            actuator,
                            &_command,
                            actuation_name,
                            sleep_duration_between_retries,
                        )
                        .await
                    {
                        #[cfg(not(test))] // only required in real-life execution
                        #[cfg(not(tarpaulin_include))]
                        log_error_chain(
                            module_path!(),
                            &format!("Actuation of {actuation_name} failed."),
                            _actuation_error,
                        );
                    }
                }
                if let Err(_e) = tx.send(true) {
                    if !lock_send {
                        #[cfg(not(test))] // only required in real-life execution
                        #[cfg(not(tarpaulin_include))]
                        error!(
                            target: module_path!(),
                            "responding to {actuation_name} control ({_e:?})",
                        );
                        lock_send = true;
                    }
                } else {
                    lock_send = false;
                }
            }
            Err(e) => match e {
                #[cfg(feature = "debug_channels")]
                AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                AquaChannelError::Empty => {
                    lock_receive = false;
                }
                AquaChannelError::Disconnected => {
                    if !lock_receive {
                        #[cfg(not(test))] // only required in real-life execution
                        #[cfg(not(tarpaulin_include))]
                        error!(
                            "{}: error when trying to receive command from {actuation_name} control ({e:?})",
                            module_path!(),
                        );
                        lock_receive = true;
                    }
                }
            },
        }
        (lock_receive, lock_send)
    }
}

#[cfg(test)]
pub mod tests {
    use crate::launch::channels::Channels;
    use crate::launch::execution_config::ExecutionConfig;
    use crate::relays::relay_manager::{RelayActuationTrait, RelayError, RelayManager};
    use crate::relays::relay_manager_channels::RelayManagerChannels;
    use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
    use crate::utilities::config::{read_config_file, ConfigData};
    use spin_sleep::SpinSleeper;
    use std::thread::scope;
    use std::time::Duration;

    /// Helper function to execute the relay manager task inside a test Tokio runtime.
    ///
    /// # Arguments
    /// * `relay_manager` - A mutable reference to the `RelayManager` instance.
    /// * `relay_manager_channels` - A mutable reference to the `RelayManagerChannels` instance.
    /// * `actuator` - A mutable reference to the relay actuator instance.
    fn execute_blocking(
        relay_manager: &mut RelayManager,
        relay_manager_channels: &mut RelayManagerChannels,
        actuator: &mut impl RelayActuationTrait,
    ) {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                // blocking execution used for testing
                relay_manager
                    .execute(relay_manager_channels, actuator)
                    .await;
            });
    }

    // Contains mock trait implementation for unit testing purposes.
    pub struct ActuateVoid {
        // execution counter for testing purposes
        execution_counter: u64,

        // heartbeat counter for testing purposes
        heartbeat_counter: u64,

        // flush counter for testing purposes
        flush_counter: u64,

        // injection of error: if not None, this value is used one time by actuate and then set to None
        onetime_error_result: Option<RelayError>,

        // injection of error: if not None, this value is used one time by actuate and then set to None
        permanent_error_result: Option<RelayError>,
    }

    impl RelayActuationTrait for ActuateVoid {
        // Mock function for testing
        /// Performs the `actuate` operation.
        ///
        /// # Arguments
        /// * `internal_command` - A parameter representing `internal_command`.
        fn actuate(&mut self, internal_command: &InternalCommand) -> Result<(), RelayError> {
            println!(
                "Received {} - increasing execution counter (currently at {})",
                internal_command, self.execution_counter
            );
            self.execution_counter += 1;
            if self.onetime_error_result.is_some() {
                // call will replace error value with "None" - inducing Ok-result for next call
                // of actuate
                Err(self.onetime_error_result.take().unwrap())
            } else {
                if self.permanent_error_result.is_some() {
                    // Workaround: RelayError does not implement the Clone trait.
                    // WriteError is the only error variant used by the test cases.
                    Err(RelayError::WriteError(module_path!().to_string()))
                } else {
                    Ok(())
                }
            }
        }

        /// Retrieves the heartbeat interval seconds.
        fn get_heartbeat_interval_seconds(&self) -> Option<u64> {
            Some(1)
        }

        /// Performs the `heartbeat` operation.
        fn heartbeat(&mut self) -> Result<(), RelayError> {
            self.heartbeat_counter += 1;
            Ok(())
        }

        /// Performs the `flush_buffer` operation.
        fn flush_buffer(&mut self) -> Result<(), RelayError> {
            self.flush_counter += 1;
            Ok(())
        }
    }

    impl ActuateVoid {
        // provides the number of executions to the test case for assertion
        /// Retrieves the execution count.
        pub fn get_execution_count(&self) -> u64 {
            self.execution_counter
        }
    }

    #[test]
    // Test case tests the channel communication for the .execute function of relay manager.
    // It also checks the heartbeat communication.
    pub fn test_relay_manager_channel_communication() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_simulator.toml".to_string()).unwrap();

        let spin_sleeper = SpinSleeper::default();
        let sleep_duration = Duration::from_secs(2);

        let mut relay_manager = RelayManager::new(config.relay_manager, ExecutionConfig::default());

        let mut channels = Channels::new_for_test();

        scope(|scope| {
            // thread for test environment
            scope.spawn(move || {
                let _ = channels.refill.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::RefillPump));
                match channels.refill.rx_refill_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for refill thread: {e:?}");
                    }
                }
                let _ = channels.heating.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::Heater));
                match channels.heating.rx_heating_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for heating thread: {e:?}");
                    }
                }
                let _ = channels.ventilation.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::Ventilation));
                match channels.ventilation.rx_ventilation_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for ventilation thread: {e:?}");
                    }
                }
                let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump1));
                match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump2));
                match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump3));
                match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump4));
                match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::Feeder));
                match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::MainPump1));
                match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::MainPump2));
                match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::AuxPump1));
                match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::AuxPump2));
                match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = channels.refill.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::RefillPump));
                match channels.refill.rx_refill_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for refill thread: {e:?}");
                    }
                }
                let _ = channels.heating.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::Heater));
                match channels.heating.rx_heating_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for heating thread: {e:?}");
                    }
                }
                let _ = channels.ventilation.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::Ventilation));
                match channels.ventilation.rx_ventilation_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for ventilation thread: {e:?}");
                    }
                }
                let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump1));
                match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump2));
                match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump3));
                match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump4));
                match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                    }
                }
                let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::Feeder));
                match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::MainPump1));
                match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::MainPump2));
                match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::AuxPump1));
                match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }
                let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::AuxPump2));
                match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                    }
                }

                spin_sleeper.sleep(sleep_duration); // wait for two seconds to test the heartbeat

                let _ = channels.signal_handler.tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
                channels.signal_handler.rx_signal_handler_from_relay_manager.blocking_recv().unwrap();
            });

            // thread for the test object
            scope.spawn(move || {
                let mut mock_actuator = ActuateVoid {
                    execution_counter: 0,
                    heartbeat_counter: 0,
                    flush_counter: 0,
                    onetime_error_result: None,
                    permanent_error_result: None,
                };

                execute_blocking(
                    &mut relay_manager,
                    &mut channels.relay_manager,
                    &mut mock_actuator,
                );

                // Test object thread has ended.
                assert_eq!(mock_actuator.get_execution_count(), 24); //Assertion checks the number of calls to the actuator.
                assert_eq!(mock_actuator.heartbeat_counter, 2); // Assertion checks the number of heartbeats.
            });
        });
    }
    #[test]
    // Test case tests if the relay manager is calling the flush functionality when the
    // actuator returns the IncorrectChecksum error.
    pub fn test_relay_manager_flush() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_simulator.toml".to_string()).unwrap();

        let mut relay_manager = RelayManager::new(config.relay_manager, ExecutionConfig::default());

        let mut channels = Channels::new_for_test();

        scope(|scope| {
            // thread for test environment
            scope.spawn(move || {
                let _ = channels.refill.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::RefillPump));
                match channels.refill.rx_refill_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for refill thread: {e:?}");
                    }
                }
                let _ = channels.heating.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::Heater));
                match channels.heating.rx_heating_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for heating thread: {e:?}");
                    }
                }

                let _ = channels.signal_handler.tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
                channels.signal_handler.rx_signal_handler_from_relay_manager.blocking_recv().unwrap();
            });

            // thread for the test object
            scope.spawn(move || {
                let mut mock_actuator = ActuateVoid {
                    execution_counter: 0,
                    heartbeat_counter: 0,
                    flush_counter: 0,
                    onetime_error_result: Some(RelayError::IncorrectChecksum(
                        module_path!().to_string(),
                    )),
                    permanent_error_result: None,
                };

                execute_blocking(
                    &mut relay_manager,
                    &mut channels.relay_manager,
                    &mut mock_actuator,
                );

                // Test object thread has ended.
                assert_eq!(mock_actuator.get_execution_count(), 3); //Assertion checks the number of calls to the actuator.
                assert_eq!(mock_actuator.flush_counter, 1); // Assertion checks the number of buffer flush operations.
            });
        });
    }

    #[test]
    // Test case tests if the relay manager is calling the actuate function multiple times
    // in case actuator returns an error.
    pub fn test_relay_manager_repeat() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_simulator.toml".to_string()).unwrap();

        let target_repeat_times = config.relay_manager.actuation_retries;

        let mut relay_manager = RelayManager::new(config.relay_manager, ExecutionConfig::default());

        let mut channels = Channels::new_for_test();

        scope(|scope| {
            // thread for test environment
            scope.spawn(move || {
                let _ = channels.refill.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::RefillPump));
                match channels.refill.rx_refill_from_relay_manager.blocking_recv() {
                    Ok(c) => {
                        assert!(c);
                    },
                    Err(e) => {
                        panic!("test_relay_manager_channel_communication: error when receiving answer for refill thread: {e:?}");
                    }
                }

                let _ = channels.signal_handler.tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
                channels.signal_handler.rx_signal_handler_from_relay_manager.blocking_recv().unwrap();
            });

            // thread for the test object
            scope.spawn(move || {
                let mut mock_actuator = ActuateVoid {
                    execution_counter: 0,
                    heartbeat_counter: 0,
                    flush_counter: 0,
                    onetime_error_result: None,
                    permanent_error_result: Some(RelayError::WriteError(
                        module_path!().to_string(),
                    )),
                };

                execute_blocking(
                    &mut relay_manager,
                    &mut channels.relay_manager,
                    &mut mock_actuator,
                );

                // Test object thread has ended.

                // Assertion checks the number of calls to the actuator.
                // Relay manager shall repeat in case of failure as configured by user.
                assert_eq!(mock_actuator.get_execution_count(), target_repeat_times + 1);
            });
        });
    }
}
