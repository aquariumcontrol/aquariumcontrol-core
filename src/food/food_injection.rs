/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Implements the physical execution of a feed profile, controlling pumps and the feeder.
//!
//! This module provides the `FoodInjectionTrait` and its concrete implementation,
//! `FoodInjection`. Its core responsibility is to translate a logical `FeedProfile`
//! into a sequence of physical actions by communicating with the `RelayManager`. It
//! manages the state of individual actuators, handles precise timing, and allows for
//! graceful interruption.
//!
//! ## Key Components
//!
//! - **`FoodInjectionTrait`**: An abstraction for the food injection process. This is a
//!   critical design element that decouples the main `Feed` controller from the
//!   concrete implementation, enabling the use of mock injectors for unit testing.
//!
//! - **`FoodInjection` Struct**: The primary implementation of the trait. It maintains the
//!   current known state of all relevant actuators (pumps, skimmer, feeder) to avoid
//!   sending redundant commands to the `RelayManager`.
//!
//! - **`inject_food()` Method**: The main entry point. It iterates through each `FeedPhase`
//!   of a given `FeedProfile`, orchestrating the complex sequence of pausing pumps,
//!   running the feeder, and waiting for specified durations.
//!
//! - **`switch_skimmer_pumps_feeder()` Method**: A private helper that compares the
//!   current actuator states with the target states for a given phase. If a state
//!   mismatch is found, it sends the appropriate `SwitchOn`/`SwitchOff` command and
//!   waits for confirmation.
//!
//! ## Design and Architecture
//!
//! The module is designed for robustness and safe operation in a concurrent environment.
//!
//! - **Stateful Execution**: By tracking the state of each device (`skimmer_state`,
//!   `main_pump1_state`, etc.), the `FoodInjection` struct ensures that it only sends
//!   commands when a state change is actually required.
//!
//! - **Interruptible Loops**: The waiting periods within `inject_food` are not simple
//!   `sleep` calls. They are implemented as tight loops that frequently check for a
//!   `Quit` command from the `SignalHandler`. This ensures that the entire feeding
//!   process can be aborted gracefully and quickly at any point.
//!
//! - **Guaranteed Cleanup**: Regardless of whether the execution of the feed profile completes, is
//!   interrupted, or encounters an error, a final cleanup step is always executed.
//!   This step ensures that all pumps are returned to their active state and the
//!   feeder is turned off, preventing the system from being left in a hazardous
//!   or undesirable state.
//!
//! - **Comprehensive Error Collection**: Instead of failing on the first error, the
//!   `inject_food` method collects all errors encountered during the process into a
//!   `Vec<FoodInjectionError>`. This provides a complete picture of all failures
//!   that occurred during a single injection attempt, which is invaluable for
//!   diagnostics.

use std::time::{Duration, Instant};

