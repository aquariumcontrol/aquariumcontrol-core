/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;
use std::fs;

use crate::balling::BallingConfig;
use crate::controllino::ControllinoConfig;
use crate::data_logger::DataLoggerConfig;
use crate::feed::FeedConfig;
use crate::gpio_handler::GpioHandlerConfig;
use crate::heating::HeatingConfig;
use crate::monitors::MonitorsConfig;
use crate::refill::RefillConfig;
use crate::sql_interface::SqlInterfaceConfig;
use crate::tcp_communication::TcpConnectionConfig;
use crate::ventilation::VentilationConfig;

/// Top level struct to hold the configuration data read from .toml file.
/// It does not contain any implementation.
#[derive(Deserialize)]
pub struct ConfigData {
    /// configuration of the SQL database interface
    pub sql_interface: SqlInterfaceConfig,

    /// configuration of the TCP connection (e.g. for simulator)
    pub tcp_connection: TcpConnectionConfig,

    /// configuration of the data logger
    pub data_logger: DataLoggerConfig,

    /// configuration GPIO handler
    pub gpio_handler: GpioHandlerConfig,

    /// configuration of the fresh water refill control
    pub refill: RefillConfig,

    /// configuratino of the heating control
    pub heating: HeatingConfig,

    /// configuration of the ventilation control
    pub ventilation: VentilationConfig,

    /// configuration of the monitors
    pub monitors: MonitorsConfig,

    /// configuration of the Balling dosing control
    pub balling: BallingConfig,

    /// configuration of the feed control
    pub feed: FeedConfig,

    /// configuration of the Controllino
    pub controllino: ControllinoConfig,
}

/// Loads the configuration data described in a struct of type ConfigData from the .toml file.
/// In case of failure, the function panics.
/// In success case, the function returns a variable of type ConfigData.
/// # Arguments:
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
