/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::Local;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::channel_content::{AquariumDevice, InternalCommand};
use crate::refill::check_pending_external_request;
use crate::refill::RefillConfig;
use crate::refill::RefillErrors;
use crate::refill::TankLevelSwitchSignals;
use crate::sql_interface_refill::SqlInterfaceRefill;

/// Trait for the execution of the refill once control has detected that water level is low.
/// This trait allows running the main control with a mock implementation for testing.
pub trait WaterInjectionTrait {
    /// Actuates the refill pump for injecting water into the main tank.
    /// # Arguments
    /// * `tx_to_controllino` - Reference to sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Reference to Receiver part of channel for communication from Controllino
    /// * `tx_to_gpio` - Reference to sender part of channel for communication to GPIO handler
    /// * `rx_from_gpio` - Reference to receiver part of channel for communication from GPIO handler
    /// * `rx_from_signal_handler` - Reference to receiver part of channel for communication from signal handler
    /// * `tx_to_data_logger` - Sender part of channel for communication to data logger
    /// * `tank_level_switch_signals` - Reference to struct of type TankLevelSwitchSignals which implements the logic for signal interpretation
    /// * `refill_errors` - Reference to struct of type RefillErrors which records the errors occurred during water injection            
    /// * `config` - Reference to configuration of refill control   
    /// * `sql_interface_refill` - Reference to SQL database interface for refill control
    fn inject_water(
        &mut self,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
        tx_to_gpio: &Sender<InternalCommand>,
        rx_from_gpio: &Receiver<f32>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        tx_to_datalogger: &Sender<bool>,
        tank_level_switch_signals: &mut TankLevelSwitchSignals,
        refill_errors: &mut RefillErrors,
        config: &RefillConfig,
        sql_interface_refill: &mut SqlInterfaceRefill,
    ) -> bool;
}

/// Implementation of the WaterInjectionTrait which contains the code for executing the refill once
/// the main control has detected that the water level is low.
pub struct WaterInjection;

impl WaterInjectionTrait for WaterInjection {
    /// this function executes the refill operation
    fn inject_water(
        &mut self,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
        tx_to_gpio: &Sender<InternalCommand>,
        rx_from_gpio: &Receiver<f32>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        tx_to_datalogger: &Sender<bool>,
        tank_level_switch_signals: &mut TankLevelSwitchSignals,
        refill_errors: &mut RefillErrors,
        config: &RefillConfig,
        sql_interface_refill: &mut SqlInterfaceRefill,
    ) -> bool {
        // inform data logger that refill is in progress
        println!("Refill: communicating to data logger thread...");
        match tx_to_datalogger.send(true) {
            Ok(_) => { /* no listening to response */ }
            Err(e) => {
                println!(
                "Refill: Error in channel communication to DataLogger when starting refill: {e:?}"
            );
            }
        }
        println!("Refill: executing refill...");
        let mut quit_command_received = false;
        // command start of refill pump
        match tx_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::RefillPump)) {
            Ok(_) => {
                // now listen for response
                match rx_from_controllino.recv() {
                    Ok(_) => {}
                    Err(e) => {
                        println!("Refill: Error when trying to receive answer from Controllino when switching on refill pump: {e:?}")
                    }
                };
            }
            Err(e) => {
                println!("Refill: Error in channel communication to Controllino when switching on refill pump: {e:?}");
            }
        }
        println!("Refill: Sent request to switch on refill pump to Controllino thread.");

        //*** wait for condition to stop refilling ***
        let spin_sleeper = SpinSleeper::default();
        let sleep_interval_millis = 100;
        let sleep_duration_hundred_milli_sec = Duration::from_millis(sleep_interval_millis);
        let sleep_interval_seconds: f64 = (sleep_interval_millis as f64) / (1000 as f64);
        let mut refill_volume = 0.0;
        let mut refill_time_seconds = 0.0;
        let mut refill_error_code = 0;
        let mut additional_refill_volume_executed = 0.0;
        let refill_increment = sleep_interval_seconds * config.refill_pump_flow;

        loop {
            spin_sleeper.sleep(sleep_duration_hundred_milli_sec);
            refill_time_seconds += sleep_interval_seconds;
            println!("Water injection: updating tank level switch signals");
            tank_level_switch_signals.update_tank_level_switch_signals(&tx_to_gpio, rx_from_gpio);
            if tank_level_switch_signals.tank_level_switch_position
                && additional_refill_volume_executed > config.additional_refill_volume
            {
                // water level changed to high
                break;
            }
            if tank_level_switch_signals.tank_level_switch_invalid {
                // switch signal is invalid
                refill_error_code = 1;
                break;
            }

            // calculate volume refilled in this operation
            refill_volume += refill_increment;
            if tank_level_switch_signals.tank_level_switch_position {
                additional_refill_volume_executed += refill_increment;
                println!(
                    "Executing additional refill at {}L, limit at {}L",
                    additional_refill_volume_executed, config.additional_refill_volume
                );
            }
            quit_command_received =
                check_pending_external_request(&rx_from_signal_handler, refill_errors);
            if quit_command_received {
                println!("Water injection: received Quit command");
                break;
            }
            println!(
                "Water injection: current refill volume at {}L, limit={}L",
                refill_volume, config.max_refill_volume
            );
            if refill_volume > config.max_refill_volume {
                refill_errors.error_switch_stuck_low = true;
                refill_error_code = 2;
                break;
            }
        }

        // command stop of refill pump
        println!(
            "Water injection: Sending request to switch off refill pump to Controllino thread"
        );
        match tx_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::RefillPump)) {
            Ok(_) => {
                // now listen for response
                match rx_from_controllino.recv() {
                    Ok(_) => {}
                    Err(e) => {
                        println!("Refill: Error when trying to receive answer from Controllino when switching off refill pump: {e:?}")
                    }
                };
            }
            Err(e) => {
                println!("Refill: Error in channel communication to Controllino when switching off refill pump: {e:?}");
            }
        }
        println!("Water injection: Sent request to switch off refill pump to Controllino thread.");

        // inform data logger that refill is finished
        match tx_to_datalogger.send(false) {
            Ok(_) => { /* no listening to response */ }
            Err(e) => {
                println!(
                "Refill: Error in channel communication to DataLogger when finishing refill: {e:?}"
            );
            }
        }
        println!("Water injection: Sent information to data logger.");

        // write result to data base
        match sql_interface_refill.insert_refill_event(
            Local::now().naive_local(),
            refill_time_seconds,
            refill_volume,
            refill_error_code,
        ) {
            Ok(_) => {}
            Err(e) => {
                println!(
                "Refill: error occurred when writing refill operation data to SQL database: {e:?}"
            );
                refill_errors.error_sql_update_failed = true;
            }
        }
        quit_command_received
    }
}

