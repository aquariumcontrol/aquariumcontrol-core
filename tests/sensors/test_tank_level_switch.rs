/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Testing the dedicated thread for monitoring and stabilizing a tank water level switch.

use aquarium_control::launch::channels::{channel, Channels};
use aquarium_control::mocks::mock_signal::tests::MockSignal;
use aquarium_control::mocks::mock_tcp::tests::mock_tcp_for_tank_level_switch;
use aquarium_control::mocks::test_command::tests::TestCommand;
use aquarium_control::sensors::tank_level_switch::{
    TankLevelSwitch, TankLevelSwitchError, TankLevelSwitchSignals,
};
use aquarium_control::utilities::channel_content::InternalCommand;
use aquarium_control::utilities::config::{read_config_file, ConfigData};
use spin_sleep::SpinSleeper;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::sensors::gpio_handler::GpioHandler;

#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::relays::actuate_gpio;
use aquarium_control::sensors::tank_level_switch_channels::TankLevelSwitchChannels;

#[test]
// test if position stabilized signal is updating when the invalid signal is true
// [itest->swreq~sensors_tank_level_switch_preprocessing~1]
pub fn test_refill_calc_tank_level_switch_position_stabilized_false_when_invalid() {
    let mut channels = Channels::new(false, false, true, true);

    let (_tx_test_environment_to_tcp, mut rx_tcp_from_test_environment) = channel(1, true);

    let config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let duration_test_case_secs = config
        .tank_level_switch
        .tank_level_switch_position_low_stabilization_count
        + 1;

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    let mut tank_level_switch = TankLevelSwitch::new(
        config.tank_level_switch,
        true,
        None,
        config.gpio_handler.tank_level_switch,
    )
    .unwrap();
    #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
    let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true).unwrap();

    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, true)));

    // thread for mock tcp
    let join_handle_mock_tcp = thread::Builder::new()
        .name("mock_tcp".to_string())
        .spawn(move || {
            mock_tcp_for_tank_level_switch(
                channels.tcp_communication,
                Some(&mut rx_tcp_from_test_environment),
                false,
                true,
                None,
            );
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration = Duration::from_secs(duration_test_case_secs.into());
            let sleep_duration_thread_processing = Duration::from_millis(50);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration);
            spin_sleeper.sleep(sleep_duration_thread_processing);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_tank_level_switch
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_tank_level_switch
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_tank_level_switch
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_tank_level_switch_to_tcp_for_test_case_finish = channels
                .tank_level_switch
                .tx_tank_level_switch_to_tcp_opt
                .clone()
                .unwrap();

            let mutex_tank_level_switch_signals_for_assert =
                mutex_tank_level_switch_signals.clone();
            let _ = execute_blocking(
                &mut tank_level_switch,
                &mut channels.tank_level_switch,
                mutex_tank_level_switch_signals,
            );

            // Get the final state after execution.
            let signals_after_test_run = mutex_tank_level_switch_signals_for_assert.lock().unwrap();
            // Assert that the stabilized position is forced to true (safe state) due to invalidity.
            assert!(signals_after_test_run.tank_level_switch_position_stabilized);
            // Confirm invalid-flag remained true
            assert!(signals_after_test_run.tank_level_switch_invalid);

            // send Quit signal to mock threads because the test object has terminated
            let _ = tx_tank_level_switch_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
        })
        .unwrap();

    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
    join_handle_mock_tcp
        .join()
        .expect("Mock TCP thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");

    println!("* [TankLevelSwitch] testing if stabilized tank level position is high when invalid bit is true succeeded.");
}

/// Helper function to execute the tank level switch task inside a test Tokio runtime.
///
/// # Arguments
/// * `tank_level_switch` - A mutable reference to the `TankLevelSwitch` instance.
/// * `tank_level_switch_channels` - A mutable reference to the `TankLevelSwitchChannels` instance.
/// * `mutex_tank_level_switch_signals` - Shared mutex for tank level switch readings.
fn execute_blocking(
    tank_level_switch: &mut TankLevelSwitch,
    tank_level_switch_channels: &mut TankLevelSwitchChannels,
    mutex_tank_level_switch_signals: Arc<Mutex<TankLevelSwitchSignals>>,
) -> Result<(), TankLevelSwitchError> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            // blocking execution used for testing
            tank_level_switch
                .execute(tank_level_switch_channels, mutex_tank_level_switch_signals)
                .await
        })
}

