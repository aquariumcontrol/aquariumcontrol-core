/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines a custom error type for validating the configuration file path.
//!
//! This module provides the `ConfigFileDefinitionError` enum, which represents all
//! possible validation failures related to the configuration file's path and name,
//! *before* any attempt is made to read or parse its contents.
//!
//! ## Purpose and Design
//!
//! This error type is used during the initial application startup phase to ensure
//! that the provided configuration file path is valid and adheres to expected
//! conventions (i.e., it exists, is a file, and has a `.toml` extension).
//!
//! By separating these path-related validation errors from the content-related
//! errors found in `ConfigError` (like I/O or parsing failures), the application
//! can provide more precise and actionable feedback to the user if the initial
//! setup is incorrect.
//!
//! The implementation leverages the `thiserror` crate to automatically generate
//! the necessary `std::error::Error` and `std::fmt::Display` implementations,
//! resulting in clean, declarative, and maintainable code.
//!
//! ## Enum Variants
//!
//! - **`PathDoesNotExist`**: The specified file path does not correspond to any
//!   existing entry on the filesystem.
//! - **`IsNotAFile`**: The path exists, but it points to a directory or another
//!   non-file entity.
//! - **`FileNameHasNoSuffix`**: The filename at the given path does not have a
//!   file extension.
//! - **`SuffixIsNotToml`**: The filename has an extension, but it is not `.toml`.

use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum ConfigFileDefinitionError {
    /// The configuration file is not an existing file system entry.
    #[error("The configuration file ({0}) is not an existing file system entry.")]
    PathDoesNotExist(String),

    /// The provided argument for the configuration file is not pointing to a file.
    #[error("The provided argument for the configuration file ({0}) is not pointing to a file.")]
    IsNotAFile(String),

    /// The provided argument for the configuration file does not have any extension.
    #[error("The provided argument for the configuration file ({0}) does not have any extension.")]
    FileNameHasNoSuffix(String),

    /// The provided argument for the configuration file does not have extension '.toml'.
    #[error(
        "The provided argument for the configuration file ({0}) does not have extension '.toml'."
    )]
    SuffixIsNotToml(String),
}
