pub struct SawToothProfileConfig {
    phase1_length: u32,
    phase2_length: u32,
    phase3_length: u32,
    phase4_length: u32,
}

pub struct SawToothProfile {
    counter: u32,
    level_normalized: f32,
    phase1_counter_limit: u32,
    phase2_counter_limit: u32,
    phase3_counter_limit: u32,
    phase4_counter_limit: u32,
}

impl SawToothProfile {
    // function provides a new instance of SawToothProfile
    ///
    /// # Arguments
    ///
    /// * `config` - struct of type SawToothProfileConfig with phase lengths in seconds
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

    // function shall be called once a second. It calculates the saw tooth profile.
    pub fn execute(&mut self) -> f32 {
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
        if self.counter > self.phase3_counter_limit && self.counter <= self.phase4_counter_limit {
            self.level_normalized = 0.0;
        }

        // if counter exceeds final limit, reset. Otherwise increment.
        if self.counter > self.phase4_counter_limit {
            self.counter = 0;
        } else {
            self.counter += 1;
        }
    }
}
