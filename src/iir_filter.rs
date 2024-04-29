use serde_derive::Deserialize;

trait FilterDefault {
    fn filter_default() -> Self;
}

impl FilterDefault for () {
    fn filter_default() -> Self {}
}

impl FilterDefault for f32 {
    fn filter_default() -> Self {
        panic!("initial value was required for filter")
    }
}

/// Configuration for a filter
#[allow(private_bounds)]
#[derive(Deserialize)]
pub struct IIRFilterConfig<T: FilterDefault> {
    /// filter coefficient: value between 1 (no filtering) and 0 (strongest filter, value remains same as initial value)
    pub filter_coefficient: f32,

    /// value which is used to initialize the filter
    #[serde(default = "T::filter_default")]
    pub initial_value: T,
}

/// The struct contains the configuration data for a filter and implements calculation of an infinite impulse response (IIR) filter.
/// The filtering works with the formula: New filtered value = (1 - filter coefficient) * Old filtered value + filter coefficient * new unfiltered value
pub struct IIRFilter {
    filter_coefficient: f32,
    old_value: f32,
}

impl IIRFilter {
    /// returns a variable of type IIRFilter where the initial value is stated as parameter
    ///
    /// # Arguments:
    ///
    /// * `filter_config` - reference to configuration of filter
    /// * `initial_value` - value with which the filter is initialized
    /// * `time_step` - delta time used in simulation: if faster than real time, then this value is used to correct the filter factor
    pub fn new_requires_initial(
        filter_config: &IIRFilterConfig<()>,
        initial_value: f32,
        time_step: f32,
    ) -> Self {
        if time_step == 0.0 {
            panic!(
                "IIRFilter initialized with invalid time step value ({}).",
                time_step
            );
        }
        Self {
            filter_coefficient: filter_config.filter_coefficient * 1.0 / time_step,
            old_value: initial_value,
        }
    }

    /*
    /// returns a variable of type IIRFilter where the initial value is contained in the filter configuration
    ///
    /// # Arguments:
    ///
    /// * `filter_config` - reference to configuration of filter
    /// * `time_step` - delta time used in simulation: if faster than real time, then this value is used to correct the filter factor
    pub fn new_initial_supplied(filter_config: &IIRFilterConfig<f32>, time_step: f32) -> Self {
        Self {
            filter_coefficient: filter_config.filter_coefficient * 1.0 / time_step,
            old_value: filter_config.initial_value,
        }
    }
    */

    /// returns the filtered value
    ///
    /// # Arguments:
    ///
    /// * `new_value` - unfiltered input value
    pub fn get_filtered_value(&mut self, new_value: f32) -> f32 {
        self.old_value =
            self.old_value * (1.0 - self.filter_coefficient) + new_value * self.filter_coefficient;
        self.old_value // new value becomes old value for next iteration
    }
}

#[cfg(test)]
pub mod tests {
    use crate::iir_filter::IIRFilter;
    use assert_float_eq::*;

    #[test]
    fn test_zero_to_one_with_coefficient_0_5() {
        let mut filter = IIRFilter {
            filter_coefficient: 0.5,
            old_value: 0.0,
        };
        assert_float_absolute_eq!(filter.get_filtered_value(1.0), 0.5, 0.001);
        assert_float_absolute_eq!(filter.get_filtered_value(1.0), 0.75, 0.001);
        assert_float_absolute_eq!(filter.get_filtered_value(1.0), 0.875, 0.001);
        assert_float_absolute_eq!(filter.get_filtered_value(1.0), 0.9375, 0.001);
    }
}
