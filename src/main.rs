/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#![deny(missing_docs)]
//#![allow(warnings)]
//! Aquarium control is an open-source application which is part of a control system for salt-water aquariums.
//! It contains the following functionality:
//! - Data acquisition of sensors (water temperature, pH, conductivity)
//! - Fresh water refill
//! - Heating control
//! - Ventilation control
//! - Automatic feeding

mod balling;
mod channel_content;
mod config;
mod controllino;
mod controllino_actuate_hardware;
mod controllino_actuate_simulator;
mod data_injection;
mod data_logger;
mod device_relay_logic;
mod feed;
mod feedpattern;
mod food_injection;
mod gpio_handler;
mod heating;
mod iir_filter;
mod logger;

#[cfg(target_os = "linux")]
mod messaging;
mod mineral_injection;
mod monitors;
mod proc_ext_req;
mod publish_pid;
mod refill;
mod sawtooth_profile;
mod schedule_check;
mod signal_handler;
mod sql_interface;
mod sql_interface_balling;
mod sql_interface_data;
mod sql_interface_feed;
mod sql_interface_heating;
mod sql_interface_refill;
mod sql_interface_schedule;
mod tcp_communication;
mod ventilation;
mod version_information;
mod water_injection;

#[cfg(target_os = "linux")]
mod i2c_interface;

use log::{info, trace, warn};
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::scope;

use crate::balling::Balling;
use crate::channel_content::InternalCommand;
use crate::config::read_config_file;
use crate::controllino::Controllino;
use crate::controllino_actuate_hardware::ControllinoActuateHardware;
use crate::controllino_actuate_simulator::ControllinoActuateSimulator;
use crate::data_injection::DataInjection;
use crate::data_logger::DataLogger;
use crate::feed::Feed;
use crate::food_injection::FoodInjection;
use crate::gpio_handler::GpioHandler;
use crate::heating::Heating;
use crate::heating::HeatingStatsDataTransfer;
use crate::logger::setup_logger;
#[cfg(target_os = "linux")]
use crate::messaging::Messaging;
use crate::mineral_injection::MineralInjection;
use crate::monitors::Monitors;
use crate::publish_pid::PublishPid;
use crate::refill::Refill;
use crate::schedule_check::ScheduleCheck;
use crate::signal_handler::handle_signals;
use crate::sql_interface::{SqlInterface, SqlInterfaceError};
use crate::sql_interface_balling::SqlInterfaceBalling;
use crate::sql_interface_data::SqlInterfaceData;
use crate::sql_interface_feed::SqlInterfaceFeed;
use crate::sql_interface_heating::SqlInterfaceHeating;
use crate::sql_interface_heating::SqlInterfaceHeatingMidnightCalculator;
use crate::sql_interface_refill::SqlInterfaceRefill;
use crate::sql_interface_schedule::SqlInterfaceSchedule;
use crate::tcp_communication::TcpCommunication;
use crate::ventilation::Ventilation;
use crate::version_information::VersionInformation;
use crate::water_injection::WaterInjection;

const LOG_TARGET_MAIN: &str = "main";

#[cfg_attr(doc, aquamarine::aquamarine)]
/// creates the channels and sets up threads.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     refill[Refill] --> gpio_handler[GpioHandler]
///     gpio_handler --> refill
///     refill --> controllino[Controllino]
///     controllino --> refill
///     datalogger[DataLogger] --> gpio_handler
///     gpio_handler --> datalogger
///     refill --> datalogger
///     signal_handler[SignalHandler] --> refill
///     refill --> signal_handler
///     signal_handler --> gpio_handler
///     gpio_handler --> signal_handler
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
///     gpio_handler --> tcp_communication
///     tcp_communication --> gpio_handler
///     balling --> controllino
///     controllino --> balling
///     feed --> controllino
///     controllino --> feed
///     heating --> datalogger
///     heating --> controllino
///     controllino --> heating
///     ventilation --> datalogger
///     ventilation --> controllino
///     controllino --> ventilation
///     monitors[Monitors] --> gpio_handler
///     gpio_handler --> monitors
///     monitors --> controllino
///     controllino --> monitors
///     monitors --> refill
///     refill --> monitors
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
///	messaging[Messaging] --> refill
///	messaging --> ventilation
///	messaging --> heating
///	messaging --> balling
///	messaging --> feed
///	messaging --> monitors
/// ```

