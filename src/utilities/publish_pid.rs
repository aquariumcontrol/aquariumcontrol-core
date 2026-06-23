/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages the application's process ID (PID) file to ensure a single instance.
//!
//! This module provides the `PublishPid` struct and associated logic to implement a
//! "singleton" pattern for the application. By creating and managing a PID file, it
//! prevents multiple instances of the service from running simultaneously, which could
//! lead to resource conflicts or unpredictable behavior.
//!
//! ## Core Logic
//!
//! The primary logic is encapsulated in the `PublishPid::new()` constructor, which is
//! intended to be called once at application startup. It performs the following sequence:
//!
//! 1.  **Check for Existing PID File**: It attempts to read the PID file specified in the configuration.
//! 2.  **Validate Content**:
//!     - If the file exists and contains a valid, non-zero process ID, it assumes another
//!       instance is already running and returns a `PidFileContainsPid` error, preventing startup.
//!     - If the file does not exist, is empty, or contains "0", it proceeds.
//! 3.  **Publish Current PID**: It writes the current process's ID to the file, effectively
//!     "locking" it and signaling that this instance is now the active one.
//!
//! The `erase_pid()` method is called during graceful shutdown to write "0" back to the
//! file, releasing the lock and allowing a new instance to start later.
//!
//! ## Key Components
//!
//! - **`PublishPid`**: A simple struct that holds the configuration and provides the main
//!   functionality.
//!
//! - **`PublishPidError`**: A custom, `thiserror`-based error enum that represents all
//!   possible failures during PID publication. This allows the main application to
//!   provide clear, actionable feedback if it fails to start (e.g., "Another instance
//!   is already running").

use crate::utilities::publish_pid_config::PublishPidConfig;
use std::num::ParseIntError;
use std::{fs, process};
use thiserror::Error;

/// Contains the error definition for PublishPid
#[derive(Debug, Error)]
pub enum PublishPidError {
    /// Reading from the PID file failed.
    #[error("[{location}] Unable to read PID file ({file}).")]
    UnableToReadPidFile {
        location: String,
        file: String,

        #[source]
        source: std::io::Error,
    },

    /// The PID file contains invalid content.
    #[error("[{location}] PID file ({file}) contains invalid content.")]
    PidFileParseError {
        location: String,
        file: String,

        #[source]
        source: ParseIntError,
    },

    /// The PID file already contains published PID.
    #[error("[{location}] PID file ({file}) already contains published PID: {pid}")]
    PidFileContainsPid {
        location: String,
        file: String,
        pid: u32,
    },

    /// Writing to the PID file failed.
    #[error("[{location}] Unable to write PID file ({file}).")]
    UnableToWritePidFile {
        location: String,
        file: String,

        #[source]
        source: std::io::Error,
    },
}
/// Contains the struct for configuration data and provides implementation for
/// publication of process id (Pid).
#[derive(Clone)]
pub struct PublishPid {
    config: PublishPidConfig,
}

