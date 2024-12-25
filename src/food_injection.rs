/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::error;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::channel_content::{ActuatorState, AquariumDevice, InternalCommand};
use crate::feed::FeedConfig;
use crate::feedpattern::Feedpattern;
use crate::proc_ext_req::ProcessExternalRequestTrait;

/// Trait for the execution of the feed pattern.
/// This trait allows running the main control with a mock implementation for testing.
pub trait FoodInjectionTrait {
    /// Actuates the refill pump for injecting water into the main tank.
    /// # Arguments
    /// * `tx_to_controllino` - Reference to sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Reference to Receiver part of channel for communication from Controllino
    /// * `rx_from_signal_handler` - Reference to receiver part of channel for communication from signal handler   
    /// * `config` - Reference to configuration of feed control
    /// * `feed_pattern` - Reference to struct holding the description of the feed pattern    
    fn inject_food(
        &mut self,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        feedpattern: &Feedpattern,
    ) -> bool;
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Struct implements the FoodInjectionTrait for executing the feed pattern.
/// It also contains state attributes for the actuators.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     food_injection[Food injection] --> controllino[Controllino]
///     controllino --> food_injection
///     signal_handler[Signal handler] --> food_injection
/// ```

pub struct FoodInjection {
    skimmer_state: ActuatorState,
    main_pump1_state: ActuatorState,
    main_pump2_state: ActuatorState,
    aux_pump1_state: ActuatorState,
    aux_pump2_state: ActuatorState,
    feeder_state: ActuatorState,
    spin_sleeper: SpinSleeper,
    sleep_duration_device_switch: Duration,
}

impl FoodInjection {
    const LOG_TARGET_FOOD_INJECTION: &'static str = "food_injection";

    /// Provide a struct of type FoodInjection
    pub fn new(config: &FeedConfig) -> FoodInjection {
        FoodInjection {
            skimmer_state: ActuatorState::Undefined,
            main_pump1_state: ActuatorState::Undefined,
            main_pump2_state: ActuatorState::Undefined,
            aux_pump1_state: ActuatorState::Undefined,
            aux_pump2_state: ActuatorState::Undefined,
            feeder_state: ActuatorState::Undefined,
            spin_sleeper: SpinSleeper::default(),
            sleep_duration_device_switch: Duration::from_millis(
                config.device_switch_delay_millis as u64,
            ),
        }
    }

