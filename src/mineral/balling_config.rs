/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the automated Balling mineral dosing system.
//!
//! This module contains the `BallingConfig` struct, which is used with `serde` to
//! deserialize the `[balling]` section from the application's TOML configuration file.
//! It holds all the parameters that control the behavior of the `Balling` thread,
//! from its activation status to the dosing schedule for each pump.

use serde_derive::Deserialize;

/// Holds the configuration data for the balling dosing control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct BallingConfig {
    /// control will only be active when the flag is set to true
    pub active: bool,

    /// indicates if the thread shall be started or not
    pub execute: bool,

    /// pump #1 will only be used when the flag is set to true
    pub pump1_active: bool,

    /// pump #2 will only be used when the flag is set to true
    pub pump2_active: bool,

    /// pump #3 will only be used when the flag is set to true
    pub pump3_active: bool,

    /// pump #4 will only be used when the flag is set to true
    pub pump4_active: bool,

    /// interval for the main control in seconds
    pub schedule_check_interval: u32,

    /// inject Balling minerals with pump 1 after each period    
    pub dosing_interval_pump1: u32,

    /// inject Balling minerals with pump 2 after each period    
    pub dosing_interval_pump2: u32,

    /// inject Balling minerals with pump 3 after each period    
    pub dosing_interval_pump3: u32,

    /// inject Balling minerals with pump 4 after each period    
    pub dosing_interval_pump4: u32,

    /// wait for a fixed period between dosings from different pumps
    pub pump_switch_delay_millis: u32,
}
