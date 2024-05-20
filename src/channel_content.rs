/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use std::fmt;

/// Enum is used for internal thread messaging from control circuits to Controllino.
/// These values are nested into enum InternalCommand. They represent all actuation devices.
#[derive(Clone, Debug)]
pub enum AquariumDevice {
    Skimmer,
    MainPump1,
    MainPump2,
    AuxPump1,
    AuxPump2,
    Heater,
    Ventilation,
    RefillPump,
    Feeder,
    PeristalticPump1,
    PeristalticPump2,
    PeristalticPump3,
    PeristalticPump4,
}

impl fmt::Display for AquariumDevice {
    /// Provides output of the enum values.
    /// # Arguments
    /// * `f` - configuration for formatting
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AquariumDevice::Skimmer => write!(f, "Skimmer"),
            AquariumDevice::MainPump1 => write!(f, "MainPump1"),
            AquariumDevice::MainPump2 => write!(f, "MainPump2"),
            AquariumDevice::AuxPump1 => write!(f, "AuxPump1"),
            AquariumDevice::AuxPump2 => write!(f, "AuxPump2"),
            AquariumDevice::Heater => write!(f, "Heater"),
            AquariumDevice::Ventilation => write!(f, "Ventilation"),
            AquariumDevice::RefillPump => write!(f, "RefillPump"),
            AquariumDevice::Feeder => write!(f, "Feeder"),
            AquariumDevice::PeristalticPump1 => write!(f, "PeristalticPump1"),
            AquariumDevice::PeristalticPump2 => write!(f, "PeristalticPump2"),
            AquariumDevice::PeristalticPump3 => write!(f, "PeristalticPump3"),
            AquariumDevice::PeristalticPump4 => write!(f, "PeristalticPump4"),
        }
    }
}

/// Enum is used for internal thread messaging between data logger and GPIO handler
/// The values indicate which information the GPIO handler thread shall return via channel.
#[derive(Clone, Debug)]
pub enum AquariumSignal {
    WaterTemperature,
    #[allow(non_camel_case_types)]
    pH,
    Conductivity,
    AmbientTemperature,
    AmbientHumidity,
    TankLevelSwitchPosition,
    TankLevelSwitchInvalid,
}

impl fmt::Display for AquariumSignal {
    /// Function provides output of the enum values.
    /// # Arguments
    /// * `f` - configuration for formatting    
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AquariumSignal::WaterTemperature => write!(f, "WaterTemperature"),
            AquariumSignal::pH => write!(f, "pH"),
            AquariumSignal::Conductivity => write!(f, "Conductivity"),
            AquariumSignal::AmbientTemperature => write!(f, "AmbientTemperature"),
            AquariumSignal::AmbientHumidity => write!(f, "AmbientHumidity"),
            AquariumSignal::TankLevelSwitchPosition => write!(f, "TankLevelSwitchPosition"),
            AquariumSignal::TankLevelSwitchInvalid => write!(f, "TankLevelSwitchInvalid"),
        }
    }
}

/// Enum is used for internal thread messaging.
#[derive(Clone, Debug)]
pub enum InternalCommand {
    SwitchOn(AquariumDevice),
    SwitchOff(AquariumDevice),
    SetRelay(u16),
    UnsetRelay(u16),
    RequestSignal(AquariumSignal),
    ResetAllErrors,
    Quit,
    Terminate,
}

impl fmt::Display for InternalCommand {
    /// Provides output of the enum values.
    /// # Arguments
    /// * `f` - configuration for formatting
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            InternalCommand::SwitchOn(_) => write!(f, "SwitchOn"),
            InternalCommand::SwitchOff(_) => write!(f, "SwitchOff"),
            InternalCommand::SetRelay(_) => write!(f, "SetRelay"),
            InternalCommand::UnsetRelay(_) => write!(f, "UnsetRelay"),
            InternalCommand::RequestSignal(_) => write!(f, "RequestSignal"),
            InternalCommand::ResetAllErrors => write!(f, "ResetAllErrors"),
            InternalCommand::Quit => write!(f, "Quit"),
            InternalCommand::Terminate => write!(f, "Terminate"),
        }
    }
}

/// Enum is used for representing the relay states.
#[derive(PartialEq)]
pub enum ActuatorState {
    On,
    Off,
    Undefined,
}
