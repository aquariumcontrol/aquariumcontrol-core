/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(not(test))]
use log::info;

#[cfg(all(feature = "debug_feed", not(test)))]
use log::debug;

#[cfg(all(feature = "debug_signal_handler", not(test)))]
use log::debug;

#[cfg(all(target_os = "linux", not(test)))]
use nix::unistd::gettid;

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::database::sql_interface_feed::DatabaseInterfaceFeedTrait;
pub(crate) use crate::food::feed_channels::FeedChannels;
use crate::food::feed_schedule_entry::FeedScheduleEntry;
use crate::food::food_injection::FoodInjectionTrait;
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;

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

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the feed control.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     feed[Feed Control] --> food_injection[Food Injection]
///     food_injection[Food Injection] --> relay_manager[Relay Manager]
///     relay_manager --> food_injection
///     feed --> signal_handler[Signal Handler]
///     signal_handler --> feed
///     signal_handler --> food_injection
///     messaging[Messaging] --> feed
/// ```
/// Communication channel to relay manager is forwarded to implementation of FoodInjectionTrait.
/// Signal handler is communicating with both directly: feed as well with implementation of FoodInjectionTrait.
pub struct Feed {
    /// Configuration data for Feed control
    config: FeedConfig,

    /// Interface to SQL database (trait object)
    sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send>,

    /// Communication from trait implementation: request to execute a certain feed profile has been received
    pub execute_command_received: bool,

    /// Communication from trait implementation: id of feed profile to be executed requested externally
    pub profile_id_requested: i32,

    /// Inhibition flag for further execution of feed profiles. It is set whenever errors occur during feeding and can be reset by external command.
    pub execution_blocked: bool,

    /// Inhibition flag to avoid flooding the log file with repeated messages about failure to read feed schedule entry from the database
    lock_error_get_feedschedule_entry: bool,

    /// recording when the last database ping happened
    pub last_ping_instant: Instant,

    /// database ping interval
    pub database_ping_interval: Duration,

    /// Inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command
    pub lock_warn_inapplicable_command_signal_handler: bool,

    /// Inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    pub lock_error_channel_receive_termination: bool
}

impl Feed {
    /// Creates a new `Feed` control instance.
    ///
    /// This constructor initializes the feed control module with its configuration
    /// and a database interface for managing feed-related data. It also sets
    /// initial states for command reception, execution blocking, and internal
    /// logging inhibition flags.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the feed control, loaded from a TOML file.
    /// * `sql_interface_feed` - A boxed trait object representing the interface
    ///   to the SQL database for feed-specific operations. This allows for
    ///   dependency injection and mock implementations during testing.
    /// * `database_ping_interval` - A `Duration` instance, providing the interval to ping the database.
    ///
    /// # Returns
    /// A new `Feed` struct, ready for operation within the application loop.
    pub fn new(
        config: FeedConfig,
        sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send>,
        database_ping_interval: Duration,
    ) -> Feed {
        Self {
            config,
            sql_interface_feed,
            execute_command_received: false,
            profile_id_requested: -1,
            execution_blocked: false,
            lock_error_get_feedschedule_entry: false,
            last_ping_instant: Instant::now(),
            database_ping_interval,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
        }
    }

    #[cfg(all(feature = "debug_feed", not(test)))]
    // Converts a vector of `FeedScheduleEntry` into a human-readable string for visualization.
    //
    // This helper function is active only in non-test builds when the `debug_feed` feature is enabled.
    // It iterates through the provided vector of feed schedule entries and formats them into a
    // single string, displaying their index, profile ID, and profile name. This is useful for
    // logging or debugging the contents of the feed schedule.
    //
    // # Arguments
    // * `feedschedule_entries` - A `Vec<FeedScheduleEntry>` (passed by value to allow internal iteration)
    //   containing the feed schedule entries to be visualized.
    //
    // # Returns
    // A `String` representation of the feed schedule entries.
    fn feedschedule_entries_to_string(feedschedule_entries: Vec<FeedScheduleEntry>) -> String {
        let mut output: String = "Vec<FeedscheduleEntry>=".to_string();
        for (i, s) in feedschedule_entries.iter().enumerate() {
            output +=
                &(" #".to_owned() + &format!("{}: ID= {} ", i, s.profile_id) + &s.profile_name);
        }
        output
    }

