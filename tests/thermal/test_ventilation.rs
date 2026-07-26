/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests for the aquarium's ventilation system.

use all_asserts::{assert_ge, assert_le};
use aquarium_control::database::thermal_set_value_updater_trait::ThermalSetValueUpdaterTrait;
use aquarium_control::launch::channels::{AquaReceiver, AquaSender, Channels};
use aquarium_control::mocks::mock_relay_manager::tests::mock_relay_manager;
use aquarium_control::mocks::mock_schedule_check::tests::mock_schedule_check;
use aquarium_control::mocks::mock_ventilation::tests::MockSqlInterfaceVentilationSetVals;
use aquarium_control::sensors::sensor_manager::SensorManagerSignals;
use aquarium_control::thermal::thermal_control_status::ThermalControlStatus;
use aquarium_control::thermal::ventilation::Ventilation;
use aquarium_control::thermal::ventilation_channels::VentilationChannels;
use aquarium_control::thermal::ventilation_mutexes::VentilationMutexes;
use aquarium_control::utilities::channel_content::{
    ActuatorState, AquariumDevice, InternalCommand,
};
use aquarium_control::utilities::config::{read_config_file, ConfigData};
use aquarium_control::utilities::sawtooth_profile::SawToothProfileConfig;
use aquarium_control::utilities::signal_handler::SignalHandlerChannels;
use spin_sleep::SpinSleeper;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Helper function to execute the ventilation task inside a test Tokio runtime.
///
/// # Arguments
/// * `ventilation` - A mutable reference to the `Ventilation` instance.
/// * `mutex_device_scheduler_ventilation` - Mutex coordinating access to the device scheduler.
/// * `ventilation_channels` - A mutable reference to the `VentilationChannels` instance.
/// * `ventilation_set_val_updater` - A mutable reference to the set value updater.
/// * `ventilation_mutexes` - Shared mutexes for ventilation conditions.
///
/// [itest->swreq~thermal_ventilation_cooling~1]
fn execute_blocking(
    ventilation: &mut Ventilation,
    mutex_device_scheduler_ventilation: Arc<tokio::sync::Mutex<i32>>,
    ventilation_channels: &mut VentilationChannels,
    ventilation_set_val_updater: &mut impl ThermalSetValueUpdaterTrait,
    ventilation_mutexes: VentilationMutexes,
) {
    tokio::runtime::Builder::new_current_thread().enable_all()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            // blocking execution used for testing
            let is_multi_thread_test = tokio::runtime::Handle::try_current()
                .map(|h| {
                    matches!(
                        h.runtime_flavor(),
                        tokio::runtime::RuntimeFlavor::MultiThread
                    )
                })
                .unwrap_or(false);

            ventilation
                .execute(
                    mutex_device_scheduler_ventilation,
                    ventilation_channels,
                    ventilation_set_val_updater,
                    ventilation_mutexes,
                    is_multi_thread_test,
                )
                .await;
        });
}

/// Prepares and returns a new `Ventilation` struct for use in test cases.
///
/// This private helper function simplifies test setup by loading the generic
/// test configuration file and then creating a `Ventilation` instance from its
/// specific `ventilation` section.
///
/// # Arguments
/// * `saw_tooth_profile_config_opt` - A parameter representing `saw_tooth_profile_config_opt`.
///
/// # Returns
/// A freshly initialized `Ventilation` struct, ready for testing.
/// Performs the `prepare_ventilation_tests` operation.
fn prepare_ventilation_tests(
    saw_tooth_profile_config_opt: Option<SawToothProfileConfig>,
) -> (ConfigData, Ventilation) {
    // prepare struct Ventilation for subsequent tests
    let mut config_for_ventilation: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    if let Some(saw_tooth_profile_config) = saw_tooth_profile_config_opt {
        config.ventilation.saw_tooth_profile_config = saw_tooth_profile_config.clone();
        config_for_ventilation.ventilation.saw_tooth_profile_config = saw_tooth_profile_config;
    }
    (config, Ventilation::new(config_for_ventilation.ventilation))
}

/// Creates a thread that simulates a test environment, managing test duration and sending shutdown signals.
///
/// This private helper function is used in integration tests to control the lifecycle of the
/// main test object (e.g., `Ventilation` module). It sleeps for a specified duration,
/// simulating the active test run, and then sends a `Quit` command followed by a `Terminate`
/// command to the test object, prompting its graceful shutdown.
/// If the mutex is provided as optional parameter, then the function will intermittently block
/// the mutex.
///
/// # Arguments
/// * `duration_seconds` - The duration (in seconds) that the test environment should "run" before initiating shutdown.
/// * `tx_signal_handler_to_ventilation` - The sender channel used by this mock environment to send commands to the `Ventilation` test object, simulating the signal handler.
/// * `rx_signal_handler_from_ventilation` - The receiver channel for this mock environment to receive acknowledgments back from the `Ventilation` test object.
/// * `mutex_ventilation_status_opt` - Mutex for ventilation actuator status wrapped in Option
///
/// # Returns
/// A `thread::JoinHandle<()>` which can be used by the main test thread to wait for this mock environment thread's completion.
///
/// # Panics
/// This function will panic if:
/// - The new thread cannot be spawned.
/// - It fails to send the `Quit` or `Terminate` command.
/// - It fails to receive the acknowledgment from the `Ventilation` test object.
fn create_test_environment(
    duration_seconds: u64,
    mut signal_handler_channels: SignalHandlerChannels,
    mutex_ventilation_status_opt: Option<Arc<Mutex<ThermalControlStatus>>>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            if let Some(mutex_ventilation_status) = mutex_ventilation_status_opt {
                // caller wants the test environment to intermittently block the mutex
                let sleep_duration_one_second = Duration::from_secs(1);
                let spin_sleeper = SpinSleeper::default();
                for i in 0..10 {
                    if i % 2 == 0 {
                        spin_sleeper.sleep(sleep_duration_one_second);
                    } else {
                        // block mutex on purpose
                        {
                            match mutex_ventilation_status.lock() {
                                Ok(_) => {
                                    spin_sleeper.sleep(sleep_duration_one_second);
                                }
                                Err(_) => {
                                    // Do nothing
                                }
                            }
                        }
                    }
                }
            } else {
                let sleep_duration_ten_seconds = Duration::from_secs(duration_seconds);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
            }
            let _ = signal_handler_channels
                .tx_signal_handler_to_ventilation
                .send(InternalCommand::Quit);
            signal_handler_channels
                .rx_signal_handler_from_ventilation
                .blocking_recv()
                .unwrap();
            let _ = signal_handler_channels
                .tx_signal_handler_to_ventilation
                .send(InternalCommand::Terminate);
        })
        .unwrap()
}

