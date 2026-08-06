/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages the setup and configuration of the application's global logger.
//!
//! This module provides the core functionality for initializing the `fern`-based
//! logging system. It is designed to be called once at application startup to
//! configure a logger that dispatches messages to both the console (`stdout`) and
//! a persistent log file.
//!
//! ## Key Components
//!
//! - **`setup_logger()`**: The primary function responsible for logger initialization.
//!   It performs robust validation on the provided log file path before setting up
//!   the logger. This prevents the application from starting with a misconfigured
//!   logging setup. The log format includes a high-precision timestamp, log level,
//!   and the source module path.
//!
//! - **`LoggerSetupError`**: A custom, `thiserror`-based error enum that represents
//!   all possible validation failures during logger setup. By providing specific
//!   error variants (e.g., `PathIsADirectory`, `ParentDirDoesNotExist`), it allows
//!   the application to give clear, actionable feedback if the configuration is invalid.
//!
//! - **`log_error_chain()`**: A utility function that provides a standardized way to
//!   log an error and its complete chain of underlying causes. This is used for
//!   debugging, as it reveals the full context of a failure, from the high-level
//!   operation down to the low-level system error.
//!
//! ## Path Validation Logic
//!
//! Before creating the log file, `setup_logger` ensures that:
//! 1. The configured log file path is not empty.
//! 2. The path does not point to an existing directory.
//! 3. The path has a valid filename component.
//! 4. The parent directory of the log file exists and is actually a directory.
//! 5. The application has the necessary permissions to create and write to the log file.

use crate::utilities::logger_config::LoggerConfig;
use log::error;
use std::error::Error;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
/// Contains the error definition for the logger setup.
pub enum LoggerSetupError {
    /// The configured log file name is empty.
    #[error("Configured log file name is empty.")]
    EmptyLogFileName,

    /// Log file configuration has no file name component.
    #[error("Log file configuration {0} has no file name component.")]
    MissingFileName(String),

    /// The configured log file path is an existing directory.
    #[error("The configured log file path {0} is an existing directory.")]
    PathIsADirectory(String),

    /// Log file configuration contains a non-existing folder.
    #[error("Log file configuration contains a non-existing folder ({0}).")]
    ParentDirDoesNotExist(String),

    /// Log file configuration contains a folder, which is not a directory.
    #[error("Log file configuration contains a folder, which is not a directory ({0}).")]
    ParentIsNotADirectory(String),

    /// Could not open the output file for logging.
    #[error("Could not open output file for logging.")]
    CouldNotOpenOutputFile {
        #[source]
        source: std::io::Error,
    },
}

/// Sets up and configures the application's global logger using the `fern` crate.
///
/// This function initializes a global logger that dispatches messages to both
/// standard output (the console) and a specified log file. It performs several
/// validation checks on the provided log file path before attempting to create
/// the file and apply the logging configuration.
///
/// Log messages are formatted to include a high-precision timestamp, log level,
/// and the source module (`target`). The default logging level is set to `Debug`.
///
/// # Arguments
/// * `logger_config` - A `LoggerConfig` struct containing the path to the log file.
///
/// # Returns
/// An empty `Result` (`Ok(())`) on successful initialization of the logger.
///
/// # Errors
/// Returns a `LoggerSetupError` variant if any validation or setup step fails:
/// - `LoggerSetupError::EmptyLogFileName`: If the configured log file name is empty or contains only whitespace.
/// - `LoggerSetupError::PathIsADirectory`: If the provided log file path points to an existing directory.
/// - `LoggerSetupError::MissingFileName`: If the log file path does not have a valid file name component (e.g., it ends in `..`).
/// - `LoggerSetupError::ParentDirDoesNotExist`: If the parent directory of the specified log file does not exist.
/// - `LoggerSetupError::ParentIsNotADirectory`: If a component of the log file's parent path is a file, not a directory.
/// - `LoggerSetupError::CouldNotOpenOutputFile`: If the log file cannot be opened or created, which typically indicates a file system permissions issue.
pub fn setup_logger(logger_config: LoggerConfig) -> Result<(), LoggerSetupError> {
    // check if the configured file name is empty
    if logger_config.logfile.trim().is_empty() {
        return Err(LoggerSetupError::EmptyLogFileName);
    }

    let path = Path::new(logger_config.logfile.as_str());

    // Explicitly check if the path points to an existing directory.
    // This is an error because we cannot create a log file with the same name.
    if path.is_dir() {
        return Err(LoggerSetupError::PathIsADirectory(logger_config.logfile));
    }

    if path.file_name().is_none() {
        return Err(LoggerSetupError::MissingFileName(logger_config.logfile));
    }

    // If `parent()` is None, it's a relative path in the current directory (e.g., "logfile.log").
    // We represent the current directory as `Path::new(".")`.
    let parent_dir = path.parent().unwrap_or_else(|| Path::new("."));

    // 3. Verify the parent directory exists and is, in fact, a directory.
    if !parent_dir.exists() {
        return Err(LoggerSetupError::ParentDirDoesNotExist(
            parent_dir.display().to_string(),
        ));
    }
    if !parent_dir.is_dir() {
        return Err(LoggerSetupError::ParentIsNotADirectory(
            parent_dir.display().to_string(),
        ));
    }

    // open the file for logging
    let log_file = match fern::log_file(logger_config.logfile) {
        Ok(c) => c,
        Err(e) => return Err(LoggerSetupError::CouldNotOpenOutputFile { source: e }),
    };

    // configure logger
    let _ = fern::Dispatch::new()
        .format(|out, message, record| {
            let local_time = chrono::Local::now(); // Get current local time
            out.finish(format_args!(
                "[{} {} {}] {}",
                local_time.format("%Y-%m-%d %H:%M:%S%.3f %:z"),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(log::LevelFilter::Debug)
        .chain(std::io::stdout()) // output to stdout
        .chain(log_file) // output to file
        .apply();

    Ok(())
}

/// Logs an error and its entire chain of sources with a given context message.
///
/// This function provides a standardized way to log errors. It logs a high-level
/// context message, followed by the primary error, and then traverses and logs
/// every underlying `source` error.
///
/// # Arguments
///
/// * `module` - module name where the error logging was triggered.
/// * `context` - additional description of error
/// * `error` - Top level error
///
pub fn log_error_chain<E: Error>(module: &str, context: &str, error: E) {
    error!(target: module,"{context} ({error})");
    let mut source = error.source();
    while let Some(cause) = source {
        error!("  Caused by: {cause}");
        source = cause.source();
    }
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    /// Tests that the function returns an error when the log file name is empty.
    #[test]
    // Verifies that empty log file name.
    fn test_empty_log_file_name() {
        let config = LoggerConfig {
            logfile: "  ".to_string(), // Whitespace only
        };
        let result = setup_logger(config);
        assert!(matches!(result, Err(LoggerSetupError::EmptyLogFileName)));
    }

    /// Tests that the function returns an error if the path has no file name component,
    /// for example, if it ends in `..`.
    #[test]
    // Verifies that path missing file name.
    fn test_path_missing_file_name() {
        // A path ending in `..` has no file name component according to `std::path::Path`.
        // We use a non-existent path to ensure the preceding `is_dir()` check is passed,
        // which allows us to specifically isolate and test the `file_name().is_none()` check.
        let path_str = "/some/non/existent/path/..".to_string();

        let config = LoggerConfig { logfile: path_str };
        let result = setup_logger(config);
        assert!(matches!(result, Err(LoggerSetupError::MissingFileName(_))));
    }
}