/// Helper function for creating the test object
///
/// # Arguments
/// * `tank_level_switch` - A parameter representing `tank_level_switch`.
/// * `tank_level_switch_channels` - A parameter representing `tank_level_switch_channels`.
/// * `mutex_tank_level_switch_signals` - A parameter representing `mutex_tank_level_switch_signals`.
/// * `assert_cycle_time_warn_flag` - Reference value for assertion
fn create_test_object(
    mut tank_level_switch: TankLevelSwitch,
    mut tank_level_switch_channels: TankLevelSwitchChannels,
    mutex_tank_level_switch_signals: Arc<Mutex<TankLevelSwitchSignals>>,
    assert_cycle_time_warn_flag: bool,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let _ = execute_blocking(
                &mut tank_level_switch,
                &mut tank_level_switch_channels,
                mutex_tank_level_switch_signals,
            );
            println!(
                "Asserting lock_warn_cycle_time_exceeded is {:?}",
                assert_cycle_time_warn_flag
            );
            assert_eq!(
                tank_level_switch.lock_warn_cycle_time_exceeded,
                assert_cycle_time_warn_flag
            );
        })
        .unwrap()
}

#[test]
// test if the position signal is updating without delay when the invalid signal is false
// [itest->swreq~sensors_tank_level_switch_invalid~1]
pub fn test_refill_calc_tank_level_switch_position_stabilized_false_true() {
    let mut channels = Channels::new(false, false, true, true);

    let (mut tx_test_environment_to_tcp, mut rx_tcp_from_test_environment) = channel(1, true);

    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.refill.initial_wait_interval = 2;
    config.refill.check_interval = 60;

    let duration_test_case_part_1: u64 = 3; // three seconds with level switch in low position
    let duration_test_case_part_2: u64 = 2; // two seconds with level switch in high position

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    let tank_level_switch = TankLevelSwitch::new(
        config.tank_level_switch,
        false,
        None,
        config.gpio_handler.tank_level_switch,
    )
    .unwrap();
    #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
    let tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, false).unwrap();
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock tcp
    let join_handle_mock_tcp = thread::Builder::new()
        .name("mock_tcp".to_string())
        .spawn(move || {
            mock_tcp_for_tank_level_switch(
                channels.tcp_communication,
                Some(&mut rx_tcp_from_test_environment),
                false,
                false,
                None,
            );
        })
        .unwrap();

    let mut tx_test_environment_to_tcp_for_test_case_finish = channels
        .tank_level_switch
        .tx_tank_level_switch_to_tcp_opt
        .clone()
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_part_1 = Duration::from_secs(duration_test_case_part_1);
            let sleep_duration_part_2 = Duration::from_secs(duration_test_case_part_2);
            let sleep_duration_thread_processing = Duration::from_millis(50);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration_part_1);
            let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                MockSignal::TankLevelSwitchPosition(true),
            ));
            spin_sleeper.sleep(sleep_duration_part_2);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            spin_sleeper.sleep(sleep_duration_thread_processing);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_tank_level_switch
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_tank_level_switch
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);
            let _ = tx_test_environment_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_tank_level_switch
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // thread for the test object
    let join_handle_test_object = create_test_object(
        tank_level_switch,
        channels.tank_level_switch,
        mutex_tank_level_switch_signals,
        false,
    );

    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
    join_handle_mock_tcp
        .join()
        .expect("Mock TCP thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");

    println!("* [TankLevelSwitch] testing immediate change false -> true for stabilized tank level position when invalid bit is false succeeded");
}

