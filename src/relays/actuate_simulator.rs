/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Provides a simulated hardware actuation layer for testing purposes.
//!
//! This module defines the `ActuateSimulator` struct, which is a test-only, "mock"
//! implementation of the `RelayActuationTrait`. Its purpose is to stand in for a real
//! hardware actuator (like `ActuateControllino` or `ActuateGpio`) during system tests,
//! allowing the application (except for the hardware-related parts) to be tested without needing any
//! physical hardware connected.
//!
//! ## Key Components
//!
//! - **`ActuateSimulator`**: The main struct that implements the `RelayActuationTrait`.
//!   Instead of communicating with a serial port or GPIO pins, it sends the final,
//!   low-level relay commands over an `mpsc` channel.
//!
//! - **`actuate()` Method**: The core of the simulation. It performs two key steps:
//!   1.  It uses the exact same `get_relay_command` logic from the real `actuate_controllino`
//!       module to translate a high-level `InternalCommand` (e.g., `SwitchOn(Heater)`)
//!       into a low-level one (e.g., `SetRelay(6)`).
//!   2.  It sends this low-level command over a channel, where a test harness or a
//!       TCP listener can receive it to verify that the correct command was generated.
//!
//! ## Design and Architecture
//!
//! This module is a cornerstone of the application's testability.
//!
//! - **High-Fidelity Mocking**: By reusing the `get_relay_command` function, this simulator
//!   ensures that the commands it generates are identical to what the real Controllino
//!   hardware would receive. This provides a high degree of confidence that the system's
//!   logic is correct.
//!
//! - **Enabling Hardware-Free Testing**: It allows for comprehensive integration and system
//!   tests to run in any environment (e.g., a developer's laptop, a CI/CD pipeline)
//!   without needing access to the specialized Controllino or Raspberry Pi hardware.
//!
//! - **Stateless Operation**: The simulator does not manage a persistent connection, so
//!   methods like `heartbeat()` and `flush_buffer()` are implemented as no-ops, as
//!   they are not relevant in this context.

use log::{error, warn};

use crate::relays::actuate_controllino::get_relay_command;
use crate::relays::actuate_controllino_config::ActuateControllinoConfig;
use crate::relays::actuate_simulator_channels::ActuateSimulatorChannels;
use crate::relays::relay_error::RelayError;
use crate::relays::relay_manager::RelayActuationTrait;
use crate::utilities::channel_content::InternalCommand;

/// Contains trait implementation for system testing purposes.
pub struct ActuateSimulator<'a> {
    config: ActuateControllinoConfig,
    actuate_simulator_channels: &'a mut ActuateSimulatorChannels,
}

