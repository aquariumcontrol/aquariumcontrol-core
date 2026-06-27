/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Implements the main control logic for automated and manual fish feeding.
//!
//! This module contains the `Feed` struct, which runs as a dedicated thread to manage all
//! aspects of the feeding process. It is responsible for executing scheduled feed profiles,
//! responding to external commands (e.g., from aquarium_client or the web server), and ensuring
//! that feeding operations are performed safely and reliably.
//!
//! ## Key Components
//!
//! - **`Feed` Struct**: The central state machine for the feeding subsystem. It holds the
//!   configuration, communication channels, and internal state flags like `execution_blocked`.
//!
//! - **`execute()` Method**: The main thread loop. It continuously performs the following actions:
//!   1.  Checks for external commands (`Start`, `Stop`, `Execute`, `Quit`).
//!   2.  If active, periodically queries the database for overdue feed schedules.
//!   3.  Uses a configurable strategy to select a single profile if multiple are overdue.
//!   4.  Executes the selected profile by calling the `FoodInjectionTrait`.
//!   5.  Logs the feed event to the database.
//!   6.  Pings the database to keep the connection alive.
//!
//! - **`execute_feed()` Method**: A private helper that orchestrates a single feeding event. It
//!   retrieves the feed profile from the database, actuates the feeder via the
//!   `FoodInjectionTrait`, and logs the event.
//!
//! ## Design and Architecture
//!
//! The `Feed` module is designed to be a robust, testable, and decoupled component.
//!
//! - **Dependency Injection**: The module relies on traits for its core dependencies:
//!   - `FoodInjectionTrait`: An abstraction for the physical act of dispensing food. This
//!     allows for mock implementations in tests without needing real hardware.
//!   - `DatabaseInterfaceFeedTrait`: An abstraction for all database operations, enabling
//!     the use of a mock database during testing.
//!
//! - **State Management**: The module maintains an internal state to handle various conditions:
//!   - `feed_inhibited`: A flag to temporarily pause scheduled feeding based on `Start`/`Stop` commands.
//!   - `execution_blocked`: A critical safety flag. If any part of a feeding process fails
//!     (e.g., cannot write to the database), this flag is set to `true`, preventing all
//!     further feeding operations until it is reset by an external command. This
//!     prevents unintended behavior due to a faulty system state.
//!
//! - **Concurrency Control**: It uses an `Arc<Mutex<i32>>` to coordinate with other device-actuating
//!   modules, ensuring that only one major physical action (like feeding or dosing) occurs at a time.
//!
//! - **Scheduling Strategy**: When multiple scheduled feeds are found in the past, a
//!   configurable strategy (`strategy_multiple_schedule_entries_in_past`) is used to
//!   decide which one to execute (e.g., the oldest, the newest, or none).
//!
//! ### Example Flow
//!
//! 1.  The `execute()` loop starts.
//! 2.  It checks the database and finds a feed profile was scheduled for 10 minutes ago.
//! 3.  It calls `execute_feed()` with the profile ID.
//! 4.  `execute_feed()` retrieves the profile.
//! 5.  It calls `food_injection.inject_food()`, which communicates with the `RelayManager`
//!     to turn the feeder relay on and then off.
//! 6.  After injection, it writes a record of the event to the `feedlog` table.
//! 7.  Finally, it updates the `feed_schedule` table to mark the entry as completed or to
//!     reschedule it if it's a repeating event.

#[cfg(not(test))] // only required in real-life execution
use log::info;

#[cfg(all(feature = "debug_feed", not(test)))] // only required in real-life execution
use log::debug;

#[cfg(all(feature = "debug_signal_handler", not(test)))] // only required in real-life execution
use log::debug;

#[cfg(all(target_os = "linux", not(test)))] // only required in real-life execution
use nix::unistd::gettid;

