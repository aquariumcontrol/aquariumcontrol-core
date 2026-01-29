/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages the application's configuration loading and structure.
//!
//! This module is the central hub for all configurations in the application. It defines
//! the top-level `ConfigData` struct, which aggregates configuration settings from all
//! other modules. It uses the `serde` framework to deserialize a TOML file into this
//! strongly typed Rust struct, providing a safe and convenient way to access configuration
//! parameters throughout the system.
//!
//! ## Key Components
//!
//! - **`ConfigData`**: The main configuration struct. It is a composite of smaller,
//!   module-specific configuration structs (e.g., `HeatingConfig`, `DataLoggerConfig`).
//!   This composition makes the configuration modular and easy to manage.
//!
//! - **`read_config_file()`**: The primary function for loading the configuration.
//!   It has two distinct implementations controlled by `#[cfg(test)]`:
//!   - **Production (`not(test)`)**: Reads a file from a given absolute path and returns
//!     a `Result`, allowing for graceful error handling if the file is missing or invalid.
//!   - **Test (`test`)**: Constructs the file path relative to the project's root
//!     (using `CARGO_MANIFEST_DIR`) and panics on failure. This is a common pattern
//!     in tests to ensure that a misconfigured test environment fails immediately.
//!
//! - **`read_config_file_with_test_database()`**: A test-only helper that loads the
//!   configuration and then programmatically overrides the database name to ensure
//!   test isolation.
//!
//! ## Conditional Compilation
//!
//! The `ConfigData` struct uses `#[cfg(...)]` attributes to include or exclude certain
//! configuration fields based on the target operating system (`target_os = "linux"`) or
//! enabled features (`feature = "target_hw"`). This allows the application to have a single
//! configuration file format while only compiling the necessary fields for a given build target.

#[cfg(not(test))]
use log::info;

use serde_derive::Deserialize;
use std::fs;

#[cfg(all(target_os = "linux", feature = "target_hw"))]
use crate::beacon::ws2812b::Ws2812BConfig;

use crate::food::feed_config::FeedConfig;
use crate::mineral::balling_config::BallingConfig;
use crate::recorder::data_logger_config::DataLoggerConfig;
use crate::relays::actuate_controllino_config::ActuateControllinoConfig;
use crate::relays::relay_manager_config::RelayManagerConfig;
#[allow(unused)] // used in conditionally compiled code
use crate::sensors::atlas_scientific_config::AtlasScientificConfig;
use crate::sensors::dht_config::DhtConfig;
use crate::sensors::gpio_handler_config::GpioHandlerConfig;
use crate::sensors::sensor_manager_config::SensorManagerConfig;
use crate::thermal::heating_config::HeatingConfig;
#[cfg(test)]
use std::env;

#[cfg(feature = "target_hw")]
use crate::sensors::i2c_interface_config::I2cInterfaceConfig;

use crate::sensors::tank_level_switch_config::TankLevelSwitchConfig;

#[cfg(target_os = "linux")]
use crate::dispatch::messaging_config::MessagingConfig;

use crate::database::sql_interface_config::SqlInterfaceConfig;
use crate::permission::schedule_check_config::ScheduleCheckConfig;
use crate::sensors::ds18b20_config::Ds18b20Config;
use crate::simulator::tcp_communication_config::TcpCommunicationConfig;
use crate::thermal::temperature_gradient_config::TemperatureGradientConfig;
use crate::thermal::ventilation_config::VentilationConfig;
use crate::utilities::config_error::ConfigError;
use crate::utilities::logger_config::LoggerConfig;
use crate::utilities::publish_pid_config::PublishPidConfig;
use crate::watchmen::memory_config::MemoryConfig;
use crate::watchmen::monitors_config::MonitorsConfig;
use crate::watchmen::watchdog_config::WatchdogConfig;
use crate::water::refill_config::RefillConfig;