    /// Loads a specified feed profile from the database, executes it via food injection, and logs the event.
    ///
    /// This private helper function orchestrates the process of executing a single feed profile.
    /// It first attempts to retrieve the full `Feedpattern` from the database. If successful,
    /// it then calls the `food_injection` trait method to actuate the feeder according to the
    /// pattern. Finally, it records the completed feed event in the database.
    ///
    /// The function also monitors for a `Quit` command from the signal handler during the
    /// food injection process, indicating an early termination request.
    ///
    /// # Arguments
    /// * `profile_id` - The unique identifier of the feed profile to be loaded and executed.
    /// * `food_injection` - A mutable reference to an object implementing the `FoodInjectionTrait`,
    ///   responsible for the physical actuation of the feeder.
    /// * `feed_channels` - A mutable reference to the struct containing the channels.
    ///
    /// # Returns
    /// A tuple `(Result<(), ()>, bool)` where:
    /// - The first element is the overall result of the operation. It is `Ok(())` only if
    ///   the food injection and the database logging both complete without error.
    /// - The second element is a `bool,` which is `true` if a `Quit` command was received
    ///   from the signal handler during the `inject_food` process, `false` otherwise. This
    ///   value is independent of the success or failure of the operation itself.
    ///
    /// # Errors
    /// This function does not return a distinct error type. Instead, it returns `Err(())`
    /// in the result tuple to signify failure. The function can fail if:
    /// - It cannot read the specified feed pattern from the database.
    /// - The `food_injection.inject_food` method reports an error.
    /// - It cannot retrieve the current timestamp from the database after injection.
    /// - It fails to insert the feed event log into the database.
    ///
    /// In all failure cases, a detailed error message is logged to the console.
    fn execute_feed(
        &mut self,
        profile_id: i32,
        food_injection: &mut impl FoodInjectionTrait,
        feed_channels: &mut FeedChannels,
    ) -> (Result<(), ()>, bool) {
        // First, load the feed pattern from the database.
        let feed_pattern = match self
            .sql_interface_feed
            .get_single_feedpattern_from_database(profile_id)
        {
            Ok(pattern) => pattern,
            Err(e) => {
                log_error_chain(
                    module_path!(),
                    "Error occurred when trying to read feed pattern {profile_id} from database.",
                    e,
                );
                return (Err(()), false); // Return failure, no quit command received yet.
            }
        };

        // *** Execute the feed pattern and destructure the returned tuple ***
        // 1. `quit_during_injection`: A bool indicating if a quit command was received.
        // 2. `injection_result`: A Result indicating if the injection itself succeeded.
        let (quit_during_injection, injection_result) =
            food_injection.inject_food(feed_channels, &feed_pattern);
        // *** Handle the Result part of the tuple independently ***
        // If errors occurred during injection, log each one.
        if let Err(errors) = &injection_result {
            for error in errors {
                log_error_chain(module_path!(), "Food injection failed.", error);
            }
        }

        // Get the current timestamp to log the event, regardless of success/failure.
        let current_timestamp = match self.sql_interface_feed.get_current_timestamp() {
            Ok(c) => c,
            Err(e) => {
                log_error_chain(
                    module_path!(),
                    "Could not get current timestamp from database.",
                    e,
                );
                // If we can't get a timestamp, we can't log the event.
                // Return the original injection failure status and the quit status.
                let overall_result = if injection_result.is_err() {
                    Err(())
                } else {
                    Ok(())
                };
                return (overall_result, quit_during_injection);
            }
        };

        //*** Insert the feed event into the log ***
        let log_result = match self.sql_interface_feed.insert_feed_event(
            current_timestamp,
            feed_pattern.calc_feeder_runtime(),
            feed_pattern.profile_name,
            profile_id,
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                log_error_chain(
                    module_path!(),
                    "Could not insert feed event into database.",
                    e,
                );
                Err(())
            }
        };

        // Determine the final result. If either the injection or the logging failed,
        // the overall result is a failure.
        let final_result = if injection_result.is_err() || log_result.is_err() {
            Err(())
        } else {
            Ok(())
        };

