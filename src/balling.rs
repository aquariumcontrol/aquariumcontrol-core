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

//use crate::channel_content::ActuatorState;
//use crate::channel_content::AquariumDevice;
use crate::channel_content::InternalCommand;
use crate::sql_interface_balling::SqlInterfaceBalling;

/// Holds the configuration data for the balling dosing control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct BallingConfig {
    /// control will only be active when the flag is set to true
    active: bool,

    /// interval for the main control in seconds
    schedule_check_interval: u32,

    /// inject Balling minerals with pump 1 after each period    
    dosing_interval_pump1: u32,

    /// inject Balling minerals with pump 2 after each period    
    dosing_interval_pump2: u32,

    /// inject Balling minerals with pump 3 after each period    
    dosing_interval_pump3: u32,

    /// inject Balling minerals with pump 4 after each period    
    dosing_interval_pump4: u32,
}

/// Checks if there are new messages in the channel.
/// if the message contains the quit command, then the function returns true, otherwise false.
/// # Arguments
/// * `rx_from_signal_handler` - receiver part of channel for communication from signal handler
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

/// Contains the configuration and the implementation for the Balling dosing control.
pub struct Balling {
    /// configuration data for Balling dosing control
    config: BallingConfig,

    /// SQL database interface for Balling dosing control
    sql_interface_balling: SqlInterfaceBalling,

    /// Count down in seconds until next Balling dosing for pump 1
    pub countdown_dosing_pump1: u32,

    /// Count down in seconds until next Balling dosing for pump 2
    pub countdown_dosing_pump2: u32,

    /// Count down in seconds until next Balling dosing for pump 3
    pub countdown_dosing_pump3: u32,

    /// Count down in seconds until next Balling dosing for pump 4
    pub countdown_dosing_pump4: u32,
}

impl Balling {
    /// Provides a struct of type Balling.
    /// # Arguments
    /// * `config` - configuration data for Balling dosing control
    /// * `sql_interface_balling` - specific SQL interface for Balling dosing control           
    pub fn new(config: BallingConfig, mut sql_interface_balling: SqlInterfaceBalling) -> Balling {
        let countdown_dosing_pump1 =
            Self::calc_countdown_next_dosing(&config, &mut sql_interface_balling, 1);
        let countdown_dosing_pump2 =
            Self::calc_countdown_next_dosing(&config, &mut sql_interface_balling, 2);
        let countdown_dosing_pump3 =
            Self::calc_countdown_next_dosing(&config, &mut sql_interface_balling, 3);
        let countdown_dosing_pump4 =
            Self::calc_countdown_next_dosing(&config, &mut sql_interface_balling, 4);

        Balling {
            config: config,
            sql_interface_balling: sql_interface_balling,
            countdown_dosing_pump1: countdown_dosing_pump1,
            countdown_dosing_pump2: countdown_dosing_pump2,
            countdown_dosing_pump3: countdown_dosing_pump3,
            countdown_dosing_pump4: countdown_dosing_pump4,
        }
    }

    /// Retrieves duration since last dosing event from database and calculates
    /// # Arguments
    /// the remaining time in seconds until next dosing for a specific pump
    /// * `config` - reference to configuration data for Balling dosing control       
    /// * `sql_interface_balling` - reference to specific SQL interface for Balling dosing control           
    /// * `pump_id` - id of pump for which the count down shall be calculated
    fn calc_countdown_next_dosing(
        config: &BallingConfig,
        sql_interface_balling: &mut SqlInterfaceBalling,
        pump_id: i64,
    ) -> u32 {
        let target_dosing_interval = match pump_id {
            1 => config.dosing_interval_pump1,
            2 => config.dosing_interval_pump2,
            3 => config.dosing_interval_pump3,
            4 => config.dosing_interval_pump4,
            _ => {
                // Panic is happening at startup of system.
                panic!(
                    "Balling countdown calculation encountered invalid pump id = {}",
                    pump_id
                );
            }
        };
        if target_dosing_interval == 0 {
            panic!(
                "Invalid dosing interval for pump #{} in configuration.",
                pump_id
            );
        }

        let duration_since_balling_dosing: u32 = match sql_interface_balling
            .get_duration_since_last_balling_dosing(pump_id)
        {
            Ok(duration_opt) => match duration_opt {
                Some(duration) => match duration.try_into() {
                    Ok(c) => c,
                    Err(e) => {
                        println!("Error {e:?} occurred when reading last Balling dosing event duration for pump {} from database.", pump_id);
                        0
                    }
                },
                None => target_dosing_interval.into(),
            },
            Err(e) => {
                panic!("Could not get duration since last Balling dosing event: {e:?}");
            }
        };

        println!(
            "Balling: Duration since last Balling dosing for pump {} is {} seconds.",
            pump_id, duration_since_balling_dosing
        );

        // limit subtraction to 0
        let countdown = match target_dosing_interval.checked_sub(duration_since_balling_dosing) {
            None => 0,
            Some(diff) => diff,
        };

        println!(
            "Balling: Countdown for next Balling dosing of pump {} is {} seconds.",
            pump_id, countdown
        );

        countdown
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `mutex_device_scheduler_balling` - clone of the mutex for avoiding parallel device actuation.
    /// * `tx_to_controllino` - Sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Receiver part of channel for communication from Controllino
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    /// * `tx_to_schedule_check` - Sender part of channel for communication to schedule checker
    /// * `rx_from_schedule_check` - Receiver part of channel for communication from schedule checker               
    pub fn execute(
        &mut self,
        _mutex_device_scheduler_balling: Arc<Mutex<i32>>,
        _tx_to_controllino: Sender<InternalCommand>,
        _rx_from_controllino: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
        tx_to_schedule_check: Sender<InternalCommand>,
        rx_from_schedule_check: Receiver<bool>,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut loop_counter = 0;

        loop {
            if self.config.active && (loop_counter % self.config.schedule_check_interval == 0) {
                println!("Balling: executing main control.");
            }

            if check_pending_external_request(&rx_from_signal_handler) {
                break;
            }
            spin_sleeper.sleep(sleep_duration_hundred_millis);

            loop_counter += 1;
        }
        // Application received request to terminate, that is why the loop was left.
        // Answer to thread which sent the request for termination, so that shutdown can proceed further.
        println!("Balling: Sending Quit confirmation to signal handler.");
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                println!(
                    "Balling: Error in trying to send quit confirmation back to signal handler: {e:?}"
                );
            }
        };

