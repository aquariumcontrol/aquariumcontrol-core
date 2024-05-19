/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::channel_content::ActuatorState;
use crate::channel_content::AquariumDevice;
use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;
use crate::sql_interface_balling::SqlInterfaceBalling;

/// This struct holds the configuration data for the balling dosing control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct BallingConfig {
    active: bool,
}

/// function checks if there are new messages in the channel.
/// if the message contains the quit command, then the function returns true, otherwise false.
fn check_pending_external_request(rx_from_signal_handler: &Receiver<InternalCommand>) -> bool {
    match rx_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Quit => true,
            _ => {
                println!("Balling: ignoring inapplicable command from signal handler.");
                false
            }
        },
        Err(e) => {
            match e {
                TryRecvError::Empty => { /* empty buffer - no action required */ }
                TryRecvError::Disconnected => {
                    println!(
                        "Balling: Error in trying to receive command from signal handler: {e:?}"
                    );
                }
            }
            false
        }
    }
}

pub struct Balling {
    config: BallingConfig,
    sql_interface_balling: SqlInterfaceBalling,
}

impl Balling {
    pub fn new(config: BallingConfig, sql_interface_balling: SqlInterfaceBalling) -> Balling {
        Balling {
            config: config,
            sql_interface_balling: sql_interface_balling,
        }
    }

    pub fn execute(
        &mut self,
        mutex_device_scheduler_balling: Arc<Mutex<i32>>,
        tx_to_controllino: Sender<InternalCommand>,
        rx_from_controllino: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        loop {
            if check_pending_external_request(&rx_from_signal_handler) {
                break;
            }
            spin_sleeper.sleep(sleep_duration_hundred_millis);
        }
        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        println!("Balling: Sending Quit confirmation to signal handler.");
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                println!(
                    "Balling: Error in trying to send quit confirmation back to signal handler: {e:?}"
                );
            }
        };

        // this thread has channel connection to underlying threads (Controllino).
        // These threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by signal_handler crate.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match rx_from_signal_handler.recv() {
                Ok(c) => match c {
                    InternalCommand::Terminate => true,
                    _ => {
                        println!("Balling: ignoring inapplicable command from signal handler.");
                        false
                    }
                },
                Err(e) => {
                    println!("Balling: error when trying to receive termination signal: {e:?}");
                    false
                }
            };
            spin_sleeper.sleep(sleep_duration_hundred_millis);
        }
    }
}