use crate::check_quit_increment_counter_ping_database;
use crate::database::database_interface_feed_trait::DatabaseInterfaceFeedTrait;
use crate::food::feed_channels::FeedChannels;
use crate::food::feed_config::FeedConfig;
use crate::food::feed_profile::FeedProfile;
use crate::food::feed_schedule_entry::FeedScheduleEntry;
use crate::food::food_injection::FoodInjectionTrait;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::database_ping_trait::DatabasePingTrait;
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the feed control.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     feed[Feed Control] --> food_injection[Food Injection]
///     food_injection --> relay_manager[Relay Manager]
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

    /// Communication from trait implementation: request to execute a certain feed profile has been received
    pub execute_command_received: bool,

    /// Communication from trait implementation: id of feed profile to be executed requested externally
    pub profile_id_requested: i32,

    /// Inhibition flag for further execution of feed profiles. It is set whenever errors occur during feeding and can be reset by external command.
    pub execution_blocked: bool,

    /// Inhibition flag to avoid flooding the log file with repeated messages about failure to read feed schedule entry from the database
    lock_error_get_feed_schedule_entry: bool,

    /// recording when the last database ping happened
    pub last_ping_instant: Instant,

    /// database ping interval
    pub database_ping_interval: Duration,

    /// Inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command
    pub lock_warn_inapplicable_command_signal_handler: bool,

    /// Inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    pub lock_error_channel_receive_termination: bool,
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
    /// * `database_ping_interval` - A `Duration` instance, providing the interval to ping the database.
    ///
    /// # Returns
    /// A new `Feed` struct, ready for operation within the application loop.
    pub fn new(config: FeedConfig, database_ping_interval: Duration) -> Feed {
        Self {
            config,
            execute_command_received: false,
            profile_id_requested: -1,
            execution_blocked: false,
            lock_error_get_feed_schedule_entry: false,
            last_ping_instant: Instant::now(),
            database_ping_interval,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
        }
    }

    #[cfg(all(feature = "debug_feed", not(test)))] // only required in real-life execution
    #[cfg(not(tarpaulin_include))]
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
    fn feed_schedule_entries_to_string(feedschedule_entries: Vec<FeedScheduleEntry>) -> String {
        let mut output: String = "Vec<FeedscheduleEntry>=".to_string();
        for (i, s) in feedschedule_entries.iter().enumerate() {
            output += &(" #".to_owned() + &format!("{}: ID= {} ", i, s.profile_id));
        }
        output
    }

    /// Logs the recently executed feed profile to the database using feed-specific interface
    ///
    /// # Arguments
    /// * `feed_profile` - The feed profile to executed.
    /// * `sql_interface_feed` - A boxed trait object representing the interface
    ///   to the SQL database for feed-specific operations. This allows for
    ///   dependency injection and mock implementations during testing.
    ///
    /// # Returns
    /// An empty Ok() or Err() depending on result of the logging operation.
    fn log_feed_operation(
        &mut self,
        feed_profile: FeedProfile,
        sql_interface_feed: &mut Box<dyn DatabaseInterfaceFeedTrait + Sync + Send>,
    ) -> Result<(), ()> {
        // Get the current timestamp to log the event
        let current_timestamp = match sql_interface_feed.get_current_timestamp() {
            Ok(c) => c,
            Err(e) => {
                log_error_chain(
                    module_path!(),
                    "Could not get current timestamp from database.",
                    e,
                );
                return Err(());
            }
        };

        //*** Insert the feed event into the log ***
        match sql_interface_feed.insert_feed_event(
            current_timestamp,
            feed_profile.calc_feeder_runtime(),
            feed_profile.profile_id,
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
        }
    }

    /// Executes a previously loaded feed profile via food injection.
    ///
    /// This private helper function orchestrates the process of executing a single feed profile.
    /// It calls the `food_injection` trait method to actuate the feeder according to the
    /// feed profile.
    ///
    /// The function also monitors for a `Quit` command from the signal handler during the
    /// food injection process, indicating an early termination request.
    ///
    /// # Arguments
    /// * `feed_profile` - The feed profile to be executed.
    /// * `food_injection` - A mutable reference to an object implementing the `FoodInjectionTrait`,
    ///   responsible for the physical actuation of the feeder.
    /// * `feed_channels` - A mutable reference to the struct containing the channels.
    ///
    /// # Returns
    /// A tuple `(Result<(), ()>, bool)` where:
    /// - The first element is the overall result of the operation. It is `Ok(())` only if
    ///   the food injection completed without error.
    /// - The second element is a `bool,` which is `true` if a `Quit` command was received
    ///   from the signal handler during the `inject_food` process, `false` otherwise. This
    ///   value is independent of the success or failure of the operation itself.
    ///
    /// # Errors
    /// This function does not return a distinct error type. Instead, it returns `Err(())`
    /// in the result tuple to signify failure. The function can fail if:
    /// the `food_injection.inject_food` method reports an error.
    fn execute_feed(
        &mut self,
        feed_profile: FeedProfile,
        food_injection: &mut impl FoodInjectionTrait,
        feed_channels: &mut FeedChannels,
    ) -> (Result<(), ()>, bool) {
        // *** Execute the feed profile and destructure the returned tuple ***
        // 1. `quit_during_injection`: A bool indicating if a quit command was received.
        // 2. `injection_result`: A Result indicating if the injection itself succeeded.
        let (quit_during_injection, injection_result) =
            food_injection.inject_food(feed_channels, &feed_profile);
        // *** Handle the Result part of the tuple independently ***
        // If errors occurred during injection, log each one.
        if let Err(errors) = &injection_result {
            for error in errors {
                log_error_chain(module_path!(), "Food injection failed.", error);
            }
        }

        let final_result = if injection_result.is_err() {
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
    /// * `sql_interface_feed` - A boxed trait object representing the interface
    ///   to the SQL database for feed-specific operations. This allows for
    ///   dependency injection and mock implementations during testing.
    pub async fn execute(
        &mut self,
        mutex_device_scheduler_feed: Arc<Mutex<i32>>,
        feed_channels: &mut FeedChannels,
        food_injection: &mut (impl FoodInjectionTrait + Send),
        mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send>,
    ) {
        #[cfg(all(target_os = "linux", not(test)))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let sleep_duration_hundred_millis = Duration::from_millis(100);

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
                #[cfg(all(feature = "debug_feed", not(test)))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                debug!(
                    target: module_path!(),
                    "Starting feed control cycle"
                );

                match sql_interface_feed.get_past_feed_schedule_entries_from_database() {
                    Ok(feed_schedule_entries_opt) => {
                        if let Some(mut feed_schedule_entries) = feed_schedule_entries_opt {
                            #[cfg(all(feature = "debug_feed", not(test)))] // only required in real-life execution
                            #[cfg(not(tarpaulin_include))]
                            debug!(
                                target: module_path!(),
                                "Discovered following feed schedule entries in the past: {}",
                                Self::feedschedule_entries_to_string(feed_schedule_entries.clone())
                            );

                            let feed_schedule_entry_opt = self
                                .select_feed_schedule_entry_for_execution(&feed_schedule_entries);

                            if let Some(feed_schedule_entry) = feed_schedule_entry_opt {
                                #[cfg(all(feature = "debug_feed", not(test)))] // only required in real-life execution
                                #[cfg(not(tarpaulin_include))]
                                debug!(
                                    target: module_path!(),
                                    "Selected following feed schedule entry for execution: {}",
                                    feed_schedule_entry
                                );

                                self.lock_error_get_feed_schedule_entry = false;

                                if !self.execution_blocked {
                                    // execute database query before locking mutex
                                    // First, load the feed profile from the database.
                                    let feed_profile_opt = match sql_interface_feed
                                        .get_single_feed_profile_from_database(
                                            feed_schedule_entry.profile_id,
                                        ) {
                                        Ok(feed_profile_loaded) => Some(feed_profile_loaded),
                                        Err(e) => {
                                            log_error_chain(
                                                    module_path!(),
                                                    "Error occurred when trying to read feed profile {profile_id} from database.",
                                                    e,
                                                );
                                            None
                                        }
                                    };

                                    if let Some(feed_profile) = feed_profile_opt {
                                        {
                                            // this scope limits the lifetime of unlocked mutex
                                            let mut mutex_data =
                                                mutex_device_scheduler_feed.lock().unwrap();

                                            let injection_result: Result<(), ()>;

                                            (injection_result, quit_command_received) = self
                                                .execute_feed(
                                                    feed_profile.clone(),
                                                    food_injection,
                                                    feed_channels,
                                                );
                                            if injection_result.is_err() {
                                                self.execution_blocked = true;
                                            }
                                            *mutex_data = mutex_data.saturating_add(1);
                                        }

                                        let logging_result = self.log_feed_operation(
                                            feed_profile,
                                            &mut sql_interface_feed,
                                        );
                                        if logging_result.is_err() {
                                            // block further execution if logging of feed event does not work
                                            self.execution_blocked = true;
                                        }
                                    }

                                    //*** update schedule ***
                                    match sql_interface_feed
                                        .update_feed_schedule_entries_in_database(
                                            &mut feed_schedule_entries,
                                        ) {
                                        Ok(()) => {
                                            // do nothing
                                        }
                                        #[cfg(not(test))] // only required in real-life execution
                                        #[cfg(not(tarpaulin_include))]
                                        Err(e) => {
                                            log_error_chain(
                                                module_path!(),
                                                "Updating feed schedule entry failed.",
                                                e,
                                            );
                                            self.execution_blocked = true;
                                        }
                                        #[cfg(test)]
                                        Err(_) => {
                                            self.execution_blocked = true;
                                        }
                                    }

                                    // update database after dropping mutex
                                }
                            }
                        }
                    }
                    Err(e) => {
                        #[cfg(not(test))] // only required in real-life execution
                        #[cfg(not(tarpaulin_include))]
                        if !self.lock_error_get_feed_schedule_entry {
                            log_error_chain(
                                module_path!(),
                                "Could not retrieve feed schedule information from database.",
                                e,
                            );
                            self.lock_error_get_feed_schedule_entry = true;
                        }
                        #[cfg(test)]
                        let _e = e; // Explicitly declare and "use" with an underscore
                        self.lock_error_get_feed_schedule_entry = true;
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
                &mut feed_channels.rx_feed_from_signal_handler,
                feed_channels.rx_feed_from_messaging_opt.as_mut(),
            );
            if quit_command_received {
                break;
            }
            if stop_command_received {
                #[cfg(not(test))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                info!(
                    target: module_path!(),
                    "received Stop command. Inhibiting feed."
                );

                #[cfg(all(feature = "debug_feed", not(test)))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                debug!(
                    target: module_path!(),
                    "received Stop command."
                );

                feed_inhibited = true;
            }
            if start_command_received {
                #[cfg(not(test))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                info!(
                    target: module_path!(),
                    "received Start command. Restarting feed."
                );

                #[cfg(all(feature = "debug_feed", not(test)))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
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
                #[cfg(all(feature = "debug_feed", not(test)))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                debug!(
                    target: module_path!(),
                    "received execution command"
                );

                // First, load the feed profile from the database.
                let feed_profile_opt = match sql_interface_feed
                    .get_single_feed_profile_from_database(self.profile_id_requested)
                {
                    Ok(feed_profile_loaded) => Some(feed_profile_loaded),
                    Err(e) => {
                        log_error_chain(
                                module_path!(),
                                "Error occurred when trying to read feed profile {profile_id} from database.",
                                e,
                            );
                        None
                    }
                };

                if let Some(feed_profile) = feed_profile_opt {
                    let feed_profile_clone = feed_profile.clone();
                    let (res_opt, join_err_opt) = tokio::task::block_in_place(|| {
                        std::thread::scope(|scope| {
                            let handle = scope.spawn(|| {
                                let mut mutex_data = mutex_device_scheduler_feed.lock().unwrap();

                                let (injection_result, quit_command_received_during_requested_feed) =
                                    self.execute_feed(feed_profile_clone.clone(), food_injection, feed_channels);

                                let logging_result =
                                    self.log_feed_operation(feed_profile_clone, &mut sql_interface_feed);

                                *mutex_data = mutex_data.saturating_add(1);

                                (injection_result, quit_command_received_during_requested_feed, logging_result)
                            });
                            match handle.join() {
                                Ok(res) => (Some(res), None),
                                Err(e) => {
                                    let panic_msg = if let Some(s) = e.downcast_ref::<&str>() {
                                        s.to_string()
                                    } else if let Some(s) = e.downcast_ref::<String>() {
                                        s.clone()
                                    } else {
                                        "Unknown panic payload".to_string()
                                    };
                                    (None, Some(ThreadJoinError(panic_msg)))
                                }
                            }
                        })
                    });

                    if let Some(err) = join_err_opt {
                        self.execution_blocked = true;
                        log_error_chain(
                            module_path!(),
                            "could not join thread",
                            err,
                        );
                        quit_command_received = false;
                    } else if let Some((injection_result, quit_command_received_during_requested_feed, logging_result)) = res_opt {
                        self.execution_blocked |= injection_result.is_err();
                        self.execution_blocked |= logging_result.is_err();
                        quit_command_received = quit_command_received_during_requested_feed;
                    }
                }
            }

            check_quit_increment_counter_ping_database!(
                quit_command_received,
                sleep_duration_hundred_millis,
                loop_counter,
                self,
                &mut *sql_interface_feed
            );
        }

        // The application received request to terminate. That is why the loop was left.
        // Answer to the thread which sent the request for termination, so that shutdown can proceed further.
        #[cfg(all(feature = "debug_signal_handler", not(test)))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        debug!(
            target: module_path!(),
            "Sending Quit confirmation to signal handler."
        );

        feed_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads.
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination_async(
            &mut feed_channels.rx_feed_from_signal_handler,
            sleep_duration_hundred_millis,
            module_path!(),
        ).await;
    }
}

