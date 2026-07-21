/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Module for calculating the temperature gradient of the aquarium water.
//!
//! This module provides the `TemperatureGradient` struct, which is responsible for
//! periodically sampling the water temperature and calculating its rate of change (gradient)
//! over a sliding window of time.
//!
//! The gradient is calculated using a least-squares linear regression method to smooth out
//! noise and provide a reliable trend indicator. This is useful for detecting rapid temperature
//! changes or verifying the effectiveness of heating/cooling systems.
//!
//! # Key Features
//! - **Periodic Sampling**: Samples temperature at a configurable interval.
//! - **Sliding Window**: Maintains a fixed-size buffer of samples (`sample_size`).
//! - **Least Squares Regression**: Calculates the slope of the temperature vs. time curve.
//! - **Thread Safety**: Uses `Arc<Mutex<...>>` for safe concurrent access to sensor data and results.
//! - **Graceful Shutdown**: Responds to external quit commands.

use crate::sensors::sensor_manager::SensorManagerSignals;
use crate::thermal::temperature_gradient_channels::TemperatureGradientChannels;
use crate::thermal::temperature_gradient_config::TemperatureGradientConfig;
use crate::thermal::temperature_gradient_error::TemperatureGradientError;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use log::error;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CYCLE_TIME_TEMPERATURE_GRADIENT_MILLIS: u64 = 100;

/// Manages the calculation of the water temperature gradient.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     signal_handler[SignalHandler] --> temperature_gradient[TemperatureGradient]
///     temperature_gradient[TemperatureGradient] --> signal_handler[SignalHandler]
/// ```
#[derive(Debug)]
pub struct TemperatureGradient {
    /// Configuration for the temperature gradient calculation.
    config: TemperatureGradientConfig,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to acquire lock on sensor manager mutex
    lock_error_sensor_manager_mutex: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to acquire lock on temperature gradient mutex
    lock_error_temperature_gradient_mutex: bool,
}

impl ProcessExternalRequestTrait for TemperatureGradient {}

impl TemperatureGradient {
    /// Creates a new `TemperatureGradient` instance, validating the provided configuration.
    ///
    /// # Arguments
    /// * `config` - The configuration for the temperature gradient calculation.
    ///
    /// # Returns
    /// A `Result` containing a `TemperatureGradient` instance on success, or a `StartupError`
    /// if the configuration is invalid.
    pub fn new(config: TemperatureGradientConfig) -> Result<Self, TemperatureGradientError> {
        if config.sampling_interval == 0 {
            return Err(TemperatureGradientError::InvalidSamplingInterval(
                module_path!().to_string(),
                config.sampling_interval,
            ));
        }
        if (config.sample_size < 2) || (config.sample_size > 100) {
            // At least 2 samples needed for a gradient
            return Err(TemperatureGradientError::InvalidSampleSize(
                module_path!().to_string(),
                config.sample_size,
            ));
        }

        Ok(Self {
            config,
            lock_error_sensor_manager_mutex: false,
            lock_error_temperature_gradient_mutex: false,
        })
    }

