/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::relays::controllino_message::{ControllinoMessage, ControllinoMessageContent};
use crate::relays::relay_error::RelayError;
use crate::relays::relay_manager::RelayActuationTrait;
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
use log::{error, warn};
use serde_derive::Deserialize;
use serialport::{ClearBuffer, SerialPort};
use spin_sleep::SpinSleeper;
use std::collections::HashSet;
use std::time::Duration;
use thiserror::Error;

const CONTROLLINO_MAX_RELAY_ID: u8 = 64;
const CONTROLLINO_MIN_RELAY_ID: u8 = 1;

/// Contains constants for communication with Controllino
#[allow(unused)]
pub mod controllino_constants {
    /// the size of ControllinoMessage is deliberately set here because checksums are added to the content of struct
    pub const MESSAGE_SIZE: usize = 8;

    /// command for setting a single relay
    pub const COMMAND_SET_RELAY: char = 'S';

    /// command for unsetting a single relay
    pub const COMMAND_UNSET_RELAY: char = 'U';

    /// command for pulsing a relay
    pub const COMMAND_PULSE_RELAY: char = 'P';

    /// command for getting relay status
    pub const COMMAND_GET_RELAY_STATUS: char = 'G';

    /// command for sending a heart beat from control application to Controllino
    pub const COMMAND_HEARTBEAT: char = 'H';

    /// command for setting a digital output    
    pub const COMMAND_SET_DIGITAL_OUT: char = 'D';

    /// command for unsetting a digital output    
    pub const COMMAND_UNSET_DIGITAL_OUT: char = 'J';

    /// command for getting digital IO status
    pub const COMMAND_GET_DIGITAL_OUT_STATUS: char = 'G';

    /// initial wait period after resetting DTR
    pub const SLEEP_AFTER_DTR_RESET_MILLIS: u64 = 1000;

    /// initial wait period after clearing buffer
    pub const SLEEP_AFTER_CLEAR_BUFFER_MILLIS: u64 = 2000;
}

/// Calculates and validates the checksums of a byte sequence received from the Controllino.
///
/// This private helper function computes two XOR-based checksums from specific bytes
/// within the input `data` array and compares them against the checksums embedded
/// in the received data itself. It's crucial for verifying the integrity of
/// messages sent by the Controllino device.
///
/// If the checksums do not match, a warning is logged with detailed information
/// about the discrepancy, aiding in debugging communication issues.
///
/// # Arguments
/// * `data` - A slice of `u8` bytes representing the message received from the Controllino.
///   It is expected to be at least 8 bytes long.
///
/// # Returns
/// - `true`: If both calculated checksums match the received checksums, indicating data integrity.
/// - `false`: If the checksums do not match.
fn check_controllino_checksums(data: &[u8]) -> bool {
    let first_checksum = data[0] ^ data[1] ^ data[2];
    let second_checksum = data[4] ^ data[5] ^ data[6];
    let ok_result = (data[3] == first_checksum) && (data[7] == second_checksum);
    if !ok_result {
        warn!(target: module_path!(),
            "data used for checksum calculation: \n\
            [0]={} [1]={} [2]={} [3]={} [4]={} [5]={} [6]={} [7]={}\n\
            first checksum ([3]) should have been {}\n\
            second checksum ([7]) should have been {}",
            data[0], data[1], data[2], data[3],
            data[4], data[5], data[6], data[7],
            first_checksum,
            second_checksum,
        );
    }
    ok_result
}

#[derive(Clone, Deserialize, Debug)]
/// Configuration settings for direct hardware actuation via the Controllino device.
///
/// This struct holds all parameters required to establish and manage communication
/// with the Controllino (e.g., serial port settings) and maps application-level
/// devices to their specific relay IDs on the Controllino board.
pub struct ActuateControllinoConfig {
    /// Disables the functionality - required for certain testing scenarios
    active: bool,

    /// Baud rate for communication via serial port
    baud_rate: u32,

    /// Port name for communication via serial port
    port_name: String,

    /// timeout in milliseconds for serial port communication
    timeout_millis: u64,

    /// time in milliseconds to wait for response
    controllino_processing_millis: u64,

    /// Relay id of the protein skimmer
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub skimmer_id: u8,

    /// Relay id of the main pump #1
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub main_pump1_id: u8,

    /// Relay id of the main pump #2
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub main_pump2_id: u8,

    /// Relay id of the auxiliary pump #1
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub aux_pump1_id: u8,

    /// Relay id of the auxiliary pump #2
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub aux_pump2_id: u8,

    /// Relay id of the refill pump
    pub refill_pump_id: u8,

    /// Relay id of the heater
    pub heater_id: u8,

    /// Relay id of ventilation
    pub ventilation_id: u8,

    /// Relay id feeder
    pub feeder_id: u8,

    /// Relay id of peristaltic pump #1
    pub peristaltic_pump1_id: u8,

    /// Relay id of peristaltic pump #2
    pub peristaltic_pump2_id: u8,

    /// Relay id of peristaltic pump #3
    pub peristaltic_pump3_id: u8,

