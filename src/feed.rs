/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::{error, info, trace, warn};
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::channel_content::InternalCommand;
use crate::food_injection::FoodInjectionTrait;
use crate::proc_ext_req::ProcessExternalRequestTrait;
use crate::sql_interface::SqlInterface;
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

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the feed control.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     feed[Feed control] --> food_injection[Food injection] --> controllino[Controllino]
///     controllino --> food_injection
///     feed --> signal_handler[Signal handler]
///     signal_handler --> feed
///     signal_handler --> food_injection
///     messaging[Messaging] --> feed
/// ```
/// Communication channel to Controllino is forwarded to implementation of FoodInjectionTrait.
/// Signal handler is communicating both directly with feed as well with implementation of FoodInjectionTrait.

pub struct Feed {
    /// configuration data for Feed control
    config: FeedConfig,

    /// SQL interface for the feed control
    sql_interface_feed: SqlInterfaceFeed,

    /// communication from trait implementation: request to execute a certain feed profile has been received
    pub execute_command_received: bool,

    /// communication from trait implementation: id of feed profile to be executed requested externally
    pub profile_id_requested: i32,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read feed schedule entry from database
    lock_error_get_feedschedule_entry: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command
    lock_warn_inapplicable_command: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to rceive termination signal via channel
    lock_error_channel_receive_termination_signal: bool,
}

impl Feed {
    const LOG_TARGET_FEED: &'static str = "feed";

    /// Provides a struct of type Feed
    /// # Arguments
    /// * `config` - configuration data for feed control
    /// * `sql_interface_feed` - specific SQL interface for feed control           
    pub fn new(config: FeedConfig, sql_interface_feed: SqlInterfaceFeed) -> Feed {
        Self {
            config: config,
            sql_interface_feed: sql_interface_feed,
            execute_command_received: false,
            profile_id_requested: -1,
            lock_error_get_feedschedule_entry: false,
            lock_warn_inapplicable_command: false,
            lock_error_channel_receive_termination_signal: false,
        }
    }

