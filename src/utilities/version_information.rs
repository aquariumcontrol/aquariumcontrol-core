/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(not(test))]
use log::info;

use std::env;
use std::fmt::{Display, Formatter};

/// Major version number
pub const VERSION_MAJOR: i32 = 0;

/// Minor version number
pub const VERSION_MINOR: i32 = 3;

/// Build number
pub const VERSION_BUILD: i32 = 0;

/// Holds the data structure and implementation for retrieving and storing version information of the application.
pub struct VersionInformation {
    /// Major version number (used to identify major changes between releases)
    pub major: i32,

    /// Minor version number (used to identify minor changes between releases)
    pub minor: i32,

    /// Build numer (increases with every published build)
    pub build: i32,

    /// Hash serves as a fingerprint to match SW with database configuration
    pub hash: String,
}

impl VersionInformation {
    /// Creates a new `VersionInformation` instance, populating it with
    /// the application's compiled-in version numbers and an SHA-256 hash
    /// of its executable file.
    ///
    /// This constructor is responsible for identifying the currently running
    /// executable and computing a unique fingerprint (hash) of its binary content.
    /// This hash is used to verify the integrity or version compatibility
    /// of the running application against a database schema.
    ///
    /// # Returns
    /// A new `VersionInformation` struct containing the major, minor, build,
    /// and the calculated executable hash.
    ///
    /// # Panics
    /// This function will panic if it fails to:
    /// - Determine the path of the current executable.
    /// - Read the contents of the executable file (e.g., due to permissions or file system errors).
    pub fn new() -> VersionInformation {
        // get the fully qualified path and file name
        let pathfilename = match env::current_exe() {
            Ok(exe_path) => {
                #[cfg(not(test))]
                info!(
                    target: module_path!(),
                    "path of this executable is: {}", exe_path.display()
                );

                exe_path
            }
            Err(e) => {
                panic!(
                    "{}: failed to get current executable path: {e:?}",
                    module_path!(),
                );
            }
        };
        // calculate hash from executable
        let bytes = std::fs::read(pathfilename).unwrap(); // Vec<u8>
        let hash = sha256::digest(&bytes);
        VersionInformation {
            major: VERSION_MAJOR,
            minor: VERSION_MINOR,
            build: VERSION_BUILD,
            hash,
        }
    }
}

impl Display for VersionInformation {
    /// Formats the `VersionInformation` for display.
    ///
    /// This implementation of the `fmt` method for the `Display` trait
    /// provides a human-readable representation of the version details.
    /// It outputs the major, minor, build numbers and the commit hash
    /// in a structured, multi-line format.
    ///
    /// # Arguments
    ///
    /// * `f` - A mutable reference to a `Formatter` to which the formatted
    ///   output will be written.
    ///
    /// # Returns
    ///
    /// `std::fmt::Result`:
    /// - `Ok(())` if the formatting was successful.
    /// - `Err` containing an ` std::fmt::Error ` if an I/O error occurred
    ///   while writing to the formatter.
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Version information:")?;
        writeln!(f, "  Major: {}", self.major)?;
        writeln!(f, "  Minor: {}", self.minor)?;
        writeln!(f, "  Build: {}", self.build)?;
        writeln!(f, "  Hash:  {}", self.hash)?;
        Ok(())
    }
}
