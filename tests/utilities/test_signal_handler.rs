/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the application's response to operating system termination signals

use aquarium_control::launch::channels::Channels;
use aquarium_control::launch::execution_config::ExecutionConfig;
use aquarium_control::utilities::channel_content::InternalCommand;
use aquarium_control::utilities::signal_handler::handle_signals;
use spin_sleep::SpinSleeper;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{process::Command, sync::Arc, thread};

/// Helper function to send a signal to the application
///
/// # Arguments
/// * `pid` - The process ID (PID) of the application.
/// * `signal` - A parameter representing `signal`.
fn send_signal(pid: u32, signal: i32) {
    Command::new("kill")
        .args([pid.to_string().as_str()])
        .output()
        .expect("Failed to send signal");
    println!("Sent signal {} to {}", signal, pid);
}

/// test if the signal handler sends shutdown requests to all threads in the right order.
#[cfg(any(test, feature = "test-utils"))]
#[test]
// [itest->swreq~utilities_process_signals~1]
fn test_signal_handling() {
    let term = Arc::new(AtomicBool::new(false));
    let term_clone = Arc::clone(&term);

    let mut channels = Channels::new_for_test_for_signal_handler();

    let join_handle_test_object = thread::Builder::new()
        .name("test_object".to_string())
        .spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(1)
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    // blocking execution used for testing
                    handle_signals(
                        term_clone,
                        channels.signal_handler,
                        ExecutionConfig::default(),
                        None,
                    )
                    .await;
                });
        })
        .unwrap();

    // use pid of the current process
    let pid = std::process::id();

    let spin_sleeper = SpinSleeper::default();
    let sleep_duration_one_second = Duration::from_secs(1);

    spin_sleeper.sleep(sleep_duration_one_second);

    send_signal(pid, signal_hook::consts::signal::SIGABRT);

    assert_eq!(
        channels
            .data_logger
            .rx_data_logger_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    _ = channels
        .data_logger
        .tx_data_logger_to_signal_handler
        .send(true);

    assert_eq!(
        channels
            .messaging
            .rx_messaging_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    _ = channels.messaging.tx_messaging_to_signal_handler.send(true);

    assert_eq!(
        channels
            .refill
            .rx_refill_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    _ = channels.refill.tx_refill_to_signal_handler.send(true);

    assert_eq!(
        channels
            .heating
            .rx_heating_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    _ = channels.heating.tx_heating_to_signal_handler.send(true);

    assert_eq!(
        channels
            .ventilation
            .rx_ventilation_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    _ = channels
        .ventilation
        .tx_ventilation_to_signal_handler
        .send(true);

    assert_eq!(
        channels
            .feed
            .rx_feed_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    _ = channels.feed.tx_feed_to_signal_handler.send(true);

    assert_eq!(
        channels
            .balling
            .rx_balling_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    _ = channels.balling.tx_balling_to_signal_handler.send(true);

    assert_eq!(
        channels
            .tank_level_switch
            .rx_tank_level_switch_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    _ = channels
        .tank_level_switch
        .tx_tank_level_switch_to_signal_handler
        .send(true);

    assert_eq!(
        channels
            .atlas_scientific
            .rx_atlas_scientific_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );

    _ = channels
        .atlas_scientific
        .tx_atlas_scientific_to_signal_handler
        .send(true);

    assert_eq!(
        channels
            .sensor_manager
            .rx_sensor_manager_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );

    _ = channels
        .sensor_manager
        .tx_sensor_manager_to_signal_handler
        .send(true);
    assert_eq!(
        channels
            .temperature_gradient
            .rx_temperature_gradient_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    _ = channels
        .temperature_gradient
        .tx_temperature_gradient_to_signal_handler
        .send(true);
    assert_eq!(
        channels
            .schedule_check
            .rx_schedule_check_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    _ = channels
        .schedule_check
        .tx_schedule_check_to_signal_handler
        .send(true);
    assert_eq!(
        channels
            .i2c_interface
            .rx_i2c_interface_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    // no response from i2c_interface
    assert_eq!(
        channels
            .dht
            .rx_dht_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    // no response from dht

    assert_eq!(
        channels
            .relay_manager
            .rx_relay_manager_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    _ = channels
        .relay_manager
        .tx_relay_manager_to_signal_handler
        .send(true);

    assert_eq!(
        channels
            .monitors
            .rx_monitors_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    _ = channels.monitors.tx_monitors_to_signal_handler.send(true);

    assert_eq!(
        channels
            .data_logger
            .rx_data_logger_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Terminate
    );
    // no response to termination request

    assert_eq!(
        channels
            .heating
            .rx_heating_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Terminate
    );
    // no response to termination request

    assert_eq!(
        channels
            .ventilation
            .rx_ventilation_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Terminate
    );
    // no response to termination request

    assert_eq!(
        channels
            .feed
            .rx_feed_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Terminate
    );
    // no response to termination request

    assert_eq!(
        channels
            .refill
            .rx_refill_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Terminate
    );
    // no response to termination request

    assert_eq!(
        channels
            .balling
            .rx_balling_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Terminate
    );
    // no response to termination request

    assert_eq!(
        channels
            .ds18b20
            .rx_ds18b20_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Quit
    );
    // no response to quit signal from ds18b20

    assert_eq!(
        channels
            .atlas_scientific
            .rx_atlas_scientific_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Terminate
    );
    // no response to termination request

    assert_eq!(
        channels
            .sensor_manager
            .rx_sensor_manager_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Terminate
    );
    // no response to termination request

    assert_eq!(
        channels
            .tank_level_switch
            .rx_tank_level_switch_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Terminate
    );
    // no response to termination request

    assert_eq!(
        channels
            .watchdog
            .rx_watchdog_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Terminate
    );
    _ = channels.watchdog.tx_watchdog_to_signal_handler.send(true);

    assert_eq!(
        channels
            .messaging
            .rx_messaging_from_signal_handler
            .blocking_recv()
            .unwrap(),
        InternalCommand::Terminate
    );
    // no response to termination request

    join_handle_test_object
        .join()
        .expect("Test object thread did not finish.");

    assert!(term.load(Ordering::Acquire));
}
