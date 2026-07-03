/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::test_server::ambient_temperature_time_profile::AmbientTemperatureTimeProfileConfig;
use crate::test_server::conductivity_reverse_compensation::ConductivityReverseCompensationConfig;
use crate::test_server::controllino_emulator::ControllinoEmulatorConfig;
use crate::test_server::evaporation::EvaporationConfig;
use crate::test_server::heat_flow::HeatFlowConfig;
use crate::test_server::light_intensity_time_profile::LightIntensityTimeProfileConfig;
use crate::test_server::pH_time_profile::pHTimeProfileConfig;
use crate::test_server::sump::SumpConfig;
use crate::test_server::tank_geometry::TankGeometryConfig;
use crate::test_server::tank_level_position::TankLevelPositionConfig;
use crate::test_server::tcp_connection_config::TcpConnectionConfig;
use crate::test_server::thermal_model::TankTemperatureModelConfig;
use serde_derive::Deserialize;
use std::fs;

/// This is the top level struct to hold the configuration data read from .toml file.
/// It does not contain any implementation.
#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct TestServerConfigData {
    pub controllino: ControllinoEmulatorConfig,
    pub conductivity_reverse_compensation: ConductivityReverseCompensationConfig,
    pub evaporation: EvaporationConfig,
    pub pH_time_profile: pHTimeProfileConfig,
    pub sump: SumpConfig,
    pub tank_geometry: TankGeometryConfig,
    pub ambient_temperature_time_profile: AmbientTemperatureTimeProfileConfig,
    pub tank_level_switch: TankLevelPositionConfig,
    pub light_intensity_time_profile: LightIntensityTimeProfileConfig,
    pub heat_flow: HeatFlowConfig,
    pub tank_temperature_model: TankTemperatureModelConfig,
    pub tcp_connection: TcpConnectionConfig,
}

/// The function loads the configuration data described in a struct of type TestServerConfigData from the .toml file.
/// In case of failure, the function panics.
/// In success case, the function returns a variable of type TestServerConfigData.
/// # Arguments:
///
/// * `user_specified_file` - Optional path to the configuration file. If None, a default path is used based on CARGO_MANIFEST_DIR.
pub fn read_test_server_config_file(user_specified_file: Option<String>) -> TestServerConfigData {
    use std::env;

    let filename = if let Some(f) = user_specified_file {
        f
    } else {
        match env::var("CARGO_MANIFEST_DIR") {
            Ok(dir) => dir + "/config-test/aquarium_control_test_server.toml",
            Err(_) => "/etc/aquarium_control/aquarium_control_test_server.toml".to_string(),
        }
    };

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
