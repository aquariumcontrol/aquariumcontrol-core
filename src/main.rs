//#![allow(warnings)]
mod channel_content;
mod config;
mod data_logger;
mod feedpattern;
mod gpio_handler;
mod sql_interface;
mod sql_interface_balling;
mod sql_interface_data;
mod sql_interface_feed;
mod sql_interface_heating;
mod sql_interface_refill;
mod tcp_communication;
mod version_information;

use chrono::NaiveDateTime;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::thread::scope;
//use std::env;

use crate::channel_content::InternalCommand;
use crate::config::read_config_file;
use crate::data_logger::DataLogger;
use crate::gpio_handler::GpioHandler;
use crate::sql_interface::{SqlInterface, SqlInterfaceError};
use crate::sql_interface_balling::SqlInterfaceBalling;
use crate::sql_interface_data::DataFrame;
use crate::sql_interface_data::SqlInterfaceData;
use crate::sql_interface_feed::SqlInterfaceFeed;
use crate::sql_interface_heating::HeatingStatsEntry;
use crate::sql_interface_heating::SqlInterfaceHeating;
use crate::sql_interface_refill::SqlInterfaceRefill;
use crate::tcp_communication::TcpCommunication;
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

    let duration_refill = match sql_interface_refill.get_duration_since_last_refill() {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not get duration since last refill event: {e:?}");
        }
    };

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

    let refill_volume_last_24h = match sql_interface_refill.get_refill_volume_of_last_24h() {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not get refill volume of last 24 hours: {e:?}");
        }
    };

    let refill_count_last_24h = match sql_interface_refill.get_refill_count_of_last_24h() {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not get refill count of last 24 hours: {e:?}");
        }
    };

    let today = match sql_interface.get_current_date() {
        Ok(c) => c,
        Err(_) => panic!("Could not get current date."),
    };

    let heating_stats_entry = HeatingStatsEntry {
        date: today.into(),
        energy: 0.9,
        ambient_temperature_average_value: 22.0,
        ambient_temperature_average_counter: 72000,
        water_temperature_average_value: 25.0,
        water_temperature_average_counter: 78000,
        heating_control_runtime: 64000,
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

    match sql_interface_heating.insert_heating_stats_entry(&heating_stats_entry) {
        Ok(_) => {}
        Err(e) => panic!("Could not insert heating statistics into database: {e:?}"),
    };

    let feed_timestamp =
        NaiveDateTime::parse_from_str("2022-04-11 11:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
    match sql_interface_feed.insert_feed_event(feed_timestamp, 5.5, "Test".to_string(), 1) {
        Ok(_) => {}
        Err(e) => panic!("Could not insert feed event into database: {e:?}"),
    }

    match sql_interface_feed.get_feedschedule_entry_from_database() {
        Ok(c) => match c {
            Some(mut d) => {
                println!(
                    "Feed schedule entry before update: timestamp={}, profile_name={}, repeat_daily={}",
                    d.timestamp,
                    d.profile_name,
                    d.repeat_daily,
                );
                match sql_interface_feed.update_feedschedule_entry_in_database(&mut d) {
                    Ok(()) => {
                        println!("Update feed schedule entry");
                    }
                    Err(e) => {
                        panic!("Error when updating feed schedule entry: {e:?}");
                    }
                }
            }
            None => {
                println!("Database has no feed schedule entry which lies in the past.");
            }
        },
        Err(e) => {
            panic!("Could not retrieve feed schedule information from database: {e:?}");
        }
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

    let current_timestamp = match SqlInterface::get_current_timestamp(&mut sql_interface.conn) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not get current timestamp from database: {e:?}");
        }
    };
    let dataframe = DataFrame {
        timestamp: current_timestamp,
        water_temperature: 25.5,
        water_temperature_filtered: 25.0,
        ph: 7.0,
        ph_filtered: 7.5,
        conductivity: 50000.0,
        conductivity_filtered: 51000.0,
        conductivity_compensated: 52000.0,
        refill_in_progress: false,
        tank_level_switch_position: false,
        tank_level_switch_invalid: false,
        surface_ventilation_status: true,
        ambient_temperature: 22.0,
        ambient_humidity: 60.0,
        heater_status: true,
    };
    match sql_interface_data.write_data_frame_to_database(&dataframe) {
        Ok(()) => {}
        Err(e) => {
            panic!("Could not write dataframe to database: {e:?}");
        }
    }

    println!("Read the following tables from database:");
    for table in tables {
        println!("* {}", table);
    }
    println!("Duration since last refill is {} seconds.", duration_refill);
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
    println!("Refill volume of last 24h is {} L.", refill_volume_last_24h);
    println!("Refill count of last 24h is {}.", refill_count_last_24h);
    println!("Heating stats entry: Date={}, Energy={}, Ambient Average={}, Ambient Counter={}, Water Average={}, Water Counter={}, Control time = {}",
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

    // create channels for communication between threads
    let (tx_data_logger_to_gpio, rx_gpio_from_data_logger): (
        Sender<InternalCommand>,
        Receiver<InternalCommand>,
    ) = mpsc::channel();
    let (tx_gpio_to_data_logger, rx_data_logger_from_gpio): (Sender<f32>, Receiver<f32>) =
        mpsc::channel();

    let data_logger = DataLogger::new(config.data_logger);
    let gpio_handler = GpioHandler::new(config.gpio_handler);

    scope(|scope| {
        let tcp_com_for_gpio_handler = TcpCommunication::new(config.tcp_connection).unwrap();
        scope.spawn(move || {
            gpio_handler.execute(
                tcp_com_for_gpio_handler,
                tx_gpio_to_data_logger,
                rx_gpio_from_data_logger,
            );
        });

        scope.spawn(move || {
            data_logger.execute(
                sql_interface_data,
                tx_data_logger_to_gpio,
                rx_data_logger_from_gpio,
            );
        });
    });
}
