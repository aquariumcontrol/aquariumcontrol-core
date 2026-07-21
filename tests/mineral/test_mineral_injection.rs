/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the physical actuation of a single mineral dosing pump.

use all_asserts::{assert_ge, assert_le};
use aquarium_control::launch::channels::{AquaReceiver, AquaSender, Channels};
use aquarium_control::mineral::mineral_injection::{MineralInjection, MineralInjectionTrait};
use aquarium_control::utilities::channel_content::{
    ActuatorState, AquariumDevice, InternalCommand,
};
use spin_sleep::SpinSleeper;
use std::thread::scope;
use std::time::Duration;

/// Simulates the Relay Manager's behavior for Mineral Injection tests.
///
/// This helper function acts as a mock for the Relay Manager. It waits to receive
/// either a `SwitchOn` or `SwitchOff` command from the `MineralInjection` test object.
/// Upon receiving a command, it sends a `true` acknowledgment back to the sender
/// and returns the received command. This is used to verify that the
/// `MineralInjection` module sends the correct commands.
///
/// Arguments:
/// * `tx_relay_manager_to_mineral_injection`: The sender channel for this mock to send
///   acknowledgments back to the `MineralInjection` test object.
/// * `rx_relay_manager_from_mineral_injection`: The receiver channel for this mock to get
///   actuation commands from the `MineralInjection` test object.
///
/// Returns:
/// The `InternalCommand` that was received from the `MineralInjection` test object.
///
/// Panics:
/// This function will panic if:
/// - It fails to receive a command from the `MineralInjection` test object.
/// - It receives an `InternalCommand` that is neither `SwitchOn` nor `SwitchOff`.
/// - It fails to send the acknowledgment back.
fn test_mineral_injection_receive_actuation_command_send_confirmation(
    tx_relay_manager_to_mineral_injection: &mut AquaSender<bool>,
    rx_relay_manager_from_mineral_injection: &mut AquaReceiver<InternalCommand>,
) -> InternalCommand {
    let command: InternalCommand;

    // receive command from the test object
    match rx_relay_manager_from_mineral_injection.blocking_recv() {
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

/// Helper function to record the actuation state of the devices.
///
/// # Arguments
/// * `command` - The command string to verify.
/// * `peristaltic_pump1_actuation_state` - A parameter representing `peristaltic_pump1_actuation_state`.
/// * `peristaltic_pump2_actuation_state` - A parameter representing `peristaltic_pump2_actuation_state`.
/// * `peristaltic_pump3_actuation_state` - A parameter representing `peristaltic_pump3_actuation_state`.
/// * `peristaltic_pump4_actuation_state` - A parameter representing `peristaltic_pump4_actuation_state`.
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

/// Performs the `mineral_injection_test_environment` operation.
///
/// # Arguments
/// * `tx_relay_manager_to_mineral_injection` - A parameter representing `tx_relay_manager_to_mineral_injection`.
/// * `rx_relay_manager_from_mineral_injection` - A parameter representing `rx_relay_manager_from_mineral_injection`.
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
// Verifies that mineral dosing happy case.
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
            assert!(!quit_command_received);
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
// Verifies that mineral dosing interrupted.
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
                .tx_signal_handler_to_balling
                .send(InternalCommand::Quit);
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
            assert!(quit_command_received);
        });
    });
}
