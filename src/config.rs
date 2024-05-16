use serde_derive::Deserialize;
use std::fs;

use crate::controllino::ControllinoConfig;
use crate::data_logger::DataLoggerConfig;
use crate::gpio_handler::GpioHandlerConfig;
use crate::heating::HeatingConfig;
use crate::refill::RefillConfig;
use crate::sql_interface::SqlInterfaceConfig;
use crate::tcp_communication::TcpConnectionConfig;

/// This is the top level struct to hold the configuration data read from .toml file.
/// It does not contain any implementation.
#[derive(Deserialize)]
pub struct ConfigData {
    pub sql_interface: SqlInterfaceConfig,
    pub tcp_connection: TcpConnectionConfig,
    pub data_logger: DataLoggerConfig,
    pub gpio_handler: GpioHandlerConfig,
    pub refill: RefillConfig,
    pub heating: HeatingConfig,
    pub controllino: ControllinoConfig,
}

/// The function loads the configuration data described in a struct of type ConfigData from the .toml file.
/// In case of failure, the function panics.
/// In success case, the function returns a variable of type ConfigData.
/// # Arguments:
///
/// * `relative_path` - configuration file shall be stored in a subfolder of the cargo base folder. The variable contains the name of the subfolder.
pub fn read_config_file(relative_path: String) -> ConfigData {
    use std::env;
    let out_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    let filename = out_dir + &relative_path;

    println!("Reading config in {}", filename);
    // inspired by https://codingpackets.com/blog/rust-load-a-toml-file/
    // Read the contents of the file using a `match` block
    // to return the `data: Ok(c)` as a `String`
    // or handle any `errors: Err(_)`.
    let contents = match fs::read_to_string(filename.clone()) {
        // If successful return the files text as `contents`.
        // `c` is a local variable.
        Ok(c) => c,
        // Handle the `error` case.
        Err(_) => {
            // Write `msg` to `stderr`.
            panic!("Could not read file `{}`", filename);
        }
    };

    // Use a `match` block to return the
    // file `contents` as a `Data struct: Ok(d)`
    // or handle any `errors: Err(_)`.
    match toml::from_str(&contents) {
        // If successful, return data as `Data` struct.
        // `d` is a local variable.
        Ok(d) => d,
        // Handle the `error` case.
        Err(e) => {
            // Write `msg` to `stderr`.
            panic!("Unable to load data from `{filename}`:{e}");
        }
    }
}
