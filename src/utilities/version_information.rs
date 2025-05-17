/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(not(test))]
use log::info;

use std::env;

/// Major version number
pub const VERSION_MAJOR: i32 = 0;

/// Minor version number
pub const VERSION_MINOR: i32 = 2;

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
    const LOG_TARGET_VERSION_INFORMATION: &'static str = "version_information";

    /// Provides a struct of type VersionInformation.
    pub fn new() -> VersionInformation {
        // get the fully qualified path and file name
        let pathfilename = match env::current_exe() {
            Ok(exe_path) => {
                #[cfg(not(test))]
                info!(
                    target: Self::LOG_TARGET_VERSION_INFORMATION,
                    "path of this executable is: {}", exe_path.display()
                );

                exe_path
            }
            Err(e) => {
                panic!(
                    "{}: failed to get current exe path: {e:?}",
                    Self::LOG_TARGET_VERSION_INFORMATION,
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