// Check if the ventilation is switched on when the temperature is high.
// Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
#[test]
// Verifies that ventilation with measured temperature high without blocking mutex.
pub fn test_ventilation_with_measured_temperature_high_without_blocking_mutex() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut ventilation) = prepare_ventilation_tests(None);

    // replacement value is used for initializing mutex
    config.sensor_manager.replacement_value_water_temperature = 30.0;

    let mut channels = Channels::new_for_test();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                None,
                true,
            );
        })
        .unwrap();

    // Mutex needed for water temperature
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));

    let mutex_device_scheduler_ventilation = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = create_mock_relay_manager_for_ventilation_on(
        channels.relay_manager.tx_relay_manager_to_ventilation,
        channels.relay_manager.rx_relay_manager_from_ventilation,
    );

    // thread for controlling duration of test run
    let join_handle_test_environment = create_test_environment(
        10,
        channels.signal_handler,
        None, // no blocking of mutex in this test case
    );

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                .ventilation
                .tx_ventilation_to_schedule_check
                .clone();
            let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                channels.ventilation.tx_ventilation_to_relay_manager.clone();

            let mut ventilation_set_value_updater =
                MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

            let ventilation_mutexes = VentilationMutexes {
                mutex_sensor_manager_signals,
                mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut ventilation,
                mutex_device_scheduler_ventilation.clone(),
                &mut channels.ventilation,
                &mut ventilation_set_value_updater,
                ventilation_mutexes,
            );

            assert!(!ventilation.mutex_access_duration_exceeded);

            // send Quit signal to mock threads because the test object has terminated
            let _ =
                tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ =
                tx_ventilation_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
            println!("* [Ventilation] checking reaction to high temperature succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

// Creates a mock Relay Manager thread for `test_ventilation_with_measured_temperature_low`.
//
// This private helper function spawns a new thread that acts as a mock for the `RelayManager`.
// It's specifically configured for the `test_ventilation_with_measured_temperature_low` test case,
// expecting a precise sequence of actuation commands from the `Ventilation` test object:
// 1.  An `InternalCommand::SwitchOff(AquariumDevice::Ventilation)` command.
// 2.  An `InternalCommand::SwitchOn(AquariumDevice::Ventilation)` command.
//
// The mock then asserts that these commands are received in the correct order and type.
//
// # Arguments
// * `tx_relay_manager_to_ventilation` - The sender channel for this mock to send acknowledgments back to the `Ventilation` test object.
// * `rx_relay_manager_from_ventilation` - The receiver channel for this mock to get actuation commands from the `Ventilation` test object.
//
// # Returns
// A `thread::JoinHandle<()>` which can be used by the main test thread to wait for this mock thread's completion.
//
// # Panics
// This function will panic if:
// - The new thread cannot be spawned.
// - The expected commands are not received in the correct order or type.
// - Any other unexpected command is received.
/// Performs the `create_mock_relay_manager` operation.
///
/// # Arguments
/// * `tx_relay_manager_to_ventilation` - A parameter representing `tx_relay_manager_to_ventilation`.
/// * `rx_relay_manager_from_ventilation` - A parameter representing `rx_relay_manager_from_ventilation`.
fn create_mock_relay_manager(
    mut tx_relay_manager_to_ventilation: AquaSender<bool>,
    mut rx_relay_manager_from_ventilation: AquaReceiver<InternalCommand>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut tx_relay_manager_to_ventilation,
                &mut rx_relay_manager_from_ventilation,
            );
            assert_eq!(actuation_events.len(), 2);
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOn(AquariumDevice::Ventilation)
            );
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOff(AquariumDevice::Ventilation)
            );
            mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::On);
        })
        .unwrap()
}