#[cfg(test)]
pub mod tests {
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::thread::scope;
    use std::time::Duration;

    use crate::channel_content::InternalCommand;
    use crate::config::ConfigData;
    use crate::read_config_file;
    use crate::water_injection::TankLevelSwitchSignals;
    use crate::water_injection::WaterInjection;
    use crate::water_injection::WaterInjectionTrait;
    use crate::Refill;
    use crate::SqlInterface;
    use crate::SqlInterfaceRefill;

    use crate::refill::tests::prepare_refill_tests;
    use crate::refill::tests::run_main_cycle_with_sensor_signals;

    fn test_water_injection_receive_actuation_command_send_confirmation(
        rx_controllino_from_refill: &Receiver<InternalCommand>,
        tx_controllino_to_refill: &Sender<bool>,
    ) {
        // receive command from test object
        match rx_controllino_from_refill.recv() {
            Ok(_) => {}
            Err(e) => {
                panic!("Refill: Error when receiving actuation command from test object: {e:?}");
            }
        }

        // send confirmation back
        match tx_controllino_to_refill.send(true) {
            Ok(_) => {}
            Err(e) => {
                panic!("Refill: Error when sending back actuation command confirmation to test object: {e:?}");
            }
        }
    }

    fn test_water_injection_receive_information_for_datalogger(
        rx_datalogger_from_refill: &Receiver<bool>,
    ) {
        // receive information for data logger
        match rx_datalogger_from_refill.recv() {
            Ok(_) => {}
            Err(e) => {
                panic!("Refill: error when receiving information for data logger: {e:?}");
            }
        }
    }

    pub fn prepare_refill_tests_tertiary() -> (ConfigData, Refill, SqlInterface) {
        // prepare struct Refill for subsequent tests
        // parallel usage of database leads to conflicts
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_refill_tertiary.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_refill = SqlInterfaceRefill::new(sql_interface.get_connection());
        let refill_config = config.refill.clone();
        (
            config,
            Refill::new(refill_config, sql_interface_refill),
            sql_interface,
        )
    }

