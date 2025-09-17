/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::mineral::balling_channels::BallingChannels;
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use log::error;
use spin_sleep::SpinSleeper;
use std::time::{Duration, Instant};

/// Trait for the execution of the Balling mineral dosing.
/// This trait allows running the main control with a mock implementation for testing.
pub trait MineralInjectionTrait {
    /// Actuates a specific peristaltic pump to inject a mineral solution into the main tank for a defined duration.
    ///
    /// This method controls the selected `mineral_pump` by sending `SwitchOn` and `SwitchOff` commands
    /// to the relay manager. It runs the pump for the `dosing_duration_millis` or until a
    /// `Quit` command is received from the signal handler, allowing for early termination.
    ///
    /// # Arguments
    /// * `balling_channels` - A mutable reference to the `BallingChannels` struct.
    /// * `mineral_pump` - The `AquariumDevice` enum variant identifying which peristaltic pump to actuate.
    /// * `dosing_duration_millis` - The target duration (in milliseconds) for which the pump should run.
    ///
    /// # Returns
    /// A tuple `(bool, u32)`:
    /// - The `bool` is `true` if a `Quit` command was received from the signal handler during injection,
    ///   indicating that the operation was interrupted. It's `false` otherwise.
    /// - The `u32` is the actual duration (in milliseconds) the pump ran before stopping or being interrupted.
    fn inject_mineral(
        &mut self,
        balling_channels: &mut BallingChannels,
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
    /// Switches on one of the peristaltic pumps for a defined period, or until a `Quit` command is received.
    ///
    /// This implementation for `inject_mineral` controls the specified `mineral_pump`.
    /// It sends a `SwitchOn` command to the relay manager, then enters a loop
    /// to keep the pump active for `dosing_duration_millis`. During this period,
    /// it continuously checks for a `Quit` command from the signal handler,
    /// allowing for immediate termination. Once the duration is met or a `Quit`
    /// command is received, it sends a `SwitchOff` command to the pump.
    ///
    /// # Arguments
    /// * `balling_channels` - A mutable reference to the `BallingChannels` struct.
    /// * `mineral_pump` - The `AquariumDevice` variant identifying which pump to actuate.
    /// * `dosing_duration_millis` - The target duration (in milliseconds) for which the pump should run.
    ///
    /// # Returns
    /// A tuple `(bool, u32)`:
    /// - The `bool` is `true` if a `Quit` command was received during injection; otherwise `false`.
    /// - The `u32` is the actual duration (in milliseconds) the pump ran before stopping or being interrupted.
    fn inject_mineral(
        &mut self,
        balling_channels: &mut BallingChannels,
        mineral_pump: AquariumDevice,
        dosing_duration_millis: u32,
    ) -> (bool, u32) {
        let spin_sleeper = SpinSleeper::default();
        let sleep_interval_millis: u32 = 1;
        let sleep_duration = Duration::from_millis(sleep_interval_millis as u64);
        let mut quit_command_received: bool;

        match balling_channels
            .send_to_relay_manager(InternalCommand::SwitchOn(mineral_pump.clone()))
        {
            Ok(_) => {
                // now listen for the response
                match balling_channels.receive_from_relay_manager() {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            target: module_path!(),
                            "receiving answer from relay manager when switching on dosing pump failed ({e:?})"
                        );
                    }
                };
            }
            Err(e) => {
                error!(
                    target: module_path!(),
                    "channel communication to relay manager when switching on dosing pump failed ({e:?})"
                );
            }
        }

        let instant_start_of_injection = Instant::now();
        let mut dosing_counter: u32 = 0;

        loop {
            // check if there is a quit request
            (quit_command_received, _, _) = self.process_external_request(
                &mut balling_channels.rx_balling_from_signal_handler,
                None,
            );
            if quit_command_received {
                break; // stop dosing
            }

            spin_sleeper.sleep(sleep_duration);

            dosing_counter = match Instant::now()
                .duration_since(instant_start_of_injection)
                .as_millis()
                .try_into() as Result<u32, _>
            {
                Ok(val) => val,
                Err(e) => {
                    error!(
                        target: module_path!(), "Error converting u128 to u32: {e}"
                    );
                    break; // abort dosing - dosing counter keeps current value
                }
            };

            // check if target dosing duration has been reached
            if dosing_counter > dosing_duration_millis {
                break; // stop dosing
            }
        }