// Check if the ventilation is switched off with low temperature and then switched on during application termination (if configured).
// This test also implicitly verifies that the execute function terminates after receiving Quit and Terminate commands.
#[test]
// Verifies that ventilation with measured temperature low.
pub fn test_ventilation_with_measured_temperature_low() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut ventilation) = prepare_ventilation_tests(None);

    // replacement value is used for initializing mutex
    config.sensor_manager.replacement_value_water_temperature = 20.0;

    let mut channels = Channels::new_for_test();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                None,
                true,
            );
        })
        .unwrap();

    // Mutex needed for water temperature
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));

    let mutex_device_scheduler_ventilation = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = create_mock_relay_manager(
        channels.relay_manager.tx_relay_manager_to_ventilation,
        channels.relay_manager.rx_relay_manager_from_ventilation,
    );

    // thread for controlling duration of test run
    let join_handle_test_environment = create_test_environment(
        58,
        channels.signal_handler,
        None, // no blocking of mutex in this test case
    );

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                .ventilation
                .tx_ventilation_to_schedule_check
                .clone();
            let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                channels.ventilation.tx_ventilation_to_relay_manager.clone();

            let mut ventilation_set_value_updater =
                MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

            let ventilation_mutexes = VentilationMutexes {
                mutex_sensor_manager_signals,
                mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut ventilation,
                mutex_device_scheduler_ventilation.clone(),
                &mut channels.ventilation,
                &mut ventilation_set_value_updater,
                ventilation_mutexes,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ =
                tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ =
                tx_ventilation_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
            println!("* [Ventilation] checking reaction to low temperature.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

// Check if the ventilation is switched on and off during two cycles of the saw tooth profile.
// Duration of ventilation switched on shall equal the duration of ventilation switched off (1% tolerance).
// Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
#[test]
// Verifies that ventilation with measured temperature 50 percent.
pub fn test_ventilation_with_measured_temperature_50_percent() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut ventilation) =
        prepare_ventilation_tests(Some(SawToothProfileConfig::new(6, 1, 6, 1)));

    println!(
        "Temperature control range is {} deg C to {} deg C.",
        config.ventilation.switch_off_temperature, config.ventilation.switch_on_temperature
    );
    // replacement value is used for initializing mutex
    config.sensor_manager.replacement_value_water_temperature = 25.0;
    let mut channels = Channels::new_for_test();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                None,
                true,
            );
        })
        .unwrap();

    // Mutex needed for water temperature
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));

    let mutex_device_scheduler_ventilation = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_ventilation,
                &mut channels.relay_manager.rx_relay_manager_from_ventilation,
            );
            println!(
                "Mock relay manager received {} commands:",
                actuation_events.len()
            );
            for actuation_event in actuation_events.clone() {
                println!("{}", actuation_event);
            }
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOn(AquariumDevice::Ventilation)
            );
            for actuation_event in actuation_events.clone() {
                println!("{}", actuation_event);
            }
            // (first and) last actuation events need to be ignored
            let _last_actuation_event = actuation_events.pop().unwrap();
            let second_last_actuation_event = actuation_events.pop().unwrap();
            let third_last_actuation_event = actuation_events.pop().unwrap();
            let fourth_last_actuation_event = actuation_events.pop().unwrap();
            let fifth_last_actuation_event = actuation_events.pop().unwrap();
            let sixth_last_actuation_event = actuation_events.pop().unwrap();
            let seventh_last_actuation_event = actuation_events.pop().unwrap();

            let mut min_actuation_duration: u128 = 100000;
            let mut max_actuation_duration: u128 = 0;

            let second_last_actuation_duration = second_last_actuation_event
                .time
                .duration_since(third_last_actuation_event.time)
                .as_millis();

            println!(
                "second_last_actuation_duration={}",
                second_last_actuation_duration
            );

            min_actuation_duration = min_actuation_duration.min(second_last_actuation_duration);
            max_actuation_duration = max_actuation_duration.max(second_last_actuation_duration);

            let third_last_actuation_duration = third_last_actuation_event
                .time
                .duration_since(fourth_last_actuation_event.time)
                .as_millis();

            println!(
                "third_last_actuation_duration={}",
                third_last_actuation_duration
            );

            min_actuation_duration = min_actuation_duration.min(third_last_actuation_duration);
            max_actuation_duration = max_actuation_duration.max(third_last_actuation_duration);

            let fourth_last_actuation_duration = fourth_last_actuation_event
                .time
                .duration_since(fifth_last_actuation_event.time)
                .as_millis();

            println!(
                "fourth_last_actuation_duration={}",
                fourth_last_actuation_duration
            );

            min_actuation_duration = min_actuation_duration.min(fourth_last_actuation_duration);
            max_actuation_duration = max_actuation_duration.max(fourth_last_actuation_duration);

            let fifth_last_actuation_duration = fifth_last_actuation_event
                .time
                .duration_since(sixth_last_actuation_event.time)
                .as_millis();

            println!(
                "fifth_last_actuation_duration={}",
                fifth_last_actuation_duration
            );

            min_actuation_duration = min_actuation_duration.min(fifth_last_actuation_duration);
            max_actuation_duration = max_actuation_duration.max(fifth_last_actuation_duration);

            let sixth_last_actuation_duration = sixth_last_actuation_event
                .time
                .duration_since(seventh_last_actuation_event.time)
                .as_millis();

            println!(
                "sixth_last_actuation_duration={}",
                sixth_last_actuation_duration
            );

            min_actuation_duration = min_actuation_duration.min(sixth_last_actuation_duration);
            max_actuation_duration = max_actuation_duration.max(sixth_last_actuation_duration);

            let delta_min_max_duration = max_actuation_duration - min_actuation_duration;

            assert_le!(second_last_actuation_duration, 6600);
            assert_ge!(second_last_actuation_duration, 6400);
            assert_le!(third_last_actuation_duration, 6600);
            assert_ge!(third_last_actuation_duration, 6400);
            assert_le!(fourth_last_actuation_duration, 6600);
            assert_ge!(fourth_last_actuation_duration, 6400);
            assert_le!(fifth_last_actuation_duration, 6600);
            assert_ge!(fifth_last_actuation_duration, 6400);
            assert_le!(sixth_last_actuation_duration, 6600);
            assert_ge!(sixth_last_actuation_duration, 6400);

            println!("delta_min_max_duration={}", delta_min_max_duration);
            assert_le!(delta_min_max_duration, 50);
            mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::On);
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = create_test_environment(
        58,
        channels.signal_handler,
        None, // no blocking of mutex in this test case
    );

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                .ventilation
                .tx_ventilation_to_schedule_check
                .clone();
            let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                channels.ventilation.tx_ventilation_to_relay_manager.clone();

            let mut ventilation_set_value_updater =
                MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

            let ventilation_mutexes = VentilationMutexes {
                mutex_sensor_manager_signals,
                mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut ventilation,
                mutex_device_scheduler_ventilation.clone(),
                &mut channels.ventilation,
                &mut ventilation_set_value_updater,
                ventilation_mutexes,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ =
                tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ =
                tx_ventilation_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
            println!("* [Ventilation] checking reaction to medium temperature succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

// Check if the ventilation is switched on for most of the time if the temperature is high within the interval.
// Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
#[test]
// Verifies that ventilation with measured temperature high within interval.
pub fn test_ventilation_with_measured_temperature_high_within_interval() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut ventilation) =
        prepare_ventilation_tests(Some(SawToothProfileConfig::new(6, 1, 6, 1)));

    // replacement value is used for initializing mutex
    config.sensor_manager.replacement_value_water_temperature = 27.5;
    let mut channels = Channels::new_for_test();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                None,
                true,
            );
        })
        .unwrap();

    // Mutex needed for water temperature
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));

    let mutex_device_scheduler_ventilation = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_ventilation,
                &mut channels.relay_manager.rx_relay_manager_from_ventilation,
            );
            println!(
                "Mock relay manager received {} commands:",
                actuation_events.len()
            );
            for actuation_event in actuation_events.clone() {
                println!("{}", actuation_event);
            }
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOn(AquariumDevice::Ventilation)
            );
            for actuation_event in actuation_events.clone() {
                println!("{}", actuation_event);
            }
            // (first and) last actuation events need to be ignored
            let _last_actuation_event = actuation_events.pop().unwrap();
            let second_last_actuation_event = actuation_events.pop().unwrap();
            let third_last_actuation_event = actuation_events.pop().unwrap();
            let fourth_last_actuation_event = actuation_events.pop().unwrap();
            let fifth_last_actuation_event = actuation_events.pop().unwrap();
            let sixth_last_actuation_event = actuation_events.pop().unwrap();
            let seventh_last_actuation_event = actuation_events.pop().unwrap();

            let second_last_actuation_duration = second_last_actuation_event
                .time
                .duration_since(third_last_actuation_event.time)
                .as_millis();

            println!(
                "second_last_actuation_duration={}, Command={}",
                second_last_actuation_duration, second_last_actuation_event.command
            );

            let third_last_actuation_duration = third_last_actuation_event
                .time
                .duration_since(fourth_last_actuation_event.time)
                .as_millis();

            println!(
                "third_last_actuation_duration={}, Command={}",
                third_last_actuation_duration, third_last_actuation_event.command
            );

            let fourth_last_actuation_duration = fourth_last_actuation_event
                .time
                .duration_since(fifth_last_actuation_event.time)
                .as_millis();

            println!(
                "fourth_last_actuation_duration={}, Command={}",
                fourth_last_actuation_duration, fourth_last_actuation_event.command
            );

            let fifth_last_actuation_duration = fifth_last_actuation_event
                .time
                .duration_since(sixth_last_actuation_event.time)
                .as_millis();

            println!(
                "fifth_last_actuation_duration={}, Command={}",
                fifth_last_actuation_duration, fifth_last_actuation_event.command
            );

            let sixth_last_actuation_duration = sixth_last_actuation_event
                .time
                .duration_since(seventh_last_actuation_event.time)
                .as_millis();

            println!(
                "sixth_last_actuation_duration={}, Command={}",
                sixth_last_actuation_duration, sixth_last_actuation_event.command
            );

            assert_le!(second_last_actuation_duration, 1100);
            assert_ge!(second_last_actuation_duration, 900);
            assert_le!(third_last_actuation_duration, 12150);
            assert_ge!(third_last_actuation_duration, 11950);
            assert_le!(fourth_last_actuation_duration, 1100);
            assert_ge!(fourth_last_actuation_duration, 900);
            assert_le!(fifth_last_actuation_duration, 12160);
            assert_ge!(fifth_last_actuation_duration, 11950);
            assert_le!(sixth_last_actuation_duration, 1100);
            assert_ge!(sixth_last_actuation_duration, 900);

            mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::On);
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = create_test_environment(
        58,
        channels.signal_handler,
        None, // no blocking of mutex in this test case
    );

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                .ventilation
                .tx_ventilation_to_schedule_check
                .clone();
            let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                channels.ventilation.tx_ventilation_to_relay_manager.clone();

            let mut ventilation_set_value_updater =
                MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

            let ventilation_mutexes = VentilationMutexes {
                mutex_sensor_manager_signals,
                mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut ventilation,
                mutex_device_scheduler_ventilation.clone(),
                &mut channels.ventilation,
                &mut ventilation_set_value_updater,
                ventilation_mutexes,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ =
                tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ =
                tx_ventilation_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
            println!("* [Ventilation] checking reaction to medium temperature succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

// Check if the ventilation is switched off for most of the time if the temperature is low within the interval.
// Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
#[test]
// Verifies that ventilation with measured temperature low within interval.
pub fn test_ventilation_with_measured_temperature_low_within_interval() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut ventilation) =
        prepare_ventilation_tests(Some(SawToothProfileConfig::new(6, 1, 6, 1)));

    // replacement value is used for initializing mutex
    config.sensor_manager.replacement_value_water_temperature = 22.5;
    let mut channels = Channels::new_for_test();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                None,
                true,
            );
        })
        .unwrap();

    // Mutex needed for water temperature
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));

    let mutex_device_scheduler_ventilation = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_ventilation,
                &mut channels.relay_manager.rx_relay_manager_from_ventilation,
            );
            println!(
                "Mock relay manager received {} commands:",
                actuation_events.len()
            );
            for actuation_event in actuation_events.clone() {
                println!("{}", actuation_event);
            }
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOn(AquariumDevice::Ventilation)
            );
            for actuation_event in actuation_events.clone() {
                println!("{}", actuation_event);
            }
            // (first and) last actuation events need to be ignored
            let _last_actuation_event = actuation_events.pop().unwrap();
            let second_last_actuation_event = actuation_events.pop().unwrap();
            let third_last_actuation_event = actuation_events.pop().unwrap();
            let fourth_last_actuation_event = actuation_events.pop().unwrap();
            let fifth_last_actuation_event = actuation_events.pop().unwrap();
            let sixth_last_actuation_event = actuation_events.pop().unwrap();
            let seventh_last_actuation_event = actuation_events.pop().unwrap();

            let second_last_actuation_duration = second_last_actuation_event
                .time
                .duration_since(third_last_actuation_event.time)
                .as_millis();

            println!(
                "second_last_actuation_duration={}, Command={}",
                second_last_actuation_duration, second_last_actuation_event.command
            );

            let third_last_actuation_duration = third_last_actuation_event
                .time
                .duration_since(fourth_last_actuation_event.time)
                .as_millis();

            println!(
                "third_last_actuation_duration={}, Command={}",
                third_last_actuation_duration, third_last_actuation_event.command
            );

            let fourth_last_actuation_duration = fourth_last_actuation_event
                .time
                .duration_since(fifth_last_actuation_event.time)
                .as_millis();

            println!(
                "fourth_last_actuation_duration={}, Command={}",
                fourth_last_actuation_duration, fourth_last_actuation_event.command
            );

            let fifth_last_actuation_duration = fifth_last_actuation_event
                .time
                .duration_since(sixth_last_actuation_event.time)
                .as_millis();

            println!(
                "fifth_last_actuation_duration={}, Command={}",
                fifth_last_actuation_duration, fifth_last_actuation_event.command
            );

            let sixth_last_actuation_duration = sixth_last_actuation_event
                .time
                .duration_since(seventh_last_actuation_event.time)
                .as_millis();

            println!(
                "sixth_last_actuation_duration={}, Command={}",
                sixth_last_actuation_duration, sixth_last_actuation_event.command
            );

            /*
            assert_le!(second_last_actuation_duration, 1100);
            assert_ge!(second_last_actuation_duration, 900);
            assert_le!(third_last_actuation_duration, 12150);
            assert_ge!(third_last_actuation_duration, 11950);
            assert_le!(fourth_last_actuation_duration, 1100);
            assert_ge!(fourth_last_actuation_duration, 900);
            assert_le!(fifth_last_actuation_duration, 12150);
            assert_ge!(fifth_last_actuation_duration, 11950);
            assert_le!(sixth_last_actuation_duration, 1100);
            assert_ge!(sixth_last_actuation_duration, 900);
            */
            mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::On);
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = create_test_environment(
        58,
        channels.signal_handler,
        None, // no blocking of mutex in this test case
    );

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                .ventilation
                .tx_ventilation_to_schedule_check
                .clone();
            let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                channels.ventilation.tx_ventilation_to_relay_manager.clone();

            let mut ventilation_set_value_updater =
                MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

            let ventilation_mutexes = VentilationMutexes {
                mutex_sensor_manager_signals,
                mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut ventilation,
                mutex_device_scheduler_ventilation.clone(),
                &mut channels.ventilation,
                &mut ventilation_set_value_updater,
                ventilation_mutexes,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ =
                tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ =
                tx_ventilation_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
            println!("* [Ventilation] checking reaction to medium temperature succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

// Check if ventilation control is only actuating when quitting while schedule check provides a negative result.
#[test]
// Verifies that ventilation block by schedule.
pub fn test_ventilation_block_by_schedule() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut ventilation) = prepare_ventilation_tests(None);

    // replacement value is used for initializing mutex
    config.sensor_manager.replacement_value_water_temperature = 30.0;

    let mut channels = Channels::new_for_test();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                None,
                false,
            );
        })
        .unwrap();

    // Mutex needed for water temperature
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));

    let mutex_device_scheduler_ventilation = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = create_mock_relay_manager(
        channels.relay_manager.tx_relay_manager_to_ventilation,
        channels.relay_manager.rx_relay_manager_from_ventilation,
    );

    // thread for controlling duration of test run
    let join_handle_test_environment = create_test_environment(
        10,
        channels.signal_handler,
        None, // no blocking of mutex in this test case
    );

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                .ventilation
                .tx_ventilation_to_schedule_check
                .clone();
            let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                channels.ventilation.tx_ventilation_to_relay_manager.clone();

            let mut ventilation_set_value_updater =
                MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

            let ventilation_mutexes = VentilationMutexes {
                mutex_sensor_manager_signals,
                mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut ventilation,
                mutex_device_scheduler_ventilation.clone(),
                &mut channels.ventilation,
                &mut ventilation_set_value_updater,
                ventilation_mutexes,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ =
                tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ =
                tx_ventilation_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

            let actuation_count = *mutex_device_scheduler_ventilation.blocking_lock();

            // check if ventilation is only actuated once (when quitting)
            assert_eq!(actuation_count, 2);

            println!("* [Ventilation] checking if schedule checker can block actuation.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

// Test case runs ventilation control and triggers inhibition by sending the stop message via the channel.
// After verification that ventilation is stopped,
// The test case sends the start message via the channel and verifies if ventilation control
// resumes operation.
#[test]
// Verifies that messaging stops starts ventilation.
/// [itest->swreq~thermal_remote_ventilation_stop~1]
///
/// [itest->swreq~thermal_remote_ventilation_start~1]
pub fn test_messaging_stops_starts_ventilation() {
    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

    // replacement value is used for initialization of mutex
    config.sensor_manager.replacement_value_water_temperature = 30.0;

    // Mutex for water temperature normally coming from Atlas Scientific
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));

    let mut ventilation = Ventilation::new(config.ventilation);

    let mut channels = Channels::new_for_test();

    let mutex_device_scheduler_ventilation = Arc::new(tokio::sync::Mutex::new(0));
    let mutex_device_scheduler_test_environment = mutex_device_scheduler_ventilation.clone();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                None,
                true,
            );
        })
        .unwrap();

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_ventilation,
                &mut channels.relay_manager.rx_relay_manager_from_ventilation,
            );
            println!("actuation_events:");
            for actuation_event in &actuation_events {
                println!("{}", actuation_event);
            }
            assert_eq!(actuation_events.len(), 5);
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOn(AquariumDevice::Ventilation)
            );
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOff(AquariumDevice::Ventilation)
            );
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOn(AquariumDevice::Ventilation)
            );
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOff(AquariumDevice::Ventilation)
            );
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOn(AquariumDevice::Ventilation)
            );
            mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::On);
        })
        .unwrap();

    // thread for test environment
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_100_millis = Duration::from_millis(100);
            let spin_sleeper = SpinSleeper::default();

            // initial wait so that ventilation control can switch on
            for _ in 0..10 {
                spin_sleeper.sleep(sleep_duration_100_millis);
            }
            // check the initial state of mutex: should be already switched on
            assert_eq!(*mutex_device_scheduler_test_environment.blocking_lock(), 1);

            // sending a message requesting stop of ventilation control
            match channels
                .messaging
                .tx_messaging_to_ventilation
                .send(InternalCommand::Stop)
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: error when sending stop command to test object ({e:?})",
                        module_path!()
                    );
                }
            }

            // wait for 1 second: Ventilation control will switch off
            for _ in 0..10 {
                spin_sleeper.sleep(sleep_duration_100_millis);
            }
            // check if ventilation is actuated
            assert_eq!(*mutex_device_scheduler_test_environment.blocking_lock(), 2);

            // sending a message requesting restart of ventilation control
            match channels
                .messaging
                .tx_messaging_to_ventilation
                .send(InternalCommand::Start)
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: error when sending start command to test object ({e:?})",
                        module_path!()
                    );
                }
            }

            // Wait for 1 second. Ventilation control will switch on
            for _ in 0..10 {
                spin_sleeper.sleep(sleep_duration_100_millis);
            }
            // check if ventilation is actuated
            assert_eq!(*mutex_device_scheduler_test_environment.blocking_lock(), 3);

            // sending a message requesting stop of ventilation control
            match channels
                .messaging
                .tx_messaging_to_ventilation
                .send(InternalCommand::Stop)
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: error when sending stop command to test object ({e:?})",
                        module_path!()
                    );
                }
            }

            // wait for 1 second: Ventilation control will switch off
            for _ in 0..10 {
                spin_sleeper.sleep(sleep_duration_100_millis);
            }
            // check if ventilation is actuated
            assert_eq!(*mutex_device_scheduler_test_environment.blocking_lock(), 4);

            // requesting ventilation control to quit
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_ventilation
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_ventilation
                .blocking_recv()
                .unwrap();

            // check if ventilation is actuated
            assert_eq!(*mutex_device_scheduler_test_environment.blocking_lock(), 5);
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_ventilation
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                .ventilation
                .tx_ventilation_to_schedule_check
                .clone();
            let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                channels.ventilation.tx_ventilation_to_relay_manager.clone();

            let mut ventilation_set_value_updater =
                MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

            let ventilation_mutexes = VentilationMutexes {
                mutex_sensor_manager_signals,
                mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut ventilation,
                mutex_device_scheduler_ventilation.clone(),
                &mut channels.ventilation,
                &mut ventilation_set_value_updater,
                ventilation_mutexes,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ =
                tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ =
                tx_ventilation_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

            println!("* [Ventilation] checking if messaging can block and restart actuation.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

// Check if the ventilation is switched on when the temperature is high.
// After a limited time period, increase the set values and observe if the heater is switched off.
// Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
// Test case uses test database #52.
#[test]
// Verifies that ventilation with decreased set values.
pub fn test_ventilation_with_decreased_set_values() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut ventilation) = prepare_ventilation_tests(None);

    // replacement value is used for initializing mutex
    config.sensor_manager.replacement_value_water_temperature = 30.0;

    let mut channels = Channels::new_for_test();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                None,
                true,
            );
        })
        .unwrap();

    // Mutex needed for water temperature
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));

    let mutex_device_scheduler_ventilation = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_ventilation,
                &mut channels.relay_manager.rx_relay_manager_from_ventilation,
            );
            assert_eq!(actuation_events.len(), 3);
            let actuation_event_3 = actuation_events.pop().unwrap();
            let actuation_event_2 = actuation_events.pop().unwrap();
            let actuation_event_1 = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event_1.command,
                InternalCommand::SwitchOn(AquariumDevice::Ventilation)
            );
            assert_eq!(
                actuation_event_2.command,
                InternalCommand::SwitchOff(AquariumDevice::Ventilation)
            );
            assert_eq!(
                actuation_event_3.command,
                InternalCommand::SwitchOn(AquariumDevice::Ventilation)
            );
            mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::On);
        })
        .unwrap();

    // thread for controlling duration of test run
    let join_handle_test_environment = create_test_environment(
        12,
        channels.signal_handler,
        None, // no blocking of mutex in this test case
    );

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                .ventilation
                .tx_ventilation_to_schedule_check
                .clone();
            let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                channels.ventilation.tx_ventilation_to_relay_manager.clone();

            // set up the set value updater to actively manipulate the set values
            let mut ventilation_set_value_updater = MockSqlInterfaceVentilationSetVals::new(
                true,
                Some(30.0),
                Some(40.0),
                Some(Duration::from_secs(10)),
            );

            let ventilation_mutexes = VentilationMutexes {
                mutex_sensor_manager_signals,
                mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut ventilation,
                mutex_device_scheduler_ventilation.clone(),
                &mut channels.ventilation,
                &mut ventilation_set_value_updater,
                ventilation_mutexes,
            );

            // send Quit signal to mock threads because the test object has terminated
            let _ =
                tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ =
                tx_ventilation_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
            println!("* [Ventilation] checking reaction to high temperature succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

// helper function to avoid warning about duplicated code.
/// Performs the `create_mock_relay_manager_for_ventilation_on` operation.
///
/// # Arguments
/// * `tx_relay_manager_to_ventilation` - A parameter representing `tx_relay_manager_to_ventilation`.
/// * `rx_relay_manager_from_ventilation` - A parameter representing `rx_relay_manager_from_ventilation`.
fn create_mock_relay_manager_for_ventilation_on(
    mut tx_relay_manager_to_ventilation: AquaSender<bool>,
    mut rx_relay_manager_from_ventilation: AquaReceiver<InternalCommand>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut tx_relay_manager_to_ventilation,
                &mut rx_relay_manager_from_ventilation,
            );
            assert_eq!(actuation_events.len(), 1);
            let actuation_event = actuation_events.pop().unwrap();
            assert_eq!(
                actuation_event.command,
                InternalCommand::SwitchOn(AquariumDevice::Ventilation)
            );
            mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::On);
        })
        .unwrap()
}

