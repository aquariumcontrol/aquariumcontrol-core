/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Provides a validation function to ensure thermal control intervals do not conflict.
//!
//! This module contains a single, critical validation function,
//! `distinct_thermal_control_interval_check`. Its primary purpose is to be called at
//! application startup to verify the `[heating]` and `[ventilation]` sections of the
//! TOML configuration file.
//!
//! The check prevents a dangerous and inefficient misconfiguration where the heating
//! and cooling systems could be active simultaneously, actively working against each
//! other. This scenario is highly undesirable as it wastes energy and can cause
//! other system issues, such as excessive water evaporation leading to refill problems.
//!
//! ## Logic and Validation
//!
//! The function ensures that there is a "dead band" or a non-interfering gap
//! between the heating and ventilation temperature ranges. Specifically, it verifies
//! that the temperature at which the heater switches **off** is strictly lower than the
//! temperature at which the ventilation fans switch **off**.

use crate::thermal::heating_config::HeatingConfig;
use crate::thermal::ventilation_config::VentilationConfig;

/// The function checks if the control parameters of heating and ventilation do not conflict each other.
/// # Arguments
///
/// * `heating_config` - A reference to the `HeatingConfig` struct containing configuration
///   data of the heating control.
/// * `ventilation_config` - A reference to the `VentilationConfig` struct containing configuration
///   data of the ventilation control.
///
/// # Returns
///
/// - `bool`: If the control intervals are distinct or one of the controls is deactivated, the
///   function returns true, otherwise false.
pub fn distinct_thermal_control_interval_check(
    heating_config: &HeatingConfig,
    ventilation_config: &VentilationConfig,
) -> bool {
    if heating_config.active && ventilation_config.active {
        heating_config.switch_off_temperature < ventilation_config.switch_off_temperature
    } else {
        true // one of the controllers is deactivated
    }
}
