/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::error;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

/// Trait for the execution of the Balling mineral dosing.
/// This trait allows running the main control with a mock implementation for testing.
pub trait MineralInjectionTrait {
    /// Actuates the peristaltic pumps for injecting a mineral solution into the main tank.
    /// # Arguments
    /// * `tx_to_relay_manager` - Reference to sender part of the channel for communication to relay manager
    /// * `rx_from_relay_manager` - Reference to Receiver part of the channel for communication from relay manager
    /// * `rx_from_signal_handler` - Reference to receiver part of the channel for communication from signal handler
    /// * `mineral_pump` - pump to be actuated of type AquariumDevice
    /// * `dosing_duration_millis` - dosing duration in milliseconds
    fn inject_mineral(
        &mut self,
        tx_to_relay_manager: &Sender<InternalCommand>,
        rx_from_relay_manager: &Receiver<bool>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        mineral_pump: AquariumDevice,
        dosing_duration_millis: u32,
    ) -> (bool, u32);
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Struct implements the MineralInjectionTrait for executing the Balling mineral dosing.
/// It also contains state attributes for the actuators.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     mineral_injection[Mineral injection] --> relay_manager[Relay Manager]
///     relay_manager --> mineral_injection
///     signal_handler[Signal handler] --> mineral_injection
/// ```
pub struct MineralInjection;

impl MineralInjection {
    /// Provide a struct of type MineralInjection
    pub fn new() -> MineralInjection {
        MineralInjection
    }
}

impl MineralInjectionTrait for MineralInjection {
    /// Switches on one of the peristaltic pumps for a defined period of time, or until a Quit command is received from the signal handler.
    /// # Arguments
    /// * `tx_to_relay_manager` - Reference to sender part of the channel for communication with relay manager
    /// * `rx_from_relay_manager` - Reference to receiver part of the channel for communication with relay manager
    /// * `rx_from_signal_handler` - Reference to receiver part of the channel for communication with signal handler
    /// * `mineral_pump` - parameter defining which pump shall be switched on
    /// * `dosing_duration_millis` - dosing duration in milliseconds    
    fn inject_mineral(
        &mut self,
        tx_to_relay_manager: &Sender<InternalCommand>,
        rx_from_relay_manager: &Receiver<bool>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        mineral_pump: AquariumDevice,
        dosing_duration_millis: u32,
    ) -> (bool, u32) {
        const LOG_TARGET_MINERAL_INJECTION: &str = "mineral_injection";
        let spin_sleeper = SpinSleeper::default();
        let sleep_interval_millis: u32 = 1;
        let sleep_duration = Duration::from_millis(sleep_interval_millis as u64);
        let mut quit_command_received: bool;
        let mut dosing_counter = 0;

        match tx_to_relay_manager.send(InternalCommand::SwitchOn(mineral_pump.clone())) {
            Ok(_) => {
                // now listen for the response
                match rx_from_relay_manager.recv() {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_MINERAL_INJECTION,
                            "receiving answer from relay manager when switching on dosing pump failed ({e:?})"
                        );
                    }
                };
            }
            Err(e) => {
                error!(
                    target: LOG_TARGET_MINERAL_INJECTION,
                    "channel communication to relay manager when switching on dosing pump failed ({e:?})"
                );
            }
        }

        loop {
            // check if there is a quit request
            (quit_command_received, _, _) =
                self.process_external_request(rx_from_signal_handler, None);
            if quit_command_received {
                break; // stop dosing
            }

            spin_sleeper.sleep(sleep_duration);

            if dosing_counter >= dosing_duration_millis {
                break; // stop dosing
            } else {
                dosing_counter = dosing_counter.saturating_add(sleep_interval_millis);
            }
        }

        match tx_to_relay_manager.send(InternalCommand::SwitchOff(mineral_pump)) {
            Ok(_) => {
                // now listen for the response
                match rx_from_relay_manager.recv() {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_MINERAL_INJECTION,
                            "receiving answer from relay manager when switching off dosing pump failed ({e:?})"
                        );
                    }
                };
            }
            Err(e) => {
                error!(
                    target: LOG_TARGET_MINERAL_INJECTION,
                    "channel communication to relay manager when switching off dosing pump failed ({e:?})"
                );
            }
        }

        (quit_command_received, dosing_counter)
    }
}

#[cfg(test)]
pub mod tests {
    use all_asserts::{assert_ge, assert_le};
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread::scope;
    use std::time::Duration;

    use crate::mineral::mineral_injection::{MineralInjection, MineralInjectionTrait};
    use crate::utilities::channel_content::{AquariumDevice, InternalCommand};