        // Return the final result and the independent quit status.
        (final_result, quit_during_injection)
    }

    /// Selects a specific feed schedule entry for execution based on the configured strategy.
    ///
    /// This function is used when multiple `FeedScheduleEntry` items are found to be
    /// in the past (i.e., overdue for execution). It applies the `strategy_multiple_schedule_entries_in_past`
    /// from the `FeedConfig` to determine which single entry, if any, should be executed.
    ///
    /// The strategy is defined as follows:
    /// - **Negative value**: No entry will be selected for execution.
    /// - **Positive value (N)**: The N-th entry from the `feedschedule_entries` vector (0-indexed)
    ///   will be selected.
    ///   - If `N` is greater than or equal to the number of available entries, the *last* entry in the vector is selected.
    ///
    /// # Arguments
    /// * `feedschedule_entries` - A slice of `FeedScheduleEntry` structs, assumed to be ordered.
    ///
    /// # Returns
    /// An `Option<FeedScheduleEntry>`:
    /// - `Some(FeedScheduleEntry)`: The selected feed schedule entry to be executed.
    /// - `None`: If no entry is selected based on the strategy (e.g., negative strategy value,
    ///   or an empty `feedschedule_entries` vector).
    fn select_feed_schedule_entry_for_execution(
        &self,
        feedschedule_entries: &[FeedScheduleEntry],
    ) -> Option<FeedScheduleEntry> {
        // MODIFIED: Refactored for conciseness and idiomatic Rust.
        let strategy = self.config.strategy_multiple_schedule_entries_in_past;

        if strategy < 0 || feedschedule_entries.is_empty() {
            return None;
        }

        // Try to get the entry at the specified index.
        // If the index is out of bounds (`get` returns None), fall back to the last element.
        feedschedule_entries
            .get(strategy as usize)
            .or_else(|| feedschedule_entries.last())
            .cloned()
    }

    /// Executes a specified feed profile and updates the feed schedule in the database.
    ///
    /// This function is a core part of the feed control logic. It first calls `execute_feed`
    /// to perform the actual food injection and log the event. Based on the outcome of
    /// `execute_feed`, it updates the internal `execution_blocked` flag if an error occurs.
    /// Finally, it triggers the update or deletion of the corresponding entries in the
    /// feed schedule database.
    ///
    /// # Arguments
    /// * `profile_id` - The numeric identifier of the feed profile to be executed.
    /// * `food_injection` - A mutable reference to an object implementing `FoodInjectionTrait`,
    ///   responsible for physical feeder actuation.
    /// * `feed_channels` - A mutable reference to the struct `FoodInjectionTrait` containing the channels.
    /// * `rx_feed_from_relay_manager` - The receiver channel for receiving acknowledgments from the relay manager.
    /// * `rx_feed_from_signal_handler` - The receiver channel for listening to signals (e.g., `Quit`) from the signal handler.
    /// * `feed_schedule_entries` - A mutable reference to the vector of `FeedScheduleEntry`
    ///   that includes the entry(ies) just executed, which will be updated/deleted in the database.
    ///
    /// # Returns
    /// A `bool` which is `true` if a `Quit` command was received from the signal handler
    /// during the feed execution, indicating that the application should shut down; otherwise `false`.
    fn execute_profile_update_database(
        &mut self,
        profile_id: i32,
        food_injection: &mut impl FoodInjectionTrait,
        feed_channels: &mut FeedChannels,
        feed_schedule_entries: &mut Vec<FeedScheduleEntry>,
    ) -> bool {
        #[cfg(all(feature = "debug_feed", not(test)))]
        debug!(
            target: module_path!(),
            "executing feed profile id #{}",
            profile_id
        );

        let (result, quit_command_received_during_feed) =
            self.execute_feed(profile_id, food_injection, feed_channels);

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
                log_error_chain(module_path!(), "Updating feed schedule entry failed.", e);
                self.execution_blocked = true;
            }
            #[cfg(test)]
            Err(_) => {
                self.execution_blocked = true;
            }
        }
        quit_command_received_during_feed
    }

    /// Executes the main control loop for the feed module.
    ///
    /// This function runs continuously, managing the automatic execution of feed profiles
    /// based on a schedule, processing external commands, and ensuring graceful shutdown.
    /// It periodically checks the database for overdue feed schedule entries
    /// and executes selected profiles, while also handling `Start`, `Stop`, and `Execute`
    /// commands received from external channels.
    ///
    /// The loop breaks when a `Quit` command is received from the signal handler.
    /// After breaking, it sends a confirmation back to the signal handler and then
    /// waits for a `Terminate` command to ensure a complete shutdown.
    ///
    /// # Arguments
    /// * `mutex_device_scheduler_feed` - An `Arc<Mutex<i32>>` used for coordinating
    ///   access to device scheduling, preventing parallel actuation across different
    ///   control modules. It holds a counter of the completed actuation.
    /// * `feed_channels` - A mutable reference to `FeedChannels` struct containing all necessary `mpsc`
    ///   sender and receiver channels for inter-thread communication (e.g., with
    ///   the signal handler, relay manager, and messaging).
    /// * `food_injection` - A mutable reference to an object implementing the
    ///   `FoodInjectionTrait`, responsible for the physical dispensing of food.
    pub fn execute(
        &mut self,
        mutex_device_scheduler_feed: Arc<Mutex<i32>>,
        feed_channels: &mut FeedChannels,
        food_injection: &mut impl FoodInjectionTrait,
    ) {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut loop_counter = 0;
        let mut feed_inhibited: bool = false; // state of feed control determined by if the start/stop command has been received
        let mut quit_command_received: bool = false; // the request to end the application has been received
        let mut start_command_received: bool; // the request to (re-)start feed has been received
        let mut stop_command_received: bool; // the request to (temporarily) stop the feed has been received

        loop {
            if self.config.active
                && !feed_inhibited
                && (loop_counter % (self.config.schedule_check_interval * 10) == 0)
            {
                #[cfg(all(feature = "debug_feed", not(test)))]
                debug!(
                    target: module_path!(),
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
                                target: module_path!(),
                                "Discovered following feed schedule entries in the past: {}",
                                Self::feedschedule_entries_to_string(feed_schedule_entries.clone())
                            );

                            let feed_schedule_entry_opt = self
                                .select_feed_schedule_entry_for_execution(&feed_schedule_entries);

                            match feed_schedule_entry_opt {
                                Some(feed_schedule_entry) => {
                                    #[cfg(all(feature = "debug_feed", not(test)))]
                                    debug!(
                                        target: module_path!(),
                                        "Selected following feed schedule entry for execution: {}",
                                        feed_schedule_entry
                                    );

                                    self.lock_error_get_feedschedule_entry = false;

                                    if !self.execution_blocked {
                                        {
                                            // scope to limit the lifetime of unlocked mutex
                                            let mut mutex_data =
                                                mutex_device_scheduler_feed.lock().unwrap();

                                            quit_command_received = self
                                                .execute_profile_update_database(
                                                    feed_schedule_entry.profile_id,
                                                    food_injection,
                                                    feed_channels,
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
                            log_error_chain(
                                module_path!(),
                                "Could not retrieve feed schedule information from database.",
                                e,
                            );
                            self.lock_error_get_feedschedule_entry = true;
                        }
                        #[cfg(test)]
                        let _e = e; // Explicitly declare and "use" with an underscore
                        self.lock_error_get_feedschedule_entry = true;
                    }
                };
            }

            // exit loop right after feed execution if the quit command has been received meanwhile
            if quit_command_received {
                break;
            }

            // check and process external requests
            (
                quit_command_received,  // the request to end the application has been received
                start_command_received, // the request to (re-)start feed has been received
                stop_command_received, // the request to (temporarily) stop the feed has been received
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
                    target: module_path!(),
                    "received Stop command. Inhibiting feed."
                );

                #[cfg(all(feature = "debug_feed", not(test)))]
                debug!(
                    target: module_path!(),
                    "received Stop command."
                );

                feed_inhibited = true;
            }
            if start_command_received {
                #[cfg(not(test))]
                info!(
                    target: module_path!(),
                    "received Start command. Restarting feed."
                );

                #[cfg(all(feature = "debug_feed", not(test)))]
                debug!(
                    target: module_path!(),
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
                    target: module_path!(),
                    "received execution command"
                );

                {
                    // scope to limit the lifetime of unlocked mutex
                    let mut mutex_data = mutex_device_scheduler_feed.lock().unwrap();

                    let (result, quit_command_received_during_requested_feed) =
                        self.execute_feed(self.profile_id_requested, food_injection, feed_channels);
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

            // Check for how long ago the last database ping happened.
            if Instant::now().duration_since(self.last_ping_instant) > self.database_ping_interval {
                self.sql_interface_feed.ping();
                self.last_ping_instant = Instant::now();
            }
        }

        // The application received request to terminate. That is why the loop was left.
        // Answer to the thread which sent the request for termination, so that shutdown can proceed further.
        #[cfg(all(feature = "debug_signal_handler", not(test)))]
        debug!(
            target: module_path!(),
            "Sending Quit confirmation to signal handler."
        );

        match feed_channels.send_to_signal_handler(true) {
            Ok(()) => {}
            Err(e) => {
                log_error_chain(
                    module_path!(),
                    "Sending quit confirmation back to signal handler failed.",
                    e,
                );
            }
        };

        // This thread has channel connections to underlying threads.
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination(
            &feed_channels.rx_feed_from_signal_handler,
            sleep_duration_hundred_millis,
            module_path!()
        );
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
    use crate::food::feed::{Feed, FeedChannels};
    use crate::food::feed_pattern::Feedpattern;
    use crate::food::feed_schedule_entry::FeedScheduleEntry;
    use crate::food::food_injection::FoodInjectionTrait;
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::config::{
        read_config_file, read_config_file_with_test_database, ConfigData,
    };
    use crate::utilities::logger::{setup_logger, LoggerConfig};

    use crate::database::sql_interface_feed::tests::insert_feed_patterns;
    use crate::database::sql_query_strings::{SQL_TABLE_FEEDLOG, SQL_TABLE_FEEDSCHEDULE};
    use crate::food::food_injection_error::FoodInjectionError;
    use crate::mocks::mock_sql_interface_feed::tests::MockSqlInterfaceFeed;

    pub struct MockFoodInjection {
        pub profile_ids_executed: Vec<i32>,
    }

    impl MockFoodInjection {
        // Creates a new `MockFoodInjection` instance.
        //
        // This constructor initializes the mock feeder, setting up an empty vector
        // to record the IDs of all feed profiles that are "executed" during testing.
        //
        // # Returns
        // A new `MockFoodInjection` struct.
        pub fn new() -> MockFoodInjection {
            MockFoodInjection {
                profile_ids_executed: Vec::new(),
            }
        }

        // Retrieves and removes the last recorded profile ID "executed" by the mock feeder.
        //
        // This method is primarily used in tests to inspect the order in which
        // feed profiles were processed by the `MockFoodInjection` instance.
        // It behaves like `Vec::pop()`, returning `None` if no profiles have been executed yet.
        //
        // # Returns
        // An `Option<i32>`:
        // - `Some(i32)` containing the last executed profile ID.
        // - `None` if the `profile_ids_executed` list is empty.
        pub fn pop_profile_id_executed(&mut self) -> Option<i32> {
            self.profile_ids_executed.pop()
        }
    }

    impl FoodInjectionTrait for MockFoodInjection {
        // Simulates the injection of food by recording the executed feed profile ID.
        //
        // This mock implementation of `inject_food` does not perform any physical actuation.
        // Instead, it serves as a testing utility by:
        // 1. Storing the `profile_id` of the `Feedpattern` that would have been "injected"
        //    into its internal `profile_ids_executed` vector.
        // 2. Printing a console message for debug visibility during test runs.
        // 3. Always returning `false` to indicate that no `Quit` command was received
        //    during this simulated injection process.
        //
        // The channel arguments are ignored in this mock implementation.
        //
        // # Arguments
        // * `_feed_channels` - (Ignored) mutable reference to struct containing the channels.
        // * `_rx_feed_from_relay_manager` - (Ignored) the receiver channel for acknowledgments from the relay manager.
        // * `_rx_feed_from_signal_handler` - (Ignored) the receiver channel for listening to signals from the signal handler.
        // * `feed_pattern` - The `Feedpattern` that contains the profile ID to be recorded.
        //
        // # Returns
        // Always returns `false`, indicating that no `Quit` command was received.
        fn inject_food(
            &mut self,
            _feed_channels: &mut FeedChannels,
            feed_pattern: &Feedpattern,
        ) -> (bool, Result<(), Vec<FoodInjectionError>>) {
            // do nothing except for logging the profile id of the feed pattern
            self.profile_ids_executed.push(feed_pattern.profile_id);
            println!(
                "inject_food (mock): logging profile id {}",
                feed_pattern.profile_id
            );
            (false, Ok(()))
        }
    }

    impl fmt::Display for MockFoodInjection {
        /// Formats the `MockFoodInjection` struct to display the IDs of executed feed profiles.
        ///
        /// This implementation is primarily for debugging and testing purposes. It provides
        /// a human-readable string listing all the feed profile IDs that this mock
        /// feeder has "executed" (i.e., recorded internally).
        ///
        /// # Arguments
        /// * `f` - A mutable reference to the formatter, as required by the `fmt::Display` trait.
        ///
        /// # Returns
        /// A `fmt::Result` indicating whether the formatting operation was successful.
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

    /// Helper function for test cases to simulate the signal handler.
    ///
    /// The function waits for a determined period and then sends the Quit and the Terminate
    /// commands to the test object.
    ///
    /// # Arguments
    /// * `tx_signal_handler_to_feed` - Sender part of the channel for communication to the test object
    /// * `rx_signal_handler_from_feed` - Receiver part of the channel for communication from the test object
    /// * `spin_sleeper` - SpinSleeper instance used for waiting before sending Quit command to the test object
    /// * `sleep_duration` - sleep duration used for waiting before sending Quit command to the test object
    fn create_mock_signal_handler(
        tx_signal_handler_to_feed: &Sender<InternalCommand>,
        rx_signal_handler_from_feed: &Receiver<bool>,
        spin_sleeper: SpinSleeper,
        sleep_duration: Duration,
    ) {
        spin_sleeper.sleep(sleep_duration);
        let _ = tx_signal_handler_to_feed.send(InternalCommand::Quit);
        rx_signal_handler_from_feed.recv().unwrap();
        let _ = tx_signal_handler_to_feed.send(InternalCommand::Terminate);
    }

    #[test]
    // "Happy" case executing a set of feed schedule entries. Additionally, it checks row limitation.
    // Test case uses test database #07.
    pub fn test_feed_schedule_execution() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let sleep_duration_8_seconds = Duration::from_millis(10000);
        let spin_sleeper = SpinSleeper::default();

        // channels for the test object
        let (_tx_relay_manager_to_feed, rx_feed_from_relay_manager) = mpsc::channel();
        let (tx_feed_to_relay_manager, _rx_relay_manager_from_feed) = mpsc::channel();
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

        let mut sql_interface_feed_unboxed = SqlInterfaceFeed::new(
            sql_interface.get_connection().unwrap(),
            config.sql_interface.max_rows_feed_pattern,
            config.sql_interface.max_rows_feed_schedule,
            config.sql_interface.max_rows_feed_log,
        )
        .unwrap();

        insert_feed_patterns(&mut sql_interface, &mut sql_interface_feed_unboxed);

        let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(sql_interface_feed_unboxed);

        // empty the feed schedule table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEEDSCHEDULE.to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }
        // empty the feed log table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEEDLOG.to_string()) {
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

        // manipulation of the database: inserting feed schedule entries with future timestamp
        sql_interface_feed
            .insert_feed_schedule_entry(
                time_stamp_feed_schedule_entry_one_time,
                1,
                "testOneTime".to_string(),
                false,
                false,
            )
            .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

        sql_interface_feed
            .insert_feed_schedule_entry(
                time_stamp_feed_schedule_entry_repeat,
                2,
                "testRepeat".to_string(),
                false,
                true,
            )
            .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

        let mut feed = Feed::new(config.feed, sql_interface_feed, Duration::from_millis(1000));

        scope(|scope| {
            let mutex_device_scheduler_feed = Arc::new(Mutex::new(0));

            // thread for mock signal handler
            scope.spawn(move || {
                create_mock_signal_handler(
                    &tx_signal_handler_to_feed,
                    &rx_signal_handler_from_feed,
                    spin_sleeper,
                    sleep_duration_8_seconds,
                );
            });

            spin_sleeper.sleep(sleep_duration_100_millis);

            let mut feed_channels = FeedChannels {
                tx_feed_to_relay_manager,
                #[cfg(feature = "debug_channels")]
                cnt_tx_feed_to_relay_manager: 0,

                rx_feed_from_relay_manager,
                #[cfg(feature = "debug_channels")]
                cnt_rx_feed_from_relay_manager: 0,

                tx_feed_to_signal_handler,
                #[cfg(feature = "debug_channels")]
                cnt_tx_feed_to_signal_handler: 0,

                rx_feed_from_signal_handler,
                #[cfg(feature = "debug_channels")]
                cnt_rx_feed_from_signal_handler: 0,

                rx_feed_from_messaging_opt: None,
                #[cfg(feature = "debug_channels")]
                cnt_rx_feed_from_messaging_opt: 0,
            };

            // thread for the test object
            scope.spawn(move || {
                feed.execute(
                    mutex_device_scheduler_feed.clone(),
                    &mut feed_channels,
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
    // Test case runs feed control and triggers inhibition by sending the stop message via the channel.
    // After verification that feed is stopped,
    // The test case sends the start message via the channel and verifies if feed control
    // resumes operation. Afterward, the test case triggers execution of a specific feed profile
    // by sending the "execute" message via the channel.
    // Test case uses test database #08.
    pub fn test_messaging_stops_starts_execute_feed() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let sleep_duration_2_secs = Duration::from_secs(2);
        let sleep_duration_10_secs = Duration::from_secs(10);
        let spin_sleeper = SpinSleeper::default();

        setup_logger(LoggerConfig::default()).unwrap();

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

        let mut sql_interface_feed_unboxed = SqlInterfaceFeed::new(
            sql_interface.get_connection().unwrap(),
            config.sql_interface.max_rows_feed_pattern,
            config.sql_interface.max_rows_feed_schedule,
            config.sql_interface.max_rows_feed_log,
        )
        .unwrap();

        insert_feed_patterns(&mut sql_interface, &mut sql_interface_feed_unboxed);

        let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(sql_interface_feed_unboxed);

        // empty the feed schedule
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEEDSCHEDULE.to_string()) {
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

        // manipulation of the database: inserting feed schedule entries with future timestamp
        sql_interface_feed
            .insert_feed_schedule_entry(
                time_stamp_feed_schedule_entry,
                1,
                "testOneTime".to_string(),
                false,
                false,
            )
            .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

        // create the test object
        let mut feed = Feed::new(config.feed, sql_interface_feed, Duration::from_millis(1000));

        let (tx_feed_to_relay_manager, _rx_relay_manager_from_feed) = mpsc::channel();
        let (_tx_relay_manager_to_feed, rx_feed_from_relay_manager) = mpsc::channel();
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
                // wait to make sure the test object can receive the message
                spin_sleeper.sleep(sleep_duration_100_millis);

                // sending the message requesting stop of feed control
                match tx_messaging_to_feed.send(InternalCommand::Stop) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending stop command to test object ({e:?})",
                            module_path!()
                        );
                    }
                }

                // Wait for 10 seconds. If the test object does not follow the stop request,
                // then food injection will happen in this period.
                spin_sleeper.sleep(sleep_duration_10_secs);

                let actuation_count_1 =
                    *mutex_device_scheduler_feed_test_environment.lock().unwrap();

                // check the amount of actuation (after stop request)
                assert_eq!(actuation_count_1, 0);

                // sending the message requesting restart of feed control
                match tx_messaging_to_feed.send(InternalCommand::Start) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending start command to test object ({e:?})",
                            module_path!()
                        );
                    }
                }

                // Wait for 10 seconds. If the test object follows the start request,
                // then food injections will happen in this period.
                spin_sleeper.sleep(sleep_duration_10_secs);

                // get count of feed profiles executed
                let actuation_count_2 =
                    *mutex_device_scheduler_feed_test_environment.lock().unwrap();

                // check the amount of actuation (after start request)
                assert_eq!(actuation_count_2, 1);

                // sending the message requesting execution of the feed profile
                match tx_messaging_to_feed.send(InternalCommand::Execute(1)) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending execution command to test object ({e:?})",
                            module_path!()
                        );
                    }
                }

                // Wait for 2 seconds. If the test object follows the "execute" request,
                // then food injection will happen in this period.
                spin_sleeper.sleep(sleep_duration_2_secs);

                // get count of feed profiles executed
                let actuation_count_3 =
                    *mutex_device_scheduler_feed_test_environment.lock().unwrap();

                // check the amount of actuation (after the "execute" request)
                assert_eq!(actuation_count_3, 2);

                // requesting feed control to quit
                let _ = tx_signal_handler_to_feed.send(InternalCommand::Quit);
                rx_signal_handler_from_feed.recv().unwrap();
                let _ = tx_signal_handler_to_feed.send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let mut feed_channels = FeedChannels {
                    tx_feed_to_relay_manager,
                    #[cfg(feature = "debug_channels")]
                    cnt_tx_feed_to_relay_manager: 0,

                    rx_feed_from_relay_manager,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_feed_from_relay_manager: 0,

                    tx_feed_to_signal_handler,
                    #[cfg(feature = "debug_channels")]
                    cnt_tx_feed_to_signal_handler: 0,

                    rx_feed_from_signal_handler,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_feed_from_signal_handler: 0,

                    rx_feed_from_messaging_opt: Some(rx_feed_from_messaging),
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_feed_from_messaging_opt: 0,
                };

                feed.execute(
                    mutex_device_scheduler_feed_test_object.clone(),
                    &mut feed_channels,
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
    // Test case runs feed control and triggers inhibition by using a mock SQL interface which returns only errors.
    // This test case does not require any communication with the database.
    pub fn test_messaging_sql_error_inhibition_feed() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let sleep_duration_10_secs = Duration::from_secs(10);
        let spin_sleeper = SpinSleeper::default();

        setup_logger(LoggerConfig::default()).unwrap();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

        // mock interface will be used in operation of the test object
        let mock_sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(MockSqlInterfaceFeed::new());

        // create the test object
        let mut feed = Feed::new(
            config.feed,
            mock_sql_interface_feed,
            Duration::from_millis(1000),
        );

        let (tx_feed_to_relay_manager, _rx_relay_manager_from_feed) = mpsc::channel();
        let (_tx_relay_manager_to_feed, rx_feed_from_relay_manager) = mpsc::channel();
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
                // Wait for 10 seconds to be able to afterward check
                // if the test object still managed to execute a feed operation
                spin_sleeper.sleep(sleep_duration_10_secs);

                let actuation_count_1 =
                    *mutex_device_scheduler_feed_test_environment.lock().unwrap();

                // check the amount of actuation (after stop request)
                assert_eq!(actuation_count_1, 0);

                // requesting feed control to quit
                let _ = tx_signal_handler_to_feed.send(InternalCommand::Quit);
                rx_signal_handler_from_feed.recv().unwrap();
                let _ = tx_signal_handler_to_feed.send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let mut feed_channels = FeedChannels {
                    tx_feed_to_relay_manager,
                    #[cfg(feature = "debug_channels")]
                    cnt_tx_feed_to_relay_manager: 0,

                    rx_feed_from_relay_manager,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_feed_from_relay_manager: 0,

                    tx_feed_to_signal_handler,
                    #[cfg(feature = "debug_channels")]
                    cnt_tx_feed_to_signal_handler: 0,

                    rx_feed_from_signal_handler,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_feed_from_signal_handler: 0,

                    rx_feed_from_messaging_opt: None,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_feed_from_messaging_opt: 0,
                };

                feed.execute(
                    mutex_device_scheduler_feed_test_object.clone(),
                    &mut feed_channels,
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
    // This test case does not require any communication with the database.
    pub fn test_select_feed_schedule_entry_for_execution_none() {
        setup_logger(LoggerConfig::default()).unwrap();
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.feed.strategy_multiple_schedule_entries_in_past = -1;

        // mock interface will be used in operation of the test object
        let mock_sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(MockSqlInterfaceFeed::new());

        // create the test object
        let feed = Feed::new(
            config.feed,
            mock_sql_interface_feed,
            Duration::from_millis(1000),
        );

        // input for the function to be tested
        let feedschedule_entries = create_mock_feedschedule_entries();

        assert_eq!(
            feed.select_feed_schedule_entry_for_execution(&feedschedule_entries),
            None
        );
    }

    #[test]
    // Test case test selection of feed schedule entry in case there are multiple entries in the past
    // This test case tests the strategy to select the first one.
    // This test case does not require any communication with the database.
    pub fn test_select_feed_schedule_entry_for_execution_first() {
        setup_logger(LoggerConfig::default()).unwrap();
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.feed.strategy_multiple_schedule_entries_in_past = 0;

        // mock interface will be used in operation of the test object
        let mock_sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(MockSqlInterfaceFeed::new());

        // create the test object
        let feed = Feed::new(
            config.feed,
            mock_sql_interface_feed,
            Duration::from_millis(1000),
        );

        // input for the function to be tested
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
    // This test case does not require any communication with the database.
    pub fn test_select_feed_schedule_entry_for_execution_middle() {
        setup_logger(LoggerConfig::default()).unwrap();
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.feed.strategy_multiple_schedule_entries_in_past = 1;

        // mock interface will be used in operation of the test object
        let mock_sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(MockSqlInterfaceFeed::new());

        // create the test object
        let feed = Feed::new(
            config.feed,
            mock_sql_interface_feed,
            Duration::from_millis(1000),
        );

        // input for the function to be tested
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
    // This test case does not require any communication with the database.
    pub fn test_select_feed_schedule_entry_for_execution_last() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.feed.strategy_multiple_schedule_entries_in_past = 99;

        // mock interface will be used in operation of the test object
        let mock_sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(MockSqlInterfaceFeed::new());

        // create the test object
        let feed = Feed::new(
            config.feed,
            mock_sql_interface_feed,
            Duration::from_millis(1000),
        );

        // input for the function to be tested
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
        let sleep_duration_100_millis = Duration::from_millis(100);
        let sleep_duration_8_seconds = Duration::from_millis(10000);
        let spin_sleeper = SpinSleeper::default();

        // channels for the test object
        let (_tx_relay_manager_to_feed, rx_feed_from_relay_manager) = mpsc::channel();
        let (tx_feed_to_relay_manager, _rx_relay_manager_from_feed) = mpsc::channel();
        let (tx_signal_handler_to_feed, rx_feed_from_signal_handler) = mpsc::channel();
        let (tx_feed_to_signal_handler, rx_signal_handler_from_feed) = mpsc::channel();

        setup_logger(LoggerConfig::default()).unwrap();

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

        let mut sql_interface_feed_unboxed = SqlInterfaceFeed::new(
            sql_interface.get_connection().unwrap(),
            config.sql_interface.max_rows_feed_pattern,
            config.sql_interface.max_rows_feed_schedule,
            config.sql_interface.max_rows_feed_log,
        )
        .unwrap();

        insert_feed_patterns(&mut sql_interface, &mut sql_interface_feed_unboxed);

        let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(sql_interface_feed_unboxed);

        // empty the feed schedule table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEEDSCHEDULE.to_string()) {
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

        // manipulation of the database: inserting feed schedule entries with future timestamp
        sql_interface_feed
            .insert_feed_schedule_entry(
                time_stamp_feed_schedule_entry_one_time,
                1,
                "testOneTimePast".to_string(),
                false,
                false,
            )
            .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

        sql_interface_feed
            .insert_feed_schedule_entry(
                time_stamp_feed_schedule_entry_repeat,
                2,
                "testRepeatPast".to_string(),
                false,
                true,
            )
            .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

        let mut feed = Feed::new(config.feed, sql_interface_feed, Duration::from_millis(1000));

        scope(|scope| {
            let mutex_device_scheduler_feed = Arc::new(Mutex::new(0));

            // thread for mock signal handler
            scope.spawn(move || {
                create_mock_signal_handler(
                    &tx_signal_handler_to_feed,
                    &rx_signal_handler_from_feed,
                    spin_sleeper,
                    sleep_duration_8_seconds,
                );
            });

            spin_sleeper.sleep(sleep_duration_100_millis);

            // thread for the test object
            scope.spawn(move || {
                let mut feed_channels = FeedChannels {
                    tx_feed_to_relay_manager,
                    #[cfg(feature = "debug_channels")]
                    cnt_tx_feed_to_relay_manager: 0,

                    rx_feed_from_relay_manager,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_feed_from_relay_manager: 0,

                    tx_feed_to_signal_handler,
                    #[cfg(feature = "debug_channels")]
                    cnt_tx_feed_to_signal_handler: 0,

                    rx_feed_from_signal_handler,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_feed_from_signal_handler: 0,

                    rx_feed_from_messaging_opt: None,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_feed_from_messaging_opt: 0,
                };

                feed.execute(
                    mutex_device_scheduler_feed.clone(),
                    &mut feed_channels,
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
        let sleep_duration_100_millis = Duration::from_millis(100);
        let sleep_duration_8_seconds = Duration::from_millis(10000);
        let spin_sleeper = SpinSleeper::default();

        // channels for the test object
        let (_tx_relay_manager_to_feed, rx_feed_from_relay_manager) = mpsc::channel();
        let (tx_feed_to_relay_manager, _rx_relay_manager_from_feed) = mpsc::channel();
        let (tx_signal_handler_to_feed, rx_feed_from_signal_handler) = mpsc::channel();
        let (tx_feed_to_signal_handler, rx_signal_handler_from_feed) = mpsc::channel();

        setup_logger(LoggerConfig::default()).unwrap();

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

        let mut sql_interface_feed_unboxed = SqlInterfaceFeed::new(
            sql_interface.get_connection().unwrap(),
            config.sql_interface.max_rows_feed_pattern,
            config.sql_interface.max_rows_feed_schedule,
            config.sql_interface.max_rows_feed_log,
        )
        .unwrap();

        insert_feed_patterns(&mut sql_interface, &mut sql_interface_feed_unboxed);

        let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(sql_interface_feed_unboxed);

        // empty the feed schedule table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEEDSCHEDULE.to_string()) {
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

        // manipulation of the database: inserting feed schedule entries with future timestamp
        sql_interface_feed
            .insert_feed_schedule_entry(
                time_stamp_feed_schedule_entry_one_time,
                1,
                "testOneTimePast".to_string(),
                false,
                false,
            )
            .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

        sql_interface_feed
            .insert_feed_schedule_entry(
                time_stamp_feed_schedule_entry_repeat,
                2,
                "testRepeatPast".to_string(),
                false,
                true,
            )
            .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

        let mut feed = Feed::new(config.feed, sql_interface_feed, Duration::from_millis(1000));

        scope(|scope| {
            let mutex_device_scheduler_feed = Arc::new(Mutex::new(0));

            // thread for mock signal handler
            scope.spawn(move || {
                create_mock_signal_handler(
                    &tx_signal_handler_to_feed,
                    &rx_signal_handler_from_feed,
                    spin_sleeper,
                    sleep_duration_8_seconds,
                );
            });

            spin_sleeper.sleep(sleep_duration_100_millis);

            // thread for the test object
            scope.spawn(move || {
                let mut feed_channels = FeedChannels {
                    tx_feed_to_relay_manager,
                    #[cfg(feature = "debug_channels")]
                    cnt_tx_feed_to_relay_manager: 0,

                    rx_feed_from_relay_manager,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_feed_from_relay_manager: 0,

                    tx_feed_to_signal_handler,
                    #[cfg(feature = "debug_channels")]
                    cnt_tx_feed_to_signal_handler: 0,

                    rx_feed_from_signal_handler,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_feed_from_signal_handler: 0,

                    rx_feed_from_messaging_opt: None,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_feed_from_messaging_opt: 0,
                };

                feed.execute(
                    mutex_device_scheduler_feed.clone(),
                    &mut feed_channels,
                    &mut mock_food_injection,
                );

                // check if the right profiles have been executed in the right order
                assert_eq!(mock_food_injection.pop_profile_id_executed(), Some(2));
                assert_eq!(mock_food_injection.pop_profile_id_executed(), None);
            });
        });
    }
}
