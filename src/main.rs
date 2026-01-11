/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#![deny(missing_docs)]
//! Aquarium control is an open-source application that is part of a control system for salt-water aquariums.
//! It contains the following functionality:
//! - Data acquisition of sensors (water temperature, pH, conductivity, ambient air temperature, ambient humidity)
//! - Fresh water refill
//! - Heating control
//! - Ventilation control
//! - Automatic feeding
//! - Balling mineral dosing

use log::{error, info, warn};

#[cfg(any(feature = "debug_main", feature = "debug_channels"))]
use log::debug;

use aquarium_control::database::database_interface_feed_trait::DatabaseInterfaceFeedTrait;
use aquarium_control::database::sql_interface::SqlInterface;
use aquarium_control::database::sql_interface_balling::SqlInterfaceBalling;
use aquarium_control::database::sql_interface_data::SqlInterfaceData;
use aquarium_control::database::sql_interface_error::SqlInterfaceError;
use aquarium_control::database::sql_interface_feed::SqlInterfaceFeed;
use aquarium_control::database::sql_interface_heating_set_vals::SqlInterfaceHeatingSetVals;
use aquarium_control::database::sql_interface_heating_stats::SqlInterfaceHeatingStats;
use aquarium_control::database::sql_interface_heating_stats_data_transfer::HeatingStatsDataTransfer;
use aquarium_control::database::sql_interface_midnight::SqlInterfaceMidnightCalculator;
use aquarium_control::database::sql_interface_refill::{DatabaseInterfaceRefillTrait, SqlInterfaceRefill};
use aquarium_control::database::sql_interface_schedule::SqlInterfaceSchedule;
use aquarium_control::database::sql_interface_ventilation_set_vals::SqlInterfaceVentilationSetVals;
#[cfg(target_os = "linux")]
use aquarium_control::dispatch::messaging::Messaging;

use aquarium_control::food::feed::Feed;
use aquarium_control::food::food_injection::FoodInjection;
use aquarium_control::launch::channels::Channels;
use aquarium_control::launch::privilege_check::is_running_as_root;
use aquarium_control::launch::startup_error::StartupError;
use aquarium_control::mineral::balling::Balling;
use aquarium_control::mineral::mineral_injection::MineralInjection;
use aquarium_control::permission::schedule_check::ScheduleCheck;
use aquarium_control::recorder::data_injection::DataInjection;
use aquarium_control::recorder::data_logger::{DataLogger, DataLoggerMutexes};
use aquarium_control::relays::actuate_controllino::ActuateControllino;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use aquarium_control::relays::actuate_gpio::{ActuateGpio, ActuateGpioConfig};
use aquarium_control::relays::actuate_simulator::ActuateSimulator;
use aquarium_control::relays::relay_manager::RelayManager;

#[cfg(all(feature = "target_hw", target_os = "linux"))]
use aquarium_control::sensors::atlas_scientific::AtlasScientific;

use aquarium_control::sensors::atlas_scientific::AtlasScientificResultData;

#[cfg(all(feature = "target_hw", target_os = "linux"))]
use aquarium_control::sensors::dht::Dht;

use aquarium_control::launch::execution_config::ExecutionConfig;
use aquarium_control::sensors::dht::DhtResult;
use aquarium_control::sensors::ds18b20::{Ds18b20, Ds18b20ResultData};
use aquarium_control::sensors::sensor_manager::{SensorManager, SensorManagerMutexes, SensorManagerSignals};
use aquarium_control::sensors::tank_level_switch::{TankLevelSwitch, TankLevelSwitchSignals};
use aquarium_control::simulator::tcp_communication::TcpCommunication;
use aquarium_control::thermal::distinct_interval_check::distinct_thermal_control_interval_check;
use aquarium_control::thermal::heating::{Heating, HeatingMutexes};
use aquarium_control::thermal::ventilation::Ventilation;
use aquarium_control::utilities::config::read_config_file;
use aquarium_control::utilities::logger::{log_error_chain, setup_logger};
use aquarium_control::utilities::logger_config::LoggerConfig;
use aquarium_control::utilities::publish_pid::PublishPid;

