/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the data structures that represent a complete, multistep feeding process.
//!
//! This module provides the `FeedProfile` and `FeedPhase` structs, which together describe
//! a single, executable feeding profile. These structures are typically populated with data
//! retrieved from the database and are used by the `Feed` controller to orchestrate the
//! feeding sequence.
//!
//! ## Key Components
//!
//! - **`FeedProfile`**: Represents a complete feeding profile. It consists of a profile name
//!   and a vector of one or more `FeedPhase`s. It acts as a container for the entire
//!   sequence of actions.
//!
//! - **`FeedPhase`**: Represents a single step within a `FeedProfile`. Each phase is composed
//!   of two parts:
//!   1.  A **pause** period, during which certain pumps can be temporarily disabled.
//!   2.  A **feed** period, where the feeder motor is activated for a specific duration.
//!
//!   The state (on or off) of the skimmer and various pumps can be configured independently
//!   for both the pause and feed parts of a phase.
//!
//! ## Design and Architecture
//!
//! The design allows for complex and flexible feeding schedules. A simple feeding might
//! consist of a single `FeedPhase` (e.g., "pause pumps, run feeder, resume pumps"). A more
//! complex profile could involve multiple phases to dispense multiple quantities of food or
//! to allow food to circulate before resuming full filtration.
//!
//! ### Example of a Multi-Phase Profile
//!
//! A `FeedProfile` could be configured to:
//! 1.  **Phase 1**: Pause the main pumps and skimmer, then run the feeder for 2 seconds to dispense food.
//! 2.  **Phase 2**: Keep pumps off for a 10-second pause to let fish eat, with no feeder activity.
//! 3.  **Phase 3**: Run the feeder for 5 seconds to dispense more food.
//! 4.  **Phase 4**: Wait another 15 seconds before all devices resume normal operation.
//!
//! This entire sequence would be contained within a single `FeedProfile` struct as a `Vec<FeedPhase>`.

/// Holds the configuration data of one feed profile.
#[derive(Clone)]
pub struct FeedProfile {
    /// Numeric ID of the feed profile
    /// Data is not used in the implementation. Therefore, it is tagged as unused.
    #[allow(unused)]
    pub profile_id: i32,

    /// name of the feed profile
    #[allow(unused)]
    pub profile_name: String,

    /// vector of type FeedPhase
    pub feedphases: Vec<FeedPhase>,
}

impl FeedProfile {
    /// Calculates the total duration (in seconds) that the feeder motor will be active for this feed profile.
    ///
    /// This function iterates through all defined `FeedPhase`-items within the profile and sums
    /// their individual `feed_duration` values. This total runtime is typically logged
    /// as part of a completed feed event.
    ///
    /// # Returns
    /// The total feeder run time for this profile, expressed as an `f64` in seconds.
    ///
    /// [impl->swreq~food_log_database~1]
    pub fn calc_feeder_runtime(&self) -> f64 {
        self.feedphases
            .iter()
            .map(|phase| phase.feed_duration as f64)
            .sum()
    }
}

/// Holds the configuration data of one feed phase.
#[derive(Clone, Debug, PartialEq)]
pub struct FeedPhase {
    /// duration of the pause before switching on the feeder motor in milliseconds
    pub pause_duration: i32,

    /// flag indicating if the protein skimmer shall remain switched on during the pause
    pub pause_skimmer: bool,

    /// flag indicating if the main pump #1 shall remain switched on during the pause
    pub pause_main_pump_1: bool,

    /// flag indicating if the main pump #2 shall remain switched on during the pause
    pub pause_main_pump_2: bool,

    /// flag indicating if the auxiliary pump #1 shall remain switched on during the pause
    pub pause_aux_pump_1: bool,

    /// flag indicating if the auxiliary pump #2 shall remain switched on during the pause
    pub pause_aux_pump_2: bool,

    /// duration of the feed after switching on the feeder motor in milliseconds
    pub feed_duration: i32,

    /// flag indicating if the protein skimmer shall remain switched on during the feed phase
    pub feed_skimmer: bool,

