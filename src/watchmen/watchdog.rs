/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::watchmen::petting::PettingTrait;
use log::error;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

#[cfg(target_os = "linux")]
use log::info;

#[cfg(target_os = "linux")]
use nix::unistd::gettid;

use crate::utilities::channel_content::InternalCommand;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

/// Holds the configuration data for the watchdog communication.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct WatchdogConfig {
    /// communication will only be active when the flag is set to true
    active: bool,

    /// filename for sending heartbeat to system watchdog
    watchdog_filename: String,

    /// heartbeat to be written to watchdog file
    watchdog_heartbeat: String,

    /// duration between two heartbeats in milliseconds
    heartbeat_duration_millis: u64,

    /// deactivation code to be written to watchdog file upon termination of the application
    watchdog_deactivation: String,
}

/// Contains the configuration and the implementation for the Watchdog communication
pub struct Watchdog {
    config: WatchdogConfig,
}

impl Watchdog {
    /// Creates a new `Watchdog` instance.
    ///
    /// This constructor initializes the watchdog communication module with its specified
    /// configuration. It performs critical **validation checks** on the configuration
    /// parameters to ensure the watchdog can operate correctly and safely.
    ///
    /// **Validation checks performed: **
    /// - `heartbeat_duration_millis` must be greater than zero.
    /// - `watchdog_filename` cannot be an empty string.
    /// - `watchdog_heartbeat` message cannot be an empty string.
    /// - `watchdog_deactivation` message cannot be an empty string.
    /// - `watchdog_heartbeat` and `watchdog_deactivation` messages must be different.
    ///
    /// # Arguments
    /// * `config` - **Configuration data** for the watchdog communication, loaded from a TOML file.
    ///   This includes filename, heartbeat messages, and timing parameters.
    ///
    /// # Returns
    /// A new **`Watchdog` struct**, ready for sending heartbeats or respond to termination requests.
    ///
    /// # Panics
    /// This function will **panic** if any of the configuration validation checks fail,
    /// as an incorrectly configured watchdog can lead to system instability or unintended resets.
    pub fn new(config: WatchdogConfig) -> Watchdog {
        if config.heartbeat_duration_millis == 0 {
            panic!(
                "{}: duration between two watchdog heartbeats is zero - please assign a value > 0",
                module_path!()
            );
        }
        if config.watchdog_filename.is_empty() {
            panic!(
                "{}: watchdog filename is empty - please assign a value",
                module_path!()
            );
        }
        if config.watchdog_heartbeat.is_empty() {
            panic!(
                "{}: watchdog heartbeat message is empty - please assign a value",
                module_path!()
            );
        }
        if config.watchdog_deactivation.is_empty() {
            panic!(
                "{}: watchdog heartbeat message is empty - please assign a value",
                module_path!()
            );
        }
        if config.watchdog_deactivation == config.watchdog_heartbeat {
            panic!(
                "{}: watchdog heartbeat and deactivation message are identical - please assign different values",
                module_path!()
            );
        }

        Watchdog { config }
    }

    /// Executes the main control loop for the watchdog communication module.
    ///
    /// This function runs continuously, managing the periodic sending of heartbeats
    /// to a system watchdog. It ensures the application's liveness is maintained
    /// and handles various external commands for controlling its operation.
    ///
    /// **Key Operations:**
    /// - **Heartbeat Sending**: If `active` and not `inhibited`, it calls the `petting.pet()`
    ///   method to send a heartbeat message to the configured `watchdog_filename`.
    /// - **Cycle Management**: It sleeps for the configured `heartbeat_duration_millis`
    ///   between each heartbeat.
    /// - **External Control**: It processes `Start` and `Stop` commands received from
    ///   messaging or the signal handler to `inhibit` or re-enable watchdog activity.
    /// - **Graceful Deactivation**: Upon receiving a `Quit` command, it sends a final
    ///   `watchdog_deactivation` message before exiting the loop.
    /// - **Shutdown Confirmation**: After exiting the loop, it sends a confirmation back
    ///   to the signal handler.
    ///
    /// # Arguments
    /// * `tx_to_signal_handler` - The sender channel for this module to send acknowledgment
    ///   back to the main signal handler during shutdown.
    /// * `rx_from_signal_handler` - The receiver channel for this module to receive commands
    ///   (e.g., `Quit`, `Terminate`) from the main signal handler.
    /// * `rx_from_messaging_opt` - An `Option` containing the receiver channel for commands
    ///   (e.g., `Start`, `Stop`) from the messaging system.
    /// * `petting` - A mutable reference to an object implementing the `PettingTrait`,
    ///   responsible for the actual writing of heartbeat/deactivation messages to the watchdog file.
    pub fn execute(
        &mut self,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
        rx_from_messaging_opt: Option<Receiver<InternalCommand>>,
        petting: &mut impl PettingTrait,
    ) {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let spin_sleeper = SpinSleeper::default();
        let sleep_duration = Duration::from_millis(self.config.heartbeat_duration_millis);
        let mut quit_command_received: bool; // the request to end the application has been received
        let mut stop_command_received: bool; // the request to (temporarily) stop watchdog communication has been received
        let mut start_command_received: bool; // the request to (temporarily) stop watchdog communication has been received
        let mut inhibited: bool = false;

        loop {
            if self.config.active && !inhibited {
                // Send the heartbeat to the watchdog
                petting.pet(
                    &self.config.watchdog_filename,
                    &self.config.watchdog_heartbeat,
                );
            }
            spin_sleeper.sleep(sleep_duration);
            (
                quit_command_received,
                start_command_received,
                stop_command_received,
            ) = self
                .process_external_request(&rx_from_signal_handler, rx_from_messaging_opt.as_ref());

            if quit_command_received {
                // Deactivate the watchdog functionality
                petting.pet(
                    &self.config.watchdog_filename,
                    &self.config.watchdog_deactivation,
                );
                break;
            }

            // For testing purposes, allow deactivation of the watchdog communication.
            // This will trigger a reset of the system.
            if start_command_received {
                inhibited = false;
            }
            if stop_command_received {
                inhibited = true;
            }
        }

        // The application received the request to terminate. That is why the loop was left.
        // Answer to the thread which sent the request for termination, so that shutdown can proceed further.
        match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: module_path!(),
                    "Error in trying to send quit confirmation back to signal handler: {e:?}"
                );
            }
        };
    }
}