#[derive(Debug)]
struct ThreadJoinError(String);

impl std::fmt::Display for ThreadJoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ThreadJoinError {}

#[cfg(test)]
pub mod tests {
    use spin_sleep::SpinSleeper;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::thread::scope;
    use std::time::Duration;

    use crate::database::database_interface_feed_trait::DatabaseInterfaceFeedTrait;
    use crate::database::sql_interface::SqlInterface;
    use crate::database::sql_interface_feed::SqlInterfaceFeed;
    use crate::food::feed::Feed;
    use crate::food::feed_schedule_entry::FeedScheduleEntry;
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::config::{
        read_config_file, read_config_file_with_test_database, ConfigData,
    };
    use crate::utilities::logger::setup_logger;
    use crate::utilities::logger_config::LoggerConfig;

    use crate::database::sql_interface_feed::tests::insert_feed_profiles;
    use crate::database::sql_query_strings::{SQL_TABLE_FEED_LOG, SQL_TABLE_FEED_SCHEDULE};
    use crate::launch::channels::{AquaReceiver, AquaSender, Channels};
    use crate::mocks::mock_food_injection::MockFoodInjection;
    use crate::mocks::mock_sql_interface_feed::tests::MockSqlInterfaceFeed;

    use crate::food::feed_channels::FeedChannels;
    use crate::food::food_injection::FoodInjectionTrait;

