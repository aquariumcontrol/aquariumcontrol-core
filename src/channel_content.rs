/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use std::fmt;

/// Enum is used for internal thread messaging from control circuits to Controllino.
/// These values are nested into enum InternalCommand. They represent all actuation devices.
#[derive(Clone, Debug, PartialEq)]
pub enum AquariumDevice {
    // protein skimmer
    Skimmer,

    // main pump #1
    MainPump1,

    // main pump #2
    MainPump2,

    // auxiliary pump #1
    AuxPump1,

    // auxiliary pump #2
    AuxPump2,

    // heater
    Heater,

    // ventilation
    Ventilation,

    // refill pump
    RefillPump,

    // feeder motor
    Feeder,

    // Balling dosing pump #1
    PeristalticPump1,

    // Balling dosing pump #2
    PeristalticPump2,

    // Balling dosing pump #3
    PeristalticPump3,

    // Balling dosing pump #4
    PeristalticPump4,
}

impl fmt::Display for AquariumDevice {
    /// Provides output of the enum values for debugging purposes.
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

/// Enum is used for internal thread messaging between various threads
/// The values indicate which information the a thread shall return.
#[derive(Clone, Debug, PartialEq)]
pub enum AquariumSignal {
    /// water temperature in °C
    WaterTemperature,

    #[allow(non_camel_case_types)]
    /// pH Value
    pH,

    /// conductivity im uS/cm
    Conductivity,

    /// ambient (air) temperature in °C
    AmbientTemperature,

    /// ambient humidity in %
    AmbientHumidity,

    /// position of tank level switch
    TankLevelSwitchPosition,

    /// status bit of tank level switch signal
    TankLevelSwitchInvalid,

    /// stabilized position of tank level switch
    TankLevelSwitchPositionStabilized,

    /// heating control status
    HeatingControlStatus,

    /// ventilation control status
    VentilationControlStatus,

    /// refill pump status
    RefillPumpStatus,

    #[cfg(all(target_os = "linux", feature = "target_hw", test))]
    MockInvalidSignal,
}

impl fmt::Display for AquariumSignal {
    /// Function provides output of the enum values for debugging purposes.
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
            AquariumSignal::TankLevelSwitchPositionStabilized => {
                write!(f, "TankLevelSwitchPositionStabilized")
            }
            AquariumSignal::HeatingControlStatus => write!(f, "HeatingControlStatus"),
            AquariumSignal::VentilationControlStatus => write!(f, "VentilationControlStatus"),
            AquariumSignal::RefillPumpStatus => write!(f, "RefillPumpStatus"),

            #[cfg(all(target_os = "linux", test, feature = "target_hw"))]
            AquariumSignal::MockInvalidSignal => write!(f, "MockInvalidSignal"),
        }
    }
}

/// Enum is used for internal thread messaging.
/// Note: Relay state and device state do not necessarily correlate.
#[derive(Clone, Debug, PartialEq)]
pub enum InternalCommand {
    /// Switch on a device
    SwitchOn(AquariumDevice),

    /// Switch off a device
    SwitchOff(AquariumDevice),

    /// Pulse a device (switch on, pause, switch off)
    #[allow(dead_code)]
    Pulse(AquariumDevice, u16),

    /// Set a relay
    SetRelay(u16),

    /// Unset a relay
    UnsetRelay(u16),

    /// Pulse a relay (switch on, pause, switch off)
    #[allow(dead_code)]
    PulseRelay(u16, u16),

    /// Request a signal
    RequestSignal(AquariumSignal),

    #[allow(dead_code)]
    /// Reset existing errors (used for refill control)
    ResetAllErrors,

    /// Quit the application - first step to terminate the application
    Quit,

    /// Terminate - second step to terminate the application
    Terminate,

    /// ScheduleCheck - request permission to run based on time of day
    ScheduleCheck,

    #[allow(dead_code)]
    /// (Re-)start controller (Balling, Ventilation, Refill, Heating)
    Start,

    #[allow(dead_code)]
    /// (Temporarily) stop controller (Balling, Ventilation, Refill, Heating)
    Stop,

    #[allow(dead_code)]
    /// Execute a (feed) profile with a given ID
    Execute(i32),

    #[allow(dead_code)]
    /// Default value used when unknown command is received via IPC message
    Unknown,
}

impl fmt::Display for InternalCommand {
    /// Provides output of the enum values for debugging purposes.
    /// # Arguments
    /// * `f` - configuration for formatting
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            InternalCommand::SwitchOn(_) => write!(f, "SwitchOn"),
            InternalCommand::SwitchOff(_) => write!(f, "SwitchOff"),
            InternalCommand::Pulse(_, _) => write!(f, "Pulse"),
            InternalCommand::SetRelay(_) => write!(f, "SetRelay"),
            InternalCommand::UnsetRelay(_) => write!(f, "UnsetRelay"),
            InternalCommand::PulseRelay(_, _) => write!(f, "PulseRelay"),
            InternalCommand::RequestSignal(_) => write!(f, "RequestSignal"),
            InternalCommand::ResetAllErrors => write!(f, "ResetAllErrors"),
            InternalCommand::Quit => write!(f, "Quit"),
            InternalCommand::Terminate => write!(f, "Terminate"),
            InternalCommand::ScheduleCheck => write!(f, "ScheduleCheck"),
            InternalCommand::Start => write!(f, "Start"),
            InternalCommand::Stop => write!(f, "Stop"),
            InternalCommand::Execute(_) => write!(f, "Execute"),
            InternalCommand::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Enum is used for representing the relay states.
#[derive(PartialEq)]
pub enum ActuatorState {
    /// Actuator is on
    On,

    /// Actuator is off
    Off,

    /// Application has not yet sent a command to the actuator
    Undefined,
}
