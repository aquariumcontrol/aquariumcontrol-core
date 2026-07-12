/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Provides a configurable single-pole IIR (Infinite Impulse Response) low-pass filter.
//!
//! This module is designed to smooth out noisy or fluctuating sensor readings, such as
//! temperature or pH, by applying a simple and efficient digital filter. The filter
//! reduces rapid changes in a signal, providing a more stable output value over time.
//!
//! ## Core Logic
//!
//! The filter implements a first-order IIR low-pass filter using the formula:
//!
//! `y[n] = (1 - a) * y[n-1] + a * x[n]`
//!
//! Where:
//! - `y[n]` is the new filtered value.
//! - `y[n-1]` is the previous filtered value.
//! - `x[n]` is the new, unfiltered input value.
//! - `a` is the `filter_coefficient`.
//!
//! A coefficient closer to `0.0` results in heavy filtering (a slow-responding output),
//! while a coefficient closer to `1.0` results in light filtering (a fast-responding output).
//!
//! ## Key Components
//!
//! - **`IIRFilter`**: The main runtime struct that holds the filter's state (`old_value`)
//!   and performs the filtering calculation via its `get_filtered_value()` method.
//!
//! - **`IIRFilterConfig`**: A `serde`-compatible struct for deserializing filter settings
//!   from a TOML configuration file.
//!
//! - **`new()`**: The primary constructor for `IIRFilter`. It is designed
//!   to take a static `filter_coefficient` from the config file but can use the
//!   `initial_value` to be provided dynamically at runtime. It also accepts a `time_step`
//!   parameter to normalize the filter's behavior in simulations or variable-rate
//!   sampling environments.
//!

#[cfg(not(test))] // only required in real-life execution
use log::warn;
use serde_derive::Deserialize;

/// Configuration for a filter
#[derive(Deserialize)]
pub struct IIRFilterConfig {
    /// filter coefficient: value between 1 (no filtering) and 0 (strongest filter, value remains same as initial value)
    pub filter_coefficient: f32,

    /// value which is used to initialize the filter in case no measurement value is available
    pub fallback_initial_value: f32,
}

/// Contains the configuration data for a filter and implements calculation of an infinite impulse response (IIR) filter.
/// The filtering works with the formula: New filtered value = (1 - filter coefficient) * Old filtered value + filter coefficient * new unfiltered value
pub struct IIRFilter {
    /// filter coefficient taken over from the configuration or default (safe) value
    filter_coefficient: f32,

    /// value from a previous step
    old_value: f32,
}

impl IIRFilter {
    /// Creates a new `IIRFilter` instance, requiring an explicit initial value and a time step.
    ///
    /// This constructor is used when the filter's initial state needs to be precisely
    /// defined and when the filter coefficient needs to be adjusted based on a simulation
    /// or sampling `time_step`. The `time_step` helps to normalize the `filter_coefficient`
    /// if the simulation speed differs from real-time.
    ///
    /// # Arguments
    /// * `filter_config` - A reference to the filter's static configuration, which provides
    ///   the base `filter_coefficient`.
    /// * `initial_value_opt` - The optional `f32` value used to set the filter's initial output state.
    ///
    /// # Returns
    /// A new `IIRFilter` struct, initialized with the provided values and a
    /// time-step-corrected filter coefficient.
    pub fn new(
        filter_config: &IIRFilterConfig,
        initial_value_opt: Option<f32>,
        #[cfg(not(test))] signal_name: &str, // only required in real-life execution
    ) -> Self {
        let safe_filter_coefficient = filter_config.filter_coefficient.clamp(0.0, 1.0);
        #[cfg(not(test))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        if safe_filter_coefficient != filter_config.filter_coefficient {
            warn!(
                target: module_path!(),
                "filter configuration for {} contains invalid coefficient of {} - replacing with {}",
                signal_name,
                filter_config.filter_coefficient,
                safe_filter_coefficient
            )
        }

        Self {
            filter_coefficient: safe_filter_coefficient,
            old_value: match initial_value_opt {
                Some(initial_value) => initial_value,
                None => filter_config.fallback_initial_value,
            },
        }
    }

    /// Applies the Infinite Impulse Response (IIR) filter to a new input value and returns the filtered result.
    ///
    /// This method updates the filter's internal state (`old_value`) using the IIR formula:
    /// `filtered_value = (1 - filter_coefficient) * old_filtered_value + filter_coefficient * new_unfiltered_value`.
    /// The calculated filtered value then becomes the `old_value` for the next filtering iteration.
    ///
    /// # Arguments
    /// * `new_value` - The latest unfiltered input value (`f32`) to be processed by the filter.
    ///
    /// # Returns
    /// The newly calculated, filtered `f32` value.
    pub fn get_filtered_value(&mut self, new_value: f32) -> f32 {
        self.old_value =
            self.old_value * (1.0 - self.filter_coefficient) + new_value * self.filter_coefficient;
        self.old_value // new value becomes old value for the next iteration
    }
}