use crate::food::feed_channels::FeedChannels;
use crate::food::food_injection_error::FoodInjectionError;
use crate::food::{feed_config::FeedConfig, feed_profile::FeedProfile};
use crate::utilities::channel_content::{ActuatorState, AquariumDevice, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

/// Trait for the execution of the feed profile.
/// This trait allows running the main control with a mock implementation for testing.
pub trait FoodInjectionTrait {
    /// Actuates the feeder according to the specified feed profile to inject food.
    ///
    /// This trait method defines the interface for physically dispensing food based
    /// on a detailed feed profile. Implementations will control relevant pumps and
    /// the feeder motor through communication with a hardware manager (e.g., relay manager),
    /// while also monitoring for external shutdown commands.
    ///
    /// # Arguments
    /// * `feed_channels` - A mutable reference to the struct containing the channels.
    /// * `feed_profile` - A reference to the struct holding the description of the feed profile.
    ///
    /// # Returns
    /// A tuple `(bool, Result<(), Vec<FoodInjectionError>>)` where:
    /// - The first element (`bool`) is `true` if a `Quit` command was received from the
    ///   signal handler, indicating an early termination request. Otherwise, it is `false`.
    /// - The second element is a `Result`. It is `Ok(())` if the entire sequence is
    ///   completed without any errors.
    ///
    /// # Errors
    /// The `Result` part of the return tuple will be `Err(Vec<FoodInjectionError>)` if one
    /// or more errors occurred during the process. The vector will contain all errors
    /// encountered, which can include:
    /// - Communication failures with the relay manager (`RelayManagerSend`, `RelayManagerReceive`).
    /// - An attempt to set a device to an undefined state (e.g., `UndefinedTargetStateSkimmer`).
    ///
    #[allow(async_fn_in_trait)]
    /// [impl->swreq~food_execute_schedule~1]
    async fn inject_food(
        &mut self,
        feed_channels: &mut FeedChannels,
        feed_profile: &FeedProfile,
    ) -> (bool, Result<(), Vec<FoodInjectionError>>);
}

/// Struct collects the target actuator states for switch_skimmer_pumps_feeder
pub struct FoodInjectionTargetActuatorStates {
    /// target state for protein skimmer
    pub target_skimmer_state: ActuatorState,

    /// target state for main pump 1
    pub target_main_pump1_state: ActuatorState,

    /// target state for main pump 2
    pub target_main_pump2_state: ActuatorState,

    /// target state for aux. pump 1
    pub target_aux_pump1_state: ActuatorState,

    /// target state for aux. pump 2
    pub target_aux_pump2_state: ActuatorState,

    /// target state for feeder
    pub target_feeder_state: ActuatorState,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Struct implements the FoodInjectionTrait for executing the feed profile.
/// It also contains state attributes for the actuators.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     food_injection[Food injection] --> relay_manager[Relay Manager]
///     relay_manager --> food_injection
///     signal_handler[Signal handler] --> food_injection
/// ```
pub struct FoodInjection {
    skimmer_state: ActuatorState,
    main_pump1_state: ActuatorState,
    main_pump2_state: ActuatorState,
    aux_pump1_state: ActuatorState,
    aux_pump2_state: ActuatorState,
    feeder_state: ActuatorState,
    sleep_duration_device_switch: Duration,
}

impl FoodInjection {
    /// Creates a new `FoodInjection` instance.
    ///
    /// This constructor initializes the food injection control module. It sets
    /// the initial state of all associated actuators (skimmer, pumps, feeder)
    /// to `Undefined` and configures internal timing mechanisms, such as the
    /// delay between device switches, based on the provided `FeedConfig`.
    ///
    /// # Arguments
    /// * `config` - A reference to the `FeedConfig` struct, which contains
    ///   parameters like `device_switch_delay_millis`.
    ///
    /// # Returns
    /// A new `FoodInjection` struct, ready to execute food dispensing operations.
    pub fn new(config: &FeedConfig) -> FoodInjection {
        FoodInjection {
            skimmer_state: ActuatorState::Undefined,
            main_pump1_state: ActuatorState::Undefined,
            main_pump2_state: ActuatorState::Undefined,
            aux_pump1_state: ActuatorState::Undefined,
            aux_pump2_state: ActuatorState::Undefined,
            feeder_state: ActuatorState::Undefined,
            sleep_duration_device_switch: Duration::from_millis(
                config.device_switch_delay_millis as u64,
            ),
        }
    }

    /// Commands the specified aquarium actuators (skimmer, main pumps, auxiliary pumps, feeder)
    /// to switch to their target states.
    ///
    /// This private helper function iterates through a set of target states for various devices.
    /// For each device whose current state differs from its target state, it sends an
    /// appropriate `SwitchOn` or `SwitchOff` command to the relay manager
    /// and waits for an acknowledgment. It includes a small delay between each switch command.
    ///
    /// # Arguments
    /// * `target_actuator_states` - A `FoodInjectionTargetActuatorStates` struct containing the
    ///   desired `ActuatorState` for each relevant device.
    /// * `feed_channels` - A mutable reference to the struct containing the channels.
    ///
    /// # Returns
    /// An `Ok(())` if all devices were switched successfully.
    ///
    /// # Errors
    /// Returns an `Err(Vec<FoodInjectionError>)` containing a list of all errors that
    /// occurred during the switching process. This function attempts to switch all devices
    /// even if some fail. Possible errors include:
    /// - `RelayManagerSend`: Failure to send a command to the relay manager's channel.
    /// - `RelayManagerReceive`: Failure to receive an acknowledgment from the relay manager.
    /// - `UndefinedTargetState...`: An attempt was made to set a device to a state other
    ///   than `On` or `Off`.
    pub(crate) async fn switch_skimmer_pumps_feeder(
        &mut self,
        target_actuator_states: FoodInjectionTargetActuatorStates,
        feed_channels: &mut FeedChannels,
    ) -> Result<(), Vec<FoodInjectionError>> {
        // Initialize a vector to collect any errors that occur.
        let mut errors: Vec<FoodInjectionError> = Vec::new();

        // --- Skimmer ---
        if self.skimmer_state != target_actuator_states.target_skimmer_state {
            tokio::time::sleep(self.sleep_duration_device_switch).await;
            let device = AquariumDevice::Skimmer;

            match target_actuator_states.target_skimmer_state {
                ActuatorState::On | ActuatorState::Off => {
                    // Command is defined
                    let command =
                        if target_actuator_states.target_skimmer_state == ActuatorState::On {
                            InternalCommand::SwitchOn(device.clone())
                        } else {
                            InternalCommand::SwitchOff(device.clone())
                        };
                    if let Err(e) = feed_channels.send_to_relay_manager(command) {
                        #[cfg(test)]
                        println!(
                            "Encountered error when sending command {:?} for {} to relay manager: {e:?}",
                            target_actuator_states.target_skimmer_state,
                            AquariumDevice::Skimmer
                        );
                        errors.push(FoodInjectionError::RelayManagerSend {
                            location: module_path!().to_string(),
                            device: device.clone(),
                            source: e,
                        });
                    } else if let Err(e) = feed_channels.receive_from_relay_manager().await {
                        println!(
                            "Encountered error when receiving command to relay manager: {e:?}"
                        );
                        errors.push(FoodInjectionError::RelayManagerReceive {
                            location: module_path!().to_string(),
                            device: device.clone(),
                            source: e,
                        });
                    } else {
                        // Only update state on success
                        self.skimmer_state = target_actuator_states.target_skimmer_state;
                    }
                }
                // Handle undefined state by pushing a specific error.
                _ => errors.push(FoodInjectionError::UndefinedTargetState(
                    module_path!().to_string(),
                    AquariumDevice::Skimmer,
                )),
            }
        }

        // --- Main pump 1 ---
        if self.main_pump1_state != target_actuator_states.target_main_pump1_state {
            tokio::time::sleep(self.sleep_duration_device_switch).await;
            let device = AquariumDevice::MainPump1;

            match target_actuator_states.target_main_pump1_state {
                ActuatorState::On | ActuatorState::Off => {
                    // Command is defined
                    let command =
                        if target_actuator_states.target_main_pump1_state == ActuatorState::On {
                            InternalCommand::SwitchOn(device.clone())
                        } else {
                            InternalCommand::SwitchOff(device.clone())
                        };
                    if let Err(e) = feed_channels.send_to_relay_manager(command) {
                        #[cfg(test)]
                        println!(
                            "Encountered error when sending command {:?} for {} to relay manager: {e:?}",
                            target_actuator_states.target_main_pump1_state,
                            AquariumDevice::MainPump1
                        );
                        errors.push(FoodInjectionError::RelayManagerSend {
                            location: module_path!().to_string(),
                            device: device.clone(),
                            source: e,
                        });
                    } else if let Err(e) = feed_channels.receive_from_relay_manager().await {
                        errors.push(FoodInjectionError::RelayManagerReceive {
                            location: module_path!().to_string(),
                            device: device.clone(),
                            source: e,
                        });
                    } else {
                        // Only update state on success
                        self.main_pump1_state = target_actuator_states.target_main_pump1_state;
                    }
                }
                // Handle undefined state by pushing a specific error.
                _ => errors.push(FoodInjectionError::UndefinedTargetState(
                    module_path!().to_string(),
                    AquariumDevice::MainPump1,
                )),
            }
        }

        // --- Main pump 2 ---
        if self.main_pump2_state != target_actuator_states.target_main_pump2_state {
            tokio::time::sleep(self.sleep_duration_device_switch).await;
            let device = AquariumDevice::MainPump2;

            match target_actuator_states.target_main_pump2_state {
                ActuatorState::On | ActuatorState::Off => {
                    // Command is defined
                    let command =
                        if target_actuator_states.target_main_pump2_state == ActuatorState::On {
                            InternalCommand::SwitchOn(device.clone())
                        } else {
                            InternalCommand::SwitchOff(device.clone())
                        };
                    if let Err(e) = feed_channels.send_to_relay_manager(command) {
                        #[cfg(test)]
                        println!(
                            "Encountered error when sending command {:?} for {} to relay manager: {e:?}",
                            target_actuator_states.target_main_pump2_state,
                            AquariumDevice::MainPump2
                        );
                        errors.push(FoodInjectionError::RelayManagerSend {
                            location: module_path!().to_string(),
                            device: device.clone(),
                            source: e,
                        });
                    } else if let Err(e) = feed_channels.receive_from_relay_manager().await {
                        errors.push(FoodInjectionError::RelayManagerReceive {
                            location: module_path!().to_string(),
                            device: device.clone(),
                            source: e,
                        });
                    } else {
                        // Only update state on success
                        self.main_pump2_state = target_actuator_states.target_main_pump2_state;
                    }
                }
                // Handle undefined state by pushing a specific error.
                _ => errors.push(FoodInjectionError::UndefinedTargetState(
                    module_path!().to_string(),
                    AquariumDevice::MainPump2,
                )),
            }
        }

        // --- Aux. pump 1 ---
        if self.aux_pump1_state != target_actuator_states.target_aux_pump1_state {
            tokio::time::sleep(self.sleep_duration_device_switch).await;
            let device = AquariumDevice::AuxPump1;

            match target_actuator_states.target_aux_pump1_state {
                ActuatorState::On | ActuatorState::Off => {
                    // Command is defined
                    let command =
                        if target_actuator_states.target_aux_pump1_state == ActuatorState::On {
                            InternalCommand::SwitchOn(device.clone())
                        } else {
                            InternalCommand::SwitchOff(device.clone())
                        };
                    if let Err(e) = feed_channels.send_to_relay_manager(command) {
                        #[cfg(test)]
                        println!(
                            "Encountered error when sending command {:?} for {} to relay manager: {e:?}",
                            target_actuator_states.target_aux_pump1_state,
                            AquariumDevice::AuxPump1
                        );
                        errors.push(FoodInjectionError::RelayManagerSend {
                            location: module_path!().to_string(),
                            device: device.clone(),
                            source: e,
                        });
                    } else if let Err(e) = feed_channels.receive_from_relay_manager().await {
                        errors.push(FoodInjectionError::RelayManagerReceive {
                            location: module_path!().to_string(),
                            device: device.clone(),
                            source: e,
                        });
                    } else {
                        // Only update state on success
                        self.aux_pump1_state = target_actuator_states.target_aux_pump1_state;
                    }
                }
                // Handle undefined state by pushing a specific error.
                _ => errors.push(FoodInjectionError::UndefinedTargetState(
                    module_path!().to_string(),
                    AquariumDevice::AuxPump1,
                )),
            }
        }

        // --- Aux. pump 2 ---
        if self.aux_pump2_state != target_actuator_states.target_aux_pump2_state {
            tokio::time::sleep(self.sleep_duration_device_switch).await;
            let device = AquariumDevice::AuxPump2;

            match target_actuator_states.target_aux_pump2_state {
                ActuatorState::On | ActuatorState::Off => {
                    // Command is defined
                    let command =
                        if target_actuator_states.target_aux_pump2_state == ActuatorState::On {
                            InternalCommand::SwitchOn(device.clone())
                        } else {
                            InternalCommand::SwitchOff(device.clone())
                        };
                    if let Err(e) = feed_channels.send_to_relay_manager(command) {
                        #[cfg(test)]
                        println!(
                            "Encountered error when sending command {:?} for {} to relay manager: {e:?}",
                            target_actuator_states.target_aux_pump2_state,
                            AquariumDevice::AuxPump2
                        );
                        errors.push(FoodInjectionError::RelayManagerSend {
                            location: module_path!().to_string(),
                            device: device.clone(),
                            source: e,
                        });
                    } else if let Err(e) = feed_channels.receive_from_relay_manager().await {
                        errors.push(FoodInjectionError::RelayManagerReceive {
                            location: module_path!().to_string(),
                            device: device.clone(),
                            source: e,
                        });
                    } else {
                        // Only update state on success
                        self.aux_pump2_state = target_actuator_states.target_aux_pump2_state;
                    }
                }
                // Handle undefined state by pushing a specific error.
                _ => errors.push(FoodInjectionError::UndefinedTargetState(
                    module_path!().to_string(),
                    AquariumDevice::AuxPump2,
                )),
            }
        }

        // --- Feeder ---
        if self.feeder_state != target_actuator_states.target_feeder_state {
            tokio::time::sleep(self.sleep_duration_device_switch).await;
            let device = AquariumDevice::Feeder;

            match target_actuator_states.target_feeder_state {
                ActuatorState::On | ActuatorState::Off => {
                    // Command is defined
                    let command = if target_actuator_states.target_feeder_state == ActuatorState::On
                    {
                        InternalCommand::SwitchOn(device.clone())
                    } else {
                        InternalCommand::SwitchOff(device.clone())
                    };
                    if let Err(e) = feed_channels.send_to_relay_manager(command) {
                        #[cfg(test)]
                        println!(
                            "Encountered error when sending command {:?} for {} to relay manager: {e:?}",
                            target_actuator_states.target_feeder_state,
                            AquariumDevice::Feeder
                        );
                        errors.push(FoodInjectionError::RelayManagerSend {
                            location: module_path!().to_string(),
                            device: device.clone(),
                            source: e,
                        });
                    } else if let Err(e) = feed_channels.receive_from_relay_manager().await {
                        errors.push(FoodInjectionError::RelayManagerReceive {
                            location: module_path!().to_string(),
                            device: device.clone(),
                            source: e,
                        });
                    } else {
                        // Only update state on success
                        self.feeder_state = target_actuator_states.target_feeder_state;
                    }
                }
                // Handle undefined state by pushing a specific error.
                _ => errors.push(FoodInjectionError::UndefinedTargetState(
                    module_path!().to_string(),
                    AquariumDevice::Feeder,
                )),
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

impl FoodInjectionTrait for FoodInjection {
    /// Actuates the feeder according to the specified feed profile to inject food.
    ///
    /// This implementation executes a sequence of feed phases defined in the `feed_profile`.
    /// For each phase, it sets the state of various pumps and the feeder, waits for a
    /// specified duration, and continuously checks for a `Quit` command from the signal
    /// handler to allow for graceful interruption.
    ///
    /// After the sequence is complete or aborted, it performs a cleanup step to ensure all
    /// pumps are returned to an active state and the feeder is off.
    ///
    /// # Arguments
    /// * `feed_channels` - A mutable reference to the struct containing the channels.
    /// * `feed_profile` - A reference to the struct holding the description of the feed profile.
    ///
    /// # Returns
    /// A tuple `(bool, Result<(), Vec<FoodInjectionError>>)` where:
    /// - The first element (`bool`) is `true` if a `Quit` command was received from the
    ///   signal handler, indicating an early termination request. Otherwise, it is `false`.
    /// - The second element is a `Result`. It is `Ok(())` if the entire sequence is
    ///   completed without any errors.
    ///
    /// # Errors
    /// The `Result` part of the return tuple will be `Err(Vec<FoodInjectionError>)` if one
    /// or more errors occurred during the process. The function aborts the feed sequence
    /// on the first error but still performs the final cleanup step. The vector will
    /// contain all errors encountered during both the main sequence and the cleanup.
    async fn inject_food(
        &mut self,
        feed_channels: &mut FeedChannels,
        feed_profile: &FeedProfile,
    ) -> (bool, Result<(), Vec<FoodInjectionError>>) {
        let sleep_interval_millis: i16 = 1;
        let sleep_duration = Duration::from_millis(sleep_interval_millis as u64);
        let mut target_skimmer_state: ActuatorState;
        let mut target_main_pump1_state: ActuatorState;
        let mut target_main_pump2_state: ActuatorState;
        let mut target_aux_pump1_state: ActuatorState;
        let mut target_aux_pump2_state: ActuatorState;
        let mut quit_command_received: bool = false;
        let mut collected_errors_cumulated: Vec<FoodInjectionError> = vec![];

        for feedphase in &feed_profile.feedphases {
            // check if there is a quit request
            (quit_command_received, _, _) =
                self.process_external_request(&mut feed_channels.rx_feed_from_signal_handler, None);
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
                let target_actuator_states = FoodInjectionTargetActuatorStates {
                    target_skimmer_state,
                    target_main_pump1_state,
                    target_main_pump2_state,
                    target_aux_pump1_state,
                    target_aux_pump2_state,
                    target_feeder_state: ActuatorState::Off,
                };
                if let Err(collected_errors) = self
                    .switch_skimmer_pumps_feeder(target_actuator_states, feed_channels)
                    .await
                {
                    collected_errors_cumulated.extend(collected_errors);
                    break; // if any error occurs, abort the feed sequence
                }
            }

            // wait for a determined period and check quit request in between
            let start_of_pause = Instant::now();
            let pause_duration = Duration::from_millis(feedphase.pause_duration as u64);
            while Instant::now().duration_since(start_of_pause) < pause_duration {
                (quit_command_received, _, _) = self
                    .process_external_request(&mut feed_channels.rx_feed_from_signal_handler, None);
                if quit_command_received {
                    #[cfg(test)]
                    println!("Received quit command during pause phase of food injection");
                    break; // exit inner loop
                }
                tokio::time::sleep(sleep_duration).await;
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
                let target_actuator_states = FoodInjectionTargetActuatorStates {
                    target_skimmer_state,
                    target_main_pump1_state,
                    target_main_pump2_state,
                    target_aux_pump1_state,
                    target_aux_pump2_state,
                    target_feeder_state: ActuatorState::On,
                };
                if let Err(collected_errors) = self
                    .switch_skimmer_pumps_feeder(target_actuator_states, feed_channels)
                    .await
                {
                    collected_errors_cumulated.extend(collected_errors);
                    break; // if any error occurs, abort the feed sequence
                }
            }

            // wait for a determined period and check quit request in between
            let start_of_pause = Instant::now();
            let feed_duration = Duration::from_millis(feedphase.feed_duration as u64);
            while Instant::now().duration_since(start_of_pause) < feed_duration {
                (quit_command_received, _, _) = self
                    .process_external_request(&mut feed_channels.rx_feed_from_signal_handler, None);
                if quit_command_received {
                    #[cfg(test)]
                    println!("Received quit command during feed phase of food injection");
                    break; // exit inner loop
                }
                tokio::time::sleep(sleep_duration).await;
            }
            if quit_command_received {
                break; // exit outer loop
            }
        }

        // The feed sequence has finished or has been aborted.
        // Switch on all pumps. Switch off feeder.
        if let Err(collected_errors) = self
            .switch_skimmer_pumps_feeder(
                FoodInjectionTargetActuatorStates {
                    target_skimmer_state: ActuatorState::On,
                    target_main_pump1_state: ActuatorState::On,
                    target_main_pump2_state: ActuatorState::On,
                    target_aux_pump1_state: ActuatorState::On,
                    target_aux_pump2_state: ActuatorState::On,
                    target_feeder_state: ActuatorState::Off,
                },
                feed_channels,
            )
            .await
        {
            collected_errors_cumulated.extend(collected_errors);
        }

        if collected_errors_cumulated.is_empty() {
            (quit_command_received, Ok(()))
        } else {
            (quit_command_received, Err(collected_errors_cumulated))
        }
    }
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub mod tests { // unit tests shall not appear in code coverage measurement
    use super::*;
    use crate::food::feed_config::FeedConfig;

    fn test_feed_config() -> FeedConfig {
        FeedConfig {
            active: true,
            execute: true,
            schedule_check_interval: 1,
            device_switch_delay_millis: 1,
            strategy_multiple_schedule_entries_in_past: 0,
        }
    }

    #[tokio::test]
    async fn test_switch_skimmer_pumps_feeder_undefined_states() {
        let config = test_feed_config();
        let mut injection = FoodInjection::new(&config);
        injection.skimmer_state = ActuatorState::Off;
        injection.main_pump1_state = ActuatorState::Off;
        injection.main_pump2_state = ActuatorState::Off;
        injection.aux_pump1_state = ActuatorState::Off;
        injection.aux_pump2_state = ActuatorState::Off;
        injection.feeder_state = ActuatorState::Off;
        let mut channels = crate::launch::channels::Channels::new_for_test();

        let target_states = FoodInjectionTargetActuatorStates {
            target_skimmer_state: ActuatorState::Undefined,
            target_main_pump1_state: ActuatorState::Undefined,
            target_main_pump2_state: ActuatorState::Undefined,
            target_aux_pump1_state: ActuatorState::Undefined,
            target_aux_pump2_state: ActuatorState::Undefined,
            target_feeder_state: ActuatorState::Undefined,
        };

        let result = injection
            .switch_skimmer_pumps_feeder(target_states, &mut channels.feed)
            .await;

        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 6);
        for err in errors {
            assert!(matches!(err, FoodInjectionError::UndefinedTargetState(_, _)));
        }
    }

    #[tokio::test]
    async fn test_switch_skimmer_pumps_feeder_send_error() {
        let config = test_feed_config();
        let mut injection = FoodInjection::new(&config);
        let mut channels = crate::launch::channels::Channels::new_for_test();

        // Drop receiver so sending fails
        drop(channels.relay_manager.rx_relay_manager_from_feed);

        let target_states = FoodInjectionTargetActuatorStates {
            target_skimmer_state: ActuatorState::On,
            target_main_pump1_state: ActuatorState::Off,
            target_main_pump2_state: ActuatorState::Off,
            target_aux_pump1_state: ActuatorState::Off,
            target_aux_pump2_state: ActuatorState::Off,
            target_feeder_state: ActuatorState::Off,
        };

        let result = injection
            .switch_skimmer_pumps_feeder(target_states, &mut channels.feed)
            .await;

        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(matches!(errors[0], FoodInjectionError::RelayManagerSend { .. }));
    }

    #[tokio::test]
    async fn test_inject_food_quit_interruption() {
        let config = test_feed_config();
        let mut injection = FoodInjection::new(&config);
        let mut channels = crate::launch::channels::Channels::new_for_test();

        let mut rx_relay = channels.relay_manager.rx_relay_manager_from_feed;
        let mut tx_relay = channels.relay_manager.tx_relay_manager_to_feed;
        tokio::spawn(async move {
            while rx_relay.recv().await.is_ok() {
                let _ = tx_relay.send(true);
            }
        });

        // Send Quit signal into rx_feed_from_signal_handler
        channels
            .signal_handler
            .tx_signal_handler_to_feed
            .send(crate::utilities::channel_content::InternalCommand::Quit)
            .unwrap();

        let profile = FeedProfile {
            profile_id: 1,
            profile_name: "test".to_string(),
            feedphases: vec![crate::food::feed_profile::FeedPhase {
                pause_duration: 10,
                pause_skimmer: false,
                pause_main_pump_1: false,
                pause_main_pump_2: false,
                pause_aux_pump_1: false,
                pause_aux_pump_2: false,
                feed_duration: 10,
                feed_skimmer: false,
                feed_main_pump_1: false,
                feed_main_pump_2: false,
                feed_aux_pump_1: false,
                feed_aux_pump_2: false,
            }],
        };

        let (quit, _res) = injection.inject_food(&mut channels.feed, &profile).await;
        assert!(quit);
    }
}
