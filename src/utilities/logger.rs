/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

const LOG_TARGET_LOGGER: &str = "logger";

/// Sets up and configures the application's logger using the `fern` crate.
///
/// This function initializes a global logger that outputs messages to both
/// standard output (console) and a log file located at `/var/log/aquarium.log`.
/// Log messages are formatted with a timestamp, log level, and target.
/// The default logging level is set to `Debug`.
///
/// # Panics
/// This function will panic if it fails to open or create the log file
/// at `/var/log/aquarium.log`. This typically indicates a permissions
/// issue or an invalid path.
pub fn setup_logger() {
    // open the file for logging
    let log_file = match fern::log_file("/var/log/aquarium.log") {
        Ok(c) => c,
        Err(e) => {
            panic!(
                "{}: Could not open output file for logging: {e:?}",
                LOG_TARGET_LOGGER
            );
        }
    };

    // configure logger
    let _ = fern::Dispatch::new()
        .format(|out, message, record| {
            let local_time = chrono::Local::now(); // Get current local time
            out.finish(format_args!(
                "[{} {} {}] {}",
                local_time.format("%Y-%m-%d %H:%M:%S%.3f %:z"),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(log::LevelFilter::Debug)
        // not utilized:
        //        .level_for("hyper", log::LevelFilter::Info)
        .chain(std::io::stdout()) // output to stdout
        .chain(log_file) // output to file
        .apply();
}
