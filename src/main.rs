/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

// @todo: check error propagation for all modules being initialized in run()

#![deny(missing_docs)]
//! Aquarium control is an open-source application that is part of a control system for salt-water aquariums.
//! It contains the following functionality:
//! - Data acquisition of sensors (water temperature, pH, conductivity, ambient air temperature, ambient humidity)
//! - Fresh water refill
//! - Heating control
//! - Ventilation control
//! - Automatic feeding
//! - Balling mineral dosing

/// This is the container module for the database communication.
/// Communicating with the SQL database includes preparing SQL queries, executing the queries and processing of query results.
mod database;

/// Communication with the user via client application or webserver using POSIX message queues.
mod dispatch;

/// This container module features all the functionality to execute the automatic feeding.
mod food;

/// This container module features all the functionality to execute the Balling mineral dosing.
mod mineral;

/// The container module contains the schedule checker.
mod permission;

/// This container module contains all functionality to periodically store signals either in one table of the SQL database or in specific text files stored in the RAM disk
mod recorder;

/// This container module features all functionality to actuate the relays which control the actuators.
mod relays;

/// This container module features all functionality to read and postprocess sensor data.
mod sensors;

/// This container module features all functionality to communicate with the test server via TCP.
mod simulator;

#[cfg(test)]
/// This container module features a set of mock functionality used by test cases.
mod mocks;

/// This container module features all functionality to either cool the water using ventilation heat the water using a heater.
mod thermal;

/// This container module features all functionality that could not be assigned to any other container module.
mod utilities;

/// This container module features all functionality to monitor the control system.
mod watchmen;

/// This container module features all functionality related to RGB LED.
mod beacon;

/// This container module features all functionality to replace the evaporated water with fresh water.
mod water;

use log::{error, info, warn};

#[cfg(feature = "debug_main")]
use log::debug;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use nix::unistd;

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_balling::SqlInterfaceBalling;
use crate::database::sql_interface_data::SqlInterfaceData;
use crate::database::sql_interface_error::SqlInterfaceError;
use crate::database::sql_interface_feed::{DatabaseInterfaceFeedTrait, SqlInterfaceFeed};
use crate::database::sql_interface_heating_setvals::SqlInterfaceHeatingSetVals;
use crate::database::sql_interface_heating_stats::SqlInterfaceHeatingStats;
use crate::database::sql_interface_heating_stats_data_transfer::HeatingStatsDataTransfer;
use crate::database::sql_interface_midnight::SqlInterfaceMidnightCalculator;
use crate::database::sql_interface_refill::{DatabaseInterfaceRefillTrait, SqlInterfaceRefill};
use crate::database::sql_interface_schedule::SqlInterfaceSchedule;
use crate::database::sql_interface_ventilation_setvals::SqlInterfaceVentilationSetVals;
#[cfg(target_os = "linux")]
use crate::dispatch::messaging::{Messaging, MessagingChannels};
use crate::food::feed::{Feed, FeedChannels};
use crate::food::food_injection::FoodInjection;
use crate::mineral::balling::{Balling, BallingChannels};
use crate::mineral::mineral_injection::MineralInjection;
use crate::permission::schedule_check::{ScheduleCheck, ScheduleCheckChannels};
use crate::recorder::data_injection::DataInjection;
use crate::recorder::data_logger::{DataLogger, DataLoggerChannels, DataLoggerMutexes};
use crate::relays::actuate_controllino::ActuateControllino;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use crate::relays::actuate_gpio::{ActuateGpio, ActuateGpioConfig};
use crate::relays::actuate_simulator::ActuateSimulator;
use crate::relays::relay_manager::{RelayManager, RelayManagerChannels};
use crate::sensors::atlas_scientific::{
    AtlasScientific, AtlasScientificChannels, AtlasScientificResultData,
};
use crate::sensors::dht::{Dht, DhtResult};
use crate::sensors::ds18b20::{Ds18b20, Ds18b20ResultData};
use crate::sensors::i2c_interface::{I2cRequest, I2cResult};
use crate::sensors::sensor_manager::{
    SensorManager, SensorManagerChannels, SensorManagerMutexes, SensorManagerSignals,
};
use crate::sensors::tank_level_switch::{TankLevelSwitch, TankLevelSwitchSignals};
use crate::simulator::tcp_communication::{
    TcpCommunication, TcpCommunicationChannelPair, TcpCommunicationChannels,
};
use crate::simulator::tcp_communication_error::TcpCommunicationError;
use crate::thermal::distinct_interval_check::distinct_thermal_control_interval_check;
use crate::thermal::heating::{Heating, HeatingChannels, HeatingMutexes};
use crate::thermal::ventilation::{Ventilation, VentilationChannels};
use crate::utilities::channel_content::InternalCommand;
use crate::utilities::config::read_config_file;
use crate::utilities::config_file_definition_error::ConfigFileDefinitionError;
use crate::utilities::logger::{log_error_chain, setup_logger, LoggerConfig};
use crate::utilities::memory_config_check::memory_config_check;
use crate::utilities::publish_pid::PublishPid;
use crate::utilities::signal_handler::{handle_signals, SignalHandlerChannels};
use crate::utilities::startup_error::StartupError;
use crate::utilities::version_information::{VersionInformation, VersionInformationError};
use crate::watchmen::monitors::{Monitors, MonitorsChannels};
use crate::watchmen::petting::Petting;
use crate::watchmen::watchdog::Watchdog;
use crate::water::refill::{Refill, RefillChannels, RefillStatus};
use crate::water::refill_monitor_view::RefillMonitorView;
use crate::water::water_injection::WaterInjection;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::{env, panic, path::Path, thread::scope, time::Duration};

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
        use crate::sensors::gpio_handler::GpioHandler;
        use crate::beacon::ws2812b::RgbLedColor;
        use crate::beacon::ws2812b::Ws2812B;
    }
}
cfg_if::cfg_if! {
    if #[cfg(all(feature = "target_hw", target_os = "linux"))] {
        use crate::sensors::i2c_interface::I2cInterface;
    }
}

/// Displays the valid command line parameter combinations
fn display_usage() {
    println!("aquarium_control requires a configuration file to be specified to enter operation. Alternatively, the following options are available:");
    println!("-h, --help: Output this page.");
    println!("-v, --version: Output version information including hash of executable.");
}

/// Checks if the string represents a user command and executes related functionality.
/// # Arguments
/// * `command` - A string slice representing the potential command.
/// # Returns
/// * A boolean value if a valid command has been recognized unless error occurred.
/// * `VersionInformationError` in case version information could not be retrieved
fn check_is_command(command: &str) -> Result<bool, VersionInformationError> {
    match command {
        "-h" | "--help" => {
            display_usage();
            Ok(true)
        }
        "-v" | "--version" => {
            // try to calculate hash of executable
            match VersionInformation::new() {
                Ok(version_information) => {
                    println!("{version_information}");
                    Ok(true)
                }
                Err(e) => Err(e),
            }
        }
        _ => Ok(false), // All other strings do not match
    }
}

