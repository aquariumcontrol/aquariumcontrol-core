/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::utilities::channel_content::InternalCommand;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

use crate::watchmen::petting::PettingTrait;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::Receiver;
use std::time::Duration;

const LOG_TARGET_WATCHDOG: &str = "watchdog";

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
    /// Provides a struct of type Watchdog.
    /// # Arguments
    /// * `config` - configuration data for Watchdog communication
    pub fn new(config: WatchdogConfig) -> Watchdog {
        if config.heartbeat_duration_millis == 0 {
            panic!(
                "{}: duration between two watchdog heartbeats is zero - please assign a value > 0",
                LOG_TARGET_WATCHDOG
            );
        }
        if config.watchdog_filename.is_empty() {
            panic!(
                "{}: watchdog filename is empty - please assign a value",
                LOG_TARGET_WATCHDOG
            );
        }
        if config.watchdog_heartbeat.is_empty() {
            panic!(
                "{}: watchdog heartbeat message is empty - please assign a value",
                LOG_TARGET_WATCHDOG
            );
        }
        if config.watchdog_deactivation.is_empty() {
            panic!(
                "{}: watchdog heartbeat message is empty - please assign a value",
                LOG_TARGET_WATCHDOG
            );
        }
        if config.watchdog_deactivation == config.watchdog_heartbeat {
            panic!(
                "{}: watchdog heartbeat and deactivation message are identical - please assign different values",
                LOG_TARGET_WATCHDOG
            );       
        }

        Watchdog { config }
    }

    /// Executes the watchdog communication in a loop until the signal handler commands termination.
    /// # Arguments
    /// * `rx_from_signal_handler` - receiver part of the channel for communication from signal handler                
    /// * `rx_from_messaging_opt` - receiver part of the channel communication wrapped in Option from Messaging
    pub fn execute(
        &mut self,
        rx_from_signal_handler: Receiver<InternalCommand>,
        rx_from_messaging_opt: Option<Receiver<InternalCommand>>,
        petting: &mut impl PettingTrait,
    ) {
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
                    &self.config.watchdog_heartbeat
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
    }
}

#[cfg(test)]
pub mod tests {
    use crate::utilities::config::{read_config_file, ConfigData};
    use crate::watchmen::watchdog::Watchdog;
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{self};
    use std::thread::scope;
    use std::time::Duration;

    use crate::stubs::mock_petting::tests::MockPetting;
    use crate::utilities::channel_content::InternalCommand;

    #[test]
    #[should_panic]
    // test if the watchdog is panicking during initialization if the configuration file contains
    // an empty string for watchdog filename
    pub fn test_watchdog_init_watchdog_file_empty() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.watchdog.watchdog_filename = "".to_string();

        let _ = Watchdog::new(config.watchdog);
    }
    
    #[test]
    #[should_panic]
    // test if the watchdog is panicking during initialization if the configuration file contains an
    // empty string for heartbeat signal
    pub fn test_watchdog_init_heartbeat_empty() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.watchdog.watchdog_heartbeat = "".to_string();

        let _ = Watchdog::new(config.watchdog);
    }
    
    #[test]
    #[should_panic]
    // test if the watchdog is panicking during initialization if the configuration file contains an
    // empty string for deactivation signal
    pub fn test_watchdog_init_deactivation_empty() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.watchdog.watchdog_deactivation = "".to_string();

        let _ = Watchdog::new(config.watchdog);
    }
    
    #[test]
    #[should_panic]
    // test if the watchdog is panicking during initialization if the configuration file contains
    // identical strings for heartbeat and deactivation
    pub fn test_watchdog_init_heartbeat_deactivation_identical() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
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
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.watchdog.heartbeat_duration_millis = 0;
        
        let _ = Watchdog::new(config.watchdog);
    }
    
    #[test]
    // test if the watchdog is executing the petting in time
    pub fn test_watchdog_execute_petting() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
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
                tx_signal_handler_to_watchdog.send(InternalCommand::Terminate).unwrap();
            });

            // thread for the test object
            scope.spawn(move || {
                watchdog.execute(
                    rx_watchdog_from_signal_handler,
                    None,
                    &mut mock_petting
                );

                assert_eq!(
                    mock_petting.petting_recorder.len(),
                    5
                );

                //println!("{}", mock_petting);
                
                // Check intervals between consecutive heartbeats
                // Iterate from the second timestamp (index 1)
                for i in 1..mock_petting.petting_recorder.len() {
                    let previous_ts = mock_petting.petting_recorder[i - 1];
                    let current_ts = mock_petting.petting_recorder[i];
                    let delta_duration = current_ts.duration_since(previous_ts).unwrap();

                    //println!("Main thread: Interval {}: {:?}", i - 1, delta_duration); // Debugging
                    
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
}
