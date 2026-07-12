/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::Local;
use log::error;
use spin_sleep::SpinSleeper;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::database::sql_interface_refill::{
    DatabaseInterfaceRefillTrait, DatabaseRefillErrorCode,
};
use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
use crate::utilities::check_mutex_access_duration::CheckMutexAccessDurationTrait;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::water::refill_channels::RefillChannels;
use crate::water::refill_config::RefillConfig;
use crate::water::refill_error_states::RefillErrorStates;

/// allow max. 10 milliseconds for mutex to be blocked by any other thread
const MAX_MUTEX_ACCESS_DURATION_MILLIS: u64 = 10;

/// Trait for the execution of the refill once control has detected that the water level is low.
/// This trait allows running the main control with a mock implementation for testing.
///
/// # Arguments
/// * `refill_channels` - A mutable reference to the `RefillChannels` communication interface.
/// * `refill_errors` - A parameter representing `refill_errors`.
/// * `sql_interface_refill` - Database interface connection for refill operations.
/// * `mutex_tank_level_switch` - Shared mutex for the tank level switch signals.
/// * `mutex_device_scheduler_refill` - Mutex coordinating access to the device scheduler for refill.
pub trait WaterInjectionTrait {
    fn inject_water(
        &mut self,
        refill_channels: &mut RefillChannels,
        refill_errors: &mut RefillErrorStates,
        sql_interface_refill: &mut Box<dyn DatabaseInterfaceRefillTrait + Sync + Send>,
        mutex_tank_level_switch: &Arc<Mutex<TankLevelSwitchSignals>>,
        mutex_device_scheduler_refill: &Arc<Mutex<i32>>,
    ) -> bool;
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Implementation of the WaterInjectionTrait which contains the code for executing the refill once
/// the main control has detected that the water level is low.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     water_injection[Water Injection] --> relay_manager[Relay Manager]
///     relay_manager --> water_injection
///     tank_level_switch[TankLevelSwitch] -.-> water_injection
///     signal_handler[Signal Handler] --> water_injection
/// ```
pub struct WaterInjection {
    /// The volume of water (in Liters) injected during a single control loop cycle.
    /// This value is calculated at initialization from the pump's flow rate and the loop's sleep interval.
    refill_increment: f64,

    /// The extra volume of water to add after the tank level switch first reports `high`.
    /// This acts as a hysteresis to prevent the pump from cycling too frequently.
    additional_refill_volume: f64,

    /// The maximum total volume of water (in Liters) allowed for a single refill operation.
    /// This serves as a safety cutoff to prevent overfilling if the level switch fails.
    max_refill_volume: f64,

    /// A high-precision sleeper utility to pause the control loop for a consistent duration.
    spin_sleeper: SpinSleeper,

    /// The `Duration` for which the control loop sleeps on each iteration (e.g., 100 ms).
    sleep_duration_hundred_milli_sec: Duration,

    /// The sleep interval of the control loop, expressed in seconds as a floating-point number
    /// for use in volume calculations.
    sleep_interval_seconds: f64,

    /// An inhibition flag to prevent flooding the log file with repeated warnings
    /// about excessive mutex access times.
    pub lock_warn_max_mutex_access_duration: bool,

    /// A flag used for testing purposes to record if the mutex access time was ever exceeded.
    /// Unlike the lock flag, this is not reset.
    #[cfg(test)]
    pub mutex_access_duration_exceeded: bool,

