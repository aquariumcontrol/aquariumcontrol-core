/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the physical execution of a feed profile, controlling pumps and the feeder.

use all_asserts::{assert_ge, assert_le};
use aquarium_control::food::feed_profile::{FeedPhase, FeedProfile};
use aquarium_control::food::food_injection::{FoodInjection, FoodInjectionTrait};
use aquarium_control::launch::channels::{AquaReceiver, AquaSender, Channels};
use aquarium_control::utilities::channel_content::{
    ActuatorState, AquariumDevice, InternalCommand,
};
use aquarium_control::utilities::config::{read_config_file, ConfigData};
use std::thread::scope;
use std::time::Instant;

/// Simulates the relay manager's behavior in response to actuation commands.
///
/// This helper function is used within test scopes to act as a mock for the relay manager.
/// It waits to receive a `SwitchOn` or `SwitchOff` command from the
/// `FoodInjection` test object, prints it, sends a confirmation back, and increments
/// counters for the number of on/off commands received.
///
/// # Arguments
/// * `tx_relay_manager_to_feed` - The sender channel for this mock to send acknowledgments back.
/// * `rx_relay_manager_from_feed` - The receiver channel for this mock to get actuation commands.
/// * `counter_switch_on` - A mutable counter to track the number of `SwitchOn` commands received.
/// * `counter_switch_off` - A mutable counter to track the number of `SwitchOff` commands received.
///
/// # Returns
/// The `InternalCommand` that was received from the test object.
///
/// # Panics
/// This function will panic if:
/// * It fails to receive a command from the `FoodInjection` test object.
/// * It receives an `InternalCommand` that is neither `SwitchOn` nor `SwitchOff`.
/// * It fails to send the acknowledgment back to the `FoodInjection` test object.
fn test_food_injection_receive_actuation_command_send_confirmation(
    tx_relay_manager_to_feed: &mut AquaSender<bool>,
    rx_relay_manager_from_feed: &mut AquaReceiver<InternalCommand>,
    counter_switch_on: &mut i32,
    counter_switch_off: &mut i32,
) -> InternalCommand {
    // receive command from the test object
    let (command, device) = match rx_relay_manager_from_feed.blocking_recv() {
        Ok(c) => match c {
            InternalCommand::SwitchOn(ref d) => (c.clone(), d.clone()),
            InternalCommand::SwitchOff(ref d) => (c.clone(), d.clone()),
            _ => {
                panic!("FoodInjection: Received invalid command from test object:");
            }
        },
        Err(e) => {
            panic!("FoodInjection: Error when receiving actuation command from test object: {e:?}");
        }
    };

    println!(
        "Received command {} for {} from test object",
        command, device
    );

    // send confirmation back
    match tx_relay_manager_to_feed.send(true) {
        Ok(_) => {}
        Err(e) => {
            panic!("FoodInjection: Error when sending back actuation command confirmation to test object: {e:?}");
        }
    }

    match command {
        InternalCommand::SwitchOn(_) => {
            *counter_switch_on += 1;
        }
        InternalCommand::SwitchOff(_) => {
            *counter_switch_off += 1;
        }
        _ => {
            // Do nothing. This case is covered with panic beforehand.
        }
    }
    command
}

/// Helper function to record the actuation state of the devices based on a command.
///
/// This test helper takes a command and updates the state of the corresponding
/// actuator in the provided mutable state variables. This allows tests to track
/// the expected state of the system.
///
/// # Arguments
/// * `command` - The `InternalCommand` (`SwitchOn` or `SwitchOff`) to process.
/// * `..._actuation_state` - Mutable references to the state variables for each device.
fn update_actuator_states(
    command: InternalCommand,
    skimmer_actuation_state: &mut ActuatorState,
    main_pump1_actuation_state: &mut ActuatorState,
    main_pump2_actuation_state: &mut ActuatorState,
    aux_pump1_actuation_state: &mut ActuatorState,
    aux_pump2_actuation_state: &mut ActuatorState,
    feeder_actuation_state: &mut ActuatorState,
) {
    // Determine the new state and the target device from the command.
    let (device, new_state) = match command {
        InternalCommand::SwitchOn(device) => (device, ActuatorState::On),
        InternalCommand::SwitchOff(device) => (device, ActuatorState::Off),
        // If it's any other command, we don't need to do anything.
        _ => return,
    };

    // Update the state of the correct pump in a single match block.
    match device {
        AquariumDevice::Skimmer => {
            *skimmer_actuation_state = new_state;
        }
        AquariumDevice::MainPump1 => {
            *main_pump1_actuation_state = new_state;
        }
        AquariumDevice::MainPump2 => {
            *main_pump2_actuation_state = new_state;
        }
        AquariumDevice::AuxPump1 => {
            *aux_pump1_actuation_state = new_state;
        }
        AquariumDevice::AuxPump2 => {
            *aux_pump2_actuation_state = new_state;
        }
        AquariumDevice::Feeder => {
            *feeder_actuation_state = new_state;
        }
        // Ignore commands for any other devices.
        _ => {}
    }
}