/// Checks if the provided string is a valid path to a `.toml` configuration file.
///
/// This function validates a user-selected configuration file path by ensuring that:
/// - The path exists on the file system.
/// - It points to a regular file (not a directory).
/// - The file has an extension.
/// - The file's extension is `.toml`.
///
/// # Arguments
/// * `user_selected_config_file_name` - A string slice representing the potential path to the configuration file.
///
/// # Returns
/// A `Result<String, ConfigFileDefinitionError>`:
/// - `Ok(String)`: The validated and owned file path if all checks pass.
/// - `Err(ConfigFileDefinitionError)`: If any validation check fails, with a descriptive error message.
fn check_argument_for_valid_config_file_name(
    user_selected_config_file_name: &str,
) -> Result<String, ConfigFileDefinitionError> {
    let path = Path::new(&user_selected_config_file_name);

    let desired_config_file_name = user_selected_config_file_name.to_string();

    if !path.exists() {
        return Err(ConfigFileDefinitionError::PathDoesNotExist(
            desired_config_file_name,
        ));
    }

    if !path.is_file() {
        return Err(ConfigFileDefinitionError::IsNotAFile(
            desired_config_file_name,
        ));
    }

    match path.extension() {
        Some(extension) => {
            if extension == "toml" {
                Ok(String::from(user_selected_config_file_name))
            } else {
                Err(ConfigFileDefinitionError::SuffixIsNotToml(
                    desired_config_file_name,
                ))
            }
        }
        None => Err(ConfigFileDefinitionError::FileNameHasNoSuffix(
            desired_config_file_name,
        )),
    }
}

