/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the sensor management thread for a DHT22 temperature and humidity sensor.

use aquarium_control::launch::channels::{channel, AquaSender};
use aquarium_control::sensors::dht::{Dht, DhtResult};
use aquarium_control::sensors::dht_channels::DhtChannels;
use aquarium_control::sensors::dht_config::DhtConfig;
use aquarium_control::utilities::channel_content::InternalCommand;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

cfg_if::cfg_if! {
    if #[cfg(all(feature = "target_hw", target_os = "linux"))] {
        use aquarium_control::utilities::logger::setup_logger;
        use aquarium_control::utilities::logger_config::LoggerConfig;
    }
}

#[cfg(all(feature = "target_hw", target_os = "linux"))]
#[test]
// Test case executes a read operation from real hardware.
// Test case does not require any communication with the database.
///

/// [itest->swreq~sensors_read_data~1]
pub fn test_dht_read() {
    setup_logger(LoggerConfig::default()).unwrap();

    let config: ConfigData =
        read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

    // open interface to the GPIO library
    let gpio = match Gpio::new() {
        Ok(c) => c,
        Err(e) => {
            panic!("test_dht_read: could not open interface to GPIO ({e:?})",);
        }
    };

    let gpio_pin_number_dht22_io = config.gpio_handler.dht22_io;
    let gpio_pin_number_dht22_vcc = config.gpio_handler.dht22_vcc;

    let mut dht = Dht::new(
        config.dht,
        Some(gpio),
        gpio_pin_number_dht22_io,
        gpio_pin_number_dht22_vcc,
    );

    let (temperature, humidity) = match dht.get_data() {
        Ok((c, d)) => (c, d),
        Err(e) => {
            panic!("{e:?}");
        }
    };

    println!("temperature={}, humidity={}", temperature, humidity);
}

/// Helper function to create a standard setup for execute function tests.
/// This reduces boilerplate code across multiple tests.
fn setup_execute_test() -> (
    Dht,
    DhtChannels,
    AquaSender<InternalCommand>,
    Arc<Mutex<DhtResult>>,
) {
    let (tx, rx) = channel(1, false);
    let channels = DhtChannels {
        rx_dht_from_signal_handler: rx,
        #[cfg(feature = "debug_channels")]
        cnt_rx_dht_from_signal_handler: 0,
    };
    // Create a default config for testing purposes.
    let config = DhtConfig {
        active: false,
        measurement_interval_millis: 2000,
        execute: true,
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        sensor_init_pin_down_duration_micros: 1000,
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        sensor_init_pin_up_duration_micros: 1000,
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        sensor_pause_duration_millis: 50,
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        sensor_reset_duration_millis: 50,
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        sensor_startup_duration_millis: 50,
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        timeout_duration_micros: 5000,
    };
    let dht = Dht::new(config, None, 0, 0);
    let mutex = Arc::new(Mutex::new(Ok((0.0, 0.0))));

    (dht, channels, tx, mutex)
}

#[test]
/// Verifies that the `execute` loop terminates gracefully when a `Quit` command is received.
///
/// [itest->swreq~sensors_read_data~1]
fn test_execute_handles_quit_command() {
    // Arrange
    let (mut dht, mut channels, mut tx, mutex) = setup_execute_test();

    // Action
    let handle = thread::spawn(move || {
        dht.execute(mutex, &mut channels);
    });
    tx.send(InternalCommand::Quit).unwrap();

    // Assert
    // The thread should join successfully, which proves it terminated as expected.
    let join_result = handle.join();
    assert!(
        join_result.is_ok(),
        "The DHT thread should terminate gracefully on Quit."
    );
}

#[test]
/// Verifies that the `execute` loop continues running after receiving an unrecognized command.
///
/// [itest->swreq~sensors_read_data~1]
fn test_execute_handles_invalid_command() {
    // Arrange
    let (mut dht, mut channels, mut tx, mutex) = setup_execute_test();

    // Action
    let handle = thread::spawn(move || {
        dht.execute(mutex, &mut channels);
    });
    // Send a command that the `execute` function doesn't explicitly handle.
    tx.send(InternalCommand::Stop).unwrap();
    // Give the thread a moment to process the invalid command.
    thread::sleep(Duration::from_millis(150));
    // Now send the Quit command to terminate the loop.
    tx.send(InternalCommand::Quit).unwrap();

    // Assert
    // The thread should not have panicked and should join successfully.
    let join_result = handle.join();
    assert!(
        join_result.is_ok(),
        "The DHT thread should handle invalid commands and continue until Quit."
    );
}