#[test]
// test if the position signal is updating with delay when the invalid signal is false
// [itest->swreq~sensors_tank_level_switch_preprocessing~1]
pub fn test_refill_calc_tank_level_switch_position_stabilized_true_false() {
    let mut channels = Channels::new(false, false, true, true);

    let (mut tx_test_environment_to_tcp, mut rx_tcp_from_test_environment) = channel(1, true);

    let config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let duration_test_case_part_1: u64 = 3; // three seconds with level switch in low position
    let duration_test_case_part_2: u64 = 8; // two seconds with level switch in high position
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    let tank_level_switch = TankLevelSwitch::new(
        config.tank_level_switch,
        true,
        None,
        config.gpio_handler.tank_level_switch,
    )
    .unwrap();
    #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
    let tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true).unwrap();
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock tcp
    let join_handle_mock_tcp = thread::Builder::new()
        .name("mock_tcp".to_string())
        .spawn(move || {
            mock_tcp_for_tank_level_switch(
                channels.tcp_communication,
                Some(&mut rx_tcp_from_test_environment),
                true,
                false,
                None,
            );
        })
        .unwrap();

    let mut tx_test_environment_to_tcp_for_test_case_finish = channels
        .tank_level_switch
        .tx_tank_level_switch_to_tcp_opt
        .clone()
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_part_1 = Duration::from_secs(duration_test_case_part_1);
            let sleep_duration_part_2 = Duration::from_secs(duration_test_case_part_2);
            let sleep_duration_thread_processing = Duration::from_millis(50);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration_part_1);
            let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                MockSignal::TankLevelSwitchPosition(false),
            ));
            spin_sleeper.sleep(sleep_duration_part_2);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            spin_sleeper.sleep(sleep_duration_thread_processing);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_tank_level_switch
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_tank_level_switch
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);
            let _ = tx_test_environment_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_tank_level_switch
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // thread for the test object
    let join_handle_test_object = create_test_object(
        tank_level_switch,
        channels.tank_level_switch,
        mutex_tank_level_switch_signals,
        false,
    );

    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
    join_handle_mock_tcp
        .join()
        .expect("Mock TCP thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");

    println!("* [TankLevelSwitch] checking delayed change true -> false for stabilized tank level position when invalid bit is false succeeded.");
}

#[test]
// test if position stabilized signal is not updating when the raw input signal is toggling
// [itest->swreq~sensors_tank_level_switch_preprocessing~1]
pub fn test_refill_calc_tank_level_switch_position_stabilized_is_true_with_toggling_input() {
    let mut channels = Channels::new(false, false, true, true);
    let (mut tx_test_environment_to_tcp, mut rx_tcp_from_test_environment) = channel(1, true);

    let config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let duration_test_case_part: u64 = 2; // two seconds with the level switch before toggling

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    let tank_level_switch = TankLevelSwitch::new(
        config.tank_level_switch,
        true,
        None,
        config.gpio_handler.tank_level_switch,
    )
    .unwrap();
    #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
    let tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true).unwrap();
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock tcp
    let join_handle_mock_tcp = thread::Builder::new()
        .name("mock_tcp".to_string())
        .spawn(move || {
            mock_tcp_for_tank_level_switch(
                channels.tcp_communication,
                Some(&mut rx_tcp_from_test_environment),
                true,
                false,
                None,
            );
        })
        .unwrap();

    let mut tx_test_environment_to_tcp_for_test_case_finish = channels
        .tank_level_switch
        .tx_tank_level_switch_to_tcp_opt
        .clone()
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_part = Duration::from_secs(duration_test_case_part);
            let sleep_duration_thread_processing = Duration::from_millis(50);
            let spin_sleeper = SpinSleeper::default();
            for _ in 0..2 {
                spin_sleeper.sleep(sleep_duration_part);
                let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                    MockSignal::TankLevelSwitchPosition(false),
                ));
                spin_sleeper.sleep(sleep_duration_part);
                let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                    MockSignal::TankLevelSwitchPosition(true),
                ));
            }
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            spin_sleeper.sleep(sleep_duration_thread_processing);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_tank_level_switch
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_tank_level_switch
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);
            let _ = tx_test_environment_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_tank_level_switch
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // thread for the test object
    let join_handle_test_object = create_test_object(
        tank_level_switch,
        channels.tank_level_switch,
        mutex_tank_level_switch_signals,
        false,
    );

    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
    join_handle_mock_tcp
        .join()
        .expect("Mock TCP thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");

    println!("* [TankLevelSwitch] checking delayed change true -> false for stabilized tank level position when invalid bit is false succeeded.");
}