/// Top level struct to hold the configuration data read from .toml file.
/// It does not contain any implementation.
#[derive(Deserialize)]
pub struct ConfigData {
    /// configuration of the SQL database interface
    pub sql_interface: SqlInterfaceConfig,

    /// configuration of the TCP connection (e.g., for simulator)
    pub tcp_communication: TcpCommunicationConfig,

    /// configuration of the data logger
    pub data_logger: DataLoggerConfig,

    /// configuration for AtlasScientific
    #[allow(unused)] // used in conditionally compiled code
    pub atlas_scientific: AtlasScientificConfig,

    /// configuration for Ambient
    pub sensor_manager: SensorManagerConfig,

    /// configuration GPIO handler
    pub gpio_handler: GpioHandlerConfig,

    /// configuration of the DHT driver
    #[allow(unused)] // used in conditionally compiled code
    pub dht: DhtConfig,

    /// configuration of the fresh water refill control
    pub refill: RefillConfig,

    /// configuration of the tank level switch calculation
    pub tank_level_switch: TankLevelSwitchConfig,

    /// configuration of the heating control
    pub heating: HeatingConfig,

    /// configuration of the ventilation control
    pub ventilation: VentilationConfig,

    /// configuration of the temperature gradient calculation
    pub temperature_gradient: TemperatureGradientConfig,

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

    /// configuration of WS2812B RGB LED
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub ws2812b: Ws2812BConfig,

    /// check of memory configuration
    pub memory: MemoryConfig,

    /// configuration of the general log file
    pub logger: LoggerConfig,
}

#[cfg(not(test))]
/// Loads configuration data from a specified `.toml` file into a `ConfigData` struct.
///
/// This version of the function is intended for production environments. It reads
/// the entire content of the `.toml` file and then attempts to parse it into the
/// `ConfigData` structure.
///
/// # Arguments
/// * `filename` - The fully qualified path (including directory and file name)
///   of the `.toml` configuration file to be read.
///
/// # Returns
/// A `Result` containing a `ConfigData` struct with the parsed configuration on success.
///
/// # Errors
/// Returns a `ConfigError` variant if any part of the process fails:
/// - `ConfigError::FileRead`: If the specified file cannot be read from the filesystem.
///   This can be caused by the file not existing, or the application lacking the
///   necessary permissions to access it.
/// - `ConfigError::TomlParse`: If the file's content is not valid TOML, or if its
///   structure does not match the `ConfigData` struct, preventing deserialization.
pub fn read_config_file(filename: String) -> Result<ConfigData, ConfigError> {
    #[cfg(not(test))]
    info!(target: module_path!(), "reading configuration from {filename}");

    // Read the contents of the file and return error in case of failure.
    let contents = fs::read_to_string(filename.clone())
        .map_err(|e| ConfigError::FileRead(filename.clone(), e))?;

    // Parse the configuration data and return error in case of failure.
    let data = toml::from_str(&contents).map_err(|e| ConfigError::TomlParse(filename, e))?;

    Ok(data)
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
pub fn read_config_file(filename: String) -> Result<ConfigData, ConfigError> {
    let pathname_test =
        env::var("CARGO_MANIFEST_DIR").map_err(|_| ConfigError::CargoManifestDir)?;

    let filename_test = pathname_test + "/" + filename.as_str();
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
    let result = match toml::from_str(&contents) {
        // If successful, return data as `Data` struct.
        // `d` is a local variable.
        Ok(d) => d,
        // Handle the `error` case.
        Err(e) => {
            // Write `msg` to `stderr`.
            panic!(
                "{}: unable to load data from `{filename}` ({e})",
                module_path!()
            );
        }
    };
    Ok(result)
}

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
            module_path!(),
            test_db_number
        )
    }

    // assemble the test database name with prefix and index.
    let test_db_name: String = "aquarium_test_".to_string() + &format!("{:02}", test_db_number);

    let mut config = read_config_file(relative_path).unwrap();

    // replace the database name from the configuration file with the test database name
    // required by the test case
    config.sql_interface.db_name = test_db_name;

    config
}
