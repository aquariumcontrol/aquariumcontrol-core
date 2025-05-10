/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(not(test))]
use log::info;

use log::{error, warn};

#[cfg(all(feature = "debug_feed", not(test)))]
use log::debug;

#[cfg(all(feature = "debug_signal_handler", not(test)))]
use log::debug;

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::database::sql_interface_feed::DatabaseInterfaceFeedTrait;
use crate::food::feed_schedule_entry::FeedScheduleEntry;
use crate::food::food_injection::FoodInjectionTrait;
use crate::utilities::channel_content::InternalCommand;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

const LOG_TARGET_FEED: &str = "feed";

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

    /// Describes the strategy to use when there are multiple feed schedule entries in the past.
    /// Using strategy_multiple_schedule_entries_in_past:
    /// negative value: execute none of them
    /// positive value: execute the xth entry according to how they are stacked in time
    /// Example 0: Execute the first entry which has a timestamp closest to the current time.
    /// Example 1: Execute the second last entry which lies closest to the current time.
    /// Example 99: Numbers higher than the number of entries found in the past will trigger execution of the
    /// last entry which has a timestamp furthest from the current time.
    strategy_multiple_schedule_entries_in_past: i32,
}

/// Struct collects the channels for communication between the feed control and the other components.
pub struct FeedChannels {
    /// Sender part of the channel for communication to Controllino
    pub tx_feed_to_controllino: Sender<InternalCommand>,

    /// Receiver part of the channel for communication from Controllino
    pub rx_feed_from_controllino: Receiver<bool>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_feed_to_signal_handler: Sender<bool>,

    /// Receiver part of the channel for communication from the signal handler  
    pub rx_feed_from_signal_handler: Receiver<InternalCommand>,

    /// Receiver part of the channel wrapped in Option for communication from the messaging  
    pub rx_feed_from_messaging_opt: Option<Receiver<InternalCommand>>,
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

    /// interface to SQL data base (trait object)
    sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send>,

    /// communication from trait implementation: request to execute a certain feed profile has been received
    pub execute_command_received: bool,

    /// communication from trait implementation: id of feed profile to be executed requested externally
    pub profile_id_requested: i32,

    /// flag inhibiting further execution of feed profiles. It is set whenever errors occur during feeding and can be reset by external command.
    pub execution_blocked: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read feed schedule entry from database
    lock_error_get_feedschedule_entry: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command
    lock_warn_inapplicable_command: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via channel
    lock_error_channel_receive_termination_signal: bool,
}

impl Feed {
    /// Provides a struct of type Feed
    /// # Arguments
    /// * `config` - configuration data for feed control
    /// * `sql_interface_feed` - interface to SQL database wrapped in Box
    pub fn new(
        config: FeedConfig,
        sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send>,
    ) -> Feed {
        Self {
            config,
            sql_interface_feed,
            execute_command_received: false,
            profile_id_requested: -1,
            execution_blocked: false,
            lock_error_get_feedschedule_entry: false,
            lock_warn_inapplicable_command: false,
            lock_error_channel_receive_termination_signal: false,
        }
    }

    #[cfg(all(feature = "debug_feed", not(test)))]
    /// Helper function to visualize a vector of feed schedule entries.
    /// # Arguments:
    /// * `feedschedule_entries` - reference to vector of feed schedule entries    
    fn feedschedule_entries_to_string(feedschedule_entries: Vec<FeedScheduleEntry>) -> String {
        let mut output: String = "Vec<FeedscheduleEntry>=".to_string();
        for (i, s) in feedschedule_entries.iter().enumerate() {
            output +=
                &(" #".to_owned() + &format!("{}: ID= {} ", i, s.profile_id) + &s.profile_name);
        }
        output
    }