    fn run_main_cycle_with_sensor_signals_and_abort(
        rx_runner_from_test_case: &Receiver<InternalCommand>,
        rx_gpio_from_water_injection: &Receiver<InternalCommand>,
        tx_gpio_to_water_injection: &Sender<f32>,
        signal_position: bool,
        signal_invalid: bool,
    ) {
        loop {
            match rx_gpio_from_water_injection.try_recv() {
                Ok(_) => {
                    println!("run_main_cycle_with_sensor_signals_and_abort: Sending to water injection (1).");
                    let _ = tx_gpio_to_water_injection.send(match signal_position {
                        true => 1.0,
                        false => 0.0,
                    });
                }
                Err(_) => { /* do nothing */ }
            }
            match rx_gpio_from_water_injection.try_recv() {
                Ok(_) => {
                    println!("water_injection: run_main_cycle_with_sensor_signals_and_abort: Sending to water injection (2).");
                    let _ = tx_gpio_to_water_injection.send(match signal_invalid {
                        true => 1.0,
                        false => 0.0,
                    });
                }
                Err(_) => { /* do nothing */ }
            }
            match rx_runner_from_test_case.try_recv() {
                Ok(c) => match c {
                    InternalCommand::Quit => {
                        // test case has finished
                        break;
                    }
                    _ => { /* Ignore */ }
                },
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            println!(
                                "water_injection: run_main_cycle_with_sensor_signals_and_abort: Error in trying to receive command from test case: {e:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    // this test case executes inject_water function and immediately sets the sensor signal to high
    pub fn test_water_injection_immediate_switch_high() {
        let (_, mut refill, _) = prepare_refill_tests();
        let mut water_injection = WaterInjection;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (_tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_water_injection_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_water_injection_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                let target_number_iterations = (refill.config.additional_refill_volume
                    / refill.config.refill_pump_flow)
                    as u32
                    * 10
                    + 1;
                // indicate sensor signal high immediately
                run_main_cycle_with_sensor_signals(
                    target_number_iterations,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    true,
                    false,
                );

                test_water_injection_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                test_water_injection_receive_information_for_datalogger(&rx_datalogger_from_refill);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };

                let result = water_injection.inject_water(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, false);
            });
        });
    }

