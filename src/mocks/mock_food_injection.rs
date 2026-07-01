/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::food::feed_channels::FeedChannels;
use crate::food::feed_profile::FeedProfile;
use crate::food::food_injection::FoodInjectionTrait;
use crate::food::food_injection_error::FoodInjectionError;
use std::fmt;

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
    /// Creates a new instance.
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
    /// Performs the `pop_profile_id_executed` operation.
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
    /// Performs the `inject_food` operation.
    ///
    /// # Arguments
    /// * `feed_channels` - A mutable reference to the `FeedChannels` communication interface.
    /// * `feed_pattern` - A parameter representing `feed_pattern`.
    fn inject_food(
        &mut self,
        _feed_channels: &mut FeedChannels,
        feed_pattern: &FeedProfile,
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
