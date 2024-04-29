use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;
use crate::sql_interface_refill::SqlInterfaceRefill;

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
    sql_interface_refill: SqlInterfaceRefill,
}

struct RefillSqlData {
    duration_refill: i64,
    refill_volume_last_24h: f64,
    refill_count_last_24h: i64,
    refill_volume_last_hour: f64,
    refill_count_last_hour: i64,    
}

impl Refill {
    /// This function returns a new instance of Refill
    pub fn new(config: RefillConfig, sql_interface_refill: SqlInterfaceRefill) -> Refill {
        Self {
            config: config,
            sql_interface_refill: sql_interface_refill,
        }
    }

    fn get_sql_refill_data(&mut self) -> RefillSqlData {
        let duration_refill = match self.sql_interface_refill.get_duration_since_last_refill() {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get duration since last refill event: {e:?}");
            }
        };
        
        let refill_volume_last_24h = match self.sql_interface_refill.get_refill_volume_of_last_24h() {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get refill volume of last 24 hours: {e:?}");
            }
        };

        let refill_count_last_24h = match self.sql_interface_refill.get_refill_count_of_last_24h() {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get refill count of last 24 hours: {e:?}");
            }
        };
        
        let refill_volume_last_hour = match self.sql_interface_refill.get_refill_volume_of_last_hour() {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get refill volume of last hour: {e:?}");
            }
        };

        let refill_count_last_hour = match self.sql_interface_refill.get_refill_count_of_last_hour() {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get refill count of last hour: {e:?}");
            }
        };
        RefillSqlData {
            duration_refill,
            refill_volume_last_24h,
            refill_count_last_24h,
            refill_volume_last_hour,
            refill_count_last_hour,
        }
    }

    /// this function contains the main cycle of refill control
    pub fn execute(&mut self, tx_to_gpio: Sender<InternalCommand>, rx_from_gpio: Receiver<bool>) {
        let sleep_duration_one_sec = Duration::new(1, 0);
        let mut i = 0;
        let spin_sleeper = SpinSleeper::default();

        // get refill data from SQL database to check for problems early after startup
        _ = self.get_sql_refill_data();

        // initial waiting time
        if self.config.initial_wait_interval > 0 {
            while i < self.config.initial_wait_interval {
                i = i + 1;
                spin_sleeper.sleep(sleep_duration_one_sec);
            }
        }
        let mut tank_level_switch_position: bool = true;
        let mut tank_level_switch_invalid: bool = false;
        let mut tank_level_switch_position_error: bool = false;

        // main control
        loop {
            i = 0;
            while i < self.config.check_interval {
                i = i + 1;
                spin_sleeper.sleep(sleep_duration_one_sec);
            }
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

                // retrieving data from SQL data base
                let sql_refill_data = self.get_sql_refill_data();
                println!(
                    "Duration since last refill is {} seconds.",
                    sql_refill_data.duration_refill
                );
                println!(
                    "Refill volume of last 24h is {} L.",
                    sql_refill_data.refill_volume_last_24h
                );
                println!(
                    "Refill count of last 24h is {}.",
                    sql_refill_data.refill_count_last_24h
                );
                println!(
                    "Refill volume of last hour is {} L.",
                    sql_refill_data.refill_volume_last_hour
                );
                println!(
                    "Refill count of last hour is {}.",
                    sql_refill_data.refill_count_last_hour
                );
            }
        }
    }
}
