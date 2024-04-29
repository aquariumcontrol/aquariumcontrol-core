use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;

/// This struct holds the configuration data for the refill control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct RefillConfig {
    initial_wait_interval: u64,
    check_interval: u64,
}

/// This struct contains the configuration and the implementation for the refill control.
pub struct Refill {
    config: RefillConfig,
    sleep_duration: Duration,
    spin_sleeper: SpinSleeper,
}

impl Refill {
    /// This function returns a new instance of Refill
    pub fn new(config: RefillConfig) -> Refill {
        let check_interval_secs = config.check_interval;
        Self {
            config: config,
            sleep_duration: Duration::new(check_interval_secs, 0),
            spin_sleeper: SpinSleeper::default(),
        }
    }

    /// this function contains the main cycle of refill control
    pub fn execute(&mut self, tx_to_gpio: Sender<InternalCommand>, rx_from_gpio: Receiver<bool>) {
        let mut tank_level_switch_position: bool = true;
        let mut tank_level_switch_invalid: bool = false;
        let mut tank_level_switch_position_error: bool = false;

        // initial waiting time
        if self.config.initial_wait_interval > 0 {
            let sleep_duration_one_sec = Duration::new(1, 0);
            let mut i = 0;
            while i < self.config.initial_wait_interval {
                i = i + 1;
                self.spin_sleeper.sleep(sleep_duration_one_sec);
            }
        }

        // main control
        loop {
            self.spin_sleeper.sleep(self.sleep_duration);
            // send request for tank level switch position
            match tx_to_gpio.send(InternalCommand::RequestSignal(
                AquariumSignal::TankLevelSwitchPosition,
            )) {
                Ok(_) => {
                    // now listen for response
                    tank_level_switch_position = match rx_from_gpio.recv() {
                        Ok(c) => c,
                        Err(_) => {
                            tank_level_switch_position_error = true; // information is used in subsequent signal
                            false
                        }
                    };
                }
                Err(e) => {
                    println!("Refill: Error in channel communication to GPIO handler when requesting tank level switch position: {e:?}");
                }
            }

            // send request for tank level switch invalid
            match tx_to_gpio.send(InternalCommand::RequestSignal(
                AquariumSignal::TankLevelSwitchInvalid,
            )) {
                Ok(_) => {
                    // now listen for response
                    tank_level_switch_invalid = match rx_from_gpio.recv() {
                        Ok(c) => c | tank_level_switch_position_error, // consider status of previous signal
                        Err(_) => true,
                    };
                }
                Err(e) => {
                    println!("Refill: Error in channel communication to GPIO handler when requesting tank level switch invalid: {e:?}");
                }
            }

            if !tank_level_switch_position && !tank_level_switch_invalid {
                println!("Tank level switch indicates lower water level.");
            }
        }
    }
}