    /// Relay id of peristaltic pump #4
    pub peristaltic_pump4_id: u8,

    /// Heartbeat interval
    pub heartbeat_interval_seconds: u64,
}

/// Contains error definitions for ActuateControllino
#[derive(Error, Debug)]
pub enum ActuateControllinoError {
    /// Relay ID is outside the allowed range.
    #[error("Relay ID for Controllino ({0}) is outside the allowed range ({1} ... {2}).")]
    RelayIdOutsideRange(u8, u8, u8),

    /// Found a duplicate value in definition of relay IDs.
    #[error("Found a duplicate value ({0}) in definition of relay IDs.")]
    RelayIdDuplicate(u8),

    /// Port name for communication with Controllino is empty.
    #[error("Port name for communication with Controllino is empty.")]
    EmptyPortName,

    /// Failed to open serial port for Controllino.
    #[error("Failed to open serial port for Controllino.")]
    SerialPortOpenFailed {
        #[source]
        source: serialport::Error,
    },

    /// Failed to set timeout for serial port communication.
    #[error(
        "Could not set timeout of {timeout_millis} milliseconds for serial port communication."
    )]
    SetTimeoutFailed {
        timeout_millis: u64,

        #[source]
        source: serialport::Error,
    },

    /// Failed to clear DTR of serial port communication.
    #[error("Failed to clear DTR of serial port communication.")]
    ClearDTRFailed {
        #[source]
        source: serialport::Error,
    },

    /// Failed to clear buffer of serial port communication.
    #[error("Failed to clear buffer of serial port communication.")]
    ClearBufferFailed {
        #[source]
        source: serialport::Error,
    },
}

/// Contains relay configuration and trait implementation for actuation using Controllino hardware
pub struct ActuateControllino {
    /// Configuration for setup of Controllino device
    config: ActuateControllinoConfig,

    /// Object is used frequently to wait for a determined period of time, so storing it instead of recreating when needed.
    /// Attribute is public because access from within test cases.
    pub spin_sleeper: SpinSleeper,

    /// Object is used frequently to wait for a determined period of time, so storing it instead of recreating when needed.
    /// Attribute is public because access from within test cases.
    pub controllino_processing_duration: Duration,

    /// Interface to serial port for communication via USB
    /// Attribute is public because access from within test cases.
    pub serial_port_opt: Option<Box<dyn SerialPort>>,
}

/// Creates the message to be sent to Controllino based on an internal command.
///
/// This function translates a high-level `InternalCommand` into a low-level,
/// hardware-specific `ControllinoMessage` ready for serial transmission.
///
/// # Arguments
/// * `internal_command` - The `InternalCommand` specifying the device and action.
/// * `config` - A reference to the `ActuateControllinoConfig` for device-to-relay mappings.
///
/// # Returns
/// A `Result` containing the constructed `ControllinoMessage` on success.
///
/// # Errors
/// Returns a `RelayError` if the `internal_command` is not applicable for actuation
/// (e.g., `ResetAllErrors`) or if `get_relay_command` fails for other reasons.
fn create_command_message(
    internal_command: InternalCommand,
    config: &ActuateControllinoConfig,
) -> Result<ControllinoMessage, RelayError> {
    match internal_command {
        InternalCommand::SwitchOn(ref c)
        | InternalCommand::SwitchOff(ref c)
        | InternalCommand::Pulse(ref c, _) => {
            let relay_command = get_relay_command(&internal_command, c.clone(), config)?;

            let (relay_command_char, relay_id, duration_millis) = match relay_command {
                InternalCommand::SetRelay(relay_id) => {
                    // relay id is set inside this crate, so the cast is safe
                    (controllino_constants::COMMAND_SET_RELAY, relay_id as u8, 0)
                }
                InternalCommand::UnsetRelay(relay_id) => {
                    // relay id is set inside this crate, so the cast is safe
                    (
                        controllino_constants::COMMAND_UNSET_RELAY,
                        relay_id as u8,
                        0,
                    )
                }
                InternalCommand::PulseRelay(relay_id, duration_millis) => {
                    // relay id is set inside this crate, so the cast is safe
                    (
                        controllino_constants::COMMAND_PULSE_RELAY,
                        relay_id as u8,
                        duration_millis,
                    )
                }
                _ => {
                    return Err(RelayError::IrrelevantCommand(
                        module_path!().to_string(),
                        internal_command,
                    ));
                }
            };
            let controllino_message_content =
                ControllinoMessageContent::new(relay_command_char, relay_id, duration_millis);
            let controllino_message = ControllinoMessage::new(controllino_message_content);
            Ok(controllino_message)
        }
        _ => Err(RelayError::IrrelevantCommand(
            module_path!().to_string(),
            internal_command,
        )),
    }
}

/// Creates a heartbeat message to be sent to the Controllino.
///
/// This function constructs a standard `ControllinoMessage` with the heartbeat
/// command character. This is used to periodically signal to the hardware
/// that the control software is still active.
///
/// # Returns
/// A new `ControllinoMessage` configured for a heartbeat.
fn create_heartbeat_message() -> ControllinoMessage {
    let message_content =
        ControllinoMessageContent::new(controllino_constants::COMMAND_HEARTBEAT, 0, 0);
    ControllinoMessage::new(message_content)
}

