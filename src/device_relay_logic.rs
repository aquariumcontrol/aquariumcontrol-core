use crate::channel_content::AquariumDevice;
use crate::channel_content::InternalCommand;
use crate::controllino::ControllinoConfig;
use crate::controllino::ControllinoError;

/// Provides one of two enum values of type InternalCommand wrapping the relay number.
/// Alternative relay logic can be considered here by choosing either SetRelay or UnsetRelay.
/// # Arguments
/// * `internal_command` - enum value which is interpreted
/// * `device` - enum value for which the function shall provide the relay id
pub fn get_relay_command(
    internal_command: &InternalCommand,
    device: AquariumDevice,
    config: &ControllinoConfig,
) -> Result<InternalCommand, ControllinoError> {
    match device {
        AquariumDevice::Skimmer => match internal_command {
            InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(config.skimmer_id as u16)),
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::UnsetRelay(config.skimmer_id as u16))
            }
            InternalCommand::Pulse(d, _) => {
                Err(ControllinoError::PulseNotAllowedForDevice(d.clone()))
            }
            _ => Err(ControllinoError::IrrelevantCommand(
                internal_command.clone(),
            )),
        },
        AquariumDevice::MainPump1 => match internal_command {
            InternalCommand::SwitchOn(_) => {
                Ok(InternalCommand::SetRelay(config.main_pump1_id as u16))
            }
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::UnsetRelay(config.main_pump1_id as u16))
            }
            InternalCommand::Pulse(d, _) => {
                Err(ControllinoError::PulseNotAllowedForDevice(d.clone()))
            }
            _ => Err(ControllinoError::IrrelevantCommand(
                internal_command.clone(),
            )),
        },
        AquariumDevice::MainPump2 => match internal_command {
            InternalCommand::SwitchOn(_) => {
                Ok(InternalCommand::SetRelay(config.main_pump2_id as u16))
            }
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::UnsetRelay(config.main_pump2_id as u16))
            }
            InternalCommand::Pulse(d, _) => {
                Err(ControllinoError::PulseNotAllowedForDevice(d.clone()))
            }
            _ => Err(ControllinoError::IrrelevantCommand(
                internal_command.clone(),
            )),
        },
        AquariumDevice::AuxPump1 => match internal_command {
            InternalCommand::SwitchOn(_) => {
                Ok(InternalCommand::SetRelay(config.aux_pump1_id as u16))
            }
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::UnsetRelay(config.aux_pump1_id as u16))
            }
            InternalCommand::Pulse(d, _) => {
                Err(ControllinoError::PulseNotAllowedForDevice(d.clone()))
            }
            _ => Err(ControllinoError::IrrelevantCommand(
                internal_command.clone(),
            )),
        },
        AquariumDevice::AuxPump2 => match internal_command {
            InternalCommand::SwitchOn(_) => {
                Ok(InternalCommand::SetRelay(config.aux_pump2_id as u16))
            }
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::UnsetRelay(config.aux_pump2_id as u16))
            }
            InternalCommand::Pulse(d, _) => {
                Err(ControllinoError::PulseNotAllowedForDevice(d.clone()))
            }
            _ => Err(ControllinoError::IrrelevantCommand(
                internal_command.clone(),
            )),
        },
        AquariumDevice::Heater => match internal_command {
            InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(config.heater_id as u16)),
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::UnsetRelay(config.heater_id as u16))
            }
            InternalCommand::Pulse(d, _) => {
                Err(ControllinoError::PulseNotAllowedForDevice(d.clone()))
            }
            _ => Err(ControllinoError::IrrelevantCommand(
                internal_command.clone(),
            )),
        },
        AquariumDevice::Ventilation => match internal_command {
            InternalCommand::SwitchOn(_) => {
                Ok(InternalCommand::SetRelay(config.ventilation_id as u16))
            }
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::UnsetRelay(config.ventilation_id as u16))
            }
            InternalCommand::Pulse(d, _) => {
                Err(ControllinoError::PulseNotAllowedForDevice(d.clone()))
            }
            _ => Err(ControllinoError::IrrelevantCommand(
                internal_command.clone(),
            )),
        },
        AquariumDevice::RefillPump => match internal_command {
            InternalCommand::SwitchOn(_) => {
                Ok(InternalCommand::SetRelay(config.refill_pump_id as u16))
            }
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::UnsetRelay(config.refill_pump_id as u16))
            }
            InternalCommand::Pulse(d, _) => {
                Err(ControllinoError::PulseNotAllowedForDevice(d.clone()))
            }
            _ => Err(ControllinoError::IrrelevantCommand(
                internal_command.clone(),
            )),
        },
        AquariumDevice::Feeder => match internal_command {
            InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(config.feeder_id as u16)),
            InternalCommand::SwitchOff(_) => {
                Ok(InternalCommand::UnsetRelay(config.feeder_id as u16))
            }
            InternalCommand::Pulse(d, _) => {
                Err(ControllinoError::PulseNotAllowedForDevice(d.clone()))
            }
            _ => Err(ControllinoError::IrrelevantCommand(
                internal_command.clone(),
            )),
        },
        AquariumDevice::PeristalticPump1 => match internal_command {
            InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(
                config.peristaltic_pump1_id as u16,
            )),
            InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(
                config.peristaltic_pump1_id as u16,
            )),
            InternalCommand::Pulse(_, t) => Ok(InternalCommand::PulseRelay(
                config.peristaltic_pump1_id as u16,
                *t,
            )),
            _ => Err(ControllinoError::IrrelevantCommand(
                internal_command.clone(),
            )),
        },
        AquariumDevice::PeristalticPump2 => match internal_command {
            InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(
                config.peristaltic_pump2_id as u16,
            )),
            InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(
                config.peristaltic_pump2_id as u16,
            )),
            InternalCommand::Pulse(_, t) => Ok(InternalCommand::PulseRelay(
                config.peristaltic_pump2_id as u16,
                *t,
            )),
            _ => Err(ControllinoError::IrrelevantCommand(
                internal_command.clone(),
            )),
        },
        AquariumDevice::PeristalticPump3 => match internal_command {
            InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(
                config.peristaltic_pump3_id as u16,
            )),
            InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(
                config.peristaltic_pump3_id as u16,
            )),
            InternalCommand::Pulse(_, t) => Ok(InternalCommand::PulseRelay(
                config.peristaltic_pump3_id as u16,
                *t,
            )),
            _ => Err(ControllinoError::IrrelevantCommand(
                internal_command.clone(),
            )),
        },
        AquariumDevice::PeristalticPump4 => match internal_command {
            InternalCommand::SwitchOn(_) => Ok(InternalCommand::SetRelay(
                config.peristaltic_pump4_id as u16,
            )),
            InternalCommand::SwitchOff(_) => Ok(InternalCommand::UnsetRelay(
                config.peristaltic_pump4_id as u16,
            )),
            InternalCommand::Pulse(_, t) => Ok(InternalCommand::PulseRelay(
                config.peristaltic_pump4_id as u16,
                *t,
            )),
            _ => Err(ControllinoError::IrrelevantCommand(
                internal_command.clone(),
            )),
        },
    }
}
