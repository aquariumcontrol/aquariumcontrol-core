/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(feature = "debug_sawtooth_profile")]
use log::debug;

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
    #[cfg(test)]
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
    /// Provides output of the struct for debugging purposes.
    /// # Arguments
    /// * `f` - configuration for formatting
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
    /// Provides a struct of type SawToothProfile.
    /// # Arguments
    /// * `config` - struct of type SawToothProfileConfig with phase lengths in seconds
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

    /// Calculates the current value of the saw tooth profile.
    /// Function receives increment as a parameter
    /// It updates the counter and the normalized level of the saw tooth profile.
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
        if self.counter >= self.phase1_counter_limit && self.counter <= self.phase2_counter_limit {
            self.level_normalized = 1.0;
        }
        // for the next phase, ramp down from 1 to 0
        if self.counter > self.phase2_counter_limit && self.counter < self.phase3_counter_limit {
            self.level_normalized = 1.0
                - (self.counter - self.phase2_counter_limit)
                    / (self.phase3_counter_limit - self.phase2_counter_limit);
        }
        // for the next phase, keep the signal at 0
        if self.counter >= self.phase3_counter_limit && self.counter <= self.phase4_counter_limit {
            self.level_normalized = 0.0;
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::utilities::sawtooth_profile::SawToothProfile;
    use crate::utilities::sawtooth_profile::SawToothProfileConfig;

    // test case iterates through the profile:
    // - beginning/end of each phase
    // - middle of each phase
    #[test]
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
