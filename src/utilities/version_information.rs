/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(not(test))]
use log::info;

use std::env;
use std::fmt::{Display, Formatter};
use std::num::ParseIntError;
use thiserror::Error;

/// Contains error definition for version information retrieval
#[derive(Debug, Error)]
pub enum VersionInformationError {
    /// Major number could not be parsed from version string
    #[error("Could not parse major number from version string ({0})")]
    ParseErrorMajor(String),

    /// Minor number could not be parsed from version string
    #[error("Could not parse minor number from version string ({0})")]
    ParseErrorMinor(String),

    /// Build number could not be parsed from version string
    #[error("Could not parse build number from version string ({0})")]
    ParseErrorBuild(String),

    /// Major number could not be converted to numeric value
    #[error("Could not parse major number from version string ({version_string})")]
    ConversionErrorMajor {
        version_string: String,

        #[source]
        source: ParseIntError,
    },

    /// Minor number could not be converted to numeric value
    #[error("Could not parse minor number from version string ({version_string})")]
    ConversionErrorMinor {
        version_string: String,

        #[source]
        source: ParseIntError,
    },

    /// Build number could not be converted to numeric value
    #[error("Could not parse build number from version string ({version_string})")]
    ConversionErrorBuild {
        version_string: String,

        #[source]
        source: ParseIntError,
    },

    #[error("Failed to retrieve current executable path.")]
    CurrentExecutablePathRetrievalFailure {
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to read executable file content.")]
    ExecutableFileReadFailure {
        #[source]
        source: std::io::Error,
    },
}

/// Holds the data structure and implementation for retrieving and storing version information of the application.
pub struct VersionInformation {
    /// Major version number (used to identify major changes between releases)
    pub major: i32,

    /// Minor version number (used to identify minor changes between releases)
    pub minor: i32,

    /// Build number (increases with every published build)
    pub build: i32,

    /// Hash serves as a fingerprint to match SW with database configuration
    pub hash: String,
}

impl VersionInformation {
    /// Creates a new `VersionInformation` instance by reading the version from
    /// `Cargo.toml` at compile time and calculating the SHA-256 hash of the
    /// current executable.
    ///
    /// This is the primary constructor for production use. It performs the following steps:
    /// 1.  Retrieves the package version string (e.g., "1.2.3") from the `CARGO_PKG_VERSION`
    ///     environment variable, which Cargo sets at compile time.
    /// 2.  Determines the full path to the running executable.
    /// 3.  Reads the executable file's binary content.
    /// 4.  Calculates a SHA-256 hash of the binary content to serve as a unique fingerprint.
    /// 5.  Parses the version string and combines it with the hash.
    ///
    /// # Returns
    /// A `Result` containing a new `VersionInformation` struct on success.
    ///
    /// # Errors
    /// Returns a `VersionInformationError` if any part of the process fails:
    /// - `VersionInformationError::CurrentExecutablePathRetrievalFailure`: If the path
    ///   to the current executable cannot be determined.
    /// - `VersionInformationError::ExecutableFileReadFailure`: If the executable file
    ///   cannot be read from the filesystem, which could be due to a permissions issue.
    /// - It will also propagate any parsing or conversion errors from the underlying
    ///   `from_string_and_hash` function if the version string from `Cargo.toml` is malformed.
    pub fn new() -> Result<VersionInformation, VersionInformationError> {
        let version_string = env!("CARGO_PKG_VERSION").to_string();

        // Get the fully qualified path and file name.
        let pathfilename = env::current_exe()
            .map_err(|e| VersionInformationError::CurrentExecutablePathRetrievalFailure { source: e })?;

        #[cfg(not(test))]
        info!(
            target: module_path!(),
            "path of this executable is: {}",
            pathfilename.display()
        );

        // Calculate hash from the executable.
        let bytes = std::fs::read(&pathfilename)
            .map_err(|e| VersionInformationError::ExecutableFileReadFailure { source: e })?;

        let hash = sha256::digest(&bytes);

        // Further process information in the testable function.
        Self::from_string_and_hash(version_string, hash)
    }

