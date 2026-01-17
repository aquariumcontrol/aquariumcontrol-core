/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the vocabulary for all inter-thread communication within the application.
//!
//! This module serves as the central "dictionary" for the entire system. It contains
//! the core `enum` definitions that represent the commands, signals, and states passed
//! between concurrent threads through `mpsc::channel`s.
//!
//! ## Purpose and Design
//!
//! By centralizing these definitions, this module establishes a single source of truth
//! for the application's internal messaging protocol. This design provides several key
//! benefits:
//!
//! - **Type Safety**: Using strongly typed enums prevents entire classes of bugs that
//!   could arise from using primitive types like strings or integers for commands.
//! - **Consistency**: All threads speak the same "language," ensuring that commands
//!   and signals are interpreted uniformly across the system.
//! - **Discoverability**: A developer can look at this single file to understand the
//!   full range of possible interactions between threads.
//!
//! ## Key Components
//!
//! - **`InternalCommand`**: The primary message type used for commanding actions. It
//!   encompasses everything from switching a device on/off to requesting a graceful
//!   shutdown of the application.
//!
//! - **`AquariumDevice`**: A sub-enum used within `InternalCommand` variants like
//!   `SwitchOn` and `SwitchOff`. It provides a type-safe way to specify which
//!   physical device is being targeted.
//!
//! - **`AquariumSignal`**: A sub-enum used by the `RequestSignal` command. It allows
//!   threads to request specific sensor readings or status information from other parts
//!   of the system.
//!
//! - **`ActuatorState`**: A simple state machine enum (`On`, `Off`, `Undefined`) used
//!   by control threads to track the logical state of the devices they manage.
//!
//! ## External Communication
//!
//! This module also contains logic for translating between the internal `InternalCommand`
//! format and external message formats, such as the numeric commands received via the
//! `messaging` feature. This isolates the external protocol from the internal application
//! logic.

#[cfg(target_os = "linux")]
use crate::dispatch::messaging::Message;
use strum_macros::Display;

/// Enum is used for internal thread messaging from control circuits to Controllino.
/// These values are nested into enum InternalCommand. They represent all actuation devices.
#[derive(Clone, Debug, PartialEq, Display)]
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

    // RGB LED
    Beacon,
}

/// Enum is used for internal thread messaging between various threads
/// The values indicate which information the thread shall return.
#[derive(Clone, Debug, Eq, PartialEq, Display, Hash)]
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

    #[allow(unused)]
    /// status bit of tank level switch signal
    TankLevelSwitchInvalid,

    #[allow(unused)]
    /// stabilized position of tank level switch
    TankLevelSwitchPositionStabilized,

    #[allow(unused)]
    /// heating control status
    HeatingControlStatus,

    #[allow(unused)]
    /// ventilation control status
    VentilationControlStatus,

    #[allow(unused)]
    /// refill pump status
    RefillPumpStatus,

    #[cfg(test)]
    MockInvalidSignal,
}

impl AquariumSignal {
    /// Formats a given floating-point signal value into a `String` based on the specific `AquariumSignal` type.
    ///
    /// This function applies specific formatting rules (e.g., number of decimal places)
    /// for common aquarium sensor signals to ensure consistent output strings,
    /// suitable for display or logging.
    ///
    /// # Arguments
    /// * `signal` - The `f32` floating-point value of the signal to be formatted.
    ///
    /// # Returns
    /// A `String` representation of the signal, formatted according to its `AquariumSignal` type.
    /// For example, `WaterTemperature` is formatted to one decimal place, `pH` to two, etc.
    pub fn output_file_string(&self, signal: f32) -> String {
        match self {
            AquariumSignal::WaterTemperature => format!("{signal:.1}"),
            AquariumSignal::Conductivity => format!("{signal:.0}"),
            AquariumSignal::pH => format!("{signal:.2}"),
            AquariumSignal::AmbientTemperature => format!("{signal:.1}"),
            AquariumSignal::AmbientHumidity => format!("{signal:.1}"),
            #[cfg(all(target_os = "linux", feature = "target_hw", test))]
            AquariumSignal::MockInvalidSignal => "MockInvalid".to_string(),
            _ => signal.to_string(),
        }
    }

    /// Defines the sequence of measurement used by AtlasScientific and also indicates
    /// completion of one measurement set
    #[allow(unused)] // used in conditionally compiled code
    pub fn get_next_atlas_scientific_signal(&self) -> (AquariumSignal, bool) {
        match self {
            AquariumSignal::WaterTemperature => (AquariumSignal::pH, false),
            AquariumSignal::pH => (AquariumSignal::Conductivity, false),
            AquariumSignal::Conductivity => (AquariumSignal::WaterTemperature, true),
            _ => (AquariumSignal::WaterTemperature, false), // default value
        }
    }
}

/// Enum is used for internal thread messaging.
/// Note: Relay state and device state do not necessarily correlate.
#[derive(Clone, Debug, PartialEq, Display)]
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

    /// Terminate - second step to terminating the application
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
    /// Default value used when unknown command is received via IPC-message
    Unknown,
}

#[cfg(target_os = "linux")]
impl InternalCommand {
    /// Converts contents of a `Message` into an `InternalCommand`.
    pub fn from_msg(msg: &Message) -> Self {
        match msg.command {
            command_values::CMD_RESET => InternalCommand::ResetAllErrors,
            command_values::CMD_STOP => InternalCommand::Stop,
            command_values::CMD_START => InternalCommand::Start,
            // The `Execute` variant carries data, so it's handled distinctly.
            command_values::CMD_EXECUTE => InternalCommand::Execute(msg.command_param1),
            _ => InternalCommand::Unknown,
        }
    }

    #[cfg(test)]
    pub fn to_numeric(&self) -> i32 {
        match self {
            InternalCommand::ResetAllErrors => Self::CMD_RESET_ALL_ERRORS,
            InternalCommand::Stop => Self::CMD_STOP,
            InternalCommand::Start => Self::CMD_START,
            InternalCommand::Execute(_id) => Self::CMD_EXECUTE,
            _ => -1,
        }
    }
}

/// Enum is used for representing the relay states.
#[derive(PartialEq, Debug)]
pub enum ActuatorState {
    /// The actuator is on.
    On,

    /// The actuator is off.
    Off,

    /// The application has not yet sent a command to the actuator.
    Undefined,
}
