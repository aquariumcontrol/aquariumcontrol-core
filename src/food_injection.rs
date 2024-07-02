/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use crate::channel_content::{ActuatorState, AquariumDevice, InternalCommand};
use crate::feed::FeedConfig;
use crate::feedpattern::Feedpattern;

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
        config: &FeedConfig,
        feedpattern: &Feedpattern,
    );
}

/// Struct implements the FoodInjectionTrait for executing the feed pattern.
/// It also contains state attributes for the actuators.
pub struct FoodInjection {
    skimmer_state: ActuatorState,
    main_pump1_state: ActuatorState,
    main_pump2_state: ActuatorState,
    aux_pump1_state: ActuatorState,
    aux_pump2_state: ActuatorState,
    feeder_state: ActuatorState,
}

impl FoodInjection {
    /// Provide a struct of type FoodInjection
    pub fn new() -> FoodInjection {
        FoodInjection {
            skimmer_state: ActuatorState::Undefined,
            main_pump1_state: ActuatorState::Undefined,
            main_pump2_state: ActuatorState::Undefined,
            aux_pump1_state: ActuatorState::Undefined,
            aux_pump2_state: ActuatorState::Undefined,
            feeder_state: ActuatorState::Undefined,
        }
    }

    fn switch_skimmer_pumps(
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
            let command = match target_skimmer_state {
                ActuatorState::On => InternalCommand::SwitchOn(AquariumDevice::Skimmer),
                ActuatorState::Off => InternalCommand::SwitchOff(AquariumDevice::Skimmer),
                _ => {
                    println!("food_injection: Error - received undefined state as target state for skimmer.");
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
                            println!("Refill: Error when trying to receive answer from Controllino when switching on skimmer: {e:?}")
                        }
                    };
                }
                Err(e) => {
                    println!("Refill: Error in channel communication to Controllino when switching on skimmer: {e:?}");
                }
            }
        }
        if self.main_pump1_state != target_main_pump1_state {
            let command = match target_main_pump1_state {
                ActuatorState::On => InternalCommand::SwitchOn(AquariumDevice::MainPump1),
                ActuatorState::Off => InternalCommand::SwitchOff(AquariumDevice::MainPump1),
                _ => {
                    println!("food_injection: Error - received undefined state as target state for main pump 1.");
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
                            println!("Refill: Error when trying to receive answer from Controllino when switching on main pump 1: {e:?}")
                        }
                    };
                }
                Err(e) => {
                    println!("Refill: Error in channel communication to Controllino when switching on main pump 1: {e:?}");
                }
            }
        }
        if self.main_pump2_state != target_main_pump2_state {
            let command = match target_main_pump2_state {
                ActuatorState::On => InternalCommand::SwitchOn(AquariumDevice::MainPump2),
                ActuatorState::Off => InternalCommand::SwitchOff(AquariumDevice::MainPump2),
                _ => {
                    println!("food_injection: Error - received undefined state as target state for main pump 2.");
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
                            println!("Refill: Error when trying to receive answer from Controllino when switching on main pump 2: {e:?}")
                        }
                    };
                }
                Err(e) => {
                    println!("Refill: Error in channel communication to Controllino when switching on main pump 2: {e:?}");
                }
            }
        }
        if self.aux_pump1_state != target_aux_pump1_state {
            let command = match target_aux_pump1_state {
                ActuatorState::On => InternalCommand::SwitchOn(AquariumDevice::AuxPump1),
                ActuatorState::Off => InternalCommand::SwitchOff(AquariumDevice::AuxPump1),
                _ => {
                    println!("food_injection: Error - received undefined state as target state for aux pump 1.");
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
                            println!("Refill: Error when trying to receive answer from Controllino when switching on aux pump 1: {e:?}")
                        }
                    };
                }
                Err(e) => {
                    println!("Refill: Error in channel communication to Controllino when switching on aux pump 1: {e:?}");
                }
            }
        }
        if self.aux_pump2_state != target_aux_pump2_state {
            let command = match target_aux_pump2_state {
                ActuatorState::On => InternalCommand::SwitchOn(AquariumDevice::AuxPump2),
                ActuatorState::Off => InternalCommand::SwitchOff(AquariumDevice::AuxPump2),
                _ => {
                    println!("food_injection: Error - received undefined state as target state for aux pump 2.");
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
                            println!("Refill: Error when trying to receive answer from Controllino when switching on aux pump 2: {e:?}")
                        }
                    };
                }
                Err(e) => {
                    println!("Refill: Error in channel communication to Controllino when switching on aux pump 2: {e:?}");
                }
            }
        }
        if self.feeder_state != target_feeder_state {
            let command = match target_feeder_state {
                ActuatorState::On => InternalCommand::SwitchOn(AquariumDevice::Feeder),
                ActuatorState::Off => InternalCommand::SwitchOff(AquariumDevice::Feeder),
                _ => {
                    println!("food_injection: Error - received undefined state as target state for feeder.");
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
                            println!("Refill: Error when trying to receive answer from Controllino when switching on feeder: {e:?}")
                        }
                    };
                }
                Err(e) => {
                    println!("Refill: Error in channel communication to Controllino when switching on aux feeder: {e:?}");
                }
            }
        }
    }
}

