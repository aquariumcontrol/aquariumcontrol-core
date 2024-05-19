/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//#![allow(warnings)]
mod balling;
mod channel_content;
mod config;
mod controllino;
mod data_logger;
mod feed;
mod feedpattern;
mod gpio_handler;
mod heating;
mod iir_filter;
mod monitors;
mod refill;
mod sawtooth_profile;
mod signal_handler;
mod sql_interface;
mod sql_interface_balling;
mod sql_interface_data;
mod sql_interface_feed;
mod sql_interface_heating;
mod sql_interface_refill;
mod tcp_communication;
mod ventilation;
mod version_information;

use chrono::NaiveDateTime;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::scope;
//use std::env;

use crate::balling::Balling;
use crate::channel_content::InternalCommand;
use crate::config::read_config_file;
use crate::controllino::Controllino;
use crate::data_logger::DataLogger;
use crate::feed::Feed;
use crate::gpio_handler::GpioHandler;
use crate::heating::Heating;
use crate::monitors::Monitors;
use crate::refill::Refill;
use crate::refill::RefillExecution;
use crate::signal_handler::handle_signals;
use crate::sql_interface::{SqlInterface, SqlInterfaceError};
use crate::sql_interface_balling::SqlInterfaceBalling;
use crate::sql_interface_data::SqlInterfaceData;
use crate::sql_interface_feed::SqlInterfaceFeed;
use crate::sql_interface_heating::SqlInterfaceHeating;
use crate::sql_interface_refill::SqlInterfaceRefill;
use crate::tcp_communication::TcpCommunication;
use crate::ventilation::Ventilation;
use crate::version_information::VersionInformation;