    /// flag indicating if the main pump #1 shall remain switched on during the feed phase
    pub feed_main_pump_1: bool,

    /// flag indicating if the main pump #2 shall remain switched on during the feed phase
    pub feed_main_pump_2: bool,

    /// flag indicating if the auxiliary pump #1 shall remain switched on during the feed phase
    pub feed_aux_pump_1: bool,

    /// flag indicating if the auxiliary pump #2 shall remain switched on during the feed phase
    pub feed_aux_pump_2: bool,
}

impl FeedPhase {
    #[allow(clippy::too_many_arguments)]
    /// Creates a new `FeedPhase` struct with explicitly provided values.
    ///
    /// This constructor is intended exclusively for use in test environments.
    /// It allows for the direct creation of a `FeedPhase` instance by setting
    /// all its properties, bypassing typical deserialization or logic.
    ///
    /// # Arguments
    /// * `pause_duration` - The duration of the pause phase in milliseconds.
    /// * `pause_skimmer` - Flag indicating if the protein skimmer is active during the pause phase.
    /// * `pause_main_pump_1` - Flag indicating if main pump #1 is active during the pause phase.
    /// * `pause_main_pump_2` - Flag indicating if main pump #2 is active during the pause phase.
    /// * `pause_aux_pump_1` - Flag indicating if auxiliary pump #1 is active during the pause phase.
    /// * `pause_aux_pump_2` - Flag indicating if auxiliary pump #2 is active during the pause phase.
    /// * `feed_duration` - The duration of the feed phase in milliseconds.
    /// * `feed_skimmer` - Flag indicating if the protein skimmer is active during the feed phase.
    /// * `feed_main_pump_1` - Flag indicating if main pump #1 is active during the feed phase.
    /// * `feed_main_pump_2` - Flag indicating if main pump #2 is active during the feed phase.
    /// * `feed_aux_pump_1` - Flag indicating if auxiliary pump #1 is active during the feed phase.
    /// * `feed_aux_pump_2` - Flag indicating if auxiliary pump #2 is active during the feed phase.
    ///
    /// # Returns
    /// A new `FeedPhase` struct initialized with the provided values.
    ///
    /// [impl->swreq~food_read_profiles~1]
    pub fn new(
        pause_duration: i32,
        pause_skimmer: bool,
        pause_main_pump_1: bool,
        pause_main_pump_2: bool,
        pause_aux_pump_1: bool,
        pause_aux_pump_2: bool,
        feed_duration: i32,
        feed_skimmer: bool,
        feed_main_pump_1: bool,
        feed_main_pump_2: bool,
        feed_aux_pump_1: bool,
        feed_aux_pump_2: bool,
    ) -> FeedPhase {
        FeedPhase {
            pause_duration,
            pause_skimmer,
            pause_main_pump_1,
            pause_main_pump_2,
            pause_aux_pump_1,
            pause_aux_pump_2,
            feed_duration,
            feed_skimmer,
            feed_main_pump_1,
            feed_main_pump_2,
            feed_aux_pump_1,
            feed_aux_pump_2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that `calc_feeder_runtime` returns 0.0 for a profile with no feed phases.
    #[test]
    // Verifies that calc feeder runtime empty.
    /// [utest->swreq~food_log_database~1]
    fn test_calc_feeder_runtime_empty() {
        println!("* Testing calc_feeder_runtime with an empty phase list...");
        let profile = FeedProfile {
            profile_id: 1,
            profile_name: "Empty Profile".to_string(),
            feedphases: vec![],
        };

        assert_eq!(
            profile.calc_feeder_runtime(),
            0.0,
            "Runtime for an empty profile should be 0.0"
        );
        println!("* Succeeded: Empty profile runtime is correct.");
    }

    /// Tests that `calc_feeder_runtime` correctly calculates the runtime for a single feed phase.
    #[test]
    // Verifies that calc feeder runtime single phase.
    /// [utest->swreq~food_log_database~1]
    fn test_calc_feeder_runtime_single_phase() {
        println!("* Testing calc_feeder_runtime with a single phase...");
        let phase = FeedPhase::new(
            1000, false, false, false, false, false, 2500, false, false, false, false, false,
        );
        let profile = FeedProfile {
            profile_id: 2,
            profile_name: "Single Phase Profile".to_string(),
            feedphases: vec![phase],
        };

        assert_eq!(
            profile.calc_feeder_runtime(),
            2500.0,
            "Runtime for a single phase should match its feed_duration"
        );
        println!("* Succeeded: Single phase runtime is correct.");
    }

    /// Tests that `calc_feeder_runtime` correctly sums the durations from multiple feed phases.
    #[test]
    // Verifies that calc feeder runtime multiple phases.
    /// [utest->swreq~food_log_database~1]
    fn test_calc_feeder_runtime_multiple_phases() {
        println!("* Testing calc_feeder_runtime with multiple phases...");
        let phase1 = FeedPhase::new(
            1000, false, false, false, false, false, 2000, false, false, false, false, false,
        );
        let phase2 = FeedPhase::new(
            500, false, false, false, false, false, 3000, false, false, false, false, false,
        );
        let phase3 = FeedPhase::new(
            0, false, false, false, false, false, 1500, false, false, false, false, false,
        );

        let profile = FeedProfile {
            profile_id: 3,
            profile_name: "Multi-Phase Profile".to_string(),
            feedphases: vec![phase1, phase2, phase3],
        };

        // Expected: 2000 + 3000 + 1500 = 6500
        assert_eq!(
            profile.calc_feeder_runtime(),
            6500.0,
            "Runtime for multiple phases should be the sum of their feed_durations"
        );
        println!("* Succeeded: Multi-phase runtime is correct.");
    }

    /// Tests that phases with a `feed_duration` of zero do not contribute to the total runtime.
    #[test]
    // Verifies that calc feeder runtime with zero duration.
    /// [utest->swreq~food_log_database~1]
    fn test_calc_feeder_runtime_with_zero_duration() {
        println!("* Testing calc_feeder_runtime with zero-duration phases...");
        let phase1 = FeedPhase::new(
            1000, false, false, false, false, false, 5000, false, false, false, false, false,
        );
        // A phase that is only for pausing, with no feeding action.
        let phase2 = FeedPhase::new(
            10000, false, false, false, false, false, 0, false, false, false, false, false,
        );
        let phase3 = FeedPhase::new(
            1000, false, false, false, false, false, 2500, false, false, false, false, false,
        );

        let profile = FeedProfile {
            profile_id: 4,
            profile_name: "Zero Duration Profile".to_string(),
            feedphases: vec![phase1, phase2, phase3],
        };

        // Expected: 5000 + 0 + 2500 = 7500
        assert_eq!(
            profile.calc_feeder_runtime(),
            7500.0,
            "Phases with zero feed_duration should not contribute to the total runtime"
        );
        println!("* Succeeded: Zero-duration phase is handled correctly.");
    }

    /// Tests the edge case where a `feed_duration` is negative, which is possible with the `i16` type.
    #[test]
    // Verifies that calc feeder runtime with negative duration.
    /// [utest->swreq~food_log_database~1]
    fn test_calc_feeder_runtime_with_negative_duration() {
        println!("* Testing calc_feeder_runtime with a negative duration (edge case)...");
        let phase1 = FeedPhase::new(
            1000, false, false, false, false, false, 5000, false, false, false, false, false,
        );
        // A phase with a negative duration. This is invalid logically but possible with the i16 type.
        let phase2 = FeedPhase::new(
            1000, false, false, false, false, false, -1000, false, false, false, false, false,
        );

        let profile = FeedProfile {
            profile_id: 5,
            profile_name: "Negative Duration Profile".to_string(),
            feedphases: vec![phase1, phase2],
        };

        // Expected: 5000 + (-1000) = 4000
        assert_eq!(
            profile.calc_feeder_runtime(),
            4000.0,
            "Negative feed_duration should be summed correctly"
        );
        println!("* Succeeded: Negative duration edge case is handled correctly.");
    }
}
