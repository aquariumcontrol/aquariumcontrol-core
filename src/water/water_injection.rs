/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::Local;
use log::error;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::database::sql_interface_refill::DatabaseInterfaceRefillTrait;
use crate::recorder::data_logger_communication::DataLoggerCommunicationTrait;
use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::water::refill::RefillConfig;
use crate::water::refill_errors::RefillErrors;

const LOG_TARGET_WATER_INJECTION: &str = "water_injection";

pub struct WaterInjectionChannels<'a> {
    /// Reference to sender part of the channel for communication to the relay manager
    pub tx_water_injection_to_relay_manager: &'a Sender<InternalCommand>,

    /// Reference to Receiver part of the channel for communication from the relay manger
    pub rx_water_injection_from_relay_manager: &'a Receiver<bool>,

    /// Reference to the receiver part of the channel for communication from the signal handler
    pub rx_water_injection_from_signal_handler: &'a Receiver<InternalCommand>,

    /// Sender part of the channel for communication to data logger
    pub tx_water_injection_to_data_logger: &'a Sender<bool>,

    /// Receiver part of the channel for communication to data logger
    pub rx_water_injection_from_data_logger: &'a Receiver<InternalCommand>,
}

/// Trait for the execution of the refill once control has detected that the water level is low.
/// This trait allows running the main control with a mock implementation for testing.
pub trait WaterInjectionTrait {
    /// Actuates the refill pump which injects water into the main tank.
    /// # Arguments
    /// * `refill_errors` - Reference to struct of type RefillErrors which records the errors occurred during water injection
    /// * `sql_interface_refill` - Reference to SQL database interface for refill control
    /// * `mutex_tank_level_switch` - Reference to mutex for reading tank level switch signals
    fn inject_water(
        &mut self,
        water_injection_channels: WaterInjectionChannels,
        refill_errors: &mut RefillErrors,
        sql_interface_refill: &mut Box<dyn DatabaseInterfaceRefillTrait + Sync + Send>,
        mutex_tank_level_switch: &Arc<Mutex<TankLevelSwitchSignals>>,
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
///     water_injection --> data_logger[Data Logger]
///     data_logger --> water_injection
///     water_injection --> tank_level_switch[Tank Level Switch]
///     tank_level_switch --> water_injection
///     signal_handler[Signal Handler] --> water_injection
/// ```
pub struct WaterInjection {
    refill_increment: f64,         // value is calculated based on RefillConfig
    additional_refill_volume: f64, // value is copied from RefillConfig
    max_refill_volume: f64,        // value is copied from RefillConfig
    spin_sleeper: SpinSleeper,
    sleep_duration_hundred_milli_sec: Duration,
    sleep_interval_seconds: f64,
    pub lock_error_channel_receive_data_logger: bool,
    pub lock_warn_inapplicable_command_data_logger: bool,
    pub lock_error_invalid_signal_request_data_logger: bool,
    pub lock_error_send_to_data_logger: bool,
}

impl WaterInjection {
    /// Provide a struct of type WaterInjection
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
            lock_error_channel_receive_data_logger: false,
            lock_warn_inapplicable_command_data_logger: false,
            lock_error_invalid_signal_request_data_logger: false,
            lock_error_send_to_data_logger: false,
        }
    }
}

