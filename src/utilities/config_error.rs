/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the custom error type for the application's configuration loading process.
//!
//! This module provides a centralized `ConfigError` enum, which represents all possible
//! failures that can occur when reading and parsing the TOML configuration file. By
//! using a dedicated error type, the application can handle configuration issues
//! gracefully and provide clear, contextual error messages.
//!
//! ## Design and Implementation
//!
//! - **`thiserror` Crate**: This enum uses the `#[derive(Error)]` macro from the
//!   `thiserror` crate. This powerful macro automatically generates the necessary
//!   implementations for `std::error::Error` and `std::fmt::Display`, significantly
//!   reducing boilerplate code.
//!
//! - **Error Chaining**: The `#[source]` attribute is used on the `FileRead` and
//!   `TomlParse` variants. This captures the underlying error (e.g., from `std::io`
//!   or `toml`) and makes it available for deeper inspection, which is a best
//!   practice for creating robust error types.
//!
//! - **Conditional Compilation**: The module uses `#[cfg(test)]` and
//!   `#[cfg(not(test))]` attributes to provide different error variants for
//!   production and test builds.
//!   - In **production**, it defines errors for file I/O and TOML parsing,
//!     allowing the main application to catch these and exit gracefully.
//!   - In **tests**, these variants are compiled out. Instead, a `CargoManifestDir`
//!     error is provided for issues with the test environment setup. File I/O and
//!     parsing failures in tests are designed to cause a `panic!`, ensuring that a
//!     misconfigured test environment fails immediately and loudly.
//!
//! ## Enum Variants
//!
//! - **`FileRead(String, std::io::Error)`**: (Production only) Occurs when the
//!   configuration file cannot be read from the filesystem.
//! - **`TomlParse(String, toml::de::Error)`**: (Production only) Occurs when the
//!   contents of the configuration file are not valid TOML.
//! - **`CargoManifestDir`**: (Test only) Occurs when the `CARGO_MANIFEST_DIR`
//!   environment variable cannot be read, which is essential for locating test
//!   configuration files.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[cfg(not(test))] // only required in real-life execution
    #[error("Could not read configuration file '{0}'")]
    FileRead(String, #[source] std::io::Error),

    #[cfg(not(test))] // only required in real-life execution
    #[error("Failed to parse TOML in configuration file '{0}'")]
    TomlParse(String, #[source] toml::de::Error),

    #[cfg(test)]
    #[error("Could not read environment variable CARGO_MANIFEST_DIR.")]
    CargoManifestDir,
}