/// Checks if the current process is running with root (UID 0) privileges.
///
/// This function inspects the Effective User ID (EUID) of the process.
/// On Unix-like systems, if the EUID is 0, the process has root privileges.
///
/// # Returns
/// `true` if the process has root privileges, `false` otherwise.
///
/// # Panics
/// This function may panic if the underlying system call `geteuid` fails,
/// which is highly unlikely in normal operation.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn is_running_as_root() -> bool {
    let euid = unistd::geteuid();
    euid.is_root()
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// The main entry point of the Aquarium Control application.
///
/// This function initializes and orchestrates all modules of the control system.
/// It performs the following high-level steps:
/// 1.  Sets up global logging and handles command-line arguments for configuration file selection.
/// 2.  Checks if the user has provided specific command line parameters and acts accordingly.
/// 3.  Checks if the user has root privileges.
/// 4.  Loads the application configuration from the specified `.toml` file.
/// 5.  Publishes the application's Process ID (PID) to a file and sets up a custom panic hook for cleanup.
/// 6.  Performs version checks against the database to ensure compatibility.
/// 7.  Initializes all SQL database interfaces and validates database readiness.
/// 8.  Sets up all inter-thread communication channels (`mpsc`).
/// 9.  Initializes all application modules (sensors, controls, managers) with their respective configurations and channels/mutexes.
/// 10. Spawns each module in its own dedicated thread within a `std::thread::scope` for structured concurrency.
/// 11. Manages the initial data acquisition phase, waiting for sensors to provide first readings before control loops become fully active.
///
/// This function is responsible for the overall lifecycle of the application, including graceful startup and panic handling.
///
/// Global thread communication is as follows:
/// ```mermaid
/// graph LR
///     tank_level_switch -.-> refill[Refill]
///     refill --> relay_manager[Controllino]
///     relay_manager --> refill
///     sensor_manager[SensorManager] -.-> data_logger[DataLogger]
///     refill -.-> data_logger
///     signal_handler[SignalHandler] --> refill
///     refill --> signal_handler
///     signal_handler --> tank_level_switch
///     tank_level_switch --> signal_handler
///     signal_handler --> tcp_communication[TcpCommunication]
///     tcp_communication --> signal_handler
///     signal_handler --> relay_manager
///     relay_manager --> signal_handler
///     balling[Balling] --> signal_handler
///     signal_handler --> balling
///     feed[Feed] --> signal_handler
///     signal_handler --> feed
///     heating[Heating] --> signal_handler
///     signal_handler --> heating
///     ventilation[Ventilation] --> signal_handler
///     signal_handler --> ventilation
///     relay_manager --> tcp_communication
///     sensor_manager --> tcp_communication
///     tcp_communication --> sensor_manager
///     atlas_scientific --> tcp_communication
///     tcp_communication --> atlas_scientific
///     tank_level_switch --> tcp_communication
///     tcp_communication --> tank_level_switch
///     balling --> relay_manager
///     relay_manager --> balling
///     feed --> relay_manager
///     relay_manager --> feed
///     atlas_scientific -.-> data_logger
///     heating -.-> data_logger
///     atlas_scientific -.-> heating
///     heating --> relay_manager
///     relay_manager --> heating
///     tank_level_switch -.-> heating
///     sensor_manager -.-> heating
///     tank_level_switch -.-> data_logger
///     ventilation -.-> data_logger
///     signal_handler --> data_logger
///     data_logger --> signal_handler
///     ventilation --> relay_manager
///     atlas_scientific -.-> ventilation
///     relay_manager --> ventilation
///     monitors[Monitors] --> relay_manager
///     relay_manager --> monitors
///     monitors --> refill
///     refill --> monitors
///     monitors --> signal_handler
///     signal_handler --> monitors
///     ventilation --> schedule_check[ScheduleCheck]
///     schedule_check --> ventilation
///     heating --> schedule_check
///     schedule_check --> heating
///     refill --> schedule_check
///     schedule_check --> refill
///     balling --> schedule_check
///     schedule_check --> balling
///     signal_handler --> schedule_check
///     schedule_check --> signal_handler
///     signal_handler --> messaging[Messaging]
///     signal_handler --> sensor_manager
///     signal_handler --> watchdog
///     sensor_manager --> signal_handler
///     signal_handler --> atlas_scientific
///     atlas_scientific --> signal_handler
///     atlas_scientific --> i2c_interface[I2C interface]
///     i2c_interface --> atlas_scientific
///     dht -.-> sensor_manager
///     messaging --> refill
///     messaging --> ventilation
///     messaging --> heating
///     messaging --> balling
///     messaging --> feed
///     messaging --> monitors
///     messaging --> watchdog
/// ```
fn run() -> Result<(), StartupError> {
    let spin_sleeper = SpinSleeper::default();

    #[cfg(all(not(target_os = "linux"), feature = "target_hw"))]
    compile_error!("Target hardware is only available with Linux operating system.");

    // get command line parameters from OS
    let args: Vec<String> = env::args().collect();

    // initialize with a default configuration file name
    let mut config_file_name = "/etc/aquarium_control/config/aquarium_control.toml".to_string();

    // Check command line parameters
    if args.len() > 1 {
        // If the first argument contains a path and file name, then let's use that.
        config_file_name = match check_argument_for_valid_config_file_name(&args[1]) {
            Ok(valid_file_name) => valid_file_name, // The file is valid. Start operation.
            Err(config_file_definition_error) => {
                let mut user_arg_identified = false;
                // Iterate through the command line parameters and check if they are valid.
                for user_arg in args[1..].iter() {
                    user_arg_identified = match check_is_command(user_arg) {
                        Ok(alternative_execution_path) => alternative_execution_path,
                        Err(e) => {
                            return Err(StartupError::VersionInformationRetrievalFailure {
                                source: e,
                            });
                        }
                    };
                }
                return if !user_arg_identified {
                    // If the user provided an invalid filename and no valid command, output error message and abort.
                    Err(StartupError::Config(config_file_definition_error))
                } else {
                    // At least one valid user command was provided. Graceful exit.
                    Ok(())
                };
            }
        }
    }
    // read the configuration file for operating the control system
    let config = read_config_file(config_file_name)?;

    // setup logging to write to the log output file and stdout
    setup_logger(config.logger).map_err(|e| StartupError::LoggingSetupFailure { source: e })?;

    // calculate hash of executable
    let version_information = VersionInformation::new()
        .map_err(|e| StartupError::VersionInformationRetrievalFailure { source: e })?;
    #[cfg(not(test))]
    info!(target: module_path!(), "{version_information}");

    // check if the user has root privileges - disabled when developing on the Windows platform
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if !is_running_as_root() {
        return Err(StartupError::NotRoot);
    }

    // publish the PID to a file so that the client application can communicate with this application
    let publish_pid = PublishPid::new(config.publish_pid);
    let publish_pid_clone = publish_pid.clone();

    panic::set_hook(Box::new(move |panic_info| {
        // Custom panic handler: resets the PID to zero before entering in panic
        // This allows later runs of the application without needing to manually
        // clear the PID file.
        // Print the panic message to stderr.
        publish_pid_clone.erase_pid();
        error!("Panic occurred: {panic_info}");

        if let Some(location) = panic_info.location() {
            error!(
                "Panic occurred in file '{}' at line {}",
                location.file(),
                location.line()
            );
        }
    }));

    // check if the memory configuration is valid
    match memory_config_check(&config.memory) {
        Ok(env_var_name_value_tuple_opt) => {
            match env_var_name_value_tuple_opt {
                Some((env_var_name, value)) => {
                    info!("Memory configuration check: {env_var_name} is configured to {value}");
                }
                None => { /* do nothing, configuration check may be disabled */ }
            }
        }
        Err(e) => {
            return Err(StartupError::InvalidMemoryConfig { source: e });
        }
    }

    // check if the control parameters of heating and ventilation are valid
    if !distinct_thermal_control_interval_check(&config.heating, &config.ventilation) {
        return Err(StartupError::InvalidThermalConfig);
    }

    // instantiate the duration for the database ping
    let database_ping_duration = Duration::from_secs(config.sql_interface.db_ping_interval);

    // get values from config before moving the config
    let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
    let max_rows_feed_pattern = config.sql_interface.max_rows_feed_pattern;
    let max_rows_feed_schedule = config.sql_interface.max_rows_feed_schedule;
    let max_rows_heating_stats = config.sql_interface.max_rows_heating_stats;
    let max_rows_schedule = config.sql_interface.max_rows_schedule;
    let max_rows_data = config.sql_interface.max_rows_data;
    let max_rows_refill = config.sql_interface.max_rows_refill;
    let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
    let max_rows_feed_log = config.sql_interface.max_rows_feed_log;

    // instantiate driver for RGB LED
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    if config.ws2812b.active {
        let led_duration = Duration::from_millis(300);
        let mut ws2812b = Ws2812B::new(config.ws2812b)
            .map_err(|e| StartupError::Ws2812BSetupFailure { source: e })?;
        if let Err(e) = ws2812b.set_color(RgbLedColor::Red) {
            warn!(target: module_path!(), "Failed to set RGB LED color: {e}");
        }
        spin_sleeper.sleep(led_duration);
        if let Err(e) = ws2812b.set_color(RgbLedColor::Blue) {
            warn!(target: module_path!(), "Failed to set RGB LED color: {e}");
        }
        spin_sleeper.sleep(led_duration);
        if let Err(e) = ws2812b.set_color(RgbLedColor::Green) {
            warn!(target: module_path!(), "Failed to set RGB LED color: {e}");
        }
    }

    // connect to SQL database, subsequently set up component-specific interfaces
    let mut sql_interface =
        SqlInterface::new(config.sql_interface).map_err(|e| StartupError::Database {
            source: Box::new(e),
        })?;

    let database_connection_schedule =
        sql_interface
            .get_connection()
            .map_err(|e| StartupError::Database {
                source: Box::new(e),
            })?;
    let sql_interface_schedule =
        SqlInterfaceSchedule::new(database_connection_schedule, max_rows_schedule).map_err(
            |e| StartupError::Database {
                source: Box::new(e),
            },
        )?;

    let database_connection_balling =
        sql_interface
            .get_connection()
            .map_err(|e| StartupError::Database {
                source: Box::new(e),
            })?;
    let sql_interface_balling = SqlInterfaceBalling::new(
        database_connection_balling,
        max_rows_balling_set_values,
        max_rows_balling_dosing_log,
    )
    .map_err(|e| StartupError::Database {
        source: Box::new(e),
    })?;

    let database_connection_refill =
        sql_interface
            .get_connection()
            .map_err(|e| StartupError::Database {
                source: Box::new(e),
            })?;
    let sql_interface_refill: Box<dyn DatabaseInterfaceRefillTrait + Sync + Send> = Box::new(
        SqlInterfaceRefill::new(database_connection_refill, max_rows_refill).map_err(|e| {
            StartupError::Database {
                source: Box::new(e),
            }
        })?,
    );

    let database_connection_midnight_calculator =
        sql_interface
            .get_connection()
            .map_err(|e| StartupError::Database {
                source: Box::new(e),
            })?;
    let sql_interface_midnight_calculator =
        SqlInterfaceMidnightCalculator::new(database_connection_midnight_calculator);

    let database_connection_heating_stats =
        sql_interface
            .get_connection()
            .map_err(|e| StartupError::Database {
                source: Box::new(e),
            })?;
    let sql_interface_heating = SqlInterfaceHeatingStats::new(
        database_connection_heating_stats,
        max_rows_heating_stats,
        Box::new(sql_interface_midnight_calculator),
    )
    .map_err(|e| StartupError::Database {
        source: Box::new(e),
    })?;

    let database_connection_feed =
        sql_interface
            .get_connection()
            .map_err(|e| StartupError::Database {
                source: Box::new(e),
            })?;
    let sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> = Box::new(
        SqlInterfaceFeed::new(
            database_connection_feed,
            max_rows_feed_pattern,
            max_rows_feed_schedule,
            max_rows_feed_log,
        )
        .map_err(|e| StartupError::Database {
            source: Box::new(e),
        })?,
    );

    let database_connection_data =
        sql_interface
            .get_connection()
            .map_err(|e| StartupError::Database {
                source: Box::new(e),
            })?;
    let sql_interface_data = SqlInterfaceData::new(database_connection_data, max_rows_data)
        .map_err(|e| StartupError::Database {
            source: Box::new(e),
        })?;

    let tables = sql_interface
        .get_tables()
        .map_err(|e| StartupError::Database {
            source: Box::new(e),
        })?;

    match sql_interface.get_hash_from_database(&version_information) {
        Ok(c) => {
            info!(
                target: module_path!(),
                "version information including hash of executable found in database: {c}"
            );
            if c != version_information.hash {
                warn!(
                    target: module_path!(),
                    "hash value for version in database does not match hash version of executable. Proceed with caution.");
            }
        }
        Err(e) => {
            if matches!(
                e,
                SqlInterfaceError::SingleVersionRequestMultipleResults(_, _)
            ) {
                warn!(
                    target: module_path!(),
                    "main: Found multiple hash values for version in database. Proceed with caution.");
            } else {
                return Err(StartupError::Database {
                    source: Box::new(e),
                });
            }
        }
    }

    let database = sql_interface
        .get_database()
        .map_err(|e| StartupError::Database {
            source: Box::new(e),
        })?;
    info!(target: module_path!(), "current database name is {database}");

    sql_interface
        .check_required_tables_existing()
        .map_err(|e| StartupError::Database {
            source: Box::new(e),
        })?;
    info!(target: module_path!(), "required tables are existing in database.");

    let mut tables_string: String = "read the following tables from database:".to_string();
    for table in tables {
        tables_string += " ";
        tables_string += &table;
    }
    info!(target: module_path!(), "{tables_string}");

    // check if any of the components requires use of the simulator
    let use_simulator = config.gpio_handler.use_simulator
        | config.relay_manager.use_simulator
        | config.sensor_manager.use_simulator
        | config.tank_level_switch.use_simulator;

    // create channels for communication between threads
    let tx_signal_handler_to_messaging_opt: Option<Sender<InternalCommand>>;
    let rx_signal_handler_from_messaging_opt: Option<Receiver<bool>>;
    let rx_refill_from_messaging_opt: Option<Receiver<InternalCommand>>;
    let rx_ventilation_from_messaging_opt: Option<Receiver<InternalCommand>>;
    let rx_heating_from_messaging_opt: Option<Receiver<InternalCommand>>;
    let rx_feed_from_messaging_opt: Option<Receiver<InternalCommand>>;
    let rx_balling_from_messaging_opt: Option<Receiver<InternalCommand>>;
    let rx_monitors_from_messaging_opt: Option<Receiver<InternalCommand>>;

    #[allow(clippy::needless_late_init)]
    let rx_watchdog_from_messaging_opt: Option<Receiver<InternalCommand>>;

    cfg_if::cfg_if! {
        if #[cfg(target_os = "linux")] {
            // channels for linux-specific code (messaging)
            // *** signal_handler => messaging ***
            let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler): (
                Sender<InternalCommand>,
                Receiver<InternalCommand>,
            ) = mpsc::channel();
            tx_signal_handler_to_messaging_opt = Some(tx_signal_handler_to_messaging);

            let (tx_messaging_to_signal_handler, rx_signal_handler_from_messaging): (
                Sender<bool>,
                Receiver<bool>,
            ) = mpsc::channel();
            rx_signal_handler_from_messaging_opt = Some(rx_signal_handler_from_messaging);

            // *** messaging => refill ***
            let (tx_messaging_to_refill, rx_refill_from_messaging): (
                Sender<InternalCommand>,
                Receiver<InternalCommand>,
            ) = mpsc::channel();
            rx_refill_from_messaging_opt = Some(rx_refill_from_messaging);

            // *** messaging => ventilation ***
            let (tx_messaging_to_ventilation, rx_ventilation_from_messaging): (
                Sender<InternalCommand>,
                Receiver<InternalCommand>,
            ) = mpsc::channel();
            rx_ventilation_from_messaging_opt = Some(rx_ventilation_from_messaging);

            // *** messaging => heating ***
            let (tx_messaging_to_heating, rx_heating_from_messaging): (
                Sender<InternalCommand>,
                Receiver<InternalCommand>,
            ) = mpsc::channel();
            rx_heating_from_messaging_opt = Some(rx_heating_from_messaging);

            // *** messaging => feed ***
            let (tx_messaging_to_feed, rx_feed_from_messaging): (
                Sender<InternalCommand>,
                Receiver<InternalCommand>,
            ) = mpsc::channel();
            rx_feed_from_messaging_opt = Some(rx_feed_from_messaging);

            // *** messaging => balling ***
            let (tx_messaging_to_balling, rx_balling_from_messaging): (
                Sender<InternalCommand>,
                Receiver<InternalCommand>,
            ) = mpsc::channel();
            rx_balling_from_messaging_opt = Some(rx_balling_from_messaging);

            // *** messaging => monitors ***
            let (tx_messaging_to_monitors, rx_monitors_from_messaging): (
                Sender<InternalCommand>,
                Receiver<InternalCommand>,
            ) = mpsc::channel();
            rx_monitors_from_messaging_opt = Some(rx_monitors_from_messaging);

            // *** messaging => monitors ***
            let (tx_messaging_to_watchdog, rx_watchdog_from_messaging): (
                Sender<InternalCommand>,
                Receiver<InternalCommand>,
            ) = mpsc::channel();
            rx_watchdog_from_messaging_opt = Some(rx_watchdog_from_messaging);
        }
        else {
            // replacement values for channels for non-linux specific code
            tx_signal_handler_to_messaging_opt = None;
            rx_signal_handler_from_messaging_opt = None;
            rx_refill_from_messaging_opt = None;
            rx_ventilation_from_messaging_opt = None;
            rx_heating_from_messaging_opt = None;
            rx_feed_from_messaging_opt = None;
            rx_balling_from_messaging_opt = None;
            rx_monitors_from_messaging_opt = None;
            rx_watchdog_from_messaging_opt = None;
        }
    }

    // *** refill <=> relay_manager ***
    let (tx_refill_to_relay_manager, rx_relay_manager_from_refill): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_relay_manager_to_refill, rx_refill_from_relay_manager): (Sender<bool>, Receiver<bool>) =
        mpsc::channel();
    // *******************************

    // *** refill <=> schedule_check ***
    let (tx_refill_to_schedule_check, rx_schedule_check_from_refill): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_schedule_check_to_refill, rx_refill_from_schedule_check): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *******************************

    // *** heating <=> relay_manager ***
    let (tx_heating_to_relay_manager, rx_relay_manager_from_heating): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_relay_manager_to_heating, rx_heating_from_relay_manager): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *******************************

    // *** heating <=> schedule_check ***
    let (tx_heating_to_schedule_check, rx_schedule_check_from_heating): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_schedule_check_to_heating, rx_heating_from_schedule_check): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *******************************

    // *** ventilation <=> relay_manager ***
    let (tx_ventilation_to_relay_manager, rx_relay_manager_from_ventilation): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_relay_manager_to_ventilation, rx_ventilation_from_relay_manager): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *******************************

    // *** ventilation <=> schedule_check ***
    let (tx_ventilation_to_schedule_check, rx_schedule_check_from_ventilation): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_schedule_check_to_ventilation, rx_ventilation_from_schedule_check): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *******************************

    // *** feed <=> relay_manager ***
    let (tx_feed_to_relay_manager, rx_relay_manager_from_feed): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_relay_manager_to_feed, rx_feed_from_relay_manager): (Sender<bool>, Receiver<bool>) =
        mpsc::channel();
    // *******************************

    // *** balling <=> relay_manager ***
    let (tx_balling_to_relay_manager, rx_relay_manager_from_balling): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_relay_manager_to_balling, rx_balling_from_relay_manager): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *******************************

    // *** balling <=> schedule_check ***
    let (tx_balling_to_schedule_check, rx_schedule_check_from_balling): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_schedule_check_to_balling, rx_balling_from_schedule_check): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *******************************

    // *** monitors <=> relay_manager ***
    let (tx_monitors_to_relay_manager, _rx_relay_manager_from_monitors): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (_tx_relay_manager_to_monitors, rx_monitors_from_relay_manager): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *******************************

    // *** refill => monitors ***
    let (tx_refill_to_monitors, rx_monitors_from_refill): (
        Sender<RefillMonitorView>,
        Receiver<RefillMonitorView>,
    ) = mpsc::channel();
    //********************************

    // *** signal handler <=> refill ***
    let (tx_signal_handler_to_refill, rx_refill_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_refill_to_signal_handler, rx_signal_handler_from_refill): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // ***********************************

    // *** signal handler <=> heating ***
    let (tx_signal_handler_to_heating, rx_heating_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_heating_to_signal_handler, rx_signal_handler_from_heating): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // ***********************************

    // *** signal handler <=> feed ***
    let (tx_signal_handler_to_feed, rx_feed_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_feed_to_signal_handler, rx_signal_handler_from_feed): (Sender<bool>, Receiver<bool>) =
        mpsc::channel();
    // ***********************************

    // *** signal handler <=> balling ***
    let (tx_signal_handler_to_balling, rx_balling_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_balling_to_signal_handler, rx_signal_handler_from_balling): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // ***********************************

    // *** signal handler <=> monitors ***
    let (tx_signal_handler_to_monitors, rx_monitors_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_monitors_to_signal_handler, rx_signal_handler_from_monitors): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // ***********************************

    // *** signal handler <=> ventilation ***
    let (tx_signal_handler_to_ventilation, rx_ventilation_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_ventilation_to_signal_handler, rx_signal_handler_from_ventilation): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // ***********************************

    // *** signal handler <=> schedule_check ***
    let (tx_signal_handler_to_schedule_check, rx_schedule_check_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_schedule_check_to_signal_handler, rx_signal_handler_from_schedule_check): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // ***********************************

    // *** signal handler <=> gpio ***
    let (tx_signal_handler_to_tank_level_switch, rx_tank_level_switch_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_tank_level_switch_to_signal_handler, rx_signal_handler_from_tank_level_switch): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *********************************

    // *** signal handler <=> relay_manager ***
    let (tx_signal_handler_to_relay_manager, rx_relay_manager_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_relay_manager_to_signal_handler, rx_signal_handler_from_relay_manager): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // ***************************************

    // *** signal handler <=> data_logger ***
    let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_data_logger_to_signal_handler, rx_signal_handler_from_data_logger): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // **************************************

    // *** signal handler <=> atlas_scientific ***
    let (tx_signal_handler_to_atlas_scientific, rx_atlas_scientific_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_atlas_scientific_to_signal_handler, rx_signal_handler_from_atlas_scientific): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // **************************************

    // *** signal handler <=> watchdog ***
    let (tx_signal_handler_to_watchdog, rx_watchdog_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_watchdog_to_signal_handler, rx_signal_handler_from_watchdog): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // **************************************

    // *** signal handler <=> sensor_manager ***
    let (tx_signal_handler_to_sensor_manager, rx_sensor_manager_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_sensor_manager_to_signal_handler, rx_signal_handler_from_sensor_manager): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // **************************************

    // *** signal handler <=> tcp ***
    let (tx_signal_handler_to_tcp, rx_tcp_communication_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_tcp_communication_to_signal_handler, rx_signal_handler_from_tcp): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *******************************

    // *** signal handler => ds18b20 ***
    let (tx_signal_handler_to_ds18b20, rx_ds18b20_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    // *******************************

    // *** relay_manager => tcp ***
    let (tx_relay_manager_to_tcp, rx_tcp_communication_from_relay_manager): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    // ****************************

    // *** sensor_manager <=> tcp ***
    let (tx_sensor_manager_to_tcp, rx_tcp_communication_from_sensor_manager): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_tcp_communication_to_sensor_manager, rx_sensor_manager_from_tcp): TcpCommunicationChannelPair = mpsc::channel();
    // **********************

    // *** tank_level_switch <=> tcp ***
    let (tx_tank_level_switch_to_tcp, rx_tcp_communication_from_tank_level_switch): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_tcp_communication_to_tank_level_switch, rx_tank_level_switch_from_tcp): TcpCommunicationChannelPair = mpsc::channel();
    // **********************

    // wrapping sender/receiver for communication with TCP thread
    let tx_signal_handler_to_tcp_opt: Option<Sender<InternalCommand>> = match use_simulator {
        true => Some(tx_signal_handler_to_tcp),
        false => None,
    };

    let rx_signal_handler_from_tcp_opt: Option<Receiver<bool>> = match use_simulator {
        true => Some(rx_signal_handler_from_tcp),
        false => None,
    };

    let tx_relay_manager_to_tcp_opt: Option<Sender<InternalCommand>> =
        match config.relay_manager.use_simulator {
            true => Some(tx_relay_manager_to_tcp),
            false => None,
        };

    let rx_tcp_communication_from_relay_manager_opt: Option<Receiver<InternalCommand>> =
        match config.relay_manager.use_simulator {
            true => Some(rx_tcp_communication_from_relay_manager),
            false => None,
        };

    let tx_tank_level_switch_to_tcp_opt: Option<Sender<InternalCommand>> =
        match config.tank_level_switch.use_simulator {
            true => Some(tx_tank_level_switch_to_tcp),
            false => None,
        };

    let rx_tcp_communication_from_tank_level_switch_opt: Option<Receiver<InternalCommand>> =
        match config.tank_level_switch.use_simulator {
            true => Some(rx_tcp_communication_from_tank_level_switch),
            false => None,
        };

    let tx_tcp_communication_to_tank_level_switch_opt: Option<
        Sender<Result<f32, TcpCommunicationError>>,
    > = match config.tank_level_switch.use_simulator {
        true => Some(tx_tcp_communication_to_tank_level_switch),
        false => None,
    };

    let rx_tank_level_switch_from_tcp_opt: Option<Receiver<Result<f32, TcpCommunicationError>>> =
        match config.tank_level_switch.use_simulator {
            true => Some(rx_tank_level_switch_from_tcp),
            false => None,
        };

    let tx_sensor_manager_to_tcp_opt: Option<Sender<InternalCommand>> =
        match config.sensor_manager.use_simulator {
            true => Some(tx_sensor_manager_to_tcp),
            false => None,
        };

    let rx_tcp_communication_from_sensor_manager_opt: Option<Receiver<InternalCommand>> =
        match config.sensor_manager.use_simulator {
            true => Some(rx_tcp_communication_from_sensor_manager),
            false => None,
        };

    let tx_tcp_communication_to_sensor_manager_opt: Option<
        Sender<Result<f32, TcpCommunicationError>>,
    > = match config.sensor_manager.use_simulator {
        true => Some(tx_tcp_communication_to_sensor_manager),
        false => None,
    };

    let rx_sensor_manager_from_tcp_opt: Option<Receiver<Result<f32, TcpCommunicationError>>> =
        match config.sensor_manager.use_simulator {
            true => Some(rx_sensor_manager_from_tcp),
            false => None,
        };

    let (tx_signal_handler_to_dht, rx_dht_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();

    #[allow(unused)] // channel used in conditionally compiled code
    let (tx_signal_handler_to_i2c_interface, rx_i2c_interface_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();

    // Atlas Scientific <=> i2c_interface
    #[allow(unused)] // channel used in conditionally compiled code
    let (tx_atlas_scientific_to_i2c_interface, rx_i2c_interface_from_atlas_scientific): (
        Sender<I2cRequest>,
        Receiver<I2cRequest>,
    ) = mpsc::channel();
    #[allow(unused)] // channel used in conditionally compiled code
    let (tx_i2c_interface_to_atlas_scientific, rx_atlas_scientific_from_i2c_interface): (
        Sender<I2cResult>,
        Receiver<I2cResult>,
    ) = mpsc::channel();

    #[cfg(target_os = "linux")]
    let mut messaging =
        Messaging::new(config.messaging).map_err(|e| StartupError::Messaging { source: e })?;

    let mut food_injection = FoodInjection::new(&config.feed);
    let mut mineral_injection = MineralInjection::new();
    let mut water_injection = WaterInjection::new(&config.refill);
    let mut petting = Petting::new();
    let mut actuate_controllino = ActuateControllino::new(config.controllino_relay.clone())
        .map_err(|e| StartupError::ControllinoSetupFailure { source: e })?;

    // The following structs have a high amount of platform-specific code:
    let mut tank_level_switch: TankLevelSwitch;

    let mut dht: Dht;
    let gpio_dht22_io = config.gpio_handler.dht22_io;
    let gpio_dht22_vcc = config.gpio_handler.dht22_vcc;

    cfg_if::cfg_if! {
        if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
            let mut actuate_gpio_opt: Option<ActuateGpio> = None;

            // clone information from config before giving config to GpioHandler
            let gpio_tank_level_switch = config.gpio_handler.tank_level_switch;
            let actuate_gpio_config = ActuateGpioConfig::new(&config.gpio_handler);
            let use_simulator_for_gpio = config.gpio_handler.use_simulator;
            let gpio_handler_opt = GpioHandler::new(config.gpio_handler).
                map_err(|e| StartupError::GpioHandlerSetupFailure { source: e })?;

            if use_simulator_for_gpio {
                // use the simulator-version for TankLevelSwitch
                tank_level_switch = TankLevelSwitch::new(
                    config.tank_level_switch,
                    true,
                    None,
                    gpio_tank_level_switch
                ).map_err(|e| StartupError::TankLevelSwitchSetupFailure { source: e })?;
                // Actual DHT sensor reading is not required in this case. Use a stub version.
                dht = Dht::new(config.dht, None, gpio_dht22_io, gpio_dht22_vcc);
            }
            else if gpio_handler_opt.is_none() {
                // defensive programming - this case should not occur
                return Err(StartupError::GpioHandlerMissing);
            }
            else {
                let gpio_handler = gpio_handler_opt.unwrap();
                let gpio_lib_handle = gpio_handler.gpio_lib_handle_opt.unwrap();
                let gpio_lib_handle_cloned_for_tank_level_switch = gpio_lib_handle.clone();
                let gpio_lib_handle_cloned_for_relay_manager = gpio_lib_handle.clone();

                tank_level_switch = TankLevelSwitch::new(
                     config.tank_level_switch,
                     true,
                     Some(gpio_lib_handle_cloned_for_tank_level_switch),
                     gpio_tank_level_switch
                ).map_err(|e| StartupError::TankLevelSwitchSetupFailure { source: e })?;
                dht = Dht::new(config.dht, Some(gpio_lib_handle.clone()), gpio_dht22_io, gpio_dht22_vcc);
                let actuate_gpio = ActuateGpio::new(
                    gpio_lib_handle_cloned_for_relay_manager,
                    actuate_gpio_config)
                .map_err(|e| StartupError::ActuateGpioSetupFailure { source: e });
                actuate_gpio_opt = Some(actuate_gpio);
            }
        }
        else {
            tank_level_switch = TankLevelSwitch::new(
                config.tank_level_switch,
                true,
            ).map_err(|e| StartupError::TankLevelSwitchSetupFailure { source: e })?;

            // Actual DHT sensor reading is not required in this case. Use a stub version.
            dht = Dht::new(config.dht, None, gpio_dht22_io, gpio_dht22_vcc);
        }
    }

    //**************************************** Mutexes *********************************************
    let mutex_device_scheduler = Arc::new(Mutex::new(0));

    // Mutexes for DS18B20 signals
    let mutex_ds18b20_water_temperature = Arc::new(Mutex::new(Ok(Ds18b20ResultData::new(
        config.sensor_manager.replacement_value_water_temperature,
    ))));
    let mutex_ds18b20_ambient_temperature = Arc::new(Mutex::new(Ok(Ds18b20ResultData::new(
        config.sensor_manager.replacement_value_ambient_temperature,
    ))));
    let mutex_ds18b20_water_temperature_clone_for_sensor_manager =
        mutex_ds18b20_water_temperature.clone();
    let mutex_ds18b20_ambient_temperature_clone_for_sensor_manager =
        mutex_ds18b20_ambient_temperature.clone();

    // Mutexes for Atlas Scientific signals
    let mutex_atlas_scientific_temperature = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_water_temperature,
    )));
    let mutex_atlas_scientific_temperature_clone_for_sensor_manager =
        mutex_atlas_scientific_temperature.clone();
    let mutex_atlas_scientific_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_ph,
    )));
    let mutex_atlas_scientific_ph_clone_for_sensor_manager = mutex_atlas_scientific_ph.clone();
    let mutex_atlas_scientific_conductivity = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_conductivity,
    )));
    let mutex_atlas_scientific_conductivity_clone_for_sensor_manager =
        mutex_atlas_scientific_conductivity.clone();

    // Mutexes for TankLevelSwitch
    let mutex_tank_level_switch_signals =
        Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));
    let mutex_tank_level_switch_signals_clone_for_data_logger =
        mutex_tank_level_switch_signals.clone();
    let mutex_tank_level_switch_signals_clone_for_heating = mutex_tank_level_switch_signals.clone();
    let mutex_tank_level_switch_signals_clone_for_refill = mutex_tank_level_switch_signals.clone();

    // Mutex for heating
    let mutex_heating_status = Arc::new(Mutex::new(false));
    let mutex_heating_status_clone_for_data_logger = mutex_heating_status.clone();

    // Mutex for ventilation
    let mutex_ventilation_status = Arc::new(Mutex::new(false));
    let mutex_ventilation_status_clone_for_data_logger = mutex_ventilation_status.clone();

    // Mutex for refill
    let refill_status = RefillStatus {
        refill_in_progress_live: false,
        refill_in_progress_for_database: false,
    };
    let mutex_refill_status = Arc::new(Mutex::new(refill_status));
    let mutex_refill_status_clone_for_data_logger = mutex_refill_status.clone();

    // Mutex for Dht
    let mutex_dht: Arc<Mutex<DhtResult>> = Arc::new(Mutex::new(Ok((0.0, 0.0))));
    let mutex_dht_clone_for_sensor_manager = mutex_dht.clone();

    // Mutex for Sensor manager
    let mutex_sensor_manager_signals: Arc<Mutex<SensorManagerSignals>> = Arc::new(Mutex::new(
        SensorManagerSignals::new(&config.sensor_manager),
    ));
    let mutex_sensor_manager_signals_clone_for_data_logger = mutex_sensor_manager_signals.clone();
    let mutex_sensor_manager_signals_clone_for_heating = mutex_sensor_manager_signals.clone();
    let mutex_sensor_manager_signals_clone_for_ventilation = mutex_sensor_manager_signals.clone();
    //**********************************************************************************************

    let mut atlas_scientific = AtlasScientific::new(config.atlas_scientific);

    let mut sensor_manager = SensorManager::new(config.sensor_manager)
        .map_err(|e| StartupError::SensorManagerSetupFailure { source: e })?;

    let mut relay_manager = RelayManager::new(config.relay_manager.clone());
    let mut schedule_check = ScheduleCheck::new(
        config.schedule_check,
        sql_interface_schedule,
        database_ping_duration,
    );

    let mut data_injection = DataInjection;
    let mut data_logger = DataLogger::new(config.data_logger, database_ping_duration)
        .map_err(|e| StartupError::DataLoggerSetupFailure { source: e })?;

    let mut refill = Refill::new(config.refill, sql_interface_refill, database_ping_duration);

    let database_connection_heating_setvals =
        sql_interface
            .get_connection()
            .map_err(|e| StartupError::Database {
                source: Box::new(e),
            })?;
    let mut heating_set_value_updater =
        SqlInterfaceHeatingSetVals::new(database_connection_heating_setvals, &config.heating)
            .map_err(|e| StartupError::Database {
                source: Box::new(e),
            })?;

    let mut heating = Heating::new(
        config.heating,
        sql_interface_heating,
        database_ping_duration,
    );

    let mut feed = Feed::new(config.feed, sql_interface_feed, database_ping_duration);

    let database_connection_ventilation =
        sql_interface
            .get_connection()
            .map_err(|e| StartupError::Database {
                source: Box::new(e),
            })?;
    let mut ventilation_set_value_updater =
        SqlInterfaceVentilationSetVals::new(database_connection_ventilation, &config.ventilation)
            .map_err(|e| StartupError::Database {
            source: Box::new(e),
        })?;

    let mut ventilation = Ventilation::new(config.ventilation);
    let mut balling = Balling::new(
        config.balling,
        sql_interface_balling,
        database_ping_duration,
    )
    .map_err(|e| StartupError::InvalidBallingConfiguration { source: e })?;
    let mut monitors = Monitors::new(config.monitors);
    let mut watchdog = Watchdog::new(config.watchdog);

    let mut ds18b20 = Ds18b20::new(config.ds18b20);

    let relay_manager_channels = RelayManagerChannels {
        tx_relay_manager_to_refill,
        rx_relay_manager_from_refill,
        tx_relay_manager_to_heating,
        rx_relay_manager_from_heating,
        tx_relay_manager_to_ventilation,
        rx_relay_manager_from_ventilation,
        tx_relay_manager_to_balling,
        rx_relay_manager_from_balling,
        tx_relay_manager_to_feed,
        rx_relay_manager_from_feed,
        tx_relay_manager_to_signal_handler,
        rx_relay_manager_from_signal_handler,
    };

    let atlas_scientific_channels = AtlasScientificChannels {
        tx_atlas_scientific_to_i2c_interface,
        rx_atlas_scientific_from_i2c_interface,
        tx_atlas_scientific_to_signal_handler,
        rx_atlas_scientific_from_signal_handler,
    };

    let refill_channels = RefillChannels {
        tx_refill_to_relay_manager,
        rx_refill_from_relay_manager,
        tx_refill_to_signal_handler,
        rx_refill_from_signal_handler,
        tx_refill_to_schedule_check,
        rx_refill_from_schedule_check,
        tx_refill_to_monitors,
        rx_refill_from_messaging_opt,
    };

    let balling_channels = BallingChannels {
        tx_balling_to_relay_manager,
        rx_balling_from_relay_manager,
        tx_balling_to_signal_handler,
        rx_balling_from_signal_handler,
        tx_balling_to_schedule_check,
        rx_balling_from_schedule_check,
        rx_balling_from_messaging_opt,
    };

    let ventilation_channels = VentilationChannels {
        tx_ventilation_to_relay_manager,
        rx_ventilation_from_relay_manager,
        tx_ventilation_to_signal_handler,
        rx_ventilation_from_signal_handler,
        tx_ventilation_to_schedule_check,
        rx_ventilation_from_schedule_check,
        rx_ventilation_from_messaging_opt,
    };

    let feed_channels = FeedChannels {
        tx_feed_to_relay_manager,
        rx_feed_from_relay_manager,
        tx_feed_to_signal_handler,
        rx_feed_from_signal_handler,
        rx_feed_from_messaging_opt,
    };

    let heating_channels = HeatingChannels {
        tx_heating_to_relay_manager,
        rx_heating_from_relay_manager,
        tx_heating_to_signal_handler,
        rx_heating_from_signal_handler,
        tx_heating_to_schedule_check,
        rx_heating_from_schedule_check,
        rx_heating_from_messaging_opt,
    };

    let tcp_communication_channels = TcpCommunicationChannels {
        rx_tcp_communication_from_relay_manager_opt,
        rx_tcp_communication_from_sensor_manager_opt,
        tx_tcp_communication_to_sensor_manager_opt,
        rx_tcp_communication_from_tank_level_switch_opt,
        tx_tcp_communication_to_tank_level_switch_opt,
        tx_tcp_communication_to_signal_handler,
        rx_tcp_communication_from_signal_handler,
    };

    let schedule_check_channels = ScheduleCheckChannels {
        tx_schedule_check_to_ventilation,
        rx_schedule_check_from_ventilation,
        tx_schedule_check_to_heating,
        rx_schedule_check_from_heating,
        tx_schedule_check_to_refill,
        rx_schedule_check_from_refill,
        tx_schedule_check_to_balling,
        rx_schedule_check_from_balling,
        tx_schedule_check_to_signal_handler,
        rx_schedule_check_from_signal_handler,
    };

    let signal_handler_channels = SignalHandlerChannels {
        tx_signal_handler_to_messaging_opt,
        rx_signal_handler_from_messaging_opt,
        tx_signal_handler_to_refill,
        rx_signal_handler_from_refill,
        tx_signal_handler_to_tank_level_switch,
        rx_signal_handler_from_tank_level_switch,
        tx_signal_handler_to_heating,
        rx_signal_handler_from_heating,
        tx_signal_handler_to_relay_manager,
        rx_signal_handler_from_relay_manager,
        tx_signal_handler_to_atlas_scientific,
        rx_signal_handler_from_atlas_scientific,
        tx_signal_handler_to_sensor_manager,
        rx_signal_handler_from_sensor_manager,
        tx_signal_handler_to_data_logger,
        rx_signal_handler_from_data_logger,
        tx_signal_handler_to_balling,
        rx_signal_handler_from_balling,
        tx_signal_handler_to_ventilation,
        rx_signal_handler_from_ventilation,
        tx_signal_handler_to_monitors,
        rx_signal_handler_from_monitors,
        tx_signal_handler_to_feed,
        rx_signal_handler_from_feed,
        tx_signal_handler_to_schedule_check,
        rx_signal_handler_from_schedule_check,
        tx_signal_handler_to_tcp_opt,
        rx_signal_handler_from_tcp_opt,
        tx_signal_handler_to_dht,
        tx_signal_handler_to_i2c_interface,
        tx_signal_handler_to_watchdog,
        rx_signal_handler_from_watchdog,
        tx_signal_handler_to_ds18b20,
    };

    let monitors_channels = MonitorsChannels {
        tx_monitors_to_relay_manager,
        rx_monitors_from_relay_manager,
        rx_monitors_from_refill,
        tx_monitors_to_signal_handler,
        rx_monitors_from_signal_handler,
        rx_monitors_from_messaging_opt,
    };

    #[cfg(target_os = "linux")]
    let messaging_channels = MessagingChannels {
        tx_to_signal_handler: tx_messaging_to_signal_handler,
        rx_from_signal_handler: rx_messaging_from_signal_handler,
        tx_to_refill: tx_messaging_to_refill,
        tx_to_ventilation: tx_messaging_to_ventilation,
        tx_to_heating: tx_messaging_to_heating,
        tx_to_feed: tx_messaging_to_feed,
        tx_to_balling: tx_messaging_to_balling,
        tx_to_monitors: tx_messaging_to_monitors,
        tx_to_watchdog: tx_messaging_to_watchdog,
    };

    scope(|scope| {
        // open tcp connection only if configuration requires use of simulator instead of HW
        if use_simulator {
            // src/main.rs
            let mut tcp_communication = TcpCommunication::new(&config.tcp_connection)
                .map_err(|e| StartupError::TcpConnection { source: e })?;

            scope.spawn(move || {
                tcp_communication.execute(tcp_communication_channels);
            });
            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "TcpCommunication terminated.");
        };

        if config.relay_manager.use_simulator {
            scope.spawn(move || {
                let mut actuate_simulator =
                    ActuateSimulator::new(tx_relay_manager_to_tcp_opt, config.controllino_relay);
                relay_manager.execute(relay_manager_channels, &mut actuate_simulator);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Controllino (simulator) terminated.");
            });
        } else {
            scope.spawn(move || {
                if config.relay_manager.use_gpio {
                    #[cfg(all(feature = "target_hw", target_os = "linux"))]
                    relay_manager.execute(
                        relay_manager_channels,
                        &mut actuate_gpio_opt.expect("Internal error: Illegal configuration (unwrapping of GPIO actuator failed)")
                    );
                } else {
                    relay_manager.execute(relay_manager_channels, &mut actuate_controllino);
                }

                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Controllino (with hardware) terminated.");
            });
        }

        scope.spawn(move || {
            let term = Arc::new(AtomicBool::new(false));

            handle_signals(term, signal_handler_channels);

            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Signal handler terminated.");
        });

        scope.spawn(move || {
            schedule_check.execute(schedule_check_channels);

            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Schedule checker terminated.");
        });

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        if !use_simulator {
            let mut i2c_interface = I2cInterface::new(config.i2c_interface)
                .map_err(|e| StartupError::I2cSetupFailure { source: e })?;

            scope.spawn(move || {
                i2c_interface.execute(
                    tx_i2c_interface_to_atlas_scientific,
                    rx_i2c_interface_from_atlas_scientific,
                    rx_i2c_interface_from_signal_handler,
                );

                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "i2c_interface terminated.");
            });
        }

        scope.spawn(move || {
            atlas_scientific.execute(
                atlas_scientific_channels,
                mutex_atlas_scientific_temperature,
                mutex_atlas_scientific_ph,
                mutex_atlas_scientific_conductivity,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Atlas Scientific terminated.");
        });

        scope.spawn(move || {
            dht.execute(mutex_dht, rx_dht_from_signal_handler);

            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Dht terminated.");
        });

        scope.spawn(move || {
            let sensor_manager_channels = SensorManagerChannels {
                tx_sensor_manager_to_tcp_opt: &tx_sensor_manager_to_tcp_opt,
                rx_sensor_manager_from_tcp_opt: &rx_sensor_manager_from_tcp_opt,
                tx_sensor_manager_to_signal_handler,
                rx_sensor_manager_from_signal_handler,
            };

            let mutexes_sensor_manager = SensorManagerMutexes {
                mutex_ds18b20_ambient_temperature:
                    mutex_ds18b20_ambient_temperature_clone_for_sensor_manager,
                mutex_ds18b20_water_temperature:
                    mutex_ds18b20_water_temperature_clone_for_sensor_manager,
                mutex_dht: mutex_dht_clone_for_sensor_manager,
                mutex_atlas_scientific_temperature:
                    mutex_atlas_scientific_temperature_clone_for_sensor_manager,
                mutex_atlas_scientific_ph: mutex_atlas_scientific_ph_clone_for_sensor_manager,
                mutex_atlas_scientific_conductivity:
                    mutex_atlas_scientific_conductivity_clone_for_sensor_manager,
                mutex_sensor_manager_signals,
            };

            sensor_manager.execute(sensor_manager_channels, mutexes_sensor_manager);

            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Ambient terminated.");
        });

        // give sensor reading components some time so that they can acquire a first data set
        let duration_one_second = Duration::from_secs(1);
        println!("trying to acquire data before starting control in 3 seconds...");
        spin_sleeper.sleep(duration_one_second);
        println!("trying to acquire data before starting control in 2 seconds...");
        spin_sleeper.sleep(duration_one_second);
        println!("trying to acquire data before starting control in 1 second...");
        spin_sleeper.sleep(duration_one_second);
        println!("starting control...");

        scope.spawn(move || {
            let data_logger_channels = DataLoggerChannels {
                tx_to_signal_handler: &tx_data_logger_to_signal_handler,
                rx_from_signal_handler: &rx_data_logger_from_signal_handler,
            };

            let data_logger_mutexes = DataLoggerMutexes {
                mutex_sensor_manager_signals: mutex_sensor_manager_signals_clone_for_data_logger,
                mutex_tank_level_switch_signals:
                    mutex_tank_level_switch_signals_clone_for_data_logger,
                mutex_heating_status: mutex_heating_status_clone_for_data_logger,
                mutex_ventilation_status: mutex_ventilation_status_clone_for_data_logger,
                mutex_refill_status: mutex_refill_status_clone_for_data_logger,
            };

            data_logger.execute(
                &mut data_injection,
                sql_interface_data,
                data_logger_channels,
                data_logger_mutexes,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Data logger terminated.");
        });

        scope.spawn(move || {
            let _ = tank_level_switch.execute(
                &tx_tank_level_switch_to_tcp_opt,
                &rx_tank_level_switch_from_tcp_opt,
                &tx_tank_level_switch_to_signal_handler,
                &rx_tank_level_switch_from_signal_handler,
                mutex_tank_level_switch_signals,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Tank level switch calculation terminated.");
        });

        // Refill needs actuation of relay
        let mutex_device_scheduler_refill = Arc::clone(&mutex_device_scheduler);

        scope.spawn(move || {
            refill.execute(
                mutex_device_scheduler_refill,
                refill_channels,
                &mut water_injection,
                mutex_tank_level_switch_signals_clone_for_refill,
                mutex_refill_status,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Refill terminated.");
        });

        // Heating needs actuation of relay
        let mutex_device_scheduler_heating = Arc::clone(&mutex_device_scheduler);

        let mut heating_stats_transfer = HeatingStatsDataTransfer;

        let heating_mutexes = HeatingMutexes {
            mutex_sensor_manager_signals: mutex_sensor_manager_signals_clone_for_heating,
            mutex_tank_level_switch_signals: mutex_tank_level_switch_signals_clone_for_heating,
            mutex_heating_status,
        };

        scope.spawn(move || {
            heating.execute(
                mutex_device_scheduler_heating,
                heating_channels,
                &mut heating_stats_transfer,
                &mut heating_set_value_updater,
                heating_mutexes,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Heating terminated.");
        });

        // Ventilation needs actuation of relay
        let mutex_device_scheduler_ventilation = Arc::clone(&mutex_device_scheduler);

        scope.spawn(move || {
            ventilation.execute(
                mutex_device_scheduler_ventilation,
                ventilation_channels,
                &mut ventilation_set_value_updater,
                mutex_sensor_manager_signals_clone_for_ventilation,
                mutex_ventilation_status,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Ventilation terminated.");
        });

        // Monitors needs communication with the Controllino device
        let mutex_device_scheduler_monitors = Arc::clone(&mutex_device_scheduler);

        scope.spawn(move || {
            monitors.execute(mutex_device_scheduler_monitors, monitors_channels);

            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Monitors terminated.");
        });

        // Feed needs actuation of relay
        let mutex_device_scheduler_feed = Arc::clone(&mutex_device_scheduler);

        scope.spawn(move || {
            feed.execute(
                mutex_device_scheduler_feed,
                feed_channels,
                &mut food_injection,
            );
            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Feed terminated.");
        });

        // Balling dosing needs actuation of relays
        let mutex_device_scheduler_balling = Arc::clone(&mutex_device_scheduler);

        scope.spawn(move || {
            balling.execute(
                mutex_device_scheduler_balling,
                balling_channels,
                &mut mineral_injection,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Balling terminated.");
        });

        // IPC via messages
        scope.spawn(move || {
            // read/send messages
            #[cfg(target_os = "linux")]
            messaging.execute(messaging_channels);

            // remove PID from the file
            publish_pid.erase_pid();

            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Messaging terminated.");
        });

        // Watchdog communication
        scope.spawn(move || {
            watchdog.execute(
                tx_watchdog_to_signal_handler,
                rx_watchdog_from_signal_handler,
                rx_watchdog_from_messaging_opt,
                &mut petting,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Watchdog terminated.");
        });

        // Ds18b20 communication
        scope.spawn(move || {
            ds18b20.execute(
                rx_ds18b20_from_signal_handler,
                mutex_ds18b20_water_temperature,
                mutex_ds18b20_ambient_temperature,
            );
        });

        Ok(())
    })
}

/// The main function is a simple error handler.
/// The main entry point for the application is within the run() function.
fn main() {
    // setup preliminary logger
    if let Err(e) = setup_logger(LoggerConfig::default()) {
        println!("Error setting up preliminary logger ({e:?})");
        return;
    }

    if let Err(e) = run() {
        log_error_chain(module_path!(), "Application failed to start.", e);
        std::process::exit(1);
    }
}