impl WaterInjectionTrait for WaterInjection {
    /// this function executes the refill operation
    fn inject_water(
        &mut self,
        water_injection_channels: WaterInjectionChannels,
        refill_errors: &mut RefillErrors,
        sql_interface_refill: &mut Box<dyn DatabaseInterfaceRefillTrait + Sync + Send>,
        mutex_tank_level_switch_signals: &Arc<Mutex<TankLevelSwitchSignals>>,
    ) -> bool {
        let mut quit_command_received = false;

        // command start of refill pump
        match water_injection_channels
            .tx_water_injection_to_relay_manager
            .send(InternalCommand::SwitchOn(AquariumDevice::RefillPump))
        {
            Ok(_) => {
                // now listen for the response
                match water_injection_channels
                    .rx_water_injection_from_relay_manager
                    .recv()
                {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_WATER_INJECTION,
                            "receiving answer from Controllino when switching on refill pump failed ({e:?})"
                        );
                    }
                };
            }
            Err(e) => {
                error!(
                    target: LOG_TARGET_WATER_INJECTION,
                    "channel communication to Controllino when switching on refill pump failed ({e:?})"
                );
            }
        }

        //*** wait for condition to stop refilling ***
        let mut refill_volume = 0.0;
        let mut refill_time_seconds = 0.0;
        let mut refill_error_code = 0;
        let mut additional_refill_volume_executed = 0.0;
        let mut tank_level_switch_position = false;
        let mut tank_level_switch_invalid = false;

        loop {
            self.communicate_with_data_logger(
                water_injection_channels.tx_water_injection_to_data_logger,
                water_injection_channels.rx_water_injection_from_data_logger,
                true,
            );

            self.spin_sleeper
                .sleep(self.sleep_duration_hundred_milli_sec);
            refill_time_seconds += self.sleep_interval_seconds;

            // access mutex for reading tank level switch signals
            {
                match mutex_tank_level_switch_signals.lock() {
                    Ok(c) => {
                        tank_level_switch_position = c.tank_level_switch_position;
                        tank_level_switch_invalid = c.tank_level_switch_invalid;
                    }
                    Err(_) => {
                        // Do nothing, keep values as they are.
                    }
                }
            }

            if tank_level_switch_position
                && additional_refill_volume_executed > self.additional_refill_volume
            {
                // the water level changed to high
                break;
            }
            if tank_level_switch_invalid {
                // switch signal is invalid
                refill_error_code = 1;
                break;
            }

            // calculate volume refilled in this operation
            refill_volume += self.refill_increment;
            if tank_level_switch_position {
                additional_refill_volume_executed += self.refill_increment;
            }
            (quit_command_received, _, _) = self.process_external_request(
                water_injection_channels.rx_water_injection_from_signal_handler,
                None,
            );
            if quit_command_received {
                break;
            }
            if refill_volume > self.max_refill_volume {
                refill_errors.error_switch_stuck_low = true;
                refill_error_code = 2;
                break;
            }
        }

        // command stop of refill pump
        match water_injection_channels
            .tx_water_injection_to_relay_manager
            .send(InternalCommand::SwitchOff(AquariumDevice::RefillPump))
        {
            Ok(_) => {
                // now listen for the response
                match water_injection_channels
                    .rx_water_injection_from_relay_manager
                    .recv()
                {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_WATER_INJECTION,
                            "receiving answer from Controllino when switching off refill pump failed ({e:?})"
                        );
                    }
                };
            }
            Err(e) => {
                error!(
                    target: LOG_TARGET_WATER_INJECTION,
                    "channel communication to Controllino when switching off refill pump failed ({e:?})"
                );
            }
        }

        // inform data logger that refill is finished
        match water_injection_channels
            .tx_water_injection_to_data_logger
            .send(false)
        {
            Ok(_) => { /* no listening to response */ }
            Err(e) => {
                error!(
                    target: LOG_TARGET_WATER_INJECTION,
                    "channel communication to data logger when finishing refill failed ({e:?})"
                );
            }
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
                    target: LOG_TARGET_WATER_INJECTION,
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
    use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
    use crate::stubs::mock_data_logger::tests::mock_data_logger;
    use crate::stubs::mock_relay_manager::tests::mock_relay_manager;
    use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
    use crate::water::refill::tests::prepare_refill_tests;
    use crate::water::refill::Refill;
    use crate::water::water_injection::{
        WaterInjection, WaterInjectionChannels, WaterInjectionTrait,
    };
    use all_asserts::{assert_ge, assert_le};
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::{sync::mpsc, thread, time::Duration};

    fn create_assert_test_object(
        tx_water_injection_to_relay_manager: Sender<InternalCommand>,
        rx_water_injection_from_relay_manager: Receiver<bool>,
        rx_water_injection_from_signal_handler: Receiver<InternalCommand>,
        tx_water_injection_to_data_logger: Sender<bool>,
        rx_water_injection_from_data_logger: Receiver<InternalCommand>,
        mut refill: Refill,
        mutex_tank_level_switch_signals: Arc<Mutex<TankLevelSwitchSignals>>,
        target_result: bool,
        target_refill_has_errors: bool,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let mut water_injection = WaterInjection::new(&refill.config);

                let water_injection_channels = WaterInjectionChannels {
                    tx_water_injection_to_data_logger: &tx_water_injection_to_data_logger,
                    rx_water_injection_from_data_logger: &rx_water_injection_from_data_logger,
                    tx_water_injection_to_relay_manager: &tx_water_injection_to_relay_manager,
                    rx_water_injection_from_relay_manager: &rx_water_injection_from_relay_manager,
                    rx_water_injection_from_signal_handler: &rx_water_injection_from_signal_handler,
                };
                let result = water_injection.inject_water(
                    water_injection_channels,
                    &mut refill.refill_errors,
                    &mut refill.sql_interface_refill,
                    &mutex_tank_level_switch_signals,
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

        let (_, refill, _) = prepare_refill_tests(None, 2, 60, Some(39));

        let (tx_water_injection_to_data_logger, _rx_data_logger_from_water_injection) =
            mpsc::channel();
        let (_tx_data_logger_to_water_injection, rx_water_injection_from_data_logger) =
            mpsc::channel();
        let (tx_water_injection_to_relay_manager, rx_relay_manager_from_water_injection) =
            mpsc::channel();
        let (tx_relay_manager_to_water_injection, rx_water_injection_from_relay_manager) =
            mpsc::channel();
        let (tx_signal_handler_to_water_injection, rx_water_injection_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_data_logger, _rx_data_logger_from_signal_handler) =
            mpsc::channel();

        let tx_signal_handler_to_relay_manager = tx_water_injection_to_relay_manager.clone();

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock Controllino - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_water_injection,
                    &rx_relay_manager_from_water_injection,
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
                    .unwrap()
                    .as_millis();
                println!("Actuation duration={}", actuation_duration);
                assert_ge!(actuation_duration, 4000);
                assert_le!(actuation_duration, 4400);
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
                let _ = tx_signal_handler_to_water_injection.send(InternalCommand::Quit);
                spin_sleeper.sleep(sleep_duration_two_seconds);
                let _ = tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Quit);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = create_assert_test_object(
            tx_water_injection_to_relay_manager,
            rx_water_injection_from_relay_manager,
            rx_water_injection_from_signal_handler,
            tx_water_injection_to_data_logger,
            rx_water_injection_from_data_logger,
            refill,
            mutex_tank_level_switch_signals,
            false,
            false,
        );

        join_handle_mock_relay_manager
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    fn create_mock_relay_manager(
        tx_relay_manager_to_water_injection: Sender<bool>,
        rx_relay_manager_from_water_injection: Receiver<InternalCommand>,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_water_injection,
                    &rx_relay_manager_from_water_injection,
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
                    .unwrap()
                    .as_millis();
                println!("Actuation duration={}", actuation_duration);
                assert_ge!(actuation_duration, 0);
                assert_le!(actuation_duration, 200);
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

        let (_, refill, _) = prepare_refill_tests(None, 2, 60, Some(40));

        let (tx_water_injection_to_data_logger, rx_data_logger_from_water_injection) =
            mpsc::channel();
        let (tx_data_logger_to_water_injection, rx_water_injection_from_data_logger) =
            mpsc::channel();
        let (tx_water_injection_to_relay_manager, rx_relay_manager_from_water_injection) =
            mpsc::channel();
        let (tx_relay_manager_to_water_injection, rx_water_injection_from_relay_manager) =
            mpsc::channel();
        let (tx_signal_handler_to_water_injection, rx_water_injection_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        let tx_signal_handler_to_relay_manager = tx_water_injection_to_relay_manager.clone();

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_water_injection,
                    &rx_data_logger_from_water_injection,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        // thread for mock Controllino - includes assertions
        let join_handle_mock_relay_manager = create_mock_relay_manager(
            tx_relay_manager_to_water_injection,
            rx_relay_manager_from_water_injection,
        );

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_seconds = Duration::from_secs(1);
                let spin_sleeper = SpinSleeper::default();
                let _ = tx_signal_handler_to_water_injection.send(InternalCommand::Quit);
                spin_sleeper.sleep(sleep_duration_two_seconds);
                let _ = tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Quit);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = create_assert_test_object(
            tx_water_injection_to_relay_manager,
            rx_water_injection_from_relay_manager,
            rx_water_injection_from_signal_handler,
            tx_water_injection_to_data_logger,
            rx_water_injection_from_data_logger,
            refill,
            mutex_tank_level_switch_signals,
            true,
            false,
        );

        join_handle_mock_relay_manager
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
    }

    fn create_test_environment(
        tx_signal_handler_to_water_injection: Sender<InternalCommand>,
        tx_signal_handler_to_relay_manager: Sender<InternalCommand>,
        tx_signal_handler_to_data_logger: Sender<InternalCommand>,
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
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Quit);
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

        let (_, refill, _) = prepare_refill_tests(None, 2, 60, Some(41));

        let (tx_water_injection_to_data_logger, rx_data_logger_from_water_injection) =
            mpsc::channel();
        let (tx_data_logger_to_water_injection, rx_water_injection_from_data_logger) =
            mpsc::channel();
        let (tx_water_injection_to_relay_manager, rx_relay_manager_from_water_injection) =
            mpsc::channel();
        let (tx_relay_manager_to_water_injection, rx_water_injection_from_relay_manager) =
            mpsc::channel();
        let (tx_signal_handler_to_water_injection, rx_water_injection_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        let tx_signal_handler_to_relay_manager = tx_water_injection_to_relay_manager.clone();

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, true)));

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_water_injection,
                    &rx_data_logger_from_water_injection,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        // thread for mock Controllino - includes assertions
        let join_handle_mock_relay_manager = create_mock_relay_manager(
            tx_relay_manager_to_water_injection,
            rx_relay_manager_from_water_injection,
        );

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            tx_signal_handler_to_water_injection,
            tx_signal_handler_to_relay_manager,
            tx_signal_handler_to_data_logger,
        );

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = create_assert_test_object(
            tx_water_injection_to_relay_manager,
            rx_water_injection_from_relay_manager,
            rx_water_injection_from_signal_handler,
            tx_water_injection_to_data_logger,
            rx_water_injection_from_data_logger,
            refill,
            mutex_tank_level_switch_signals,
            false,
            false,
        );

        join_handle_mock_relay_manager
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
    }

    #[test]
    // This test case executes the inject_water function and sets the sensor signal
    // to high after some time has passed (the happy case).
    // Test case uses test database #42.
    pub fn test_water_injection_delayed_switch_high() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, refill, _) = prepare_refill_tests(None, 2, 60, Some(42));

        let (tx_water_injection_to_data_logger, rx_data_logger_from_water_injection) =
            mpsc::channel();
        let (tx_data_logger_to_water_injection, rx_water_injection_from_data_logger) =
            mpsc::channel();
        let (tx_water_injection_to_relay_manager, rx_relay_manager_from_water_injection) =
            mpsc::channel();
        let (tx_relay_manager_to_water_injection, rx_water_injection_from_relay_manager) =
            mpsc::channel();
        let (tx_signal_handler_to_water_injection, rx_water_injection_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        let tx_signal_handler_to_relay_manager = tx_water_injection_to_relay_manager.clone();

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
        let mutex_tank_level_switch_signals_clone_for_test_environment =
            mutex_tank_level_switch_signals.clone();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_water_injection,
                    &rx_data_logger_from_water_injection,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        // thread for mock Controllino - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_water_injection,
                    &rx_relay_manager_from_water_injection,
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
                    .unwrap()
                    .as_millis();
                println!("Actuation duration={}", actuation_duration);
                assert_ge!(actuation_duration, 5890);
                assert_le!(actuation_duration, 6250);
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
                let _ = tx_signal_handler_to_water_injection.send(InternalCommand::Quit);

                // wait for inject_water to terminate
                spin_sleeper.sleep(sleep_duration_two_seconds);

                // request termination to mock threads
                let _ = tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Quit);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = create_assert_test_object(
            tx_water_injection_to_relay_manager,
            rx_water_injection_from_relay_manager,
            rx_water_injection_from_signal_handler,
            tx_water_injection_to_data_logger,
            rx_water_injection_from_data_logger,
            refill,
            mutex_tank_level_switch_signals,
            false,
            false,
        );

        join_handle_mock_relay_manager
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
    }

    #[test]
    // This test case executes inject_water function with simulation of level switch stuck at low position.
    // Test case uses test database #43.
    pub fn test_water_injection_switch_stuck_low() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut refill, _) = prepare_refill_tests(None, 2, 60, Some(43));
        let mut water_injection = WaterInjection::new(&refill.config);

        let (tx_water_injection_to_data_logger, rx_data_logger_from_water_injection) =
            mpsc::channel();
        let (tx_data_logger_to_water_injection, rx_water_injection_from_data_logger) =
            mpsc::channel();
        let (tx_water_injection_to_relay_manager, rx_relay_manager_from_water_injection) =
            mpsc::channel();
        let (tx_relay_manager_to_water_injection, rx_water_injection_from_relay_manager) =
            mpsc::channel();
        let (_tx_signal_handler_to_water_injection, rx_water_injection_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        let tx_signal_handler_to_relay_manager = tx_water_injection_to_relay_manager.clone();

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_water_injection,
                    &rx_data_logger_from_water_injection,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        // thread for mock Controllino - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_water_injection,
                    &rx_relay_manager_from_water_injection,
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
                    .unwrap()
                    .as_millis();
                println!("Actuation duration={}", actuation_duration);
                assert_ge!(actuation_duration, 40000);
                assert_le!(actuation_duration, 43000);
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
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Quit);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let water_injection_channels = WaterInjectionChannels {
                    tx_water_injection_to_data_logger: &tx_water_injection_to_data_logger,
                    rx_water_injection_from_data_logger: &rx_water_injection_from_data_logger,
                    tx_water_injection_to_relay_manager: &tx_water_injection_to_relay_manager,
                    rx_water_injection_from_relay_manager: &rx_water_injection_from_relay_manager,
                    rx_water_injection_from_signal_handler: &rx_water_injection_from_signal_handler,
                };
                let result = water_injection.inject_water(
                    water_injection_channels,
                    &mut refill.refill_errors,
                    &mut refill.sql_interface_refill,
                    &mutex_tank_level_switch_signals,
                );
                assert_eq!(result, false);
                assert_eq!(refill.refill_errors.has_error(), true);
                assert_eq!(refill.refill_errors.error_switch_stuck_low, true);
            })
            .unwrap();

        join_handle_mock_relay_manager
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
    }

    #[test]
    // This test case executes inject_water function and sends Quit signal
    // after some time when the water level is still low.
    // Test case uses test database #44.
    pub fn test_water_injection_quit_signal() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut refill, _) = prepare_refill_tests(None, 10, 60, Some(44));
        let mut water_injection = WaterInjection::new(&refill.config);

        let (tx_water_injection_to_data_logger, rx_data_logger_from_water_injection) =
            mpsc::channel();
        let (tx_data_logger_to_water_injection, rx_water_injection_from_data_logger) =
            mpsc::channel();
        let (tx_water_injection_to_relay_manager, rx_relay_manager_from_water_injection) =
            mpsc::channel();
        let (tx_relay_manager_to_water_injection, rx_water_injection_from_relay_manager) =
            mpsc::channel();
        let (tx_signal_handler_to_water_injection, rx_water_injection_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        let tx_signal_handler_to_relay_manager = tx_water_injection_to_relay_manager.clone();

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_water_injection,
                    &rx_data_logger_from_water_injection,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        // thread for mock Controllino - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_water_injection,
                    &rx_relay_manager_from_water_injection,
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
                    .unwrap()
                    .as_millis();
                println!("Actuation duration={}", actuation_duration);
                assert_ge!(actuation_duration, 1900);
                assert_le!(actuation_duration, 2100);
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            tx_signal_handler_to_water_injection,
            tx_signal_handler_to_relay_manager,
            tx_signal_handler_to_data_logger,
        );

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let water_injection_channels = WaterInjectionChannels {
                    tx_water_injection_to_data_logger: &tx_water_injection_to_data_logger,
                    rx_water_injection_from_data_logger: &rx_water_injection_from_data_logger,
                    tx_water_injection_to_relay_manager: &tx_water_injection_to_relay_manager,
                    rx_water_injection_from_relay_manager: &rx_water_injection_from_relay_manager,
                    rx_water_injection_from_signal_handler: &rx_water_injection_from_signal_handler,
                };

                let result = water_injection.inject_water(
                    water_injection_channels,
                    &mut refill.refill_errors,
                    &mut refill.sql_interface_refill,
                    &mutex_tank_level_switch_signals,
                );
                assert_eq!(result, true);
                assert_eq!(refill.refill_errors.has_error(), false);
            })
            .unwrap();

        join_handle_mock_relay_manager
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
    }
}