    fn switch_skimmer_pumps_feeder(
        &mut self,
        target_skimmer_state: ActuatorState,
        target_main_pump1_state: ActuatorState,
        target_main_pump2_state: ActuatorState,
        target_aux_pump1_state: ActuatorState,
        target_aux_pump2_state: ActuatorState,
        target_feeder_state: ActuatorState,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
    ) {
        if self.skimmer_state != target_skimmer_state {
            self.spin_sleeper.sleep(self.sleep_duration_device_switch);
            let command = match target_skimmer_state {
                ActuatorState::On => InternalCommand::SwitchOn(AquariumDevice::Skimmer),
                ActuatorState::Off => InternalCommand::SwitchOff(AquariumDevice::Skimmer),
                _ => {
                    error!(
                        target: Self::LOG_TARGET_FOOD_INJECTION,
                        "received undefined state as target state for skimmer."
                    );
                    return;
                }
            };
            match tx_to_controllino.send(command) {
                Ok(_) => {
                    match rx_from_controllino.recv() {
                        Ok(_) => {
                            self.skimmer_state = target_skimmer_state;
                        }
                        Err(e) => {
                            error!(
                                target: Self::LOG_TARGET_FOOD_INJECTION,
                                "receiving answer from Controllino when switching on skimmer failed ({e:?})"
                            );
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: Self::LOG_TARGET_FOOD_INJECTION,
                        "channel communication to Controllino when switching on skimmer failed ({e:?})"
                    );
                }
            }
        }
        if self.main_pump1_state != target_main_pump1_state {
            self.spin_sleeper.sleep(self.sleep_duration_device_switch);
            let command = match target_main_pump1_state {
                ActuatorState::On => InternalCommand::SwitchOn(AquariumDevice::MainPump1),
                ActuatorState::Off => InternalCommand::SwitchOff(AquariumDevice::MainPump1),
                _ => {
                    error!(
                        target: Self::LOG_TARGET_FOOD_INJECTION,
                        "received undefined state as target state for main pump 1."
                    );
                    return;
                }
            };
            match tx_to_controllino.send(command) {
                Ok(_) => {
                    match rx_from_controllino.recv() {
                        Ok(_) => {
                            self.main_pump1_state = target_main_pump1_state;
                        }
                        Err(e) => {
                            error!(
                                target: Self::LOG_TARGET_FOOD_INJECTION,
                                "receiving answer from Controllino when switching on main pump 1 failed ({e:?})"
                            );
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: Self::LOG_TARGET_FOOD_INJECTION,
                        "channel communication to Controllino when switching on main pump 1 failed ({e:?})"
                    );
                }
            }
        }
        if self.main_pump2_state != target_main_pump2_state {
            self.spin_sleeper.sleep(self.sleep_duration_device_switch);
            let command = match target_main_pump2_state {
                ActuatorState::On => InternalCommand::SwitchOn(AquariumDevice::MainPump2),
                ActuatorState::Off => InternalCommand::SwitchOff(AquariumDevice::MainPump2),
                _ => {
                    error!(
                        target: Self::LOG_TARGET_FOOD_INJECTION,
                        "received undefined state as target state for main pump 2."
                    );
                    return;
                }
            };
            match tx_to_controllino.send(command) {
                Ok(_) => {
                    match rx_from_controllino.recv() {
                        Ok(_) => {
                            self.main_pump2_state = target_main_pump2_state;
                        }
                        Err(e) => {
                            error!(
                                target: Self::LOG_TARGET_FOOD_INJECTION,
                                "receiving answer from Controllino when switching on main pump 2 failed ({e:?})"
                            );
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: Self::LOG_TARGET_FOOD_INJECTION,
                        "channel communication to Controllino when switching on main pump 2 failed ({e:?})"
                    );
                }
            }
        }
        if self.aux_pump1_state != target_aux_pump1_state {
            self.spin_sleeper.sleep(self.sleep_duration_device_switch);
            let command = match target_aux_pump1_state {
                ActuatorState::On => InternalCommand::SwitchOn(AquariumDevice::AuxPump1),
                ActuatorState::Off => InternalCommand::SwitchOff(AquariumDevice::AuxPump1),
                _ => {
                    error!(
                        target: Self::LOG_TARGET_FOOD_INJECTION,
                        "received undefined state as target state for aux pump 1."
                    );
                    return;
                }
            };
            match tx_to_controllino.send(command) {
                Ok(_) => {
                    match rx_from_controllino.recv() {
                        Ok(_) => {
                            self.aux_pump1_state = target_aux_pump1_state;
                        }
                        Err(e) => {
                            error!(
                                target: Self::LOG_TARGET_FOOD_INJECTION,
                                "receiving answer from Controllino when switching on aux pump 1 failed ({e:?})"
                            );
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: Self::LOG_TARGET_FOOD_INJECTION,
                        "channel communication to Controllino when switching on aux pump 1 failed ({e:?})"
                    );
                }
            }
        }
        if self.aux_pump2_state != target_aux_pump2_state {
            self.spin_sleeper.sleep(self.sleep_duration_device_switch);
            let command = match target_aux_pump2_state {
                ActuatorState::On => InternalCommand::SwitchOn(AquariumDevice::AuxPump2),
                ActuatorState::Off => InternalCommand::SwitchOff(AquariumDevice::AuxPump2),
                _ => {
                    error!(
                        target: Self::LOG_TARGET_FOOD_INJECTION,
                        "received undefined state as target state for aux pump 2."
                    );
                    return;
                }
            };
            match tx_to_controllino.send(command) {
                Ok(_) => {
                    match rx_from_controllino.recv() {
                        Ok(_) => {
                            self.aux_pump2_state = target_aux_pump2_state;
                        }
                        Err(e) => {
                            error!(
                                target: Self::LOG_TARGET_FOOD_INJECTION,
                                "receiving answer from Controllino when switching on aux pump 2 failed ({e:?})"
                            );
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: Self::LOG_TARGET_FOOD_INJECTION,
                        "channel communication to Controllino when switching on aux pump 2 failed ({e:?})"
                    );
                }
            }
        }
        if self.feeder_state != target_feeder_state {
            self.spin_sleeper.sleep(self.sleep_duration_device_switch);
            let command = match target_feeder_state {
                ActuatorState::On => InternalCommand::SwitchOn(AquariumDevice::Feeder),
                ActuatorState::Off => InternalCommand::SwitchOff(AquariumDevice::Feeder),
                _ => {
                    error!(
                        target: Self::LOG_TARGET_FOOD_INJECTION,
                        "received undefined state as target state for feeder."
                    );
                    return;
                }
            };
            match tx_to_controllino.send(command) {
                Ok(_) => {
                    match rx_from_controllino.recv() {
                        Ok(_) => {
                            self.feeder_state = target_feeder_state;
                        }
                        Err(e) => {
                            error!(
                                target: Self::LOG_TARGET_FOOD_INJECTION,
                                "receiving answer from Controllino when switching on feeder failed ({e:?})"
                            );
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: Self::LOG_TARGET_FOOD_INJECTION,
                        "channel communication to Controllino when switching on aux feeder failed ({e:?})"
                    );
                }
            }
        }
    }
}

impl FoodInjectionTrait for FoodInjection {
    fn inject_food(
        &mut self,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        feedpattern: &Feedpattern,
    ) -> bool {
        let spin_sleeper = SpinSleeper::default();
        let sleep_interval_millis: i16 = 1;
        let sleep_duration = Duration::from_millis(sleep_interval_millis as u64);
        let mut target_skimmer_state: ActuatorState;
        let mut target_main_pump1_state: ActuatorState;
        let mut target_main_pump2_state: ActuatorState;
        let mut target_aux_pump1_state: ActuatorState;
        let mut target_aux_pump2_state: ActuatorState;
        let mut sleep_counter_millis: i16;
        let mut quit_command_received: bool = false;

        for feedphase in &feedpattern.feedphases {
            // check if there is a quit request
            (quit_command_received, _, _) =
                self.process_external_request(rx_from_signal_handler, None);
            if quit_command_received {
                break; // exit outer loop
            }

            // keep feeder stopped
            if feedphase.pause_duration > 0 {
                target_skimmer_state = match feedphase.pause_skimmer {
                    true => ActuatorState::On,
                    false => ActuatorState::Off,
                };
                target_main_pump1_state = match feedphase.pause_main_pump_1 {
                    true => ActuatorState::On,
                    false => ActuatorState::Off,
                };
                target_main_pump2_state = match feedphase.pause_main_pump_2 {
                    true => ActuatorState::On,
                    false => ActuatorState::Off,
                };
                target_aux_pump1_state = match feedphase.pause_aux_pump_1 {
                    true => ActuatorState::On,
                    false => ActuatorState::Off,
                };
                target_aux_pump2_state = match feedphase.pause_aux_pump_2 {
                    true => ActuatorState::On,
                    false => ActuatorState::Off,
                };
                self.switch_skimmer_pumps_feeder(
                    target_skimmer_state,
                    target_main_pump1_state,
                    target_main_pump2_state,
                    target_aux_pump1_state,
                    target_aux_pump2_state,
                    ActuatorState::Off,
                    tx_to_controllino,
                    rx_from_controllino,
                );
            }

            // wait for determined period and check quit request in between
            sleep_counter_millis = 0;
            while sleep_counter_millis < feedphase.pause_duration {
                (quit_command_received, _, _) =
                    self.process_external_request(rx_from_signal_handler, None);
                if quit_command_received {
                    break; // exit inner loop
                }
                sleep_counter_millis += sleep_interval_millis;
                spin_sleeper.sleep(sleep_duration);
            }
            if quit_command_received {
                break; // exit outer loop
            }

            // run the feeder
            if feedphase.feed_duration > 0 {
                target_skimmer_state = match feedphase.feed_skimmer {
                    true => ActuatorState::On,
                    false => ActuatorState::Off,
                };
                target_main_pump1_state = match feedphase.feed_main_pump_1 {
                    true => ActuatorState::On,
                    false => ActuatorState::Off,
                };
                target_main_pump2_state = match feedphase.feed_main_pump_2 {
                    true => ActuatorState::On,
                    false => ActuatorState::Off,
                };
                target_aux_pump1_state = match feedphase.feed_aux_pump_1 {
                    true => ActuatorState::On,
                    false => ActuatorState::Off,
                };
                target_aux_pump2_state = match feedphase.feed_aux_pump_2 {
                    true => ActuatorState::On,
                    false => ActuatorState::Off,
                };
                self.switch_skimmer_pumps_feeder(
                    target_skimmer_state,
                    target_main_pump1_state,
                    target_main_pump2_state,
                    target_aux_pump1_state,
                    target_aux_pump2_state,
                    ActuatorState::On,
                    tx_to_controllino,
                    rx_from_controllino,
                );
            }

            // wait for determined period and check quit request in between
            sleep_counter_millis = 0;
            while sleep_counter_millis < feedphase.feed_duration {
                (quit_command_received, _, _) =
                    self.process_external_request(rx_from_signal_handler, None);
                if quit_command_received {
                    break; // exit inner loop
                }
                sleep_counter_millis += sleep_interval_millis;
                spin_sleeper.sleep(sleep_duration);
            }
            if quit_command_received {
                break; // exit outer loop
            }
        }

        self.switch_skimmer_pumps_feeder(
            ActuatorState::On,
            ActuatorState::On,
            ActuatorState::On,
            ActuatorState::On,
            ActuatorState::On,
            ActuatorState::Off,
            tx_to_controllino,
            rx_from_controllino,
        );

        quit_command_received
    }
}

#[cfg(test)]
pub mod tests {
    use all_asserts::{assert_ge, assert_le};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread::scope;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::channel_content::InternalCommand;
    use crate::config::ConfigData;
    use crate::feedpattern::{FeedPhase, Feedpattern};
    use crate::food_injection::{FoodInjection, FoodInjectionTrait};
    use crate::read_config_file;

    fn test_food_injection_receive_actuation_command_send_confirmation(
        tx_controllino_to_feed: &Sender<bool>,
        rx_controllino_from_feed: &Receiver<InternalCommand>,
    ) -> InternalCommand {
        // receive command from test object
        let (command, device) = match rx_controllino_from_feed.recv() {
            Ok(c) => match c {
                InternalCommand::SwitchOn(ref d) => (c.clone(), d.clone()),
                InternalCommand::SwitchOff(ref d) => (c.clone(), d.clone()),
                _ => {
                    panic!("FoodInjection: Received invalid command from test object:");
                }
            },
            Err(e) => {
                panic!(
                    "FoodInjection: Error when receiving actuation command from test object: {e:?}"
                );
            }
        };

        println!(
            "Received command {} for {} from test object",
            command, device
        );

        // send confirmation back
        match tx_controllino_to_feed.send(true) {
            Ok(_) => {}
            Err(e) => {
                panic!("FoodInjection: Error when sending back actuation command confirmation to test object: {e:?}");
            }
        }

        command
    }

    #[test]
    // this test cases executes the food injection without interruption from signal handler
    pub fn test_food_injection_uninterrupted() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        let mut food_injection = FoodInjection::new(&config.feed);

        let (tx_feed_to_controllino, rx_controllino_from_feed) = mpsc::channel();
        let (tx_controllino_to_feed, rx_feed_from_controllino) = mpsc::channel();
        let (_tx_signal_handler_to_feed, rx_feed_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                let mut counter_switch_on: i32 = 0;
                let mut counter_switch_off: i32 = 0;

                for i in 0..125 {
                    println!("test_food_injection: loop #{}", i);
                    match test_food_injection_receive_actuation_command_send_confirmation(
                        &tx_controllino_to_feed,
                        &rx_controllino_from_feed,
                    ) {
                        InternalCommand::SwitchOn(_) => {
                            counter_switch_on += 1;
                        }
                        InternalCommand::SwitchOff(_) => {
                            counter_switch_off += 1;
                        }
                        _ => {
                            // Do nothing. This case is covered with panic in lower level.
                        }
                    }
                }
                println!(
                    "counter_switch_on={} counter_switch_off={}",
                    counter_switch_on, counter_switch_off
                );
                assert_eq!(counter_switch_on, 65);
                assert_eq!(counter_switch_off, 60);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let feedphase = FeedPhase::new(
                    200, true, true, true, true, true, 200, false, false, false, false, false,
                );
                // Each phase will trigger switching on 6 devices and switching off 6 devices
                let mut feedphases: Vec<FeedPhase> = Vec::new();
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());

                // After all phases are executed, feeder will be switched off
                // and 5 devices will be switched on

                let feedpattern = Feedpattern {
                    profile_id: 1,
                    profile_name: "Test1".to_string(),
                    feedphases: feedphases,
                };

                let start_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                food_injection.inject_food(
                    &tx_feed_to_controllino,
                    &rx_feed_from_controllino,
                    &rx_feed_from_signal_handler,
                    &feedpattern,
                );
                let end_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

                let execution_duration = end_time - start_time;

                println!(
                    "execution_duration (milliseconds)= {}",
                    execution_duration.as_millis()
                );

                assert_ge!(execution_duration.as_millis(), 22500);
                assert_le!(execution_duration.as_millis(), 29700);
            });
        });
    }

    #[test]
    // this test cases executes the food injection with interruption from signal handler
    pub fn test_food_injection_interrupted() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        let mut food_injection = FoodInjection::new(&config.feed);

        let (tx_feed_to_controllino, rx_controllino_from_feed) = mpsc::channel();
        let (tx_controllino_to_feed, rx_feed_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_feed, rx_feed_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                let mut counter_switch_on: i32 = 0;
                let mut counter_switch_off: i32 = 0;

                for i in 0..5 {
                    println!("test_food_injection: loop #{}", i);
                    match test_food_injection_receive_actuation_command_send_confirmation(
                        &tx_controllino_to_feed,
                        &rx_controllino_from_feed,
                    ) {
                        InternalCommand::SwitchOn(_) => {
                            counter_switch_on += 1;
                        }
                        InternalCommand::SwitchOff(_) => {
                            counter_switch_off += 1;
                        }
                        _ => {
                            // Do nothing. This case is covered with panic in lower level.
                        }
                    }
                }
                println!("test_food_injection_interrupted: sending Quit signal");
                _ = tx_signal_handler_to_feed.send(InternalCommand::Quit);
                for i in 0..1 {
                    println!("test_food_injection: loop #{}", i);
                    match test_food_injection_receive_actuation_command_send_confirmation(
                        &tx_controllino_to_feed,
                        &rx_controllino_from_feed,
                    ) {
                        InternalCommand::SwitchOn(_) => {
                            counter_switch_on += 1;
                        }
                        InternalCommand::SwitchOff(_) => {
                            counter_switch_off += 1;
                        }
                        _ => {
                            // Do nothing. This case is covered with panic in lower level.
                        }
                    }
                }
                println!(
                    "counter_switch_on={} counter_switch_off={}",
                    counter_switch_on, counter_switch_off
                );
                assert_eq!(counter_switch_on, 5);
                assert_eq!(counter_switch_off, 1);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let feedphase = FeedPhase::new(
                    200, true, true, true, true, true, 200, false, false, false, false, false,
                );
                // Each phase will trigger switching on 6 devices and switching off 6 devices
                let mut feedphases: Vec<FeedPhase> = Vec::new();
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());
                feedphases.push(feedphase.clone());

                // After all phases are executed, feeder will be switched off
                // and 5 devices will be switched on

                let feedpattern = Feedpattern {
                    profile_id: 1,
                    profile_name: "Test1".to_string(),
                    feedphases: feedphases,
                };

                let start_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                food_injection.inject_food(
                    &tx_feed_to_controllino,
                    &rx_feed_from_controllino,
                    &rx_feed_from_signal_handler,
                    &feedpattern,
                );
                let end_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

                let execution_duration = end_time - start_time;

                println!(
                    "execution_duration (milliseconds)= {}",
                    execution_duration.as_millis()
                );

                assert_ge!(execution_duration.as_millis(), 850);
                assert_le!(execution_duration.as_millis(), 1085);
            });
        });
    }
}