    fn test_mineral_injection_receive_actuation_command_send_confirmation(
        tx_relay_manager_to_mineral_injection: &Sender<bool>,
        rx_relay_manager_from_mineral_injection: &Receiver<InternalCommand>,
    ) -> InternalCommand {
        let command: InternalCommand;

        // receive command from the test object
        match rx_relay_manager_from_mineral_injection.recv() {
            Ok(c) => match c {
                InternalCommand::SwitchOn(_) => command = c,
                InternalCommand::SwitchOff(_) => command = c,
                _ => {
                    panic!("MineralInjection: Received invalid command from the test object:");
                }
            },
            Err(e) => {
                panic!(
                    "MineralInjection: Error when receiving actuation command from the test object: {e:?}"
                );
            }
        }

        // send confirmation back
        match tx_relay_manager_to_mineral_injection.send(true) {
            Ok(_) => {}
            Err(e) => {
                panic!("MineralInjection: Error when sending back actuation command confirmation to test object: {e:?}");
            }
        }

        command
    }

    // This test case executes the mineral injection without interruption from the signal handler.
    #[test]
    pub fn test_mineral_dosing_happy_case() {
        let (tx_mineral_injection_to_relay_manager, rx_relay_manager_from_mineral_injection) =
            mpsc::channel();
        let (tx_relay_manager_to_mineral_injection, rx_mineral_injection_from_relay_manager) =
            mpsc::channel();
        let (_tx_signal_handler_to_mineral_injection, rx_mineral_injection_from_signal_handler) =
            mpsc::channel();

        let mut mineral_injection = MineralInjection::new();

        let target_dosing_duration_millis = 500;

        scope(|scope| {
            // thread for test environment
            scope.spawn(move || {
                // allow switching on peristaltic pump
                test_mineral_injection_receive_actuation_command_send_confirmation(
                    &tx_relay_manager_to_mineral_injection,
                    &rx_relay_manager_from_mineral_injection,
                );

                // allow switching off peristaltic pump
                test_mineral_injection_receive_actuation_command_send_confirmation(
                    &tx_relay_manager_to_mineral_injection,
                    &rx_relay_manager_from_mineral_injection,
                );
            });

            // thread for the test object
            scope.spawn(move || {
                let (quit_command_received, actual_dosing_duration_millis) = mineral_injection
                    .inject_mineral(
                        &tx_mineral_injection_to_relay_manager,
                        &rx_mineral_injection_from_relay_manager,
                        &rx_mineral_injection_from_signal_handler,
                        AquariumDevice::PeristalticPump1,
                        target_dosing_duration_millis,
                    );
                assert_eq!(quit_command_received, false);
                assert_eq!(actual_dosing_duration_millis, target_dosing_duration_millis);
            });
        });
    }

    // This test case executes the mineral injection with interruption from the signal handler.
    #[test]
    pub fn test_mineral_dosing_interrupted() {
        let (tx_mineral_injection_to_relay_manager, rx_relay_manager_from_mineral_injection) =
            mpsc::channel();
        let (tx_relay_manager_to_mineral_injection, rx_mineral_injection_from_relay_manager) =
            mpsc::channel();
        let (tx_signal_handler_to_mineral_injection, rx_mineral_injection_from_signal_handler) =
            mpsc::channel();

        let mut mineral_injection = MineralInjection::new();

        let target_dosing_duration_millis = 500;

        let spin_sleeper = SpinSleeper::default();
        let sleep_interval_millis: u32 = 100;
        let sleep_duration = Duration::from_millis(sleep_interval_millis as u64);
        scope(|scope| {
            // thread for signal handler
            scope.spawn(move || {
                spin_sleeper.sleep(sleep_duration);
                let _ = tx_signal_handler_to_mineral_injection.send(InternalCommand::Quit);
            });

            // thread for rest of test environment
            scope.spawn(move || {
                // allow switching on peristaltic pump
                test_mineral_injection_receive_actuation_command_send_confirmation(
                    &tx_relay_manager_to_mineral_injection,
                    &rx_relay_manager_from_mineral_injection,
                );

                // allow switching off peristaltic pump
                test_mineral_injection_receive_actuation_command_send_confirmation(
                    &tx_relay_manager_to_mineral_injection,
                    &rx_relay_manager_from_mineral_injection,
                );
            });

            // thread for the test object
            scope.spawn(move || {
                let (quit_command_received, actual_dosing_duration_millis) = mineral_injection
                    .inject_mineral(
                        &tx_mineral_injection_to_relay_manager,
                        &rx_mineral_injection_from_relay_manager,
                        &rx_mineral_injection_from_signal_handler,
                        AquariumDevice::PeristalticPump1,
                        target_dosing_duration_millis,
                    );

                let lower_bound_float: f32 = sleep_interval_millis as f32 * 0.9;
                let lower_bound: u32 = lower_bound_float.round() as u32;

                assert_ge!(actual_dosing_duration_millis, lower_bound);
                assert_le!(actual_dosing_duration_millis, sleep_interval_millis);
                assert_eq!(quit_command_received, true);
            });
        });
    }
}
