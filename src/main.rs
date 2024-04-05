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

    let duration = match sql_interface.get_duration_since_last_refill() {
        Ok(c) => c,
        Err(e) => {
            panic!("Could not test maria DB: {e:?}");
        }
    };

    println!("Duration since last refill is {} seconds.", duration);
}