/// Creates a Controllino-specific message from a high-level internal command.
///
/// This private helper function translates a generic `InternalCommand` (like `SwitchOn`, `SwitchOff`, or `Pulse`)
/// into the precise `ControllinoMessage` format required for serial communication with the Controllino hardware.
/// It uses the provided configuration to map `AquariumDevice` types to their
/// corresponding relay IDs and to determine the correct relay actuation logic (e.g., `SetRelay` vs. `UnsetRelay`).
///
/// # Arguments
/// * `internal_command` - The `InternalCommand` enum value representing the desired action (e.g., switching a device on/off or pulsing a relay).
/// * `config` - A reference to the `ActuateControllinoConfig`, which provides device-to-relay ID mappings and other settings.
///
/// # Returns
/// A `Result` containing the low-level `InternalCommand` (e.g., `SetRelay`, `PulseRelay`) on success.
///
/// # Errors
/// This function will return a `RelayError` if:
/// - The `internal_command` is not one that this function can translate for Controllino actuation (e.g., `ResetAllErrors`), resulting in `IrrelevantCommand`.
/// - A `Pulse` command is issued for a device that does not support pulsing, resulting in `PulseNotAllowedForDevice`.
pub fn get_relay_command(
    internal_command: &InternalCommand,
    device: AquariumDevice,
    config: &ActuateControllinoConfig,
) -> Result<InternalCommand, RelayError> {
    let (relay_id, is_inverted, supports_pulse) = match device {
        // Inverted logic, no pulse
        AquariumDevice::Skimmer => (config.skimmer_id, true, false),
        AquariumDevice::MainPump1 => (config.main_pump1_id, true, false),
        AquariumDevice::MainPump2 => (config.main_pump2_id, true, false),
        AquariumDevice::AuxPump1 => (config.aux_pump1_id, true, false),
        AquariumDevice::AuxPump2 => (config.aux_pump2_id, true, false),

        // Normal logic, no pulse
        AquariumDevice::Heater => (config.heater_id, false, false),
        AquariumDevice::Ventilation => (config.ventilation_id, false, false),
        AquariumDevice::RefillPump => (config.refill_pump_id, false, false),
        AquariumDevice::Feeder => (config.feeder_id, false, false),

        // Normal logic, with pulse
        AquariumDevice::PeristalticPump1 => (config.peristaltic_pump1_id, false, true),
        AquariumDevice::PeristalticPump2 => (config.peristaltic_pump2_id, false, true),
        AquariumDevice::PeristalticPump3 => (config.peristaltic_pump3_id, false, true),
        AquariumDevice::PeristalticPump4 => (config.peristaltic_pump4_id, false, true),
    };

    match internal_command {
        InternalCommand::SwitchOn(_) => {
            if is_inverted {
                Ok(InternalCommand::UnsetRelay(relay_id as u16))
            } else {
                Ok(InternalCommand::SetRelay(relay_id as u16))
            }
        }
        InternalCommand::SwitchOff(_) => {
            if is_inverted {
                Ok(InternalCommand::SetRelay(relay_id as u16))
            } else {
                Ok(InternalCommand::UnsetRelay(relay_id as u16))
            }
        }
        InternalCommand::Pulse(_, t) => {
            if supports_pulse {
                Ok(InternalCommand::PulseRelay(relay_id as u16, *t))
            } else {
                Err(RelayError::PulseNotAllowedForDevice(
                    module_path!().to_string(),
                    device,
                ))
            }
        }
        _ => Err(RelayError::IrrelevantCommand(
            module_path!().to_string(),
            internal_command.clone(),
        )),
    }
}

