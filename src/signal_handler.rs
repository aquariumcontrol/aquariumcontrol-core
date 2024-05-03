use crate::channel_content::InternalCommand;
use spin_sleep::SpinSleeper;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

/// This function handles the termination signal from operating system (SIGTERM)
/// Upon receiving the termination signal, it will send the internal command for quitting
/// to all threads in a dedicated order which allows safe shutdown.
pub fn handle_signals(tx_to_refill: Sender<InternalCommand>, rx_from_refill: Receiver<bool>) {
    let spin_sleeper = SpinSleeper::default();
    let sleep_interval_millis = 100;
    let sleep_duration_hundred_milli_sec = Duration::from_millis(sleep_interval_millis);
    let term = Arc::new(AtomicBool::new(false));
    match signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term)) {
        Ok(c) => {
            println!("Registered hook for signal {c:?}");
        }
        Err(e) => {
            panic!("handle_signals: Could not hook SIGTERM routine: {e:?}");
        }
    }

    while !term.load(Ordering::Relaxed) {
        spin_sleeper.sleep(sleep_duration_hundred_milli_sec);
    }

    match tx_to_refill.send(InternalCommand::Quit) {
        Ok(_) => {
            // now listen for response
            match rx_from_refill.recv() {
                Ok(_) => {}
                Err(e) => {
                    println!("handle_signals: could not receive answer from refill thread {e:?}")
                }
            };
        }
        Err(e) => {
            println!("handle_signals: Error in channel communication to Refill when sending termination request: {e:?}");
        }
    }
}
