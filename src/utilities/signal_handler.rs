/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(any(feature = "debug_signal_handler", feature = "debug_channels"))]
use log::debug;
use log::error;
#[cfg(any(target_os = "linux", feature = "debug_signal_handler", not(test)))]
use log::info;

#[cfg(target_os = "linux")]
use nix::unistd::gettid;

use crate::launch::channels::AquaSender;
use crate::launch::execution_config::ExecutionConfig;
use crate::utilities::channel_content::InternalCommand;
pub(crate) use crate::utilities::signal_handler_channels::SignalHandlerChannels;
use spin_sleep::SpinSleeper;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::{sync::Arc, time::Duration};

/// Sends a specified `InternalCommand` to a target thread's channel and optionally waits for an acknowledgment.
///
/// The signal handler uses this helper function to communicate termination
/// or other control commands to various application threads. It attempts to send
/// the command and, if a receiver channel is provided, waits for a boolean acknowledgment
/// to confirm the command was processed.
///
/// # Arguments
/// * `command` - The `InternalCommand` to be sent to the target thread.
/// * `tx_to_thread_opt` - An `Option` containing the sender part of the channel
///   for communication to the target thread. If `None`, no command is sent.
/// * `rx_from_thread_opt` - An `Option` containing the receiver part of the channel
///   for communication *from* the target thread, used to receive an acknowledgment.
///   If `None`, no acknowledgment is expected or waited for.
/// * `thread_name` - The name of the target thread (for logging and debugging purposes).
fn send_command(
    command: InternalCommand,
    tx_to_thread_opt: Option<&mut AquaSender<InternalCommand>>,
    rx_from_thread_opt: Option<&Receiver<bool>>,
    thread_name: &str,
) {
    match tx_to_thread_opt {
        Some(tx_to_thread) => {
            match tx_to_thread.send(command) {
                Ok(_) => {
                    #[cfg(feature = "debug_signal_handler")]
                    debug!(target: module_path!(), "QUIT sent to {}", thread_name);

                    match rx_from_thread_opt {
                        Some(rx_from_thread) => match rx_from_thread.recv() {
                            Ok(_) => {
                                #[cfg(feature = "debug_signal_handler")]
                                debug!(target: module_path!(), "received acknowledgement from {}", thread_name);
                            }
                            Err(e) => {
                                error!(
                                    target: module_path!(),
                                    "could not receive answer from {thread_name} thread {e:?}"
                                );
                            }
                        },
                        None => { /* do nothing */ }
                    };
                }
                Err(e) => {
                    error!(
                        target: module_path!(),
                        "channel communication to {thread_name} thread failed when sending Quit command ({e:?})"
                    );
                }
            }
        }
        None => {
            /* do nothing */
            #[cfg(feature = "debug_signal_handler")]
            {
                let message = format!("not sending QUIT to {thread_name}");
                debug!(target: module_path!(), "{message}");
            }
        }
    }
}

