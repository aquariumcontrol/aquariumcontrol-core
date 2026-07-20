/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT of OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Provides a trait for monitoring the effect of thermal actuators on the temperature gradient.

use log::error;
#[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
use log::info;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// A trait to monitor the effect of a thermal actuator on the water temperature gradient.
pub trait TemperatureGradientMonitorTrait {
    /// Returns the configured maximum time in seconds for the actuator effect monitor.
    fn max_time_actuator_effect_monitor(&self) -> u64;

    /// Returns mutable references to the state variables for monitoring.
    fn monitor_state(&mut self) -> (&mut Option<Instant>, &mut bool);

    /// Monitors the temperature gradient and logs a message if it does not have the expected sign
    /// after a specified duration of actuator activity.
    ///
    /// # Arguments
    /// * `mutex_temperature_gradient` - A mutex-protected `f64` representing the current temperature gradient.
    /// * `actuator_status` - A boolean indicating if the actuator is currently active (`true`) or not (`false`).
    /// * `expected_gradient_is_positive` - `true` if a positive gradient is expected, `false` for a negative gradient.
    /// * `component_name` - The name of the component being monitored (e.g., "Heating", "Ventilation").
    ///
    /// # Returns
    /// * `true` if the gradient is incorrect after the specified duration (i.e., a warning condition).
    /// * `false` if the gradient is correct, the duration hasn't been exceeded, or the monitor is inactive.
    fn temperature_gradient_monitor(
        &mut self,
        mutex_temperature_gradient: &Arc<Mutex<f64>>,
        actuator_status: bool,
        expected_gradient_is_positive: bool,
        #[cfg(all(not(test), not(feature = "test-utils")))]
        component_name: &str, // only required in real-life execution
    ) -> bool {
        let max_time = self.max_time_actuator_effect_monitor();
        if max_time == 0 {
            return false; // Monitor is deactivated
        }

        let (last_actuator_on_time, log_inhibition) = self.monitor_state();

        if actuator_status {
            let now = Instant::now();
            let start_time = last_actuator_on_time.get_or_insert(now);

            let duration_on = now.duration_since(*start_time).as_secs();

            if duration_on > max_time {
                return match mutex_temperature_gradient.lock() {
                    Ok(gradient_guard) => {
                        let gradient = *gradient_guard;
                        let gradient_sign_correct = (expected_gradient_is_positive
                            && gradient > 0.0)
                            || (!expected_gradient_is_positive && gradient < 0.0);

                        if !gradient_sign_correct {
                            if !*log_inhibition {
                                #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                                #[cfg(not(tarpaulin_include))]
                                info!(
                                    target: module_path!(),
                                    "{} has been active for {}s, but the temperature gradient ({:.4}) does not have the expected {} sign.",
                                    component_name, duration_on, gradient, if expected_gradient_is_positive { "positive" } else { "negative" }
                                );
                                *log_inhibition = true;
                            }
                            true // Gradient is incorrect
                        } else {
                            // Gradient is correct, reset inhibition
                            *log_inhibition = false;
                            false
                        }
                    }
                    Err(e) => {
                        error!(target: module_path!(), "Failed to lock temperature gradient mutex: {}", e);
                        false // Treat lock failure as "no error detected" to avoid spamming
                    }
                };
            }
        } else {
            // Actuator is off, reset state
            *last_actuator_on_time = None;
            *log_inhibition = false;
        }
        false
    }
}