#[test]
// this test case executes the food injection without interruption from the signal handler
pub fn test_food_injection_uninterrupted() {
    let config: ConfigData =
        read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let mut food_injection = FoodInjection::new(&config.feed);

    let mut channels = Channels::new_for_test();

    scope(|scope| {
        // this scope is providing the test environment
        scope.spawn(move || {
            let mut counter_switch_on: i32 = 0;
            let mut counter_switch_off: i32 = 0;
            let mut skimmer_actuation_state = ActuatorState::On;
            let mut main_pump1_actuation_state = ActuatorState::On;
            let mut main_pump2_actuation_state = ActuatorState::On;
            let mut aux_pump1_actuation_state = ActuatorState::On;
            let mut aux_pump2_actuation_state = ActuatorState::On;
            let mut feeder_actuation_state = ActuatorState::Off;

            for i in 0..126 {
                println!("test_food_injection: loop #{}", i);
                let command = test_food_injection_receive_actuation_command_send_confirmation(
                    &mut channels.relay_manager.tx_relay_manager_to_feed,
                    &mut channels.relay_manager.rx_relay_manager_from_feed,
                    &mut counter_switch_on,
                    &mut counter_switch_off,
                );
                update_actuator_states(
                    command,
                    &mut skimmer_actuation_state,
                    &mut main_pump1_actuation_state,
                    &mut main_pump2_actuation_state,
                    &mut aux_pump1_actuation_state,
                    &mut aux_pump2_actuation_state,
                    &mut feeder_actuation_state,
                );
            }
            println!(
                "counter_switch_on={} counter_switch_off={}",
                counter_switch_on, counter_switch_off
            );
            assert_eq!(counter_switch_on, 65);
            assert_eq!(counter_switch_off, 61);
            assert_eq!(skimmer_actuation_state, ActuatorState::On);
            assert_eq!(main_pump1_actuation_state, ActuatorState::On);
            assert_eq!(main_pump2_actuation_state, ActuatorState::On);
            assert_eq!(aux_pump1_actuation_state, ActuatorState::On);
            assert_eq!(aux_pump2_actuation_state, ActuatorState::On);
            assert_eq!(feeder_actuation_state, ActuatorState::Off);
        });

        // this scope is executing the test object
        scope.spawn(move || {
            let feedphase = FeedPhase::new(
                200, true, true, true, true, true, 200, false, false, false, false, false,
            );
            // Each phase will trigger switching on 6 devices and switching off 6 devices
            let feedphases = vec![feedphase.clone(); 10];

            // After all phases are executed, the feeder will be switched off
            // and 5 devices will be switched on

            let feed_profile = FeedProfile {
                profile_id: 1,
                profile_name: "Test1".to_string(),
                feedphases,
            };

            let start_time = Instant::now();
            let result_tuple = food_injection.inject_food(&mut channels.feed, &feed_profile);
            let end_time = Instant::now();

            let execution_duration = end_time - start_time;

            println!(
                "execution_duration (milliseconds)= {}",
                execution_duration.as_millis()
            );

            let (interrupted, result) = result_tuple;
            assert!(!interrupted);
            assert!(result.is_ok());

            assert_ge!(execution_duration.as_millis(), 22500);
            assert_le!(execution_duration.as_millis(), 29700);
        });
    });
}

