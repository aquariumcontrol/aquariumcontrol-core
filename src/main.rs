mod config;
mod sql_interface;

use crate::config::read_config_file;
use crate::sql_interface::SqlInterface;

fn main() {
    let config = read_config_file("/config/aquarium_control.toml".to_string());

    let mut sql_interface = match SqlInterface::new(config.sql_interface) {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not connect to SQL database: {e:?}");
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
    
    println!("Duration since last refill is {} seconds.", duration_refill);
    println!(
        "Duration since last Balling dosing is {} seconds.",
        duration_balling_dosing
    );
    println!("Duration until midnight is {} seconds.", duration_until_midnight);
}