impl<'a> RelayActuationTrait for ActuateSimulator<'a> {
    /// Simulates relay actuation by sending a command over a channel, typically for testing.
    ///
    /// This function translates a high-level `InternalCommand` (like `SwitchOn` or `SwitchOff`)
    /// into a low-level relay command (e.g., `SetRelay`) using the same logic as the
    /// physical Controllino actuator. The resulting command is then sent over an `mpsc`
    /// channel, presumably to a test harness or a TCP listener that simulates the hardware.
    ///
    /// This implementation is intended **solely for development and system testing**
    /// and does not interact with physical hardware.
    ///
    /// # Arguments
    /// * `internal_command` - The `InternalCommand` specifying the `AquariumDevice` and the
    ///   desired state (`SwitchOn` or `SwitchOff`).
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the translated command was successfully sent
    /// via the channel.
    ///
    /// # Errors
    /// This function will return a `RelayError` if any part of the simulation fails:
    /// - `RelayError::SimulatorNotInitialized`: If the simulator was created without a
    ///   valid sender channel (`tx_to_tcp_opt` was `None`).
    /// - `RelayError::IrrelevantCommand`: If the provided `internal_command` is not one that
    ///   can be translated into a relay action (e.g., `RequestSignal`).
    /// - `RelayError::PulseNotAllowedForDevice`: If a `Pulse` command is attempted for a
    ///   device that doesn't support it (propagated from `get_relay_command`).
    /// - `RelayError::SimulatorSendFailed`: If sending the command over the `mpsc`
    ///   channel fails, which typically means the receiver has been dropped.
    fn actuate(&mut self, internal_command: &InternalCommand) -> Result<(), RelayError> {
        if !matches!(
            internal_command,
            InternalCommand::SwitchOn(_) | InternalCommand::SwitchOff(_) | InternalCommand::Pulse(_, _)
        ) {
            warn!(
                target: module_path!(),
                "ignoring irrelevant internal command {internal_command}."
            );
            return Err(RelayError::IrrelevantCommand(
                module_path!().to_string(),
                internal_command.clone(),
            ));
        }
        
        let tx_actuate_simulator_to_tcp = self
            .actuate_simulator_channels
            .tx_actuate_simulator_to_tcp_opt
            .as_mut()
            .ok_or_else(|| {
                RelayError::SimulatorNotInitialized(module_path!().to_string())
            })?;
        match internal_command {
            InternalCommand::SwitchOn(ref c) | InternalCommand::SwitchOff(ref c) => {
                let relay_command = get_relay_command(
                    internal_command,
                    c.clone(),
                    &self.config,
                )?;
                tx_actuate_simulator_to_tcp.send(relay_command.clone()).map_err(|e| {
                    error!(
                        "Controllino: error when sending relay command {relay_command} to TCP thread: {e:?}"
                    );
                    RelayError::SimulatorSendFailed(module_path!().to_string())
                })
            }
            _ => {
                warn!(
                    target: module_path!(),
                    "ignoring irrelevant internal command {internal_command}."
                );
                Err(RelayError::IrrelevantCommand(
                    module_path!().to_string(),
                    internal_command.clone(),
                ))
            }
        }
    }

    /// Returns the heartbeat interval, which is not applicable for the simulator.
    ///
    /// The simulator does not require a keep-alive signal, so this implementation
    /// always returns `None`.
    ///
    /// # Returns
    /// Always returns `None`.
    fn get_heartbeat_interval_seconds(&self) -> Option<u64> {
        None
    }

    /// Performs a heartbeat action, which is not applicable for the simulator.
    ///
    /// This is a no-op for the simulator implementation as there is no persistent
    /// communication channel that needs to be kept alive.
    ///
    /// # Returns
    /// Always returns `Ok(())`.
    ///
    /// # Errors
    /// This function will never return an error.
    fn heartbeat(&mut self) -> Result<(), RelayError> {
        // do nothing
        Ok(())
    }

    /// Flushes a communication buffer, which is not applicable for the simulator.
    ///
    /// This is a no-op for the simulator implementation as there is no serial
    /// communication buffer to flush.
    ///
    /// # Returns
    /// Always returns `Ok(())`.
    ///
    /// # Errors
    /// This function will never return an error.
    fn flush_buffer(&mut self) -> Result<(), RelayError> {
        // do nothing
        Ok(())
    }
}

impl<'a> ActuateSimulator<'a> {
    /// Creates a new `ActuateSimulator` instance.
    ///
    /// This constructor initializes the simulator with an optional sender channel
    /// for TCP communication and the Controllino-specific configuration.
    /// It's used to set up the mock actuation logic for testing purposes.
    ///
    /// # Arguments
    /// * `actuate_simulator_channels` - A mutable reference to the struct containing the channel.
    /// * `config` - The `ActuateControllinoConfig` struct, providing device-to-relay mappings
    ///   and other configuration relevant when simulating Controllino behavior.
    ///
    /// # Returns
    /// A new `ActuateSimulator` struct.
    pub fn new(
        actuate_simulator_channels: &'a mut ActuateSimulatorChannels,
        config: ActuateControllinoConfig,
    ) -> Self {
        Self {
            actuate_simulator_channels,
            config,
        }
    }
}
