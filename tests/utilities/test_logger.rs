/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the setup and configuration of the application's global logger.

use std::fs;
use std::fs::File;
use tempfile::tempdir;
use aquarium_control::utilities::logger::{setup_logger, LoggerSetupError};
use aquarium_control::utilities::logger_config::LoggerConfig;

/// Tests that the function returns an error if the log file name is a directory.
#[test]
// Verifies that path is a directory.
fn test_path_is_a_directory() {
    let dir = tempdir().unwrap();
    // Get the path of the temporary directory itself.
    let path_str = dir.path().to_str().unwrap().to_string();

    let config = LoggerConfig { logfile: path_str };
    let result = setup_logger(config);
    assert!(matches!(result, Err(LoggerSetupError::PathIsADirectory(_))));
}

/// Tests that the function returns an error if the parent directory does not exist.
#[test]
// Verifies that parent directory does not exist.
fn test_parent_directory_does_not_exist() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("nonexistent_subdir/bad.log");

    let config = LoggerConfig {
        logfile: file_path.to_str().unwrap().to_string(),
    };
    let result = setup_logger(config);
    assert!(matches!(
            result,
            Err(LoggerSetupError::ParentDirDoesNotExist(_))
        ));
}

/// Tests that the function returns an error if the parent path is a file, not a directory.
#[test]
// Verifies that parent is a file.
fn test_parent_is_a_file() {
    let dir = tempdir().unwrap();
    let file_as_parent = dir.path().join("file.txt");
    File::create(&file_as_parent).unwrap(); // Create the file

    let invalid_path = file_as_parent.join("log.log");

    let config = LoggerConfig {
        logfile: invalid_path.to_str().unwrap().to_string(),
    };
    let result = setup_logger(config);
    assert!(matches!(
            result,
            Err(LoggerSetupError::ParentIsNotADirectory(_))
        ));
}

/// Tests that the function returns an error if it cannot create the log file,
/// for example, due to permissions. This test is specific to Unix-like systems.
#[test]
#[cfg(unix)]
// Verifies that cannot open file in readonly directory.
fn test_cannot_open_file_in_readonly_directory() {
    use std::os::unix::fs::PermissionsExt;

    // 1. Create a temporary directory
    let dir = tempdir().unwrap();

    // 2. Set its permissions to read-only (mode 555: r-xr-xr-x)
    let mut perms = fs::metadata(dir.path()).unwrap().permissions();
    perms.set_mode(0o555);
    fs::set_permissions(dir.path(), perms).unwrap();

    // 3. Attempt to set up a logger in the read-only directory
    let file_path = dir.path().join("unwritable.log");
    let config = LoggerConfig {
        logfile: file_path.to_str().unwrap().to_string(),
    };
    let result = setup_logger(config);

    // 4. IMPORTANT: Reset permissions so the tempdir can be cleaned up
    let mut perms = fs::metadata(dir.path()).unwrap().permissions();
    perms.set_mode(0o755); // rwxr-xr-x
    fs::set_permissions(dir.path(), perms).unwrap();

    // 5. Assert that the correct error was returned
    assert!(matches!(
            result,
            Err(LoggerSetupError::CouldNotOpenOutputFile { .. })
        ));
}