    #[test]
    // this test case executes inject_water function and immediately sets the sensor signal to high
    // Additionally, the Quit signal is sent.
    // Expected behavior: immediate stop of additional refill operation.
    pub fn test_water_injection_immediate_switch_high_and_quit_signal() {
        let (_, mut refill, _) = prepare_refill_tests();
        let mut water_injection = WaterInjection;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_water_injection_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_water_injection_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                // send Quit signal
                match tx_signal_handler_to_refill.send(InternalCommand::Quit) {
                    Ok(_) => {},
                    Err(e) => {
                        panic!("Refill: test_execute_refill_delayed_switch_high_with_invalid_true_and_quit_signal\
                            Error when sending Quit command to test object: {e:?}");
                    }
                }

                // leave switch high
                run_main_cycle_with_sensor_signals(
                    1,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );

                test_water_injection_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );
                test_water_injection_receive_information_for_datalogger(&rx_datalogger_from_refill);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };

                let result = water_injection.inject_water(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, true);
            });
        });
    }

    #[test]
    // this test case executes inject_water function and immediately sets the sensor signal to high
    // additionally the test case also sets the invalid bit.
    // Additionally, water injection shall be aborted.
    pub fn test_water_injection_immediate_switch_high_with_invalid_true() {
        let (_, mut refill, _) = prepare_refill_tests();
        let mut water_injection = WaterInjection;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (_tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_water_injection_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_water_injection_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                let target_number_iterations = 1;
                // indicate sensor signal high immediately
                run_main_cycle_with_sensor_signals(
                    target_number_iterations,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    true,
                    true,
                );

                test_water_injection_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                test_water_injection_receive_information_for_datalogger(&rx_datalogger_from_refill);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };

                let result = water_injection.inject_water(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, false);
            });
        });
    }

    #[test]
    // this test case executes inject_water function and sets the sensor signal to high after defined number of iterations (happy case)
    pub fn test_water_injection_delayed_switch_high() {
        let (_, mut refill, _) = prepare_refill_tests();
        let mut water_injection = WaterInjection;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (_tx_signal_handler_from_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_water_injection_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_water_injection_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                let target_number_iterations_main_fill =
                    (0.15 / refill.config.refill_pump_flow) as u32 * 10 + 1;
                // leave switch at low for some time
                run_main_cycle_with_sensor_signals(
                    target_number_iterations_main_fill,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );
                // tank is full. Indicate switch at high position
                let target_number_iterations_additional_fill =
                    (refill.config.additional_refill_volume / refill.config.refill_pump_flow)
                        as u32
                        * 10
                        + 1;
                run_main_cycle_with_sensor_signals(
                    target_number_iterations_additional_fill,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    true,
                    false,
                );

                test_water_injection_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                test_water_injection_receive_information_for_datalogger(&rx_datalogger_from_refill);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };

                let result = water_injection.inject_water(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, false);
            });
        });
    }

    #[test]
    // this test case executes inject_water function with simulation of level switch stuck at low position
    pub fn test_water_injection_switch_stuck_low() {
        let (_, mut refill, _) = prepare_refill_tests();
        let mut water_injection = WaterInjection;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (_tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_water_injection_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_water_injection_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                // Switch is stuck at low position. Check max limitation of refill
                let target_number_iterations_additional_fill =
                    (refill.config.max_refill_volume / refill.config.refill_pump_flow) as u32 * 10;
                run_main_cycle_with_sensor_signals(
                    target_number_iterations_additional_fill,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );

                test_water_injection_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                test_water_injection_receive_information_for_datalogger(&rx_datalogger_from_refill);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };

                let result = water_injection.inject_water(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, false);
            });
        });
    }

    #[test]
    // this test case executes inject_water function and sends Quit signal after some time when water level is still low
    pub fn test_water_injection_quit_signal() {
        let (_, mut refill, _) = prepare_refill_tests_tertiary();
        let mut water_injection = WaterInjection;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_test_case_to_runner, rx_runner_from_test_case) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_water_injection_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_water_injection_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                run_main_cycle_with_sensor_signals_and_abort(
                    &rx_runner_from_test_case,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );
                println!("Water injection test: test_water_injection_quit_signal: Test runner terminated.");                
                test_water_injection_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                test_water_injection_receive_information_for_datalogger(&rx_datalogger_from_refill);
                println!("Water injection test: test_water_injection_quit_signal: Test environment completed execution.");
            });

            scope.spawn(move || {
                let target_duration = 0.15 / refill.config.refill_pump_flow;
                let sleep_duration_refill = Duration::new(target_duration as u64 + 1, 0);
                let sleep_duration_processing = Duration::new(1, 0);
                let spin_sleeper = SpinSleeper::default();

                spin_sleeper.sleep(sleep_duration_refill);
                // send Quit signal
                match tx_signal_handler_to_refill.send(InternalCommand::Quit) {
                    Ok(_) => {},
                    Err(e) => {
                        panic!("water_injection: test_water_injection_quit_signal\
                            Error when sending Quit command to test object: {e:?}");
                    }
                }
                println!("Water injection test: test_water_injection_quit_signal: sent Quit command to test object.");                

                // give control thread some time to process the Quit signal
                spin_sleeper.sleep(sleep_duration_processing);

                // now we can stop the thread simulating the GPIO response               
                match tx_test_case_to_runner.send(InternalCommand::Quit) {
                    Ok(_) => {},
                    Err(e) => {
                        panic!("water_injection: test_water_injection_quit_signal\
                            Error when sending Quit command to test runner: {e:?}");
                    }
                }
                println!("Water injection test: test_water_injection_quit_signal: sent Quit command to test runner.");                
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };

                let result = water_injection.inject_water(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, true);
            });
        });
    }

    #[test]
    // Test case xecutes inject_water function and sets the sensor invalid signal to true after defined number of iterations
    // Expectation is that water injection ceases immediately.
    pub fn test_water_injection_delayed_switch_high_with_invalid_true() {
        let (_, mut refill, _) = prepare_refill_tests();
        let mut water_injection = WaterInjection;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (_tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_water_injection_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_water_injection_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                let target_number_iterations_main_fill =
                    (0.15 / refill.config.refill_pump_flow) as u32 * 10 + 1;
                // leave switch at low for some time
                run_main_cycle_with_sensor_signals(
                    target_number_iterations_main_fill,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );
                // Now let's simulate invalid sensor signal while position signal stays low.
                // Expected behavior: Refill operation shall abort immediately.
                run_main_cycle_with_sensor_signals(
                    1,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    true,
                );

                test_water_injection_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                test_water_injection_receive_information_for_datalogger(&rx_datalogger_from_refill);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };

                let result = water_injection.inject_water(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, false);
            });
        });
    }
}
