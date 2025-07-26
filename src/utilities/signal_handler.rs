/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::error;
#[cfg(any(feature = "debug_signal_handler", not(test)))]
use log::info;

#[cfg(feature = "debug_signal_handler")]
use log::debug;

use crate::utilities::channel_content::InternalCommand;
use spin_sleep::SpinSleeper;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
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
    tx_to_thread_opt: Option<&Sender<InternalCommand>>,
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
                                    "could not receive answer from {} thread {e:?}",
                                    thread_name
                                );
                            }
                        },
                        None => { /* do nothing */ }
                    };
                }
                Err(e) => {
                    error!(
                        target: module_path!(),
                        "channel communication to {} thread failed when sending Quit command ({e:?})",
                        thread_name
                    );
                }
            }
        }
        None => {
            /* do nothing */
            #[cfg(feature = "debug_signal_handler")]
            debug!(target: module_path!(), "not sending QUIT to i2c_interface");
        }
    }
}

pub struct SignalHandlerChannels {
    /// sender part of the channel for communication to messaging wrapped in Option
    pub tx_signal_handler_to_messaging_opt: Option<Sender<InternalCommand>>,

    /// receiver part of the channel for communication to messaging wrapped in Option
    pub rx_signal_handler_from_messaging_opt: Option<Receiver<bool>>,

    /// sender part of the channel for communication to refill control
    pub tx_signal_handler_to_refill: Sender<InternalCommand>,

    /// receiver part of the channel for communication from refill control
    pub rx_signal_handler_from_refill: Receiver<bool>,

    /// sender part of the channel for communication to tank level switch
    pub tx_signal_handler_to_tank_level_switch: Sender<InternalCommand>,

    /// receiver part of the channel for communication from tank level switch
    pub rx_signal_handler_from_tank_level_switch: Receiver<bool>,

    /// sender part of the channel for communication to heating control
    pub tx_signal_handler_to_heating: Sender<InternalCommand>,

    /// receiver part of the channel for communication from heating control
    pub rx_signal_handler_from_heating: Receiver<bool>,

    /// sender part of the channel for communication to Controllino
    pub tx_signal_handler_to_relay_manager: Sender<InternalCommand>,

    /// receiver part of the channel for communication from Controllino
    pub rx_signal_handler_from_relay_manager: Receiver<bool>,

    /// sender part of the channel for communication to Atlas Scientific
    pub tx_signal_handler_to_atlas_scientific: Sender<InternalCommand>,

    /// receiver part of the channel for communication from Atlas Scientific
    pub rx_signal_handler_from_atlas_scientific: Receiver<bool>,

    /// sender part of the channel for communication to the sensor manager
    pub tx_signal_handler_to_sensor_manager: Sender<InternalCommand>,

    /// receiver part of the channel for communication from the sensor manager
    pub rx_signal_handler_from_sensor_manager: Receiver<bool>,

    /// sender part of the channel for communication to data logger
    pub tx_signal_handler_to_data_logger: Sender<InternalCommand>,

    /// receiver part of the channel for communication from data logger
    pub rx_signal_handler_from_data_logger: Receiver<bool>,

    /// sender part of the channel for communication to Balling dosing control
    pub tx_signal_handler_to_balling: Sender<InternalCommand>,

    /// receiver part of the channel for communication from Balling dosing control
    pub rx_signal_handler_from_balling: Receiver<bool>,

    /// sender part of the channel for communication to ventilation control
    pub tx_signal_handler_to_ventilation: Sender<InternalCommand>,

    /// receiver part of the channel for communication from ventilation control
    pub rx_signal_handler_from_ventilation: Receiver<bool>,

    /// sender part of the channel for communication to monitors
    pub tx_signal_handler_to_monitors: Sender<InternalCommand>,

    /// receiver part of the channel for communication from monitors
    pub rx_signal_handler_from_monitors: Receiver<bool>,

