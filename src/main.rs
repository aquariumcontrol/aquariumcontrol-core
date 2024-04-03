mod config;
mod sql_interface;

use crate::config::read_config_file;
use crate::sql_interface::SqlInterface;

fn main() {
    let config = read_config_file("/config/aquarium_control.toml".to_string());

    let sql_interface = SqlInterface::new(config.sql_interface);

    SqlInterface::testmariadb();
}
