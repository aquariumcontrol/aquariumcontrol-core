/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(not(test))]
use log::error;
#[cfg(all(not(test), target_os = "linux"))]
use log::info;
use std::fmt;

#[cfg(not(test))]
use log::warn;

#[cfg(feature = "debug_relay_manager")]
use log::debug;

#[cfg(all(not(test), target_os = "linux"))]
use nix::unistd::gettid;

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use crate::relays::relay_error::RelayError;
use crate::utilities::channel_content::InternalCommand;
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

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

    /// The number of retries in case write operation to actuator fails
    /// Applicable only to Controllino, because GPIO actuation does not execute a write operation
    pub actuation_retries: u64,

    /// Pause time in milliseconds between retries
    pub pause_between_retries_millis: u64,
}

impl fmt::Display for RelayManagerConfig {
    /// Formats the `RelayManagerConfig` for display purposes.
    ///
    /// This implementation provides a clean, multi-line string representation of the
    /// relay manager's configuration, which is ideal for logging at startup or
    /// for debugging purposes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Relay Manager Configuration:")?;
        writeln!(f, "  Active: {}", self.active)?;
        writeln!(f, "  Use Simulator: {}", self.use_simulator)?;
        writeln!(f, "  Use GPIO: {}", self.use_gpio)?;
        writeln!(f, "  Actuation Retries: {}", self.actuation_retries)?;
        write!(
            f,
            "  Pause Between Retries: {} ms",
            self.pause_between_retries_millis
        )
    }
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
    fn actuate_with_retries(
        &mut self,
        actuator: &mut impl RelayActuationTrait,
        command: &InternalCommand,
        actuation_name: &str, // Name for logging, e.g., "refill pump", "heater"
        spin_sleeper: &mut SpinSleeper,
        sleep_duration: Duration,
    ) -> Result<(), RelayError> {
        let mut attempts = 0;
        let max_attempts = 1 + self.config.actuation_retries; // 1 initial attempt + `actuation_retries` retries
        let mut error_to_be_reported = RelayError::InitialValue(module_path!().to_string());

        while attempts < max_attempts {
            match actuator.actuate(command) {
                Ok(()) => {
                    if attempts > 0 {
                        #[cfg(not(test))]
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
                        if let Err(f) = actuator.flush_buffer() {
                            // if we cannot flush the buffer, mission abort
                            error_to_be_reported = f;
                            break;
                        }
                    }

                    // Log error only after the final attempt
                    if attempts == max_attempts - 1 {
                        error_to_be_reported = e;
                        break;
                    } else {
                        // Wait a little before retrying
                        spin_sleeper.sleep(sleep_duration);
                    }
                    attempts += 1;
                }
            }
        }
        Err(RelayError::ActuationFailed {
            location: module_path!().to_string(),
            actuation_name: actuation_name.to_string(),
            last_attempt_number: attempts + 1,
            max_attempt_allowed: max_attempts,
            source: Box::new(error_to_be_reported),
        })
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
    /// * `relay_manager_channels` - A struct containing all `mpsc` channels necessary for
    ///   communication with other control threads.
    /// * `actuator` - A mutable reference to an object implementing the `RelayActuationTrait`,
    ///   which handles the actual hardware interaction or simulation.
    pub fn execute(
        &mut self,
        relay_manager_channels: RelayManagerChannels,
        actuator: &mut impl RelayActuationTrait,
    ) {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let mut last_heartbeat_time = Instant::now();
        let mut spin_sleeper = SpinSleeper::default();
        let sleep_duration_between_retries =
            Duration::from_millis(self.config.pause_between_retries_millis);
        let sleep_duration_main_cycle = Duration::from_millis(10);

        loop {
            // Check for quit command first
            if self
                .process_external_request(
                    &relay_manager_channels.rx_relay_manager_from_signal_handler,
                    None,
                )
                .0
            {
                #[cfg(feature = "debug_relay_manager")]
                debug!(target: module_path!(), "received QUIT command");
                break;
            }

            (
                self.lock_error_channel_receive_refill,
                self.lock_error_channel_send_refill,
            ) = self.process_actuation_request(
                (
                    &relay_manager_channels.rx_relay_manager_from_refill,
                    &relay_manager_channels.tx_relay_manager_to_refill,
                ),
                (
                    self.lock_error_channel_receive_refill,
                    self.lock_error_channel_send_refill,
                ),
                "refill",
                actuator,
                &mut spin_sleeper,
                sleep_duration_between_retries,
            );

            (
                self.lock_error_channel_receive_heating,
                self.lock_error_channel_send_heating,
            ) = self.process_actuation_request(
                (
                    &relay_manager_channels.rx_relay_manager_from_heating,
                    &relay_manager_channels.tx_relay_manager_to_heating,
                ),
                (
                    self.lock_error_channel_receive_heating,
                    self.lock_error_channel_send_heating,
                ),
                "heating",
                actuator,
                &mut spin_sleeper,
                sleep_duration_between_retries,
            );

            (
                self.lock_error_channel_receive_ventilation,
                self.lock_error_channel_send_ventilation,
            ) = self.process_actuation_request(
                (
                    &relay_manager_channels.rx_relay_manager_from_ventilation,
                    &relay_manager_channels.tx_relay_manager_to_ventilation,
                ),
                (
                    self.lock_error_channel_receive_ventilation,
                    self.lock_error_channel_send_ventilation,
                ),
                "ventilation",
                actuator,
                &mut spin_sleeper,
                sleep_duration_between_retries,
            );

            (
                self.lock_error_channel_receive_balling,
                self.lock_error_channel_send_balling,
            ) = self.process_actuation_request(
                (
                    &relay_manager_channels.rx_relay_manager_from_balling,
                    &relay_manager_channels.tx_relay_manager_to_balling,
                ),
                (
                    self.lock_error_channel_receive_balling,
                    self.lock_error_channel_send_balling,
                ),
                "Balling dosing",
                actuator,
                &mut spin_sleeper,
                sleep_duration_between_retries,
            );

            (
                self.lock_error_channel_receive_feed,
                self.lock_error_channel_send_feed,
            ) = self.process_actuation_request(
                (
                    &relay_manager_channels.rx_relay_manager_from_feed,
                    &relay_manager_channels.tx_relay_manager_to_feed,
                ),
                (
                    self.lock_error_channel_receive_feed,
                    self.lock_error_channel_send_feed,
                ),
                "feed",
                actuator,
                &mut spin_sleeper,
                sleep_duration_between_retries,
            );

            // *** send heartbeat ***
            if let Some(interval_secs) = actuator.get_heartbeat_interval_seconds() {
                if Instant::now().duration_since(last_heartbeat_time).as_secs() >= interval_secs {
                    if let Err(e) = actuator.heartbeat() {
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
            spin_sleeper.sleep(sleep_duration_main_cycle);
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
            #[cfg(not(test))]
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
    fn process_actuation_request(
        &mut self,
        channels: (&Receiver<InternalCommand>, &Sender<bool>),
        locks: (bool, bool),
        actuation_name: &str,
        actuator: &mut impl RelayActuationTrait,
        spin_sleeper: &mut SpinSleeper,
        sleep_duration_between_retries: Duration,
    ) -> (bool, bool) {
        let (rx, tx) = channels;
        let (mut lock_receive, mut lock_send) = locks;
        match rx.try_recv() {
            Ok(_command) => {
                lock_receive = false;
                if self.config.active {
                    if let Err(_actuation_error) = self.actuate_with_retries(
                        actuator,
                        &_command,
                        actuation_name,
                        spin_sleeper,
                        sleep_duration_between_retries,
                    ) {
                        #[cfg(not(test))]
                        log_error_chain(
                            module_path!(),
                            &format!("Actuation of {actuation_name} failed."),
                            _actuation_error,
                        );
                    }
                }
                if let Err(_e) = tx.send(true) {
                    if !lock_send {
                        #[cfg(not(test))]
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
                TryRecvError::Empty => {
                    lock_receive = false;
                }
                TryRecvError::Disconnected => {
                    if !lock_receive {
                        #[cfg(not(test))]
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
    use crate::relays::relay_manager::{
        RelayActuationTrait, RelayError, RelayManager, RelayManagerChannels,
    };
    use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
    use crate::utilities::config::{read_config_file, ConfigData};
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::thread::scope;
    use std::time::Duration;

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

        fn get_heartbeat_interval_seconds(&self) -> Option<u64> {
            Some(1)
        }

        fn heartbeat(&mut self) -> Result<(), RelayError> {
            self.heartbeat_counter += 1;
            Ok(())
        }

        fn flush_buffer(&mut self) -> Result<(), RelayError> {
            self.flush_counter += 1;
            Ok(())
        }
    }

    impl ActuateVoid {
        // provides the number of executions to the test case for assertion
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

                spin_sleeper.sleep(sleep_duration); // wait for two seconds to test the heartbeat

                let _ = tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
                rx_signal_handler_from_relay_manager.recv().unwrap();
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

        let mut relay_manager = RelayManager::new(config.relay_manager);

        let (tx_relay_manager_to_refill, rx_refill_from_relay_manager) = mpsc::channel();
        let (tx_refill_to_relay_manager, rx_relay_manager_from_refill) = mpsc::channel();
        let (tx_relay_manager_to_heating, rx_heating_from_relay_manager) = mpsc::channel();
        let (tx_heating_to_relay_manager, rx_relay_manager_from_heating) = mpsc::channel();
        let (tx_relay_manager_to_ventilation, _rx_ventilation_from_relay_manager) = mpsc::channel();
        let (_tx_ventilation_to_relay_manager, rx_relay_manager_from_ventilation) = mpsc::channel();
        let (tx_relay_manager_to_balling, _rx_balling_from_relay_manager) = mpsc::channel();
        let (_tx_balling_to_relay_manager, rx_relay_manager_from_balling) = mpsc::channel();
        let (tx_relay_manager_to_feed, _rx_feed_from_relay_manager) = mpsc::channel();
        let (_tx_feed_to_relay_manager, rx_relay_manager_from_feed) = mpsc::channel();
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

                let _ = tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
                rx_signal_handler_from_relay_manager.recv().unwrap();
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

        let mut relay_manager = RelayManager::new(config.relay_manager);

        let (tx_relay_manager_to_refill, rx_refill_from_relay_manager) = mpsc::channel();
        let (tx_refill_to_relay_manager, rx_relay_manager_from_refill) = mpsc::channel();
        let (tx_relay_manager_to_heating, _rx_heating_from_relay_manager) = mpsc::channel();
        let (_tx_heating_to_relay_manager, rx_relay_manager_from_heating) = mpsc::channel();
        let (tx_relay_manager_to_ventilation, _rx_ventilation_from_relay_manager) = mpsc::channel();
        let (_tx_ventilation_to_relay_manager, rx_relay_manager_from_ventilation) = mpsc::channel();
        let (tx_relay_manager_to_balling, _rx_balling_from_relay_manager) = mpsc::channel();
        let (_tx_balling_to_relay_manager, rx_relay_manager_from_balling) = mpsc::channel();
        let (tx_relay_manager_to_feed, _rx_feed_from_relay_manager) = mpsc::channel();
        let (_tx_feed_to_relay_manager, rx_relay_manager_from_feed) = mpsc::channel();
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

                let _ = tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
                rx_signal_handler_from_relay_manager.recv().unwrap();
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

                // Test object thread has ended.

                // Assertion checks the number of calls to the actuator.
                // Relay manager shall repeat in case of failure as configured by user.
                assert_eq!(mock_actuator.get_execution_count(), target_repeat_times + 1);
            });
        });
    }
}