impl PublishPid {
    /// Creates a new `PublishPid` instance and publishes the current process ID (PID) to a file.
    ///
    /// This constructor performs a critical step of publishing the application's PID to a
    /// designated file. This is often used to prevent multiple instances of the application
    /// from running concurrently.
    ///
    /// The process involves:
    /// 1.  Checking for the existence of the PID file.
    /// 2.  If the file exists and contains a valid, positive PID, the function returns an error,
    ///     indicating that another instance of the application is likely already running.
    /// 3.  If the file does not exist, is empty, or contains "0", the function
    ///     proceeds to write the current process's PID to the file, creating it if necessary.
    ///
    /// # Arguments
    /// * `config` - Configuration data for publishing the PID, primarily containing
    ///   the `pid_filename`.
    ///
    /// # Returns
    /// A `Result` containing a new `PublishPid` struct on success.
    ///
    /// # Errors
    /// Returns a `PublishPidError` if the PID cannot be published:
    /// - `PublishPidError::UnableToReadPidFile`: If the specified PID file exists but cannot
    ///   be read due to, for example, insufficient permissions.
    /// - `PublishPidError::PidFileParseError`: If the PID file contains content that cannot
    ///   be parsed into a `u32` integer.
    /// - `PublishPidError::PidFileContainsPid`: If the PID file already contains a valid,
    ///   positive PID, indicating a potential duplicate instance is running.
    /// - `PublishPidError::UnableToWritePidFile`: If writing the current process's PID to
    ///   the file fails, for example due to insufficient write permissions or an invalid path.
    pub fn new(config: PublishPidConfig) -> Result<PublishPid, PublishPidError> {
        match fs::read_to_string(&config.pid_filename) {
            Ok(file_data) => {
                // File exists, check its content.
                let file_data_trimmed = file_data.trim();
                if !file_data_trimmed.is_empty() {
                    let process_id_read: u32 = file_data_trimmed.parse().map_err(|e| {
                        PublishPidError::PidFileParseError {
                            location: module_path!().to_string(),
                            file: config.pid_filename.clone(),
                            source: e,
                        }
                    })?;

                    if process_id_read > 0 {
                        // A common extension here is to check if a process with this PID is
                        // actually running, but for now, checking the file is a robust start.
                        return Err(PublishPidError::PidFileContainsPid {
                            location: module_path!().to_string(),
                            file: config.pid_filename.clone(),
                            pid: process_id_read,
                        });
                    }
                }
                // If we are here, the file was empty or contained "0". We can proceed to write.
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File doesn't exist, which is a valid state. We will create it.
            }
            Err(e) => {
                // Any other read error is fatal.
                return Err(PublishPidError::UnableToReadPidFile {
                    location: module_path!().to_string(),
                    file: config.pid_filename.clone(),
                    source: e,
                });
            }
        }

        // If all checks have passed, it's safe to write our PID.
        let process_id_numeric: u32 = process::id();
        let process_id_string = format!("{process_id_numeric}\n");
        fs::write(&config.pid_filename, process_id_string).map_err(|e| {
            PublishPidError::UnableToWritePidFile {
                location: module_path!().to_string(),
                file: config.pid_filename.clone(),
                source: e,
            }
        })?;

        Ok(PublishPid { config })
    }

    /// Erases the PID from the file by writing "0" to it.
    ///
    /// This is typically called during a graceful shutdown to release the application "lock"
    /// and allow a new instance to start up without conflicts.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) on a successful write.
    ///
    /// # Errors
    /// Returns a `PublishPidError::UnableToWritePidFile` if the PID file cannot be
    /// written to. This could be due to a permissions issue, an invalid path, or if
    /// the file was unexpectedly deleted.
    pub fn erase_pid(&self) -> Result<(), PublishPidError> {
        fs::write(&self.config.pid_filename, "0\n").map_err(|e| {
            PublishPidError::UnableToWritePidFile {
                location: module_path!().to_string(),
                file: self.config.pid_filename.clone(),
                source: e,
            }
        })
    }
}

#[cfg(test)]
pub mod tests {
    use std::fs;
    use std::process;

    use crate::utilities::config::{read_config_file, ConfigData};
    use crate::utilities::publish_pid::{PublishPid, PublishPidError};

    #[test]
    pub fn test_publish_pid() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let pid_filename = config.publish_pid.pid_filename.clone();
        fs::write(pid_filename.clone(), "0".to_string())
            .expect("test_publish_PID: Could not erase PID.");

        // create the test object and run code to be tested
        _ = PublishPid::new(config.publish_pid);

        let process_id_numeric: u32 = process::id();
        let process_id_string: String = process_id_numeric.to_string() + "\n";

        let file_data =
            fs::read_to_string(pid_filename).expect("test_publish_PID: Unable to read file");

        assert_eq!(file_data, process_id_string);
        println!(
            "Data read from file ({}) equals data generated ({}).",
            { file_data.trim() },
            { process_id_string.trim() }
        );
    }

    #[test]
    pub fn test_publish_pid_invalid_content_pid_file() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.publish_pid.pid_filename =
            "/var/local/aquarium-ctrl-test-invalid-content.pid".to_string();
        let result = PublishPid::new(config.publish_pid);
        assert!(matches!(
            result,
            Err(PublishPidError::PidFileParseError { .. })
        ));
    }

    #[test]
    pub fn test_publish_pid_file_already_contains_pid() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.publish_pid.pid_filename = "/var/local/aquarium-ctrl-test-already-filled.pid".to_string();
        let result = PublishPid::new(config.publish_pid);
        assert!(matches!(
            result,
            Err(PublishPidError::PidFileContainsPid { .. })
        ));
    }
}