    /// The maximum permissible duration for a mutex lock to be held before a warning is issued.
    pub max_mutex_access_duration: Duration,
}

impl WaterInjection {
    /// Creates a new `WaterInjection` instance.
    ///
    /// This constructor initializes the water injection module, setting up parameters
    /// crucial for controlling the refill pump during a water injection operation.
    /// It calculates the `refill_increment` based on pump flow, copies refill limits
    /// from the configuration, and sets up internal timing mechanisms.
    /// All internal "lock" flags (used to prevent log flooding) are initialized to `false`.
    ///
    /// # Arguments
    /// * `config` - A reference to the **`RefillConfig`** struct, which contains
    ///   essential parameters such as `refill_pump_flow`, `additional_refill_volume`,
    ///   and `max_refill_volume`.
    ///
    /// # Returns
    /// A new **`WaterInjection` struct**, ready to perform water injection operations.
    pub fn new(config: &RefillConfig) -> WaterInjection {
        let sleep_interval_millis = 100;
        let sleep_interval_seconds: f64 = (sleep_interval_millis as f64) / (1000f64);

        WaterInjection {
            refill_increment: sleep_interval_seconds * config.refill_pump_flow,
            additional_refill_volume: config.additional_refill_volume,
            max_refill_volume: config.max_refill_volume,
            spin_sleeper: SpinSleeper::default(),
            sleep_duration_hundred_milli_sec: Duration::from_millis(sleep_interval_millis),
            sleep_interval_seconds,
            lock_warn_max_mutex_access_duration: false,
            #[cfg(test)]
            mutex_access_duration_exceeded: false,
            max_mutex_access_duration: Duration::from_millis(MAX_MUTEX_ACCESS_DURATION_MILLIS),
        }
    }
}

impl WaterInjectionTrait for WaterInjection {
    /// Executes the freshwater refill operation, controlling the refill pump and monitoring tank level.
    ///
    /// This function orchestrates the physical water injection. It first switches on the refill pump,
    /// then enters a loop to continuously monitor the tank level switch and check for various
    /// termination conditions:
    /// - Water level reaching `high` (plus `additional_refill_volume`).
    /// - Tank level switch signal becoming `invalid`.
    /// - Total refilled `volume` exceeding `max_refill_volume` (safety timeout).
    /// - A `Quit` command being received from the signal handler.
    ///
    /// During the refill, it periodically communicates the pump status to the `DataLogger`
    /// and logs the final refill event details (timestamp, duration, volume, error code) to the database.
    ///
    /// # Arguments
    /// * `refill_channels` - A mutable reference to a struct containing all necessary communication channels with
    ///   `RelayManager`, `DataLogger`, and `SignalHandler`.
    /// * `refill_errors` - A mutable reference to a `RefillErrors` struct, where specific error
    ///   flags (e.g., `error_switch_stuck_low`, `error_sql_update_failed`) will be set if issues occur during injection.
    /// * `sql_interface_refill` - A mutable reference to the SQL database interface for refill control,
    ///   used to log the refill event.
    /// * `mutex_tank_level_switch_signals` - A mutable reference to the `Arc<Mutex<TankLevelSwitchSignals>>`
    ///   for reading the current tank level switch position and validity.
    ///
    /// # Returns
    /// `true` if the refill operation was **aborted due to a `Quit` command** received from the signal handler;
    /// `false` otherwise (meaning it completed normally or aborted due to other conditions like level reached or errors).
    /// * `mutex_device_scheduler_refill` - Mutex coordinating access to the device scheduler for refill.
    fn inject_water(
        &mut self,
        refill_channels: &mut RefillChannels,
        refill_errors: &mut RefillErrorStates,
        sql_interface_refill: &mut Box<dyn DatabaseInterfaceRefillTrait + Sync + Send>,
        mutex_tank_level_switch_signals: &Arc<Mutex<TankLevelSwitchSignals>>,
        mutex_device_scheduler_refill: &Arc<Mutex<i32>>,
    ) -> bool {
        let mut refill_volume = 0.0;
        let mut refill_time_seconds = 0.0;
        let mut refill_error_code = DatabaseRefillErrorCode::Success;
        let mut additional_refill_volume_executed = 0.0;
        let mut tank_level_switch_position = false;
        let mut tank_level_switch_invalid = false;
        let mut quit_command_received = false;

        {
            // internal scope to limit the lifetime of the mutex lock
            let mut mutex_device_scheduler = mutex_device_scheduler_refill.lock().unwrap();
            // now we are given exclusive access to actuators

            // command start of refill pump
            match refill_channels
                .send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::RefillPump))
            {
                Ok(_) => {
                    // now listen for the response
                    match refill_channels.rx_refill_from_relay_manager.blocking_recv() {
                        Ok(_) => {}
                        Err(e) => {
                            error!(
                                target: module_path!(),
                                "receiving answer from relay manager when switching on refill pump failed ({e:?})"
                            );
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: module_path!(),
                        "channel communication to relay manager when switching on refill pump failed ({e:?})"
                    );
                }
            }

            //*** wait for condition to stop refilling ***
            loop {
                self.spin_sleeper
                    .sleep(self.sleep_duration_hundred_milli_sec);
                refill_time_seconds += self.sleep_interval_seconds;

                let instant_before_locking_mutex = Instant::now();
                let mut instant_after_locking_mutex = Instant::now(); // initialization is overwritten

                // access mutex for reading tank level switch signals
                {
                    match mutex_tank_level_switch_signals.lock() {
                        Ok(c) => {
                            tank_level_switch_position = c.tank_level_switch_position;
                            tank_level_switch_invalid = c.tank_level_switch_invalid;
                            instant_after_locking_mutex = Instant::now();
                        }
                        Err(_) => {
                            // Do nothing, keep values as they are.
                        }
                    }
                }

                // check if access to mutex took too long
                self.check_mutex_access_duration(
                    None,
                    instant_after_locking_mutex,
                    instant_before_locking_mutex,
                );

                if tank_level_switch_position
                    && additional_refill_volume_executed > self.additional_refill_volume
                {
                    // the water level changed to high
                    break;
                }
                if tank_level_switch_invalid {
                    // switch signal is invalid
                    refill_error_code = DatabaseRefillErrorCode::InvalidSwitch;
                    break;
                }

                // calculate volume refilled in this operation
                refill_volume += self.refill_increment;
                if tank_level_switch_position {
                    additional_refill_volume_executed += self.refill_increment;
                }
                (quit_command_received, _, _) = self.process_external_request(
                    &mut refill_channels.rx_refill_from_signal_handler,
                    None,
                );
                if quit_command_received {
                    break;
                }
                if refill_volume > self.max_refill_volume {
                    refill_errors.error_switch_stuck_low = true;
                    refill_error_code = DatabaseRefillErrorCode::StuckLowTimeout;
                    break;
                }
            }

            // command stop of refill pump
            match refill_channels
                .send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::RefillPump))
            {
                Ok(_) => {
                    // now listen for the response
                    match refill_channels.rx_refill_from_relay_manager.blocking_recv() {
                        Ok(_) => {}
                        Err(e) => {
                            error!(
                                target: module_path!(),
                                "receiving answer from relay manager when switching off refill pump failed ({e:?})"
                            );
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: module_path!(),
                        "channel communication to relay manager when switching off refill pump failed ({e:?})"
                    );
                }
            }

            *mutex_device_scheduler = mutex_device_scheduler.saturating_add(1);
        }