// Check if the ventilation is switched on when the temperature is high.
// Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
#[test]
// Verifies that ventilation with measured temperature high with blocking mutex.
pub fn test_ventilation_with_measured_temperature_high_with_blocking_mutex() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut ventilation) = prepare_ventilation_tests(None);

    // replacement value is used for initializing mutex
    config.sensor_manager.replacement_value_water_temperature = 30.0;

    let mut channels = Channels::new_for_test();

    let mutex_ventilation_status = Arc::new(Mutex::new(ThermalControlStatus::default()));
    let mutex_ventilation_status_clone_for_test_environment = mutex_ventilation_status.clone();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                None,
                true,
            );
        })
        .unwrap();

    // Mutex needed for water temperature
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));

    let mutex_device_scheduler_ventilation = Arc::new(tokio::sync::Mutex::new(0));

    // thread for mock relay manager - includes assertions
    let join_handle_mock_relay_manager = create_mock_relay_manager_for_ventilation_on(
        channels.relay_manager.tx_relay_manager_to_ventilation,
        channels.relay_manager.rx_relay_manager_from_ventilation,
    );

    // thread for controlling duration of test run
    let join_handle_test_environment = create_test_environment(
        10,
        channels.signal_handler,
        Some(mutex_ventilation_status_clone_for_test_environment), // blocking of mutex in this test case
    );

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
            let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                .ventilation
                .tx_ventilation_to_schedule_check
                .clone();
            let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                channels.ventilation.tx_ventilation_to_relay_manager.clone();

            let mut ventilation_set_value_updater =
                MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

            let ventilation_mutexes = VentilationMutexes {
                mutex_sensor_manager_signals,
                mutex_ventilation_status,
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut ventilation,
                mutex_device_scheduler_ventilation.clone(),
                &mut channels.ventilation,
                &mut ventilation_set_value_updater,
                ventilation_mutexes,
            );

            assert!(ventilation.mutex_access_duration_exceeded);

            // send Quit signal to mock threads because the test object has terminated
            let _ =
                tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ =
                tx_ventilation_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
            println!("* [Ventilation] checking reaction to high temperature succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check did not finish.");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager thread did not finish.");
    join_handle_test_environment
        .join()
        .expect("Test environment thread did not finish.");
    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");
}

