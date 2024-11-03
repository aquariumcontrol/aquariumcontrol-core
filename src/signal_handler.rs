/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::channel_content::InternalCommand;
use log::{error, info};
use spin_sleep::SpinSleeper;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

const LOG_TARGET_SIGNAL_HANDLER: &str = "signal_handler";

/// Handles the termination signal from operating system (SIGTERM).
/// Upon receiving the termination signal, it will send the internal command for quitting
/// to all threads in a dedicated order which allows safe shutdown.
/// # Arguments
/// * `tx_to_messaging` - sender part of channel for communication to messaging
/// * `tx_to_refill` - sender part of channel for communication to refill control
/// * `rx_from_refill` - receiver part of channel for communication from refill control
/// * `tx_to_heating` - sender part of channel for communication to heating control
/// * `rx_from_heating` - receiver part of channel for communication from heating control
/// * `tx_to_controllino` - sender part of channel for communication to Controllino
/// * `rx_from_controllino` - receiver part of channel for communication from Controllino
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
/// * `tx_to_feed` - sender part of channel for communication to TcpCommunication wrapped in Option
/// * `rx_from_feed` - receiver part of channel for communication from TcpCommunication wrapped in Option
#[cfg_attr(doc, aquamarine::aquamarine)]
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     signal_handler[Signal handler] --> messaging[Messaging]
///     signal_handler --> refill[Refill control]
///     refill --> signal_handler
///     signal_handler --> heating[Heating control]
///     heating --> signal_handler
///     signal_handler --> controllino[Controllino]
///     controllino --> signal_handler
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
///     atlas_scientific[AtlasScientific] --> signal_handler
///     signal_handler --> atlas_scientific
///     ambient[Ambient] --> signal_handler
///     signal_handler --> ambient
///     tank_level_switch[TankLevelSwitch] --> signal_handler
///     signal_handler --> tank_level_switch
///     schedule_check[ScheduleCheck] --> signal_handler
///     signal_handler --> schedule_check
///     signal_handler --> tcp_communication[TCP communication]
///     tcp_communication --> signal_handler
/// ```

