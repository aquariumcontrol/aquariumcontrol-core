/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT of OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Provides a trait for monitoring the effect of thermal actuators on the temperature gradient.

use log::error;
#[cfg(not(test))] // only required in real-life execution
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
        #[cfg(not(test))] component_name: &str, // only required in real-life execution
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
                                #[cfg(not(test))] // only required in real-life execution
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    struct MockMonitor {
        max_time: u64,
        last_actuator_on_time: Option<Instant>,
        log_inhibition: bool,
    }

    impl MockMonitor {
        fn new(max_time: u64) -> Self {
            Self {
                max_time,
                last_actuator_on_time: None,
                log_inhibition: false,
            }
        }
    }

    impl TemperatureGradientMonitorTrait for MockMonitor {
        fn max_time_actuator_effect_monitor(&self) -> u64 {
            self.max_time
        }

        fn monitor_state(&mut self) -> (&mut Option<Instant>, &mut bool) {
            (&mut self.last_actuator_on_time, &mut self.log_inhibition)
        }
    }

    #[test]
    fn test_monitor_deactivated() {
        let mut monitor = MockMonitor::new(0);
        let gradient = Arc::new(Mutex::new(-1.0)); // Incorrect gradient

        // Simulate actuator on for a long time
        monitor.last_actuator_on_time = Some(Instant::now() - Duration::from_secs(100));

        let result = monitor.temperature_gradient_monitor(
            &gradient,
            true,
            true,
            #[cfg(not(test))] // only required in real-life execution
            "TestComponent",
        );

        assert!(!result, "Should return false when max_time is 0");
        assert!(
            !monitor.log_inhibition,
            "Should not inhibit/log when max_time is 0"
        );
    }

    #[test]
    fn test_actuator_off_resets_state() {
        let mut monitor = MockMonitor::new(10);
        monitor.last_actuator_on_time = Some(Instant::now());
        monitor.log_inhibition = true;
        let gradient = Arc::new(Mutex::new(1.0));

        let result = monitor.temperature_gradient_monitor(
            &gradient,
            false,
            true,
            #[cfg(not(test))] // only required in real-life execution
            "TestComponent",
        );

        assert!(!result, "Should return false when actuator is off");
        assert!(
            monitor.last_actuator_on_time.is_none(),
            "Should reset start time"
        );
        assert!(!monitor.log_inhibition, "Should reset inhibition");
    }

    #[test]
    fn test_duration_short_no_action() {
        let mut monitor = MockMonitor::new(10);
        let gradient = Arc::new(Mutex::new(-1.0)); // Incorrect gradient

        // Simulate actuator on for 5 seconds (less than 10)
        monitor.last_actuator_on_time = Some(Instant::now() - Duration::from_secs(5));

        let result = monitor.temperature_gradient_monitor(
            &gradient,
            true,
            true,
            #[cfg(not(test))] // only required in real-life execution
            "TestComponent",
        );

        assert!(!result, "Should return false if duration < max_time");
        assert!(
            !monitor.log_inhibition,
            "Should not log if duration < max_time"
        );
    }

    #[test]
    fn test_gradient_correct_positive() {
        let mut monitor = MockMonitor::new(10);
        let gradient = Arc::new(Mutex::new(0.5)); // Correct positive gradient

        // Simulate actuator on for 15 seconds
        monitor.last_actuator_on_time = Some(Instant::now() - Duration::from_secs(15));

        let result = monitor.temperature_gradient_monitor(
            &gradient,
            true,
            true,
            #[cfg(not(test))] // only required in real-life execution
            "TestComponent",
        );

        assert!(!result, "Should return false if gradient is correct");
        assert!(
            !monitor.log_inhibition,
            "Should not log if gradient is correct"
        );
    }

    #[test]
    fn test_gradient_correct_negative() {
        let mut monitor = MockMonitor::new(10);
        let gradient = Arc::new(Mutex::new(-0.5)); // Correct negative gradient

        // Simulate actuator on for 15 seconds
        monitor.last_actuator_on_time = Some(Instant::now() - Duration::from_secs(15));

        let result = monitor.temperature_gradient_monitor(
            &gradient,
            true,
            false,
            #[cfg(not(test))] // only required in real-life execution
            "TestComponent",
        );

        assert!(!result, "Should return false if gradient is correct");
        assert!(
            !monitor.log_inhibition,
            "Should not log if gradient is correct"
        );
    }

    #[test]
    fn test_gradient_incorrect_positive_expected() {
        let mut monitor = MockMonitor::new(10);
        let gradient = Arc::new(Mutex::new(-0.5)); // Incorrect (negative instead of positive)

        // Simulate actuator on for 15 seconds
        monitor.last_actuator_on_time = Some(Instant::now() - Duration::from_secs(15));

        let result = monitor.temperature_gradient_monitor(
            &gradient,
            true,
            true,
            #[cfg(not(test))] // only required in real-life execution
            "TestComponent",
        );

        assert!(result, "Should return true when gradient is incorrect");
        assert!(
            monitor.log_inhibition,
            "Should set inhibition (log warning) when gradient incorrect"
        );
    }

    #[test]
    fn test_gradient_incorrect_negative_expected() {
        let mut monitor = MockMonitor::new(10);
        let gradient = Arc::new(Mutex::new(0.5)); // Incorrect (positive instead of negative)

        // Simulate actuator on for 15 seconds
        monitor.last_actuator_on_time = Some(Instant::now() - Duration::from_secs(15));

        let result = monitor.temperature_gradient_monitor(
            &gradient,
            true,
            false,
            #[cfg(not(test))] // only required in real-life execution
            "TestComponent",
        );

        assert!(result, "Should return true when gradient is incorrect");
        assert!(
            monitor.log_inhibition,
            "Should set inhibition (log warning) when gradient incorrect"
        );
    }

    #[test]
    fn test_recovery_resets_inhibition() {
        let mut monitor = MockMonitor::new(10);
        let gradient = Arc::new(Mutex::new(0.5)); // Now correct

        // Simulate actuator on for 15 seconds, and previously inhibited
        monitor.last_actuator_on_time = Some(Instant::now() - Duration::from_secs(15));
        monitor.log_inhibition = true;

        let result = monitor.temperature_gradient_monitor(
            &gradient,
            true,
            true,
            #[cfg(not(test))] // only required in real-life execution
            "TestComponent",
        );

        assert!(!result, "Should return false when gradient becomes correct");
        assert!(
            !monitor.log_inhibition,
            "Should reset inhibition when gradient becomes correct"
        );
    }

    #[test]
    fn test_initial_start_time_set() {
        let mut monitor = MockMonitor::new(10);
        let gradient = Arc::new(Mutex::new(0.0));

        assert!(monitor.last_actuator_on_time.is_none());

        let result = monitor.temperature_gradient_monitor(
            &gradient,
            true,
            true,
            #[cfg(not(test))] // only required in real-life execution
            "TestComponent",
        );

        assert!(!result, "Should return false on first call (duration ~0)");
        assert!(
            monitor.last_actuator_on_time.is_some(),
            "Should set start time on first call"
        );
        assert!(!monitor.log_inhibition);
    }
}