/// Handles operating system termination signals (e.g., SIGTERM).
///
/// Upon receiving a termination signal, this function orchestrates a **safe, multiphase shutdown**
/// of all application threads. It sends specific `InternalCommand` messages (`Quit` then `Terminate`)
/// to each module in a predetermined order, ensuring dependencies are gracefully
/// shut down before their consumers.
///
/// # Arguments
/// * `term` - An atomic boolean flag that becomes `true` when a termination signal is received
///   from the operating system. The function continuously monitors this flag.
/// * `signal_handler_channels` - A struct containing all necessary `mpsc` sender and receiver
///   channel pairs for communicating with different application threads and modules.
///
/// # Panics
/// * If `signal_handler_channels` contains an inconsistent setup for TCP communication
///   (e.g., a sender but no receiver, or vice versa).
/// * If the function fails to register a hook for the OS termination signals.
///
/// # Thread Communication Flow
/// The controlled shutdown sequence is visualized below:
#[cfg_attr(doc, aquamarine::aquamarine)]
/// ```mermaid
/// graph LR
///     signal_handler[Signal handler] --> messaging[Messaging]
///     signal_handler --> refill[Refill control]
///     refill --> signal_handler
///     signal_handler --> tank_level_switch[Tank Level Switch]
///     tank_level_switch --> signal_handler
///     signal_handler --> heating[Heating control]
///     heating --> signal_handler
///     signal_handler --> relay_manager[Relay Manager]
///     relay_manager --> signal_handler
///     signal_handler --> atlas_scientific[Atlas Scientific]
///     atlas_scientific --> signal_handler
///     signal_handler --> ambient[Ambient]
///     ambient --> signal_handler
///     signal_handler --> data_logger[Data logger]
///     data_logger --> signal_handler
///     signal_handler --> balling[Balling dosing control]
///     balling --> signal_handler
///     signal_handler --> ventilation[Ventilation control]
///     ventilation --> signal_handler
///     signal_handler --> monitors[Monitors]
///     monitors --> signal_handler
///     signal_handler --> feed[Feed control]
///     feed --> signal_handler
///     schedule_check[ScheduleCheck] --> signal_handler
///     signal_handler --> schedule_check
///     signal_handler --> tcp_communication[TCP communication]
///     tcp_communication --> signal_handler
///     signal_handler --> dht[Dht]
///     signal_handler --> i2c_interface[I2C interface]
///     signal_handler --> watchdog[Watchdog]
///     watchdog --> signal_handler
/// ```
pub fn handle_signals(
    term: Arc<AtomicBool>,
    mut signal_handler_channels: SignalHandlerChannels,
    execution_config: ExecutionConfig,
) {
    #[cfg(target_os = "linux")]
    info!(target: module_path!(), "Thread started with TID: {}", gettid());

    if signal_handler_channels
        .tx_signal_handler_to_tcp_opt
        .is_some()
        != signal_handler_channels
            .rx_signal_handler_from_tcp_opt
            .is_some()
    {
        panic!(
            "{}: inconsistent channel parameters for TcpCommunication provided",
            module_path!()
        );
    }
    let use_simulator = signal_handler_channels
        .tx_signal_handler_to_tcp_opt
        .is_some();

    let spin_sleeper = SpinSleeper::default();
    let sleep_interval_millis = 100;
    let sleep_duration_hundred_milli_sec = Duration::from_millis(sleep_interval_millis);

    for signal in signal_hook::consts::TERM_SIGNALS {
        match signal_hook::flag::register(*signal, Arc::clone(&term)) {
            Ok(_) => {
                #[cfg(feature = "debug_signal_handler")]
                info!(
                    target: module_path!(),
                    "registered hook for signal {}",
                    signal
                );
            }
            Err(e) => {
                panic!("{}: Could not hook SIGTERM routine: {e:?}", module_path!());
            }
        }
    }

    while !term.load(Ordering::Relaxed) {
        spin_sleeper.sleep(sleep_duration_hundred_milli_sec);
    }
    #[cfg(not(test))]
    info!(
        target: module_path!(),
        "received termination signal from operating system",
    );

    // phase 1: shut down highest level controls: data_logger
    // data logger initiates communication to heating, refill, atlas_scientific, ambient
    if execution_config.data_logger {
        send_command(
            InternalCommand::Quit,
            Some(&mut signal_handler_channels.tx_signal_handler_to_data_logger),
            Some(&signal_handler_channels.rx_signal_handler_from_data_logger),
            "data_logger",
        );
    }
    send_command(
        InternalCommand::Quit,
        signal_handler_channels
            .tx_signal_handler_to_messaging_opt
            .as_mut(),
        signal_handler_channels
            .rx_signal_handler_from_messaging_opt
            .as_ref(),
        "messaging",
    );
    #[cfg(feature = "debug_signal_handler")]
    debug!(
        target: module_path!(),
        "phase 1 of termination concluded",
    );

    // phase 2: shut down high-level controls: heating, ventilation, feed, refill, balling
    // these controls access the Controllino, Atlas Scientific and Dht
    if execution_config.refill {
        send_command(
            InternalCommand::Quit,
            Some(&mut signal_handler_channels.tx_signal_handler_to_refill),
            Some(&signal_handler_channels.rx_signal_handler_from_refill),
            "refill",
        );
    }
    if execution_config.heating {
        send_command(
            InternalCommand::Quit,
            Some(&mut signal_handler_channels.tx_signal_handler_to_heating),
            Some(&signal_handler_channels.rx_signal_handler_from_heating),
            "heating",
        );
    }
    if execution_config.ventilation {
        send_command(
            InternalCommand::Quit,
            Some(&mut signal_handler_channels.tx_signal_handler_to_ventilation),
            Some(&signal_handler_channels.rx_signal_handler_from_ventilation),
            "ventilation",
        );
    }
    if execution_config.feed {
        send_command(
            InternalCommand::Quit,
            Some(&mut signal_handler_channels.tx_signal_handler_to_feed),
            Some(&signal_handler_channels.rx_signal_handler_from_feed),
            "feed",
        );
    }
    if execution_config.balling {
        send_command(
            InternalCommand::Quit,
            Some(&mut signal_handler_channels.tx_signal_handler_to_balling),
            Some(&signal_handler_channels.rx_signal_handler_from_balling),
            "balling",
        );
    }
    #[cfg(feature = "debug_signal_handler")]
    debug!(
        target: module_path!(),
        "phase 2 of termination concluded",
    );

    // phase 3: shut down medium level controls
    // these controls receive commands from upper level controls and communicate to i2c_interface and dht
    if execution_config.tank_level_switch {
        send_command(
            InternalCommand::Quit,
            Some(&mut signal_handler_channels.tx_signal_handler_to_tank_level_switch),
            Some(&signal_handler_channels.rx_signal_handler_from_tank_level_switch),
            "tank_level_switch",
        );
    }
    if !use_simulator && execution_config.atlas_scientific {
        send_command(
            InternalCommand::Quit,
            Some(&mut signal_handler_channels.tx_signal_handler_to_atlas_scientific),
            Some(&signal_handler_channels.rx_signal_handler_from_atlas_scientific),
            "atlas_scientific",
        );
    }
    if execution_config.sensor_manager {
        send_command(
            InternalCommand::Quit,
            Some(&mut signal_handler_channels.tx_signal_handler_to_sensor_manager),
            Some(&signal_handler_channels.rx_signal_handler_from_sensor_manager),
            "ambient",
        );
    }
    if execution_config.schedule_check {
        send_command(
            InternalCommand::Quit,
            Some(&mut signal_handler_channels.tx_signal_handler_to_schedule_check),
            Some(&signal_handler_channels.rx_signal_handler_from_schedule_check),
            "schedule_check",
        );
    }
    #[cfg(feature = "debug_signal_handler")]
    debug!(
        target: module_path!(),
        "phase 3 of termination concluded",
    );

    // phase 4: shut down low-level controls
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    {
        if execution_config.i2c_interface {
            send_command(
                InternalCommand::Quit,
                Some(&signal_handler_channels.tx_signal_handler_to_i2c_interface),
                None,
                "i2c_interface",
            );
        }
        if execution_config.dht {
            send_command(
                InternalCommand::Quit,
                Some(&signal_handler_channels.tx_signal_handler_to_dht),
                None,
                "dht",
            );
        }
    }
    if execution_config.relay_manager {
        send_command(
            InternalCommand::Quit,
            Some(&mut signal_handler_channels.tx_signal_handler_to_relay_manager),
            Some(&signal_handler_channels.rx_signal_handler_from_relay_manager),
            "relay_manager",
        );
    }
    send_command(
        InternalCommand::Quit,
        signal_handler_channels
            .tx_signal_handler_to_tcp_opt
            .as_mut(),
        signal_handler_channels
            .rx_signal_handler_from_tcp_opt
            .as_ref(),
        "tcp",
    );
    if execution_config.monitors {
        send_command(
            InternalCommand::Quit,
            Some(&mut signal_handler_channels.tx_signal_handler_to_monitors),
            Some(&signal_handler_channels.rx_signal_handler_from_monitors),
            "monitors",
        );
    }
    #[cfg(feature = "debug_signal_handler")]
    debug!(
        target: module_path!(),
        "phase 4 of termination concluded",
    );

    // phase 5: send termination command to the highest level controls
    if execution_config.data_logger {
        send_command(
            InternalCommand::Terminate,
            Some(&mut signal_handler_channels.tx_signal_handler_to_data_logger),
            None,
            "data_logger",
        );
    }
    #[cfg(feature = "debug_signal_handler")]
    debug!(
        target: module_path!(),
        "phase 5 of termination concluded",
    );

    // phase 6: send termination command to high-level controls
    if execution_config.heating {
        send_command(
            InternalCommand::Terminate,
            Some(&mut signal_handler_channels.tx_signal_handler_to_heating),
            None,
            "heating",
        );
    }
    if execution_config.ventilation {
        send_command(
            InternalCommand::Terminate,
            Some(&mut signal_handler_channels.tx_signal_handler_to_ventilation),
            None,
            "ventilation",
        );
    }
    if execution_config.feed {
        send_command(
            InternalCommand::Terminate,
            Some(&mut signal_handler_channels.tx_signal_handler_to_feed),
            None,
            "feed",
        );
    }
    if execution_config.refill {
        send_command(
            InternalCommand::Terminate,
            Some(&mut signal_handler_channels.tx_signal_handler_to_refill),
            None,
            "refill",
        );
    }
    if execution_config.balling {
        send_command(
            InternalCommand::Terminate,
            Some(&mut signal_handler_channels.tx_signal_handler_to_balling),
            None,
            "balling",
        );
    }
    #[cfg(feature = "debug_signal_handler")]
    debug!(
        target: module_path!(),
        "phase 6 of termination concluded",
    );

    // phase 7: send quit/termination command to medium level controls
    if execution_config.ds18b20 {
        send_command(
            InternalCommand::Quit,
            Some(&mut signal_handler_channels.tx_signal_handler_to_ds18b20),
            None,
            "ds18b20",
        );
    }
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    {
        if execution_config.atlas_scientific {
            send_command(
                InternalCommand::Terminate,
                Some(&signal_handler_channels.tx_signal_handler_to_atlas_scientific),
                None,
                "atlas_scientific",
            );
        }
    }

    if execution_config.sensor_manager {
        send_command(
            InternalCommand::Terminate,
            Some(&mut signal_handler_channels.tx_signal_handler_to_sensor_manager),
            None,
            "ambient",
        );
    }
    if execution_config.tank_level_switch {
        send_command(
            InternalCommand::Terminate,
            Some(&mut signal_handler_channels.tx_signal_handler_to_tank_level_switch),
            None,
            "tank_level_switch",
        );
    }
    #[cfg(feature = "debug_signal_handler")]
    debug!(
        target: module_path!(),
        "phase 7 of termination concluded",
    );

    // phase 8: allow messaging thread and watchdog to close
    if execution_config.watchdog {
        send_command(
            InternalCommand::Terminate,
            Some(&mut signal_handler_channels.tx_signal_handler_to_watchdog),
            Some(&signal_handler_channels.rx_signal_handler_from_watchdog),
            "watchdog",
        );
    }
    send_command(
        InternalCommand::Terminate,
        signal_handler_channels
            .tx_signal_handler_to_messaging_opt
            .as_mut(),
        None,
        "messaging",
    );

    println!("Termination of application completed");

    #[cfg(feature = "debug_channels")]
    debug!(target: module_path!(), "{signal_handler_channels}");
}

