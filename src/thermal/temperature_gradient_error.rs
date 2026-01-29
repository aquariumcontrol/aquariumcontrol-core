/* Copyright 2026 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the custom error types for the temperature gradient calculation.
//!
//! This module contains the `TemperatureGradientError` enum, which consolidates all potential
//! failures that can occur during the initialization and execution of the `TemperatureGradient`
//! calculation. By using a dedicated, descriptive error type, the system can
//! provide clear diagnostics when the dosing logic fails.
//!
//! ## Design and Purpose
//!
//! The `TemperatureGradientError` enum is designed using the `thiserror` crate to provide
//! structured and informative error messages.
//!
//! - **Rich Context**: Each error variant is designed to include important context,
//!   such as the code `location` and the specific parameter values that are involved
//!   when the error occurred.
//!

use thiserror::Error;

/// Contains error definitions for TemperatureGradient
#[derive(Debug, Error)]
pub enum TemperatureGradientError {
    /// Sampling interval is not valid.
    #[error("[{0}] Invalid sampling interval ({1}): a value > 0 required (indicating the interval in seconds).")]
    InvalidSamplingInterval(String, u64),

    /// Sample size is not valid.
    #[error("[{0}] Invalid sample size ({1}): a value >= 2 <= 100 required (indicating the number of samples).")]
    InvalidSampleSize(String, usize),
}
