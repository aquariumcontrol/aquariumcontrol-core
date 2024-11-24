/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::Local;
use log::{error, warn};
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use crate::channel_content::{AquariumDevice, AquariumSignal, InternalCommand};
use crate::proc_ext_req::ProcessExternalRequestTrait;
use crate::refill::RefillConfig;
use crate::refill::RefillErrors;
use crate::sql_interface_refill::SqlInterfaceRefill;

const LOG_TARGET_WATER_INJECTION: &str = "water_injection";

/// Trait for the execution of the refill once control has detected that water level is low.
/// This trait allows running the main control with a mock implementation for testing.
pub trait WaterInjectionTrait {
    /// Actuates the refill pump for injecting water into the main tank.
    /// # Arguments
    /// * `tx_to_controllino` - Reference to sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Reference to Receiver part of channel for communication from Controllino
    /// * `tx_to_tank_level_switch - Reference to sender part of channel for communication to tank level switch calculation
    /// * `tx_from_tank_level_switch` - Reference to receiver part of channel for communication from tank level switch calculation
    /// * `rx_from_signal_handler` - Reference to receiver part of channel for communication from signal handler
    /// * `tx_to_data_logger` - Sender part of channel for communication to data logger
    /// * `refill_errors` - Reference to struct of type RefillErrors which records the errors occurred during water injection            
    /// * `sql_interface_refill` - Reference to SQL database interface for refill control
    fn inject_water(
        &mut self,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
        tx_to_tank_level_switch: &Sender<InternalCommand>,
        rx_from_tank_level_switch: &Receiver<bool>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        tx_to_data_logger: &Sender<bool>,
        rx_from_data_logger: &Receiver<InternalCommand>,
        refill_errors: &mut RefillErrors,
        sql_interface_refill: &mut SqlInterfaceRefill,
    ) -> bool;
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Implementation of the WaterInjectionTrait which contains the code for executing the refill once
/// the main control has detected that the water level is low.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     water_injection[Mineral injection] --> controllino[Controllino]
///     controllino --> water_injection
///     water_injection --> data_logger[Data logger]
///     data_logger --> water_injection
///     water_injection --> tank_level_switch[tank level switch calculation]
///     tank_level_switch --> water_injection
///     signal_handler[Signal handler] --> water_injection
/// ```
pub struct WaterInjection {
    refill_increment: f64,         // value is calculated based on RefillConfig
    additional_refill_volume: f64, // value is copied from RefillConfig
    max_refill_volume: f64,        // value is copied from RefillConfig
    spin_sleeper: SpinSleeper,
    sleep_duration_hundred_milli_sec: Duration,
    sleep_interval_seconds: f64,
    lock_error_channel_send_tank_level_switch_position: bool,
    lock_error_channel_receive_tank_level_switch_position: bool,
    lock_error_channel_send_tank_level_switch_invalid: bool,
    lock_error_channel_receive_tank_level_switch_invalid: bool,
    lock_error_channel_receive_data_logger: bool,
    lock_warn_inapplicable_command_data_logger: bool,
    lock_error_invalid_signal_request_data_logger: bool,
    lock_error_send_to_data_logger: bool,
}

impl WaterInjection {
    /// Provide a struct of type WaterInjection
    pub fn new(config: &RefillConfig) -> WaterInjection {
        let sleep_interval_millis = 100;
        let sleep_interval_seconds: f64 = (sleep_interval_millis as f64) / (1000 as f64);

        WaterInjection {
            refill_increment: sleep_interval_seconds * config.refill_pump_flow,
            additional_refill_volume: config.additional_refill_volume,
            max_refill_volume: config.max_refill_volume,
            spin_sleeper: SpinSleeper::default(),
            sleep_duration_hundred_milli_sec: Duration::from_millis(sleep_interval_millis),
            sleep_interval_seconds: sleep_interval_seconds,
            lock_error_channel_send_tank_level_switch_position: false,
            lock_error_channel_receive_tank_level_switch_position: false,
            lock_error_channel_send_tank_level_switch_invalid: false,
            lock_error_channel_receive_tank_level_switch_invalid: false,
            lock_error_channel_receive_data_logger: false,
            lock_warn_inapplicable_command_data_logger: false,
            lock_error_invalid_signal_request_data_logger: false,
            lock_error_send_to_data_logger: false,
        }
    }