#[test]
// test if the cycle time monitoring is triggered when a delay in communication is applied.
// [itest->swreq~sensors_tank_level_switch_invalid~1]
pub fn test_tank_level_switch_cycle_time_monitoring() {
    let mut channels = Channels::new(false, false, true, true);

    let (mut tx_test_environment_to_tcp, mut rx_tcp_from_test_environment) = channel(1, true);

    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.refill.initial_wait_interval = 2;
    config.refill.check_interval = 60;

    let duration_test_case_part_1: u64 = 4; // three seconds with level switch in low position
    let duration_test_case_part_2: u64 = 3; // two seconds with level switch in high position

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    let tank_level_switch = TankLevelSwitch::new(
        config.tank_level_switch,
        false,
        None,
        config.gpio_handler.tank_level_switch,
    )
    .unwrap();
    #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
    let tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, false).unwrap();
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

    // thread for mock tcp
    let join_handle_mock_tcp = thread::Builder::new()
        .name("mock_tcp".to_string())
        .spawn(move || {
            mock_tcp_for_tank_level_switch(
                channels.tcp_communication,
                Some(&mut rx_tcp_from_test_environment),
                false,
                false,
                Some(Duration::from_millis(1000)),
            );
        })
        .unwrap();

    let mut tx_test_environment_to_tcp_for_test_case_finish = channels
        .tank_level_switch
        .tx_tank_level_switch_to_tcp_opt
        .clone()
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_part_1 = Duration::from_secs(duration_test_case_part_1);
            let sleep_duration_part_2 = Duration::from_secs(duration_test_case_part_2);
            let sleep_duration_thread_processing = Duration::from_millis(50);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration_part_1);
            let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                MockSignal::TankLevelSwitchPosition(true),
            ));
            spin_sleeper.sleep(sleep_duration_part_2);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            spin_sleeper.sleep(sleep_duration_thread_processing);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_tank_level_switch
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_tank_level_switch
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);
            let _ = tx_test_environment_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_tank_level_switch
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // thread for the test object
    let join_handle_test_object = create_test_object(
        tank_level_switch,
        channels.tank_level_switch,
        mutex_tank_level_switch_signals,
        true,
    );

    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
    join_handle_mock_tcp
        .join()
        .expect("Mock TCP thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");

    println!("* [TankLevelSwitch] testing immediate change false -> true for stabilized tank level position when invalid bit is false succeeded");
}

#[cfg(all(target_os = "linux", feature = "target_hw"))]
fn test_transition_stabilized_true_false(
    spin_sleeper: &SpinSleeper,
    sleep_duration_part: Duration,
    mutex_tank_level_switch_signals: &mut Arc<Mutex<TankLevelSwitchSignals>>,
) {
    for i in 0..20 {
        // loop for the first 10 seconds
        spin_sleeper.sleep(sleep_duration_part);
        {
            let tank_level_switch_signals = mutex_tank_level_switch_signals.lock().unwrap();
            println!(
                "Tank level switch signals at iteration {}: Position={} Invalid={} Stabilized={}",
                i,
                tank_level_switch_signals.tank_level_switch_position,
                tank_level_switch_signals.tank_level_switch_invalid,
                tank_level_switch_signals.tank_level_switch_position_stabilized
            );
            if i < 10 {
                assert_eq!(tank_level_switch_signals.tank_level_switch_position, false);
                assert_eq!(
                    tank_level_switch_signals.tank_level_switch_position_stabilized,
                    true
                );
            } else {
                assert_eq!(tank_level_switch_signals.tank_level_switch_position, false);
                assert_eq!(
                    tank_level_switch_signals.tank_level_switch_position_stabilized,
                    false
                );
            }
        }
    }
}

#[test]
#[ignore]
#[cfg(all(target_os = "linux", feature = "target_hw"))]
// test functionality on real hardware
// [itest->swreq~sensors_tank_level_switch_preprocessing~1]
pub fn test_tank_level_switch_position_on_hardware() {
    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.gpio_handler.use_simulator = false;
    config.tank_level_switch.use_simulator = false;
    let mut channels = Channels::new_for_test();

    let tank_level_switch_pin = config.gpio_handler.tank_level_switch;

    let gpio_handler = GpioHandler::new(config.gpio_handler).unwrap().unwrap();

    let gpio_lib_handle = gpio_handler.gpio_lib_handle_opt.clone().unwrap();

    let mut stimuli_pin =
        actuate_gpio::get_output_pin(&gpio_lib_handle, 21, "Tank level switch stimuli").unwrap();

    let mut tank_level_switch = TankLevelSwitch::new(
        config.tank_level_switch,
        true,
        Some(gpio_handler.gpio_lib_handle_opt.unwrap()),
        tank_level_switch_pin,
    )
    .unwrap();

    let mut mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));
    let mutex_tank_level_switch_signals_clone_for_test_object =
        mutex_tank_level_switch_signals.clone();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_part = Duration::from_millis(500);
            let spin_sleeper = SpinSleeper::default();

            test_transition_stabilized_true_false(
                &spin_sleeper,
                sleep_duration_part,
                &mut mutex_tank_level_switch_signals
            );

            stimuli_pin.set_high();

            for i in 0..20 {
                // loop for the second 10 seconds
                spin_sleeper.sleep(sleep_duration_part);
                {
                    let tank_level_switch_signals =
                        mutex_tank_level_switch_signals.lock().unwrap();
                    println!(
                        "Tank level switch signals at iteration {}: Position={} Invalid={} Stabilized={}",
                        i,
                        tank_level_switch_signals.tank_level_switch_position,
                        tank_level_switch_signals.tank_level_switch_invalid,
                        tank_level_switch_signals.tank_level_switch_position_stabilized
                    );
                    assert_eq!(tank_level_switch_signals.tank_level_switch_position, true);
                    assert_eq!(tank_level_switch_signals.tank_level_switch_position_stabilized, true);
                }
            }

            stimuli_pin.set_low();

            test_transition_stabilized_true_false(
                &spin_sleeper,
                sleep_duration_part,
                &mut mutex_tank_level_switch_signals
            );

            let _ = channels.signal_handler.tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
            let _ = channels.signal_handler.rx_signal_handler_from_tank_level_switch.blocking_recv().unwrap();
            let _ = channels.signal_handler.tx_signal_handler_to_tank_level_switch.send(InternalCommand::Terminate);
            spin_sleeper.sleep(sleep_duration_part);
        })
        .unwrap();

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let _ = execute_blocking(
                &mut tank_level_switch,
                &mut channels.tank_level_switch,
                mutex_tank_level_switch_signals_clone_for_test_object,
            );
        })
        .unwrap();

    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
}

