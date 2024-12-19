/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::channel_content::InternalCommand;
use log::{error, info};

#[cfg(feature = "debug_signal_handler")]
use log::debug;

use spin_sleep::SpinSleeper;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

const LOG_TARGET_SIGNAL_HANDLER: &str = "signal_handler";

/// Sends the QUIT command to the thread and waits for confirmation
/// # Arguments
/// * `tx_to_thread` - reference to sender part of channel for communication to thread
/// * `tx_to_refill` - reference to receiver part of channel for communication from thread
/// * `thread_name` - name of thread (for debugging purposes)
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
                    debug!(target: LOG_TARGET_SIGNAL_HANDLER, "QUIT sent to {}", thread_name);

                    match rx_from_thread_opt {
                        Some(rx_from_thread) => match rx_from_thread.recv() {
                            Ok(_) => {
                                #[cfg(feature = "debug_signal_handler")]
                                debug!(target: LOG_TARGET_SIGNAL_HANDLER, "received acknowledgement from {}", thread_name);
                            }
                            Err(e) => {
                                error!(
                                    target: LOG_TARGET_SIGNAL_HANDLER,
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
                        target: LOG_TARGET_SIGNAL_HANDLER,
                        "channel communication to {} thread failed when sending Quit command ({e:?})",
                        thread_name
                    );
                }
            }
        }
        None => {
            /* do nothing */
            #[cfg(feature = "debug_signal_handler")]
            debug!(target: LOG_TARGET_SIGNAL_HANDLER, "not sending QUIT to i2c_interface");
        }
    }
}

/// Handles the termination signal from operating system (SIGTERM).
/// Upon receiving the termination signal, it will send the internal command for quitting
/// to all threads in a dedicated order which allows safe shutdown.
/// # Arguments
/// * `tx_to_messaging` - sender part of channel for communication to messaging
/// * `tx_to_refill` - sender part of channel for communication to refill control
/// * `rx_from_refill` - receiver part of channel for communication from refill control
/// * `tx_to_tank_level_switch` - sender part of channel for communication to tank level switch
/// * `rx_from_tank_level_switch` - receiver part of channel for communication from tank level switch
/// * `tx_to_heating` - sender part of channel for communication to heating control
/// * `rx_from_heating` - receiver part of channel for communication from heating control
/// * `tx_to_controllino` - sender part of channel for communication to Controllino
/// * `rx_from_controllino` - receiver part of channel for communication from Controllino
/// * `tx_to_atlas_scientific` - sender part of channel for communication to Atlas Scientific
/// * `rx_from_atlas_scientific` - receiver part of channel for communication from Atlas Scientific
/// * `tx_to_ambient` - sender part of channel for communication to Ambient
/// * `rx_from_ambient` - receiver part of channel for communication from Ambient
/// * `tx_to_data_logger` - sender part of channel for communication to data logger
/// * `rx_from_data_logger` - receiver part of channel for communication from data logger
/// * `tx_to_balling` - sender part of channel for communication to Balling dosing control
/// * `rx_from_balling` - receiver part of channel for communication from Balling dosing control
/// * `tx_to_ventilation` - sender part of channel for communication to ventilation control
/// * `rx_from_ventilation` - receiver part of channel for communication from ventilation control
/// * `tx_to_monitors` - sender part of channel for communication to monitors
/// * `rx_from_monitors` - receiver part of channel for communication from monitors
/// * `tx_to_feed` - sender part of channel for communication to feed control
/// * `rx_from_feed` - receiver part of channel for communication from feed control
/// * `tx_to_schedule_check` - sender part of channel for communication to schedule check
/// * `rx_from_schedule_check` - receiver part of channel for communication schedule check
/// * `tx_to_tcp_opt` - sender part of channel for communication to TCPCommunication wrapped in Option
/// * `rx_from_tcp_opt` - receiver part of channel for communication to TCPCommunication wrapped in Option
/// * `tx_to_dht_opt` - sender part of channel for communication to Dht wrapped in Option
/// * `tx_to_i2c_interface_opt` - sender part of channel for communication to I2C interface wrapped in Option

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     signal_handler[Signal handler] --> messaging[Messaging]
///     signal_handler --> refill[Refill control]
///     refill --> signal_handler
///     signal_handler --> tank_level_switch[Tank Level Switch]
///     tank_level_switch --> signal_handler
///     signal_handler --> heating[Heating control]
///     heating --> signal_handler
///     signal_handler --> controllino[Controllino]
///     controllino --> signal_handler
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
/// ```