    fn execute_blocking(
        feed: &mut Feed,
        mutex_device_scheduler_feed: Arc<Mutex<i32>>,
        feed_channels: &mut FeedChannels,
        food_injection: &mut (impl FoodInjectionTrait + Send),
        sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send>,
    ) {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                feed.execute(
                    mutex_device_scheduler_feed,
                    feed_channels,
                    food_injection,
                    sql_interface_feed,
                ).await;
            });
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
        tx_signal_handler_to_feed: &mut AquaSender<InternalCommand>,
        rx_signal_handler_from_feed: &mut AquaReceiver<bool>,
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

        let mut channels = Channels::new_for_test();

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
            config.sql_interface.max_rows_feed_profile,
            config.sql_interface.max_rows_feed_schedule,
            config.sql_interface.max_rows_feed_log,
        )
        .unwrap();

        insert_feed_profiles(&mut sql_interface, &mut sql_interface_feed_unboxed);

        let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(sql_interface_feed_unboxed);

        // empty the feed schedule table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string())
        {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }
        // empty the feed log table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_LOG.to_string()) {
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
            .insert_feed_schedule_entry(time_stamp_feed_schedule_entry_one_time, 1, false, false)
            .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

        sql_interface_feed
            .insert_feed_schedule_entry(time_stamp_feed_schedule_entry_repeat, 2, false, true)
            .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

        let mut feed = Feed::new(config.feed, Duration::from_millis(1000));

        scope(|scope| {
            let mutex_device_scheduler_feed = Arc::new(Mutex::new(0));

            // thread for mock signal handler
            scope.spawn(move || {
                create_mock_signal_handler(
                    &mut channels.signal_handler.tx_signal_handler_to_feed,
                    &mut channels.signal_handler.rx_signal_handler_from_feed,
                    spin_sleeper,
                    sleep_duration_8_seconds,
                );
            });

            spin_sleeper.sleep(sleep_duration_100_millis);

            // thread for the test object
            scope.spawn(move || {
                execute_blocking(
                    &mut feed,
                    mutex_device_scheduler_feed.clone(),
                    &mut channels.feed,
                    &mut mock_food_injection,
                    sql_interface_feed,
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
            config.sql_interface.max_rows_feed_profile,
            config.sql_interface.max_rows_feed_schedule,
            config.sql_interface.max_rows_feed_log,
        )
        .unwrap();

        insert_feed_profiles(&mut sql_interface, &mut sql_interface_feed_unboxed);

        let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(sql_interface_feed_unboxed);

        // empty the feed schedule
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string())
        {
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
            .insert_feed_schedule_entry(time_stamp_feed_schedule_entry, 1, false, false)
            .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

        // create the test object
        let mut feed = Feed::new(config.feed, Duration::from_millis(1000));

        let mut channels = Channels::new_for_test();

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
                match channels
                    .messaging
                    .tx_messaging_to_feed
                    .send(InternalCommand::Stop)
                {
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
                match channels
                    .messaging
                    .tx_messaging_to_feed
                    .send(InternalCommand::Start)
                {
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
                match channels
                    .messaging
                    .tx_messaging_to_feed
                    .send(InternalCommand::Execute(1))
                {
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
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_feed
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_feed
                    .recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_feed
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                execute_blocking(
                    &mut feed,
                    mutex_device_scheduler_feed_test_object.clone(),
                    &mut channels.feed,
                    &mut mock_food_injection,
                    sql_interface_feed,
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
        let mut feed = Feed::new(config.feed, Duration::from_millis(1000));

        let mut channels = Channels::new_for_test();

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
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_feed
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_feed
                    .recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_feed
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                execute_blocking(
                    &mut feed,
                    mutex_device_scheduler_feed_test_object.clone(),
                    &mut channels.feed,
                    &mut mock_food_injection,
                    mock_sql_interface_feed,
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
    fn create_mock_feed_schedule_entries() -> Vec<FeedScheduleEntry> {
        let mut feed_schedule_entries: Vec<FeedScheduleEntry> = Vec::new();

        feed_schedule_entries.push(FeedScheduleEntry::new_for_test(
            "2024-10-24 10:00:00",
            77,
            true,
        ));
        feed_schedule_entries.push(FeedScheduleEntry::new_for_test(
            "2024-10-24 11:00:00",
            78,
            true,
        ));
        feed_schedule_entries.push(FeedScheduleEntry::new_for_test(
            "2024-10-24 12:00:00",
            79,
            true,
        ));

        feed_schedule_entries
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

        // create the test object
        let feed = Feed::new(config.feed, Duration::from_millis(1000));

        // input for the function to be tested
        let feed_schedule_entries = create_mock_feed_schedule_entries();

        assert_eq!(
            feed.select_feed_schedule_entry_for_execution(&feed_schedule_entries),
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

        // create the test object
        let feed = Feed::new(config.feed, Duration::from_millis(1000));

        // input for the function to be tested
        let feed_schedule_entries = create_mock_feed_schedule_entries();

        assert_eq!(
            feed.select_feed_schedule_entry_for_execution(&feed_schedule_entries)
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

        // create the test object
        let feed = Feed::new(config.feed, Duration::from_millis(1000));

        // input for the function to be tested
        let feed_schedule_entries = create_mock_feed_schedule_entries();

        assert_eq!(
            feed.select_feed_schedule_entry_for_execution(&feed_schedule_entries)
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

        // create the test object
        let feed = Feed::new(config.feed, Duration::from_millis(1000));

        // input for the function to be tested
        let feed_schedule_entries = create_mock_feed_schedule_entries();

        assert_eq!(
            feed.select_feed_schedule_entry_for_execution(&feed_schedule_entries)
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

        let mut channels = Channels::new_for_test();

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
            config.sql_interface.max_rows_feed_profile,
            config.sql_interface.max_rows_feed_schedule,
            config.sql_interface.max_rows_feed_log,
        )
        .unwrap();

        insert_feed_profiles(&mut sql_interface, &mut sql_interface_feed_unboxed);

        let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(sql_interface_feed_unboxed);

        // empty the feed schedule table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string())
        {
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
            .insert_feed_schedule_entry(time_stamp_feed_schedule_entry_one_time, 1, false, false)
            .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

        sql_interface_feed
            .insert_feed_schedule_entry(time_stamp_feed_schedule_entry_repeat, 2, false, true)
            .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

        let mut feed = Feed::new(config.feed, Duration::from_millis(1000));

        scope(|scope| {
            let mutex_device_scheduler_feed = Arc::new(Mutex::new(0));

            // thread for mock signal handler
            scope.spawn(move || {
                create_mock_signal_handler(
                    &mut channels.signal_handler.tx_signal_handler_to_feed,
                    &mut channels.signal_handler.rx_signal_handler_from_feed,
                    spin_sleeper,
                    sleep_duration_8_seconds,
                );
            });

            spin_sleeper.sleep(sleep_duration_100_millis);

            // thread for the test object
            scope.spawn(move || {
                execute_blocking(
                    &mut feed,
                    mutex_device_scheduler_feed.clone(),
                    &mut channels.feed,
                    &mut mock_food_injection,
                    sql_interface_feed,
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

        let mut channels = Channels::new_for_test();

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
            config.sql_interface.max_rows_feed_profile,
            config.sql_interface.max_rows_feed_schedule,
            config.sql_interface.max_rows_feed_log,
        )
        .unwrap();

        insert_feed_profiles(&mut sql_interface, &mut sql_interface_feed_unboxed);

        let mut sql_interface_feed: Box<dyn DatabaseInterfaceFeedTrait + Sync + Send> =
            Box::new(sql_interface_feed_unboxed);

        // empty the feed schedule table
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string())
        {
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
            .insert_feed_schedule_entry(time_stamp_feed_schedule_entry_one_time, 1, false, false)
            .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

        sql_interface_feed
            .insert_feed_schedule_entry(time_stamp_feed_schedule_entry_repeat, 2, false, true)
            .expect("test_feed_schedule_execution: Could not insert stimuli into database: {e:?}");

        let mut feed = Feed::new(config.feed, Duration::from_millis(1000));

        scope(|scope| {
            let mutex_device_scheduler_feed = Arc::new(Mutex::new(0));

            // thread for mock signal handler
            scope.spawn(move || {
                create_mock_signal_handler(
                    &mut channels.signal_handler.tx_signal_handler_to_feed,
                    &mut channels.signal_handler.rx_signal_handler_from_feed,
                    spin_sleeper,
                    sleep_duration_8_seconds,
                );
            });

            spin_sleeper.sleep(sleep_duration_100_millis);

            // thread for the test object
            scope.spawn(move || {
                execute_blocking(
                    &mut feed,
                    mutex_device_scheduler_feed.clone(),
                    &mut channels.feed,
                    &mut mock_food_injection,
                    sql_interface_feed,
                );

                // check if the right profiles have been executed in the right order
                assert_eq!(mock_food_injection.pop_profile_id_executed(), Some(2));
                assert_eq!(mock_food_injection.pop_profile_id_executed(), None);
            });
        });
    }

    #[test]
    // Verifies that the `Feed::new` constructor correctly initializes the struct.
    //
    // This test checks that all fields are set to their expected default values upon creation,
    // ensuring a predictable starting state for the Feed controller.
    //
    // Test case uses test database #65 for configuration loading, as requested.
    fn test_feed_new() {
        println!("* Testing Feed::new constructor...");

        // --- Setup ---
        // Load a test configuration. The database number is part of the config loading
        // but is not directly used by the `Feed::new` constructor itself.
        let config = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            65,
        );
        let ping_interval = Duration::from_secs(30);

        // --- Execution ---
        let feed = Feed::new(config.feed.clone(), ping_interval);

        // --- Assertions ---

        // 1. Check that the configuration and interval were stored correctly.
        assert_eq!(
            &feed.config, &config.feed,
            "The provided FeedConfig should be stored in the struct."
        );
        assert_eq!(
            feed.database_ping_interval, ping_interval,
            "The provided database_ping_interval should be stored in the struct."
        );
        println!("* Succeeded: Config and ping interval are stored correctly.");

        // 2. Check the initial state of command/request flags.
        assert_eq!(
            feed.execute_command_received, false,
            "execute_command_received should be initialized to false."
        );
        assert_eq!(
            feed.profile_id_requested, -1,
            "profile_id_requested should be initialized to -1."
        );
        println!("* Succeeded: Command flags are initialized correctly.");

        // 3. Check the initial state of all blocking and locking flags.
        assert_eq!(
            feed.execution_blocked, false,
            "execution_blocked should be initialized to false."
        );
        assert_eq!(
            feed.lock_error_get_feed_schedule_entry, false,
            "lock_error_get_feedschedule_entry should be initialized to false."
        );
        assert_eq!(
            feed.lock_warn_inapplicable_command_signal_handler, false,
            "lock_warn_inapplicable_command_signal_handler should be initialized to false."
        );
        assert_eq!(
            feed.lock_error_channel_receive_termination, false,
            "lock_error_channel_receive_termination should be initialized to false."
        );
        println!("* Succeeded: State and lock flags are initialized to false.");

        // 4. Check the `last_ping_instant`.
        // We can't check for an exact time, but we can assert that it was created very recently.
        assert!(
            feed.last_ping_instant.elapsed() < Duration::from_millis(100),
            "last_ping_instant should be set to a very recent time."
        );
        println!("* Succeeded: last_ping_instant is initialized to the current time.");

        println!("* Test for Feed::new completed successfully.");
    }
}