    // Channel communication with tank level switch calculation
    // The function returns a tuple of boolean variables.
    /// # Arguments
    /// * `tx_to_tank_level_switch` - reference to sender part of channel
    /// * `rx_from_tank_level_switch` - reference to receiver part of channel
    fn get_tank_level_switch_signals(
        &mut self,
        tx_to_tank_level_switch: &Sender<InternalCommand>,
        rx_from_tank_level_switch: &Receiver<bool>,
    ) -> (bool, bool) {
        // send request for tank level switch position
        let tank_level_switch_position = match tx_to_tank_level_switch.send(
            InternalCommand::RequestSignal(AquariumSignal::TankLevelSwitchPosition),
        ) {
            Ok(_) => {
                self.lock_error_channel_send_tank_level_switch_position = false;
                // Sent request for tank level switch position.
                // now listen for response
                match rx_from_tank_level_switch.recv() {
                    Ok(c) => {
                        self.lock_error_channel_receive_tank_level_switch_position = false;
                        c
                    }
                    Err(e) => {
                        if !self.lock_error_channel_receive_tank_level_switch_position {
                            error!(
                                target: LOG_TARGET_WATER_INJECTION,
                                "receiving tank level switch position failed ({e:?})"
                            );
                            self.lock_error_channel_receive_tank_level_switch_position = true;
                        }
                        true // default value in case of error
                    }
                }
            }
            Err(e) => {
                if !self.lock_error_channel_send_tank_level_switch_position {
                    error!(
                        target: LOG_TARGET_WATER_INJECTION,
                        "channel communication for tank level switch position failed ({e:?})"
                    );
                    self.lock_error_channel_send_tank_level_switch_position = true;
                }
                true // default value in case of error
            }
        };

        // send request for tank level switch position
        let tank_level_switch_invalid = match tx_to_tank_level_switch.send(
            InternalCommand::RequestSignal(AquariumSignal::TankLevelSwitchInvalid),
        ) {
            Ok(_) => {
                self.lock_error_channel_send_tank_level_switch_invalid = false;
                // Sent request for tank level switch position.
                // now listen for response
                match rx_from_tank_level_switch.recv() {
                    Ok(c) => {
                        self.lock_error_channel_receive_tank_level_switch_invalid = false;
                        c
                    }
                    Err(e) => {
                        if !self.lock_error_channel_receive_tank_level_switch_invalid {
                            error!(
                                target: LOG_TARGET_WATER_INJECTION,
                                "receiving tank level switch invalid failed ({e:?})"
                            );
                            self.lock_error_channel_receive_tank_level_switch_invalid = true;
                        }
                        true // default value in case of error
                    }
                }
            }
            Err(e) => {
                if !self.lock_error_channel_send_tank_level_switch_invalid {
                    error!(
                        target: LOG_TARGET_WATER_INJECTION,
                        "channel communication for tank level switch invalid failed ({e:?})"
                    );
                    self.lock_error_channel_send_tank_level_switch_invalid = true;
                }
                true // default value in case of error
            }
        };

        (tank_level_switch_position, tank_level_switch_invalid)
    }