#[test]
// Verifies that ventilation gradient monitoring disables actuator.
pub fn test_ventilation_gradient_monitoring_disables_actuator() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut ventilation) = prepare_ventilation_tests(None);

    // Configure gradient monitoring to be very aggressive
    config.ventilation.max_time_actuator_effect_monitor = 2;
    config.ventilation.switch_off_when_no_effect = true;
    config.ventilation.switch_on_when_terminating = false;
    ventilation.config = config.ventilation;

    // replacement value is used for initializing mutex - needs to start with a high value
    config.sensor_manager.replacement_value_water_temperature = 30.0;

    let mut channels = Channels::new_for_test();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                None,
                true,
            );
        })
        .unwrap();

    // Mutex needed for water temperature
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));

    let mutex_device_scheduler_ventilation = Arc::new(tokio::sync::Mutex::new(0));

    // Mutex for ventilation status to check assertions
    let mutex_ventilation_status = Arc::new(Mutex::new(ThermalControlStatus::default()));
    let mutex_ventilation_status_clone = mutex_ventilation_status.clone();

    // Mutex for temperature gradient
    let mutex_temperature_gradient = Arc::new(Mutex::new(-0.1)); // Start with cooling gradient
    let mutex_temperature_gradient_clone = mutex_temperature_gradient.clone();

    // thread for mock relay manager
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_ventilation,
                &mut channels.relay_manager.rx_relay_manager_from_ventilation,
            );
            // Expect On, Off
            assert_eq!(actuation_events.len(), 2);
            assert_eq!(
                actuation_events[0].command,
                InternalCommand::SwitchOn(AquariumDevice::Ventilation)
            );
            assert_eq!(
                actuation_events[1].command,
                InternalCommand::SwitchOff(AquariumDevice::Ventilation)
            );

            mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::Off);
        })
        .unwrap();

    // thread for controlling test phases
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_phase = Duration::from_secs(5);
            let spin_sleeper = SpinSleeper::default();

            // Phase 1: High temp, good gradient. Wait 5s.
            spin_sleeper.sleep(sleep_duration_phase);
            {
                let status = mutex_ventilation_status_clone.lock().unwrap();
                assert!(
                    status.actuator_switched_on,
                    "Phase 1: Ventilation should be ON"
                );
            }

            // Phase 2: Constant temp (gradient indicates that actuator is ineffective).
            {
                let mut gradient = mutex_temperature_gradient_clone.lock().unwrap();
                *gradient = 0.0;
            }
            // Wait 5s (monitor timeout is 2s)
            spin_sleeper.sleep(sleep_duration_phase);
            {
                let status = mutex_ventilation_status_clone.lock().unwrap();
                assert!(
                    !status.actuator_switched_on,
                    "Phase 2: Ventilation should be OFF due to gradient monitor"
                );
            }

            // Terminate
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_ventilation
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_ventilation
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_ventilation
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                .ventilation
                .tx_ventilation_to_schedule_check
                .clone();
            let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                channels.ventilation.tx_ventilation_to_relay_manager.clone();

            let mut ventilation_set_value_updater =
                MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

            let ventilation_mutexes = VentilationMutexes {
                mutex_sensor_manager_signals,
                mutex_ventilation_status,
                mutex_temperature_gradient,
            };

            execute_blocking(
                &mut ventilation,
                mutex_device_scheduler_ventilation.clone(),
                &mut channels.ventilation,
                &mut ventilation_set_value_updater,
                ventilation_mutexes,
            );

            // send Quit signal to mock threads
            let _ =
                tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ =
                tx_ventilation_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
            println!("* [Ventilation] checking gradient monitoring succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check failed");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager failed");
    join_handle_test_environment
        .join()
        .expect("Test environment failed");
    join_handle_test_object.join().expect("Test object failed");
}

