/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the communication of the application's process ID (PID) file.

use std::fs;
use std::process;

use aquarium_control::utilities::config::{read_config_file, ConfigData};
use aquarium_control::utilities::publish_pid::{PublishPid, PublishPidError};

#[test]
// Verifies publishing process ID (happy case).
// [itest->swreq~utilities_write_pid_file~1]
pub fn test_publish_pid() {
    let config: ConfigData =
        read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let pid_filename = config.publish_pid.pid_filename.clone();
    fs::write(pid_filename.clone(), "0").expect("test_publish_PID: Could not erase PID.");

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
// Verifies publish process ID with invalid content pid file.
// [itest->swreq~utilities_write_pid_file~1]
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
// Verifies publish process ID with file already containing a pid.
// [itest->swreq~utilities_write_pid_file~1]
pub fn test_publish_pid_file_already_contains_pid() {
    let mut config: ConfigData =
        read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.publish_pid.pid_filename =
        "/var/local/aquarium-ctrl-test-already-filled.pid".to_string();
    let result = PublishPid::new(config.publish_pid);
    assert!(matches!(
        result,
        Err(PublishPidError::PidFileContainsPid { .. })
    ));
}
