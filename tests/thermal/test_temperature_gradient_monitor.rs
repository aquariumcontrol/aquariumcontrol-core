/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests a trait for monitoring the effect of thermal actuators on the temperature gradient.

use aquarium_control::thermal::temperature_gradient_monitor::TemperatureGradientMonitorTrait;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct MockMonitor {
    max_time: u64,
    last_actuator_on_time: Option<Instant>,
    log_inhibition: bool,
}

impl MockMonitor {
    /// Creates a new instance.
    ///
    /// # Arguments
    /// * `max_time` - A parameter representing `max_time`.
    fn new(max_time: u64) -> Self {
        Self {
            max_time,
            last_actuator_on_time: None,
            log_inhibition: false,
        }
    }
}

impl TemperatureGradientMonitorTrait for MockMonitor {
    /// Performs the `max_time_actuator_effect_monitor` operation.
    fn max_time_actuator_effect_monitor(&self) -> u64 {
        self.max_time
    }

    /// Performs the `monitor_state` operation.
    fn monitor_state(&mut self) -> (&mut Option<Instant>, &mut bool) {
        (&mut self.last_actuator_on_time, &mut self.log_inhibition)
    }
}

#[test]
// Verifies that monitor deactivated.
// [utest->swreq~thermal_temperature_gradient_monitor~1]
fn test_monitor_deactivated() {
    let mut monitor = MockMonitor::new(0);
    let gradient = Arc::new(Mutex::new(-1.0)); // Incorrect gradient

    // Simulate actuator on for a long time
    monitor.last_actuator_on_time = Some(Instant::now() - Duration::from_secs(100));

    let result = monitor.temperature_gradient_monitor(&gradient, true, true, "TestComponent");

    assert!(!result, "Should return false when max_time is 0");
    assert!(
        !monitor.log_inhibition,
        "Should not inhibit/log when max_time is 0"
    );
}

#[test]
// Verifies that actuator off resets state.
// [utest->swreq~thermal_temperature_gradient_monitor~1]
fn test_actuator_off_resets_state() {
    let mut monitor = MockMonitor::new(10);
    monitor.last_actuator_on_time = Some(Instant::now());
    monitor.log_inhibition = true;
    let gradient = Arc::new(Mutex::new(1.0));

    let result = monitor.temperature_gradient_monitor(&gradient, false, true, "TestComponent");

    assert!(!result, "Should return false when actuator is off");
    assert!(
        monitor.last_actuator_on_time.is_none(),
        "Should reset start time"
    );
    assert!(!monitor.log_inhibition, "Should reset inhibition");
}

#[test]
// Verifies that duration short no action.
// [utest->swreq~thermal_temperature_gradient_monitor~1]
fn test_duration_short_no_action() {
    let mut monitor = MockMonitor::new(10);
    let gradient = Arc::new(Mutex::new(-1.0)); // Incorrect gradient

    // Simulate actuator on for 5 seconds (less than 10)
    monitor.last_actuator_on_time = Some(Instant::now() - Duration::from_secs(5));

    let result = monitor.temperature_gradient_monitor(&gradient, true, true, "TestComponent");

    assert!(!result, "Should return false if duration < max_time");
    assert!(
        !monitor.log_inhibition,
        "Should not log if duration < max_time"
    );
}

#[test]
// Verifies that gradient correct positive.
// [utest->swreq~thermal_temperature_gradient_monitor~1]
fn test_gradient_correct_positive() {
    let mut monitor = MockMonitor::new(10);
    let gradient = Arc::new(Mutex::new(0.5)); // Correct positive gradient

    // Simulate actuator on for 15 seconds
    monitor.last_actuator_on_time = Some(Instant::now() - Duration::from_secs(15));

    let result = monitor.temperature_gradient_monitor(&gradient, true, true, "TestComponent");

    assert!(!result, "Should return false if gradient is correct");
    assert!(
        !monitor.log_inhibition,
        "Should not log if gradient is correct"
    );
}

#[test]
// Verifies that gradient correct negative.
// [utest->swreq~thermal_temperature_gradient_monitor~1]
fn test_gradient_correct_negative() {
    let mut monitor = MockMonitor::new(10);
    let gradient = Arc::new(Mutex::new(-0.5)); // Correct negative gradient

    // Simulate actuator on for 15 seconds
    monitor.last_actuator_on_time = Some(Instant::now() - Duration::from_secs(15));

    let result = monitor.temperature_gradient_monitor(&gradient, true, false, "TestComponent");

    assert!(!result, "Should return false if gradient is correct");
    assert!(
        !monitor.log_inhibition,
        "Should not log if gradient is correct"
    );
}

#[test]
// Verifies that gradient incorrect positive expected.
// [utest->swreq~thermal_temperature_gradient_monitor~1]
fn test_gradient_incorrect_positive_expected() {
    let mut monitor = MockMonitor::new(10);
    let gradient = Arc::new(Mutex::new(-0.5)); // Incorrect (negative instead of positive)

    // Simulate actuator on for 15 seconds
    monitor.last_actuator_on_time = Some(Instant::now() - Duration::from_secs(15));

    let result = monitor.temperature_gradient_monitor(&gradient, true, true, "TestComponent");

    assert!(result, "Should return true when gradient is incorrect");
    assert!(
        monitor.log_inhibition,
        "Should set inhibition (log warning) when gradient incorrect"
    );
}

#[test]
// Verifies that gradient incorrect negative expected.
// [utest->swreq~thermal_temperature_gradient_monitor~1]
fn test_gradient_incorrect_negative_expected() {
    let mut monitor = MockMonitor::new(10);
    let gradient = Arc::new(Mutex::new(0.5)); // Incorrect (positive instead of negative)

    // Simulate actuator on for 15 seconds
    monitor.last_actuator_on_time = Some(Instant::now() - Duration::from_secs(15));

    let result = monitor.temperature_gradient_monitor(&gradient, true, false, "TestComponent");

    assert!(result, "Should return true when gradient is incorrect");
    assert!(
        monitor.log_inhibition,
        "Should set inhibition (log warning) when gradient incorrect"
    );
}

#[test]
// Verifies that recovery resets inhibition.
// [utest->swreq~thermal_temperature_gradient_monitor~1]
fn test_recovery_resets_inhibition() {
    let mut monitor = MockMonitor::new(10);
    let gradient = Arc::new(Mutex::new(0.5)); // Now correct

    // Simulate actuator on for 15 seconds, and previously inhibited
    monitor.last_actuator_on_time = Some(Instant::now() - Duration::from_secs(15));
    monitor.log_inhibition = true;

    let result = monitor.temperature_gradient_monitor(&gradient, true, true, "TestComponent");

    assert!(!result, "Should return false when gradient becomes correct");
    assert!(
        !monitor.log_inhibition,
        "Should reset inhibition when gradient becomes correct"
    );
}

#[test]
// Verifies that initial start time set.
// [utest->swreq~thermal_temperature_gradient_monitor~1]
fn test_initial_start_time_set() {
    let mut monitor = MockMonitor::new(10);
    let gradient = Arc::new(Mutex::new(0.0));

    assert!(monitor.last_actuator_on_time.is_none());

    let result = monitor.temperature_gradient_monitor(&gradient, true, true, "TestComponent");

    assert!(!result, "Should return false on first call (duration ~0)");
    assert!(
        monitor.last_actuator_on_time.is_some(),
        "Should set start time on first call"
    );
    assert!(!monitor.log_inhibition);
}