        match balling_channels.send_to_relay_manager(InternalCommand::SwitchOff(mineral_pump)) {
            Ok(_) => {
                // now listen for the response
                match balling_channels.receive_from_relay_manager() {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            target: module_path!(),
                            "receiving answer from relay manager when switching off dosing pump failed ({e:?})"
                        );
                    }
                };
            }
            Err(e) => {
                error!(
                    target: module_path!(),
                    "channel communication to relay manager when switching off dosing pump failed ({e:?})"
                );
            }
        }

        (quit_command_received, dosing_counter)
    }
}

#[cfg(test)]
pub mod tests {
    use crate::launch::channels::{AquaReceiver, AquaSender, Channels};
    use crate::mineral::mineral_injection::{MineralInjection, MineralInjectionTrait};
    use crate::utilities::channel_content::{ActuatorState, AquariumDevice, InternalCommand};
    use all_asserts::{assert_ge, assert_le};
    use spin_sleep::SpinSleeper;
    use std::thread::scope;
    use std::time::Duration;

    // Simulates the Relay Manager's behavior for Mineral Injection tests.
    //
    // This helper function acts as a mock for the Relay Manager. It waits to receive
    // either a `SwitchOn` or `SwitchOff` command from the `MineralInjection` test object.
    // Upon receiving a command, it sends a `true` acknowledgment back to the sender
    // and returns the received command. This is used to verify that the
    // `MineralInjection` module sends the correct commands.
    //
    // Arguments:
    // * `tx_relay_manager_to_mineral_injection`: The sender channel for this mock to send
    //   acknowledgments back to the `MineralInjection` test object.
    // * `rx_relay_manager_from_mineral_injection`: The receiver channel for this mock to get
    //   actuation commands from the `MineralInjection` test object.
    //
    // Returns:
    // The `InternalCommand` that was received from the `MineralInjection` test object.
    //
    // Panics:
    // This function will panic if:
    // - It fails to receive a command from the `MineralInjection` test object.
    // - It receives an `InternalCommand` that is neither `SwitchOn` nor `SwitchOff`.
    // - It fails to send the acknowledgment back.
    fn test_mineral_injection_receive_actuation_command_send_confirmation(
        tx_relay_manager_to_mineral_injection: &mut AquaSender<bool>,
        rx_relay_manager_from_mineral_injection: &mut AquaReceiver<InternalCommand>,
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

    // Helper function to record the actuation state of the devices.
    fn update_actuator_states(
        command: InternalCommand,
        peristaltic_pump1_actuation_state: &mut ActuatorState,
        peristaltic_pump2_actuation_state: &mut ActuatorState,
        peristaltic_pump3_actuation_state: &mut ActuatorState,
        peristaltic_pump4_actuation_state: &mut ActuatorState,
    ) {
        // First, determine the new state and the target device from the command.
        let (device, new_state) = match command {
            InternalCommand::SwitchOn(device) => (device, ActuatorState::On),
            InternalCommand::SwitchOff(device) => (device, ActuatorState::Off),
            // If it's any other command, we don't need to do anything.
            _ => return,
        };

        // Now, update the state of the correct pump in a single match block.
        match device {
            AquariumDevice::PeristalticPump1 => {
                *peristaltic_pump1_actuation_state = new_state;
            }
            AquariumDevice::PeristalticPump2 => {
                *peristaltic_pump2_actuation_state = new_state;
            }
            AquariumDevice::PeristalticPump3 => {
                *peristaltic_pump3_actuation_state = new_state;
            }
            AquariumDevice::PeristalticPump4 => {
                *peristaltic_pump4_actuation_state = new_state;
            }
            // Ignore commands for any other devices.
            _ => {}
        }
    }

    fn mineral_injection_test_environment(
        mut tx_relay_manager_to_mineral_injection: AquaSender<bool>,
        mut rx_relay_manager_from_mineral_injection: AquaReceiver<InternalCommand>,
    ) {
        let mut peristaltic_pump1_actuation_state = ActuatorState::Off;
        let mut peristaltic_pump2_actuation_state = ActuatorState::Off;
        let mut peristaltic_pump3_actuation_state = ActuatorState::Off;
        let mut peristaltic_pump4_actuation_state = ActuatorState::Off;

        // allow switching on peristaltic pump
        let command = test_mineral_injection_receive_actuation_command_send_confirmation(
            &mut tx_relay_manager_to_mineral_injection,
            &mut rx_relay_manager_from_mineral_injection,
        );
        update_actuator_states(
            command,
            &mut peristaltic_pump1_actuation_state,
            &mut peristaltic_pump2_actuation_state,
            &mut peristaltic_pump3_actuation_state,
            &mut peristaltic_pump4_actuation_state,
        );

        // allow switching off peristaltic pump
        let command = test_mineral_injection_receive_actuation_command_send_confirmation(
            &mut tx_relay_manager_to_mineral_injection,
            &mut rx_relay_manager_from_mineral_injection,
        );
        update_actuator_states(
            command,
            &mut peristaltic_pump1_actuation_state,
            &mut peristaltic_pump2_actuation_state,
            &mut peristaltic_pump3_actuation_state,
            &mut peristaltic_pump4_actuation_state,
        );
        assert_eq!(peristaltic_pump1_actuation_state, ActuatorState::Off);
        assert_eq!(peristaltic_pump2_actuation_state, ActuatorState::Off);
        assert_eq!(peristaltic_pump3_actuation_state, ActuatorState::Off);
        assert_eq!(peristaltic_pump4_actuation_state, ActuatorState::Off);
    }

    // This test case executes the mineral injection without interruption from the signal handler.
    #[test]
    pub fn test_mineral_dosing_happy_case() {
        let mut channels = Channels::new_for_test();

        let mut mineral_injection = MineralInjection::new();

        let target_dosing_duration_millis = 500;

        scope(|scope| {
            // thread for test environment
            scope.spawn(move || {
                mineral_injection_test_environment(
                    channels.relay_manager.tx_relay_manager_to_balling,
                    channels.relay_manager.rx_relay_manager_from_balling,
                );
            });

            // thread for the test object
            scope.spawn(move || {
                let (quit_command_received, actual_dosing_duration_millis) = mineral_injection
                    .inject_mineral(
                        &mut channels.balling,
                        AquariumDevice::PeristalticPump1,
                        target_dosing_duration_millis,
                    );
                assert_eq!(quit_command_received, false);
                assert_ge!(
                    actual_dosing_duration_millis,
                    target_dosing_duration_millis - 5
                );
                assert_le!(
                    actual_dosing_duration_millis,
                    target_dosing_duration_millis + 5
                );
            });
        });
    }

    // This test case executes the mineral injection with interruption from the signal handler.
    #[test]
    pub fn test_mineral_dosing_interrupted() {
        let mut channels = Channels::new_for_test();

        let mut mineral_injection = MineralInjection::new();

        let target_dosing_duration_millis = 500;

        let spin_sleeper = SpinSleeper::default();
        let sleep_interval_millis: u32 = 100;
        let sleep_duration = Duration::from_millis(sleep_interval_millis as u64);
        scope(|scope| {
            // thread for signal handler
            scope.spawn(move || {
                spin_sleeper.sleep(sleep_duration);
                let _ = channels
                    .signal_handler
                    .send_to_balling(InternalCommand::Quit);
            });

            // thread for the rest of test environment
            scope.spawn(move || {
                mineral_injection_test_environment(
                    channels.relay_manager.tx_relay_manager_to_balling,
                    channels.relay_manager.rx_relay_manager_from_balling,
                );
            });

            // thread for the test object
            scope.spawn(move || {
                let (quit_command_received, actual_dosing_duration_millis) = mineral_injection
                    .inject_mineral(
                        &mut channels.balling,
                        AquariumDevice::PeristalticPump1,
                        target_dosing_duration_millis,
                    );

                let lower_bound_float: f32 = sleep_interval_millis as f32 * 0.9;
                let lower_bound: u32 = lower_bound_float.round() as u32;

                assert_ge!(actual_dosing_duration_millis, lower_bound);
                assert_le!(actual_dosing_duration_millis, sleep_interval_millis + 15);
                assert_eq!(quit_command_received, true);
            });
        });
    }
}
