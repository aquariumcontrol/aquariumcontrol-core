/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::channel_content::InternalCommand;
use spin_sleep::SpinSleeper;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

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
/// * `tx_to_gpio` - sender part of channel for communication to GPIO handler
/// * `rx_from_gpio` - receiver part of channel for communication from GPIO handler
/// * `tx_to_datalogger` - sender part of channel for communication to data logger
/// * `rx_from_datalogger` - receiver part of channel for communication from data logger
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
pub fn handle_signals(
    tx_to_messaging: Sender<InternalCommand>,
    tx_to_refill: Sender<InternalCommand>,
    rx_from_refill: Receiver<bool>,
    tx_to_heating: Sender<InternalCommand>,
    rx_from_heating: Receiver<bool>,
    tx_to_controllino: Sender<InternalCommand>,
    rx_from_controllino: Receiver<bool>,
    tx_to_gpio: Sender<InternalCommand>,
    rx_from_gpio: Receiver<bool>,
    tx_to_datalogger: Sender<InternalCommand>,
    rx_from_datalogger: Receiver<bool>,
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
        panic!("handle_signals: inconsistent channel parameters for TcpCommunication provided to handle_signals.");
    }

    let spin_sleeper = SpinSleeper::default();
    let sleep_interval_millis = 100;
    let sleep_duration_hundred_milli_sec = Duration::from_millis(sleep_interval_millis);
    let term = Arc::new(AtomicBool::new(false));
    for signal in signal_hook::consts::TERM_SIGNALS {
        match signal_hook::flag::register(*signal, Arc::clone(&term)) {
            Ok(c) => {
                println!("handle_signals: Registered hook for signal {c:?}");
            }
            Err(e) => {
                panic!("handle_signals: Could not hook SIGTERM routine: {e:?}");
            }
        }
    }

    while !term.load(Ordering::Relaxed) {
        spin_sleeper.sleep(sleep_duration_hundred_milli_sec);
    }
    println!("handle_signals: received termination signal.");

    // first, let's shut down the refill control
    match tx_to_refill.send(InternalCommand::Quit) {
        Ok(_) => {
            println!("signal_handler: Waiting for Refill thread to shut down.");
            // now listen for response
            match rx_from_refill.recv() {
                Ok(_) => {}
                Err(e) => {
                    println!("handle_signals: could not receive answer from refill thread {e:?}")
                }
            };
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Refill when sending Quit command: {e:?}");
        }
    }

    // second, let's shut down the heating control
    match tx_to_heating.send(InternalCommand::Quit) {
        Ok(_) => {
            println!("signal_handler: Waiting for Heating thread to shut down.");
            // now listen for response
            match rx_from_heating.recv() {
                Ok(_) => {}
                Err(e) => {
                    println!("handle_signals: could not receive answer from heating thread {e:?}")
                }
            };
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Heating when sending Quit command: {e:?}");
        }
    }

    // next, let's shut down the heating control
    match tx_to_ventilation.send(InternalCommand::Quit) {
        Ok(_) => {
            println!("signal_handler: Waiting for Ventilation thread to shut down.");
            // now listen for response
            match rx_from_ventilation.recv() {
                Ok(_) => {}
                Err(e) => {
                    println!(
                        "handle_signals: could not receive answer from ventilation thread {e:?}"
                    )
                }
            };
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Ventilation when sending Quit command: {e:?}");
        }
    }

    // next, let's shut down the monitors
    match tx_to_monitors.send(InternalCommand::Quit) {
        Ok(_) => {
            println!("signal_handler: Waiting for Monitors thread to shut down.");
            // now listen for response
            match rx_from_monitors.recv() {
                Ok(_) => {}
                Err(e) => {
                    println!("handle_signals: could not receive answer from Monitors thread {e:?}")
                }
            };
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Monitors when sending Quit command: {e:?}");
        }
    }

    // next, let's shut down the monitors
    match tx_to_feed.send(InternalCommand::Quit) {
        Ok(_) => {
            println!("signal_handler: Waiting for Feed thread to shut down.");
            // now listen for response
            match rx_from_feed.recv() {
                Ok(_) => {}
                Err(e) => {
                    println!("handle_signals: could not receive answer from Feed thread {e:?}")
                }
            };
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Feed when sending Quit command: {e:?}");
        }
    }

    // next, let's shut down the monitors
    match tx_to_balling.send(InternalCommand::Quit) {
        Ok(_) => {
            println!("signal_handler: Waiting for Balling thread to shut down.");
            // now listen for response
            match rx_from_balling.recv() {
                Ok(_) => {}
                Err(e) => {
                    println!("handle_signals: could not receive answer from Balling thread {e:?}")
                }
            };
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Balling when sending Quit command: {e:?}");
        }
    }

    // next, let's shut down the schedule checker
    match tx_to_schedule_check.send(InternalCommand::Quit) {
        Ok(_) => {
            println!("signal_handler: Waiting for ScheduleCheck thread to shut down.");
            // now listen for response
            match rx_from_schedule_check.recv() {
                Ok(_) => {}
                Err(e) => {
                    println!(
                        "handle_signals: could not receive answer from ScheduleCheck thread {e:?}"
                    )
                }
            };
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to ScheduleCheck when sending Quit command: {e:?}");
        }
    }

    // next, let's shut down the data logging
    match tx_to_datalogger.send(InternalCommand::Quit) {
        Ok(_) => {
            println!("signal_handler: Waiting for DataLogger thread to terminate.");
            // now listen for response
            match rx_from_datalogger.recv() {
                Ok(_) => {
                    println!("handle_signals: received Quit confirmation from data logger.");
                }
                Err(e) => {
                    println!(
                        "handle_signals: could not receive answer from datalogger thread {e:?}"
                    )
                }
            };
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to DataLogger when sending Quit command: {e:?}");
        }
    }

    // next, let's shut down Controllino
    match tx_to_controllino.send(InternalCommand::Quit) {
        Ok(_) => {
            println!("signal_handler: Waiting for Controllino thread to terminate.");
            // now listen for response
            match rx_from_controllino.recv() {
                Ok(_) => {
                    println!("handle_signals: received Quit confirmation from Controllino.");
                }
                Err(e) => {
                    println!(
                        "handle_signals: could not receive answer from Controllino thread {e:?}"
                    )
                }
            };
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Controllino when sending Quit command: {e:?}");
        }
    }

    // next, let's shut down GpioHandler
    match tx_to_gpio.send(InternalCommand::Quit) {
        Ok(_) => {
            println!("signal_handler: Waiting for Gpio thread to terminate.");
            // now listen for response
            match rx_from_gpio.recv() {
                Ok(_) => {}
                Err(e) => {
                    println!(
                        "handle_signals: could not receive answer from GpioHandler thread {e:?}"
                    )
                }
            };
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to GpioHandler when sending Quit command: {e:?}");
        }
    }

    // finally, let's shut down TcpCommunication
    match tx_to_tcp_opt {
        Some(tx_to_tcp) => match tx_to_tcp.send(InternalCommand::Quit) {
            Ok(_) => {
                println!("signal_handler: Waiting for TcpCommunication thread to terminate.");
                // now listen for response
                match rx_from_tcp_opt {
                    Some(rx_from_tcp) => match rx_from_tcp.recv() {
                        Ok(_) => {}
                        Err(e) => {
                            println!("handle_signals: could not receive answer from TcpCommunication thread {e:?}")
                        }
                    },
                    None => { /* do nothing */ }
                }
            }
            Err(e) => {
                println!("handle_signals: Error in channel communication to TcpCommunication when sending Quit command: {e:?}");
            }
        },
        None => { /* do nothing */ }
    }

    // DataLogger thread has already ceased operation. It is now awaiting termination command.
    match tx_to_datalogger.send(InternalCommand::Terminate) {
        Ok(_) => {
            println!("signal_handler: Sent termination signal to DataLogger thread.");
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to DataLogger when sending Terminate command: {e:?}");
        }
    }

    // Heating thread has already ceased operation. It is now awaiting termination command.
    match tx_to_heating.send(InternalCommand::Terminate) {
        Ok(_) => {
            println!("signal_handler: Sent termination signal to Heating thread.");
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Heating when sending Terminate command: {e:?}");
        }
    }

    // Ventilation thread has already ceased operation. It is now awaiting termination command.
    match tx_to_ventilation.send(InternalCommand::Terminate) {
        Ok(_) => {
            println!("signal_handler: Sent termination signal to Ventilation thread.");
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Ventilation when sending Terminate command: {e:?}");
        }
    }

    // Feed thread has already ceased operation. It is now awaiting termination command.
    match tx_to_feed.send(InternalCommand::Terminate) {
        Ok(_) => {
            println!("signal_handler: Sent termination signal to Feed thread.");
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Feed when sending Terminate command: {e:?}");
        }
    }

    // Balling thread has already ceased operation. It is now awaiting termination command.
    match tx_to_balling.send(InternalCommand::Terminate) {
        Ok(_) => {
            println!("signal_handler: Sent termination signal to Balling thread.");
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Balling when sending Terminate command: {e:?}");
        }
    }

    // Monitors thread has already ceased operation. It is now awaiting termination command.
    match tx_to_monitors.send(InternalCommand::Terminate) {
        Ok(_) => {
            println!("signal_handler: Sent termination signal to Monitors thread.");
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Monitors when sending Terminate command: {e:?}");
        }
    }

    // Refill thread has already ceased operation. It is now awaiting termination command.
    match tx_to_refill.send(InternalCommand::Terminate) {
        Ok(_) => {
            println!("signal_handler: Sent termination signal to Refill thread.");
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Refill when sending Terminate command: {e:?}");
        }
    }
    println!("signal_handler: finished.");

    // Messaging is checking for termination signal periodically.
    match tx_to_messaging.send(InternalCommand::Terminate) {
        Ok(_) => {
            println!("signal_handler: Sent termination signal to Messaging thread.");
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Messaging when sending Terminate command: {e:?}");
        }
    }
}
