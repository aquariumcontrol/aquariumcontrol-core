/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use msg::ftok;
use ipc_rs::MessageQueue;
use serde_derive::Deserialize;
use std::fs;
use std::process;

/// Holds the configuration data for the messaging.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct MessagingConfig {
    /// filename used for publishing the PID
    pid_filename: String,

    /// filename used for generating the messsage queue identifier
    mq_filename: String,    
}

pub struct Messaging {
    config: MessagingConfig,

    /// key generated from mq_filename to be used for creation of message queue
    mq_key: u64,
    
    // Message queue
    //queue: MessageQueue,
}

impl Messaging {
    /// Checks if PID file already contains published PID.
    /// If yes, then application will panic.
    /// If no, then function publishes PID provides a struct of type Messaging.
    /// If publishing does not work, then the application will panic.
    /// # Arguments
    /// * `config` - configuration data for Balling dosing control
    pub fn new(config: MessagingConfig) -> Messaging {
        let file_data = fs::read_to_string(config.pid_filename.clone())
            .expect("Messaging: Unable to read PID file.");
        let process_id_read: u32 = match file_data.trim().parse() {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "Messaging: PID file ({}) contains invalid content: {e:?}",
                    config.pid_filename
                );
            }
        };
        if process_id_read > 0 {
            panic!(
                "Messaging: PID file ({}) already contains published PID.",
                config.pid_filename
            );
        }
        let process_id_numeric: u32 = process::id();
        let process_id_string: String = process_id_numeric.to_string() + "\n";
        fs::write(config.pid_filename.clone(), process_id_string)
            .expect("Messaging: Could not publish PID.");

        let mq_key = match ftok!(config.mq_filename.clone()) {
                Some(c) => c,
                None => {
                    panic!("Messaging: could not create key from filename {}",
                        config.mq_filename);
                }
        };
  
        /*      
        let queue = match PosixMq::create(mq_key) {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "Messaging: failure to create message queue using key {}, error={e:?}",
                    mq_key
                );
            }
        };
        */
        
        Messaging {
            config: config,
            mq_key: mq_key,
            // queue: queue,
        }
    }

    /// Erases PID from previously generated file
    pub fn erase_pid(&self) {
        fs::write(self.config.pid_filename.clone(), "0".to_string())
            .expect("Messaging: Could not erase PID.");
    }
}

#[cfg(test)]
pub mod tests {
    use crate::config::ConfigData;
    use crate::messaging::Messaging;
    use crate::read_config_file;
    use std::fs;
    use std::process;

    #[test]
    pub fn test_messaging_publish_PID() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_messaging.toml".to_string());
        let pid_filename = config.messaging.pid_filename.clone();
        fs::write(pid_filename.clone(), "0".to_string())
            .expect("test_messaging_publish_PID: Could not erase PID.");

        // create test object and run code to be tested
        _ = Messaging::new(config.messaging);

        let process_id_numeric: u32 = process::id();
        let process_id_string: String = process_id_numeric.to_string() + "\n";

        let file_data = fs::read_to_string(pid_filename)
            .expect("test_messaging_publish_PID: Unable to read file");

        assert_eq!(file_data, process_id_string);
        println!(
            "Data read from file ({}) equals data generated ({}).",
            { file_data.trim() },
            { process_id_string.trim() }
        );
    }

    #[test]
    #[should_panic]
    pub fn test_messaging_non_existent_pid_file() {
        let config: ConfigData = read_config_file(
            "/config/aquarium_control_test_messaging_non_existent_pid_file.toml".to_string(),
        );
        _ = Messaging::new(config.messaging);
    }

    #[test]
    #[should_panic]
    pub fn test_messaging_invalid_content_pid_file() {
        let config: ConfigData = read_config_file(
            "/config/aquarium_control_test_messaging_invalid_content_pid_file.toml".to_string(),
        );
        _ = Messaging::new(config.messaging);
    }
}