        // write the result to the database
        match sql_interface_refill.insert_refill_event(
            Local::now().naive_local(),
            refill_time_seconds,
            refill_volume,
            refill_error_code,
        ) {
            Ok(_) => {}
            Err(e) => {
                error!(
                    target: module_path!(),
                    "writing refill operation data to SQL database failed ({e:?})"
                );
                refill_errors.error_sql_update_failed = true;
            }
        }

        quit_command_received // return information if refill was aborted due to request from signal handler
    }
}

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface_refill::SqlInterfaceRefill;
    use crate::launch::channels::{AquaReceiver, AquaSender, Channels};
    use crate::mocks::mock_relay_manager::tests::mock_relay_manager;
    use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
    use crate::utilities::channel_content::{ActuatorState, AquariumDevice, InternalCommand};
    use crate::water::refill::tests::prepare_refill_tests;
    use crate::water::refill::Refill;
    use crate::water::refill_channels::RefillChannels;
    use crate::water::water_injection::{WaterInjection, WaterInjectionTrait};
    use all_asserts::{assert_ge, assert_le};
    use spin_sleep::SpinSleeper;
    use std::sync::{Arc, Mutex};
    use std::{thread, time::Duration};

    // Creates a thread to run `WaterInjection::inject_water` and assert its outcome.
    //
    // This private helper function simplifies complex test scenarios for `WaterInjection`.
    // It spawns a new thread.
    // 1. It initializes a `WaterInjection` instance using the provided `Refill`'s configuration.
    // 2. It sets up the `WaterInjectionChannels` by borrowing the necessary `mpsc` channels from the test environment.
    // 3. It calls `water_injection.inject_water()` with all its required parameters.
    // 4. It asserts that the `inject_water` function's return value (`result`) matches `target_result`.
    // 5. It asserts that the `refill_errors.has_error()` status matches `target_refill_has_errors` after the operation.
    //
    // # Arguments
    // * `refill_channels` - Struct containing the signals for communication with other threads.
    // * `rx_water_injection_from_relay_manager` - Receiver channel for `WaterInjection` to receive acknowledgments from the mock `RelayManager`.
    // * `rx_water_injection_from_signal_handler` - Receiver channel for `WaterInjection` to receive commands from the mock `SignalHandler`.
    // * `refill` - The `Refill` struct (moved into this thread) whose `refill_errors` and `sql_interface_refill` will be used and modified by `inject_water`.
    // * `mutex_tank_level_switch_signals` - The `Arc<Mutex<TankLevelSwitchSignals>>` providing shared access to mock tank level switch signals.
    // * `target_result` - The expected boolean return value of the `inject_water` call (`true` if `Quit` was received, `false` otherwise).
    // * `target_refill_has_errors` - The expected boolean result of `refill.refill_errors.has_error()` after `inject_water` completes.
    //
    // # Returns
    // A `thread::JoinHandle<()>` which can be used by the main test thread to wait for this spawned thread's completion.
    //
    // # Panics
    // This function will panic if the new thread cannot be spawned, or if any of the `assert_eq!` calls inside the spawned thread fails.
    /// Performs the `create_assert_test_object` operation.
    ///
    /// # Arguments
    /// * `refill_channels` - A mutable reference to the `RefillChannels` communication interface.
    /// * `refill` - A parameter representing `refill`.
    /// * `sql_interface_refill` - Database interface connection for refill operations.
    /// * `mutex_tank_level_switch_signals` - A parameter representing `mutex_tank_level_switch_signals`.
    /// * `target_result` - A parameter representing `target_result`.
    /// * `target_refill_has_errors` - A parameter representing `target_refill_has_errors`.
    fn create_assert_test_object(
        mut refill_channels: RefillChannels,
        mut refill: Refill,
        sql_interface_refill: SqlInterfaceRefill,
        mutex_tank_level_switch_signals: Arc<Mutex<TankLevelSwitchSignals>>,
        target_result: bool,
        target_refill_has_errors: bool,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let mutex_device_scheduler_refill: Arc<Mutex<i32>> = Arc::new(Mutex::new(0));
                let mut water_injection = WaterInjection::new(&refill.config);

                let mut boxed_sql_interface_refill: Box<
                    dyn crate::database::sql_interface_refill::DatabaseInterfaceRefillTrait
                        + Sync
                        + Send,
                > = Box::new(sql_interface_refill);

                let result = water_injection.inject_water(
                    &mut refill_channels,
                    &mut refill.refill_errors,
                    &mut boxed_sql_interface_refill,
                    &mutex_tank_level_switch_signals,
                    &mutex_device_scheduler_refill,
                );
                assert_eq!(result, target_result);
                assert_eq!(refill.refill_errors.has_error(), target_refill_has_errors);
            })
            .unwrap()
    }

    #[test]
    // This test case executes the inject_water-function and immediately sets the sensor signal to high value.
    // Test case uses test database #39.
    pub fn test_water_injection_immediate_switch_high_no_quit_signal() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, refill, _, sql_interface_refill) = prepare_refill_tests(None, 2, 60, Some(39));

        let mut channels = Channels::new_for_test();

        let mut tx_signal_handler_to_relay_manager =
            channels.refill.tx_refill_to_relay_manager.clone();

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_refill,
                    &mut channels.relay_manager.rx_relay_manager_from_refill,
                );
                assert_eq!(actuation_events.len(), 2);
                let last_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    last_actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::RefillPump)
                );
                let first_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    first_actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::RefillPump)
                );
                let actuation_duration = last_actuation_event
                    .time
                    .duration_since(first_actuation_event.time)
                    .as_millis();
                println!("Actuation duration={}", actuation_duration);
                assert_ge!(actuation_duration, 4000);
                assert_le!(actuation_duration, 4500);
                assert_eq!(
                    mock_actuator_states.refill_pump_actuation_state,
                    ActuatorState::Off
                );
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_six_seconds = Duration::from_secs(6);
                let sleep_duration_two_seconds = Duration::from_secs(2);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_six_seconds);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Quit);
                spin_sleeper.sleep(sleep_duration_two_seconds);
                let _ = tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Quit);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = create_assert_test_object(
            channels.refill,
            refill,
            sql_interface_refill,
            mutex_tank_level_switch_signals,
            false,
            false,
        );

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

    /// Performs the `create_mock_relay_manager` operation.
    ///
    /// # Arguments
    /// * `tx_relay_manager_to_water_injection` - Sender channel to communicate relay state changes to water injection.
    /// * `rx_relay_manager_from_water_injection` - Receiver channel to read commands from water injection.
    fn create_mock_relay_manager(
        mut tx_relay_manager_to_water_injection: AquaSender<bool>,
        mut rx_relay_manager_from_water_injection: AquaReceiver<InternalCommand>,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut tx_relay_manager_to_water_injection,
                    &mut rx_relay_manager_from_water_injection,
                );
                assert_eq!(actuation_events.len(), 2);
                let last_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    last_actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::RefillPump)
                );
                let first_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    first_actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::RefillPump)
                );
                let actuation_duration = last_actuation_event
                    .time
                    .duration_since(first_actuation_event.time)
                    .as_millis();
                println!("Actuation duration={}", actuation_duration);
                assert_ge!(actuation_duration, 0);
                assert_le!(actuation_duration, 200);
                assert_eq!(
                    mock_actuator_states.refill_pump_actuation_state,
                    ActuatorState::Off
                );
            })
            .unwrap()
    }

    #[test]
    // This test case executes the inject_water function and immediately sets the sensor signal to high value.
    // Additionally, the Quit signal is sent.
    // Expected behavior: immediate stop of additional refill operation.
    // Test case uses test database #40.
    pub fn test_water_injection_immediate_switch_high_and_quit_signal() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, refill, _, sql_interface_refill) = prepare_refill_tests(None, 2, 60, Some(40));

        let mut channels = Channels::new_for_test();

        let mut tx_text_environment_to_relay_manager =
            channels.refill.tx_refill_to_relay_manager.clone();

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = create_mock_relay_manager(
            channels.relay_manager.tx_relay_manager_to_refill,
            channels.relay_manager.rx_relay_manager_from_refill,
        );

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_seconds = Duration::from_secs(1);
                let spin_sleeper = SpinSleeper::default();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Quit);
                println!("Sent quit signal for refill.");
                spin_sleeper.sleep(sleep_duration_two_seconds);
                let _ = tx_text_environment_to_relay_manager.send(InternalCommand::Quit);
                println!("Sent quit signal for relay manager.");
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = create_assert_test_object(
            channels.refill,
            refill,
            sql_interface_refill,
            mutex_tank_level_switch_signals,
            true,
            false,
        );

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

    // Creates a thread that simulates a test environment for `WaterInjection` scenarios.
    //
    // This private helper function is used in integration tests to control the test's timeline
    // and send specific shutdown signals. It simulates a period of normal operation, then
    // triggers a `Quit` command to the `WaterInjection` module, and finally sends `Quit`
    // commands to other mock threads to ensure a clean test environment shutdown.
    //
    // # Arguments
    // * `tx_signal_handler_to_water_injection` - The sender channel for this mock environment to send
    //   commands to the `WaterInjection` test object, simulating the signal handler.
    // * `tx_signal_handler_to_relay_manager` - The sender channel for this mock environment to send
    //   commands to the mock `RelayManager` thread.
    //
    // # Returns
    // A `thread::JoinHandle<()>` which can be used by the main test thread to wait for this
    // mock environment thread's completion.
    //
    // # Panics
    // This function will panic if:
    // - The new thread cannot be spawned.
    // - It fails to send any of the `InternalCommand::Quit` messages through the channels.
    /// Performs the `create_test_environment` operation.
    ///
    /// # Arguments
    /// * `tx_signal_handler_to_water_injection` - A parameter representing `tx_signal_handler_to_water_injection`.
    /// * `tx_signal_handler_to_relay_manager` - A parameter representing `tx_signal_handler_to_relay_manager`.
    fn create_test_environment(
        mut tx_signal_handler_to_water_injection: AquaSender<InternalCommand>,
        mut tx_signal_handler_to_relay_manager: AquaSender<InternalCommand>,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_seconds = Duration::from_secs(2);
                let spin_sleeper = SpinSleeper::default();

                // wait for the refill operation to inject some water
                spin_sleeper.sleep(sleep_duration_two_seconds);
                let _ = tx_signal_handler_to_water_injection.send(InternalCommand::Quit);

                // wait for the test object to terminate
                spin_sleeper.sleep(sleep_duration_two_seconds);

                // request termination to mock threads
                let _ = tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
            })
            .unwrap()
    }

    #[test]
    // This test case executes the inject_water function and immediately
    // - sets the sensor signal to high value
    // - sets the invalid bit.
    // Water injection shall be aborted without adding further water.
    // Test case uses test database #41.
    pub fn test_water_injection_immediate_switch_high_with_invalid_true() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, refill, _, sql_interface_refill) = prepare_refill_tests(None, 2, 60, Some(41));

        let channels = Channels::new_for_test();

        let tx_signal_handler_to_relay_manager = channels.refill.tx_refill_to_relay_manager.clone();

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, true)));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = create_mock_relay_manager(
            channels.relay_manager.tx_relay_manager_to_refill,
            channels.relay_manager.rx_relay_manager_from_refill,
        );

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            channels.signal_handler.tx_signal_handler_to_refill,
            tx_signal_handler_to_relay_manager,
        );

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = create_assert_test_object(
            channels.refill,
            refill,
            sql_interface_refill,
            mutex_tank_level_switch_signals,
            false,
            false,
        );

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
    // This test case executes the inject_water function and sets the sensor signal
    // to high after some time has passed (the happy case).
    // Test case uses test database #42.
    pub fn test_water_injection_delayed_switch_high() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, refill, _, sql_interface_refill) = prepare_refill_tests(None, 2, 60, Some(42));

        let mut channels = Channels::new_for_test();

        let mut tx_test_environment_to_relay_manager =
            channels.refill.tx_refill_to_relay_manager.clone();

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
        let mutex_tank_level_switch_signals_clone_for_test_environment =
            mutex_tank_level_switch_signals.clone();

        // thread for Mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_refill,
                    &mut channels.relay_manager.rx_relay_manager_from_refill,
                );
                assert_eq!(actuation_events.len(), 2);
                let last_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    last_actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::RefillPump)
                );
                let first_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    first_actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::RefillPump)
                );
                let actuation_duration = last_actuation_event
                    .time
                    .duration_since(first_actuation_event.time)
                    .as_millis();
                println!("Actuation duration={}", actuation_duration);
                assert_ge!(actuation_duration, 5890);
                assert_le!(actuation_duration, 6250);
                assert_eq!(
                    mock_actuator_states.refill_pump_actuation_state,
                    ActuatorState::Off
                );
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let sleep_duration_two_seconds = Duration::from_secs(2);
                let spin_sleeper = SpinSleeper::default();

                // run with tank level switch in low position
                spin_sleeper.sleep(sleep_duration_two_seconds);

                // update tank level switch stabilized signal to high
                {
                    let mut tank_level_switch_signals =
                        mutex_tank_level_switch_signals_clone_for_test_environment
                            .lock()
                            .unwrap();
                    tank_level_switch_signals.tank_level_switch_position_stabilized = true;
                    tank_level_switch_signals.tank_level_switch_position = true;
                }

                // wait for the refill operation to finish
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_refill
                    .send(InternalCommand::Quit);

                // wait for inject_water to terminate
                spin_sleeper.sleep(sleep_duration_two_seconds);

                // request termination to mock threads
                let _ = tx_test_environment_to_relay_manager.send(InternalCommand::Quit);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = create_assert_test_object(
            channels.refill,
            refill,
            sql_interface_refill,
            mutex_tank_level_switch_signals,
            false,
            false,
        );

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
    // This test case executes inject_water function with simulation of level switch stuck at low position.
    // Test case uses test database #43.
    pub fn test_water_injection_switch_stuck_low() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut refill, _, sql_interface_refill) = prepare_refill_tests(None, 2, 60, Some(43));

        let mut water_injection = WaterInjection::new(&refill.config);

        let mut channels = Channels::new_for_test();

        let mut tx_signal_handler_to_relay_manager =
            channels.refill.tx_refill_to_relay_manager.clone();

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));

        // thread for Mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_refill,
                    &mut channels.relay_manager.rx_relay_manager_from_refill,
                );
                assert_eq!(actuation_events.len(), 2);
                let last_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    last_actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::RefillPump)
                );
                let first_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    first_actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::RefillPump)
                );
                let actuation_duration = last_actuation_event
                    .time
                    .duration_since(first_actuation_event.time)
                    .as_millis();
                println!("Actuation duration={}", actuation_duration);
                assert_ge!(actuation_duration, 40000);
                assert_le!(actuation_duration, 43100);
                actuator_states.check_terminal_condition_refill();
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_fourtyfive_seconds = Duration::from_secs(45);
                let spin_sleeper = SpinSleeper::default();

                // wait for the refill operation to abort
                spin_sleeper.sleep(sleep_duration_fourtyfive_seconds);

                // request termination to mock threads
                let _ = tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let mutex_device_scheduler_refill: Arc<Mutex<i32>> = Arc::new(Mutex::new(0));
                let mut boxed_sql_interface_refill: Box<
                    dyn crate::database::sql_interface_refill::DatabaseInterfaceRefillTrait
                        + Sync
                        + Send,
                > = Box::new(sql_interface_refill);

                let result = water_injection.inject_water(
                    &mut channels.refill,
                    &mut refill.refill_errors,
                    &mut boxed_sql_interface_refill,
                    &mutex_tank_level_switch_signals,
                    &mutex_device_scheduler_refill,
                );
                assert_eq!(result, false);
                assert_eq!(refill.refill_errors.has_error(), true);
                assert_eq!(refill.refill_errors.error_switch_stuck_low, true);
            })
            .unwrap();

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
    // This test case executes inject_water function and sends Quit signal
    // after some time when the water level is still low.
    // Test case uses test database #44.
    pub fn test_water_injection_quit_signal() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut refill, _, sql_interface_refill) = prepare_refill_tests(None, 10, 60, Some(44));
        let mut water_injection = WaterInjection::new(&refill.config);

        let mut channels = Channels::new_for_test();

        let tx_signal_handler_to_relay_manager = channels.refill.tx_refill_to_relay_manager.clone();

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));

        // thread for Mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_refill,
                    &mut channels.relay_manager.rx_relay_manager_from_refill,
                );
                assert_eq!(actuation_events.len(), 2);
                let last_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    last_actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::RefillPump)
                );
                let first_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    first_actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::RefillPump)
                );
                let actuation_duration = last_actuation_event
                    .time
                    .duration_since(first_actuation_event.time)
                    .as_millis();
                println!("Actuation duration={}", actuation_duration);
                assert_ge!(actuation_duration, 1900);
                assert_le!(actuation_duration, 2100);
                assert_eq!(
                    mock_actuator_states.refill_pump_actuation_state,
                    ActuatorState::Off
                );
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            channels.signal_handler.tx_signal_handler_to_refill,
            tx_signal_handler_to_relay_manager,
        );

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let mutex_device_scheduler_refill: Arc<Mutex<i32>> = Arc::new(Mutex::new(0));
                let mut boxed_sql_interface_refill: Box<
                    dyn crate::database::sql_interface_refill::DatabaseInterfaceRefillTrait
                        + Sync
                        + Send,
                > = Box::new(sql_interface_refill);
                let result = water_injection.inject_water(
                    &mut channels.refill,
                    &mut refill.refill_errors,
                    &mut boxed_sql_interface_refill,
                    &mutex_tank_level_switch_signals,
                    &mutex_device_scheduler_refill,
                );
                assert_eq!(result, true);
                assert_eq!(refill.refill_errors.has_error(), false);
            })
            .unwrap();

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
}
