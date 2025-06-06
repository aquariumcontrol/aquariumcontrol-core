/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(not(test))]
use log::info;

use serde_derive::Deserialize;
use std::fs;

#[cfg(test)]
use std::env;

use crate::food::feed::FeedConfig;
use crate::mineral::balling::BallingConfig;
use crate::recorder::data_logger::DataLoggerConfig;
use crate::relays::actuate_controllino::ActuateControllinoConfig;
use crate::relays::relay_manager::RelayManagerConfig;
use crate::sensors::ambient::AmbientConfig;
use crate::sensors::atlas_scientific::AtlasScientificConfig;
use crate::sensors::dht::DhtConfig;
use crate::sensors::gpio_handler::GpioHandlerConfig;
use crate::thermal::heating::HeatingConfig;

#[cfg(feature = "target_hw")]
use crate::sensors::i2c_interface::I2cInterfaceConfig;

use crate::sensors::tank_level_switch::TankLevelSwitchConfig;

#[cfg(target_os = "linux")]
use crate::dispatch::messaging::MessagingConfig;

use crate::database::sql_interface::SqlInterfaceConfig;
use crate::permission::schedule_check::ScheduleCheckConfig;
use crate::sensors::ds18b20::Ds18b20Config;
use crate::simulator::tcp_communication::TcpConnectionConfig;
use crate::thermal::ventilation::VentilationConfig;
use crate::utilities::publish_pid::PublishPidConfig;
use crate::watchmen::monitors::MonitorsConfig;
use crate::watchmen::watchdog::WatchdogConfig;
use crate::water::refill::RefillConfig;

const LOG_TARGET_CONFIG: &str = "config";

/// Top level struct to hold the configuration data read from .toml file.
/// It does not contain any implementation.
#[derive(Deserialize)]
pub struct ConfigData {
    /// configuration of the SQL database interface
    pub sql_interface: SqlInterfaceConfig,

    /// configuration of the TCP connection (e.g., for simulator)
    pub tcp_connection: TcpConnectionConfig,

    /// configuration of the data logger
    pub data_logger: DataLoggerConfig,

    /// configuration for AtlasScientific
    pub atlas_scientific: AtlasScientificConfig,

    /// configuration for Ambient
    pub ambient: AmbientConfig,

    /// configuration GPIO handler
    pub gpio_handler: GpioHandlerConfig,

    /// configuration of the DHT driver
    pub dht: DhtConfig,

    /// configuration of the fresh water refill control
    pub refill: RefillConfig,

    /// configuration of the tank level switch calculation
    pub tank_level_switch: TankLevelSwitchConfig,

    /// configuration of the heating control
    pub heating: HeatingConfig,

    /// configuration of the ventilation control
    pub ventilation: VentilationConfig,

    /// configuration of the monitors
    pub monitors: MonitorsConfig,

    /// configuration of the Balling dosing control
    pub balling: BallingConfig,

    /// configuration of the feed control
    pub feed: FeedConfig,

    /// configuration of the relay manager
    pub relay_manager: RelayManagerConfig,

    /// configuration of the Controllino relays
    pub controllino_relay: ActuateControllinoConfig,

    /// configuration for schedule check
    pub schedule_check: ScheduleCheckConfig,

    #[cfg(feature = "target_hw")]
    /// configuration of I2C interface
    pub i2c_interface: I2cInterfaceConfig,

    #[cfg(target_os = "linux")]
    /// configuration of messaging (IPC)
    pub messaging: MessagingConfig,

    /// configuration of PID publication
    pub publish_pid: PublishPidConfig,

    /// configuration of Watchdog communication
    pub watchdog: WatchdogConfig,

    /// configuration of DS18B20 temperature sensor communication
    pub ds18b20: Ds18b20Config,
}

#[cfg(not(test))]
/// Loads configuration data from a specified `.toml` file into a `ConfigData` struct.
///
/// This version of the function is intended for execution environments (`#[cfg(not(test))]`).
/// It reads the entire content of the `.toml` file and then attempts to parse
/// it into the `ConfigData` structure.
///
/// # Arguments
/// * `filename` - The fully qualified path (including directory and file name)
///   of the `.toml` configuration file to be read.
///
/// # Returns
/// A `ConfigData` struct containing the parsed configuration if successful.
///
/// # Panics
/// This function will panic in the following scenarios:
/// - If the specified file cannot be read (e.g., file not found, permission denied).
/// - If the file content is not valid TOML or cannot be deserialized into `ConfigData`.
pub fn read_config_file(filename: String) -> ConfigData {
    info!(target: LOG_TARGET_CONFIG, "reading configuration from {}", filename);

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

#[cfg(test)]
// Loads configuration data from a specified `.toml` file into a `ConfigData` struct for testing.
//
// This version of the function is specifically designed for test environments (`#[cfg(test)]`).
// It constructs the full file path by prepending the `CARGO_MANIFEST_DIR`
// environment variable (which points to the root of the current Cargo package)
// to the provided `filename`. It then reads and attempts to parse the TOML content.
//
// # Arguments
// * `filename` - The relative path to the configuration file, expected to be located
//   within a subfolder of the Cargo base directory.
//
// # Returns
// A `ConfigData` struct containing the parsed configuration if successful.
//
// # Panics
// This function will panic in the following scenarios:
// - If the `CARGO_MANIFEST_DIR` environment variable is not set.
// - If the specified file cannot be read (e.g., the file is not found, permissions issues).
// - If the file content is not valid TOML or cannot be deserialized into `ConfigData`.
pub fn read_config_file(filename: String) -> ConfigData {
    let filename_test = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| {
        panic!("Could not read environment variable CARGO_MANIFEST_DIR.");
    }) + "/"
        + filename.as_str();

    // inspired by https://codingpackets.com/blog/rust-load-a-toml-file/
    // Read the contents of the file using a `match` block
    // to return the `data: Ok(c)` as a `String`
    // or handle any `errors: Err(_)`.
    let contents = match fs::read_to_string(filename_test.clone()) {
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

#[cfg(test)]
#[cfg(test)]
// Loads configuration data from a `.toml` file and overrides the database name for testing.
//
// This helper function is exclusively used in test environments. It reads the
// configuration from a specified `.toml` file (relative to `CARGO_MANIFEST_DIR`)
// and then *replaces* the `db_name` within the `SqlInterfaceConfig` to point
// to a unique test database, ensuring test isolation.
//
// # Arguments
// * `relative_path` - The relative path to the configuration file within the Cargo base folder.
// * `test_db_number` - A unique number (between 1 and 99, inclusive) used to
//   construct the specific test database name (e.g., `aquarium_test_01`, `aquarium_test_28`).
//
// # Returns
// A `ConfigData` struct with the database name specifically set for testing.
//
// # Panics
// This function will panic in the following scenarios:
// - If `test_db_number` is 0 or greater than 99.
// - If `read_config_file` (the underlying function it calls) panics
//   (e.g., file not found, invalid TOML, `CARGO_MANIFEST_DIR` not set).
pub fn read_config_file_with_test_database(
    relative_path: String,
    test_db_number: u32,
) -> ConfigData {
    if test_db_number == 0 || test_db_number > 99 {
        panic!(
            "{}: number for test database ({}) is out of bounds.",
            LOG_TARGET_CONFIG, test_db_number
        )
    }

    // assemble the test database name with prefix and index.
    let test_db_name: String = "aquarium_test_".to_string() + &format!("{:02}", test_db_number);

    let mut config = read_config_file(relative_path);

    // replace the database name from the configuration file with the test database name
    // required by the test case
    config.sql_interface.db_name = test_db_name;

    config
}
