/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use log::info;
use serde_derive::Deserialize;
use std::fs;
use std::process;

/// Holds the configuration data for the PID publication.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct PublishPidConfig {
    /// filename used for publishing the PID
    pid_filename: String,
}

/// Contains the struct for configuration data and provides implementation for
/// publication of process id (Pid).
pub struct PublishPid {
    config: PublishPidConfig,
}

impl PublishPid {
    const LOG_TARGET_PUBLISH_PID: &'static str = "publish_pid";

    /// Checks if PID file already contains published PID.
    /// If yes, then application will panic.
    /// If no, then function publishes PID provides a struct of type PublishPid
    /// If publishing does not work, then the application will panic.
    /// # Arguments
    /// * `config` - configuration data for PublishPid
    pub fn new(config: PublishPidConfig) -> PublishPid {
        let file_data = fs::read_to_string(config.pid_filename.clone())
            .expect("PublishPid: Unable to read PID file.");
        let file_data_trimmed = file_data.trim();

        if !file_data_trimmed.is_empty() {
            let process_id_read: u32 = match file_data_trimmed.parse() {
                Ok(c) => c,
                Err(e) => {
                    info!(
                        target: Self::LOG_TARGET_PUBLISH_PID,
                        "PID file ({}) contains invalid content: {e:?}",
                        config.pid_filename
                    );                    
                    panic!(
                        "{}: PID file ({}) contains invalid content: {e:?}",
                        Self::LOG_TARGET_PUBLISH_PID,
                        config.pid_filename
                    );
                }
            };
            if process_id_read > 0 {
                info!(
                    target: Self::LOG_TARGET_PUBLISH_PID,
                    "PID file ({}) already contains published PID.",                    
                    config.pid_filename
                );                
                panic!(
                    "{}: PID file ({}) already contains published PID.",
                    Self::LOG_TARGET_PUBLISH_PID,
                    config.pid_filename
                );
            }
        }
        let process_id_numeric: u32 = process::id();
        let process_id_string: String = process_id_numeric.to_string() + "\n";
        fs::write(config.pid_filename.clone(), process_id_string)
            .expect("PublishPid: Could not publish PID.");

        PublishPid { config: config }
    }

    /// Erases PID from previously generated file
    pub fn erase_pid(&self) {
        fs::write(self.config.pid_filename.clone(), "0".to_string())
            .expect("PublishPid: Could not erase PID.");
    }
}

#[cfg(test)]
pub mod tests {
    use crate::config::ConfigData;
    use crate::publish_pid::PublishPid;
    use crate::read_config_file;
    use std::fs;
    use std::process;

    #[test]
    pub fn test_publish_pid() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_publish_pid.toml".to_string());
        let pid_filename = config.publish_pid.pid_filename.clone();
        fs::write(pid_filename.clone(), "0".to_string())
            .expect("test_publish_PID: Could not erase PID.");

        // create test object and run code to be tested
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
    #[should_panic]
    pub fn test_non_existent_pid_file() {
        let config: ConfigData = read_config_file(
            "/config/aquarium_control_test_publish_pid_non_existent_pid_file.toml".to_string(),
        );
        _ = PublishPid::new(config.publish_pid);
    }

    #[test]
    #[should_panic]
    pub fn test_publish_pid_invalid_content_pid_file() {
        let config: ConfigData = read_config_file(
            "/config/aquarium_control_test_publish_pid_invalid_content_pid_file.toml".to_string(),
        );
        _ = PublishPid::new(config.publish_pid);
    }
}