pub fn handle_signals(
    tx_to_messaging: Sender<InternalCommand>,
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

    // first, let's shut down the refill control
    match tx_to_refill.send(InternalCommand::Quit) {
        Ok(_) => {
            // now listen for response
            match rx_from_refill.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_SIGNAL_HANDLER,
                        "could not receive answer from refill thread ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to refill control failed when sending Quit command ({e:?})"
            );
        }
    }

    // second, let's shut down the heating control
    match tx_to_heating.send(InternalCommand::Quit) {
        Ok(_) => {
            // now listen for response
            match rx_from_heating.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_SIGNAL_HANDLER,
                        "could not receive answer from heating thread ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to heating control failed when sending Quit command ({e:?})"
            );
        }
    }

    // next, let's shut down the ventilation control
    match tx_to_ventilation.send(InternalCommand::Quit) {
        Ok(_) => {
            // now listen for response
            match rx_from_ventilation.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_SIGNAL_HANDLER,
                        "could not receive answer from ventilation control ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to ventilation control failed when sending Quit command ({e:?})"
            );
        }
    }

    // next, let's shut down the tank level switch calculation
    match tx_to_tank_level_switch.send(InternalCommand::Quit) {
        Ok(_) => {
            match rx_from_tank_level_switch.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_SIGNAL_HANDLER,
                        "could not receive answer from tank level switch thread ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to tank level switch calculation thread failed when sending Quit command ({e:?})"
            );
        }
    }

    // next, let's shut down the communication with Atlas Scientific sensor unit
    match tx_to_atlas_scientific.send(InternalCommand::Quit) {
        Ok(_) => {
            match rx_from_atlas_scientific.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_SIGNAL_HANDLER,
                        "could not receive answer from Atlas Scientific thread ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to Atlas Scientific thread failed when sending Quit command ({e:?})"
            );
        }
    }

    // next, let's shut down the communication with DHT sensor
    match tx_to_ambient.send(InternalCommand::Quit) {
        Ok(_) => {
            match rx_from_ambient.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_SIGNAL_HANDLER,
                        "could not receive answer from Ambient thread ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to Ambient thread failed when sending Quit command ({e:?})"
            );
        }
    }

    // next, let's shut down the monitors
    match tx_to_monitors.send(InternalCommand::Quit) {
        Ok(_) => {
            match rx_from_monitors.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_SIGNAL_HANDLER,
                        "could not receive answer from monitors thread ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to monitors thread failed when sending Quit command ({e:?})"
            );
        }
    }

    // next, let's shut down the feed control
    match tx_to_feed.send(InternalCommand::Quit) {
        Ok(_) => {
            match rx_from_feed.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_SIGNAL_HANDLER,
                        "could not receive answer from feed control ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to feed control failed when sending Quit command ({e:?})"
            );
        }
    }

    // next, let's shut down Balling dosing control
    match tx_to_balling.send(InternalCommand::Quit) {
        Ok(_) => {
            match rx_from_balling.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_SIGNAL_HANDLER,
                        "could not receive answer from Balling dosing control ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to Balling dosing congtrol failed when sending Quit command ({e:?})"
            );
        }
    }

    // next, let's shut down the schedule checker
    match tx_to_schedule_check.send(InternalCommand::Quit) {
        Ok(_) => {
            match rx_from_schedule_check.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_SIGNAL_HANDLER,
                        "could not receive answer from ScheduleCheck thread ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to ScheduleCheck failed when sending Quit command ({e:?})"
            );
        }
    }

    // next, let's shut down the data logging
    match tx_to_data_logger.send(InternalCommand::Quit) {
        Ok(_) => {
            match rx_from_data_logger.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_SIGNAL_HANDLER,
                        "could not receive answer from data logger thread {e:?}"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "hannel communication to data logger thread failed when sending Quit command ({e:?})"
            );
        }
    }

    // next, let's shut down Controllino
    match tx_to_controllino.send(InternalCommand::Quit) {
        Ok(_) => {
            match rx_from_controllino.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_SIGNAL_HANDLER,
                        "could not receive answer from Controllino thread ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to Controllino failed when sending Quit command ({e:?})"
            );
        }
    }

    // finally, let's shut down TcpCommunication
    match tx_to_tcp_opt {
        Some(tx_to_tcp) => match tx_to_tcp.send(InternalCommand::Quit) {
            Ok(_) => {
                match rx_from_tcp_opt {
                    Some(rx_from_tcp) => match rx_from_tcp.recv() {
                        Ok(_) => {}
                        Err(e) => {
                            error!(
                                target: LOG_TARGET_SIGNAL_HANDLER,
                                "could not receive answer from TcpCommunication thread ({e:?})"
                            );
                        }
                    },
                    None => { /* do nothing */ }
                }
            }
            Err(e) => {
                error!(
                    target: LOG_TARGET_SIGNAL_HANDLER,
                    "channel communication to TcpCommunication when sending Quit command ({e:?})"
                );
            }
        },
        None => { /* do nothing */ }
    }

    // data logger thread has already ceased operation. It is now awaiting termination command.
    match tx_to_data_logger.send(InternalCommand::Terminate) {
        Ok(_) => {}
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to data logger failed when sending Terminate command ({e:?})"
            );
        }
    }

    // heating thread has already ceased operation. It is now awaiting termination command.
    match tx_to_heating.send(InternalCommand::Terminate) {
        Ok(_) => {}
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to heating control failed when sending Terminate command ({e:?})"
            );
        }
    }

    // ventilation thread has already ceased operation. It is now awaiting termination command.
    match tx_to_ventilation.send(InternalCommand::Terminate) {
        Ok(_) => {}
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to ventilation control failed when sending Terminate command ({e:?})"
            );
        }
    }

    // Atlas Scientific thread has already ceased operation. It is now awaiting termination command.
    match tx_to_atlas_scientific.send(InternalCommand::Terminate) {
        Ok(_) => {}
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to Atlas Scientific failed when sending Terminate command ({e:?})"
            );
        }
    }

    // Ambient thread has already ceased operation. It is now awaiting termination command.
    match tx_to_ambient.send(InternalCommand::Terminate) {
        Ok(_) => {}
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to Ambient failed when sending Terminate command ({e:?})"
            );
        }
    }

    // Feed control has already ceased operation. It is now awaiting termination command.
    match tx_to_feed.send(InternalCommand::Terminate) {
        Ok(_) => {}
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to feed control failed when sending Terminate command ({e:?})"
            );
        }
    }

    // Balling control has already ceased operation. It is now awaiting termination command.
    match tx_to_balling.send(InternalCommand::Terminate) {
        Ok(_) => {}
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to Balling dosing control failed when sending Terminate command ({e:?})"
            );
        }
    }

    // Monitors thread has already ceased operation. It is now awaiting termination command.
    match tx_to_monitors.send(InternalCommand::Terminate) {
        Ok(_) => {}
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to Monitors failed when sending Terminate command ({e:?})"
            );
        }
    }

    // Refill thread has already ceased operation. It is now awaiting termination command.
    match tx_to_refill.send(InternalCommand::Terminate) {
        Ok(_) => {}
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to refill control failed when sending Terminate command ({e:?})"
            );
        }
    }

    // TankLevelSwitch thread has already ceased operation. It is now awaiting termination command.
    match tx_to_tank_level_switch.send(InternalCommand::Terminate) {
        Ok(_) => {}
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to TankLevelSwitch failed when sending Terminate command ({e:?})"
            );
        }
    }

    // Messaging is checking for Quit signal periodically.
    match tx_to_messaging.send(InternalCommand::Quit) {
        Ok(_) => {}
        Err(e) => {
            error!(
                target: LOG_TARGET_SIGNAL_HANDLER,
                "channel communication to Messaging failed when sending Terminate command ({e:?})"
            );
        }
    }
}
