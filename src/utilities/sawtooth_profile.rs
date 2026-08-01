/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Provides a configurable, four-phase sawtooth wave generator.
//!
//! This module is designed to create a periodic, normalized signal (`0.0` to `1.0`)
//! that follows a sawtooth (or trapezoidal) pattern. Its primary purpose is to
//! provide a "PWM-like" modulation for simple on/off actuators, such as the
//! heater or ventilation fans.
//!
//! ## Purpose and Design
//!
//! In the `Heating` and `Ventilation` modules, this profile is used for proportional
//! control. When the measured temperature is within the control band, the actuator's
//! on/off state is determined by comparing the sawtooth's current output level
//! against a normalized control deviation. This allows the system to modulate the
//! "duty cycle" of the actuator, providing smoother control than a simple on/off
//! hysteresis.
//!
//! The profile consists of four distinct phases, each with a configurable length:
//! 1.  **Phase 1**: A linear ramp-up from 0.0 to 1.0.
//! 2.  **Phase 2**: A constant "high" period where the output is 1.0.
//! 3.  **Phase 3**: A linear ramp-down from 1.0 to 0.0.
//! 4.  **Phase 4**: A constant "low" period where the output is 0.0.
//!
//! The cycle then repeats.
//!
//! ## Key Components
//!
//! - **`SawToothProfileConfig`**: A `serde`-compatible struct for deserializing the
//!   lengths of the four phases from the main TOML configuration file.
//!
//! - **`SawToothProfile`**: The main runtime struct that holds the generator's state,
//!   including the internal `counter` and the current `level_normalized` output.
//!
//! - **`new()`**: The constructor, which takes a `SawToothProfileConfig` and
//!   pre-calculates the counter-limits for each phase for efficient runtime execution.
//!
//! - **`execute_with()`**: The core method that advances the profile's internal
//!   counter by a given `increment` and recalculates the `level_normalized` output
//!   based on the current phase. This is typically called once per control loop cycle.

use serde_derive::Deserialize;
use std::fmt;

/// Holds the data for the calculation of the saw tooth profile.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Clone)]
pub struct SawToothProfileConfig {
    /// Phase 1 is a ramp from zero to one.
    /// The length describes the number of iterations necessary to reach the end of the ramp.
    phase1_length: u32,

    /// Phase 2 is a constant value of one.
    /// The length describes for how many iterations this value is kept.
    phase2_length: u32,

    /// Phase 3 is a ramp from one to zero.
    /// The length describes the number of iterations necessary to reach the end of the ramp.
    phase3_length: u32,

    /// Phase 4 is a constant value of zero.
    /// The length describes for how many iterations this value is kept.    
    phase4_length: u32,
}

impl SawToothProfileConfig {
    /// Creates a new `SawToothProfileConfig` with specified phase lengths.
    ///
    /// This constructor is exclusively for use in test environments to quickly
    /// define and set up a sawtooth profile configuration without needing to
    /// load from a file or use default deserialization.
    ///
    /// # Arguments
    /// * `phase1_length` - The length of the first phase (ramp up from 0 to 1).
    /// * `phase2_length` - The length of the second phase (constant value of 1).
    /// * `phase3_length` - The length of the third phase (ramp down from 1 to 0).
    /// * `phase4_length` - The length of the fourth phase (constant value of 0).
    //
    /// # Returns
    /// A new `SawToothProfileConfig` instance initialized with the given phase lengths.
    pub fn new(
        phase1_length: u32,
        phase2_length: u32,
        phase3_length: u32,
        phase4_length: u32,
    ) -> SawToothProfileConfig {
        SawToothProfileConfig {
            phase1_length,
            phase2_length,
            phase3_length,
            phase4_length,
        }
    }
}

/// Contains the profile configuration and the implementation for the saw tooth profile.
pub struct SawToothProfile {
    /// This variable counts the number of iterations executed and is reset to zero when reaching the end of the profile.
    counter: f32,

    /// Variable with values from zero to one. This is the output of the saw tooth profile component.
    pub level_normalized: f32,

    /// This variable is set to the same value as the length of the first phase.
    phase1_counter_limit: f32,

    /// This variable is set to the cumulated length of the first and second phase.
    phase2_counter_limit: f32,

    /// This variable is set to the cumulated length of the first three phases.
    phase3_counter_limit: f32,

    /// This variable is set to the cumulated length of all phases.
    phase4_counter_limit: f32,
}

impl fmt::Display for SawToothProfile {
    /// Formats the `SawToothProfile` struct into a multi-line, human-readable string.
    ///
    /// This implementation is primarily for debugging and logging purposes, providing
    /// a detailed view of the filter's current state, including its internal counter,
    /// normalized output level, and the limits for each phase.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to the formatter, as required by the `fmt::Display` trait.
    ///
    /// # Returns
    /// A `fmt::Result` indicating whether the formatting operation was successful.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "SawToothProfile:\n\
            \tcounter={}\n\
            \tlevel_normalized={}\n\
            \tphase1_counter_limit={}\n\
            \tphase2_counter_limit={}\n\
            \tphase3_counter_limit={}\n\
            \tphase4_counter_limit={}",
            self.counter,
            self.level_normalized,
            self.phase1_counter_limit,
            self.phase2_counter_limit,
            self.phase3_counter_limit,
            self.phase4_counter_limit
        )
    }
}

