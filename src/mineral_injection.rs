/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use crate::channel_content::{AquariumDevice, InternalCommand};

/// Trait for the execution of the Balling mineral dosing.
/// This trait allows running the main control with a mock implementation for testing.
pub trait MineralInjectionTrait {
    /// Actuates the peristaltic pumps for injecting mineral solution into the main tank.
    /// # Arguments
    /// * `tx_to_controllino` - Reference to sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Reference to Receiver part of channel for communication from Controllino
    /// * `rx_from_signal_handler` - Reference to receiver part of channel for communication from signal handler   
    /// * `config` - Reference to configuration of Balling dosing control
    /// * `pump_id` - ID of pump to be actuated
    /// * `dosing_duration_millis` - dosing duration in milliseconds
    fn inject_mineral(
        &mut self,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        mineral_pump: AquariumDevice,
        dosing_duration_millis: u32,
    ) -> (bool, u32);
}

/// Struct implements the MineralInjectionTrait for executing the Balling mineral dosing.
/// It also contains state attributes for the actuators.
pub struct MineralInjection;

impl MineralInjection {
    /// Provide a struct of type MineralInjection
    pub fn new() -> MineralInjection {
        MineralInjection
    }
}

/// Checks if there are new messages in the channel.
/// if the message contains the quit command, then the function returns true, otherwise false.
/// # Arguments
/// * `rx_from_signal_handler` - Reference to receiver part of channel for communication from signal handler
pub fn check_pending_external_request(rx_from_signal_handler: &Receiver<InternalCommand>) -> bool {
    match rx_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Quit => {
                println!("MineralInjection: Received Quit command");
                true
            }
            _ => {
                println!("MineralInjection: ignoring inapplicable command from signal handler.");
                false
            }
        },
        Err(e) => {
            match e {
                TryRecvError::Empty => { /* empty buffer - no action required */ }
                TryRecvError::Disconnected => {
                    println!("MineralInjection: Error in trying to receive command from signal handler: {e:?}");
                }
            }
            false
        }
    }
}

impl MineralInjectionTrait for MineralInjection {
    fn inject_mineral(
        &mut self,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        mineral_pump: AquariumDevice,
        dosing_duration_millis: u32,
    ) -> (bool, u32) {
        let spin_sleeper = SpinSleeper::default();
        let sleep_interval_millis: i16 = 1;
        let sleep_duration = Duration::from_millis(sleep_interval_millis as u64);
        let mut quit_command_received: bool = false;
        let mut dosing_counter = 0;

        match tx_to_controllino.send(InternalCommand::SwitchOn(mineral_pump.clone())) {
            Ok(_) => {
                // now listen for response
                match rx_from_controllino.recv() {
                    Ok(_) => {}
                    Err(e) => {
                        println!("Balling mineral dosing: Error when trying to receive answer from Controllino when switching on dosing pump: {e:?}")
                    }
                };
            }
            Err(e) => {
                println!("Balling mineral dosing: Error in channel communication to Controllino when switching on dosing pump: {e:?}");
            }
        }

        loop {
            // check if there is a quit request
            if check_pending_external_request(rx_from_signal_handler) {
                quit_command_received = true;
                break; // stop dosing
            }

            spin_sleeper.sleep(sleep_duration);

            if dosing_counter >= dosing_duration_millis {
                break; // stop dosing
            } else {
                dosing_counter += dosing_duration_millis;
            }
        }

        match tx_to_controllino.send(InternalCommand::SwitchOff(mineral_pump)) {
            Ok(_) => {
                // now listen for response
                match rx_from_controllino.recv() {
                    Ok(_) => {}
                    Err(e) => {
                        println!("Balling mineral dosing: Error when trying to receive answer from Controllino when switching off dosing pump: {e:?}")
                    }
                };
            }
            Err(e) => {
                println!("Balling mineral dosing: Error in channel communication to Controllino when switching off dosing pump: {e:?}");
            }
        }

        (quit_command_received, dosing_counter)
    }
}