pub fn handle_signals(
    tx_to_messaging_opt: Option<Sender<InternalCommand>>,
    tx_to_refill: Sender<InternalCommand>,
    rx_from_refill: Receiver<bool>,
    tx_to_tank_level_switch: Sender<InternalCommand>,
    rx_from_tank_level_switch: Receiver<bool>,
    tx_to_heating: Sender<InternalCommand>,
    rx_from_heating: Receiver<bool>,
    tx_to_controllino: Sender<InternalCommand>,
    rx_from_controllino: Receiver<bool>,
    tx_to_atlas_scientific: Sender<InternalCommand>,
    rx_from_atlas_scientific: Receiver<bool>,
    tx_to_ambient: Sender<InternalCommand>,
    rx_from_ambient: Receiver<bool>,
    tx_to_data_logger: Sender<InternalCommand>,
    rx_from_data_logger: Receiver<bool>,
    tx_to_balling: Sender<InternalCommand>,
    rx_from_balling: Receiver<bool>,
    tx_to_ventilation: Sender<InternalCommand>,
    rx_from_ventilation: Receiver<bool>,
    tx_to_monitors: Sender<InternalCommand>,
    rx_from_monitors: Receiver<bool>,
    tx_to_feed: Sender<InternalCommand>,
    rx_from_feed: Receiver<bool>,
    tx_to_schedule_check: Sender<InternalCommand>,
    rx_from_schedule_check: Receiver<bool>,
    tx_to_tcp_opt: Option<Sender<InternalCommand>>,
    rx_from_tcp_opt: Option<Receiver<bool>>,
    tx_to_dht_opt: Option<Sender<InternalCommand>>,
    tx_to_i2c_interface_opt: Option<Sender<InternalCommand>>,
) {
    if tx_to_tcp_opt.is_some() != rx_from_tcp_opt.is_some() {
        panic!(
            "{}: inconsistent channel parameters for TcpCommunication provided",
            LOG_TARGET_SIGNAL_HANDLER
        );
    }

    let spin_sleeper = SpinSleeper::default();
    let sleep_interval_millis = 100;
    let sleep_duration_hundred_milli_sec = Duration::from_millis(sleep_interval_millis);
    let term = Arc::new(AtomicBool::new(false));

    for signal in signal_hook::consts::TERM_SIGNALS {
        match signal_hook::flag::register(*signal, Arc::clone(&term)) {
            Ok(c) => {
                info!(
                    target: LOG_TARGET_SIGNAL_HANDLER,
                    "registered hook for signal {c:?}"
                );
            }
            Err(e) => {
                panic!(
                    "{}: Could not hook SIGTERM routine: {e:?}",
                    LOG_TARGET_SIGNAL_HANDLER
                );
            }
        }
    }

    while !term.load(Ordering::Relaxed) {
        spin_sleeper.sleep(sleep_duration_hundred_milli_sec);
    }
    info!(
        target: LOG_TARGET_SIGNAL_HANDLER,
        "received termination signal from operating system",
    );

    // phase 1: shut down highest level controls: data_logger and monitors
    // data logger initiates communication to heating, refill, atlas_scientific, ambient
    send_command(
        InternalCommand::Quit,
        Some(&tx_to_data_logger),
        Some(&rx_from_data_logger),
        "data_logger",
    );
    send_command(
        InternalCommand::Quit,
        Some(&tx_to_monitors),
        Some(&rx_from_monitors),
        "monitors",
    );
    send_command(
        InternalCommand::Quit,
        tx_to_messaging_opt.as_ref(),
        None,
        "messaging",
    );

    // phase 2: shut down high level controls: heating, ventilation, feed, refill, balling
    // these controls access the Controllino, Atlas Scientific and Dht
    send_command(
        InternalCommand::Quit,
        Some(&tx_to_refill),
        Some(&rx_from_refill),
        "refill",
    );
    send_command(
        InternalCommand::Quit,
        Some(&tx_to_heating),
        Some(&rx_from_heating),
        "heating",
    );
    send_command(
        InternalCommand::Quit,
        Some(&tx_to_ventilation),
        Some(&rx_from_ventilation),
        "ventilation",
    );
    send_command(
        InternalCommand::Quit,
        Some(&tx_to_feed),
        Some(&rx_from_feed),
        "feed",
    );
    send_command(
        InternalCommand::Quit,
        Some(&tx_to_balling),
        Some(&rx_from_balling),
        "balling",
    );

    // phase 3: shut down medium level controls
    // these controls receive commands from upper level controls and communicate to i2c_interface and dht
    send_command(
        InternalCommand::Quit,
        Some(&tx_to_tank_level_switch),
        Some(&rx_from_tank_level_switch),
        "tank_level_switch",
    );
    send_command(
        InternalCommand::Quit,
        Some(&tx_to_atlas_scientific),
        Some(&rx_from_atlas_scientific),
        "atlas_scientific",
    );
    send_command(
        InternalCommand::Quit,
        Some(&tx_to_ambient),
        Some(&rx_from_ambient),
        "ambient",
    );
    send_command(
        InternalCommand::Quit,
        Some(&tx_to_schedule_check),
        Some(&rx_from_schedule_check),
        "schedule_check",
    );

    // phase 4: shut down low level controls
    send_command(
        InternalCommand::Quit,
        tx_to_i2c_interface_opt.as_ref(),
        None,
        "i2c_interface",
    );
    send_command(InternalCommand::Quit, tx_to_dht_opt.as_ref(), None, "dht");

    send_command(
        InternalCommand::Quit,
        Some(&tx_to_controllino),
        Some(&rx_from_controllino),
        "controllino",
    );
    send_command(
        InternalCommand::Quit,
        tx_to_tcp_opt.as_ref(),
        rx_from_tcp_opt.as_ref(),
        "tcp",
    );

    // phase 5: send termination command to highest level controls
    send_command(
        InternalCommand::Terminate,
        Some(&tx_to_data_logger),
        None,
        "data_logger",
    );
    send_command(
        InternalCommand::Terminate,
        Some(&tx_to_monitors),
        None,
        "monitors",
    );

    // phase 6: send termination command to high level controls
    send_command(
        InternalCommand::Terminate,
        Some(&tx_to_heating),
        None,
        "heating",
    );
    send_command(
        InternalCommand::Terminate,
        Some(&tx_to_ventilation),
        None,
        "ventilation",
    );
    send_command(InternalCommand::Terminate, Some(&tx_to_feed), None, "feed");
    send_command(
        InternalCommand::Terminate,
        Some(&tx_to_refill),
        None,
        "refill",
    );
    send_command(
        InternalCommand::Terminate,
        Some(&tx_to_balling),
        None,
        "balling",
    );

    // phase 7: send termination command to medium level controls
    send_command(
        InternalCommand::Terminate,
        Some(&tx_to_atlas_scientific),
        None,
        "atlas_scientific",
    );
    send_command(
        InternalCommand::Terminate,
        Some(&tx_to_ambient),
        None,
        "ambient",
    );
    send_command(
        InternalCommand::Terminate,
        Some(&tx_to_tank_level_switch),
        None,
        "tank_level_switch",
    );
}
