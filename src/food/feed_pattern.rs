/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// Holds the configuration data of one feed pattern.
pub struct Feedpattern {
    /// Numeric ID of the feed profile
    /// Data is not used in the implementation. Therefore, it is tagged as unused.
    #[allow(unused)]
    pub profile_id: i32,

    /// name of the feed profile
    pub profile_name: String,

    /// vector of type FeedPhase
    pub feedphases: Vec<FeedPhase>,
}

impl Feedpattern {
    /// Calculates the total duration (in seconds) that the feeder motor will be active for this feed pattern.
    ///
    /// This function iterates through all defined `FeedPhase`es within the pattern and sums
    /// their individual `feed_duration` values. This total runtime is typically logged
    /// as part of a completed feed event.
    ///
    /// # Returns
    /// The total feeder run time for this pattern, expressed as an `f64` in seconds.
    pub fn calc_feeder_runtime(&self) -> f64 {
        let mut feeder_run_time = 0.0;

        for feedphase in &self.feedphases {
            feeder_run_time += feedphase.feed_duration as f64;
        }

        feeder_run_time
    }
}

/// Holds the configuration data of one feed phase.
#[derive(Clone, Debug, PartialEq)]
pub struct FeedPhase {
    /// duration of the pause before switching on the feeder motor in milliseconds
    pub pause_duration: i16,

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
    pub feed_duration: i16,

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
    #[cfg(test)]
    // Creates a new `FeedPhase` struct with explicitly provided values.
    //
    // This constructor is intended exclusively for use in test environments.
    // It allows for the direct creation of a `FeedPhase` instance by setting
    // all its properties, bypassing typical deserialization or logic.
    //
    // # Arguments
    // * `pause_duration` - The duration of the pause phase in milliseconds.
    // * `pause_skimmer` - Flag indicating if the protein skimmer is active during the pause phase.
    // * `pause_main_pump_1` - Flag indicating if main pump #1 is active during the pause phase.
    // * `pause_main_pump_2` - Flag indicating if main pump #2 is active during the pause phase.
    // * `pause_aux_pump_1` - Flag indicating if auxiliary pump #1 is active during the pause phase.
    // * `pause_aux_pump_2` - Flag indicating if auxiliary pump #2 is active during the pause phase.
    // * `feed_duration` - The duration of the feed phase in milliseconds.
    // * `feed_skimmer` - Flag indicating if the protein skimmer is active during the feed phase.
    // * `feed_main_pump_1` - Flag indicating if main pump #1 is active during the feed phase.
    // * `feed_main_pump_2` - Flag indicating if main pump #2 is active during the feed phase.
    // * `feed_aux_pump_1` - Flag indicating if auxiliary pump #1 is active during the feed phase.
    // * `feed_aux_pump_2` - Flag indicating if auxiliary pump #2 is active during the feed phase.
    //
    // # Returns
    // A new `FeedPhase` struct initialized with the provided values.
    pub fn new(
        pause_duration: i16,
        pause_skimmer: bool,
        pause_main_pump_1: bool,
        pause_main_pump_2: bool,
        pause_aux_pump_1: bool,
        pause_aux_pump_2: bool,
        feed_duration: i16,
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