fn main() {
    setup_logger();

    //env::set_var("RUST_BACKTRACE", "1");
    let config = read_config_file("/config/aquarium_control.toml".to_string());

    let version_information = VersionInformation::new();
    info!(target: LOG_TARGET_MAIN,
        "hash of this executable is: {}",
        version_information.hash
    );

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

    let sql_interface_refill = SqlInterfaceRefill::new(sql_interface.get_connection());

    let sql_interface_midnight_calculator =
        SqlInterfaceHeatingMidnightCalculator::new(sql_interface.get_connection());

    let sql_interface_heating = SqlInterfaceHeating::new(
        sql_interface.get_connection(),
        Box::new(sql_interface_midnight_calculator),
    );

    let sql_interface_feed = SqlInterfaceFeed::new(sql_interface.get_connection());

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

    let use_simulator = config.gpio_handler.use_simulator | config.controllino.use_simulator;

    // create channels for communication between threads
    // *** signal_handler => messaging ***
    #[cfg(target_os = "linux")]
    let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    #[cfg(not(target_os = "linux"))]
    let (tx_signal_handler_to_messaging, _): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();

    // ****************************

    // *** messaging => refill ***
    #[cfg(target_os = "linux")]
    let (tx_messaging_to_refill, rx_refill_from_messaging): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    #[cfg(not(target_os = "linux"))]
    let (_, rx_refill_from_messaging): (Sender<InternalCommand>, Receiver<InternalCommand>) =
        mpsc::channel();
    // ****************************

    // *** messaging => ventilation ***
    #[cfg(target_os = "linux")]
    let (tx_messaging_to_ventilation, rx_ventilation_from_messaging): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    #[cfg(not(target_os = "linux"))]
    let (_, rx_ventilation_from_messaging): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    // ****************************

    // *** messaging => heating ***
    #[cfg(target_os = "linux")]
    let (tx_messaging_to_heating, rx_heating_from_messaging): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    #[cfg(not(target_os = "linux"))]
    let (_, rx_heating_from_messaging): (Sender<InternalCommand>, Receiver<InternalCommand>) =
        mpsc::channel();
    // ****************************

    // *** messaging => feed ***
    #[cfg(target_os = "linux")]
    let (tx_messaging_to_feed, rx_feed_from_messaging): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    #[cfg(not(target_os = "linux"))]
    let (_, rx_feed_from_messaging): (Sender<InternalCommand>, Receiver<InternalCommand>) =
        mpsc::channel();
    // ****************************

    // *** messaging => balling ***
    #[cfg(target_os = "linux")]
    let (tx_messaging_to_balling, rx_balling_from_messaging): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    #[cfg(not(target_os = "linux"))]
    let (_, rx_balling_from_messaging): (Sender<InternalCommand>, Receiver<InternalCommand>) =
        mpsc::channel();
    // ****************************

    // *** messaging => monitors ***
    #[cfg(target_os = "linux")]
    let (tx_messaging_to_monitors, rx_monitors_from_messaging): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    #[cfg(not(target_os = "linux"))]
    let (_, rx_monitors_from_messaging): (Sender<InternalCommand>, Receiver<InternalCommand>) =
        mpsc::channel();
    // ****************************

    // *** data_logger <=> gpio ***
    let (tx_data_logger_to_gpio, rx_gpio_from_data_logger): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();

    let (tx_gpio_to_data_logger, rx_data_logger_from_gpio): (Sender<f32>, Receiver<f32>) =
        mpsc::channel();
    // ****************************

    // *** refill <=> gpio ***
    let (tx_refill_to_gpio, rx_gpio_from_refill): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_gpio_to_refill, rx_refill_from_gpio): (Sender<f32>, Receiver<f32>) = mpsc::channel();
    // *****************************

    // *** heating <=> gpio ***
    let (tx_heating_to_gpio, rx_gpio_from_heating): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_gpio_to_heating, rx_heating_from_gpio): (Sender<f32>, Receiver<f32>) = mpsc::channel();
    // *****************************

    // *** ventilation <=> gpio ***
    let (tx_ventilation_to_gpio, rx_gpio_from_ventilation): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_gpio_to_ventilation, rx_ventilation_from_gpio): (Sender<f32>, Receiver<f32>) =
        mpsc::channel();
    // *****************************

    // *** monitors <=> gpio ***
    let (tx_monitors_to_gpio, rx_gpio_from_monitors): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_gpio_to_monitors, rx_monitors_from_gpio): (Sender<f32>, Receiver<f32>) =
        mpsc::channel();
    // *****************************

    // *** refill => datalogger ***
    let (tx_refill_to_datalogger, rx_datalogger_from_refill): (Sender<bool>, Receiver<bool>) =
        mpsc::channel();
    // *****************************

    // *** heating => datalogger ***
    let (tx_heating_to_datalogger, rx_datalogger_from_heating): (Sender<bool>, Receiver<bool>) =
        mpsc::channel();
    // *****************************

    // *** ventilation => datalogger ***
    let (tx_ventilation_to_datalogger, rx_datalogger_from_ventilation): (
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
    let (tx_signal_handler_to_gpio, rx_gpio_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_gpio_to_signal_handler, rx_signal_handler_from_gpio): (Sender<bool>, Receiver<bool>) =
        mpsc::channel();
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

    // *** signal handler <=> datalogger ***
    let (tx_signal_handler_to_datalogger, rx_datalogger_from_signal_handler): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_datalogger_to_signal_handler, rx_signal_handler_from_datalogger): (
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

    // *** gpio <=> tcp ***
    let (tx_gpio_to_tcp, rx_tcp_from_gpio): (Sender<InternalCommand>, Receiver<InternalCommand>) =
        mpsc::channel();
    let (tx_tcp_to_gpio, rx_gpio_from_tcp): (Sender<f32>, Receiver<f32>) = mpsc::channel();
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

    let tx_gpio_to_tcp_opt: Option<Sender<InternalCommand>> =
        match config.gpio_handler.use_simulator {
            true => Some(tx_gpio_to_tcp),
            false => None,
        };

    let rx_tcp_from_gpio_opt: Option<Receiver<InternalCommand>> =
        match config.gpio_handler.use_simulator {
            true => Some(rx_tcp_from_gpio),
            false => None,
        };

    let tx_tcp_to_gpio_opt: Option<Sender<f32>> = match config.gpio_handler.use_simulator {
        true => Some(tx_tcp_to_gpio),
        false => None,
    };

    let rx_gpio_from_tcp_opt: Option<Receiver<f32>> = match config.gpio_handler.use_simulator {
        true => Some(rx_gpio_from_tcp),
        false => None,
    };

    let publish_pid = PublishPid::new(config.publish_pid);

    #[cfg(target_os = "linux")]
    let mut messaging = Messaging::new(config.messaging);

    let mut food_injection = FoodInjection::new(&config.feed);
    let mut mineral_injection = MineralInjection::new();

    #[cfg(target_os = "linux")]
    let mut gpio_handler = GpioHandler::new(config.gpio_handler, config.i2c_interface);
    #[cfg(not(target_os = "linux"))]
    let mut gpio_handler = GpioHandler::new(config.gpio_handler);

    let mut controllino = Controllino::new(config.controllino);
    let mut schedule_check = ScheduleCheck::new(config.schedule_check, sql_interface_schedule);

    let mut data_injection = DataInjection;
    let mut data_logger = DataLogger::new(config.data_logger);

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
            let mut tcp_connection = TcpCommunication::new(&config.tcp_connection).unwrap();
            scope.spawn(move || {
                tcp_connection.execute(
                    rx_tcp_from_controllino_opt,
                    rx_tcp_from_gpio_opt,
                    tx_tcp_to_gpio_opt,
                    tx_tcp_to_signal_handler,
                    rx_tcp_from_signal_handler,
                );
            });
        };

        if use_simulator {
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
            });
            trace!(target: LOG_TARGET_MAIN, "Controllino terminated.");
        }

        scope.spawn(move || {
            handle_signals(
                tx_signal_handler_to_messaging,
                tx_signal_handler_to_refill,
                rx_signal_handler_from_refill,
                tx_signal_handler_to_heating,
                rx_signal_handler_from_heating,
                tx_signal_handler_to_controllino,
                rx_signal_handler_from_controllino,
                tx_signal_handler_to_gpio,
                rx_signal_handler_from_gpio,
                tx_signal_handler_to_datalogger,
                rx_signal_handler_from_datalogger,
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
            );
            trace!(target: LOG_TARGET_MAIN, "Signal handler terminated.");
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
            trace!(target: LOG_TARGET_MAIN, "Schedule checker terminated.");
        });

        scope.spawn(move || {
            gpio_handler.execute(
                tx_gpio_to_tcp_opt,
                rx_gpio_from_tcp_opt,
                tx_gpio_to_data_logger,
                rx_gpio_from_data_logger,
                tx_gpio_to_refill,
                rx_gpio_from_refill,
                tx_gpio_to_heating,
                rx_gpio_from_heating,
                tx_gpio_to_ventilation,
                rx_gpio_from_ventilation,
                tx_gpio_to_monitors,
                rx_gpio_from_monitors,
                tx_gpio_to_signal_handler,
                rx_gpio_from_signal_handler,
            );
            trace!(target: LOG_TARGET_MAIN, "GPIO handler terminated.");
        });

        scope.spawn(move || {
            data_logger.execute(
                &mut data_injection,
                sql_interface_data,
                tx_data_logger_to_gpio,
                rx_data_logger_from_gpio,
                rx_datalogger_from_refill,
                rx_datalogger_from_heating,
                rx_datalogger_from_ventilation,
                tx_datalogger_to_signal_handler,
                rx_datalogger_from_signal_handler,
            );
            trace!(target: LOG_TARGET_MAIN, "Data logger terminated.");
        });

        // Refill needs actuation of relay
        let mutex_device_scheduler_refill = Arc::clone(&mutex_device_scheduler);

        let mut water_injection = WaterInjection;

        scope.spawn(move || {
            refill.execute(
                mutex_device_scheduler_refill,
                tx_refill_to_gpio,
                rx_refill_from_gpio,
                tx_refill_to_controllino,
                rx_refill_from_controllino,
                tx_refill_to_signal_handler,
                rx_refill_from_signal_handler,
                tx_refill_to_schedule_check,
                rx_refill_from_schedule_check,
                tx_refill_to_datalogger,
                rx_refill_from_messaging,
                &mut water_injection,
            );
            trace!(target: LOG_TARGET_MAIN, "Refill terminated.");
        });

        // Heating needs actuation of relay
        let mutex_device_scheduler_heating = Arc::clone(&mutex_device_scheduler);

        let mut heating_stats_transfer = HeatingStatsDataTransfer;

        scope.spawn(move || {
            heating.execute(
                mutex_device_scheduler_heating,
                tx_heating_to_gpio,
                rx_heating_from_gpio,
                tx_heating_to_controllino,
                rx_heating_from_controllino,
                tx_heating_to_signal_handler,
                rx_heating_from_signal_handler,
                tx_heating_to_datalogger,
                tx_heating_to_schedule_check,
                rx_heating_from_schedule_check,
                rx_heating_from_messaging,
                &mut sql_interface,
                &mut heating_stats_transfer,
            );
            trace!(target: LOG_TARGET_MAIN, "Heating terminated.");
        });

        // Ventilation needs actuation of relay
        let mutex_device_scheduler_ventilation = Arc::clone(&mutex_device_scheduler);

        scope.spawn(move || {
            ventilation.execute(
                mutex_device_scheduler_ventilation,
                tx_ventilation_to_gpio,
                rx_ventilation_from_gpio,
                tx_ventilation_to_controllino,
                rx_ventilation_from_controllino,
                tx_ventilation_to_signal_handler,
                rx_ventilation_from_signal_handler,
                tx_ventilation_to_datalogger,
                tx_ventilation_to_schedule_check,
                rx_ventilation_from_schedule_check,
                rx_ventilation_from_messaging,
            );
            trace!(target: LOG_TARGET_MAIN, "Ventilation terminated.");
        });

        // Monitors needs communication with Controllino device
        let mutex_device_scheduler_monitors = Arc::clone(&mutex_device_scheduler);

        scope.spawn(move || {
            monitors.execute(
                mutex_device_scheduler_monitors,
                tx_monitors_to_gpio,
                rx_monitors_from_gpio,
                tx_monitors_to_controllino,
                rx_monitors_from_controllino,
                tx_monitors_to_signal_handler,
                rx_monitors_from_signal_handler,
                rx_monitors_from_messaging,
            );
            trace!(target: LOG_TARGET_MAIN, "Monitors terminated.");
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
                rx_feed_from_messaging,
                &mut food_injection,
            );
            trace!(target: LOG_TARGET_MAIN, "Feed terminated.");
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
                rx_balling_from_messaging,
                &mut mineral_injection,
            );
            trace!(target: LOG_TARGET_MAIN, "Balling terminated.");
        });

        // IPC via messages
        scope.spawn(move || {
            // read/send messages
            #[cfg(target_os = "linux")]
            messaging.execute(
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

            trace!(target: LOG_TARGET_MAIN, "Messaging terminated.");
        });
    });
}