fn main() {
    //env::set_var("RUST_BACKTRACE", "1");
    let config = read_config_file("/config/aquarium_control.toml".to_string());

    let version_information = VersionInformation::new();
    println!("Hash of this executable is: {}", version_information.hash);

    let mut sql_interface = match SqlInterface::new(config.sql_interface) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };

    let mut sql_interface_balling = SqlInterfaceBalling::new(sql_interface.get_connection());

    let mut sql_interface_refill = SqlInterfaceRefill::new(sql_interface.get_connection());

    let mut sql_interface_heating = SqlInterfaceHeating::new(sql_interface.get_connection());

    let mut sql_interface_feed = SqlInterfaceFeed::new(sql_interface.get_connection());

    let mut sql_interface_data = SqlInterfaceData::new(sql_interface.get_connection());

    let tables = match sql_interface.get_tables() {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not read list of tables from SQL database: {e:?}");
        }
    };

    match sql_interface.get_hash_from_database(&version_information) {
        Ok(c) => {
            println!(
                "Version information including hash of executable found in database: {}",
                c
            );
            if c != version_information.hash {
                println!("*** WARNING *** hash value for version in database does not match hash version of executable. Proceed with caution.");
            }
        }
        Err(e) => {
            if e == SqlInterfaceError::SingleVersionRequestMultipleResults {
                println!("*** WARNING *** found multiple hash values for version in database. Proceed with caution.");
            } else {
                panic!("Could not find version information in database: {e:?}");
            }
        }
    }

    let duration_balling_dosing =
        match sql_interface_balling.get_duration_since_last_balling_dosing() {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get duration since last Balling dosing event: {e:?}");
            }
        };

    let duration_until_midnight = match sql_interface_heating.get_duration_until_midnight() {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not get duration until midnight: {e:?}");
        }
    };

    let database = match sql_interface.get_database() {
        Ok(c) => c,
        Err(e) => panic!("Could not get database name: {e:?}"),
    };
    println!("Current database name is {}", database);

    match sql_interface.check_required_tables_existing() {
        Ok(_) => {
            println!("Required tables are existing in database.")
        }
        Err(e) => panic!("{e:?}"),
    }

    let refill_timestamp =
        NaiveDateTime::parse_from_str("2022-04-11 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
    match sql_interface_refill.insert_refill_event(refill_timestamp, 3.6, 0.4, 0) {
        Ok(_) => {}
        Err(e) => panic!("Could not insert refill event into database: {e:?}"),
    };

    let balling_timestamp =
        NaiveDateTime::parse_from_str("2022-04-11 11:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
    match sql_interface_balling.insert_balling_event(balling_timestamp, 1, 0.5) {
        Ok(_) => {}
        Err(e) => panic!("Could not insert Balling dosing event into database: {e:?}"),
    }
    let balling_setval_pump1 =
        match sql_interface_balling.get_single_balling_setval_from_database(1) {
            Ok(c) => c,
            Err(e) => panic!("Could not retrieve Balling set values from database: {e:?}"),
        };

    let heating_stats = match sql_interface_heating.get_single_heating_stats_from_database() {
        Ok(c) => c,
        Err(e) => panic!("Could not get heating stats from database:{e:?}"),
    };

    let feedpattern = match sql_interface_feed.get_single_feedpattern_from_database(1) {
        Ok(c) => c,
        Err(e) => panic!("Could not get feed pattern from database:{e:?}"),
    };

    println!("Read the following tables from database:");
    for table in tables {
        println!("* {}", table);
    }
    println!(
        "Duration since last Balling dosing is {} seconds.",
        duration_balling_dosing
    );
    println!(
        "Balling set values are: pump_id={}, dosing_speed={}, dosing_volume={}, label={}",
        balling_setval_pump1.pump_id,
        balling_setval_pump1.dosing_speed,
        balling_setval_pump1.dosing_volume,
        balling_setval_pump1.label
    );
    println!(
        "Duration until midnight is {} seconds.",
        duration_until_midnight
    );
    println!(
        "Heating stats entry: Date={}, Energy={}, Ambient Average={}, Ambient Counter={}, \
        Water Average={}, Water Counter={}, Control         time = {}",
        heating_stats.date.to_string(),
        heating_stats.energy,
        heating_stats.ambient_temperature_average_value,
        heating_stats.ambient_temperature_average_counter,
        heating_stats.water_temperature_average_value,
        heating_stats.water_temperature_average_counter,
        heating_stats.heating_control_runtime
    );
    println!(
        "Feed pattern: profile ID={} profile name={}",
        feedpattern.profile_id, feedpattern.profile_name
    );

    let use_simulator = config.gpio_handler.use_simulator | config.controllino.use_simulator;

    // create channels for communication between threads
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

    // *** heating <=> controllino ***
    let (tx_heating_to_controllino, rx_controllino_from_heating): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_controllino_to_heating, rx_heating_from_controllino): (Sender<bool>, Receiver<bool>) =
        mpsc::channel();
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

    // *** monitors <=> controllino ***
    let (tx_monitors_to_controllino, rx_controllino_from_monitors): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_controllino_to_monitors, rx_monitors_from_controllino): (Sender<bool>, Receiver<bool>) =
        mpsc::channel();
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

    let mut gpio_handler = GpioHandler::new(config.gpio_handler);
    let mut controllino = Controllino::new(config.controllino);

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
        }

        scope.spawn(move || {
            handle_signals(
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
                tx_signal_handler_to_tcp_opt,
                rx_signal_handler_from_tcp_opt,
            );
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
        });

        scope.spawn(move || {
            data_logger.execute(
                sql_interface_data,
                tx_data_logger_to_gpio,
                rx_data_logger_from_gpio,
                rx_datalogger_from_refill,
                rx_datalogger_from_heating,
                rx_datalogger_from_ventilation,
                tx_datalogger_to_signal_handler,
                rx_datalogger_from_signal_handler,
            );
        });

        // Refill needs actuation of relay
        let mutex_device_scheduler_refill = Arc::clone(&mutex_device_scheduler);

        let mut refill_execution = RefillExecution;

        scope.spawn(move || {
            refill.execute(
                mutex_device_scheduler_refill,
                tx_refill_to_gpio,
                rx_refill_from_gpio,
                tx_refill_to_controllino,
                rx_refill_from_controllino,
                tx_refill_to_signal_handler,
                rx_refill_from_signal_handler,
                tx_refill_to_datalogger,
                &mut refill_execution,
            );
        });

        // Heating needs actuation of relay
        let mutex_device_scheduler_heating = Arc::clone(&mutex_device_scheduler);

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
                &mut sql_interface,
            );
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
            );
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
            );
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
            );
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
            );
        });

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
            );
        });
    });
}