    /// sender part of the channel for communication to feed control
    pub tx_signal_handler_to_feed: Sender<InternalCommand>,

    /// receiver part of the channel for communication from feed control
    pub rx_signal_handler_from_feed: Receiver<bool>,

    /// sender part of the channel for communication to schedule check
    pub tx_signal_handler_to_schedule_check: Sender<InternalCommand>,

    /// receiver part of the channel for communication schedule check
    pub rx_signal_handler_from_schedule_check: Receiver<bool>,

    /// sender part of the channel for communication to TCPCommunication wrapped in Option
    pub tx_signal_handler_to_tcp_opt: Option<Sender<InternalCommand>>,

    /// receiver part of the channel for communication to TCPCommunication wrapped in Option
    pub rx_signal_handler_from_tcp_opt: Option<Receiver<bool>>,

    /// sender part of the channel for communication to Dht wrapped in Option
    pub tx_signal_handler_to_dht: Sender<InternalCommand>,

    /// sender part of the channel for communication to I2C interface
    pub tx_signal_handler_to_i2c_interface: Sender<InternalCommand>,

    /// sender part of the channel for communication to watchdog
    pub tx_signal_handler_to_watchdog: Sender<InternalCommand>,

    /// receiver part of the channel for communication from watchdog
    pub rx_signal_handler_from_watchdog: Receiver<bool>,

