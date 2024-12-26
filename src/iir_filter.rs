/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;

const LOG_TARGET_IIR_FILTER: &str = "iir_filter";

trait FilterDefault {
    fn filter_default() -> Self;
}

impl FilterDefault for () {
    fn filter_default() -> Self {}
}

impl FilterDefault for f32 {
    fn filter_default() -> Self {
        panic!(
            "{}: initial value was required for filter",
            LOG_TARGET_IIR_FILTER
        )
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
    #[allow(unused)]
    pub initial_value: T,
}

/// Contains the configuration data for a filter and implements calculation of an infinite impulse response (IIR) filter.
/// The filtering works with the formula: New filtered value = (1 - filter coefficient) * Old filtered value + filter coefficient * new unfiltered value
pub struct IIRFilter {
    filter_coefficient: f32,
    old_value: f32,
}

impl IIRFilter {
    /// Returns a struct of type IIRFilter where the initial value is stated as parameter
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
                "{}: filter initialized with invalid time step value ({}).",
                LOG_TARGET_IIR_FILTER, time_step
            );
        }
        Self {
            filter_coefficient: filter_config.filter_coefficient * 1.0 / time_step,
            old_value: initial_value,
        }
    }

    /// Returns the filtered value.
    /// # Arguments:
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
