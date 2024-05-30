/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

//use crate::channel_content::ActuatorState;
//use crate::channel_content::AquariumDevice;
use crate::channel_content::InternalCommand;
use crate::sql_interface_schedule::{ScheduleEntry, ScheduleType, SqlInterfaceSchedule};

/// Holds the configuration data for the schedule checker.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct ScheduleCheckConfig {
    /// interval for polling the SQL database
    schedule_check_interval: u32,

    /// strategy if entry could not be found in database:
    /// - true allows actuation
    /// - false inhibits actuation
    strategy_if_schedule_not_found: bool,
}

/// Checks if there are new messages in the channel from signal handler.
/// If the message contains the quit command, then the function returns true, otherwise false.
/// # Arguments
/// * `rx_from_signal_handler` - receiver part of channel for communication from signal handler
fn check_pending_external_request(rx_from_signal_handler: &Receiver<InternalCommand>) -> bool {
    match rx_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Quit => true,
            _ => {
                println!("ScheduleCheck: ignoring inapplicable command from signal handler.");
                false
            }
        },
        Err(e) => {
            match e {
                TryRecvError::Empty => { /* empty buffer - no action required */ }
                TryRecvError::Disconnected => {
                    println!(
                        "ScheduleCheck: Error in trying to receive command from signal handler: {e:?}"
                    );
                }
            }
            false
        }
    }
}

/// Contains the configuration and the implementation for the schedule checker.
pub struct ScheduleCheck {
    /// configuration data for schedule checker
    config: ScheduleCheckConfig,

    /// SQL database interface for Balling dosing control
    sql_interface_schedule: SqlInterfaceSchedule,
}

impl ScheduleCheck {
    /// Provides a struct of type ScheduleCheck.
    /// # Arguments
    /// * `config` - configuration data for Balling dosing control
    /// * `sql_interface_schedule` - specific SQL interface for ScheduleCheck           
    pub fn new(
        config: ScheduleCheckConfig,
        sql_interface_schedule: SqlInterfaceSchedule,
    ) -> ScheduleCheck {
        ScheduleCheck {
            config: config,
            sql_interface_schedule: sql_interface_schedule,
        }
    }

    /// Checks if there are new messages in the channel from either Balling, Refill, Ventilation or Heating.
    /// If the message contains the ScheduleCheck command, then the function returns a flag indicating
    /// if the thread is allowed to actuate the device.
    /// # Arguments
    /// * `tx` - sender part of channel for communication to one of the above mentioned threads
    /// * `rx` - receiver part of channel for communication from one of the above mentioned threads
    /// * `schedule_entry` - schedule data for the specific thread
    fn check_pending_schedule_request(
        &self,
        tx: &Sender<bool>,
        rx: &Receiver<InternalCommand>,
        schedule_entry: Option<&ScheduleEntry>,
    ) {
        match tx.send (
            match rx.try_recv() {
            Ok(c) => match c {
                InternalCommand::ScheduleCheck => {
                    match schedule_entry {
                        Some(c) => c.check_if_allowed(),
                        None => self.config.strategy_if_schedule_not_found,
                    }
                },
                _ => {
                    println!("ScheduleCheck: ignoring inapplicable command from signal handler.");
                    return;
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        println!(
                            "ScheduleCheck: Error in trying to receive command from signal handler: {e:?}"
                        );
                    }
                }
                return;
            }
        }
        ) {
            Ok(_) => { /* nothing to be done */ },
            Err(e) => {
                println!("ScheduleCheck: Error in sending back message.");
            }
        };
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `tx_to_ventilation` - Sender part of channel for communication to ventilation
    /// * `rx_from_ventilation` - Receiver part of channel for communication from ventilation
    /// * `tx_to_heating` - Sender part of channel for communication to heating
    /// * `rx_from_heating` - Receiver part of channel for communication from heating
    /// * `tx_to_refill` - Sender part of channel for communication to refill
    /// * `rx_from_refill` - Receiver part of channel for communication from refill
    /// * `tx_to_balling` - Sender part of channel for communication to Balling dosing control
    /// * `rx_from_balling` - Receiver part of channel for communication from Balling dosing control
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    pub fn execute(
        &mut self,
        tx_to_ventilation: Sender<bool>,
        rx_from_ventilation: Receiver<InternalCommand>,
        tx_to_heating: Sender<bool>,
        rx_from_heating: Receiver<InternalCommand>,
        tx_to_refill: Sender<bool>,
        rx_from_refill: Receiver<InternalCommand>,
        tx_to_balling: Sender<bool>,
        rx_from_balling: Receiver<InternalCommand>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut loop_counter = 0;

        loop {
            if loop_counter % self.config.schedule_check_interval * 10 == 0 {
                println!("ScheduleCheck: polling SQL database.");
            }

            if check_pending_external_request(&rx_from_signal_handler) {
                break;
            }

            let schedule_entry_ventilation_opt = match self
                .sql_interface_schedule
                .find_schedule(ScheduleType::Ventilation)
            {
                Ok(c) => Some(c),
                Err(e) => {
                    println!("ScheduleCheck: could not find schedule for Ventilation.");
                    None
                }
            };
            self.check_pending_schedule_request(
                &tx_to_ventilation,
                &rx_from_ventilation,
                schedule_entry_ventilation_opt,
            );

            let schedule_entry_heating_opt = match self
                .sql_interface_schedule
                .find_schedule(ScheduleType::Heating)
            {
                Ok(c) => Some(c),
                Err(e) => {
                    println!("ScheduleCheck: could not find schedule for Heating.");
                    None
                }
            };
            self.check_pending_schedule_request(
                &tx_to_heating,
                &rx_from_heating,
                schedule_entry_heating_opt,
            );

            let schedule_entry_refill_opt = match self
                .sql_interface_schedule
                .find_schedule(ScheduleType::Refill)
            {
                Ok(c) => Some(c),
                Err(e) => {
                    println!("ScheduleCheck: could not find schedule for Refill.");
                    None
                }
            };
            self.check_pending_schedule_request(
                &tx_to_refill,
                &rx_from_refill,
                schedule_entry_refill_opt,
            );

            let schedule_entry_balling_opt = match self
                .sql_interface_schedule
                .find_schedule(ScheduleType::Balling)
            {
                Ok(c) => Some(c),
                Err(e) => {
                    println!("ScheduleCheck: could not find schedule for Balling.");
                    None
                }
            };
            self.check_pending_schedule_request(
                &tx_to_balling,
                &rx_from_balling,
                schedule_entry_balling_opt,
            );

            spin_sleeper.sleep(sleep_duration_hundred_millis);

            loop_counter += 1;
        }
        // Application received request to terminate, that is why the loop was left.
        // Answer to thread which sent the request for termination, so that shutdown can proceed further.
        println!("ScheduleCheck: Sending Quit confirmation to signal handler.");
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                println!(
                    "ScheduleCheck: Error in trying to send quit confirmation back to signal handler: {e:?}"
                );
            }
        };
    }
}
