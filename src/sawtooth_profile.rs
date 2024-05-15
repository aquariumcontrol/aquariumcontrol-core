use std::fmt;

pub struct SawToothProfileConfig {
    phase1_length: u32,
    phase2_length: u32,
    phase3_length: u32,
    phase4_length: u32,
}

pub struct SawToothProfile {
    counter: u32,
    pub level_normalized: f32,
    phase1_counter_limit: u32,
    phase2_counter_limit: u32,
    phase3_counter_limit: u32,
    phase4_counter_limit: u32,
}

impl fmt::Display for SawToothProfile {
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
    // function provides a new instance of SawToothProfile
    ///
    /// # Arguments
    ///
    /// * `config` - struct of type SawToothProfileConfig with phase lengths in seconds
    ///
    pub fn new(config: SawToothProfileConfig) -> SawToothProfile {
        SawToothProfile {
            counter: 0,
            level_normalized: 0.0,
            phase1_counter_limit: config.phase1_length,
            phase2_counter_limit: config.phase1_length + config.phase2_length,
            phase3_counter_limit: config.phase1_length
                + config.phase2_length
                + config.phase3_length,
            phase4_counter_limit: config.phase1_length
                + config.phase2_length
                + config.phase3_length
                + config.phase4_length,
        }
    }

    /// function shall be called once a second. It calculates the saw tooth profile.
    /// It updates the counter and the normalized level of the saw tooth profile.
    pub fn execute(&mut self) {
        // if counter exceeds final limit, reset. Otherwise increment.
        if self.counter >= (self.phase4_counter_limit - 1) {
            self.counter = 0;
        } else {
            self.counter += 1;
        }
        
        // in the first phase, increase level to max.
        if self.counter <= self.phase1_counter_limit {
            self.level_normalized = self.counter as f32 / self.phase1_counter_limit as f32;
        }
        // for the next phase, keep signal at 1
        if self.counter > self.phase1_counter_limit && self.counter <= self.phase2_counter_limit {
            self.level_normalized = 1.0;
        }
        // for the next phase, ramp down from 1 to 0
        if self.counter > self.phase2_counter_limit && self.counter <= self.phase3_counter_limit {
            self.level_normalized = 1.0
                - (self.counter - self.phase2_counter_limit) as f32
                    / (self.phase3_counter_limit - self.phase2_counter_limit) as f32;
        }
        // for the next phase, keep signal at 0
        if self.counter > self.phase3_counter_limit && self.counter < self.phase4_counter_limit {
            self.level_normalized = 0.0;
        }

    }
}

#[cfg(test)]
pub mod tests {
    use crate::sawtooth_profile::SawToothProfile;
    use crate::sawtooth_profile::SawToothProfileConfig;

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
        let mut saw_tooth_profile = SawToothProfile::new(saw_tooth_profile_config);
        println!("{}", saw_tooth_profile);

        // initial calculation of normalized level
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 0.0);
        assert_eq!(saw_tooth_profile.counter, 0);
        
        // iterate to the middle of first interval
        for _ in 0..5 {
            saw_tooth_profile.execute();
        }
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 0.5);
        assert_eq!(saw_tooth_profile.counter, 5);

        // iterate to the end of first interval
        for _ in 0..5 {
            saw_tooth_profile.execute();
        }
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 1.0);
        assert_eq!(saw_tooth_profile.counter, 10);

        // iterate to the middle of second interval
        saw_tooth_profile.execute();
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 1.0);
        assert_eq!(saw_tooth_profile.counter, 11);

        // iterate to the end of second interval
        saw_tooth_profile.execute();
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 1.0);
        assert_eq!(saw_tooth_profile.counter, 12);

        // iterate to the middle of third interval
        for _ in 0..5 {
            saw_tooth_profile.execute();
        }
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 0.5);
        assert_eq!(saw_tooth_profile.counter, 17);
        
        // iterate to the end of third interval
        for _ in 0..5 {
            saw_tooth_profile.execute();
        }
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 0.0);
        assert_eq!(saw_tooth_profile.counter, 22);
        
        // iterate to the middle of fourth interval
        saw_tooth_profile.execute();
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 0.0);
        assert_eq!(saw_tooth_profile.counter, 23);
        
        // trigger start of new cycle
        saw_tooth_profile.execute();
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 0.0);
        assert_eq!(saw_tooth_profile.counter, 0);

        // check if new cycle continues as expected
        saw_tooth_profile.execute();
        println!("{}", saw_tooth_profile);
        assert_eq!(saw_tooth_profile.level_normalized, 0.1);
        assert_eq!(saw_tooth_profile.counter, 1);
    }
}