impl RelayActuationTrait for ActuateControllino {
    /// Actuates a specific relay on the Controllino hardware via serial port communication.
    ///
    /// This function translates high-level `InternalCommand`s (`SwitchOn`, `SwitchOff`, `Pulse`)
    /// into low-level serial messages for the Controllino. It sends the command, waits for
    /// the Controllino to process it (including any pulse durations), and then reads
    /// and validates the response, including checksum verification.
    ///
    /// # Arguments
    /// * `internal_command` - The `InternalCommand` specifying the device and desired action (e.g., `SwitchOn(Heater)`, `Pulse(PeristalticPump1, 500)`).
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the command was sent successfully and a valid acknowledgment was received from the Controllino.
    ///
    /// # Errors
    /// This function will return a `RelayError` if any step in the communication process fails:
    /// - `FailureCommandMessageCreation`: If there's an issue preparing the Controllino message.
    /// - `WriteError`: If the command cannot be written to the serial port.
    /// - `ReadError`: If a response cannot be read from the serial port within the timeout.
    /// - `IncorrectChecksum`: If the received response has an invalid checksum, indicating data corruption.
    /// - `SerialPortNotConfigured`: If the serial port was not initialized (e.g., `active` is false in config).
    /// - `IrrelevantCommand`: If an inapplicable command (e.g., `ResetAllErrors`) is provided.
    fn actuate(&mut self, internal_command: &InternalCommand) -> Result<(), RelayError> {
        match internal_command {
            InternalCommand::SwitchOn(ref _c)
            | InternalCommand::SwitchOff(ref _c)
            | InternalCommand::Pulse(ref _c, _) => {
                // create a message to be sent to Controllino
                let message = match create_command_message(internal_command.clone(), &self.config) {
                    Ok(c) => c,
                    Err(e) => {
                        error!(
                            target: module_path!(),
                            "preparation of communication failed ({e:?})"
                        );
                        return Err(RelayError::FailureCommandMessageCreation(
                            module_path!().to_string(),
                        ));
                    }
                };

                // write a message to Controllino
                if let Some(serial_port) = self.serial_port_opt.as_mut() {
                    match serial_port.write(&message.data) {
                        Ok(amount_byte_written) => {
                            if amount_byte_written != message.data.len() {
                                warn!(
                                        "{}: Writing to Controllino device failed. Amount of bytes written: {}. Expected: {}",
                                        module_path!(),
                                        amount_byte_written,
                                        message.data.len()
                                    );
                            }
                        }
                        Err(e) => {
                            error!(
                                target: module_path!(),
                                "sending command to Controllino failed ({e:?})"
                            );
                            return Err(RelayError::WriteError(module_path!().to_string()));
                        }
                    }

                    // give Controllino some time to process the command
                    self.spin_sleeper
                        .sleep(self.controllino_processing_duration);

                    // for pulse command, also wait the pulse duration
                    match internal_command {
                        InternalCommand::Pulse(_, t) => {
                            let pulse_duration_millis = *t;
                            let pulse_duration =
                                Duration::from_millis(pulse_duration_millis as u64);
                            self.spin_sleeper.sleep(pulse_duration);
                        }
                        _ => { /* do nothing */ }
                    }

                    // read response from Controllino
                    let mut serial_buf: Vec<u8> = vec![0; controllino_constants::MESSAGE_SIZE];
                    match serial_port.read_exact(serial_buf.as_mut_slice()) {
                        Ok(()) => {
                            if !check_controllino_checksums(serial_buf.as_slice()) {
                                warn!(
                                    target: module_path!(),
                                    "Received response from device with incorrect checksums"
                                );
                                Err(RelayError::IncorrectChecksum(module_path!().to_string()))
                            } else {
                                Ok(())
                            }
                        }
                        Err(_) => {
                            error!(
                                target: module_path!(),
                                "receiving response from device failed."
                            );
                            Err(RelayError::ReadError(module_path!().to_string()))
                        }
                    }
                } else {
                    Err(RelayError::SerialPortNotConfigured(
                        module_path!().to_string(),
                    ))
                }
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

    /// Gets the configured heartbeat interval in seconds.
    ///
    /// This function returns the interval at which the `heartbeat` method should be called
    /// to keep the hardware connection alive.
    ///
    /// # Returns
    /// An `Option<u64>` containing the heartbeat interval in seconds. It returns `Some`
    /// for this implementation as the interval is always configured.
    fn get_heartbeat_interval_seconds(&self) -> Option<u64> {
        Some(self.config.heartbeat_interval_seconds)
    }

    /// Sends a heartbeat signal to the Controllino to indicate the software is active.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) on success.
    ///
    /// # Errors
    /// This function will return a `RelayError` if:
    /// - The serial port is not configured (`SerialPortNotConfigured`).
    /// - Sending the heartbeat message over the serial port fails (`SendToSerialPortFailed`).
    fn heartbeat(&mut self) -> Result<(), RelayError> {
        if let Some(serial_port) = self.serial_port_opt.as_mut() {
            // create a message to be sent to Controllino
            let message = create_heartbeat_message();

            self.spin_sleeper
                .sleep(self.controllino_processing_duration);

            // write a message to Controllino
            match serial_port.write(&message.data) {
                Ok(amount_byte_written) => {
                    if amount_byte_written != message.data.len() {
                        warn!(
                            "{}: Writing heartbeat message to Controllino device failed. Amount of bytes written: {}. Expected: {}",
                            module_path!(),
                            amount_byte_written,
                            message.data.len()
                        );
                    }
                }
                Err(e) => {
                    return Err(RelayError::SendToSerialPortFailed {
                        location: module_path!().to_string(),
                        source: e,
                    });
                }
            }

            self.spin_sleeper
                .sleep(self.controllino_processing_duration);
        }
        Ok(())
    }

    /// Flushes the serial communication buffer to recover from framing or synchronization errors.
    ///
    /// The root cause is usually a desynchronization between the sender (Controllino) and receiver (Raspberry Pi). This can happen due to:
    /// - Noise on the line: A stray bit flip could make the Pi misinterpret the start of a message.
    /// - Timing issues: The Pi's read might start too early or too late relative to when the Controllino actually begins sending data.
    /// - Buffer state: Leftover bytes from a previous, incomplete, or corrupted transmission can throw off further reads.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) on success.
    ///
    /// # Errors
    /// This function will return a `RelayError` if:
    /// - The serial port is not configured (`SerialPortNotConfigured`).
    /// - Clearing the serial port buffer fails (`SerialPortFlushFailed`).
    fn flush_buffer(&mut self) -> Result<(), RelayError> {
        if let Some(serial_port) = self.serial_port_opt.as_mut() {
            match serial_port.clear(ClearBuffer::All) {
                Ok(_) => {
                    warn!(
                        "{}: flushed Controllino communication buffer.",
                        module_path!()
                    );
                }
                Err(e) => {
                    return Err(RelayError::SerialPortFlushFailed {
                        location: module_path!().to_string(),
                        source: e,
                    });
                }
            }

            self.spin_sleeper
                .sleep(self.controllino_processing_duration);
        }
        Ok(())
    }
}

/// Opens and initializes a serial port connection to the Controllino device.
///
/// This function attempts to establish a serial communication link using the
/// provided port name and baud rate. It also configures the port timeout,
/// performs a DTR reset, and clears input/output buffers to ensure a clean
/// communication state before returning the opened port.
///
/// # Arguments
/// * `port_name` - The name of the serial port (e.g., "/dev/ttyUSB0" on Linux).
/// * `baud_rate` - The baud rate for serial communication (e.g., 9600).
/// * `timeout_millis` - The read/write timeout for serial port operations in milliseconds.
///
/// # Returns
/// A `Result` containing a `Box<dyn SerialPort>` representing the opened and configured serial port on success.
///
/// # Errors
/// Returns an `ActuateControllinoError` if any step of the initialization fails:
/// - `SerialPortOpenFailed`: If the port cannot be opened.
/// - `SetTimeoutFailed`: If the timeout cannot be set on the port.
/// - `ClearDTRFailed`: If the Data Terminal Ready signal cannot be cleared.
/// - `ClearBufferFailed`: If the serial port's internal buffers cannot be cleared.
fn controllino_open_serial_port(
    port_name: String,
    baud_rate: u32,
    timeout_millis: u64,
) -> Result<Box<dyn SerialPort>, ActuateControllinoError> {
    let spin_sleeper = SpinSleeper::default();

    let mut serial_port = serialport::new(port_name.clone(), baud_rate)
        .open()
        .map_err(|e| ActuateControllinoError::SerialPortOpenFailed { source: e })?;

    let timeout_duration = Duration::from_millis(timeout_millis);
    serial_port.set_timeout(timeout_duration).map_err(|e| {
        ActuateControllinoError::SetTimeoutFailed {
            timeout_millis,
            source: e,
        }
    })?;

    // write to Data Terminal Ready pin: clear the signal
    serial_port
        .write_data_terminal_ready(false)
        .map_err(|e| ActuateControllinoError::ClearDTRFailed { source: e })?;

    // wait for the device
    let sleep_duration_after_dtr_reset =
        Duration::from_millis(controllino_constants::SLEEP_AFTER_DTR_RESET_MILLIS);
    spin_sleeper.sleep(sleep_duration_after_dtr_reset);

    // flush the buffer
    serial_port
        .clear(ClearBuffer::All)
        .map_err(|e| ActuateControllinoError::ClearBufferFailed { source: e })?;

    // wait for the device
    let sleep_duration_after_buffer_clear =
        Duration::from_millis(controllino_constants::SLEEP_AFTER_CLEAR_BUFFER_MILLIS);
    spin_sleeper.sleep(sleep_duration_after_buffer_clear);

    Ok(serial_port)
}

impl ActuateControllino {
    /// Validates the provided relay ID configuration for the Controllino.
    ///
    /// This function performs two critical checks:
    /// 1. Ensures that all configured relay IDs fall within the permissible range
    ///    defined by `CONTROLLINO_MIN_RELAY_ID` and `CONTROLLINO_MAX_RELAY_ID`.
    /// 2. It confirms that all assigned relay IDs are unique, preventing conflicts
    ///    where multiple devices might be configured to the same physical relay.
    ///
    /// # Arguments
    /// * `config` - A reference to the `ActuateControllinoConfig` containing the
    ///   relay ID mappings for various aquarium devices.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the configuration is valid.
    ///
    /// # Errors
    /// Returns an `ActuateControllinoError` if an invalid configuration is found:
    /// - `RelayIdOutsideRange`: If a relay ID is less than the minimum or greater than the maximum allowed value.
    /// - `RelayIdDuplicate`: If the same relay ID is assigned to more than one device.
    pub fn check_valid_relay_id_config(
        config: &ActuateControllinoConfig,
    ) -> Result<(), ActuateControllinoError> {
        // check if relay ids are in valid range and if they are distinct
        let mut seen_values = HashSet::new();

        let relay_id_values = [
            config.aux_pump1_id,
            config.aux_pump2_id,
            config.feeder_id,
            config.heater_id,
            config.main_pump1_id,
            config.main_pump2_id,
            config.peristaltic_pump1_id,
            config.peristaltic_pump2_id,
            config.peristaltic_pump3_id,
            config.peristaltic_pump4_id,
            config.refill_pump_id,
            config.ventilation_id,
        ];

        for &value in &relay_id_values {
            if !(CONTROLLINO_MIN_RELAY_ID..=CONTROLLINO_MAX_RELAY_ID).contains(&value) {
                return Err(ActuateControllinoError::RelayIdOutsideRange(
                    value,
                    CONTROLLINO_MIN_RELAY_ID,
                    CONTROLLINO_MAX_RELAY_ID,
                ));
            }
            if !seen_values.insert(value) {
                return Err(ActuateControllinoError::RelayIdDuplicate(value));
            }
        }

        Ok(())
    }

    /// Creates a new `ActuateControllino` instance, establishing communication with the Controllino hardware.
    ///
    /// This constructor performs essential setup for hardware actuation. It validates the provided
    /// relay ID configuration for correctness and uniqueness, ensures a serial port name is provided,
    /// and then opens and initializes the serial port connection to the Controllino device if `active` is true.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the Controllino relays, which is moved into the new instance.
    ///
    /// # Returns
    /// A `Result` containing a new `ActuateControllino` struct, ready for sending commands to the hardware.
    ///
    /// # Errors
    /// Returns an `ActuateControllinoError` if any part of the setup fails:
    /// - If `check_valid_relay_id_config` finds an error in the relay IDs.
    /// - `EmptyPortName`: If the `port_name` in the configuration is an empty string.
    /// - If `controllino_open_serial_port` fails to open and configure the serial port.
    pub fn new(
        config: ActuateControllinoConfig,
    ) -> Result<ActuateControllino, ActuateControllinoError> {
        let spin_sleeper = SpinSleeper::default();

        // check the validity of configuration
        Self::check_valid_relay_id_config(&config)?;

        if config.port_name.is_empty() {
            return Err(ActuateControllinoError::EmptyPortName);
        }

        let mut serial_port_opt: Option<Box<dyn SerialPort>> = None;
        if config.active {
            serial_port_opt = Some(controllino_open_serial_port(
                config.port_name.clone(),
                config.baud_rate,
                config.timeout_millis,
            )?);
        }
        let controllino_processing_millis = config.controllino_processing_millis;
        let controllino_processing_duration = Duration::from_millis(controllino_processing_millis);

        Ok(ActuateControllino {
            config,
            spin_sleeper,
            controllino_processing_duration,
            serial_port_opt,
        })
    }
}

#[cfg(test)]
pub mod tests {
    // feature-specific imports
    cfg_if::cfg_if! {
        if #[cfg(feature = "controllino_hw")] {
            use serialport::SerialPort;
            use spin_sleep::SpinSleeper;
            use std::time::Duration;

            use crate::relays::controllino::{Controllino, RelayActuationTrait};
            use crate::relays::controllino_actuate_hardware::ControllinoActuateHardware;
        }
    }

    use crate::utilities::channel_content::AquariumDevice::{
        AuxPump1, AuxPump2, Feeder, Heater, MainPump1, MainPump2, PeristalticPump1,
        PeristalticPump2, PeristalticPump3, PeristalticPump4, Skimmer, Ventilation,
    };

    use crate::relays::actuate_controllino::{
        controllino_constants, create_command_message, ActuateControllino, ActuateControllinoError,
        CONTROLLINO_MAX_RELAY_ID, CONTROLLINO_MIN_RELAY_ID,
    };
    use crate::relays::relay_error::RelayError;
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::config::{read_config_file, ConfigData};

    #[test]
    pub fn test_invalid_relay_id_config_low() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

        config.controllino_relay.main_pump1_id = CONTROLLINO_MIN_RELAY_ID - 1;

        let test_result = ActuateControllino::new(config.controllino_relay);
        assert!(matches!(
            test_result,
            Err(ActuateControllinoError::RelayIdOutsideRange(_, _, _))
        ));
    }
    #[test]
    pub fn test_invalid_relay_id_config_high() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

        config.controllino_relay.main_pump1_id = CONTROLLINO_MAX_RELAY_ID + 1;

        let test_result = ActuateControllino::new(config.controllino_relay);
        assert!(matches!(
            test_result,
            Err(ActuateControllinoError::RelayIdOutsideRange(_, _, _))
        ));
    }

