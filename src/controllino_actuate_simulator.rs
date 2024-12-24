/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::warn;
use serialport::SerialPort;
use spin_sleep::SpinSleeper;
use std::{sync::mpsc::Sender, time::Duration};

use crate::channel_content::InternalCommand;
use crate::controllino::RelayActuationTrait;
use crate::controllino::{ControllinoConfig, ControllinoError};
use crate::device_relay_logic::get_relay_command;

const LOG_TARGET_CONTROLLINO_ACTUATE_SIMULATOR: &str = "controllino_actuate_simulator";

/// Contains trait implementation for system testing purposes.
pub struct ControllinoActuateSimulator;

impl RelayActuationTrait for ControllinoActuateSimulator {
    /// Triggers actuation on simulator via TCP communication
    /// This function is used for development purposes only.
    /// It translates the commands SwitchOn and SwitchOff to SetRelay and UnsetRelay
    /// considering relay id and relay logic.
    /// # Arguments
    /// * `tx_to_tcp` - Reference to Sender part of communication channel with TcpCommunication
    /// * `internal_command` - enum value used for identifying content which shall be sent
    fn actuate(
        &mut self,
        internal_command: InternalCommand,
        tx_to_tcp_opt: Option<&Sender<InternalCommand>>,
        _serial_port_opt: &mut Option<Box<dyn SerialPort>>,
        _spin_sleeper_opt: &Option<SpinSleeper>,
        _controllino_processing_duration_opt: &Option<Duration>,
        config: &ControllinoConfig,
    ) -> Result<(), ControllinoError> {
        let tx_to_tcp = tx_to_tcp_opt.unwrap();
        match internal_command {
            InternalCommand::SwitchOn(ref c) | InternalCommand::SwitchOff(ref c) => {
                let relay_command = match get_relay_command(&internal_command, c.clone(), config) {
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
                    internal_command.to_string()
                );
                Err(ControllinoError::IrrelevantCommand(internal_command))
            }
        }
    }
}
