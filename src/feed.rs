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
use crate::food_injection::FoodInjectionTrait;
use crate::sql_interface_feed::SqlInterfaceFeed;

/// Holds the configuration data for the feed control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct FeedConfig {
    /// control will only be active when the flag is set to true
    active: bool,

    /// interval for the main control in seconds    
    schedule_check_interval: u32,

    /// waiting time in milliseconds before switching on or off a device during feed
    pub device_switch_delay_millis: u32,
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
        mutex_device_scheduler_feed: Arc<Mutex<i32>>,
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
                                    .get_single_feedpattern_from_database(
                                        feed_schedule_entry.profile_id,
                                    ) {
                                    Ok(feed_pattern) => {
                                        //*** execute feed pattern ***
                                        let mut mutex_data =
                                            mutex_device_scheduler_feed.lock().unwrap();
                                        food_injection.inject_food(
                                            &tx_to_controllino,
                                            &rx_from_controllino,
                                            &rx_from_signal_handler,
                                            &feed_pattern,
                                        );
                                        *mutex_data += 1;

                                        //*** update schedule ***
                                        match self
                                            .sql_interface_feed
                                            .update_feedschedule_entry_in_database(
                                                &mut feed_schedule_entry,
                                            ) {
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
                        println!(
                            "Could not retrieve feed schedule information from database: {e:?}"
                        );
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

#[cfg(test)]
pub mod tests {
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::Duration;

    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::feed::Feed;
    use crate::feedpattern::Feedpattern;
    use crate::food_injection::FoodInjectionTrait;
    use crate::sql_interface::SqlInterface;
    use crate::sql_interface_feed::SqlInterfaceFeed;

    struct MockFoodInjection {
        profile_ids_executed: Vec<i32>,
    }

    impl MockFoodInjection {
        pub fn new() -> MockFoodInjection {
            MockFoodInjection {
                profile_ids_executed: Vec::new(),
            }
        }

        pub fn pop_profile_id_executed(&mut self) -> Option<i32> {
            self.profile_ids_executed.pop()
        }
    }

    impl FoodInjectionTrait for MockFoodInjection {
        fn inject_food(
            &mut self,
            _tx_to_controllino: &Sender<InternalCommand>,
            _rx_from_controllino: &Receiver<bool>,
            _rx_from_signal_handler: &Receiver<InternalCommand>,
            feed_pattern: &Feedpattern,
        ) {
            // do nothing except for logging the profile id of the feed pattern
            self.profile_ids_executed.push(feed_pattern.profile_id);
            println!(
                "inject_food (mock): logging profile id {}",
                feed_pattern.profile_id
            );
        }
    }

    #[test]
    pub fn test_feed_schedule_execution() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let sleep_duration_8_seconds = Duration::from_millis(10000);
        let spin_sleeper = SpinSleeper::default();

        // channels for the test object
        let (_tx_controllino_to_feed, rx_feed_from_controllino) = mpsc::channel();
        let (tx_feed_to_controllino, _rx_controllino_from_feed) = mpsc::channel();
        let (tx_signal_handler_to_feed, rx_feed_from_signal_handler) = mpsc::channel();
        let (tx_feed_to_signal_handler, rx_signal_handler_from_feed) = mpsc::channel();

        let mut mock_food_injection = MockFoodInjection::new();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_feed.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let mut sql_interface_feed = SqlInterfaceFeed::new(sql_interface.get_connection());

        // empty the feed schedule table
        match SqlInterface::truncate_table(&mut sql_interface, "feedschedule".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let time_stamp_feed_schedule_entry_one_time =
            match SqlInterface::get_current_timestamp_offset_seconds(&mut sql_interface.conn, 2) {
                Ok(c) => c,
                Err(e) => {
                    panic!(
                        "test_feed_schedule_execution: Could not calculate timestamp for stimuli: {e:?}"
                    );
                }
            };

        let time_stamp_feed_schedule_entry_repeat =
            match SqlInterface::get_current_timestamp_offset_seconds(&mut sql_interface.conn, 4) {
                Ok(c) => c,
                Err(e) => {
                    panic!(
                        "test_feed_schedule_execution: Could not calculate timestamp for stimuli: {e:?}"
                    );
                }
            };

        // manipulation of database: inserting feed schedule entries with future timestamp
        match sql_interface_feed.insert_feed_schedule_entry(
            time_stamp_feed_schedule_entry_one_time,
            1,
            "testOneTime".to_string(),
            false,
            false,
        ) {
            Ok(()) => { /* do nothing */ }
            Err(e) => {
                panic!(
                    "test_feed_schedule_execution: Could not insert stimuli into database: {e:?}"
                );
            }
        };

        match sql_interface_feed.insert_feed_schedule_entry(
            time_stamp_feed_schedule_entry_repeat,
            2,
            "testRepeat".to_string(),
            false,
            true,
        ) {
            Ok(()) => { /* do nothing */ }
            Err(e) => {
                panic!(
                    "test_feed_schedule_execution: Could not insert stimuli into database: {e:?}"
                );
            }
        };

        // create test object
        let mut feed = Feed::new(config.feed, sql_interface_feed);

        scope(|scope| {
            let mutex_device_scheduler_feed = Arc::new(Mutex::new(0));

            // thread for mock signal handler
            scope.spawn(move || {
                spin_sleeper.sleep(sleep_duration_8_seconds);
                let _ = tx_signal_handler_to_feed.send(InternalCommand::Quit);
                rx_signal_handler_from_feed.recv().unwrap();
                let _ = tx_signal_handler_to_feed.send(InternalCommand::Terminate);
            });

            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                feed.execute(
                    mutex_device_scheduler_feed.clone(),
                    tx_feed_to_controllino,
                    rx_feed_from_controllino,
                    tx_feed_to_signal_handler,
                    rx_feed_from_signal_handler,
                    &mut mock_food_injection,
                );

                // check if the right profiles have been executed in the right order
                assert_eq!(mock_food_injection.pop_profile_id_executed(), Some(2));
                assert_eq!(mock_food_injection.pop_profile_id_executed(), Some(1));
                assert_eq!(mock_food_injection.pop_profile_id_executed(), None);

                println!("* [Feed] checking feed schedule execution succeeded.");
            });
        });
    }
}