    /// sender part of the channel for communication to DS18B20 sensor reading
    pub tx_signal_handler_to_ds18b20: Sender<InternalCommand>,
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
pub fn handle_signals(term: Arc<AtomicBool>, signal_handler_channels: SignalHandlerChannels) {
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
                panic!(
                    "{}: Could not hook SIGTERM routine: {e:?}",
                    module_path!()
                );
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
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_data_logger),
        Some(&signal_handler_channels.rx_signal_handler_from_data_logger),
        "data_logger",
    );
    send_command(
        InternalCommand::Quit,
        signal_handler_channels
            .tx_signal_handler_to_messaging_opt
            .as_ref(),
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
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_refill),
        Some(&signal_handler_channels.rx_signal_handler_from_refill),
        "refill",
    );
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_heating),
        Some(&signal_handler_channels.rx_signal_handler_from_heating),
        "heating",
    );
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_ventilation),
        Some(&signal_handler_channels.rx_signal_handler_from_ventilation),
        "ventilation",
    );
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_feed),
        Some(&signal_handler_channels.rx_signal_handler_from_feed),
        "feed",
    );
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_balling),
        Some(&signal_handler_channels.rx_signal_handler_from_balling),
        "balling",
    );
    #[cfg(feature = "debug_signal_handler")]
    debug!(
        target: module_path!(),
        "phase 2 of termination concluded",
    );

    // phase 3: shut down medium level controls
    // these controls receive commands from upper level controls and communicate to i2c_interface and dht
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_tank_level_switch),
        Some(&signal_handler_channels.rx_signal_handler_from_tank_level_switch),
        "tank_level_switch",
    );
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_atlas_scientific),
        Some(&signal_handler_channels.rx_signal_handler_from_atlas_scientific),
        "atlas_scientific",
    );
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_sensor_manager),
        Some(&signal_handler_channels.rx_signal_handler_from_sensor_manager),
        "ambient",
    );
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_schedule_check),
        Some(&signal_handler_channels.rx_signal_handler_from_schedule_check),
        "schedule_check",
    );
    #[cfg(feature = "debug_signal_handler")]
    debug!(
        target: module_path!(),
        "phase 3 of termination concluded",
    );

    // phase 4: shut down low-level controls
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_i2c_interface),
        None,
        "i2c_interface",
    );
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_dht),
        None,
        "dht",
    );
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_relay_manager),
        Some(&signal_handler_channels.rx_signal_handler_from_relay_manager),
        "relay_manager",
    );
    send_command(
        InternalCommand::Quit,
        signal_handler_channels
            .tx_signal_handler_to_tcp_opt
            .as_ref(),
        signal_handler_channels
            .rx_signal_handler_from_tcp_opt
            .as_ref(),
        "tcp",
    );
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_monitors),
        Some(&signal_handler_channels.rx_signal_handler_from_monitors),
        "monitors",
    );
    #[cfg(feature = "debug_signal_handler")]
    debug!(
        target: module_path!(),
        "phase 4 of termination concluded",
    );

    // phase 5: send termination command to the highest level controls
    send_command(
        InternalCommand::Terminate,
        Some(&signal_handler_channels.tx_signal_handler_to_data_logger),
        None,
        "data_logger",
    );
    #[cfg(feature = "debug_signal_handler")]
    debug!(
        target: module_path!(),
        "phase 5 of termination concluded",
    );

    // phase 6: send termination command to high-level controls
    send_command(
        InternalCommand::Terminate,
        Some(&signal_handler_channels.tx_signal_handler_to_heating),
        None,
        "heating",
    );
    send_command(
        InternalCommand::Terminate,
        Some(&signal_handler_channels.tx_signal_handler_to_ventilation),
        None,
        "ventilation",
    );
    send_command(
        InternalCommand::Terminate,
        Some(&signal_handler_channels.tx_signal_handler_to_feed),
        None,
        "feed",
    );
    send_command(
        InternalCommand::Terminate,
        Some(&signal_handler_channels.tx_signal_handler_to_refill),
        None,
        "refill",
    );
    send_command(
        InternalCommand::Terminate,
        Some(&signal_handler_channels.tx_signal_handler_to_balling),
        None,
        "balling",
    );
    #[cfg(feature = "debug_signal_handler")]
    debug!(
        target: module_path!(),
        "phase 6 of termination concluded",
    );

    // phase 7: send quit/termination command to medium level controls
    send_command(
        InternalCommand::Quit,
        Some(&signal_handler_channels.tx_signal_handler_to_ds18b20),
        None,
        "ds18b20",
    );
    send_command(
        InternalCommand::Terminate,
        Some(&signal_handler_channels.tx_signal_handler_to_atlas_scientific),
        None,
        "atlas_scientific",
    );
    send_command(
        InternalCommand::Terminate,
        Some(&signal_handler_channels.tx_signal_handler_to_sensor_manager),
        None,
        "ambient",
    );
    send_command(
        InternalCommand::Terminate,
        Some(&signal_handler_channels.tx_signal_handler_to_tank_level_switch),
        None,
        "tank_level_switch",
    );
    #[cfg(feature = "debug_signal_handler")]
    debug!(
        target: module_path!(),
        "phase 7 of termination concluded",
    );

    // phase 8: allow messaging thread and watchdog to close
    send_command(
        InternalCommand::Terminate,
        Some(&signal_handler_channels.tx_signal_handler_to_watchdog),
        Some(&signal_handler_channels.rx_signal_handler_from_watchdog),
        "watchdog",
    );

    send_command(
        InternalCommand::Terminate,
        signal_handler_channels
            .tx_signal_handler_to_messaging_opt
            .as_ref(),
        None,
        "messaging",
    );

    println!("Termination of application completed");
}

#[cfg(test)]
pub mod tests {
    use spin_sleep::SpinSleeper;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::time::Duration;
    use std::{process::Command, sync::Arc, thread};

    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::signal_handler::{handle_signals, SignalHandlerChannels};

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

        // *** signal handler <=> refill ***
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***********************************

        // *** signal handler <=> heating ***
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***********************************

        // *** signal handler <=> feed ***
        let (tx_signal_handler_to_feed, rx_feed_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_feed_to_signal_handler, rx_signal_handler_from_feed): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***********************************

        // *** signal handler <=> balling ***
        let (tx_signal_handler_to_balling, rx_balling_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_balling_to_signal_handler, rx_signal_handler_from_balling): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***********************************

        // *** signal handler <=> monitors ***
        let (tx_signal_handler_to_monitors, rx_monitors_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_monitors_to_signal_handler, rx_signal_handler_from_monitors): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***********************************