#[cfg(all(feature = "target_hw", target_os = "linux"))]
use aquarium_control::utilities::signal_handler;

use aquarium_control::utilities::signal_handler::handle_signals;
use aquarium_control::utilities::version_information::{VersionInformation, VersionInformationError};
use aquarium_control::watchmen::monitors::Monitors;
use aquarium_control::watchmen::petting::Petting;
use aquarium_control::watchmen::watchdog::Watchdog;
use aquarium_control::water::refill::{Refill, RefillStatus};
use aquarium_control::water::water_injection::WaterInjection;
use spin_sleep::SpinSleeper;
pub use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::{env, panic, thread::scope, time::Duration};
use aquarium_control::launch::argument_parser::check_argument_for_valid_config_file_name;
#[cfg(all(feature = "target_hw", target_os = "linux"))]
use aquarium_control::utilities::channel_content::InternalCommand;

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
        use aquarium_control::sensors::gpio_handler::GpioHandler;
        use aquarium_control::beacon::ws2812b::RgbLedColor;
        use aquarium_control::beacon::ws2812b::Ws2812B;
    }
}
cfg_if::cfg_if! {
    if #[cfg(all(feature = "target_hw", target_os = "linux"))] {
        use aquarium_control::sensors::i2c_interface::I2cInterface;
    }
}
use aquarium_control::watchmen::memory::Memory;

cfg_if::cfg_if! {
    if #[cfg(feature = "jemalloc")] {
        use tikv_jemallocator::Jemalloc;
        #[global_allocator]
        static GLOBAL: Jemalloc = Jemalloc;
    }
}

/// Displays the valid command line parameter combinations
fn display_usage() {
    println!("aquarium_control requires a configuration file to be specified to enter operation. Alternatively, the following options are available:");
    println!("-h, --help: Output this page.");
    println!("-v, --version: Output version information including hash of executable.");
}