    /// check if data logger sent command and respond
    fn communicate_with_data_logger(
        &mut self,
        tx_to_data_logger: &Sender<bool>,
        rx_from_data_logger: &Receiver<InternalCommand>,
    ) {
        match rx_from_data_logger.try_recv() {
            Ok(c) => {
                self.lock_error_channel_receive_data_logger = false;
                match c {
                    InternalCommand::RequestSignal(d) => {
                        self.lock_warn_inapplicable_command_data_logger = false;
                        match d {
                            AquariumSignal::RefillPumpStatus => {
                                self.lock_error_invalid_signal_request_data_logger = true;
                                match tx_to_data_logger.send(false) {
                                    Ok(_) => {
                                        self.lock_error_send_to_data_logger = false;
                                    }
                                    Err(e) => {
                                        if !self.lock_error_send_to_data_logger {
                                            error!(
                                                target: LOG_TARGET_WATER_INJECTION,
                                                "channel communication to data logger failed ({e:?})"
                                            );
                                            self.lock_error_send_to_data_logger = true;
                                        }
                                    }
                                };
                            }
                            _ => {
                                if !self.lock_error_invalid_signal_request_data_logger {
                                    error!(
                                        target: LOG_TARGET_WATER_INJECTION,
                                        "internal error: received request for invalid signal {} from data logger",
                                        d
                                    );
                                    self.lock_error_invalid_signal_request_data_logger = true;
                                }
                            }
                        }
                    }
                    _ => {
                        if !self.lock_warn_inapplicable_command_data_logger {
                            warn!(
                                target: LOG_TARGET_WATER_INJECTION,
                                "ignoring inapplicable command from data logger thread."
                            );
                            self.lock_warn_inapplicable_command_data_logger = true;
                        }
                    }
                }
            }
            Err(e) => {
                match e {
                    TryRecvError::Empty => {
                        /* empty buffer - no action required */
                        self.lock_error_channel_receive_data_logger = false;
                    }
                    TryRecvError::Disconnected => {
                        if !self.lock_error_channel_receive_data_logger {
                            error!(
                                target: LOG_TARGET_WATER_INJECTION,
                                "receiving command from data logger thread failed ({e:?})"
                            );
                            self.lock_error_channel_receive_data_logger = true;
                        }
                    }
                }
            }
        };
    }
}

impl WaterInjectionTrait for WaterInjection {
    /// this function executes the refill operation
    fn inject_water(
        &mut self,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
        tx_to_tank_level_switch: &Sender<InternalCommand>,
        rx_from_tank_level_switch: &Receiver<bool>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        tx_to_data_logger: &Sender<bool>,
        rx_from_data_logger: &Receiver<InternalCommand>,
        refill_errors: &mut RefillErrors,
        sql_interface_refill: &mut SqlInterfaceRefill,
    ) -> bool {
        let mut quit_command_received = false;

        // command start of refill pump
        match tx_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::RefillPump)) {
            Ok(_) => {
                // now listen for response
                match rx_from_controllino.recv() {
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

        loop {
            self.communicate_with_data_logger(&tx_to_data_logger, &rx_from_data_logger);

            self.spin_sleeper
                .sleep(self.sleep_duration_hundred_milli_sec);
            refill_time_seconds += self.sleep_interval_seconds;

            let (tank_level_switch_position, tank_level_switch_invalid) = self
                .get_tank_level_switch_signals(tx_to_tank_level_switch, rx_from_tank_level_switch);

            if tank_level_switch_position
                && additional_refill_volume_executed > self.additional_refill_volume
            {
                // water level changed to high
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
            (quit_command_received, _, _) =
                self.process_external_request(&rx_from_signal_handler, None);
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
        match tx_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::RefillPump)) {
            Ok(_) => {
                // now listen for response
                match rx_from_controllino.recv() {
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
        match tx_to_data_logger.send(false) {
            Ok(_) => { /* no listening to response */ }
            Err(e) => {
                error!(
                    target: LOG_TARGET_WATER_INJECTION,
                    "channel communication to data logger when finishing refill failed ({e:?})"
                );
            }
        }

        // write result to data base
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
        quit_command_received
    }
}

#[cfg(test)]
pub mod tests {
    use all_asserts::{assert_ge, assert_le};
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::channel_content::{AquariumDevice, InternalCommand};
    use crate::mock::tests::mock_controllino;
    use crate::mock::tests::mock_data_logger;
    use crate::mock::tests::mock_tank_level_switch;
    use crate::mock::tests::MockSignal;
    use crate::mock::tests::TestCommand;
    use crate::refill::tests::prepare_refill_tests;
    use crate::water_injection::WaterInjection;
    use crate::water_injection::WaterInjectionTrait;

    #[test]
    // this test case executes inject_water function and immediately sets the sensor signal to high
    pub fn test_water_injection_immediate_switch_high_no_quit_signal() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut refill, _) =
            prepare_refill_tests("/config/aquarium_control_test_refill.toml", None);
        let mut water_injection = WaterInjection::new(&refill.config);

        let (tx_water_injection_to_tank_level_switch, rx_tank_level_switch_from_water_injection) =
            mpsc::channel();
        let (tx_tank_level_switch_to_water_injection, rx_water_injection_from_tank_level_switch) =
            mpsc::channel();
        let (tx_water_injection_to_data_logger, rx_data_logger_from_water_injection) =
            mpsc::channel();
        let (tx_data_logger_to_water_injection, rx_water_injection_from_data_logger) =
            mpsc::channel();
        let (tx_water_injection_to_controllino, rx_controllino_from_water_injection) =
            mpsc::channel();
        let (tx_controllino_to_water_injection, rx_water_injection_from_controllino) =
            mpsc::channel();
        let (tx_signal_handler_to_water_injection, rx_water_injection_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        let tx_signal_handler_to_controllino = tx_water_injection_to_controllino.clone();
        let tx_signal_handler_to_tank_level_switch =
            tx_water_injection_to_tank_level_switch.clone();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_water_injection,
                    &rx_tank_level_switch_from_water_injection,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    true,
                    false,
                    true,
                );
            })
            .unwrap();

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
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events = mock_controllino(
                    &tx_controllino_to_water_injection,
                    &rx_controllino_from_water_injection,
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
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_controllino.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Quit);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let result = water_injection.inject_water(
                    &tx_water_injection_to_controllino,
                    &rx_water_injection_from_controllino,
                    &tx_water_injection_to_tank_level_switch,
                    &rx_water_injection_from_tank_level_switch,
                    &rx_water_injection_from_signal_handler,
                    &tx_water_injection_to_data_logger,
                    &rx_water_injection_from_data_logger,
                    &mut refill.refill_errors,
                    &mut refill.sql_interface_refill,
                );
                assert_eq!(result, false);
                assert_eq!(refill.refill_errors.has_error(), false);
            })
            .unwrap();

        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_controllino
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
    // this test case executes inject_water function and immediately sets the sensor signal to high
    // Additionally, the Quit signal is sent.
    // Expected behavior: immediate stop of additional refill operation.
    pub fn test_water_injection_immediate_switch_high_and_quit_signal() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut refill, _) =
            prepare_refill_tests("/config/aquarium_control_test_refill.toml", None);
        let mut water_injection = WaterInjection::new(&refill.config);

        let (tx_water_injection_to_tank_level_switch, rx_tank_level_switch_from_water_injection) =
            mpsc::channel();
        let (tx_tank_level_switch_to_water_injection, rx_water_injection_from_tank_level_switch) =
            mpsc::channel();
        let (tx_water_injection_to_data_logger, rx_data_logger_from_water_injection) =
            mpsc::channel();
        let (tx_data_logger_to_water_injection, rx_water_injection_from_data_logger) =
            mpsc::channel();
        let (tx_water_injection_to_controllino, rx_controllino_from_water_injection) =
            mpsc::channel();
        let (tx_controllino_to_water_injection, rx_water_injection_from_controllino) =
            mpsc::channel();
        let (tx_signal_handler_to_water_injection, rx_water_injection_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        let tx_signal_handler_to_controllino = tx_water_injection_to_controllino.clone();
        let tx_signal_handler_to_tank_level_switch =
            tx_water_injection_to_tank_level_switch.clone();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_water_injection,
                    &rx_tank_level_switch_from_water_injection,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    true,
                    false,
                    true,
                );
            })
            .unwrap();

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
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events = mock_controllino(
                    &tx_controllino_to_water_injection,
                    &rx_controllino_from_water_injection,
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
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_seconds = Duration::from_secs(2);
                let spin_sleeper = SpinSleeper::default();
                let _ = tx_signal_handler_to_water_injection.send(InternalCommand::Quit);
                spin_sleeper.sleep(sleep_duration_two_seconds);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_controllino.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Quit);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let result = water_injection.inject_water(
                    &tx_water_injection_to_controllino,
                    &rx_water_injection_from_controllino,
                    &tx_water_injection_to_tank_level_switch,
                    &rx_water_injection_from_tank_level_switch,
                    &rx_water_injection_from_signal_handler,
                    &tx_water_injection_to_data_logger,
                    &rx_water_injection_from_data_logger,
                    &mut refill.refill_errors,
                    &mut refill.sql_interface_refill,
                );
                assert_eq!(result, true);
                assert_eq!(refill.refill_errors.has_error(), false);
            })
            .unwrap();

        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_controllino
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
    // this test case executes inject_water function and immediately
    // - sets the sensor signal to high
    // - sets the invalid bit.
    // Water injection shall be aborted without adding further water
    pub fn test_water_injection_immediate_switch_high_with_invalid_true() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut refill, _) =
            prepare_refill_tests("/config/aquarium_control_test_refill.toml", None);
        let mut water_injection = WaterInjection::new(&refill.config);

        let (tx_water_injection_to_tank_level_switch, rx_tank_level_switch_from_water_injection) =
            mpsc::channel();
        let (tx_tank_level_switch_to_water_injection, rx_water_injection_from_tank_level_switch) =
            mpsc::channel();
        let (tx_water_injection_to_data_logger, rx_data_logger_from_water_injection) =
            mpsc::channel();
        let (tx_data_logger_to_water_injection, rx_water_injection_from_data_logger) =
            mpsc::channel();
        let (tx_water_injection_to_controllino, rx_controllino_from_water_injection) =
            mpsc::channel();
        let (tx_controllino_to_water_injection, rx_water_injection_from_controllino) =
            mpsc::channel();
        let (tx_signal_handler_to_water_injection, rx_water_injection_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        let tx_signal_handler_to_controllino = tx_water_injection_to_controllino.clone();
        let tx_signal_handler_to_tank_level_switch =
            tx_water_injection_to_tank_level_switch.clone();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_water_injection,
                    &rx_tank_level_switch_from_water_injection,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    true,
                    true,
                    true,
                );
            })
            .unwrap();

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
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events = mock_controllino(
                    &tx_controllino_to_water_injection,
                    &rx_controllino_from_water_injection,
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
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_seconds = Duration::from_secs(2);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_two_seconds);
                let _ = tx_signal_handler_to_water_injection.send(InternalCommand::Quit);
                spin_sleeper.sleep(sleep_duration_two_seconds);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_controllino.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Quit);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let result = water_injection.inject_water(
                    &tx_water_injection_to_controllino,
                    &rx_water_injection_from_controllino,
                    &tx_water_injection_to_tank_level_switch,
                    &rx_water_injection_from_tank_level_switch,
                    &rx_water_injection_from_signal_handler,
                    &tx_water_injection_to_data_logger,
                    &rx_water_injection_from_data_logger,
                    &mut refill.refill_errors,
                    &mut refill.sql_interface_refill,
                );
                assert_eq!(result, false);
                assert_eq!(refill.refill_errors.has_error(), false);
            })
            .unwrap();

        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_controllino
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
    // this test case executes inject_water function and sets the sensor signal to high after some time has passed (happy case)
    pub fn test_water_injection_delayed_switch_high() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut refill, _) =
            prepare_refill_tests("/config/aquarium_control_test_refill.toml", None);
        let mut water_injection = WaterInjection::new(&refill.config);

        let (tx_water_injection_to_tank_level_switch, rx_tank_level_switch_from_water_injection) =
            mpsc::channel();
        let (tx_tank_level_switch_to_water_injection, rx_water_injection_from_tank_level_switch) =
            mpsc::channel();
        let (tx_water_injection_to_data_logger, rx_data_logger_from_water_injection) =
            mpsc::channel();
        let (tx_data_logger_to_water_injection, rx_water_injection_from_data_logger) =
            mpsc::channel();
        let (tx_water_injection_to_controllino, rx_controllino_from_water_injection) =
            mpsc::channel();
        let (tx_controllino_to_water_injection, rx_water_injection_from_controllino) =
            mpsc::channel();
        let (tx_signal_handler_to_water_injection, rx_water_injection_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        let tx_signal_handler_to_controllino = tx_water_injection_to_controllino.clone();
        let tx_signal_handler_to_tank_level_switch =
            tx_water_injection_to_tank_level_switch.clone();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_water_injection,
                    &rx_tank_level_switch_from_water_injection,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    false,
                    false,
                    false,
                );
            })
            .unwrap();

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
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events = mock_controllino(
                    &tx_controllino_to_water_injection,
                    &rx_controllino_from_water_injection,
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
                assert_ge!(actuation_duration, 5950);
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
                let _ = tx_test_environment_to_tank_level_switch.send(TestCommand::UpdateSignal(
                    MockSignal::TankLevelSwitchPosition(true),
                ));
                let _ = tx_test_environment_to_tank_level_switch.send(TestCommand::UpdateSignal(
                    MockSignal::TankLevelSwitchPositionStabilized(true),
                ));

                // wait for refill operation to finish
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = tx_signal_handler_to_water_injection.send(InternalCommand::Quit);

                // wait for inject_water to terminate
                spin_sleeper.sleep(sleep_duration_two_seconds);

                // request termination to mock threads
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_controllino.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Quit);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let result = water_injection.inject_water(
                    &tx_water_injection_to_controllino,
                    &rx_water_injection_from_controllino,
                    &tx_water_injection_to_tank_level_switch,
                    &rx_water_injection_from_tank_level_switch,
                    &rx_water_injection_from_signal_handler,
                    &tx_water_injection_to_data_logger,
                    &rx_water_injection_from_data_logger,
                    &mut refill.refill_errors,
                    &mut refill.sql_interface_refill,
                );
                assert_eq!(result, false);
                assert_eq!(refill.refill_errors.has_error(), false);
            })
            .unwrap();

        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_controllino
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
    // this test case executes inject_water function with simulation of level switch stuck at low position
    pub fn test_water_injection_switch_stuck_low() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut refill, _) =
            prepare_refill_tests("/config/aquarium_control_test_refill.toml", None);
        let mut water_injection = WaterInjection::new(&refill.config);

        let (tx_water_injection_to_tank_level_switch, rx_tank_level_switch_from_water_injection) =
            mpsc::channel();
        let (tx_tank_level_switch_to_water_injection, rx_water_injection_from_tank_level_switch) =
            mpsc::channel();
        let (tx_water_injection_to_data_logger, rx_data_logger_from_water_injection) =
            mpsc::channel();
        let (tx_data_logger_to_water_injection, rx_water_injection_from_data_logger) =
            mpsc::channel();
        let (tx_water_injection_to_controllino, rx_controllino_from_water_injection) =
            mpsc::channel();
        let (tx_controllino_to_water_injection, rx_water_injection_from_controllino) =
            mpsc::channel();
        let (_tx_signal_handler_to_water_injection, rx_water_injection_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        let tx_signal_handler_to_controllino = tx_water_injection_to_controllino.clone();
        let tx_signal_handler_to_tank_level_switch =
            tx_water_injection_to_tank_level_switch.clone();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_water_injection,
                    &rx_tank_level_switch_from_water_injection,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    false,
                    false,
                    false,
                );
            })
            .unwrap();

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
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events = mock_controllino(
                    &tx_controllino_to_water_injection,
                    &rx_controllino_from_water_injection,
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
                assert_ge!(actuation_duration, 40500);
                assert_le!(actuation_duration, 43000);
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_fourtyfive_seconds = Duration::from_secs(45);
                let spin_sleeper = SpinSleeper::default();

                // wait for refill operation to abort
                spin_sleeper.sleep(sleep_duration_fourtyfive_seconds);

                // request termination to mock threads
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_controllino.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Quit);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let result = water_injection.inject_water(
                    &tx_water_injection_to_controllino,
                    &rx_water_injection_from_controllino,
                    &tx_water_injection_to_tank_level_switch,
                    &rx_water_injection_from_tank_level_switch,
                    &rx_water_injection_from_signal_handler,
                    &tx_water_injection_to_data_logger,
                    &rx_water_injection_from_data_logger,
                    &mut refill.refill_errors,
                    &mut refill.sql_interface_refill,
                );
                assert_eq!(result, false);
                assert_eq!(refill.refill_errors.has_error(), true);
                assert_eq!(refill.refill_errors.error_switch_stuck_low, true);
            })
            .unwrap();

        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_controllino
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
    // this test case executes inject_water function and sends Quit signal after some time when water level is still low
    pub fn test_water_injection_quit_signal() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut refill, _) =
            prepare_refill_tests("/config/aquarium_control_test_refill_tertiary.toml", None);
        let mut water_injection = WaterInjection::new(&refill.config);

        let (tx_water_injection_to_tank_level_switch, rx_tank_level_switch_from_water_injection) =
            mpsc::channel();
        let (tx_tank_level_switch_to_water_injection, rx_water_injection_from_tank_level_switch) =
            mpsc::channel();
        let (tx_water_injection_to_data_logger, rx_data_logger_from_water_injection) =
            mpsc::channel();
        let (tx_data_logger_to_water_injection, rx_water_injection_from_data_logger) =
            mpsc::channel();
        let (tx_water_injection_to_controllino, rx_controllino_from_water_injection) =
            mpsc::channel();
        let (tx_controllino_to_water_injection, rx_water_injection_from_controllino) =
            mpsc::channel();
        let (tx_signal_handler_to_water_injection, rx_water_injection_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        let tx_signal_handler_to_controllino = tx_water_injection_to_controllino.clone();
        let tx_signal_handler_to_tank_level_switch =
            tx_water_injection_to_tank_level_switch.clone();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_water_injection,
                    &rx_tank_level_switch_from_water_injection,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    false,
                    false,
                    false,
                );
            })
            .unwrap();

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
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events = mock_controllino(
                    &tx_controllino_to_water_injection,
                    &rx_controllino_from_water_injection,
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
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_seconds = Duration::from_secs(2);
                let spin_sleeper = SpinSleeper::default();

                // wait for refill operation to inject some water
                spin_sleeper.sleep(sleep_duration_two_seconds);
                let _ = tx_signal_handler_to_water_injection.send(InternalCommand::Quit);

                // wait for test object to terminate
                spin_sleeper.sleep(sleep_duration_two_seconds);

                // request termination to mock threads
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_controllino.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Quit);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let result = water_injection.inject_water(
                    &tx_water_injection_to_controllino,
                    &rx_water_injection_from_controllino,
                    &tx_water_injection_to_tank_level_switch,
                    &rx_water_injection_from_tank_level_switch,
                    &rx_water_injection_from_signal_handler,
                    &tx_water_injection_to_data_logger,
                    &rx_water_injection_from_data_logger,
                    &mut refill.refill_errors,
                    &mut refill.sql_interface_refill,
                );
                assert_eq!(result, true);
                assert_eq!(refill.refill_errors.has_error(), false);
            })
            .unwrap();

        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_controllino
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
