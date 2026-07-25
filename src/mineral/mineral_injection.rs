/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Implements the physical actuation of a single mineral dosing pump.
//!
//! This module provides the `MineralInjectionTrait` and its concrete implementation,
//! `MineralInjection`. Its sole responsibility is to translate a command to dose a
//! specific mineral into the physical actions of switching a peristaltic pump on and
//! off for a precise duration. It acts as the final link between the logical `Balling`
//! controller and the `RelayManager` that controls the hardware.
//!
//! ## Key Components
//!
//! - **`MineralInjectionTrait`**: An abstraction for the mineral injection process.
//!   This is a critical design element that decouples the `Balling` controller from the
//!   concrete implementation, enabling the use of mock injectors for unit testing.
//!
//! - **`MineralInjection` Struct**: The primary, stateless implementation of the trait.
//!
//! - **`inject_mineral()` Method**: The main entry point. It orchestrates the sequence of:
//!   1. Sending a `SwitchOn` command to the `RelayManager`.
//!   2. Entering a timed loop that keeps the pump running.
//!   3. Continuously checking for a `Quit` command from the `SignalHandler`.
//!   4. Sending a `SwitchOff` command upon completion or interruption.
//!
//! ## Design and Architecture
//!
//! The module is designed for safe, robust, and interruptible operation.
//!
//! - **Interruptible Operation**: The waiting period is not a simple `sleep` call. It's
//!   an active `loop` using a `SpinSleeper` that frequently checks for external `Quit`
//!   commands. This ensures that a dosing operation can be aborted almost instantly,
//!   which is a crucial safety and responsiveness feature.
//!
//! - **Guaranteed Cleanup**: The `SwitchOff` command is always sent to the `RelayManager`
//!   at the end of the function, regardless of whether the dosing completed normally or
//!   was interrupted. This prevents a pump from being left in the "on" state indefinitely.
//!
//! - **Trait-based Decoupling**: By depending on the `MineralInjectionTrait`, the main
//!   `Balling` module can be tested in isolation without needing real hardware or a
//!   running `RelayManager` thread. The tests in this module use a mock version of `RelayManager`.

use crate::mineral::balling_channels::BallingChannels;
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use log::error;
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
    #[allow(async_fn_in_trait)]
    async fn inject_mineral(
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

    /// Sends a command to the relay manager and handles the response.
    /// Logs an error if either the send or receive operation fails.
    ///
    /// # Arguments
    /// * `balling_channels` - A mutable reference to the `BallingChannels` communication interface.
    /// * `action_description` - A parameter representing `action_description`.
    async fn send_and_confirm(
        &self,
        balling_channels: &mut BallingChannels,
        command: InternalCommand,
        action_description: &str,
    ) {
        if let Err(e) = balling_channels.send_to_relay_manager(command) {
            error!(
                target: module_path!(),
                "Channel communication to relay manager when {} failed ({:?})", action_description, e
            );
            return; // Don't wait for a response if the send-operation failed
        }

        if let Err(e) = balling_channels.receive_from_relay_manager().await {
            error!(
                target: module_path!(),
                "Receiving answer from relay manager when {} failed ({:?})", action_description, e
            );
        }
    }
}

impl Default for MineralInjection {
    /// Creates a default instance of this type.
    fn default() -> Self {
        Self::new()
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
    async fn inject_mineral(
        &mut self,
        balling_channels: &mut BallingChannels,
        mineral_pump: AquariumDevice,
        dosing_duration_millis: u32,
    ) -> (bool, u32) {
        let sleep_interval_millis: u32 = 1;
        let sleep_duration = Duration::from_millis(sleep_interval_millis as u64);
        let mut quit_command_received: bool;

        // --- Switch On ---
        self.send_and_confirm(
            balling_channels,
            InternalCommand::SwitchOn(mineral_pump.clone()),
            "switching on dosing pump",
        )
        .await;

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

            tokio::time::sleep(sleep_duration).await;

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

        // --- Switch Off (Guaranteed Cleanup) ---
        self.send_and_confirm(
            balling_channels,
            InternalCommand::SwitchOff(mineral_pump),
            "switching off dosing pump",
        )
        .await;

        (quit_command_received, dosing_counter)
    }
}