    /// Loads and executes a given feed profile.
    /// # Arguments
    /// * `profile_id` - ID of feed profile to be executed
    /// * `food_injection` - Trait method for actuation to execute the feed profile
    /// * `tx_feed_to_controllino` - Reference to sender part of the channel for communication to Controllino
    /// * `rx_feed_from_controllino` - Reference to receiver part of the channel for communication from Controllino
    /// * `rx_feed_from_signal_handler` - Reference to receiver part of the channel for communication from signal handler  
    fn execute_feed(
        &mut self,
        profile_id: i32,
        food_injection: &mut impl FoodInjectionTrait,
        tx_feed_to_controllino: &Sender<InternalCommand>,
        rx_feed_from_controllino: &Receiver<bool>,
        rx_feed_from_signal_handler: &Receiver<InternalCommand>,
    ) -> (Result<(), ()>, bool) {
        let mut quit_command_received: bool = false;
        // load feed pattern from database
        #[allow(unused_variables)]
        let result = match self
            .sql_interface_feed
            .get_single_feedpattern_from_database(profile_id)
        {
            Ok(feed_pattern) => {
                //*** execute feed pattern ***
                quit_command_received = food_injection.inject_food(
                    tx_feed_to_controllino,
                    rx_feed_from_controllino,
                    rx_feed_from_signal_handler,
                    &feed_pattern,
                );

                let current_timestamp = match self.sql_interface_feed.get_current_timestamp() {
                    Ok(c) => c,
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_FEED,
                            "could not get current timestamp from database ({e:?})"
                        );
                        return (Err(()), quit_command_received);
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
                            target: LOG_TARGET_FEED,
                            "could not insert feed event into database ({e:?})"
                        );
                        Err(())
                    }
                }
            }
            Err(e) => {
                #[cfg(not(test))]
                error!(
                    target: LOG_TARGET_FEED,
                    "Error when reading feed pattern from database: {e:?}"
                );

                Err(())
            }
        };
        (result, quit_command_received)
    }

    /// Applies the configured strategy to select one of multiple feed schedule entries
    /// if there are multiple entries in the past.
    /// Using strategy_multiple_schedule_entries_in_past:
    /// negative value: execute none of them
    /// positive value: execute the xth entry according to how they are stacked in time
    /// Example 0: Execute the first entry which has a timestamp closest to the current time.
    /// Example 1: Execute the second last entry which lies closest to the current time.
    /// Example 99: Numbers higher than the number of entries found in the past will trigger execution of the
    /// last entry which has a timestamp furthest from the current time.
    /// # Arguments
    /// * `feedschedule_entries` - reference to a vector of feed schedule entries
    fn select_feed_schedule_entry_for_execution(
        &self,
        feedschedule_entries: &[FeedScheduleEntry],
    ) -> Option<FeedScheduleEntry> {
        if (self.config.strategy_multiple_schedule_entries_in_past >= 0)
            && (!feedschedule_entries.is_empty())
        {
            // user chose strategy to select one element
            if (self.config.strategy_multiple_schedule_entries_in_past as usize + 1)
                > feedschedule_entries.len()
            {
                // vector has not enough elements - choose the last element
                feedschedule_entries.last().cloned()
            } else {
                // vector has sufficient number of elements - choose as per parameter
                feedschedule_entries
                    .get(self.config.strategy_multiple_schedule_entries_in_past as usize)
                    .cloned()
            }
        } else {
            // user chose strategy to select no element
            None
        }
    }

    /// Execute the feed profile and update the SQL database.
    /// # Arguments
    /// * `profile_id` - Numeric ID of the feed profile to be executed
    /// * `tx_feed_to_controllino` - Reference to sender part of the channel for communication to Controllino
    /// * `rx_feed_from_controllino` - Reference to receiver part of the channel for communication from Controllino
    /// * `rx_feed_from_signal_handler` - Reference to receiver part of the channel for communication from signal handler  
    /// * `food_injection` - Trait method for actuation to execute the feed profile  
    fn execute_profile_update_database(
        &mut self,
        profile_id: i32,
        food_injection: &mut impl FoodInjectionTrait,
        tx_feed_to_controllino: &Sender<InternalCommand>,
        rx_feed_from_controllino: &Receiver<bool>,
        rx_feed_from_signal_handler: &Receiver<InternalCommand>,
        feed_schedule_entries: &mut Vec<FeedScheduleEntry>,
    ) -> bool {
        #[cfg(all(feature = "debug_feed", not(test)))]
        debug!(
            target: LOG_TARGET_FEED,
            "executing feed profile id #{}",
            profile_id
        );

        let (result, quit_command_received_during_feed) = self.execute_feed(
            profile_id,
            food_injection,
            tx_feed_to_controllino,
            rx_feed_from_controllino,
            rx_feed_from_signal_handler,
        );

        match result {
            Ok(_) => { /* do nothing */ }
            Err(_) => {
                // error occurred. Block subsequent executions
                self.execution_blocked = true;
            }
        }

        //*** update schedule ***
        match self
            .sql_interface_feed
            .update_feedschedule_entries_in_database(feed_schedule_entries)
        {
            Ok(()) => {
                // do nothing
            }
            #[cfg(not(test))]
            Err(e) => {
                error!(
                    target: LOG_TARGET_FEED,
                    "updating feed schedule entry failed ({e:?})"
                );
                self.execution_blocked = true;
            }
            #[cfg(test)]
            Err(_) => {
                self.execution_blocked = true;
            }
        }
        quit_command_received_during_feed
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `mutex_device_scheduler_feed` - clone of the mutex for avoiding parallel device actuation.
    /// * `feed_channels` - channels for communication with other threads
    /// * `food_injection` - Trait method for actuation to execute the feed profile  
    pub fn execute(
        &mut self,
        mutex_device_scheduler_feed: Arc<Mutex<i32>>,
        feed_channels: FeedChannels,
        food_injection: &mut impl FoodInjectionTrait,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut loop_counter = 0;
        let mut feed_inhibited: bool = false; // state of feed control determined by if start/stop command has been received
        let mut quit_command_received: bool = false; // request to end the application has been received
        let mut start_command_received: bool; // request to (re-)start feed has been received
        let mut stop_command_received: bool; // request to (temporarily) stop feed has been received

        loop {
            if self.config.active
                && !feed_inhibited
                && (loop_counter % (self.config.schedule_check_interval * 10) == 0)
            {
                #[cfg(all(feature = "debug_feed", not(test)))]
                debug!(
                    target: LOG_TARGET_FEED,
                    "Starting feed control cycle"
                );

                match self
                    .sql_interface_feed
                    .get_past_feedschedule_entries_from_database()
                {
                    Ok(c) => match c {
                        Some(mut feed_schedule_entries) => {
                            #[cfg(all(feature = "debug_feed", not(test)))]
                            debug!(
                                target: LOG_TARGET_FEED,
                                "Discovered following feed schedule entries in the past: {}",
                                Self::feedschedule_entries_to_string(feed_schedule_entries.clone())
                            );

                            let feed_schedule_entry_opt = self
                                .select_feed_schedule_entry_for_execution(&feed_schedule_entries);

                            match feed_schedule_entry_opt {
                                Some(feed_schedule_entry) => {
                                    #[cfg(all(feature = "debug_feed", not(test)))]
                                    debug!(
                                        target: LOG_TARGET_FEED,
                                        "Selected following feed schedule entry for execution: {}",
                                        feed_schedule_entry
                                    );

                                    self.lock_error_get_feedschedule_entry = false;

                                    if !self.execution_blocked {
                                        {
                                            // scope to limit lifetime of unlocked mutex
                                            let mut mutex_data =
                                                mutex_device_scheduler_feed.lock().unwrap();

                                            quit_command_received = self
                                                .execute_profile_update_database(
                                                    feed_schedule_entry.profile_id,
                                                    food_injection,
                                                    &feed_channels.tx_feed_to_controllino,
                                                    &feed_channels.rx_feed_from_controllino,
                                                    &feed_channels.rx_feed_from_signal_handler,
                                                    &mut feed_schedule_entries,
                                                );

                                            *mutex_data = mutex_data.saturating_add(1);
                                        }
                                    }
                                }
                                None => { /* do nothing - none of the feed schedule entries in the past has been selected */
                                }
                            }
                        }
                        None => { /* do nothing - no feed schedule entry in the past */ }
                    },
                    Err(e) => {
                        #[cfg(not(test))]
                        if !self.lock_error_get_feedschedule_entry {
                            error!(
                                target: LOG_TARGET_FEED,
                                "could not retrieve feed schedule information from database ({e:?})"
                            );
                            self.lock_error_get_feedschedule_entry = true;
                        }
                        #[cfg(test)]
                        let _e = e; // Explicitly declare and "use" with an underscore
                        self.lock_error_get_feedschedule_entry = true;
                    }
                };
            }

            // exit loop right after feed execution if quit command has been received meanwhile
            if quit_command_received {
                break;
            }

            // check and process external requests
            (
                quit_command_received,  // request to end the application has been received
                start_command_received, // request to (re-)start feed has been received
                stop_command_received,  // request to (temporarily) stop feed has been received
            ) = self.process_external_request(
                &feed_channels.rx_feed_from_signal_handler,
                feed_channels.rx_feed_from_messaging_opt.as_ref(),
            );
            if quit_command_received {
                break;
            }
            if stop_command_received {
                #[cfg(not(test))]
                info!(
                    target: LOG_TARGET_FEED,
                    "received Stop command. Inhibiting feed."
                );

                #[cfg(all(feature = "debug_feed", not(test)))]
                debug!(
                    target: LOG_TARGET_FEED,
                    "received Stop command."
                );

                feed_inhibited = true;
            }
            if start_command_received {
                #[cfg(not(test))]
                info!(
                    target: LOG_TARGET_FEED,
                    "received Start command. Restarting feed."
                );

                #[cfg(all(feature = "debug_feed", not(test)))]
                debug!(
                    target: LOG_TARGET_FEED,
                    "received Start command"
                );

                feed_inhibited = false;
            }
            if self.execute_command_received
                && self.profile_id_requested >= 0
                && !self.execution_blocked
            {
                #[cfg(all(feature = "debug_feed", not(test)))]
                debug!(
                    target: LOG_TARGET_FEED,
                    "received execution command"
                );

                {
                    // scope to limit lifetime of unlocked mutex
                    let mut mutex_data = mutex_device_scheduler_feed.lock().unwrap();

                    let (result, quit_command_received_during_requested_feed) = self.execute_feed(
                        self.profile_id_requested,
                        food_injection,
                        &feed_channels.tx_feed_to_controllino,
                        &feed_channels.rx_feed_from_controllino,
                        &feed_channels.rx_feed_from_signal_handler,
                    );
                    match result {
                        Ok(_) => { /* do nothing */ }
                        Err(_) => {
                            // error occurred. Block subsequent executions
                            self.execution_blocked = true;
                        }
                    }
                    quit_command_received = quit_command_received_during_requested_feed;
                    *mutex_data = mutex_data.saturating_add(1);
                }
            }
            if quit_command_received {
                break;
            }

            spin_sleeper.sleep(sleep_duration_hundred_millis);

            loop_counter += 1;

            // avoid overflow
            if loop_counter == 1_000_000 {
                loop_counter = 0;
            }
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        #[cfg(all(feature = "debug_signal_handler", not(test)))]
        debug!(
            target: LOG_TARGET_FEED,
            "Sending Quit confirmation to signal handler."
        );

        match feed_channels.tx_feed_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_FEED,
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // this thread has channel connection to underlying threads (Controllino).
        // These threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by signal_handler module.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match feed_channels.rx_feed_from_signal_handler.recv() {
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
                                    target: LOG_TARGET_FEED,
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
                            target: LOG_TARGET_FEED,
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
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::thread::scope;
    use std::time::Duration;

    use crate::database::sql_interface::SqlInterface;
    use crate::database::sql_interface_feed::{DatabaseInterfaceFeedTrait, SqlInterfaceFeed};
    use crate::food::feed::{Feed, FeedChannels, LOG_TARGET_FEED};
    use crate::food::feed_pattern::Feedpattern;
    use crate::food::feed_schedule_entry::FeedScheduleEntry;
    use crate::food::food_injection::FoodInjectionTrait;
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::config::{
        read_config_file, read_config_file_with_test_database, ConfigData,
    };
    use crate::utilities::logger::setup_logger;

    use crate::database::sql_interface_feed::tests::insert_feed_patterns;
    use crate::stubs::mock_sql_interface_feed::tests::MockSqlInterfaceFeed;

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
            _tx_feed_to_controllino: &Sender<InternalCommand>,
            _rx_feed_from_controllino: &Receiver<bool>,
            _rx_feed_from_signal_handler: &Receiver<InternalCommand>,
            feed_pattern: &Feedpattern,
        ) -> bool {
            // do nothing except for logging the profile id of the feed pattern
            self.profile_ids_executed.push(feed_pattern.profile_id);
            println!(
                "inject_food (mock): logging profile id {}",
                feed_pattern.profile_id
            );
            false
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
    // "happy" case executing a set of feed schedule entries
    // Test case uses test database #07.
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

        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            7,
        );

        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };

        let mut sql_interface_feed_unboxed = SqlInterfaceFeed::new(sql_interface.get_connection());

        insert_feed_patterns(&mut sql_interface, &mut sql_interface_feed_unboxed);

        let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(sql_interface_feed_unboxed);

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

            let feed_channels = FeedChannels {
                tx_feed_to_controllino,
                rx_feed_from_controllino,
                tx_feed_to_signal_handler,
                rx_feed_from_signal_handler,
                rx_feed_from_messaging_opt: None,
            };

            // thread for test object
            scope.spawn(move || {
                feed.execute(
                    mutex_device_scheduler_feed.clone(),
                    feed_channels,
                    &mut mock_food_injection,
                );

                // check if the right profiles have been executed in the right order
                assert_eq!(mock_food_injection.pop_profile_id_executed(), Some(2));
                assert_eq!(mock_food_injection.pop_profile_id_executed(), Some(1));
                assert_eq!(mock_food_injection.pop_profile_id_executed(), None);
            });
        });
    }

    #[test]
    // Test case runs feed control and triggers inhibition by sending stop message via channel.
    // After verification that feed is stopped,
    // test case sends start message via channel and verifies if feed control
    // resumes operation. Afterwards test case triggers execution of a specific feed profile
    // by sending "execute" message via channel.
    // Test case uses test database #08.
    pub fn test_messaging_stops_starts_execute_feed() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let sleep_duration_2_secs = Duration::from_secs(2);
        let sleep_duration_10_secs = Duration::from_secs(10);
        let spin_sleeper = SpinSleeper::default();

        setup_logger();

        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            8,
        );

        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };

        let mut sql_interface_feed_unboxed = SqlInterfaceFeed::new(sql_interface.get_connection());

        insert_feed_patterns(&mut sql_interface, &mut sql_interface_feed_unboxed);

        let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(sql_interface_feed_unboxed);

        // empty the feed schedule
        match SqlInterface::truncate_table(&mut sql_interface, "feedschedule".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let time_stamp_feed_schedule_entry =
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
            time_stamp_feed_schedule_entry,
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

        // create test object
        let mut feed = Feed::new(config.feed, sql_interface_feed);

        let (tx_feed_to_controllino, _rx_controllino_from_feed) = mpsc::channel();
        let (_tx_controllino_to_feed, rx_feed_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_feed, rx_feed_from_signal_handler) = mpsc::channel();
        let (tx_feed_to_signal_handler, rx_signal_handler_from_feed) = mpsc::channel();
        let (tx_messaging_to_feed, rx_feed_from_messaging) = mpsc::channel();

        let mut mock_food_injection = MockFoodInjection::new();

        let mutex_device_scheduler_feed_test_object = Arc::new(Mutex::new(0));
        let mutex_device_scheduler_feed_test_environment =
            mutex_device_scheduler_feed_test_object.clone();

        // thread for test environment
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                // wait to make sure test object can receive message
                spin_sleeper.sleep(sleep_duration_100_millisec);

                // sending message requesting stop of feed control
                match tx_messaging_to_feed.send(InternalCommand::Stop) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending stop command to test object ({e:?})",
                            LOG_TARGET_FEED
                        );
                    }
                }

                // wait for 10 seconds. If test object does not follow stop request,
                // then food injection will happen in this period.
                spin_sleeper.sleep(sleep_duration_10_secs);

                let actuation_count_1 =
                    *mutex_device_scheduler_feed_test_environment.lock().unwrap();

                // check amount of actuations (after stop request)
                assert_eq!(actuation_count_1, 0);

                // sending message requesting restart of feed control
                match tx_messaging_to_feed.send(InternalCommand::Start) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending start command to test object ({e:?})",
                            LOG_TARGET_FEED
                        );
                    }
                }

                // wait for 10 seconds. If test object follows start request,
                // then food injections will happen in this period.
                spin_sleeper.sleep(sleep_duration_10_secs);

                // get count of feed profiles executed
                let actuation_count_2 =
                    *mutex_device_scheduler_feed_test_environment.lock().unwrap();

                // check amount of actuations (after start request)
                assert_eq!(actuation_count_2, 1);

                // sending message requesting execution of feed profile
                match tx_messaging_to_feed.send(InternalCommand::Execute(1)) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending execution command to test object ({e:?})",
                            LOG_TARGET_FEED
                        );
                    }
                }

                // wait for 2 seconds. If test object follows execute request,
                // then food injection will happen in this period.
                spin_sleeper.sleep(sleep_duration_2_secs);

                // get count of feed profiles executed
                let actuation_count_3 =
                    *mutex_device_scheduler_feed_test_environment.lock().unwrap();

                // check amount of actuations (after execute request)
                assert_eq!(actuation_count_3, 2);

                // requesting feed control to quit
                let _ = tx_signal_handler_to_feed.send(InternalCommand::Quit);
                rx_signal_handler_from_feed.recv().unwrap();
                let _ = tx_signal_handler_to_feed.send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let feed_channels = FeedChannels {
                    tx_feed_to_controllino,
                    rx_feed_from_controllino,
                    tx_feed_to_signal_handler,
                    rx_feed_from_signal_handler,
                    rx_feed_from_messaging_opt: Some(rx_feed_from_messaging),
                };

                feed.execute(
                    mutex_device_scheduler_feed_test_object.clone(),
                    feed_channels,
                    &mut mock_food_injection,
                );
                println!("MockFoodInjection:\n{}", mock_food_injection);

                // check if the right feed profiles have been executed
                assert_eq!(mock_food_injection.profile_ids_executed.len(), 2);
                assert_eq!(mock_food_injection.profile_ids_executed[0], 1);
                assert_eq!(mock_food_injection.profile_ids_executed[1], 1);
            })
            .unwrap();

        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case runs feed control and triggers inhibition by using mock SQL interface which returns only errors.
    // This test case does not require any communication with SQL database.
    pub fn test_messaging_sql_error_inhibition_feed() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let sleep_duration_10_secs = Duration::from_secs(10);
        let spin_sleeper = SpinSleeper::default();

        setup_logger();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());

        // mock interface will be used in operation of test object
        let mock_sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(MockSqlInterfaceFeed::new());

        // create test object
        let mut feed = Feed::new(config.feed, mock_sql_interface_feed);

        let (tx_feed_to_controllino, _rx_controllino_from_feed) = mpsc::channel();
        let (_tx_controllino_to_feed, rx_feed_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_feed, rx_feed_from_signal_handler) = mpsc::channel();
        let (tx_feed_to_signal_handler, rx_signal_handler_from_feed) = mpsc::channel();

        let mut mock_food_injection = MockFoodInjection::new();

        let mutex_device_scheduler_feed_test_object = Arc::new(Mutex::new(0));
        let mutex_device_scheduler_feed_test_environment =
            mutex_device_scheduler_feed_test_object.clone();

        // thread for test environment
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                // wait for 10 seconds to be able to afterwards check
                // if test object still managed to execute a feed operation
                spin_sleeper.sleep(sleep_duration_10_secs);

                let actuation_count_1 =
                    *mutex_device_scheduler_feed_test_environment.lock().unwrap();

                // check amount of actuations (after stop request)
                assert_eq!(actuation_count_1, 0);

                // requesting feed control to quit
                let _ = tx_signal_handler_to_feed.send(InternalCommand::Quit);
                rx_signal_handler_from_feed.recv().unwrap();
                let _ = tx_signal_handler_to_feed.send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let feed_channels = FeedChannels {
                    tx_feed_to_controllino,
                    rx_feed_from_controllino,
                    tx_feed_to_signal_handler,
                    rx_feed_from_signal_handler,
                    rx_feed_from_messaging_opt: None,
                };

                feed.execute(
                    mutex_device_scheduler_feed_test_object.clone(),
                    feed_channels,
                    &mut mock_food_injection,
                );
                println!("MockFoodInjection:\n{}", mock_food_injection);

                // check if the right feed profiles have been executed
                assert_eq!(mock_food_injection.profile_ids_executed.len(), 0);
            })
            .unwrap();

        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    // Helper function that creates an array of feed schedule entries
    // with data from the past.
    fn create_mock_feedschedule_entries() -> Vec<FeedScheduleEntry> {
        let mut feedschedule_entries: Vec<FeedScheduleEntry> = Vec::new();

        feedschedule_entries.push(FeedScheduleEntry::new_for_test(
            "2024-10-24 10:00:00",
            77,
            "dummy1",
            true,
        ));
        feedschedule_entries.push(FeedScheduleEntry::new_for_test(
            "2024-10-24 11:00:00",
            78,
            "dummy2",
            true,
        ));
        feedschedule_entries.push(FeedScheduleEntry::new_for_test(
            "2024-10-24 12:00:00",
            79,
            "dummy3",
            true,
        ));

        feedschedule_entries
    }

    #[test]
    // Test case test selection of feed schedule entry in case there are multiple entries in the past
    // This test case tests the strategy to select none of them.
    // This test case does not require any communication with SQL database.
    pub fn test_select_feed_schedule_entry_for_execution_none() {
        setup_logger();
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.feed.strategy_multiple_schedule_entries_in_past = -1;

        // mock interface will be used in operation of test object
        let mock_sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(MockSqlInterfaceFeed::new());

        // create test object
        let feed = Feed::new(config.feed, mock_sql_interface_feed);

        // input for function to be tested
        let feedschedule_entries = create_mock_feedschedule_entries();

        assert_eq!(
            feed.select_feed_schedule_entry_for_execution(&feedschedule_entries),
            None
        );
    }

    #[test]
    // Test case test selection of feed schedule entry in case there are multiple entries in the past
    // This test case tests the strategy to select the first one.
    // This test case does not require any communication with SQL database.
    pub fn test_select_feed_schedule_entry_for_execution_first() {
        setup_logger();
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.feed.strategy_multiple_schedule_entries_in_past = 0;

        // mock interface will be used in operation of test object
        let mock_sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(MockSqlInterfaceFeed::new());

        // create test object
        let feed = Feed::new(config.feed, mock_sql_interface_feed);

        // input for function to be tested
        let feedschedule_entries = create_mock_feedschedule_entries();

        assert_eq!(
            feed.select_feed_schedule_entry_for_execution(&feedschedule_entries)
                .unwrap()
                .profile_id,
            77
        );
    }

    #[test]
    // Test case test selection of feed schedule entry in case there are multiple entries in the past
    // This test case tests the strategy to select a middle one.
    // This test case does not require any communication with SQL database.
    pub fn test_select_feed_schedule_entry_for_execution_middle() {
        setup_logger();
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.feed.strategy_multiple_schedule_entries_in_past = 1;

        // mock interface will be used in operation of test object
        let mock_sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(MockSqlInterfaceFeed::new());

        // create test object
        let feed = Feed::new(config.feed, mock_sql_interface_feed);

        // input for function to be tested
        let feedschedule_entries = create_mock_feedschedule_entries();

        assert_eq!(
            feed.select_feed_schedule_entry_for_execution(&feedschedule_entries)
                .unwrap()
                .profile_id,
            78
        );
    }

    #[test]
    // Test case tests selection of feed schedule entry in case there are multiple entries in the past.
    // This test case tests the strategy to select the last one.
    // This test case does not require any communication with SQL database.
    pub fn test_select_feed_schedule_entry_for_execution_last() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.feed.strategy_multiple_schedule_entries_in_past = 99;

        // mock interface will be used in operation of test object
        let mock_sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(MockSqlInterfaceFeed::new());

        // create test object
        let feed = Feed::new(config.feed, mock_sql_interface_feed);

        // input for function to be tested
        let feedschedule_entries = create_mock_feedschedule_entries();

        assert_eq!(
            feed.select_feed_schedule_entry_for_execution(&feedschedule_entries)
                .unwrap()
                .profile_id,
            79
        );
    }

    #[test]
    // Test case test implementation with multiple feed schedule entries in the past.
    // The youngest feed schedule entry shall be selected.
    // Test case uses test database #09.
    pub fn test_feed_schedule_execution_past_entries_select_youngest() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let sleep_duration_8_seconds = Duration::from_millis(10000);
        let spin_sleeper = SpinSleeper::default();

        // channels for the test object
        let (_tx_controllino_to_feed, rx_feed_from_controllino) = mpsc::channel();
        let (tx_feed_to_controllino, _rx_controllino_from_feed) = mpsc::channel();
        let (tx_signal_handler_to_feed, rx_feed_from_signal_handler) = mpsc::channel();
        let (tx_feed_to_signal_handler, rx_signal_handler_from_feed) = mpsc::channel();

        setup_logger();

        let mut mock_food_injection = MockFoodInjection::new();

        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            9,
        );

        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };

        let mut sql_interface_feed_unboxed = SqlInterfaceFeed::new(sql_interface.get_connection());

        insert_feed_patterns(&mut sql_interface, &mut sql_interface_feed_unboxed);

        let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(sql_interface_feed_unboxed);

        // empty the feed schedule table
        match SqlInterface::truncate_table(&mut sql_interface, "feedschedule".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let time_stamp_feed_schedule_entry_one_time =
            match SqlInterface::get_current_timestamp_offset_seconds(&mut sql_interface.conn, -2) {
                Ok(c) => c,
                Err(e) => {
                    panic!(
                        "test_feed_schedule_execution: Could not calculate timestamp for stimuli: {e:?}"
                    );
                }
            };

        let time_stamp_feed_schedule_entry_repeat =
            match SqlInterface::get_current_timestamp_offset_seconds(&mut sql_interface.conn, -4) {
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
            "testOneTimePast".to_string(),
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
            "testRepeatPast".to_string(),
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

            // thread for the test object
            scope.spawn(move || {
                let feed_channels = FeedChannels {
                    tx_feed_to_controllino,
                    rx_feed_from_controllino,
                    tx_feed_to_signal_handler,
                    rx_feed_from_signal_handler,
                    rx_feed_from_messaging_opt: None,
                };

                feed.execute(
                    mutex_device_scheduler_feed.clone(),
                    feed_channels,
                    &mut mock_food_injection,
                );

                // check if the right profiles have been executed in the right order
                assert_eq!(mock_food_injection.pop_profile_id_executed(), Some(1));
                assert_eq!(mock_food_injection.pop_profile_id_executed(), None);
            });
        });
    }

    #[test]
    // Test case test implementation with multiple feed schedule entries in the past.
    // The oldest feed schedule entry shall be selected.
    // Test case uses test database #10.
    pub fn test_feed_schedule_execution_past_entries_select_oldest() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let sleep_duration_8_seconds = Duration::from_millis(10000);
        let spin_sleeper = SpinSleeper::default();

        // channels for the test object
        let (_tx_controllino_to_feed, rx_feed_from_controllino) = mpsc::channel();
        let (tx_feed_to_controllino, _rx_controllino_from_feed) = mpsc::channel();
        let (tx_signal_handler_to_feed, rx_feed_from_signal_handler) = mpsc::channel();
        let (tx_feed_to_signal_handler, rx_signal_handler_from_feed) = mpsc::channel();

        setup_logger();

        let mut mock_food_injection = MockFoodInjection::new();

        let mut config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            10,
        );

        config.feed.strategy_multiple_schedule_entries_in_past = 99;

        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };

        let mut sql_interface_feed_unboxed = SqlInterfaceFeed::new(sql_interface.get_connection());

        insert_feed_patterns(&mut sql_interface, &mut sql_interface_feed_unboxed);

        let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(sql_interface_feed_unboxed);

        // empty the feed schedule table
        match SqlInterface::truncate_table(&mut sql_interface, "feedschedule".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let time_stamp_feed_schedule_entry_one_time =
            match SqlInterface::get_current_timestamp_offset_seconds(&mut sql_interface.conn, -2) {
                Ok(c) => c,
                Err(e) => {
                    panic!(
                        "test_feed_schedule_execution: Could not calculate timestamp for stimuli: {e:?}"
                    );
                }
            };

        let time_stamp_feed_schedule_entry_repeat =
            match SqlInterface::get_current_timestamp_offset_seconds(&mut sql_interface.conn, -4) {
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
            "testOneTimePast".to_string(),
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
            "testRepeatPast".to_string(),
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

            // thread for the test object
            scope.spawn(move || {
                let feed_channels = FeedChannels {
                    tx_feed_to_controllino,
                    rx_feed_from_controllino,
                    tx_feed_to_signal_handler,
                    rx_feed_from_signal_handler,
                    rx_feed_from_messaging_opt: None,
                };

                feed.execute(
                    mutex_device_scheduler_feed.clone(),
                    feed_channels,
                    &mut mock_food_injection,
                );

                // check if the right profiles have been executed in the right order
                assert_eq!(mock_food_injection.pop_profile_id_executed(), Some(2));
                assert_eq!(mock_food_injection.pop_profile_id_executed(), None);
            });
        });
    }
}
