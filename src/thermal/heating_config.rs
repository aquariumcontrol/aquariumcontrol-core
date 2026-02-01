/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the `Heating` control thread.
//!
//! This module contains the `HeatingConfig` struct, which is used with `serde`
//! to deserialize the `[heating]` section from the application's TOML
//! configuration file. It holds all the parameters that govern the behavior of the
//! heating system, from basic temperature setpoints to advanced safety interlocks
//! and control strategies.

use crate::utilities::sawtooth_profile::SawToothProfileConfig;
use serde_derive::Deserialize;

/// Holds the configuration data for the heating control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Clone)]
pub struct HeatingConfig {
    /// Control will only be active when the flag is set to true.
    pub active: bool,

    /// indicates if the thread shall be started or not
    pub execute: bool,

    /// The temperature at which the heater is switched on for 100% of the time.
    /// This temperature must be lower than switch_off_temperature.
    /// It is overwritten with valid database values once obtained.
    pub switch_on_temperature: f32,

    /// The temperature at which the heater is switched off for 100% of the time.
    /// This temperature must be higher than switch_on_temperature.
    /// It is overwritten with valid database values once obtained.
    pub switch_off_temperature: f32,

    /// Flag to determine behavior in case the refill control has a problem
    /// and the water level stays low for longer periods of time.
    /// If this flag is true, then the heating control will switch off the heater
    /// regardless of measured temperature to avoid overheating
    /// of the fish tank or the heater itself.
    /// Background: If the water level is too low, the temperature sensor may be exposed to ambient air instead of water.
    /// This condition results in a wrong temperature reading and thus in the wrong actuation of the heater.
    /// Furthermore, exposing the heater to air instead of water and switching it on
    /// will not allow the heater to dissipate the heat, and it therefore might suffer damage.
    pub stop_on_refill_error: bool,

    /// Number of seconds which need to pass with low water level before the heater is switched off
    pub stop_on_refill_error_delay: u32,

    /// Configuration of saw tooth profile for generation of low-frequency PWM signal
    pub saw_tooth_profile_config: SawToothProfileConfig,

    /// Electrical power consumption of heater in Watt
    pub heater_power: f32,

    /// strategy when schedule check does not allow operation:
    /// true=heating will be switched on; false=heating will be switched off
    pub switch_on_when_out_of_schedule: bool,

    /// strategy when the external request for stopping heating control has been received:
    /// true=heating will be switched on; false=heating will be switched off
    pub switch_on_when_external_stop: bool,

    /// interval in seconds for polling the database
    pub set_value_check_interval: u64,

    /// Permitted duration in seconds until the gradient must have the expected sign given that the actuator is switched on permanently.
    pub max_time_actuator_effect_monitor: u64,

    /// If true, then ventilation is switched off when the gradient monitor indicates that the actuator has no effect
    pub switch_off_when_no_effect: bool,
}
