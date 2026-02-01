/* Copyright 2024-2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(target_os = "linux")]
use posixmq::PosixMq;

use aquarium_control::dispatch::ipc_message::Message;
use aquarium_control::dispatch::messaging_commands::command_values;
use aquarium_control::dispatch::messaging_domain::MessagingDomain;
use aquarium_control::utilities::config::ConfigData;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::process::exit;
use std::{env, fs};

/// Contains a definition of all permissible domain strings
mod domain_strings {
    pub const REFILL: &str = "refill";
    pub const VENTILATION: &str = "ventilation";
    pub const FEED: &str = "feed";
    pub const HEATING: &str = "heating";
    pub const BALLING: &str = "balling";
    pub const MONITORS: &str = "monitors";
    pub const WATCHDOG: &str = "watchdog";
}

/// Contains a definition of all permissible command strings
mod command_strings {
    pub const RESET: &str = "reset";
    pub const STOP: &str = "stop";
    pub const START: &str = "start";
    pub const EXEC: &str = "execute";
}

/// Enumeration containing the representation of the specific errors for this program that may occur.
#[derive(Debug)]
enum AquariumClientError {
    /// domain string could not be parsed.
    UnidentifiedDomain,

    /// the command string could not be parsed.
    UnidentifiedCommand,
}

/// Parses the string provided as a parameter and provides
/// wrapped in Result either the numeric representation of the command
/// or an error of type AquariumClientError.
/// # Arguments
/// * `cmd` - string containing command to be parsed
fn get_cmd(cmd: &str) -> Result<i32, AquariumClientError> {
    match cmd {
        command_strings::RESET => Ok(command_values::RESET),
        command_strings::STOP => Ok(command_values::STOP),
        command_strings::START => Ok(command_values::START),
        command_strings::EXEC => Ok(command_values::EXEC),
        _ => Err(AquariumClientError::UnidentifiedCommand),
    }
}

/// Parses the string provided as a parameter and provides
/// wrapped in Result either the numeric representation of the domain
/// or an error of type AquariumClientError.
/// # Arguments
/// * `dmn` - string containing domain to be parsed
fn get_domain(dmn: &str) -> Result<i32, AquariumClientError> {
    match dmn {
        domain_strings::VENTILATION => Ok(MessagingDomain::Ventilation as i32),
        domain_strings::HEATING => Ok(MessagingDomain::Heating as i32),
        domain_strings::FEED => Ok(MessagingDomain::Feed as i32),
        domain_strings::BALLING => Ok(MessagingDomain::Balling as i32),
        domain_strings::REFILL => Ok(MessagingDomain::Refill as i32),
        domain_strings::MONITORS => Ok(MessagingDomain::Monitors as i32),
        domain_strings::WATCHDOG => Ok(MessagingDomain::Watchdog as i32),
        _ => Err(AquariumClientError::UnidentifiedDomain),
    }
}

/// display the usage information for this program.
fn display_usage() {
    println!("aquarium_client requires [CONFIG FILE (OPTIONAL)] [DOMAIN] [COMMAND] [PARAMETER (OPTIONAL)]:");
    println!(" - Domain 'refill' processes following commands:");
    println!("   'reset': reset errors of refill control");
    println!("   'stop': stop refill control");
    println!("   'start': start refill control");
    println!(" - Domain 'ventilation' processes following commands:");
    println!("   'reset': reset errors of ventilation control");
    println!("   'stop': stop ventilation control");
    println!("   'start': start ventilation control");
    println!(" - Domain 'heating' processes following commands:");
    println!("   'reset': reset errors of heating control");
    println!("   'stop': stop ventilation control");
    println!("   'start': start ventilation control");
    println!(" - Domain 'feeder' processes following commands:");
    println!("   'reset': reset errors of feed control");
    println!("   'execute [feed schedule entry ID]': execute a feed profile");
    println!("   'stop': stop feed control");
    println!("   'start': start feed control");
    println!(" - Domain 'balling' processes following commands:");
    println!("   'execute [dosing profile ID]': execute a dosing profile");
    println!("   'stop': stop dosing control");
    println!("   'start': start dosing control");
}

/// Loads the configuration data described in a struct of type ConfigData from the .toml file.
/// In case of failure, the function panics.
/// In success case, the function returns a variable of type ConfigData.
/// # Arguments:
/// * `file_name` - When compiling for development, the configuration file shall be stored in a
///   subfolder of the cargo base folder. The variable contains the name of the subfolder.
pub fn read_config_file(user_specified_file: Option<String>) -> ConfigData {
    const LOG_TARGET_CONFIG: &str = "config";

    let filename = if let Some(f) = user_specified_file {
        f
    } else {
        // Code for debug versions
        #[cfg(debug_assertions)]
        let out_dir = match env::var("CARGO_MANIFEST_DIR") {
            Ok(c) => c + "/config",
            Err(_) => {
                println!(
                    "aquarium_client: could not read environment variable CARGO_MANIFEST_DIR. Using fallback path.");
                "/etc/aquarium_control".to_string()
            }
        };

        // Code for release versions
        #[cfg(not(debug_assertions))]
        let out_dir = "/etc/aquarium_control/".to_string();

        out_dir + "/aquarium_control.toml"
    };

    #[cfg(not(test))]
    println!("aquarium_client: reading configuration from {}", filename);

    // inspired by https://codingpackets.com/blog/rust-load-a-toml-file/
    // Read the contents of the file using a `match` block
    // to return the `data: Ok(c)` as a `String`
    // or handle any `errors: Err(_)`.
    let contents = match fs::read_to_string(filename.clone()) {
        // If successful, return the file's text as `contents`.
        // `c` is a local variable.
        Ok(c) => c,
        // Handle the `error` case.
        Err(_) => {
            // Write `msg` to `stderr`.
            panic!("Could not read file `{}`", filename);
        }
    };

    // Use a `match` block to return the
    // file `contents` as a `Data struct: Ok(d)`
    // or handle any `errors: Err(_)`.
    match toml::from_str(&contents) {
        // If successful, return data as `Data` struct.
        // `d` is a local variable.
        Ok(d) => d,
        // Handle the `error` case.
        Err(e) => {
            // Write `msg` to `stderr`.
            panic!(
                "{}: unable to load data from `{filename}` ({e})",
                LOG_TARGET_CONFIG
            );
        }
    }
}

fn main() {
    // get command line parameters from OS
    let args: Vec<String> = env::args().collect();

    // Basic length check to prevent out of bounds early on
    if args.len() < 2 {
        display_usage();
        println!("aquarium_client: Application requires arguments.");
        exit(-1);
    }

    // Determine if the first argument is a config file or a domain.
    // We try to parse args[1] as a domain. If it fails, we assume it's a config file.
    let mut arg_index = 1;
    let mut config_path: Option<String> = None;

    if get_domain(&args[1]).is_err() {
        // It's not a domain, so treat it as a config file path
        config_path = Some(args[1].clone());
        arg_index += 1;
    }

    // Check if we have enough arguments remaining (Domain + Command)
    // args.len() must be greater than the index of the last required argument (arg_index + 1)
    if args.len() <= arg_index + 1 {
        display_usage();
        println!("aquarium_client: Insufficient arguments provided.");
        exit(-1);
    }

    // read the configuration file for this client program
    let config = read_config_file(config_path);

    // Reading PID from the file
    // UPDATED: Accessing pid_filename from publish_pid section
    let file = File::open(&config.publish_pid.pid_filename)
        .expect("aquarium_client: Failed to open PID file");
    let reader = BufReader::new(file);
    let pid: i32 = reader
        .lines()
        .next()
        .expect("aquarium_client: Failed to read PID from file")
        .expect("aquarium_client: Failed to read line")
        .parse()
        .expect("aquarium_client: Failed to parse PID");

    // check if PID has a meaningful value
    if pid <= 0 {
        println!("aquarium_client: Control application is not running. Aborting.");
        exit(-1);
    }

    // open the message queue
    #[cfg(target_os = "linux")]
    // UPDATED: Accessing mq_filename from messaging section
    let mq = match PosixMq::open(&config.messaging.mq_filename) {
        Ok(c) => c,
        Err(e) => {
            println!("aquarium_client: failure to open message queue. ({e:?})");
            exit(-1);
        }
    };

    // Preparing the message object
    let mut msg = Message::new();

    // parse the domain from the command line parameter
    msg.domain = match get_domain(&args[arg_index]) {
        Ok(c) => c,
        Err(e) => {
            println!("aquarium_client: Error parsing domain: {e:?}");
            exit(-1);
        }
    };

    // parse the command from the command line parameter
    msg.command = match get_cmd(&args[arg_index + 1]) {
        Ok(c) => c,
        Err(e) => {
            println!("aquarium_client: Error parsing command: {e:?}");
            exit(-1);
        }
    };

    // if more than the minimum number of parameters is given, then try to parse the numeric parameter
    if args.len() > arg_index + 2 {
        msg.command_param1 = args[arg_index + 2]
            .parse()
            .expect("aquarium_client: Invalid first numeric parameter");
    }

    // if more than the minimum number of parameters plus one is given, then try to parse further numeric parameter
    if args.len() > arg_index + 3 {
        msg.command_param2 = args[arg_index + 3]
            .parse()
            .expect("aquarium_client: Invalid second numeric parameter");
    }

    // sending an IPC message containing user command
    #[cfg(target_os = "linux")]
    match mq.send(0, &msg.to_bytes()) {
        Ok(()) => {
            println!("message sent to Aquarium control application.");
        }
        Err(e) => {
            println!("Error occurred when sending message to Aquarium control application ({e:?})");
        }
    }
    #[cfg(not(target_os = "linux"))]
    println!(
        "This terminal client for aquarium_control needs to run on the Linux-based controller."
    );
    #[cfg(not(target_os = "linux"))]
    println!("You provided the following data: {:?}", msg);
}
