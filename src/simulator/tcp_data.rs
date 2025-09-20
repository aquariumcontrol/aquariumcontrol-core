/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the constants for the text-based TCP simulator protocol.
//!
//! This module centralizes all the "magic strings" that make up the custom,
//! text-based protocol used to communicate with the external hardware simulator.
//! By consolidating these constants into distinct submodules (`command`, `category`,
//! `signal`), it provides a single source of truth for the protocol, preventing
//! typos and improving code readability and maintainability.

/// Contains constants representing commands received from the control application.
#[allow(dead_code)]
pub mod command {
    /// command to switch on a relay (not necessarily switching on a device)
    pub const SETSINGLE: &str = "SETSINGLE";

    /// command to switch off a relay (not necessarily switching off a device)
    pub const UNSETSINGLE: &str = "UNSETSINGLE";

    /// command to retrieve the status of Controllino (currently not used)
    pub const GETSTATUS: &str = "GETSTATUS";

    /// command to switch on and off a relay for a defined, short time
    pub const PULSE: &str = "PULSE";

    /// command to send a live signal to the Controllino
    pub const HEARTBEAT: &str = "HEARTBEAT";

    /// command to retrieve the status of a certain relay (currently not used)
    pub const READ: &str = "READ";

    /// command to indicate that the main application has finished initialization (currently not used)    
    pub const INITIALIZED: &str = "INITIALIZED";
}

/// Contains constants representing the category received from the control application.
/// The category describes the device to which the command shall be applied.
#[allow(dead_code)]
pub mod category {
    /// Controllino is the relay actuator.
    pub const CONTROLLINO: &str = "CONTROLLINO";

    /// DHT22 is the sensor for measuring ambient temperature and humidity.
    pub const DHT22: &str = "DHT22";

    /// AtlasScientific is the group of sensors which includes water temperature, pH and conductivity.
    pub const ATLASSCIENTIFIC: &str = "ATLSSCNTFC";

    /// The tank level switch is measured via GPIO.
    pub const TANKLEVELSWITCH: &str = "TNKLVLSWTCH";

    /// This category addresses the refill control - it is used to reset errors from the outside.
    pub const REFILLCONTROL: &str = "REFILLCTRL";
}

/// Contains constants representing the signal request received from the control application.
#[allow(dead_code)]
pub mod signal {
    #[allow(dead_code)]
    /// ambient air temperature in °C measured by DHT22
    pub const AMBIENTTEMPERATURE: &str = "AmbTemp";

    /// ambient humidity in % measured by DHT22
    pub const AMBIENTHUMIDITY: &str = "AmbHum";

    /// tank level switch position
    pub const TANKLEVELSWITCHPOSITION: &str = "TnkLvlSwtch";

    /// tank level switch invalid    
    pub const TANKLEVELSWITCHINVALID: &str = "TnkLvlSwtchInvld";

    /// water temperature in °C
    pub const ATLASSCIENTIFICTEMPERATURE: &str = "temperature";

    /// pH value
    pub const ATLASSCIENTIFICPH: &str = "pH";

    /// conductivity in uS/cm
    pub const ATLASSCIENTIFICCONDUCTIVITY: &str = "conductivity";
}