/// Checks if a command-line argument is a recognized command and executes it.
///
/// This function handles special command-line flags that provide information
/// to the user (e.g., help, version) without starting the main application.
///
/// # Arguments
/// * `command` - A string slice representing the potential command (e.g., "-h", "--version").
///
/// # Returns
/// A `Result` indicating the outcome:
/// - `Ok(true)`: If the argument was a recognized command and was handled successfully.
/// - `Ok(false)`: If the argument was not a recognized command.
///
/// # Errors
/// Returns a `VersionInformationError` if the `-v` or `--version` command is used,
/// but the application fails to retrieve its own version information (e.g., cannot
/// calculate the executable's hash).
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
/// # Returns
/// An empty `Result` (`Ok(())`) on a graceful shutdown. This occurs if the user
/// provides a command-line flag like `-h` or if the application is terminated
/// normally by the signal handler.
///
/// # Errors
/// Returns a `StartupError` if any part of the initialization or setup fails.
/// This error enum covers a wide range of potential issues, including:
/// - `StartupError::Config`: If the configuration file is invalid or cannot be parsed.
/// - `StartupError::LoggingSetupFailure`: If the logger cannot be initialized (e.g., due to file permissions).
/// - `StartupError::NotRoot`: If the application is run without the necessary root privileges.
/// - `StartupError::PidCheckError`: If another instance is already running or the PID file cannot be written.
/// - `StartupError::Database`: If any database connection, query, or setup fails.
/// - `StartupError::GpioHandlerSetupFailure`: If the GPIO interface cannot be initialized on the target hardware.
/// - And various other setup failures for specific modules.
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
    let mut config_file_name = "/etc/aquarium_control/aquarium_control.toml".to_string();

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

    let execution_config = ExecutionConfig::new(&config);
    let execution_config_clone_for_signal_handler = execution_config.clone();
    let execution_config_clone_for_schedule_check = execution_config.clone();
    let execution_config_clone_for_relay_manager = execution_config.clone();
    let execution_config_clone_for_tcp_communication = execution_config.clone();
    #[cfg(target_os = "linux")]
    let execution_config_clone_for_messaging = execution_config.clone();

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
    let publish_pid = PublishPid::new(config.publish_pid)
        .map_err(|e| StartupError::PidCheckError { source: e })?;

    let publish_pid_clone = publish_pid.clone();

    panic::set_hook(Box::new(move |panic_info| {
        // Custom panic handler: resets the PID to zero before entering in panic
        // This allows later runs of the application without needing to manually
        // clear the PID file.
        // Print the panic message to stderr.
        let result_reset_pid = publish_pid_clone.erase_pid();
        error!("Panic occurred: {panic_info}");

        if let Some(location) = panic_info.location() {
            error!(
                "Panic occurred in file '{}' at line {}",
                location.file(),
                location.line()
            );
        }
        // Additionally, log an error if the PID could not be reset
        if let Err(error_reset_pid) = result_reset_pid {
            error!("Could not reset PID file: {error_reset_pid}");
        }
    }));

    // check if the control parameters of heating and ventilation are valid
    if !distinct_thermal_control_interval_check(&config.heating, &config.ventilation) {
        return Err(StartupError::InvalidThermalConfig);
    }

    // instantiate the duration for the database ping
    let database_ping_duration = Duration::from_secs(config.sql_interface.db_ping_interval);

    // get values from config before moving the config
    let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
    let max_rows_feed_profile = config.sql_interface.max_rows_feed_profile;
    let max_rows_feed_schedule = config.sql_interface.max_rows_feed_schedule;
    let max_rows_heating_stats = config.sql_interface.max_rows_heating_stats;
    let max_rows_schedule = config.sql_interface.max_rows_schedule;
    let max_rows_data = config.sql_interface.max_rows_data;
    let max_rows_refill = config.sql_interface.max_rows_refill;
    let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
    let max_rows_feed_log = config.sql_interface.max_rows_feed_log;

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    let atlas_scientific_active = config.atlas_scientific.active;

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
    let mut sql_interface_balling = SqlInterfaceBalling::new(
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
            max_rows_feed_profile,
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
                SqlInterfaceError::SingleVersionRequestMultipleResults(_, _, _, _, _)
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

    let mut channels = Channels::new(
        config.relay_manager.use_simulator,
        config.sensor_manager.use_simulator,
        config.tank_level_switch.use_simulator,
    );

    #[cfg(target_os = "linux")]
    let mut messaging = Messaging::new(config.messaging, execution_config_clone_for_messaging)
        .map_err(|e| StartupError::Messaging { source: e })?;

    let mut food_injection = FoodInjection::new(&config.feed);
    let mut mineral_injection = MineralInjection::new();
    let mut water_injection = WaterInjection::new(&config.refill);
    let mut petting = Petting::new();
    let mut actuate_controllino = ActuateControllino::new(config.controllino_relay.clone())
        .map_err(|e| StartupError::ControllinoSetupFailure { source: e })?;

    // The following structs have a high amount of platform-specific code:
    let mut tank_level_switch: TankLevelSwitch;

    cfg_if::cfg_if! {
        if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
            let mut dht;
            let gpio_dht22_io = config.gpio_handler.dht22_io;
            let gpio_dht22_vcc = config.gpio_handler.dht22_vcc;
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
                if config.relay_manager.use_gpio {
                    let actuate_gpio = ActuateGpio::new(
                        gpio_lib_handle_cloned_for_relay_manager,
                        actuate_gpio_config)
                        .map_err(|e| StartupError::ActuateGpioSetupFailure { source: e })?;
                    actuate_gpio_opt = Some(actuate_gpio);
                }
            }
            let mut i2c_interface = I2cInterface::new(config.i2c_interface)
                .map_err(|e| StartupError::I2cSetupFailure { source: e })?;
        }
        else {
            tank_level_switch = TankLevelSwitch::new(
                config.tank_level_switch,
                true,
            ).map_err(|e| StartupError::TankLevelSwitchSetupFailure { source: e })?;
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

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    let atlas_scientific_drivers = AtlasScientific::create_drivers(&config.atlas_scientific)
        .map_err(|e| StartupError::AtlasScientificSetupFailure { source: e })?;
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    let mut atlas_scientific =
        AtlasScientific::new(config.atlas_scientific, atlas_scientific_drivers);

    let mut sensor_manager = SensorManager::new(config.sensor_manager)
        .map_err(|e| StartupError::SensorManagerSetupFailure { source: e })?;

    let mut relay_manager = RelayManager::new(
        config.relay_manager.clone(),
        execution_config_clone_for_relay_manager,
    );
    let mut schedule_check = ScheduleCheck::new(config.schedule_check, database_ping_duration);

    let mut data_injection = DataInjection;
    let mut data_logger = DataLogger::new(config.data_logger, database_ping_duration)
        .map_err(|e| StartupError::DataLoggerSetupFailure { source: e })?;

    let mut refill = Refill::new(config.refill, database_ping_duration);

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

    let mut heating = Heating::new(config.heating, database_ping_duration);

    let mut feed = Feed::new(config.feed, database_ping_duration);

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
        database_ping_duration,
        &mut sql_interface_balling,
    )
    .map_err(|e| StartupError::InvalidBallingConfiguration { source: e })?;
    let mut monitors = Monitors::new(config.monitors);
    let mut watchdog = Watchdog::new(config.watchdog)
        .map_err(|e| StartupError::WatchdogSetupFailure { source: e })?;
    let mut memory =
        Memory::new(config.memory).map_err(|e| StartupError::InvalidMemoryConfig { source: e })?;

    let mut ds18b20 = Ds18b20::new(config.ds18b20)
        .map_err(|e| StartupError::Ds18b20SetupFailure { source: e })?;

    let mut tcp_communication_opt = None;
    if use_simulator {
        let tcp_communication = TcpCommunication::new(
            &config.tcp_communication,
            execution_config_clone_for_tcp_communication,
        )
        .map_err(|e| StartupError::TcpConnection { source: e })?;
        tcp_communication_opt = Some(tcp_communication);
    }

    let tcp_communication_thread_started = execution_config.tcp_communication;
    let relay_manager_thread_started = execution_config.relay_manager;
    scope(|scope| {
        // spawn thread for sensor communication first to allow checks
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        if !use_simulator && execution_config.i2c_interface {
            scope.spawn(move || {
                i2c_interface.execute(&mut channels.i2c_interface);
                #[cfg(all(target_os = "linux", feature = "target_hw", feature = "debug_channels"))]
                debug!(target: module_path!(), "{0}", channels.i2c_interface);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "i2c_interface terminated.");
            });
        }

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        if !use_simulator && atlas_scientific_active {
            let result = atlas_scientific.init_devices(&mut channels.atlas_scientific);
            if let Err(e) = result {
                // terminate the I2cInterface thread before exiting the application
                signal_handler::send_command(
                    InternalCommand::Quit,
                    Some(&mut channels.signal_handler.tx_signal_handler_to_i2c_interface),
                    None,
                    "i2c_interface",
                );
                return Err(StartupError::AtlasScientificSensorTypeMismatch { source: e });
            }
        }

        // open tcp connection only if configuration requires use of simulator instead of HW
        if use_simulator && tcp_communication_thread_started {
            scope.spawn(move || {
                if let Some(mut tcp_communication) = tcp_communication_opt {
                    tcp_communication.execute(&mut channels.tcp_communication);
                }
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.tcp_communication);
            });
            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "TcpCommunication terminated.");
        };

        if config.relay_manager.use_simulator && relay_manager_thread_started {
            scope.spawn(move || {
                let mut actuate_simulator = ActuateSimulator::new(
                    &mut channels.actuate_simulator,
                    config.controllino_relay,
                );
                relay_manager.execute(&mut channels.relay_manager, &mut actuate_simulator);
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.actuate_simulator);
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.relay_manager);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Controllino (simulator) terminated.");
            });
        } else if relay_manager_thread_started {
            scope.spawn(move || {
                    if config.relay_manager.use_gpio {
                        #[cfg(all(feature = "target_hw", target_os = "linux"))]
                        relay_manager.execute(
                            &mut channels.relay_manager,
                            &mut actuate_gpio_opt.expect("Internal error: Illegal configuration (unwrapping of GPIO actuator failed)")
                        );
                    } else {
                        relay_manager.execute(&mut channels.relay_manager, &mut actuate_controllino);
                    }
                    #[cfg(feature = "debug_channels")]
                    debug!(target: module_path!(), "{0}", channels.relay_manager);
                    #[cfg(feature = "debug_main")]
                    debug!(target: module_path!(), "Controllino (with hardware) terminated.");
                });
        }

        scope.spawn(move || {
            let term = Arc::new(AtomicBool::new(false));

            handle_signals(
                term,
                channels.signal_handler,
                execution_config_clone_for_signal_handler,
            );
            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Signal handler terminated.");
        });

        if execution_config.schedule_check {
            scope.spawn(move || {
                schedule_check.execute(
                    &mut channels.schedule_check,
                    execution_config_clone_for_schedule_check,
                    sql_interface_schedule,
                );
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.schedule_check);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Schedule checker terminated.");
            });
        }

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        if !use_simulator && execution_config.atlas_scientific {
            scope.spawn(move || {
                atlas_scientific.execute(
                    &mut channels.atlas_scientific,
                    mutex_atlas_scientific_temperature,
                    mutex_atlas_scientific_ph,
                    mutex_atlas_scientific_conductivity,
                );
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.atlas_scientific);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Atlas Scientific terminated.");
            });
        }

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        if execution_config.dht {
            scope.spawn(move || {
                dht.execute(mutex_dht, &mut channels.dht);
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.dht);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Dht terminated.");
            });
        }

        if execution_config.sensor_manager {
            scope.spawn(move || {
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

                sensor_manager.execute(&mut channels.sensor_manager, mutexes_sensor_manager);
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.sensor_manager);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "SensorManager terminated.");
            });
        }

        // give sensor reading components some time so that they can acquire a first data set
        let duration_one_second = Duration::from_secs(1);
        println!("trying to acquire data before starting control in 3 seconds...");
        spin_sleeper.sleep(duration_one_second);
        println!("trying to acquire data before starting control in 2 seconds...");
        spin_sleeper.sleep(duration_one_second);
        println!("trying to acquire data before starting control in 1 second...");
        spin_sleeper.sleep(duration_one_second);
        println!("starting control...");

        if execution_config.data_logger {
            scope.spawn(move || {
                let data_logger_mutexes = DataLoggerMutexes {
                    mutex_sensor_manager_signals:
                        mutex_sensor_manager_signals_clone_for_data_logger,
                    mutex_tank_level_switch_signals:
                        mutex_tank_level_switch_signals_clone_for_data_logger,
                    mutex_heating_status: mutex_heating_status_clone_for_data_logger,
                    mutex_ventilation_status: mutex_ventilation_status_clone_for_data_logger,
                    mutex_refill_status: mutex_refill_status_clone_for_data_logger,
                };

                data_logger.execute(
                    &mut data_injection,
                    sql_interface_data,
                    &mut channels.data_logger,
                    data_logger_mutexes,
                );
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.data_logger);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Data logger terminated.");
            });
        }

        if execution_config.tank_level_switch {
            scope.spawn(move || {
                let _ = tank_level_switch.execute(
                    &mut channels.tank_level_switch,
                    mutex_tank_level_switch_signals,
                );
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.tank_level_switch);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Tank level switch calculation terminated.");
            });
        }

        if execution_config.refill {
            // Refill needs actuation of relay
            let mutex_device_scheduler_refill = Arc::clone(&mutex_device_scheduler);

            scope.spawn(move || {
                refill.execute(
                    mutex_device_scheduler_refill,
                    &mut channels.refill,
                    &mut water_injection,
                    mutex_tank_level_switch_signals_clone_for_refill,
                    mutex_refill_status,
                    sql_interface_refill,
                );
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.refill);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Refill terminated.");
            });
        }

        if execution_config.heating {
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
                    &mut channels.heating,
                    &mut heating_stats_transfer,
                    &mut heating_set_value_updater,
                    heating_mutexes,
                    sql_interface_heating,
                );
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.heating);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Heating terminated.");
            });
        }

        if execution_config.ventilation {
            // Ventilation needs actuation of relay
            let mutex_device_scheduler_ventilation = Arc::clone(&mutex_device_scheduler);

            scope.spawn(move || {
                ventilation.execute(
                    mutex_device_scheduler_ventilation,
                    &mut channels.ventilation,
                    &mut ventilation_set_value_updater,
                    mutex_sensor_manager_signals_clone_for_ventilation,
                    mutex_ventilation_status,
                );
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.ventilation);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Ventilation terminated.");
            });
        }

        if execution_config.monitors {
            // Monitors needs communication with the Controllino device
            let mutex_device_scheduler_monitors = Arc::clone(&mutex_device_scheduler);

            scope.spawn(move || {
                monitors.execute(mutex_device_scheduler_monitors, &mut channels.monitors);
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.monitors);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Monitors terminated.");
            });
        }

        if execution_config.feed {
            // Feed needs actuation of relay
            let mutex_device_scheduler_feed = Arc::clone(&mutex_device_scheduler);

            scope.spawn(move || {
                feed.execute(
                    mutex_device_scheduler_feed,
                    &mut channels.feed,
                    &mut food_injection,
                    sql_interface_feed,
                );
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.feed);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Feed terminated.");
            });
        }

        if execution_config.balling {
            // Balling dosing needs actuation of relays
            let mutex_device_scheduler_balling = Arc::clone(&mutex_device_scheduler);

            scope.spawn(move || {
                balling.execute(
                    mutex_device_scheduler_balling,
                    &mut channels.balling,
                    &mut mineral_injection,
                    sql_interface_balling,
                );
                #[cfg(feature = "debug_channels")]
                debug!(target: module_path!(), "{0}", channels.balling);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Balling terminated.");
            });
        }

        // IPC via messages
        scope.spawn(move || {
            // read/send messages
            #[cfg(target_os = "linux")]
            messaging.execute(&mut channels.messaging);

            // remove PID from the file
            let publish_pid_result = publish_pid.erase_pid();
            if let Err(e) = publish_pid_result {
                log_error_chain(
                    module_path!(),
                    "Error occurred when trying to reset PID.",
                    e,
                );
            }
            #[cfg(all(target_os = "linux", feature = "debug_channels"))]
            debug!(target: module_path!(), "{0}", channels.messaging);
            #[cfg(feature = "debug_main")]
            debug!(target: module_path!(), "Messaging terminated.");
        });

        if execution_config.watchdog {
            // Watchdog communication
            scope.spawn(move || {
                watchdog.execute(&mut channels.watchdog, &mut petting);
                #[cfg(all(target_os = "linux", feature = "target_hw", feature = "debug_channels"))]
                debug!(target: module_path!(), "{0}", channels.watchdog);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Watchdog terminated.");
            });
        }

        if execution_config.memory {
            // Memory utilization monitoring
            scope.spawn(move || {
                memory.execute(&mut channels.memory);
                #[cfg(all(target_os = "linux", feature = "target_hw", feature = "debug_channels"))]
                debug!(target: module_path!(), "{0}", channels.memory);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Memory terminated.");
            });
        }

        if execution_config.ds18b20 {
            // Ds18b20 communication
            scope.spawn(move || {
                ds18b20.execute(
                    &mut channels.ds18b20,
                    mutex_ds18b20_water_temperature,
                    mutex_ds18b20_ambient_temperature,
                );
                #[cfg(all(target_os = "linux", feature = "target_hw", feature = "debug_channels"))]
                debug!(target: module_path!(), "{0}", channels.ds18b20);
                #[cfg(feature = "debug_main")]
                debug!(target: module_path!(), "Ds18b20 terminated");
            });
        }
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