#[cfg(feature = "test-utils")]
#[test]
// test if mutex access duration is measured and monitored
// [itest->swreq~sensors_tank_level_switch_preprocessing~1]
pub fn test_refill_calc_tank_level_switch_position_stabilized_false_true_with_mutex_blocked_too_long(
) {
    let mut channels = Channels::new(false, false, true, true);

    let (mut tx_test_environment_to_tcp, mut rx_tcp_from_test_environment) = channel(1, true);

    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.refill.initial_wait_interval = 2;
    config.refill.check_interval = 60;

    let duration_test_case_part_1: u64 = 3; // three seconds with level switch in low position
    let duration_test_case_part_2: u64 = 2; // two seconds with level switch in high position

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    let mut tank_level_switch = TankLevelSwitch::new(
        config.tank_level_switch,
        false,
        None,
        config.gpio_handler.tank_level_switch,
    )
    .unwrap();
    #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
    let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, false).unwrap();
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));
    let mutex_tank_level_switch_signals_clone_for_test_environment =
        mutex_tank_level_switch_signals.clone();

    // thread for mock tcp
    let join_handle_mock_tcp = thread::Builder::new()
        .name("mock_tcp".to_string())
        .spawn(move || {
            mock_tcp_for_tank_level_switch(
                channels.tcp_communication,
                Some(&mut rx_tcp_from_test_environment),
                false,
                false,
                None,
            );
        })
        .unwrap();

    let mut tx_test_environment_to_tcp_for_test_case_finish = channels
        .tank_level_switch
        .tx_tank_level_switch_to_tcp_opt
        .clone()
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_part_1 = Duration::from_secs(duration_test_case_part_1);
            let sleep_duration_part_2 = Duration::from_secs(duration_test_case_part_2);
            let sleep_duration_thread_processing = Duration::from_millis(50);
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(sleep_duration_part_1);
            let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                MockSignal::TankLevelSwitchPosition(true),
            ));
            {
                // block the mutex on purpose
                let _unused = mutex_tank_level_switch_signals_clone_for_test_environment.lock();
                spin_sleeper.sleep(sleep_duration_part_2);
            }
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Quit);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Quit);
            spin_sleeper.sleep(sleep_duration_thread_processing);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_tank_level_switch
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_tank_level_switch
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_heating
                .send(InternalCommand::Terminate);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_refill
                .send(InternalCommand::Terminate);
            let _ = tx_test_environment_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_tank_level_switch
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let _ = execute_blocking(
                &mut tank_level_switch,
                &mut channels.tank_level_switch,
                mutex_tank_level_switch_signals,
            );

            // check if monitoring recorded excessive mutex blocking time
            assert!(tank_level_switch.mutex_access_duration_exceeded);
        })
        .unwrap();

    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
    join_handle_mock_tcp
        .join()
        .expect("Mock TCP thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");

    println!("* [TankLevelSwitch] testing immediate change false -> true for stabilized tank level position when invalid bit is false succeeded while having mutex blocked");
}