#[test]
// Verifies that ventilation reset errors enables actuator.
pub fn test_ventilation_reset_errors_enables_actuator() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut ventilation) = prepare_ventilation_tests(None);

    // Start in disabled state
    ventilation.actuator_disabled = true;

    // replacement value is used for initializing mutex - needs to start with a high value
    config.sensor_manager.replacement_value_water_temperature = 30.0;
    config.ventilation.switch_off_when_no_effect = true;
    config.ventilation.switch_on_when_terminating = false;
    ventilation.config = config.ventilation;

    let mut channels = Channels::new_for_test();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                None,
                true,
            );
        })
        .unwrap();

    // Mutex needed for water temperature
    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));

    let mutex_device_scheduler_ventilation = Arc::new(tokio::sync::Mutex::new(0));

    // Mutex for ventilation status to check assertions
    let mutex_ventilation_status = Arc::new(Mutex::new(ThermalControlStatus::default()));
    let mutex_ventilation_status_clone = mutex_ventilation_status.clone();

    // Mutex for temperature gradient
    let mutex_temperature_gradient = Arc::new(Mutex::new(0.1)); // Start with positive gradient
    let mutex_temperature_gradient_clone = mutex_temperature_gradient.clone();

    // thread for mock relay manager
    let join_handle_mock_relay_manager = thread::Builder::new()
        .name("mock_relay_manager".to_string())
        .spawn(move || {
            let (actuation_events, mock_actuator_states) = mock_relay_manager(
                &mut channels.relay_manager.tx_relay_manager_to_ventilation,
                &mut channels.relay_manager.rx_relay_manager_from_ventilation,
            );
            // Expect SwitchOn (after reset), SwitchOff (at termination)
            assert_eq!(actuation_events.len(), 2);
            assert_eq!(
                actuation_events[0].command,
                InternalCommand::SwitchOn(AquariumDevice::Ventilation)
            );
            assert_eq!(
                actuation_events[1].command,
                InternalCommand::SwitchOff(AquariumDevice::Ventilation)
            );

            mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::Off);
        })
        .unwrap();

    // thread for controlling test phases
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let sleep_duration_phase = Duration::from_secs(5);
            let spin_sleeper = SpinSleeper::default();

            // Phase 1: Disabled state. Wait 5s.
            spin_sleeper.sleep(sleep_duration_phase);
            {
                let status = mutex_ventilation_status_clone.lock().unwrap();
                assert!(
                    !status.actuator_switched_on,
                    "Phase 1: Ventilation should be OFF (disabled)"
                );
            }

            // Send ResetAllErrors command
            channels
                .messaging
                .tx_messaging_to_ventilation
                .send(InternalCommand::ResetAllErrors)
                .unwrap();

            // Phase 2: Enabled state. Wait 5s.
            spin_sleeper.sleep(sleep_duration_phase);
            {
                let status = mutex_ventilation_status_clone.lock().unwrap();
                assert!(
                    status.actuator_switched_on,
                    "Phase 2: Ventilation should be ON (enabled)"
                );
            }

            // Terminate
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_ventilation
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_ventilation
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_ventilation
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                .ventilation
                .tx_ventilation_to_schedule_check
                .clone();
            let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                channels.ventilation.tx_ventilation_to_relay_manager.clone();

            let mut ventilation_set_value_updater =
                MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

            let ventilation_mutexes = VentilationMutexes {
                mutex_sensor_manager_signals,
                mutex_ventilation_status,
                mutex_temperature_gradient: mutex_temperature_gradient_clone,
            };

            execute_blocking(
                &mut ventilation,
                mutex_device_scheduler_ventilation.clone(),
                &mut channels.ventilation,
                &mut ventilation_set_value_updater,
                ventilation_mutexes,
            );

            // Add a small delay to allow mock_relay_manager to process the final command
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(Duration::from_millis(100));

            // send Quit signal to mock threads
            let _ =
                tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ =
                tx_ventilation_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
            println!("* [Ventilation] checking reset errors enables actuator succeeded.");
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check failed");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager failed");
    join_handle_test_environment
        .join()
        .expect("Test environment failed");
    join_handle_test_object.join().expect("Test object failed");
}

