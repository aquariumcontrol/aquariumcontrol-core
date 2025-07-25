use crate::thermal::heating::HeatingConfig;
use crate::thermal::ventilation::VentilationConfig;

/// The function checks if the control parameters of heating and ventilation do not conflict each other.
/// This shall avoid the scenario that heating and ventilation are working against each other,
/// leading to excessive consumption of energy and fresh water and potentially triggering
/// deactivation of the refill control.
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