#[cfg(test)]
pub mod tests {
    use crate::launch::channels::Channels;
    use crate::launch::execution_config::ExecutionConfig;
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::signal_handler::handle_signals;
    use spin_sleep::SpinSleeper;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use std::{process::Command, sync::Arc, thread};

    // helper function to send a signal to the application
    fn send_signal(pid: u32, signal: i32) {
        Command::new("kill")
            .args(&[pid.to_string().as_str()])
            .output()
            .expect("Failed to send signal");
        println!("Sent signal {} to {}", signal, pid);
    }

    // test if the signal handler sends shutdown requests to all threads in the right order.
    #[test]
    fn test_signal_handling() {
        let term = Arc::new(AtomicBool::new(false));
        let term_clone = Arc::clone(&term);

        let mut channels = Channels::new_for_test();

        // start the test object in a separate thread
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                handle_signals(
                    term_clone,
                    channels.signal_handler,
                    ExecutionConfig::default(),
                );
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
                .recv()
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
                .recv()
                .unwrap(),
            InternalCommand::Quit
        );
        _ = channels.messaging.tx_messaging_to_signal_handler.send(true);

        assert_eq!(
            channels
                .refill
                .rx_refill_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Quit
        );
        _ = channels.refill.tx_refill_to_signal_handler.send(true);