    /// Creates a new `VersionInformation` instance from a version string and a hash.
    ///
    /// This function is designed to be used in testing environments, allowing for the injection
    /// of arbitrary version strings and hashes to validate parsing and behavior.
    ///
    /// # Arguments
    /// * `version_string` - A string slice representing the version (e.g., "1.2.3").
    /// * `hash` - The `String` to use for the hash value.
    ///
    /// # Returns
    /// A `Result` containing a new `VersionInformation` struct, or a `VersionInformationError`
    /// if the version string cannot be parsed.
    pub fn from_string_and_hash(
        version_string: String,
        hash: String,
    ) -> Result<VersionInformation, VersionInformationError> {
        // Split the "x.y.z" string into its parts, returning specific error on failure
        let mut parts = version_string.split('.');
        let major_str = match parts.next() {
            Some(major_str) => major_str,
            None => {
                return Err(VersionInformationError::ParseErrorMajor(version_string));
            }
        };

        let minor_str = match parts.next() {
            Some(minor_str) => minor_str,
            None => {
                return Err(VersionInformationError::ParseErrorMinor(version_string));
            }
        };

        // The patch/build number might have suffixes like "-alpha", so we take only the numeric part.
        let build_str = match parts.next() {
            Some(build_part) => match build_part.split('-').next() {
                Some(build_str) => build_str,
                None => {
                    return Err(VersionInformationError::ParseErrorBuild(version_string));
                }
            },
            None => {
                return Err(VersionInformationError::ParseErrorBuild(version_string));
            }
        };

        // Parse each part into an i32, returning a specific error on failure.
        let major = major_str.parse::<i32>().map_err(|e| {
            VersionInformationError::ConversionErrorMajor {
                version_string: major_str.to_string(),
                source: e,
            }
        })?;

        let minor = minor_str.parse::<i32>().map_err(|e| {
            VersionInformationError::ConversionErrorMinor {
                version_string: minor_str.to_string(),
                source: e,
            }
        })?;

        let build = build_str.parse::<i32>().map_err(|e| {
            VersionInformationError::ConversionErrorBuild {
                version_string: build_str.to_string(),
                source: e,
            }
        })?;

        Ok(VersionInformation {
            major,
            minor,
            build,
            hash,
        })
    }
}

impl Display for VersionInformation {
    /// Formats the `VersionInformation` for display.
    ///
    /// This implementation provides a human-readable, multi-line representation of the
    /// version details, including major, minor, build numbers, and the executable hash.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to a `Formatter` where the output will be written.
    ///
    /// # Returns
    /// An `Ok(())` on successful formatting.
    ///
    /// # Errors
    /// Returns an `Err` containing a `std::fmt::Error` if an I/O error occurs
    /// while writing to the formatter.
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Version information:")?;
        writeln!(f, "  Major: {}", self.major)?;
        writeln!(f, "  Minor: {}", self.minor)?;
        writeln!(f, "  Build: {}", self.build)?;
        writeln!(f, "  Hash:  {}", self.hash)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_hash() -> String {
        "dummy_hash_for_testing".to_string()
    }

    #[test]
    fn test_valid_version_string() {
        let info =
            VersionInformation::from_string_and_hash("1.2.3".to_string(), dummy_hash()).unwrap();
        assert_eq!(info.major, 1);
        assert_eq!(info.minor, 2);
        assert_eq!(info.build, 3);
    }

    #[test]
    fn test_version_string_with_prerelease_tag() {
        let info =
            VersionInformation::from_string_and_hash("0.4.5-beta.1".to_string(), dummy_hash())
                .unwrap();
        assert_eq!(info.major, 0);
        assert_eq!(info.minor, 4);
        assert_eq!(info.build, 5);
    }

    #[test]
    fn test_invalid_major_version() {
        let result = VersionInformation::from_string_and_hash("a.2.3".to_string(), dummy_hash());
        assert!(matches!(
            result,
            Err(VersionInformationError::ConversionErrorMajor { .. })
        ));
    }

    #[test]
    fn test_invalid_minor_version() {
        let result = VersionInformation::from_string_and_hash("1.a.3".to_string(), dummy_hash());
        assert!(matches!(
            result,
            Err(VersionInformationError::ConversionErrorMinor { .. })
        ));
    }

    #[test]
    fn test_invalid_build_version() {
        let result = VersionInformation::from_string_and_hash("1.2.x".to_string(), dummy_hash());
        assert!(matches!(
            result,
            Err(VersionInformationError::ConversionErrorBuild { .. })
        ));
    }

    #[test]
    fn test_incomplete_version_string() {
        let result = VersionInformation::from_string_and_hash("1.2".to_string(), dummy_hash());
        assert!(matches!(
            result,
            Err(VersionInformationError::ParseErrorBuild { .. })
        ));
    }
}