    /// The main execution loop for the temperature gradient calculation.
    ///
    /// # Arguments
    /// * `sensor_manager_signals` - `Arc<Mutex>` to read temperature data from.
    /// * `mutex_sensor_manager_signals` - Shared mutex for consolidated sensor signals.
    /// * `temperature_gradient_channels` - A parameter representing `temperature_gradient_channels`.
    /// * `mutex_temperature_gradient` - A parameter representing `mutex_temperature_gradient`.
    pub async fn execute(
        &mut self,
        mutex_sensor_manager_signals: Arc<Mutex<SensorManagerSignals>>,
        temperature_gradient_channels: &mut TemperatureGradientChannels,
        mutex_temperature_gradient: Arc<Mutex<f64>>,
    ) {
        let sampling_interval = Duration::from_secs(self.config.sampling_interval);
        let sample_size = self.config.sample_size;
        let cycle_time_duration = Duration::from_millis(CYCLE_TIME_TEMPERATURE_GRADIENT_MILLIS);
        let mut instant_last_sampling = Instant::now();

        // Storage of the historical temperature samples for gradient calculation
        let mut temperature_samples: VecDeque<(f64, f64)> =
            VecDeque::with_capacity(self.config.sample_size);

        loop {
            let (quit_command_received, _, _) = self.process_external_request(
                &mut temperature_gradient_channels.rx_temperature_gradient_from_signal_handler,
                None,
            );
            if quit_command_received {
                break;
            }

            if self.config.active {
                // check if sufficient amount of time has passed since last sampling
                if instant_last_sampling.elapsed() >= sampling_interval {
                    instant_last_sampling = Instant::now();

                    // Acquire temperature data
                    let data_retrieval_result = match mutex_sensor_manager_signals.lock() {
                        Ok(sensor_manager_signals) => {
                            // initiate gradient calculation
                            let current_temperature =
                                sensor_manager_signals.water_temperature as f64;
                            let current_timestamp = current_timestamp_as_f64();

                            temperature_samples.push_back((current_timestamp, current_temperature));

                            // Keep the deque at the specified sample_size
                            while temperature_samples.len() > sample_size {
                                temperature_samples.pop_front();
                            }
                            Ok(())
                        }
                        Err(e) => {
                            if !self.lock_error_sensor_manager_mutex {
                                self.lock_error_sensor_manager_mutex = true;
                                error!(target: module_path!(), "error when trying to lock sensor manager mutex: {e}");
                            }
                            Err(e)
                        }
                    };

                    // Check if data retrieval was successful
                    if data_retrieval_result.is_ok() {
                        // Perform calculation if enough samples are available
                        if temperature_samples.len() == sample_size {
                            let gradient = calculate_least_squares_gradient(&temperature_samples);
                            match mutex_temperature_gradient.lock() {
                                Ok(mut calculated_gradient_guard) => {
                                    *calculated_gradient_guard = gradient;
                                }
                                Err(e) => {
                                    if !self.lock_error_temperature_gradient_mutex {
                                        self.lock_error_temperature_gradient_mutex = true;
                                        error!(target: module_path!(), "error when trying to lock temperature gradient mutex: {e}");
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // wait for a short period before checking the duration again
            tokio::time::sleep(cycle_time_duration).await;
        }

        temperature_gradient_channels.acknowledge_signal_handler();
    }
}

/// Helper function to get current timestamp as f64 (seconds since epoch).
fn current_timestamp_as_f64() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

/// Least squares gradient calculation.
///
/// # Arguments
/// * `samples` - A parameter representing `samples`.
pub fn calculate_least_squares_gradient(samples: &VecDeque<(f64, f64)>) -> f64 {
    if samples.len() < 2 {
        return 0.0; // Cannot calculate gradient with less than 2 points
    }

    let x_offset = samples[0].0;

    // Simple linear regression (y = mx + b)
    // m = (N * sum(xy) - sum(x) * sum(y)) / (N * sum(x^2) - (sum(x))^2)
    let n = samples.len() as f64;
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    let mut sum_xy = 0.0;
    let mut sum_x2 = 0.0;

    for &(x_raw, y) in samples.iter() {
        let x = x_raw - x_offset;
        sum_x += x;
        sum_y += y;
        sum_xy += x * y;
        sum_x2 += x * x;
    }

    let numerator = n * sum_xy - sum_x * sum_y;
    let denominator = n * sum_x2 - sum_x * sum_x;

    if denominator == 0.0 {
        return 0.0; // Avoid division by zero (e.g., all x values are the same)
    }

    numerator / denominator
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thermal::temperature_gradient_config::TemperatureGradientConfig;

    /// Performs the `create_default_config` operation.
    fn create_default_config() -> TemperatureGradientConfig {
        TemperatureGradientConfig {
            active: true,
            execute: true,
            sampling_interval: 10,
            sample_size: 5,
        }
    }

    #[test]
    // Verifies that new invalid sampling interval.
    fn test_new_invalid_sampling_interval() {
        let mut config = create_default_config();
        config.sampling_interval = 0;

        let result = TemperatureGradient::new(config);
        assert!(result.is_err());
        match result.unwrap_err() {
            TemperatureGradientError::InvalidSamplingInterval(module, interval) => {
                assert_eq!(
                    module,
                    "aquarium_control::thermal::temperature_gradient".to_string()
                );
                assert_eq!(interval, 0);
            }
            _ => panic!("Expected InvalidSamplingInterval error"),
        }
    }

    #[test]
    // Verifies that new invalid sample size too small.
    fn test_new_invalid_sample_size_too_small() {
        let mut config = create_default_config();
        config.sample_size = 1;

        let result = TemperatureGradient::new(config);
        assert!(result.is_err());
        match result.unwrap_err() {
            TemperatureGradientError::InvalidSampleSize(module, size) => {
                assert_eq!(
                    module,
                    "aquarium_control::thermal::temperature_gradient".to_string()
                );
                assert_eq!(size, 1);
            }
            _ => panic!("Expected InvalidSampleSize error"),
        }
    }

    #[test]
    // Verifies that new invalid sample size too large.
    fn test_new_invalid_sample_size_too_large() {
        let mut config = create_default_config();
        config.sample_size = 101;

        let result = TemperatureGradient::new(config);
        assert!(result.is_err());
        match result.unwrap_err() {
            TemperatureGradientError::InvalidSampleSize(module, size) => {
                assert_eq!(
                    module,
                    "aquarium_control::thermal::temperature_gradient".to_string()
                );
                assert_eq!(size, 101);
            }
            _ => panic!("Expected InvalidSampleSize error"),
        }
    }

    #[test]
    // Verifies that new valid config.
    fn test_new_valid_config() {
        let config = create_default_config();
        let result = TemperatureGradient::new(config);
        assert!(result.is_ok());
    }

    #[test]
    // Verifies that calculate least squares gradient less than two samples.
    fn test_calculate_least_squares_gradient_less_than_two_samples() {
        let mut samples = VecDeque::new();
        samples.push_back((1.0, 10.0));
        let gradient = calculate_least_squares_gradient(&samples);
        assert_eq!(gradient, 0.0);
    }

    #[test]
    // Verifies that calculate least squares gradient zero gradient.
    fn test_calculate_least_squares_gradient_zero_gradient() {
        let mut samples = VecDeque::new();
        let now = current_timestamp_as_f64();
        samples.push_back((now, 10.0));
        samples.push_back((now + 1.0, 10.0));
        samples.push_back((now + 2.0, 10.0));
        let gradient = calculate_least_squares_gradient(&samples);
        assert_eq!(gradient, 0.0);
    }

    #[test]
    // Verifies that calculate least squares gradient positive gradient one.
    fn test_calculate_least_squares_gradient_positive_gradient_one() {
        let mut samples = VecDeque::new();
        let now = current_timestamp_as_f64();
        samples.push_back((now, 1.0));
        samples.push_back((now + 1.0, 2.0));
        samples.push_back((now + 2.0, 3.0));
        let gradient = calculate_least_squares_gradient(&samples);
        assert_eq!(gradient, 1.0);
    }

    #[test]
    // Verifies that calculate least squares gradient negative gradient one.
    fn test_calculate_least_squares_gradient_negative_gradient_one() {
        let mut samples = VecDeque::new();
        let now = current_timestamp_as_f64();
        samples.push_back((now, 3.0));
        samples.push_back((now + 1.0, 2.0));
        samples.push_back((now + 2.0, 1.0));
        let gradient = calculate_least_squares_gradient(&samples);
        assert_eq!(gradient, -1.0);
    }

    #[test]
    // Verifies that calculate least squares gradient very high positive gradient.
    fn test_calculate_least_squares_gradient_very_high_positive_gradient() {
        let mut samples = VecDeque::new();
        samples.push_back((1.0, 1.0));
        samples.push_back((1.1, 1000.0));
        let gradient = calculate_least_squares_gradient(&samples);
        // Expected gradient is (1000 - 1) / (1.1 - 1.0) = 999 / 0.1 = 9990.0
        assert!((gradient - 9990.0).abs() < 0.001);
    }

    #[test]
    // Verifies that calculate least squares gradient very high negative gradient.
    fn test_calculate_least_squares_gradient_very_high_negative_gradient() {
        let mut samples = VecDeque::new();
        samples.push_back((1.0, 1000.0));
        samples.push_back((1.1, 1.0));
        let gradient = calculate_least_squares_gradient(&samples);
        // Expected gradient is (1 - 1000) / (1.1 - 1.0) = -999 / 0.1 = -9990.0
        assert!((gradient - (-9990.0)).abs() < 0.001);
    }

    #[test]
    // Verifies that calculate least squares gradient x values are the same.
    fn test_calculate_least_squares_gradient_x_values_are_the_same() {
        let mut samples = VecDeque::new();
        samples.push_back((1.0, 10.0));
        samples.push_back((1.0, 20.0));
        samples.push_back((1.0, 30.0));
        let gradient = calculate_least_squares_gradient(&samples);
        assert_eq!(gradient, 0.0);
    }
}