impl SawToothProfile {
    /// Creates a new `SawToothProfile` instance based on the provided configuration.
    ///
    /// This constructor initializes the sawtooth wave generator, setting up its
    /// internal counter and pre-calculating the limits for each phase based on
    /// the lengths defined in the `SawToothProfileConfig`. The `level_normalized`
    /// output is initially set to `0.0`.
    ///
    /// # Arguments
    /// * `config` - A reference to a `SawToothProfileConfig` struct, which specifies
    ///   the desired lengths of the four phases (ramp up, constant high, ramp down, constant low)
    ///   in terms of iterations or time units.
    ///
    /// # Returns
    /// A new `SawToothProfile` struct, ready to generate values.
    pub fn new(config: &SawToothProfileConfig) -> SawToothProfile {
        SawToothProfile {
            counter: 0.0,
            level_normalized: 0.0,
            phase1_counter_limit: config.phase1_length as f32,
            phase2_counter_limit: (config.phase1_length + config.phase2_length) as f32,
            phase3_counter_limit: (config.phase1_length
                + config.phase2_length
                + config.phase3_length) as f32,
            phase4_counter_limit: (config.phase1_length
                + config.phase2_length
                + config.phase3_length
                + config.phase4_length) as f32,
        }
    }

    /// Advances the internal state of the sawtooth profile and calculates its current normalized output level.
    ///
    /// This method increments an internal counter. When the counter reaches the end of a full cycle
    /// (defined by the sum of all phase lengths), it resets to the beginning.
    /// Based on the current value of the counter, the function then determines which phase of the
    /// sawtooth profile is active and updates `level_normalized` accordingly. The output
    /// `level_normalized` ranges from `0.0` to `1.0`.
    ///
    /// # Arguments
    /// * `increment` - The step value by which the internal counter should be advanced in this execution.
    ///   This value determines the "speed" at which the profile progresses.
    pub fn execute_with(&mut self, increment: f32) {
        // If the counter exceeds the final limit, reset it. Otherwise, increment.
        if self.counter >= (self.phase4_counter_limit - 3.0 * increment) {
            #[cfg(feature = "debug_sawtooth_profile")]
            debug!("saw tooth profile resetting phase");
            self.counter = 0.0;
        } else {
            self.counter += increment;
        }

        // in the first phase, increase level to max.
        if self.counter < self.phase1_counter_limit {
            self.level_normalized = self.counter / self.phase1_counter_limit;
        }
        // for the next phase, keep the signal at 1
        else if self.counter >= self.phase1_counter_limit
            && self.counter <= self.phase2_counter_limit
        {
            self.level_normalized = 1.0;
        }
        // for the next phase, ramp down from 1 to 0
        else if self.counter > self.phase2_counter_limit
            && self.counter < self.phase3_counter_limit
        {
            self.level_normalized = 1.0
                - (self.counter - self.phase2_counter_limit)
                    / (self.phase3_counter_limit - self.phase2_counter_limit);
        }
        // for the next phase, keep the signal at 0
        else if self.counter >= self.phase3_counter_limit
            && self.counter <= self.phase4_counter_limit
        {
            self.level_normalized = 0.0;
        }
        #[cfg(feature = "debug_sawtooth_profile")]
        println!(
            "[saw_tooth_profile] level_normalized={}",
            self.level_normalized
        );
    }
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub mod tests { // unit tests shall not appear in code coverage measurement
    use crate::utilities::sawtooth_profile::SawToothProfile;
    use crate::utilities::sawtooth_profile::SawToothProfileConfig;

    // test case iterates through the profile:
    // - beginning/end of each phase
    // - middle of each phase
    #[test]
    // Verifies that saw tooth profile.
    pub fn test_saw_tooth_profile() {
        let saw_tooth_profile_config = SawToothProfileConfig {
            phase1_length: 10,
            phase2_length: 2,
            phase3_length: 10,
            phase4_length: 2,
        };
        let mut saw_tooth_profile = SawToothProfile::new(&saw_tooth_profile_config);
        println!("{}", saw_tooth_profile);

        // initial calculation of normalized level
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 0.0);
        assert_eq!(saw_tooth_profile.counter, 0.0);

        // iterate to the middle of the first interval
        for _ in 0..5 {
            saw_tooth_profile.execute_with(1.0);
        }
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 0.5);
        assert_eq!(saw_tooth_profile.counter, 5.0);

        // iterate to the end of the first interval
        for _ in 0..5 {
            saw_tooth_profile.execute_with(1.0);
        }
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 1.0);
        assert_eq!(saw_tooth_profile.counter, 10.0);

        // iterate to the middle of the second interval
        saw_tooth_profile.execute_with(1.0);
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 1.0);
        assert_eq!(saw_tooth_profile.counter, 11.0);

        // iterate to the end of the second interval
        saw_tooth_profile.execute_with(1.0);
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 1.0);
        assert_eq!(saw_tooth_profile.counter, 12.0);

        // iterate to the middle of the third interval
        for _ in 0..5 {
            saw_tooth_profile.execute_with(1.0);
        }
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 0.5);
        assert_eq!(saw_tooth_profile.counter, 17.0);

        // iterate to the end of the third interval
        for _ in 0..5 {
            saw_tooth_profile.execute_with(1.0);
        }
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 0.0);
        assert_eq!(saw_tooth_profile.counter, 0.0);

        // iterate to the middle of the fourth interval
        saw_tooth_profile.execute_with(1.0);
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 0.1);
        assert_eq!(saw_tooth_profile.counter, 1.0);

        // trigger start of new cycle
        saw_tooth_profile.execute_with(1.0);
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 0.2);
        assert_eq!(saw_tooth_profile.counter, 2.0);

        // check if the new cycle continues as expected
        saw_tooth_profile.execute_with(1.0);
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 0.3);
        assert_eq!(saw_tooth_profile.counter, 3.0);
    }
}
