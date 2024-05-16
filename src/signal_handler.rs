use crate::channel_content::InternalCommand;
use spin_sleep::SpinSleeper;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

/// This function handles the termination signal from operating system (SIGTERM)
/// Upon receiving the termination signal, it will send the internal command for quitting
/// to all threads in a dedicated order which allows safe shutdown.
pub fn handle_signals(
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
            println!("signal_handler: Waiting for Refill thread to terminate.");
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

    // next, let's shut down the data logging
    match tx_to_datalogger.send(InternalCommand::Quit) {
        Ok(_) => {
            println!("signal_handler: Waiting for DataLogger thread to terminate.");
            // now listen for response
            match rx_from_datalogger.recv() {
                Ok(_) => {}
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
                Ok(_) => {}
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

    // Refill thread has already ceased operation. It is now awaiting termination command.
    match tx_to_refill.send(InternalCommand::Terminate) {
        Ok(_) => {
            println!("signal_handler: Sent termination signal to Refill thread.");
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Refill when sending Terminate command: {e:?}");
        }
    }
}