#[test]
// Verifies that ventilation actuation is inhibited when device_scheduler mutex is blocked.
// The test environment thread acquires and holds the device_scheduler mutex lock before ventilation actuation occurs.
// While the lock is held, ventilation actuation is blocked. Once released, ventilation actuation proceeds normally.
// Test case uses test database #71.
pub fn test_ventilation_actuation_with_device_scheduler_mutex_blocked() {
    let sleep_duration_100_millis = Duration::from_millis(100);
    let spin_sleeper = SpinSleeper::default();

    let (mut config, mut ventilation) = prepare_ventilation_tests(None);
    config.sensor_manager.replacement_value_water_temperature = 30.0;

    let mut channels = Channels::new_for_test();

    // thread for mock schedule check
    let join_handle_mock_schedule_check = thread::Builder::new()
        .name("mock_schedule_check".to_string())
        .spawn(move || {
            mock_schedule_check(
                &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                None,
                true,
            );
        })
        .unwrap();

    let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
        &config.sensor_manager,
    )));

    let mutex_device_scheduler_ventilation = Arc::new(tokio::sync::Mutex::new(0));
    let mutex_device_scheduler_environment = mutex_device_scheduler_ventilation.clone();

    // thread for mock relay manager
    let join_handle_mock_relay_manager = create_mock_relay_manager_for_ventilation_on(
        channels.relay_manager.tx_relay_manager_to_ventilation,
        channels.relay_manager.rx_relay_manager_from_ventilation,
    );

    // thread for controlling duration of test run and blocking device_scheduler mutex
    let join_handle_test_environment = thread::Builder::new()
        .name("test_environment".to_string())
        .spawn(move || {
            let spin_sleeper = SpinSleeper::default();

            // 1. Acquire lock on device_scheduler mutex before ventilation actuation executes
            let lock_guard = mutex_device_scheduler_environment.blocking_lock();

            // Hold lock for 3 seconds
            spin_sleeper.sleep(Duration::from_secs(3));

            // Verify count remains 0 while lock is held
            assert_eq!(*lock_guard, 0);

            // 2. Release lock so ventilation actuation can proceed
            drop(lock_guard);

            // Wait 3 seconds for actuation to complete
            spin_sleeper.sleep(Duration::from_secs(3));

            let _ = channels
                .signal_handler
                .tx_signal_handler_to_ventilation
                .send(InternalCommand::Quit);
            channels
                .signal_handler
                .rx_signal_handler_from_ventilation
                .blocking_recv()
                .unwrap();
            let _ = channels
                .signal_handler
                .tx_signal_handler_to_ventilation
                .send(InternalCommand::Terminate);
        })
        .unwrap();

    spin_sleeper.sleep(sleep_duration_100_millis);

    // thread for the test object
    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                .ventilation
                .tx_ventilation_to_schedule_check
                .clone();
            let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                channels.ventilation.tx_ventilation_to_relay_manager.clone();

            let mut ventilation_set_value_updater =
                MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

            let ventilation_mutexes = VentilationMutexes {
                mutex_sensor_manager_signals,
                mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
            };

            execute_blocking(
                &mut ventilation,
                mutex_device_scheduler_ventilation.clone(),
                &mut channels.ventilation,
                &mut ventilation_set_value_updater,
                ventilation_mutexes,
            );

            assert!(*mutex_device_scheduler_ventilation.blocking_lock() >= 1);

            let _ =
                tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            let _ =
                tx_ventilation_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
            println!(
                "* [Ventilation] checking actuation with device scheduler mutex blocked succeeded."
            );
        })
        .unwrap();

    join_handle_mock_schedule_check
        .join()
        .expect("Mock schedule check failed");
    join_handle_mock_relay_manager
        .join()
        .expect("Mock relay manager failed");
    join_handle_test_environment
        .join()
        .expect("Test environment failed");
    join_handle_test_object.join().expect("Test object failed");
}
