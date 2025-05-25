/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::warn;
use std::sync::mpsc::Sender;

use crate::relays::actuate_controllino::{get_relay_command, ActuateControllinoConfig};
use crate::relays::relay_manager::{RelayActuationTrait, RelayError};
use crate::utilities::channel_content::InternalCommand;

const LOG_TARGET_CONTROLLINO_ACTUATE_SIMULATOR: &str = "controllino_actuate_simulator";

/// Contains trait implementation for system testing purposes.
pub struct ActuateSimulator {
    config: ActuateControllinoConfig,
    tx_to_tcp_opt: Option<Sender<InternalCommand>>,
}

impl RelayActuationTrait for ActuateSimulator {
    /// Triggers simulated relay actuation via TCP communication for development and testing.
    ///
    /// This function translates `SwitchOn` and `SwitchOff` commands for specific aquarium devices
    /// into their corresponding `SetRelay` or `UnsetRelay` commands (considering the configured
    /// relay logic). These translated commands are then sent over a TCP communication channel
    /// to a listener (typically a simulator or a test harness).
    ///
    /// This implementation is intended **solely for development and system testing**
    /// and does not interact with physical hardware directly.
    ///
    /// # Arguments
    /// * `internal_command` - The `InternalCommand` specifying the `AquariumDevice` and the desired state (`SwitchOn` or `SwitchOff`).
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(())` if the translated command was successfully sent via the TCP channel.
    /// - `Err(RelayError::IrrelevantCommand)` if the `internal_command` is not `SwitchOn` or `SwitchOff` (e.g., `Pulse`, `RequestSignal`).
    ///
    /// # Panics
    /// This function will panic if:
    /// - The internal `tx_to_tcp_opt` channel sender is `None` (meaning the TCP communication channel was not set up).
    /// - There's an error translating the `internal_command` into a relay-specific command (e.g., `Pulse` command for a non-pulsable device).
    /// - There's a failure sending the command through the TCP channel.
    fn actuate(&mut self, internal_command: InternalCommand) -> Result<(), RelayError> {
        let tx_to_tcp = self.tx_to_tcp_opt.as_ref().unwrap();
        match internal_command {
            InternalCommand::SwitchOn(ref c) | InternalCommand::SwitchOff(ref c) => {
                let relay_command = match get_relay_command(
                    &internal_command,
                    c.clone(),
                    &self.config,
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        panic!("Controllino: Error in preparation of communication to simulator: {e:?}");
                    }
                };
                match tx_to_tcp.send(relay_command.clone()) {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        panic!(
                            "Controllino: error when sending relay command {} to TCP thread: {e:?}",
                            relay_command
                        );
                    }
                }
            }
            _ => {
                warn!(
                    target: LOG_TARGET_CONTROLLINO_ACTUATE_SIMULATOR,
                    "ignoring irrelevant internal command {}.",
                    internal_command
                );
                Err(RelayError::IrrelevantCommand(internal_command))
            }
        }
    }
}

impl ActuateSimulator {
    /// Creates a new `ActuateSimulator` instance.
    ///
    /// This constructor initializes the simulator with an optional sender channel
    /// for TCP communication and the Controllino-specific configuration.
    /// It's used to set up the mock actuation logic for testing purposes.
    ///
    /// # Arguments
    /// * `tx_to_tcp_opt` - An `Option` containing the sender channel (`Sender<InternalCommand>`)
    ///   used to send simulated relay commands via TCP. If `None`, attempts to `actuate`
    ///   will panic.
    /// * `config` - The `ActuateControllinoConfig` struct, providing device-to-relay mappings
    ///   and other configuration relevant for simulating Controllino behavior.
    ///
    /// # Returns
    /// A new `ActuateSimulator` struct.
    pub fn new(
        tx_to_tcp_opt: Option<Sender<InternalCommand>>,
        config: ActuateControllinoConfig,
    ) -> Self {
        Self {
            tx_to_tcp_opt,
            config,
        }
    }
}