    #[test]
    pub fn test_invalid_relay_config_duplicate() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

        config.controllino_relay.main_pump1_id = CONTROLLINO_MAX_RELAY_ID;
        config.controllino_relay.main_pump2_id = CONTROLLINO_MAX_RELAY_ID;

        let test_result = ActuateControllino::new(config.controllino_relay);
        assert!(matches!(
            test_result,
            Err(ActuateControllinoError::RelayIdDuplicate(_))
        ));
    }

    #[cfg(all(target_os = "macos", feature = "controllino_hw"))]
    fn get_platform_specific_configuration_file() -> String {
        "/config/aquarium_control_test_controllino_macos.toml".to_string()
    }

    #[cfg(all(target_os = "linux", feature = "controllino_hw"))]
    fn get_platform_specific_configuration_file() -> String {
        "/config/aquarium_control_test_controllino_linux.toml".to_string()
    }

    #[cfg(feature = "controllino_hw")]
    #[test]
    // Test case actuates on real hardware.
    // Execution of this test case requires serial port connection to the Controllino device.
    pub fn test_controllino_switch_on_off_pulse() {
        let sleep_duration_500_millis = Duration::from_millis(500);
        let spin_sleeper = SpinSleeper::default();

        let config: ConfigData = read_config_file(get_platform_specific_configuration_file());
        let controllino = Controllino::new(config.relay_manager);

        let config2: ConfigData = read_config_file(get_platform_specific_configuration_file());

        let serial_port: Box<dyn SerialPort>;
        serial_port = match controllino.serial_port_opt.as_ref().unwrap().try_clone() {
            Ok(c) => c,
            Err(_) => {
                panic!("Controllino: Error occurred when trying to clone serial port.");
            }
        };

        let mut actuator = ControllinoActuateHardware;

        let expect_info =
            "Controllino test_switch_on_off_pulse failed cloning serial port interface.";

        // Request Controllino to switch on and off all relays sequentially
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(Skimmer),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(Ventilation),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(actuator.actuate(InternalCommand::SwitchOn(Heater),), Ok(()));
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(MainPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(MainPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(AuxPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(AuxPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(PeristalticPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(PeristalticPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(PeristalticPump3),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(PeristalticPump4),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(actuator.actuate(InternalCommand::SwitchOn(Feeder),), Ok(()));
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(Skimmer),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(Ventilation),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(Heater),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(MainPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(MainPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(AuxPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(AuxPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(PeristalticPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(PeristalticPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(PeristalticPump3),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(PeristalticPump4),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(Feeder),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);

        // Request Controllino to pulse all relays for peristaltic pumps
        assert_eq!(
            actuator.actuate(InternalCommand::Pulse(PeristalticPump1, 500),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::Pulse(PeristalticPump2, 500),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::Pulse(PeristalticPump3, 500),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::Pulse(PeristalticPump4, 500),),
            Ok(())
        );
    }

    #[test]
    /// Test case tests if an invalid command is rejected when preparing the message for Controllino.
    pub fn test_create_message_invalid_command() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_simulator.toml".to_string()).unwrap();
        let test_result =
            create_command_message(InternalCommand::ResetAllErrors, &config.controllino_relay);

        assert!(matches!(
            test_result,
            Err(RelayError::IrrelevantCommand(
                _,
                InternalCommand::ResetAllErrors
            )),
        ));
    }

    #[test]
    // Test case tests the implementation of the relay logic.
    // During the creation of the message to Controllino, the application considers
    // if relay needs to bet set or unset to switch on/off a device.
    pub fn test_controllino_create_message_switch_on_with_set_relay_switch_off_with_unset_relay() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_simulator.toml".to_string()).unwrap();

        // switching on all devices
        let mut message = create_command_message(
            InternalCommand::SwitchOn(Skimmer),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.skimmer_id);

        message = create_command_message(
            InternalCommand::SwitchOn(Ventilation),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.ventilation_id);

        message =
            create_command_message(InternalCommand::SwitchOn(Heater), &config.controllino_relay)
                .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.heater_id);

        message = create_command_message(
            InternalCommand::SwitchOn(MainPump1),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.main_pump1_id);

        message = create_command_message(
            InternalCommand::SwitchOn(MainPump2),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.main_pump2_id);

        message = create_command_message(
            InternalCommand::SwitchOn(AuxPump1),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.aux_pump1_id);

        message = create_command_message(
            InternalCommand::SwitchOn(AuxPump2),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.aux_pump2_id);

        message = create_command_message(
            InternalCommand::SwitchOn(PeristalticPump1),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump1_id
        );

        message = create_command_message(
            InternalCommand::SwitchOn(PeristalticPump2),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump2_id
        );

        message = create_command_message(
            InternalCommand::SwitchOn(PeristalticPump3),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump3_id
        );

        message = create_command_message(
            InternalCommand::SwitchOn(PeristalticPump4),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump4_id
        );

        message =
            create_command_message(InternalCommand::SwitchOn(Feeder), &config.controllino_relay)
                .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.feeder_id);

        // switching off all devices
        let message = create_command_message(
            InternalCommand::SwitchOff(Skimmer),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.skimmer_id);

        let mut message = create_command_message(
            InternalCommand::SwitchOff(Ventilation),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.ventilation_id);

        message = create_command_message(
            InternalCommand::SwitchOff(Heater),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.heater_id);

        message = create_command_message(
            InternalCommand::SwitchOff(MainPump1),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.main_pump1_id);

        message = create_command_message(
            InternalCommand::SwitchOff(MainPump2),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.main_pump2_id);

        message = create_command_message(
            InternalCommand::SwitchOff(AuxPump1),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.aux_pump1_id);

        message = create_command_message(
            InternalCommand::SwitchOff(AuxPump2),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_SET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.aux_pump2_id);

        message = create_command_message(
            InternalCommand::SwitchOff(PeristalticPump1),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump1_id
        );

        message = create_command_message(
            InternalCommand::SwitchOff(PeristalticPump2),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump2_id
        );

        message = create_command_message(
            InternalCommand::SwitchOff(PeristalticPump3),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump3_id
        );

        message = create_command_message(
            InternalCommand::SwitchOff(PeristalticPump4),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump4_id
        );

        message = create_command_message(
            InternalCommand::SwitchOff(Feeder),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_UNSET_RELAY as u8
        );
        assert_eq!(message.data[1], config.controllino_relay.feeder_id);
    }

    #[test]
    // Tests the message creation for pulse command.
    // Pulse command is only permitted for a subset of the devices (e.g., peristaltic pumps).
    pub fn test_create_message_pulse() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_simulator.toml".to_string()).unwrap();

        let mut error_message = create_command_message(
            InternalCommand::Pulse(Skimmer, 1000),
            &config.controllino_relay,
        );
        assert!(matches!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(_, Skimmer))
        ));

        error_message = create_command_message(
            InternalCommand::Pulse(Ventilation, 1000),
            &config.controllino_relay,
        );
        assert!(matches!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(_, Ventilation))
        ));

        error_message = create_command_message(
            InternalCommand::Pulse(Heater, 1000),
            &config.controllino_relay,
        );
        assert!(matches!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(_, Heater))
        ));

        error_message = create_command_message(
            InternalCommand::Pulse(MainPump1, 1000),
            &config.controllino_relay,
        );
        assert!(matches!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(_, MainPump1))
        ));

        error_message = create_command_message(
            InternalCommand::Pulse(MainPump2, 1000),
            &config.controllino_relay,
        );
        assert!(matches!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(_, MainPump2))
        ));

        error_message = create_command_message(
            InternalCommand::Pulse(AuxPump1, 1000),
            &config.controllino_relay,
        );
        assert!(matches!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(_, AuxPump1))
        ));

        error_message = create_command_message(
            InternalCommand::Pulse(AuxPump2, 1000),
            &config.controllino_relay,
        );
        assert!(matches!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(_, AuxPump2))
        ));

        let mut message = create_command_message(
            InternalCommand::Pulse(PeristalticPump1, 1000),
            &config.controllino_relay,
        )
        .unwrap();

        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump1_id
        );

        message = create_command_message(
            InternalCommand::Pulse(PeristalticPump2, 1000),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump2_id
        );

        message = create_command_message(
            InternalCommand::Pulse(PeristalticPump3, 1000),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump3_id
        );

        message = create_command_message(
            InternalCommand::Pulse(PeristalticPump4, 1000),
            &config.controllino_relay,
        )
        .unwrap();
        assert_eq!(
            message.data[0],
            controllino_constants::COMMAND_PULSE_RELAY as u8
        );
        assert_eq!(
            message.data[1],
            config.controllino_relay.peristaltic_pump4_id
        );

        error_message = create_command_message(
            InternalCommand::Pulse(Feeder, 1000),
            &config.controllino_relay,
        );
        assert!(matches!(
            error_message,
            Err(RelayError::PulseNotAllowedForDevice(_, Feeder))
        ));
    }

    #[cfg(feature = "controllino_hw")]
    #[test]
    // Test case actuates on real hardware.
    // Execution of this test case requires serial port connection to the Controllino device.
    pub fn test_controllino_simulate_feed_profile() {
        let sleep_duration_500_millis = Duration::from_millis(500);
        let sleep_duration_8_secs = Duration::from_secs(8);
        let sleep_duration_6_secs = Duration::from_secs(6);
        let spin_sleeper = SpinSleeper::default();

        let config: ConfigData = read_config_file(get_platform_specific_configuration_file());
        let controllino = Controllino::new(config.relay_manager);

        let config2: ConfigData = read_config_file(get_platform_specific_configuration_file());

        let serial_port: Box<dyn SerialPort>;
        serial_port = match controllino.serial_port_opt.as_ref().unwrap().try_clone() {
            Ok(c) => c,
            Err(_) => {
                panic!("Controllino: Error occurred when trying to clone serial port.");
            }
        };

        let mut actuator = ControllinoActuateHardware;

        let expect_info =
            "Controllino test_switch_on_off_pulse failed cloning serial port interface.";

        // Request Controllino to switch on and off all relays sequentially
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(MainPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(MainPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(AuxPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_8_secs);
        assert_eq!(actuator.actuate(InternalCommand::SwitchOn(Feeder),), Ok(()));
        spin_sleeper.sleep(sleep_duration_6_secs);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(Feeder),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_8_secs);
        assert_eq!(actuator.actuate(InternalCommand::SwitchOn(Feeder),), Ok(()));
        spin_sleeper.sleep(sleep_duration_6_secs);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOff(Feeder),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_8_secs);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(MainPump1),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(MainPump2),),
            Ok(())
        );
        spin_sleeper.sleep(sleep_duration_500_millis);
        assert_eq!(
            actuator.actuate(InternalCommand::SwitchOn(AuxPump1),),
            Ok(())
        );
    }
}
