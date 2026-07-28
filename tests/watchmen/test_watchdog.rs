/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the system's hardware or software watchdog timer.

use aquarium_control::launch::channels::Channels;
use aquarium_control::mocks::mock_petting::tests::MockPetting;
use aquarium_control::utilities::channel_content::InternalCommand;
use aquarium_control::utilities::config::{read_config_file, ConfigData};
use aquarium_control::watchmen::petting::Petting;
use aquarium_control::watchmen::watchdog::{Watchdog, WatchdogError};
use spin_sleep::SpinSleeper;
use std::fs::OpenOptions;
use std::io::Read;
use std::thread::scope;
use std::time::Duration;

/// Synchronously executes the main task loop for testing.
///
/// # Arguments
/// * `watchdog` - A parameter representing `watchdog`.
/// * `watchdog_channels` - A parameter representing `watchdog_channels`.
/// * `petting` - A parameter representing `petting`.
fn execute_blocking(
    watchdog: &mut Watchdog,
    watchdog_channels: &mut aquarium_control::watchmen::watchdog_channels::WatchDogChannels,
    petting: &mut impl aquarium_control::watchmen::petting::PettingTrait,
) {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            // blocking execution used for testing
            watchdog.execute(watchdog_channels, petting).await;
        });
}

#[test]
// test if Watchdog::new fails if the configuration contains an empty watchdog filename
/// [itest->swreq~watchmen_pet_watchdog~1]
fn test_new_fails_with_empty_filename() {
    // Arrange
    let mut config: ConfigData =
        read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.watchdog.watchdog_filename = "".to_string();

    // Act
    let result = Watchdog::new(config.watchdog);

    // Assert
    assert!(matches!(result, Err(WatchdogError::FilenameIsEmpty(_))));
}

#[test]
// test if Watchdog::new fails if the configuration contains an empty heartbeat message
fn test_new_fails_with_empty_heartbeat() {
    // Arrange
    let mut config: ConfigData =
        read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.watchdog.watchdog_heartbeat = "".to_string();

    // Act
    let result = Watchdog::new(config.watchdog);

    // Assert
    assert!(matches!(
        result,
        Err(WatchdogError::HeartbeatMessageIsEmpty(_))
    ));
}

#[test]
// test if Watchdog::new fails if the configuration contains an empty deactivation message
fn test_new_fails_with_empty_deactivation_message() {
    // Arrange
    let mut config: ConfigData =
        read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.watchdog.watchdog_deactivation = "".to_string();

    // Act
    let result = Watchdog::new(config.watchdog);

    // Assert
    assert!(matches!(
        result,
        Err(WatchdogError::DeactivationMessageIsEmpty(_))
    ));
}

#[test]
// test if Watchdog::new fails if the configuration contains identical heartbeat and deactivation messages
fn test_new_fails_with_identical_messages() {
    // Arrange
    let mut config: ConfigData =
        read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.watchdog.watchdog_deactivation = "T".to_string();
    config.watchdog.watchdog_heartbeat = "T".to_string();

    // Act
    let result = Watchdog::new(config.watchdog);

    // Assert
    assert!(matches!(
        result,
        Err(WatchdogError::HeartbeatAndDeactivationMessagesAreIdentical(
            _,
            _
        ))
    ));
}

#[test]
// test if Watchdog::new fails if the configuration contains a zero-heartbeat duration
fn test_new_fails_with_zero_heartbeat_duration() {
    // Arrange
    let mut config: ConfigData =
        read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.watchdog.heartbeat_duration_millis = 0;

    // Act
    let result = Watchdog::new(config.watchdog);

    // Assert
    assert!(matches!(
        result,
        Err(WatchdogError::DurationBetweenTwoHeartbeatsIsZero(_))
    ));
}

#[test]
// test if the watchdog is executing the petting in time
// [itest->swreq~watchmen_reboot_on_stall~1]
fn test_watchdog_execute_petting() {
    let config: ConfigData =
        read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

    let mut channels = Channels::new_for_test();

    let min_duration = Duration::from_millis(config.watchdog.heartbeat_duration_millis - 50);
    let max_duration = Duration::from_millis(config.watchdog.heartbeat_duration_millis + 50);

    let reference_heartbeat_signal = config.watchdog.watchdog_heartbeat.clone();
    let reference_deactivation_signal = config.watchdog.watchdog_deactivation.clone();

    let mut watchdog = Watchdog::new(config.watchdog).unwrap();
    let mut mock_petting = MockPetting::new();

    scope(|scope| {
        // thread for the test environment
        scope.spawn(move || {
            let sleep_duration_test_environment = Duration::from_millis(3100);
            let spin_sleeper_test_environment = SpinSleeper::default();

            spin_sleeper_test_environment.sleep(sleep_duration_test_environment);
            channels
                .signal_handler
                .tx_signal_handler_to_watchdog
                .send(InternalCommand::Terminate)
                .unwrap();
            channels
                .signal_handler
                .rx_signal_handler_from_watchdog
                .blocking_recv()
                .unwrap();
        });

        // thread for the test object
        scope.spawn(move || {
            execute_blocking(&mut watchdog, &mut channels.watchdog, &mut mock_petting);

            assert_eq!(mock_petting.petting_recorder.len(), 5);

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
                    mock_petting.signal_recorder[i], reference_heartbeat_signal,
                    "Signal at index {} was expected to be '{}', but was '{}'",
                    i, reference_deactivation_signal, mock_petting.signal_recorder[i]
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
// [itest->swreq~watchmen_reboot_on_stall~1]
fn test_watchdog_check_heartbeat_message() {
    let mut config: ConfigData =
        read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

    config.watchdog.watchdog_filename = "/var/log/watchdog.log".to_string();

    let mut channels = Channels::new_for_test();

    let reference_heartbeat_signal = config.watchdog.watchdog_heartbeat.clone();
    let reference_deactivation_signal = config.watchdog.watchdog_deactivation.clone();

    // Erase content of the watchdog file
    let file = OpenOptions::new()
        .write(true)
        .open(config.watchdog.watchdog_filename.clone())
        .unwrap();
    _ = file.set_len(0); // This is the core operation

    let watchdog_filename_for_assert = config.watchdog.watchdog_filename.clone();
    let mut watchdog = Watchdog::new(config.watchdog).unwrap();
    let mut petting = Petting::new();

    scope(|scope| {
        // thread for the test environment
        scope.spawn(move || {
            let sleep_duration_test_environment = Duration::from_millis(3100);
            let spin_sleeper_test_environment = SpinSleeper::default();

            spin_sleeper_test_environment.sleep(sleep_duration_test_environment);
            channels
                .signal_handler
                .tx_signal_handler_to_watchdog
                .send(InternalCommand::Terminate)
                .unwrap();
            channels
                .signal_handler
                .rx_signal_handler_from_watchdog
                .blocking_recv()
                .unwrap();
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
            execute_blocking(&mut watchdog, &mut channels.watchdog, &mut petting);
        });
    });
}