        // This thread has channel connection to underlying threads (Controllino).
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

#[cfg(test)]
pub mod tests {
    //    use spin_sleep::SpinSleeper;
    //    use std::sync::mpsc;
    //    use std::sync::mpsc::{Receiver, Sender};
    //    use std::sync::{Arc, Mutex};
    //    use std::thread::scope;
    //    use std::time::Duration;

    //    use crate::channel_content::InternalCommand;
    use crate::balling::Balling;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::sql_interface::SqlInterface;
    use crate::sql_interface_balling::SqlInterfaceBalling;

    #[test]
    #[should_panic]
    pub fn test_balling_initialization_invalid_intervals() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_balling_panic.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_balling = SqlInterfaceBalling::new(sql_interface.get_connection());

        // empty the Balling dosing log table
        match SqlInterface::truncate_table(&mut sql_interface, "ballingdosinglog".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        // create test object
        let _ = Balling::new(config.balling, sql_interface_balling);
    }

    #[test]
    pub fn test_balling_initialization_empty_dosing_log() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_balling_empty.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_balling = SqlInterfaceBalling::new(sql_interface.get_connection());

        // empty the Balling dosing log table
        match SqlInterface::truncate_table(&mut sql_interface, "ballingdosinglog".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        // create test object
        let balling = Balling::new(config.balling, sql_interface_balling);

        // check if the countdown calculation was correct
        assert_eq!(balling.countdown_dosing_pump1, 0);
        assert_eq!(balling.countdown_dosing_pump2, 0);
        assert_eq!(balling.countdown_dosing_pump3, 0);
        assert_eq!(balling.countdown_dosing_pump4, 0);
    }

    #[test]
    pub fn test_balling_initialization_full_dosing_log() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_balling.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let mut sql_interface_balling = SqlInterfaceBalling::new(sql_interface.get_connection());

        // empty the Balling dosing log table
        match SqlInterface::truncate_table(&mut sql_interface, "ballingdosinglog".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }
        let naive_datetime_100secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 100);
        let naive_datetime_200secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 200);
        let naive_datetime_300secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 300);
        let naive_datetime_400secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 400);

        // insert balling dosing events
        match sql_interface_balling.insert_balling_event(naive_datetime_100secs_ago, 1, 0.5) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not insert Balling dosing event for pump #1 into database: {e:?}")
            }
        };
        match sql_interface_balling.insert_balling_event(naive_datetime_200secs_ago, 2, 0.5) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not insert Balling dosing event for pump #2 into database: {e:?}")
            }
        };
        match sql_interface_balling.insert_balling_event(naive_datetime_300secs_ago, 3, 0.5) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not insert Balling dosing event for pump #3 into database: {e:?}")
            }
        };
        match sql_interface_balling.insert_balling_event(naive_datetime_400secs_ago, 4, 0.5) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not insert Balling dosing event for pump #4 into database: {e:?}")
            }
        };

        // create test object
        let balling = Balling::new(config.balling, sql_interface_balling);

        // check if the countdown calculation was correct
        assert_eq!(balling.countdown_dosing_pump1, 100);
        assert_eq!(balling.countdown_dosing_pump2, 0);
        assert_eq!(balling.countdown_dosing_pump3, 0);
        assert_eq!(balling.countdown_dosing_pump4, 0);
    }
}