    /// Loads and executes a given feed profile.
    /// # Arguments
    /// * `profile_id` - ID of feed profile to be executed
    /// * `tx_to_controllino` - Reference to sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Reference to receiver part of channel for communication from Controllino
    /// * `rx_from_signal_handler` - Reference to receiver part of channel for communication from signal handler  
    /// * `food_injection` - Trait method for actuation to execute the feed profile
    fn execute_feed(
        &mut self,
        profile_id: i32,
        food_injection: &mut impl FoodInjectionTrait,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
    ) -> Result<(), ()> {
        // load feed pattern from database
        match self
            .sql_interface_feed
            .get_single_feedpattern_from_database(profile_id)
        {
            Ok(feed_pattern) => {
                //*** execute feed pattern ***
                food_injection.inject_food(
                    &tx_to_controllino,
                    &rx_from_controllino,
                    &rx_from_signal_handler,
                    &feed_pattern,
                );

                let current_timestamp =
                    match SqlInterface::get_current_timestamp(&mut self.sql_interface_feed.conn) {
                        Ok(c) => c,
                        Err(e) => {
                            error!(
                                target: Self::LOG_TARGET_FEED,
                                "could not get current timestamp from database ({e:?})"
                            );
                            return Err(());
                        }
                    };

                //*** insert feed event in log ***
                match self.sql_interface_feed.insert_feed_event(
                    current_timestamp,
                    feed_pattern.calc_feeder_runtime(),
                    feed_pattern.profile_name,
                    profile_id,
                ) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        error!(
                            target: Self::LOG_TARGET_FEED,
                            "could not insert feed event into database ({e:?})"
                        );
                        Err(())
                    }
                }
            }
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_FEED,
                    "Error when reading feed pattern from database: {e:?}"
                );
                Err(())
            }
        }
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `mutex_device_scheduler_feed` - clone of the mutex for avoiding parallel device actuation.
    /// * `tx_to_controllino` - Sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Receiver part of channel for communication from Controllino
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler  
    /// * `rx_from_messaging` - Receiver part of channel for communication from messaging  
    /// * `food_injection` - Trait method for actuation to execute the feed profile  
    pub fn execute(
        &mut self,
        mutex_device_scheduler_feed: Arc<Mutex<i32>>,
        tx_to_controllino: Sender<InternalCommand>,
        rx_from_controllino: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
        rx_from_messaging: Receiver<InternalCommand>,
        food_injection: &mut impl FoodInjectionTrait,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut loop_counter = 0;
        let mut feed_inhibited: bool = false; // state of feed control determined by if start/stop command has been received
        let mut execution_blocked: bool = false;

        loop {
            if self.config.active
                && !feed_inhibited
                && (loop_counter % (self.config.schedule_check_interval * 10) == 0)
            {
                match self
                    .sql_interface_feed
                    .get_feedschedule_entry_from_database()
                {
                    Ok(c) => match c {
                        Some(mut feed_schedule_entry) => {
                            self.lock_error_get_feedschedule_entry = false;
                            //println!(
                            //    "Feed schedule entry before update: \
                            //    timestamp={}, profile_name={}, repeat_daily={}",
                            //    feed_schedule_entry.timestamp,
                            //    feed_schedule_entry.profile_name,
                            //    feed_schedule_entry.repeat_daily
                            //);

                            if !execution_blocked {
                                {
                                    // scope to limit lifetime of unlocked mutex
                                    let mut mutex_data =
                                        mutex_device_scheduler_feed.lock().unwrap();

                                    match self.execute_feed(
                                        feed_schedule_entry.profile_id,
                                        food_injection,
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                        &rx_from_signal_handler,
                                    ) {
                                        Ok(_) => { /* do nothing */ }
                                        Err(_) => {
                                            // error occurred. Block subsequent executions
                                            execution_blocked = true;
                                        }
                                    }

                                    *mutex_data += 1;

                                    //*** update schedule ***
                                    match self
                                        .sql_interface_feed
                                        .update_feedschedule_entry_in_database(
                                            &mut feed_schedule_entry,
                                        ) {
                                        Ok(()) => {
                                            // do nothing
                                            //println!("Updated feed schedule entry");
                                        }
                                        Err(e) => {
                                            error!(
                                                target: Self::LOG_TARGET_FEED,
                                                "updating feed schedule entry failed ({e:?})"
                                            );
                                            execution_blocked = true;
                                        }
                                    }
                                }
                            }
                        }
                        None => {
                            // do nothing
                            //println!("Database has no feed schedule entry which lies in the past.");
                        }
                    },
                    Err(e) => {
                        if !self.lock_error_get_feedschedule_entry {
                            error!(
                                target: Self::LOG_TARGET_FEED,
                                "could not retrieve feed schedule information from database ({e:?})"
                            );
                            self.lock_error_get_feedschedule_entry = true;
                        }
                    }
                };
            }

            // check and process external requests
            let (
                quit_command_received,  // request to end the application has been received
                start_command_received, // request to (re-)start feed has been received
                stop_command_received,  // request to (temporarily) stop feed has been received
            ) = self.process_external_request(&rx_from_signal_handler, Some(&rx_from_messaging));
            if quit_command_received {
                break;
            }
            if stop_command_received {
                info!(
                    target: Self::LOG_TARGET_FEED,
                    "received Stop command. Inhibiting feed."
                );
                feed_inhibited = true;
            }
            if start_command_received {
                info!(
                    target: Self::LOG_TARGET_FEED,
                    "received Start command. Restarting feed."
                );
                feed_inhibited = false;
            }
            if self.execute_command_received && self.profile_id_requested >= 0 && !execution_blocked
            {
                {
                    // scope to limit lifetime of unlocked mutex
                    let mut mutex_data = mutex_device_scheduler_feed.lock().unwrap();

                    match self.execute_feed(
                        self.profile_id_requested,
                        food_injection,
                        &tx_to_controllino,
                        &rx_from_controllino,
                        &rx_from_signal_handler,
                    ) {
                        Ok(_) => { /* do nothing */ }
                        Err(_) => {
                            // error occurred. Block subsequent executions
                            execution_blocked = true;
                        }
                    }
                    *mutex_data += 1;
                }
            }

            spin_sleeper.sleep(sleep_duration_hundred_millis);

            loop_counter += 1;
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        trace!(
            target: Self::LOG_TARGET_FEED,
            "Sending Quit confirmation to signal handler."
        );
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_FEED,
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // this thread has channel connection to underlying threads (Controllino).
        // These threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by signal_handler crate.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match rx_from_signal_handler.recv() {
                Ok(c) => {
                    self.lock_error_channel_receive_termination_signal = false;
                    match c {
                        InternalCommand::Terminate => {
                            self.lock_warn_inapplicable_command = false;
                            true
                        }
                        _ => {
                            if !self.lock_warn_inapplicable_command {
                                warn!(
                                    target: Self::LOG_TARGET_FEED,
                                    "ignoring inapplicable command from signal handler ({})",
                                    c
                                );
                                self.lock_warn_inapplicable_command = true;
                            }
                            false
                        }
                    }
                }
                Err(e) => {
                    if !self.lock_error_channel_receive_termination_signal {
                        error!(
                            target: Self::LOG_TARGET_FEED,
                            "receiving termination signal failed ({e:?})"
                        );
                        self.lock_error_channel_receive_termination_signal = true;
                    }
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
    use std::fmt;
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

    pub struct MockFoodInjection {
        pub profile_ids_executed: Vec<i32>,
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

    impl fmt::Display for MockFoodInjection {
        /// Provides output of the struct for debugging purposes.
        /// # Arguments
        /// * `f` - configuration for formatting
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let mut output: String = "Feed profile IDs: ".into();
            if self.profile_ids_executed.is_empty() {
                output = "No food has been injected.".to_string();
            } else {
                for id in &self.profile_ids_executed {
                    output += "#";
                    output += &id.to_string();
                    output += " ";
                }
            }
            write!(f, "{}", output)
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
        let (_tx_messaging_to_feed, rx_feed_from_messaging) = mpsc::channel();

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
                    rx_feed_from_messaging,
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