#[cfg(test)]
pub mod tests {
    use crate::utilities::config::{read_config_file, ConfigData};
    use crate::watchmen::watchdog::Watchdog;
    use spin_sleep::SpinSleeper;
    use std::fs::OpenOptions;
    use std::io::Read;
    use std::sync::mpsc::{self};
    use std::thread::scope;
    use std::time::Duration;

    use crate::mocks::mock_petting::tests::MockPetting;
    use crate::utilities::channel_content::InternalCommand;
    use crate::watchmen::petting::Petting;

    #[test]
    #[should_panic]
    // test if the watchdog is panicking during initialization if the configuration file contains
    // an empty string for watchdog filename
    pub fn test_watchdog_init_watchdog_file_empty() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.watchdog.watchdog_filename = "".to_string();

        let _ = Watchdog::new(config.watchdog);
    }

    #[test]
    #[should_panic]
    // test if the watchdog is panicking during initialization if the configuration file contains an
    // empty string for heartbeat signal
    pub fn test_watchdog_init_heartbeat_empty() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.watchdog.watchdog_heartbeat = "".to_string();

        let _ = Watchdog::new(config.watchdog);
    }

    #[test]
    #[should_panic]
    // test if the watchdog is panicking during initialization if the configuration file contains an
    // empty string for deactivation signal
    pub fn test_watchdog_init_deactivation_empty() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.watchdog.watchdog_deactivation = "".to_string();

        let _ = Watchdog::new(config.watchdog);
    }

    #[test]
    #[should_panic]
    // test if the watchdog is panicking during initialization if the configuration file contains
    // identical strings for heartbeat and deactivation
    pub fn test_watchdog_init_heartbeat_deactivation_identical() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.watchdog.watchdog_deactivation = "T".to_string();
        config.watchdog.watchdog_heartbeat = "T".to_string();

        let _ = Watchdog::new(config.watchdog);
    }

    #[test]
    #[should_panic]
    // test if the watchdog is panicking during initialization if the configuration file contains
    // zero value for duration of pause between two heartbeats
    pub fn test_watchdog_init_heartbeat_duration_zero() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.watchdog.heartbeat_duration_millis = 0;

        let _ = Watchdog::new(config.watchdog);
    }

    #[test]
    // test if the watchdog is executing the petting in time
    pub fn test_watchdog_execute_petting() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let (tx_watchdog_to_signal_handler, rx_signal_handler_from_watchdog) = mpsc::channel();
        let (tx_signal_handler_to_watchdog, rx_watchdog_from_signal_handler) = mpsc::channel();

        let min_duration = Duration::from_millis(config.watchdog.heartbeat_duration_millis - 50);
        let max_duration = Duration::from_millis(config.watchdog.heartbeat_duration_millis + 50);

        let reference_heartbeat_signal = config.watchdog.watchdog_heartbeat.clone();
        let reference_deactivation_signal = config.watchdog.watchdog_deactivation.clone();

        let mut watchdog = Watchdog::new(config.watchdog);
        let mut mock_petting = MockPetting::new();

        scope(|scope| {
            // thread for the test environment
            scope.spawn(move || {
                let sleep_duration_test_environment = Duration::from_millis(3100);
                let spin_sleeper_test_environment = SpinSleeper::default();

                spin_sleeper_test_environment.sleep(sleep_duration_test_environment);
                tx_signal_handler_to_watchdog
                    .send(InternalCommand::Terminate)
                    .unwrap();
                rx_signal_handler_from_watchdog.recv().unwrap();
            });

            // thread for the test object
            scope.spawn(move || {
                watchdog.execute(
                    tx_watchdog_to_signal_handler,
                    rx_watchdog_from_signal_handler,
                    None,
                    &mut mock_petting
                );

                assert_eq!(
                    mock_petting.petting_recorder.len(),
                    5
                );

                // Check intervals between consecutive heartbeats
                // Iterate from the second timestamp (index 1)
                for i in 1..mock_petting.petting_recorder.len() {
                    let previous_ts = mock_petting.petting_recorder[i - 1];
                    let current_ts = mock_petting.petting_recorder[i];
                    let delta_duration = current_ts.duration_since(previous_ts);

                    // Assert that the delta is within the allowed range
                    assert!(
                        delta_duration >= min_duration && delta_duration <= max_duration,
                        "Heartbeat delta {:?} at index {} is outside the allowed range [{:?}, {:?}]",
                        delta_duration,
                        i - 1,
                        min_duration,
                        max_duration,
                    );
                }

                // Assert that the first 4 entries are "1"
                for i in 0..4 {
                    assert_eq!(
                        mock_petting.signal_recorder[i],
                        reference_heartbeat_signal,
                        "Signal at index {} was expected to be '{}', but was '{}'",
                        i,
                        reference_deactivation_signal,
                        mock_petting.signal_recorder[i]
                    );
                }

                assert_eq!(
                    mock_petting.signal_recorder[4],
                    reference_deactivation_signal,
                    "Signal was expected to be '{}', but was '{}'",
                    reference_heartbeat_signal,
                    mock_petting.signal_recorder.last().unwrap()
                );
            });
        });
    }

    #[test]
    // test if the watchdog is writing the right heartbeat messages to the watchdog file
    pub fn test_watchdog_check_heartbeat_message() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

        config.watchdog.watchdog_filename = "/var/log/watchdog.log".to_string();
        let (tx_watchdog_to_signal_handler, rx_signal_handler_from_watchdog) = mpsc::channel();
        let (tx_signal_handler_to_watchdog, rx_watchdog_from_signal_handler) = mpsc::channel();

        let reference_heartbeat_signal = config.watchdog.watchdog_heartbeat.clone();
        let reference_deactivation_signal = config.watchdog.watchdog_deactivation.clone();

        // Erase content of the watchdog file
        let file = OpenOptions::new()
            .write(true)
            .open(config.watchdog.watchdog_filename.clone())
            .unwrap();
        _ = file.set_len(0); // This is the core operation

        let watchdog_filename_for_assert = config.watchdog.watchdog_filename.clone();
        let mut watchdog = Watchdog::new(config.watchdog);
        let mut petting = Petting::new();

        scope(|scope| {
            // thread for the test environment
            scope.spawn(move || {
                let sleep_duration_test_environment = Duration::from_millis(3100);
                let spin_sleeper_test_environment = SpinSleeper::default();

                spin_sleeper_test_environment.sleep(sleep_duration_test_environment);
                tx_signal_handler_to_watchdog
                    .send(InternalCommand::Terminate)
                    .unwrap();
                rx_signal_handler_from_watchdog.recv().unwrap();
            });

            // thread for the assertions
            scope.spawn(move || {
                let sleep_duration_assert1 = Duration::from_millis(500);
                let sleep_duration_assert2 = Duration::from_millis(4500);
                let spin_sleeper_asserts = SpinSleeper::default();

                // let the test object start writing to the watchdog file
                spin_sleeper_asserts.sleep(sleep_duration_assert1);

                // assert the heartbeat message
                let mut file = OpenOptions::new()
                    .read(true)
                    .open(watchdog_filename_for_assert.clone())
                    .unwrap();
                let mut content = String::new();
                file.read_to_string(&mut content).unwrap();
                assert_eq!(content, reference_heartbeat_signal);

                // wait for the termination of the test object
                spin_sleeper_asserts.sleep(sleep_duration_assert2);

                // assert the watchdog deactivation message
                let mut file = OpenOptions::new()
                    .read(true)
                    .open(watchdog_filename_for_assert)
                    .unwrap();
                let mut content = String::new();
                file.read_to_string(&mut content).unwrap();
                assert_eq!(content, reference_deactivation_signal);
            });

            // thread for the test object
            scope.spawn(move || {
                watchdog.execute(
                    tx_watchdog_to_signal_handler,
                    rx_watchdog_from_signal_handler,
                    None,
                    &mut petting,
                );
            });
        });
    }
}
