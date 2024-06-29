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

use crate::channel_content::InternalCommand;
use crate::sql_interface_feed::SqlInterfaceFeed;
use crate::food_injection::FoodInjectionTrait;

/// Holds the configuration data for the feed control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct FeedConfig {
    /// control will only be active when the flag is set to true
    active: bool,

    /// interval for the main control in seconds    
    schedule_check_interval: u32,
}

/// Checks if there are new messages in the channel.
/// if the message contains the quit command, then the function returns true, otherwise false.
/// # Arguments
/// * `rx_from_signal_handler` - receiver part of channel    
fn check_pending_external_request(rx_from_signal_handler: &Receiver<InternalCommand>) -> bool {
    match rx_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Quit => true,
            _ => {
                println!("Feed: ignoring inapplicable command from signal handler.");
                false
            }
        },
        Err(e) => {
            match e {
                TryRecvError::Empty => { /* empty buffer - no action required */ }
                TryRecvError::Disconnected => {
                    println!("Feed: Error in trying to receive command from signal handler: {e:?}");
                }
            }
            false
        }
    }
}

/// Contains the configuration and the implementation for the feed control.
pub struct Feed {
    /// configuration data for Feed control
    config: FeedConfig,

    /// SQL interface for the feed control
    sql_interface_feed: SqlInterfaceFeed,
}

impl Feed {
    /// Provides a struct of type Feed
    /// # Arguments
    /// * `config` - configuration data for feed control
    /// * `sql_interface_feed` - specific SQL interface for feed control           
    pub fn new(config: FeedConfig, sql_interface_feed: SqlInterfaceFeed) -> Feed {
        Self {
            config: config,
            sql_interface_feed: sql_interface_feed,
        }
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `mutex_device_scheduler_feed` - clone of the mutex for avoiding parallel device actuation.
    /// * `tx_to_controllino` - Sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Receiver part of channel for communication from Controllino
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler  
    pub fn execute(
        &mut self,
        _mutex_device_scheduler_feed: Arc<Mutex<i32>>,
        tx_to_controllino: Sender<InternalCommand>,
        rx_from_controllino: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
        food_injection: &mut impl FoodInjectionTrait,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut loop_counter = 0;

        loop {
            if self.config.active
                && (loop_counter % (self.config.schedule_check_interval * 10) == 0)
            {
                println!("Feed: checking schedule...");
                match self
                    .sql_interface_feed
                    .get_feedschedule_entry_from_database()
                {
                    Ok(c) => {
                        match c {
                            Some(mut feed_schedule_entry) => {
                                println!(
                                    "Feed schedule entry before update: \
                                timestamp={}, profile_name={}, repeat_daily={}",
                                    feed_schedule_entry.timestamp,
                                    feed_schedule_entry.profile_name,
                                    feed_schedule_entry.repeat_daily
                                );
                                // load feed pattern from database
                                match self
                                    .sql_interface_feed
                                    .get_single_feedpattern_from_database(feed_schedule_entry.profile_id)
                                {
                                    Ok(feed_pattern) => {
                                        //*** execute feed pattern ***
                                        food_injection.inject_food(
                                            &tx_to_controllino,
                                            &rx_from_controllino,
                                            &rx_from_signal_handler,
                                            &self.config,
                                            &feed_pattern,                                                
                                        );
                                        //*** update schedule ***
                                        match self
                                            .sql_interface_feed
                                            .update_feedschedule_entry_in_database(&mut feed_schedule_entry)
                                        {
                                            Ok(()) => {
                                                println!("Update feed schedule entry");
                                            }
                                            Err(e) => {
                                                println!("Error when updating feed schedule entry: {e:?}");
                                            }
                                        }

                                        //*** insert feed event in log ***
                                        match self.sql_interface_feed.insert_feed_event(
                                            feed_schedule_entry.timestamp,
                                            feed_pattern.calc_feeder_runtime(),
                                            feed_schedule_entry.profile_name,
                                            feed_schedule_entry.profile_id,
                                        ) {
                                            Ok(_) => {}
                                            Err(e) => {
                                                panic!("Could not insert feed event into database: {e:?}")
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        println!(
                                            "Error when reading feed pattern from database: {e:?}"
                                        );
                                    }
                                }
                            }
                            None => {
                                println!(
                                    "Database has no feed schedule entry which lies in the past."
                                );
                            }
                        }
                    }
                    Err(e) => {
                        println!("Could not retrieve feed schedule information from database: {e:?}");
                    }
                };
            }
            if check_pending_external_request(&rx_from_signal_handler) {
                break;
            }

            spin_sleeper.sleep(sleep_duration_hundred_millis);

            loop_counter += 1;
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        println!("Feed: Sending Quit confirmation to signal handler.");
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                println!(
                    "Feed: Error in trying to send quit confirmation back to signal handler: {e:?}"
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
                        println!("Feed: ignoring inapplicable command from signal handler.");
                        false
                    }
                },
                Err(e) => {
                    println!("Feed: error when trying to receive termination signal: {e:?}");
                    false
                }
            };
            spin_sleeper.sleep(sleep_duration_hundred_millis);
        }
    }
}