#[cfg(test)]
pub mod tests {
    use assert_float_eq::*;

    use crate::utilities::iir_filter::IIRFilter;
    #[cfg(test)]
    pub mod tests {
        use crate::utilities::iir_filter::{IIRFilter, IIRFilterConfig};
        use assert_float_eq::*;

        /// Helper to create a default config for tests.
        fn prepare_filter_config() -> IIRFilterConfig {
            IIRFilterConfig {
                filter_coefficient: 0.5,
                fallback_initial_value: 100.0,
            }
        }

        #[test]
        /// Verifies the `new` constructor correctly initializes the filter's starting value.
        fn test_new_constructor_initialization() {
            let config = prepare_filter_config();
            #[cfg(not(test))] // only required in real-life execution
            #[cfg(not(tarpaulin_include))]
            let signal_name = "test_signal";

            // Scenario 1: An explicit initial value is provided via Some(10.0).
            // The filter's `old_value` should be 10.0.
            let filter_with_some = IIRFilter::new(
                &config,
                Some(10.0),
                #[cfg(not(test))] // only required in real-life execution
                signal_name,
            );
            assert_float_absolute_eq!(filter_with_some.old_value, 10.0, 0.001);

            // Scenario 2: The initial value is None.
            // The filter should use the `fallback_initial_value` from the config (100.0).
            let filter_with_none = IIRFilter::new(
                &config,
                None,
                #[cfg(not(test))] // only required in real-life execution
                signal_name,
            );
            assert_float_absolute_eq!(
                filter_with_none.old_value,
                config.fallback_initial_value,
                0.001
            );
        }

        #[test]
        /// Verifies the constructor sanitizes invalid coefficients from the config file.
        fn test_new_constructor_sanitizes_config_coefficient() {
            #[cfg(not(test))] // only required in real-life execution
            #[cfg(not(tarpaulin_include))]
            let signal_name = "test_signal";

            // Scenario 1: Coefficient is > 1.0
            let config_high = IIRFilterConfig {
                filter_coefficient: 1.5,
                fallback_initial_value: 0.0,
            };
            let filter_high = IIRFilter::new(
                &config_high,
                Some(0.0),
                #[cfg(not(test))] // only required in real-life execution
                signal_name,
            );

            // It should be clamped to 1.0
            assert_float_absolute_eq!(filter_high.filter_coefficient, 1.0, 0.001);

            // Scenario 2: Coefficient is < 0.0
            let config_low = IIRFilterConfig {
                filter_coefficient: -0.5,
                fallback_initial_value: 0.0,
            };
            let filter_low = IIRFilter::new(
                &config_low,
                Some(0.0),
                #[cfg(not(test))] // only required in real-life execution
                signal_name,
            );
            // It should be clamped to 0.0
            assert_float_absolute_eq!(filter_low.filter_coefficient, 0.0, 0.001);
        }

        #[test]
        /// Verifies filter behavior when the coefficient is 0.0 (strongest filtering).
        fn test_filter_with_coefficient_zero() {
            let mut filter = IIRFilter {
                filter_coefficient: 0.0,
                old_value: 50.0,
            };
            // With a zero coefficient, the output should never change from its initial value,
            // regardless of the new input.
            assert_float_absolute_eq!(filter.get_filtered_value(10.0), 50.0, 0.001);
            assert_float_absolute_eq!(filter.get_filtered_value(-20.0), 50.0, 0.001);
            assert_float_absolute_eq!(filter.get_filtered_value(100.0), 50.0, 0.001);
        }

        #[test]
        /// Verifies filter behavior when the coefficient is 1.0 (no filtering).
        fn test_filter_with_coefficient_one() {
            let mut filter = IIRFilter {
                filter_coefficient: 1.0,
                old_value: 50.0,
            };
            // With a coefficient of one, the output should always be exactly the new input value.
            assert_float_absolute_eq!(filter.get_filtered_value(10.0), 10.0, 0.001);
            assert_float_absolute_eq!(filter.get_filtered_value(-20.0), -20.0, 0.001);
            assert_float_absolute_eq!(filter.get_filtered_value(100.0), 100.0, 0.001);
        }
    }

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
