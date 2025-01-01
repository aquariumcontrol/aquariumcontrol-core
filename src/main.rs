/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#![deny(missing_docs)]
//! Aquarium control is an open-source application which is part of a control system for salt-water aquariums.
//! It contains the following functionality:
//! - Data acquisition of sensors (water temperature, pH, conductivity, ambient air temperature, ambient humidity)
//! - Fresh water refill
//! - Heating control
//! - Ventilation control
//! - Automatic feeding
//! - Balling mineral dosing

/// This is the container module for the database communication.
/// Communicating with SQL data base includes preparing SQL queries, executing the queries and processing of query results.
mod database;

#[cfg(target_os = "linux")]
/// Communication with terminal user or webserver using POSIX message queues.
mod dispatch;

/// This container module features all functionality to execute the automatic feeding.
mod food;

/// This container module features all functionality to execute the Balling mineral dosing.
mod mineral;

/// The container module contains the schedule checker.
mod permission;

/// This container module contains all functionality to periodically store signals either in one table of the SQL databaseor in specific text files stored in RAM-disk
mod recorder;

/// This container module features all functionality to actuate the relays which control the actuators.
mod relays;

/// This container module features all functionality to read and postprocess sensor data.
mod sensors;

/// This container module features all functionality to commiunicate with the test server via TCP.
mod simulator;

/// This container module features a set of mock functionality used by test cases.
mod stubs;

/// This container module features all functionality to either cool the water using ventilation heat the water using a heater.
mod thermal;

/// This container module features all functionality which could not be assigned to any other container module.
mod utilities;

/// This container module features all functionality to monitor the control system.
mod watchmen;

/// This container module features all functionality to replace the evaporated water with fresh water.
mod water;

use log::{error, info, warn};

#[cfg(feature = "debug_main")]
use log::debug;

use spin_sleep::SpinSleeper;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::{env, fmt, panic, path::Path, thread::scope, time::Duration};

use crate::food::feed::Feed;
use crate::food::food_injection::FoodInjection;
use crate::mineral::balling::Balling;
use crate::recorder::data_injection::DataInjection;
use crate::recorder::data_logger::DataLogger;
use crate::relays::controllino::Controllino;
use crate::relays::controllino_actuate_hardware::ControllinoActuateHardware;
use crate::relays::controllino_actuate_simulator::ControllinoActuateSimulator;
use crate::sensors::ambient::Ambient;
use crate::sensors::atlas_scientific::AtlasScientific;
use crate::sensors::gpio_handler::GpioHandler;
use crate::thermal::heating::Heating;
use crate::thermal::heating::HeatingStatsDataTransfer;
use crate::utilities::channel_content::InternalCommand;
use crate::utilities::config::read_config_file;
use crate::utilities::logger::setup_logger;

#[cfg(target_os = "linux")]
use crate::dispatch::messaging::Messaging;

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_balling::SqlInterfaceBalling;
use crate::database::sql_interface_data::SqlInterfaceData;
use crate::database::sql_interface_error::SqlInterfaceError;
use crate::database::sql_interface_feed::{DatabaseInterfaceFeedTrait, SqlInterfaceFeed};
use crate::database::sql_interface_heating::{
    SqlInterfaceHeating, SqlInterfaceHeatingMidnightCalculator,
};
use crate::database::sql_interface_refill::{DatabaseInterfaceRefillTrait, SqlInterfaceRefill};
use crate::database::sql_interface_schedule::SqlInterfaceSchedule;
use crate::mineral::mineral_injection::MineralInjection;
use crate::permission::schedule_check::ScheduleCheck;
use crate::sensors::tank_level_switch::TankLevelSwitch;
use crate::simulator::tcp_communication::TcpCommunication;
use crate::thermal::ventilation::Ventilation;
use crate::utilities::publish_pid::PublishPid;
use crate::utilities::signal_handler::handle_signals;
use crate::utilities::version_information::VersionInformation;
use crate::watchmen::monitors::Monitors;
use crate::water::refill::Refill;
use crate::water::water_injection::WaterInjection;

cfg_if::cfg_if! {
    if #[cfg(all(feature = "target_hw", target_os = "linux"))] {
        use crate::sensors::dht::DhtError;
        use crate::sensors::i2c_interface::{I2cError, I2cInterface};
    }
}

const LOG_TARGET_MAIN: &str = "main";

enum ConfigFileDefinitionError {
    PathDoesNotExist(String),
    IsNotAFile(String),
    FileNameHasNoSuffix(String),
    SuffixIsNotToml(String),
}

impl fmt::Display for ConfigFileDefinitionError {
    /// Function provides output of the error messages wrapped by enum values
    /// # Arguments
    /// * `f` - configuration for formatting    
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ConfigFileDefinitionError::PathDoesNotExist(c) => write!(f, "{}", c),
            ConfigFileDefinitionError::IsNotAFile(c) => write!(f, "{}", c),
            ConfigFileDefinitionError::FileNameHasNoSuffix(c) => write!(f, "{}", c),
            ConfigFileDefinitionError::SuffixIsNotToml(c) => write!(f, "{}", c),
        }
    }
}