/// Checks if there are new messages in the channel.
/// if the message contains the quit command, then the function returns true, otherwise false.
/// # Arguments
/// * `rx_from_signal_handler` - Reference to receiver part of channel for communication from signal handler
pub fn check_pending_external_request(rx_from_signal_handler: &Receiver<InternalCommand>) -> bool {
    match rx_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Quit => true,
            _ => {
                println!("Feed: ignoring inapplicable command from signal handler.");
                false
            }
        },
        Err(e) => {
            match e {
                TryRecvError::Empty => { /* empty buffer - no action required */ }
                TryRecvError::Disconnected => {
                    println!("Feed: Error in trying to receive command from signal handler: {e:?}");
                }
            }
            false
        }
    }
}

impl FoodInjectionTrait for FoodInjection {
    fn inject_food(
        &mut self,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        _config: &FeedConfig,
        feedpattern: &Feedpattern,
    ) {
        let spin_sleeper = SpinSleeper::default();
        let sleep_interval_millis: i16 = 100;
        let sleep_duration_hundred_milli_sec = Duration::from_millis(sleep_interval_millis as u64);
        let mut target_skimmer_state: ActuatorState;
        let mut target_main_pump1_state: ActuatorState;
        let mut target_main_pump2_state: ActuatorState;
        let mut target_aux_pump1_state: ActuatorState;
        let mut target_aux_pump2_state: ActuatorState;
        let mut sleep_counter_millis: i16;

        for feedphase in &feedpattern.feedphases {
            // check if there is a quit request
            if check_pending_external_request(rx_from_signal_handler) {
                break;
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
                self.switch_skimmer_pumps(
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
            while sleep_counter_millis < feedphase.pause_duration
                && !check_pending_external_request(rx_from_signal_handler)
            {
                sleep_counter_millis += sleep_interval_millis;
                spin_sleeper.sleep(sleep_duration_hundred_milli_sec);
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
                self.switch_skimmer_pumps(
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
            while sleep_counter_millis < feedphase.feed_duration
                && !check_pending_external_request(rx_from_signal_handler)
            {
                sleep_counter_millis += sleep_interval_millis;
                spin_sleeper.sleep(sleep_duration_hundred_milli_sec);
            }
        }
        self.switch_skimmer_pumps(
            ActuatorState::On,
            ActuatorState::On,
            ActuatorState::On,
            ActuatorState::On,
            ActuatorState::On,
            ActuatorState::On,
            tx_to_controllino,
            rx_from_controllino,
        );
    }
}
