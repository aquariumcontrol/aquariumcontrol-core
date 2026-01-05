/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the custom error type for the `DataLogger` module.
//!
//! This module provides the `DataLoggerError` enum, which encapsulates all possible
//! failures that can occur during the initialization or operation of the `DataLogger`.
//! By using a dedicated error type, the application can handle these specific issues
//! gracefully and provide clear, contextual feedback.
//!
//! ## Design and Implementation
//!
//! - **`thiserror` Crate**: This enum uses the `#[derive(Error)]` macro from the
//!   `thiserror` crate. This powerful macro automatically generates the necessary
//!   implementations for `std::error::Error` and `std::fmt::Display`, significantly
//!   reducing boilerplate code and ensuring consistent error message formatting.
//!
//! - **Error Chaining**: The `FileResetFailure` variant uses the `#[source]` attribute
//!   to capture the underlying `std::io::Error`. This preserves the original error
//!   context, which is invaluable for debugging I/O issues.
//!
//! - **Specific Variants**: Each enum variant represents a distinct,
//!   failure mode, making error handling logic in the calling code more precise.
//!
//! ## Enum Variants
//!
//! - **`OutputFilenameEmpty`**: Occurs during initialization if `output_to_disk` is
//!   enabled, but one of the required output filenames is an empty string.
//! - **`FileResetFailure`**: Occurs when an I/O error is encountered while trying to
//!   create or truncate an output file.
//! - **`InvalidFilterCoefficient`**: Occurs during initialization if a configured
//!   IIR filter coefficient is outside the valid range of `(0.0, 1.0]`.

use thiserror::Error;

/// Contains the error definition for DataLogger
#[derive(Error, Debug)]
pub enum DataLoggerError {
    /// Empty output file name
    #[error("[{0}] One of the output file names for the data logging to filesystem is empty.")]
    OutputFilenameEmpty(String),

    /// Failed to reset the content of an output file
    #[error("[{location}] Could not reset content of output file for {signal_name} ({output_file_name})")]
    FileResetFailure {
        location: String,
        signal_name: String,
        output_file_name: String,

        #[source]
        source: std::io::Error,
    },

    /// Invalid filter coefficient
    #[error("[{0}] Invalid filter coefficient for {1}: {2} (Valid values are > 0.0 and < = 1.0)")]
    InvalidFilterCoefficient(String, String, f32),
}
