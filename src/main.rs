/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#![deny(missing_docs)]
//! Aquarium control is an open-source application that is part of a control system for salt-water aquariums.
//! It contains the following functionality:
//! - Data acquisition of sensors (water temperature, pH, conductivity, ambient air temperature, ambient humidity)
//! - Fresh water refill
//! - Heating control
//! - Ventilation control
//! - Automatic feeding
//! - Balling mineral dosing

use aquarium_control::launch::run::run;
#[cfg(all(not(test), not(feature = "test-utils")))]
#[cfg(not(tarpaulin_include))]
use aquarium_control::utilities::logger::log_error_chain;
use aquarium_control::utilities::logger::{setup_logger};
use aquarium_control::utilities::logger_config::LoggerConfig;

/// The main function is a simple error handler.
/// The main entry point for the application is within the run() function.
fn main() {
    // setup preliminary logger
    if let Err(e) = setup_logger(LoggerConfig::default()) {
        println!("Error setting up preliminary logger ({e:?})");
        return;
    }

    let run_res = run(None, None);

    if let Err(_e) = run_res {
        #[cfg(all(not(test), not(feature = "test-utils")))]
        #[cfg(not(tarpaulin_include))]
        log_error_chain(
            // not required during testing
            module_path!(),
            "Application failed to start.",
            _e,
        );
        std::process::exit(1);
    }
}
