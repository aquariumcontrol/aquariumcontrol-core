mod config;
mod feedpattern;
mod sql_interface;

use chrono::Utc;
//use std::env;

use crate::config::read_config_file;
use crate::sql_interface::HeatingStatsEntry;
use crate::sql_interface::SqlInterface;

fn main() {
    //env::set_var("RUST_BACKTRACE", "1");
    let config = read_config_file("/config/aquarium_control.toml".to_string());

    let mut sql_interface = match SqlInterface::new(config.sql_interface) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
        }
    };

    let tables = match sql_interface.get_tables() {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not read list of tables from SQL database: {e:?}");
        }
    };

    let duration_refill = match sql_interface.get_duration_since_last_refill() {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not get duration since last refill event: {e:?}");
        }
    };

    let duration_balling_dosing = match sql_interface.get_duration_since_last_balling_dosing() {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not get duration since last Balling dosing event: {e:?}");
        }
    };

    let duration_until_midnight = match sql_interface.get_duration_until_midnight() {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not get duration until midnight: {e:?}");
        }
    };

    let refill_volume_last_24h = match sql_interface.get_refill_volume_of_last_24h() {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not get refill volume of last 24 hours: {e:?}");
        }
    };

    let refill_count_last_24h = match sql_interface.get_refill_count_of_last_24h() {
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

    match sql_interface.check_required_tables_existing() {
        Ok(_) => {
            println!("Required tables are existing in database.")
        }
        Err(e) => panic!("{e:?}"),
    }

    match sql_interface.insert_refill_event(Utc::now().naive_utc(), 3.6, 0.4, 0) {
        Ok(_) => {}
        Err(e) => panic!("Could not insert refill event into database: {e:?}"),
    };

    match sql_interface.insert_heating_stats_entry(heating_stats_entry) {
        Ok(_) => {}
        Err(e) => panic!("Could not insert heating statistics into database: {e:?}"),
    };

    match sql_interface.insert_feed_event(Utc::now().naive_utc(), 5.5, "Test".to_string(), 1) {
        Ok(_) => {}
        Err(e) => panic!("Could not insert feed event into database: {e:?}"),
    }

    match sql_interface.insert_balling_event(Utc::now().naive_utc(), 1, 0.5) {
        Ok(_) => {}
        Err(e) => panic!("Could not insert Balling dosing event into database: {e:?}"),
    }

    let heating_stats = match sql_interface.get_single_heating_stats_from_database() {
        Ok(c) => c,
        Err(e) => panic!("Could not get heating stats from database:{e:?}"),
    };

    let feedpattern = match sql_interface.get_single_feedpattern_from_database(1) {
        Ok(c) => c,
        Err(e) => panic!("Could not get feed pattern from database:{e:?}"),
    };

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
}