        // *** signal handler <=> ventilation ***
        let (tx_signal_handler_to_ventilation, rx_ventilation_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_ventilation_to_signal_handler, rx_signal_handler_from_ventilation): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***********************************

        // *** signal handler <=> schedule_check ***
        let (tx_signal_handler_to_schedule_check, rx_schedule_check_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_schedule_check_to_signal_handler, rx_signal_handler_from_schedule_check): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***********************************

        // *** signal handler <=> gpio ***
        let (tx_signal_handler_to_tank_level_switch, rx_tank_level_switch_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_tank_level_switch_to_signal_handler, rx_signal_handler_from_tank_level_switch): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // *********************************

        // *** signal handler <=> relay_manager ***
        let (tx_signal_handler_to_relay_manager, rx_relay_manager_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_relay_manager_to_signal_handler, rx_signal_handler_from_relay_manager): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***************************************

        // *** signal handler <=> data_logger ***
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_data_logger_to_signal_handler, rx_signal_handler_from_data_logger): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // **************************************

        // *** signal handler => ds18b20 ***
        let (tx_signal_handler_to_ds18b20, rx_ds18b20_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        // **************************************

        // *** signal handler <=> atlas_scientific ***
        let (tx_signal_handler_to_atlas_scientific, rx_atlas_scientific_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_atlas_scientific_to_signal_handler, rx_signal_handler_from_atlas_scientific): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // **************************************

        // *** signal handler <=> ambient ***
        let (tx_signal_handler_to_sensor_manager, rx_sensor_manager_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_sensor_manager_to_signal_handler, rx_signal_handler_from_sensor_manager): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // **************************************

        let (tx_signal_handler_to_tcp, rx_tcp_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_tcp_to_signal_handler, rx_signal_handler_from_tcp): (Sender<bool>, Receiver<bool>) =
            mpsc::channel();

        let use_simulator = true;

        // wrapping sender/receiver for communication with TCP thread
        let tx_signal_handler_to_tcp_opt: Option<Sender<InternalCommand>> = match use_simulator {
            true => Some(tx_signal_handler_to_tcp),
            false => None,
        };

        let rx_signal_handler_from_tcp_opt: Option<Receiver<bool>> = match use_simulator {
            true => Some(rx_signal_handler_from_tcp),
            false => None,
        };

        // communication with messaging is simulated - so no OS dependency here
        let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let tx_signal_handler_to_messaging_opt = Some(tx_signal_handler_to_messaging);

        let (tx_messaging_to_signal_handler, rx_signal_handler_from_messaging): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        let rx_signal_handler_from_messaging_opt = Some(rx_signal_handler_from_messaging);

        let (tx_signal_handler_to_dht, rx_dht_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();

        let (tx_signal_handler_to_i2c_interface, rx_i2c_interface_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();

        let (tx_signal_handler_to_watchdog, rx_watchdog_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_watchdog_to_signal_handler, rx_signal_handler_from_watchdog): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();

        let term_clone = Arc::clone(&term);

        // start the test object in a separate thread
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let signal_handler_channels = SignalHandlerChannels {
                    tx_signal_handler_to_messaging_opt,
                    rx_signal_handler_from_messaging_opt,
                    tx_signal_handler_to_refill,
                    rx_signal_handler_from_refill,
                    tx_signal_handler_to_tank_level_switch,
                    rx_signal_handler_from_tank_level_switch,
                    tx_signal_handler_to_heating,
                    rx_signal_handler_from_heating,
                    tx_signal_handler_to_relay_manager,
                    rx_signal_handler_from_relay_manager,
                    tx_signal_handler_to_atlas_scientific,
                    rx_signal_handler_from_atlas_scientific,
                    tx_signal_handler_to_sensor_manager,
                    tx_signal_handler_to_data_logger,
                    rx_signal_handler_from_data_logger,
                    tx_signal_handler_to_balling,
                    rx_signal_handler_from_balling,
                    tx_signal_handler_to_ventilation,
                    rx_signal_handler_from_ventilation,
                    tx_signal_handler_to_monitors,
                    rx_signal_handler_from_monitors,
                    tx_signal_handler_to_feed,
                    rx_signal_handler_from_feed,
                    tx_signal_handler_to_schedule_check,
                    rx_signal_handler_from_schedule_check,
                    tx_signal_handler_to_tcp_opt,
                    rx_signal_handler_from_tcp_opt,
                    tx_signal_handler_to_dht,
                    tx_signal_handler_to_i2c_interface,
                    tx_signal_handler_to_watchdog,
                    rx_signal_handler_from_watchdog,
                    tx_signal_handler_to_ds18b20,
                    rx_signal_handler_from_sensor_manager,
                };

                handle_signals(term_clone, signal_handler_channels);
            })
            .unwrap();

        // use pid of the current process
        let pid = std::process::id();

        let spin_sleeper = SpinSleeper::default();
        let sleep_duration_one_second = Duration::from_secs(1);

        spin_sleeper.sleep(sleep_duration_one_second);

        send_signal(pid, signal_hook::consts::signal::SIGABRT);

        assert_eq!(
            rx_data_logger_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = tx_data_logger_to_signal_handler.send(true);

        assert_eq!(
            rx_messaging_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = tx_messaging_to_signal_handler.send(true);

        assert_eq!(
            rx_refill_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = tx_refill_to_signal_handler.send(true);

        assert_eq!(
            rx_heating_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = tx_heating_to_signal_handler.send(true);

        assert_eq!(
            rx_ventilation_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = tx_ventilation_to_signal_handler.send(true);

        assert_eq!(
            rx_feed_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = tx_feed_to_signal_handler.send(true);

        assert_eq!(
            rx_balling_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = tx_balling_to_signal_handler.send(true);

        assert_eq!(
            rx_tank_level_switch_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = tx_tank_level_switch_to_signal_handler.send(true);
        assert_eq!(
            rx_atlas_scientific_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = tx_atlas_scientific_to_signal_handler.send(true);
        assert_eq!(
            rx_sensor_manager_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = tx_sensor_manager_to_signal_handler.send(true);
        assert_eq!(
            rx_schedule_check_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = tx_schedule_check_to_signal_handler.send(true);
        assert_eq!(
            rx_i2c_interface_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        // no response from i2c_interface

        assert_eq!(
            rx_dht_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        // no response from dht

        assert_eq!(
            rx_relay_manager_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = tx_relay_manager_to_signal_handler.send(true);

        assert_eq!(
            rx_tcp_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = tx_tcp_to_signal_handler.send(true);

        assert_eq!(
            rx_monitors_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        _ = tx_monitors_to_signal_handler.send(true);

        assert_eq!(
            rx_data_logger_from_signal_handler.recv().unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            rx_heating_from_signal_handler.recv().unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            rx_ventilation_from_signal_handler.recv().unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            rx_feed_from_signal_handler.recv().unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            rx_refill_from_signal_handler.recv().unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            rx_balling_from_signal_handler.recv().unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            rx_ds18b20_from_signal_handler.recv().unwrap(),
            InternalCommand::Quit
        );
        // no response to quit signal from ds18b20

        assert_eq!(
            rx_atlas_scientific_from_signal_handler.recv().unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            rx_sensor_manager_from_signal_handler.recv().unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            rx_tank_level_switch_from_signal_handler.recv().unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        assert_eq!(
            rx_watchdog_from_signal_handler.recv().unwrap(),
            InternalCommand::Terminate
        );
        _ = tx_watchdog_to_signal_handler.send(true);

        assert_eq!(
            rx_messaging_from_signal_handler.recv().unwrap(),
            InternalCommand::Terminate
        );
        // no response to termination request

        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");

        assert!(term.load(Ordering::Acquire));
    }
}