/// check if the string provided as contains a valid file name to be used as configuration file
/// # Arguments    
/// * `config_file_name` - reference to string provided by command line parameter
fn check_argument_for_valid_config_file_name(
    relative_config_file_name: &str,
) -> Result<String, ConfigFileDefinitionError> {
    let out_dir = match env::var("CARGO_MANIFEST_DIR") {
        Ok(c) => c,
        Err(_) => {
            error!(target: LOG_TARGET_MAIN,
                "could not read environment variable CARGO_MANIFEST_DIR for checking alternative configuration file. Using fallback path.");
            "/usr/local/src/aquarium_control".to_string()
        }
    };

    let config_file_name = out_dir + relative_config_file_name;

    let path = Path::new(&config_file_name);

    if !path.exists() {
        return Err(ConfigFileDefinitionError::PathDoesNotExist(
            "The provided argument for the configuration file is not an existing file system entry."
            .to_string()
        ));
    }

    if !path.is_file() {
        return Err(ConfigFileDefinitionError::IsNotAFile(
            "The provided argument for the configuration file is not pointing to a file."
                .to_string(),
        ));
    }

    match path.extension() {
        Some(extension) => {
            if extension == "toml" {
                return Ok(String::from(relative_config_file_name));
            } else {
                return Err(ConfigFileDefinitionError::SuffixIsNotToml(
                    "The provided argument for the configuration file does not have extension '.toml'."
                    .to_string()
                ));
            }
        }
        None => {
            return Err(ConfigFileDefinitionError::FileNameHasNoSuffix(
                "The provided argument for the configuration file does not have any extension."
                    .to_string(),
            ));
        }
    }
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// creates the channels and sets up threads.
/// Global thread communication is as follows:
/// ```mermaid
/// graph LR
///     refill[Refill] --> tank_level_switch[TankLevelSwitch]
///     tank_level_switch --> refill
///     refill --> controllino[Controllino]
///     controllino --> refill
///     ambient[Ambient] --> data_logger[DataLogger]
///     data_logger --> ambient
///     refill --> data_logger
///     data_logger --> refill
///     signal_handler[SignalHandler] --> refill
///     refill --> signal_handler
///     signal_handler --> tank_level_switch
///     tank_level_switch --> signal_handler
///     signal_handler --> tcp_communication[TcpCommunication]
///     tcp_communication --> signal_handler
///     signal_handler --> controllino
///     controllino --> signal_handler
///     balling[Balling] --> signal_handler
///     signal_handler --> balling
///     feed[Feed] --> signal_handler
///     signal_handler --> feed
///     heating[Heating] --> signal_handler
///     signal_handler --> heating
///     ventilation[Ventilation] --> signal_handler
///     signal_handler --> ventilation
///     controllino --> tcp_communication
///     ambient --> tcp_communication
///     tcp_communication --> ambient
///     atlas_scientific --> tcp_communication
///     tcp_communication --> atlas_scientific
///     tank_level_switch --> tcp_communication
///     tcp_communication --> tank_level_switch
///     balling --> controllino
///     controllino --> balling
///     feed --> controllino
///     controllino --> feed
///     atlas_scientific --> data_logger
///     data_logger --> atlas_scientific
///     heating --> data_logger
///     data_logger --> heating
///     heating --> atlas_scientific[AtlasScientific]
///     atlas_scientific --> heating
///     heating --> controllino
///     controllino --> heating
///     heating --> tank_level_switch
///     tank_level_switch --> heating
///     heating --> ambient
///     ambient --> heating
///     tank_level_switch --> data_logger
///     data_logger --> tank_level_switch
///     ventilation --> data_logger
///     data_logger --> ventilation
///     signal_handler --> data_logger
///     data_logger --> signal_handler
///     ventilation --> controllino
///     ventilation --> atlas_scientific
///     atlas_scientific --> ventilation
///     controllino --> ventilation
///     monitors[Monitors] --> controllino
///     controllino --> monitors
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
///	signal_handler --> messaging[Messaging]
///     signal_handler --> ambient
///     ambient --> signal_handler
///     signal_handler --> atlas_scientific
///     atlas_scientific --> signal_handler
///     atlas_scientific --> i2c_interface[I2C interface]
///     i2c_interface --> atlas_scientific
///     ambient --> dht[Dht]
///     dht --> ambient
///	    messaging --> refill
///	    messaging --> ventilation
///	    messaging --> heating
///	    messaging --> balling
///	    messaging --> feed
///	    messaging --> monitors
/// ```

fn main() {
    #[cfg(all(not(target_os = "linux"), feature = "target_hw"))]
    compile_error!("Target hardware is only available with Linux operating system.");

    // setup logging to write to log output file and stdout
    setup_logger();

    // get command line parameters from OS
    let args: Vec<String> = std::env::args().collect();

    let config_file_name: String;

    if args.len() > 1 {
        // if first argument contains a path and file name, then let's use that
        config_file_name = match check_argument_for_valid_config_file_name(&args[1]) {
            Ok(valid_file_name) => valid_file_name,
            Err(error_message) => {
                // if user provided an invalid filename, output error message and abort
                panic!("{}: {}", LOG_TARGET_MAIN, error_message);
            }
        }
    } else {
        // use standard file name
        config_file_name = "/config/aquarium_control.toml".to_string();
    }
    // read configuration file for operating the control system
    let config = read_config_file(config_file_name);

    // publish the PID to a file so that client application can communicate with this application
    let publish_pid = PublishPid::new(config.publish_pid);
    let publish_pid_clone = publish_pid.clone();

    panic::set_hook(Box::new(move |panic_info| {
        // custom panic handler: resets the PID to zero before entering in panic
        // this allows subsequent runs of the application without needing to manually
        // clear the PID file.
        // Print the panic message to stderr.
        publish_pid_clone.erase_pid();
        error!("Panic occurred: {}", panic_info);

        // Optionally, you can include more details like:
        if let Some(location) = panic_info.location() {
            error!(
                "Panic occurred in file '{}' at line {}",
                location.file(),
                location.line()
            );
        }
    }));

    // calculate hash of executable
    let version_information = VersionInformation::new();
    info!(target: LOG_TARGET_MAIN,
        "hash of this executable is: {}",
        version_information.hash
    );

    // connect to SQL database, subsequently setup component-specific interfaces
    let mut sql_interface = match SqlInterface::new(config.sql_interface) {
        Ok(c) => c,
        Err(e) => {
            panic!(
                "{}: could not connect to SQL database: {e:?}",
                LOG_TARGET_MAIN
            );
        }
    };

    let sql_interface_schedule = SqlInterfaceSchedule::new(sql_interface.get_connection());

    let sql_interface_balling = SqlInterfaceBalling::new(sql_interface.get_connection());

    let sql_interface_refill: Box<dyn DatabaseInterfaceRefillTrait + Sync + Send> =
        Box::new(SqlInterfaceRefill::new(sql_interface.get_connection()));

    let sql_interface_midnight_calculator =
        SqlInterfaceHeatingMidnightCalculator::new(sql_interface.get_connection());

    let sql_interface_heating = SqlInterfaceHeating::new(
        sql_interface.get_connection(),
        Box::new(sql_interface_midnight_calculator),
    );

    let sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
        Box::new(SqlInterfaceFeed::new(sql_interface.get_connection()));

    let sql_interface_data = SqlInterfaceData::new(sql_interface.get_connection());

    let tables = match sql_interface.get_tables() {
        Ok(c) => c,
        Err(e) => {
            panic!(
                "{} Could not read list of tables from SQL database: {e:?}",
                LOG_TARGET_MAIN
            );
        }
    };

    match sql_interface.get_hash_from_database(&version_information) {
        Ok(c) => {
            info!(
                target: LOG_TARGET_MAIN,
                "version information including hash of executable found in database: {}",
                c
            );
            if c != version_information.hash {
                warn!(
                    target: LOG_TARGET_MAIN,
                    "hash value for version in database does not match hash version of executable. Proceed with caution.");
            }
        }
        Err(e) => {
            if e == SqlInterfaceError::SingleVersionRequestMultipleResults {
                warn!(
                    target: LOG_TARGET_MAIN,
                    "main: Found multiple hash values for version in database. Proceed with caution.");
            } else {
                panic!(
                    "{}: Could not find version information in database: {e:?}",
                    LOG_TARGET_MAIN
                );
            }
        }
    }

    let database = match sql_interface.get_database() {
        Ok(c) => c,
        Err(e) => {
            panic!("{}: Could not get database name ({e:?})", LOG_TARGET_MAIN);
        }
    };
    info!(
        target: LOG_TARGET_MAIN,
        "current database name is {}", database);

    match sql_interface.check_required_tables_existing() {
        Ok(_) => {
            info!(
                target: LOG_TARGET_MAIN,
                "required tables are existing in database.")
        }
        Err(e) => panic!("{e:?}"),
    }

    let mut tables_string: String = "read the following tables from database:".to_string();
    for table in tables {
        tables_string += " ";
        tables_string += &table;
    }
    info!(
        target: LOG_TARGET_MAIN,
        "{}", tables_string);

    // check if any of the components requires use of simulator
    let use_simulator = config.gpio_handler.use_simulator
        | config.controllino.use_simulator
        | config.ambient.use_simulator
        | config.atlas_scientific.use_simulator
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
        }
    }

    // *** data_logger <=> ambient ***
    let (tx_data_logger_to_ambient, rx_ambient_from_data_logger): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_ambient_to_data_logger, rx_data_logger_from_ambient): (Sender<f32>, Receiver<f32>) =
        mpsc::channel();
    // ****************************

    // *** data_logger <=> atlas_scientific ***
    let (tx_data_logger_to_atlas_scientific, rx_atlas_scientific_from_data_logger): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_atlas_scientific_to_data_logger, rx_data_logger_from_atlas_scientific): (
        Sender<f32>,
        Receiver<f32>,
    ) = mpsc::channel();
    // ****************************

    // *** heating <=> atlas_scientific ***
    let (tx_heating_to_atlas_scientific, rx_atlas_scientific_from_heating): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();

    let (tx_atlas_scientific_to_heating, rx_heating_from_atlas_scientific): (
        Sender<f32>,
        Receiver<f32>,
    ) = mpsc::channel();
    // ****************************

    // *** refill <=> tank_level_switch ***
    let (tx_refill_to_tank_level_switch, rx_tank_level_switch_from_refill): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_tank_level_switch_to_refill, rx_refill_from_tank_level_switch): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *****************************

    // *** heating <=> tank_level_switch ***
    let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_tank_level_switch_to_heating, rx_heating_from_tank_level_switch): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *****************************

    // *** data logger <=> tank_level_switch ***
    let (tx_data_logger_to_tank_level_switch, rx_tank_level_switch_from_data_logger): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_tank_level_switch_to_data_logger, rx_data_logger_from_tank_level_switch): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *****************************

    // *** heating <=> ambient ***
    let (tx_heating_to_ambient, rx_ambient_from_heating): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_ambient_to_heating, rx_heating_from_ambient): (Sender<f32>, Receiver<f32>) =
        mpsc::channel();
    // *****************************

    // *** ventilation <=> atlas_scientific ***
    let (tx_ventilation_to_atlas_scientific, rx_atlas_scientific_from_ventilation): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_atlas_scientific_to_ventilation, rx_ventilation_from_atlas_scientific): (
        Sender<f32>,
        Receiver<f32>,
    ) = mpsc::channel();
    // *****************************

    // *** refill <=> data_logger ***
    let (tx_data_logger_to_refill, rx_refill_from_data_logger): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_refill_to_data_logger, rx_data_logger_from_refill): (Sender<bool>, Receiver<bool>) =
        mpsc::channel();
    // *****************************

    // *** heating <=> data_logger ***
    let (tx_data_logger_to_heating, rx_heating_from_data_logger): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_heating_to_data_logger, rx_data_logger_from_heating): (Sender<bool>, Receiver<bool>) =
        mpsc::channel();
    // *****************************

    // *** ventilation <=> data_logger ***
    let (tx_data_logger_to_ventilation, rx_ventilation_from_data_logger): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_ventilation_to_data_logger, rx_data_logger_from_ventilation): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *****************************

    // *** refill <=> controllino ***
    let (tx_refill_to_controllino, rx_controllino_from_refill): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_controllino_to_refill, rx_refill_from_controllino): (Sender<bool>, Receiver<bool>) =
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

    // *** heating <=> controllino ***
    let (tx_heating_to_controllino, rx_controllino_from_heating): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_controllino_to_heating, rx_heating_from_controllino): (Sender<bool>, Receiver<bool>) =
        mpsc::channel();
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

    // *** ventilation <=> controllino ***
    let (tx_ventilation_to_controllino, rx_controllino_from_ventilation): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_controllino_to_ventilation, rx_ventilation_from_controllino): (
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

    // *** feed <=> controllino ***
    let (tx_feed_to_controllino, rx_controllino_from_feed): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_controllino_to_feed, rx_feed_from_controllino): (Sender<bool>, Receiver<bool>) =
        mpsc::channel();
    // *******************************

    // *** balling <=> controllino ***
    let (tx_balling_to_controllino, rx_controllino_from_balling): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_controllino_to_balling, rx_balling_from_controllino): (Sender<bool>, Receiver<bool>) =
        mpsc::channel();
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

    // *** monitors <=> controllino ***
    let (tx_monitors_to_controllino, _rx_controllino_from_monitors): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (_tx_controllino_to_monitors, rx_monitors_from_controllino): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // *******************************

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

    // *** signal handler <=> controllino ***
    let (tx_signal_handler_to_controllino, rx_controllino_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_controllino_to_signal_handler, rx_signal_handler_from_controllino): (
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

    // *** signal handler <=> ambient ***
    let (tx_signal_handler_to_ambient, rx_ambient_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_ambient_to_signal_handler, rx_signal_handler_from_ambient): (
        Sender<bool>,
        Receiver<bool>,
    ) = mpsc::channel();
    // **************************************

    // *** signal handler <=> tcp ***
    let (tx_signal_handler_to_tcp, rx_tcp_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_tcp_to_signal_handler, rx_signal_handler_from_tcp): (Sender<bool>, Receiver<bool>) =
        mpsc::channel();
    // *******************************

    // *** controllino => tcp ***
    let (tx_controllino_to_tcp, rx_tcp_from_controllino): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    // ****************************

    // *** ambient <=> tcp ***
    let (tx_ambient_to_tcp, rx_tcp_from_ambient): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_tcp_to_ambient, rx_ambient_from_tcp): (Sender<f32>, Receiver<f32>) = mpsc::channel();
    // **********************

    // *** atlas_scientific <=> tcp ***
    let (tx_atlas_scientific_to_tcp, rx_tcp_from_atlas_scientific): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_tcp_to_atlas_scientific, rx_atlas_scientific_from_tcp): (Sender<f32>, Receiver<f32>) =
        mpsc::channel();
    // **********************

    // *** tank_level_switch <=> tcp ***
    let (tx_tank_level_switch_to_tcp, rx_tcp_from_tank_level_switch): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_tcp_to_tank_level_switch, rx_tank_level_switch_from_tcp): (Sender<f32>, Receiver<f32>) =
        mpsc::channel();
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

    let tx_controllino_to_tcp_opt: Option<Sender<InternalCommand>> =
        match config.controllino.use_simulator {
            true => Some(tx_controllino_to_tcp),
            false => None,
        };

    let rx_tcp_from_controllino_opt: Option<Receiver<InternalCommand>> =
        match config.controllino.use_simulator {
            true => Some(rx_tcp_from_controllino),
            false => None,
        };

    let tx_tank_level_switch_to_tcp_opt: Option<Sender<InternalCommand>> =
        match config.tank_level_switch.use_simulator {
            true => Some(tx_tank_level_switch_to_tcp),
            false => None,
        };

    let rx_tcp_from_tank_level_switch_opt: Option<Receiver<InternalCommand>> =
        match config.tank_level_switch.use_simulator {
            true => Some(rx_tcp_from_tank_level_switch),
            false => None,
        };

    let tx_tcp_to_tank_level_switch_opt: Option<Sender<f32>> =
        match config.tank_level_switch.use_simulator {
            true => Some(tx_tcp_to_tank_level_switch),
            false => None,
        };

    let rx_tank_level_switch_from_tcp_opt: Option<Receiver<f32>> =
        match config.tank_level_switch.use_simulator {
            true => Some(rx_tank_level_switch_from_tcp),
            false => None,
        };

    let tx_ambient_to_tcp_opt: Option<Sender<InternalCommand>> = match config.ambient.use_simulator
    {
        true => Some(tx_ambient_to_tcp),
        false => None,
    };

    let rx_tcp_from_ambient_opt: Option<Receiver<InternalCommand>> =
        match config.ambient.use_simulator {
            true => Some(rx_tcp_from_ambient),
            false => None,
        };

    let tx_tcp_to_ambient_opt: Option<Sender<f32>> = match config.ambient.use_simulator {
        true => Some(tx_tcp_to_ambient),
        false => None,
    };

    let rx_ambient_from_tcp_opt: Option<Receiver<f32>> = match config.ambient.use_simulator {
        true => Some(rx_ambient_from_tcp),
        false => None,
    };

    let tx_atlas_scientific_to_tcp_opt: Option<Sender<InternalCommand>> =
        match config.atlas_scientific.use_simulator {
            true => Some(tx_atlas_scientific_to_tcp),
            false => None,
        };

    let rx_tcp_from_atlas_scientific_opt: Option<Receiver<InternalCommand>> =
        match config.atlas_scientific.use_simulator {
            true => Some(rx_tcp_from_atlas_scientific),
            false => None,
        };

    let tx_tcp_to_atlas_scientific_opt: Option<Sender<f32>> =
        match config.atlas_scientific.use_simulator {
            true => Some(tx_tcp_to_atlas_scientific),
            false => None,
        };

    let rx_atlas_scientific_from_tcp_opt: Option<Receiver<f32>> =
        match config.atlas_scientific.use_simulator {
            true => Some(rx_atlas_scientific_from_tcp),
            false => None,
        };

    // communication channels between Ambient and Dht
    // platform specific channels for Linux wrapped in Option
    cfg_if::cfg_if! {
        if #[cfg(target_os = "linux")] {
            cfg_if::cfg_if! {
            if #[cfg(feature = "target_hw")] {
                // ambient <=> dht
                let (tx_ambient_to_dht, rx_dht_from_ambient): (
                    Sender<InternalCommand>,
                    Receiver<InternalCommand>,
                ) = mpsc::channel();
                let (tx_dht_to_ambient, rx_ambient_from_dht): (
                    Sender<Result<(f32, f32), DhtError>>,
                    Receiver<Result<(f32, f32), DhtError>>,
                ) = mpsc::channel();
                let tx_signal_handler_to_dht = tx_ambient_to_dht.clone();
                let tx_ambient_to_dht_opt: Option<Sender<InternalCommand>> =
                    Some(tx_ambient_to_dht);
                let rx_ambient_from_dht_opt: Option<Receiver<Result<(f32, f32), DhtError>>> =
                    Some(rx_ambient_from_dht);
                let tx_signal_handler_to_dht_opt: Option<Sender<InternalCommand>> =
                    Some(tx_signal_handler_to_dht);

                // Atlas Scientific <=> i2c_interface
                let (tx_atlas_scientific_to_i2c_interface, rx_i2c_interface_from_atlas_scientific): (
                    Sender<InternalCommand>,
                    Receiver<InternalCommand>,
                ) = mpsc::channel();
                let (tx_i2c_interface_to_atlas_scientific, rx_atlas_scientific_from_i2c_interface): (
                    Sender<Result<(f32, f32, f32), I2cError>>,
                    Receiver<Result<(f32, f32, f32), I2cError>>,
                ) = mpsc::channel();
                let tx_signal_handler_to_i2c_interface = tx_atlas_scientific_to_i2c_interface.clone();
                let tx_atlas_scientific_to_i2c_interface_opt: Option<Sender<InternalCommand>> =
                    Some(tx_atlas_scientific_to_i2c_interface);
                let rx_atlas_scientific_from_i2c_interface_opt: Option<Receiver<Result<(f32, f32, f32), I2cError>>> =
                    Some(rx_atlas_scientific_from_i2c_interface);
                let tx_signal_handler_to_i2c_interface_opt: Option<Sender<InternalCommand>> =
                    Some(tx_signal_handler_to_i2c_interface);
            }
            else {
                // ambient <=> dht and signal handler => dht
                let tx_ambient_to_dht_opt = None;
                let rx_ambient_from_dht_opt = None;
                let tx_signal_handler_to_dht_opt = None;
                // Atlas Scientific <=> i2c_interface and signal handler => i2c_interface
                let tx_atlas_scientific_to_i2c_interface_opt = None;
                let rx_atlas_scientific_from_i2c_interface_opt = None;
                let tx_signal_handler_to_i2c_interface_opt = None;
            }
            }
        }
        else {
            // ambient <=> dht and signal handler => dht
            let tx_ambient_to_dht_opt = None;
            let rx_ambient_from_dht_opt = None;
            let tx_signal_handler_to_dht_opt = None;

            // Atlas Scientific <=> i2c_interface and signal handler => i2c_interface
            let tx_atlas_scientific_to_i2c_interface_opt = None;
            let rx_atlas_scientific_from_i2c_interface_opt = None;
            let tx_signal_handler_to_i2c_interface_opt = None;
        }
    }

    #[cfg(target_os = "linux")]
    let mut messaging = Messaging::new(config.messaging);

    let mut food_injection = FoodInjection::new(&config.feed);
    let mut mineral_injection = MineralInjection::new();
    let mut water_injection = WaterInjection::new(&config.refill);

    cfg_if::cfg_if! {
        if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
            let (_, tank_level_switch_input_pin_opt, dht_opt) =
                GpioHandler::new(config.gpio_handler, config.dht);
        }
        else {
            let _ = GpioHandler::new(config.gpio_handler);
        }
    }

    let mut atlas_scientific = AtlasScientific::new(config.atlas_scientific);
    let mut ambient = Ambient::new(config.ambient);

    let mut controllino = Controllino::new(config.controllino.clone());
    let mut schedule_check = ScheduleCheck::new(config.schedule_check, sql_interface_schedule);

    let mut data_injection = DataInjection;
    let mut data_logger = DataLogger::new(config.data_logger);

    cfg_if::cfg_if! {
        if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
            let mut tank_level_switch = TankLevelSwitch::new(
                config.tank_level_switch,
                true,
                tank_level_switch_input_pin_opt,
            );
        }
        else {
            let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true);
        }
    }

    let mut refill = Refill::new(config.refill, sql_interface_refill);
    let mut heating = Heating::new(config.heating, sql_interface_heating);
    let mut feed = Feed::new(config.feed, sql_interface_feed);
    let mut ventilation = Ventilation::new(config.ventilation);
    let mut balling = Balling::new(config.balling, sql_interface_balling);
    let mut monitors = Monitors::new(config.monitors);

    scope(|scope| {
        let mutex_device_scheduler = Arc::new(Mutex::new(0));

        // open tcp connection only if configuration requires use of simulator instead of HW
        if use_simulator {
            let mut tcp_connection = match TcpCommunication::new(&config.tcp_connection) {
                Ok(c) => c,
                Err(e) => {
                    panic!(
                        "{}: Error occurred when trying to open TCP connection to test server ({e:?}).",
                        LOG_TARGET_MAIN,
                    )
                }
            };
            scope.spawn(move || {
                tcp_connection.execute(
                    rx_tcp_from_controllino_opt,
                    rx_tcp_from_ambient_opt,
                    tx_tcp_to_ambient_opt,
                    rx_tcp_from_atlas_scientific_opt,
                    tx_tcp_to_atlas_scientific_opt,
                    rx_tcp_from_tank_level_switch_opt,
                    tx_tcp_to_tank_level_switch_opt,
                    tx_tcp_to_signal_handler,
                    rx_tcp_from_signal_handler,
                );
            });
            #[cfg(feature = "debug_main")]
            debug!(target: LOG_TARGET_MAIN, "TcpCommunication terminated.");
        };

        if config.controllino.use_simulator {
            scope.spawn(move || {
                controllino.execute(
                    tx_controllino_to_tcp_opt,
                    tx_controllino_to_refill,
                    rx_controllino_from_refill,
                    tx_controllino_to_heating,
                    rx_controllino_from_heating,
                    tx_controllino_to_ventilation,
                    rx_controllino_from_ventilation,
                    tx_controllino_to_balling,
                    rx_controllino_from_balling,
                    tx_controllino_to_feed,
                    rx_controllino_from_feed,
                    tx_controllino_to_signal_handler,
                    rx_controllino_from_signal_handler,
                    &mut ControllinoActuateSimulator,
                );

                #[cfg(feature = "debug_main")]
                debug!(target: LOG_TARGET_MAIN, "Controllino (simulator) terminated.");
            });
        } else {
            scope.spawn(move || {
                controllino.execute(
                    tx_controllino_to_tcp_opt,
                    tx_controllino_to_refill,
                    rx_controllino_from_refill,
                    tx_controllino_to_heating,
                    rx_controllino_from_heating,
                    tx_controllino_to_ventilation,
                    rx_controllino_from_ventilation,
                    tx_controllino_to_balling,
                    rx_controllino_from_balling,
                    tx_controllino_to_feed,
                    rx_controllino_from_feed,
                    tx_controllino_to_signal_handler,
                    rx_controllino_from_signal_handler,
                    &mut ControllinoActuateHardware,
                );

                #[cfg(feature = "debug_main")]
                debug!(target: LOG_TARGET_MAIN, "Controllino (with hardware) terminated.");
            });
        }

        scope.spawn(move || {
            let term = Arc::new(AtomicBool::new(false));
            handle_signals(
                term,
                tx_signal_handler_to_messaging_opt,
                rx_signal_handler_from_messaging_opt,
                tx_signal_handler_to_refill,
                rx_signal_handler_from_refill,
                tx_signal_handler_to_tank_level_switch,
                rx_signal_handler_from_tank_level_switch,
                tx_signal_handler_to_heating,
                rx_signal_handler_from_heating,
                tx_signal_handler_to_controllino,
                rx_signal_handler_from_controllino,
                tx_signal_handler_to_atlas_scientific,
                rx_signal_handler_from_atlas_scientific,
                tx_signal_handler_to_ambient,
                rx_signal_handler_from_ambient,
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
                tx_signal_handler_to_dht_opt,
                tx_signal_handler_to_i2c_interface_opt,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: LOG_TARGET_MAIN, "Signal handler terminated.");
        });

        scope.spawn(move || {
            schedule_check.execute(
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
            );

            #[cfg(feature = "debug_main")]
            debug!(target: LOG_TARGET_MAIN, "Schedule checker terminated.");
        });

        scope.spawn(move || {
            atlas_scientific.execute(
                tx_atlas_scientific_to_tcp_opt,
                rx_atlas_scientific_from_tcp_opt,
                tx_atlas_scientific_to_heating,
                rx_atlas_scientific_from_heating,
                tx_atlas_scientific_to_ventilation,
                rx_atlas_scientific_from_ventilation,
                tx_atlas_scientific_to_data_logger,
                rx_atlas_scientific_from_data_logger,
                tx_atlas_scientific_to_i2c_interface_opt,
                rx_atlas_scientific_from_i2c_interface_opt,
                tx_atlas_scientific_to_signal_handler,
                rx_atlas_scientific_from_signal_handler,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: LOG_TARGET_MAIN, "Atlas Scientific terminated.");
        });

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        if !use_simulator {
            let mut i2c_interface = I2cInterface::new(config.i2c_interface);
            let mut dht = dht_opt.expect("unwrapping of Dht failed");

            scope.spawn(move || {
                dht.execute(
                    tx_dht_to_ambient,
                    rx_dht_from_ambient, // channel is shared with signal handler
                );

                #[cfg(feature = "debug_main")]
                debug!(target: LOG_TARGET_MAIN, "Dht terminated.");
            });

            scope.spawn(move || {
                i2c_interface.execute(
                    tx_i2c_interface_to_atlas_scientific,
                    rx_i2c_interface_from_atlas_scientific,
                );

                #[cfg(feature = "debug_main")]
                debug!(target: LOG_TARGET_MAIN, "i2c_interface terminated.");
            });
        }

        scope.spawn(move || {
            ambient.execute(
                &tx_ambient_to_tcp_opt,
                &rx_ambient_from_tcp_opt,
                tx_ambient_to_heating,
                rx_ambient_from_heating,
                tx_ambient_to_data_logger,
                rx_ambient_from_data_logger,
                tx_ambient_to_dht_opt,
                rx_ambient_from_dht_opt,
                tx_ambient_to_signal_handler,
                rx_ambient_from_signal_handler,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: LOG_TARGET_MAIN, "Ambient terminated.");
        });

        // give Ambient ant AtlasScientific some time so that they can aquire a first data set
        let spin_sleeper = SpinSleeper::default();
        let duration_one_second = Duration::from_secs(1);
        println!("trying to aquire data before starting control in 3 seconds...");
        spin_sleeper.sleep(duration_one_second);
        println!("trying to aquire data before starting control in 2 seconds...");
        spin_sleeper.sleep(duration_one_second);
        println!("trying to aquire data before starting control in 1 second...");
        spin_sleeper.sleep(duration_one_second);
        println!("starting control...");

        scope.spawn(move || {
            data_logger.execute(
                &mut data_injection,
                sql_interface_data,
                tx_data_logger_to_ambient,
                rx_data_logger_from_ambient,
                tx_data_logger_to_atlas_scientific,
                rx_data_logger_from_atlas_scientific,
                tx_data_logger_to_tank_level_switch,
                rx_data_logger_from_tank_level_switch,
                tx_data_logger_to_refill,
                rx_data_logger_from_refill,
                tx_data_logger_to_heating,
                rx_data_logger_from_heating,
                tx_data_logger_to_ventilation,
                rx_data_logger_from_ventilation,
                tx_data_logger_to_signal_handler,
                rx_data_logger_from_signal_handler,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: LOG_TARGET_MAIN, "Data logger terminated.");
        });

        scope.spawn(move || {
            tank_level_switch.execute(
                &tx_tank_level_switch_to_tcp_opt,
                &rx_tank_level_switch_from_tcp_opt,
                &tx_tank_level_switch_to_data_logger,
                &rx_tank_level_switch_from_data_logger,
                &tx_tank_level_switch_to_refill,
                &rx_tank_level_switch_from_refill,
                &tx_tank_level_switch_to_heating,
                &rx_tank_level_switch_from_heating,
                &tx_tank_level_switch_to_signal_handler,
                &rx_tank_level_switch_from_signal_handler,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: LOG_TARGET_MAIN, "Tank level switch calculation terminated.");
        });

        // Refill needs actuation of relay
        let mutex_device_scheduler_refill = Arc::clone(&mutex_device_scheduler);

        scope.spawn(move || {
            refill.execute(
                mutex_device_scheduler_refill,
                tx_refill_to_tank_level_switch,
                rx_refill_from_tank_level_switch,
                tx_refill_to_controllino,
                rx_refill_from_controllino,
                tx_refill_to_signal_handler,
                rx_refill_from_signal_handler,
                tx_refill_to_schedule_check,
                rx_refill_from_schedule_check,
                tx_refill_to_data_logger,
                rx_refill_from_data_logger,
                rx_refill_from_messaging_opt,
                &mut water_injection,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: LOG_TARGET_MAIN, "Refill terminated.");
        });

        // Heating needs actuation of relay
        let mutex_device_scheduler_heating = Arc::clone(&mutex_device_scheduler);

        let mut heating_stats_transfer = HeatingStatsDataTransfer;

        scope.spawn(move || {
            heating.execute(
                mutex_device_scheduler_heating,
                tx_heating_to_ambient,
                rx_heating_from_ambient,
                tx_heating_to_atlas_scientific,
                rx_heating_from_atlas_scientific,
                tx_heating_to_tank_level_switch,
                rx_heating_from_tank_level_switch,
                tx_heating_to_controllino,
                rx_heating_from_controllino,
                tx_heating_to_signal_handler,
                rx_heating_from_signal_handler,
                tx_heating_to_data_logger,
                rx_heating_from_data_logger,
                tx_heating_to_schedule_check,
                rx_heating_from_schedule_check,
                rx_heating_from_messaging_opt,
                &mut heating_stats_transfer,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: LOG_TARGET_MAIN, "Heating terminated.");
        });

        // Ventilation needs actuation of relay
        let mutex_device_scheduler_ventilation = Arc::clone(&mutex_device_scheduler);

        scope.spawn(move || {
            ventilation.execute(
                mutex_device_scheduler_ventilation,
                tx_ventilation_to_atlas_scientific,
                rx_ventilation_from_atlas_scientific,
                tx_ventilation_to_controllino,
                rx_ventilation_from_controllino,
                tx_ventilation_to_signal_handler,
                rx_ventilation_from_signal_handler,
                tx_ventilation_to_data_logger,
                rx_ventilation_from_data_logger,
                tx_ventilation_to_schedule_check,
                rx_ventilation_from_schedule_check,
                rx_ventilation_from_messaging_opt,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: LOG_TARGET_MAIN, "Ventilation terminated.");
        });

        // Monitors needs communication with Controllino device
        let mutex_device_scheduler_monitors = Arc::clone(&mutex_device_scheduler);

        scope.spawn(move || {
            monitors.execute(
                mutex_device_scheduler_monitors,
                tx_monitors_to_controllino,
                rx_monitors_from_controllino,
                tx_monitors_to_signal_handler,
                rx_monitors_from_signal_handler,
                rx_monitors_from_messaging_opt,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: LOG_TARGET_MAIN, "Monitors terminated.");
        });

        // Feed needs actuation of relay
        let mutex_device_scheduler_feed = Arc::clone(&mutex_device_scheduler);

        scope.spawn(move || {
            feed.execute(
                mutex_device_scheduler_feed,
                tx_feed_to_controllino,
                rx_feed_from_controllino,
                tx_feed_to_signal_handler,
                rx_feed_from_signal_handler,
                rx_feed_from_messaging_opt,
                &mut food_injection,
            );
            #[cfg(feature = "debug_main")]
            debug!(target: LOG_TARGET_MAIN, "Feed terminated.");
        });

        // Balling dosing needs actuation of relays
        let mutex_device_scheduler_balling = Arc::clone(&mutex_device_scheduler);

        scope.spawn(move || {
            balling.execute(
                mutex_device_scheduler_balling,
                tx_balling_to_controllino,
                rx_balling_from_controllino,
                tx_balling_to_signal_handler,
                rx_balling_from_signal_handler,
                tx_balling_to_schedule_check,
                rx_balling_from_schedule_check,
                rx_balling_from_messaging_opt,
                &mut mineral_injection,
            );

            #[cfg(feature = "debug_main")]
            debug!(target: LOG_TARGET_MAIN, "Balling terminated.");
        });

        // IPC via messages
        scope.spawn(move || {
            // read/send messages
            #[cfg(target_os = "linux")]
            messaging.execute(
                tx_messaging_to_signal_handler,
                rx_messaging_from_signal_handler,
                tx_messaging_to_refill,
                tx_messaging_to_ventilation,
                tx_messaging_to_heating,
                tx_messaging_to_feed,
                tx_messaging_to_balling,
                tx_messaging_to_monitors,
            );

            // remove PID from file
            publish_pid.erase_pid();

            #[cfg(feature = "debug_main")]
            debug!(target: LOG_TARGET_MAIN, "Messaging terminated.");
        });
    });
}