#[test]
// this test case executes the food injection with interruption from the signal handler
pub fn test_food_injection_interrupted() {
    let config: ConfigData =
        read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let mut food_injection = FoodInjection::new(&config.feed);

    let mut channels = Channels::new_for_test();

    scope(|scope| {
        // this scope is providing the test environment
        scope.spawn(move || {
            let mut counter_switch_on: i32 = 0;
            let mut counter_switch_off: i32 = 0;
            let mut skimmer_actuation_state = ActuatorState::On;
            let mut main_pump1_actuation_state = ActuatorState::On;
            let mut main_pump2_actuation_state = ActuatorState::On;
            let mut aux_pump1_actuation_state = ActuatorState::On;
            let mut aux_pump2_actuation_state = ActuatorState::On;
            let mut feeder_actuation_state = ActuatorState::Off;

            for i in 0..5 {
                println!("test_food_injection: loop #{}", i);
                let command = test_food_injection_receive_actuation_command_send_confirmation(
                    &mut channels.relay_manager.tx_relay_manager_to_feed,
                    &mut channels.relay_manager.rx_relay_manager_from_feed,
                    &mut counter_switch_on,
                    &mut counter_switch_off,
                );
                update_actuator_states(
                    command,
                    &mut skimmer_actuation_state,
                    &mut main_pump1_actuation_state,
                    &mut main_pump2_actuation_state,
                    &mut aux_pump1_actuation_state,
                    &mut aux_pump2_actuation_state,
                    &mut feeder_actuation_state,
                );
            }
            println!("test_food_injection_interrupted: sending Quit signal");
            _ = channels
                .signal_handler
                .tx_signal_handler_to_feed
                .send(InternalCommand::Quit);
            for i in 0..1 {
                println!("test_food_injection: loop #{}", i);
                let command = test_food_injection_receive_actuation_command_send_confirmation(
                    &mut channels.relay_manager.tx_relay_manager_to_feed,
                    &mut channels.relay_manager.rx_relay_manager_from_feed,
                    &mut counter_switch_on,
                    &mut counter_switch_off,
                );
                update_actuator_states(
                    command,
                    &mut skimmer_actuation_state,
                    &mut main_pump1_actuation_state,
                    &mut main_pump2_actuation_state,
                    &mut aux_pump1_actuation_state,
                    &mut aux_pump2_actuation_state,
                    &mut feeder_actuation_state,
                );
            }
            println!(
                "counter_switch_on={} counter_switch_off={}",
                counter_switch_on, counter_switch_off
            );
            assert_eq!(counter_switch_on, 5);
            assert_eq!(counter_switch_off, 1);
            assert_eq!(skimmer_actuation_state, ActuatorState::On);
            assert_eq!(main_pump1_actuation_state, ActuatorState::On);
            assert_eq!(main_pump2_actuation_state, ActuatorState::On);
            assert_eq!(aux_pump1_actuation_state, ActuatorState::On);
            assert_eq!(aux_pump2_actuation_state, ActuatorState::On);
            assert_eq!(feeder_actuation_state, ActuatorState::Off);
        });

        // this scope is executing the test object
        scope.spawn(move || {
            let feedphase = FeedPhase::new(
                200, true, true, true, true, true, 200, false, false, false, false, false,
            );
            // Each phase will trigger switching on 6 devices and switching off 6 devices
            let feedphases = vec![feedphase.clone(); 10];

            // After all phases are executed, the feeder will be switched off
            // and 5 devices will be switched on

            let feed_profile = FeedProfile {
                profile_id: 1,
                profile_name: "Test1".to_string(),
                feedphases,
            };

            let start_time = Instant::now();
            let result_tuple = food_injection.inject_food(&mut channels.feed, &feed_profile);
            let end_time = Instant::now();

            let execution_duration = end_time - start_time;
            let (interrupted, result) = result_tuple;

            println!(
                "execution_duration (milliseconds)= {}",
                execution_duration.as_millis()
            );
            if let Err(error_vector) = result.clone() {
                println!("result contains error(s):",);
                for e in error_vector {
                    println!("{:?}", e);
                }
            }

            assert!(interrupted);
            assert!(result.is_ok());
            assert_ge!(execution_duration.as_millis(), 850);
            assert_le!(execution_duration.as_millis(), 1085);
        });
    });
}