        assert_eq!(
            channels
                .heating
                .rx_heating_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Quit
        );
        _ = channels.heating.tx_heating_to_signal_handler.send(true);

        assert_eq!(
            channels
                .ventilation
                .rx_ventilation_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Quit
        );
        _ = channels
            .ventilation
            .tx_ventilation_to_signal_handler
            .send(true);

        assert_eq!(
            channels.feed.rx_feed_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = channels.feed.tx_feed_to_signal_handler.send(true);

        assert_eq!(
            channels
                .balling
                .rx_balling_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Quit
        );
        _ = channels.balling.tx_balling_to_signal_handler.send(true);

        assert_eq!(
            channels
                .tank_level_switch
                .rx_tank_level_switch_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Quit
        );
        _ = channels
            .tank_level_switch
            .tx_tank_level_switch_to_signal_handler
            .send(true);
        #[cfg(all(feature = "target_hw", target_os = "linux"))]
        {
            assert_eq!(
                channels
                    .atlas_scientific
                    .rx_atlas_scientific_from_signal_handler
                    .recv()
                    .unwrap(),
                InternalCommand::Quit
            );
            _ = channels
                .atlas_scientific
                .tx_atlas_scientific_to_signal_handler
                .send(true);
        }
        assert_eq!(
            channels
                .sensor_manager
                .rx_sensor_manager_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Quit
        );
        _ = channels
            .sensor_manager
            .tx_sensor_manager_to_signal_handler
            .send(true);
        assert_eq!(
            channels
                .schedule_check
                .rx_schedule_check_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Quit
        );
        _ = channels
            .schedule_check
            .tx_schedule_check_to_signal_handler
            .send(true);
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        assert_eq!(
            channels
                .i2c_interface
                .rx_i2c_interface_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Quit
        );
        // no response from i2c_interface

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        assert_eq!(
            channels.dht.rx_dht_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        // no response from dht

        assert_eq!(
            channels
                .relay_manager
                .rx_relay_manager_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Quit
        );
        _ = channels
            .relay_manager
            .tx_relay_manager_to_signal_handler
            .send(true);

        assert_eq!(
            channels
                .tcp_communication
                .rx_tcp_communication_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Quit
        );
        _ = channels
            .tcp_communication
            .tx_tcp_communication_to_signal_handler
            .send(true);

        assert_eq!(
            channels
                .monitors
                .rx_monitors_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Quit
        );
        _ = channels.monitors.tx_monitors_to_signal_handler.send(true);

        assert_eq!(
            channels
                .data_logger
                .rx_data_logger_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            channels
                .heating
                .rx_heating_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            channels
                .ventilation
                .rx_ventilation_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            channels.feed.rx_feed_from_signal_handler.recv().unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            channels
                .refill
                .rx_refill_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            channels
                .balling
                .rx_balling_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            channels
                .ds18b20
                .rx_ds18b20_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Quit
        );
        // no response to quit signal from ds18b20

        #[cfg(all(feature = "target_hw", target_os = "linux"))]
        {
            assert_eq!(
                channels
                    .atlas_scientific
                    .rx_atlas_scientific_from_signal_handler
                    .recv()
                    .unwrap(),
                InternalCommand::Terminate
            );
        }
        // no response to termination request

        assert_eq!(
            channels
                .sensor_manager
                .rx_sensor_manager_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            channels
                .tank_level_switch
                .rx_tank_level_switch_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            channels
                .watchdog
                .rx_watchdog_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Terminate
        );
        _ = channels.watchdog.tx_watchdog_to_signal_handler.send(true);

        assert_eq!(
            channels
                .messaging
                .rx_messaging_from_signal_handler
                .recv()
                .unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");

        assert!(term.load(Ordering::Acquire));
    }
}
