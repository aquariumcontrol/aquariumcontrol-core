/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::utilities::sawtooth_profile::SawToothProfileConfig;
use serde_derive::Deserialize;

/// Holds the configuration data for the ventilation control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct VentilationConfig {
    /// Control will only be active when the flag is set to true.
    pub active: bool,

    /// indicates if the thread shall be started or not
    pub execute: bool,

    /// The temperature at which the ventilation is switched on for 100% of the time.
    /// This temperature must be higher than switch_off_temperature.
    /// It is overwritten with valid database values once obtained.
    pub switch_on_temperature: f32,

    /// The temperature at which the ventilation is switched off for 100% of the time.
    /// This temperature must be lower than switch_on_temperature.
    /// It is overwritten with valid database values once obtained.
    pub switch_off_temperature: f32,

    /// A flag to determine the behavior of the control when the application is terminating
    /// If this flag is true, the application will switch on the ventilation when exiting.
    pub switch_on_when_terminating: bool,

    /// The configuration of saw tooth profile for generation of low-frequency PWM signal
    pub saw_tooth_profile_config: SawToothProfileConfig,

    /// The strategy when schedule check does not allow operation:
    /// true=ventilation will be switched on; false=ventilation will be switched off
    pub switch_on_when_out_of_schedule: bool,

    /// The strategy when an external request for stopping ventilation control has been received:
    /// true=ventilation will be switched on; false=ventilation will be switched off
    pub switch_on_when_external_stop: bool,

    /// interval in seconds for polling the database
    pub set_value_check_interval: u64,
}
