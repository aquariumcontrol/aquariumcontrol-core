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
    initial_wait_interval: i64,
    check_interval: i64,
    max_refill_count_24h: i64,
    max_refill_volume_24h: f64,
    max_refill_count_hour: i64,
    max_refill_volume_hour: f64,
    min_interval_last_refill: i64,
    tank_level_switch_position_high_stabilization_count: u32,
    max_duration_since_last_refill: i64,
}

/// This struct contains the configuration and the implementation for the refill control.
pub struct Refill {
    config: RefillConfig,
    sql_interface_refill: SqlInterfaceRefill,
    tank_level_switch_position_high_stabilization_counter: u32,
    error_switch_stuck_high: bool,
    error_switch_stuck_low: bool,
}

struct RefillSqlData {
    duration_since_last_refill: i64,
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
            tank_level_switch_position_high_stabilization_counter: 0,
            error_switch_stuck_high: false,
            error_switch_stuck_low: false,
        }
    }

    /// function calculates a stabilized tank level switch position signal considering:
    /// - if invalid bit of signal is true, then use the current value of the stabilized position signal.
    /// - if the new signal is true (indicating high water level), then set the stabilized signal immediately to true (high).
    /// - if the the new signal is false (indicating low water level), then set increase the stabilization counter.
    /// - if the stabiliyation counter exceeds the configured limit, then set the counter to zero and set the stabilized
    /// signal to true indicating high water level.
    fn calc_tank_level_switch_position_stabilized(
        &mut self,
        current_signal: bool,
        new_signal: bool,
        signal_invalid: bool,
    ) -> bool {
        if signal_invalid {
            current_signal
        } else {
            match new_signal {
                true => true,
                false => {
                    if self.tank_level_switch_position_high_stabilization_counter
                        < self.config.tank_level_switch_position_high_stabilization_count
                    {
                        self.tank_level_switch_position_high_stabilization_counter += 1;
                        true
                    } else {
                        self.tank_level_switch_position_high_stabilization_counter = 0;
                        false
                    }
                }
            }
        }
    }

    /// retrieve the refill-related information from SQL database:
    /// - duration since last refill in seconds
    /// - volume refilled in last 24h 
    /// - number of refill operations in last 24h
    /// - volume refilled in the last hour
    /// - number of refill operations in the last hour
    fn get_sql_refill_data(&mut self) -> RefillSqlData {
        let duration_since_last_refill =
            match self.sql_interface_refill.get_duration_since_last_refill() {
                Ok(c) => c,
                Err(e) => {
                    panic!("Could not get duration since last refill event: {e:?}");
                }
            };

        let refill_volume_last_24h = match self.sql_interface_refill.get_refill_volume_of_last_24h()
        {
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

        let refill_volume_last_hour =
            match self.sql_interface_refill.get_refill_volume_of_last_hour() {
                Ok(c) => c,
                Err(e) => {
                    panic!("Could not get refill volume of last hour: {e:?}");
                }
            };

        let refill_count_last_hour = match self.sql_interface_refill.get_refill_count_of_last_hour()
        {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get refill count of last hour: {e:?}");
            }
        };
        RefillSqlData {
            duration_since_last_refill,
            refill_volume_last_24h,
            refill_count_last_24h,
            refill_volume_last_hour,
            refill_count_last_hour,
        }
    }

    /// function checks if the duration since the last refill operation eceeds a configurable limit.
    /// if yes, then corresponding error flag is set.
    fn check_error_condition_switch_stuck_high(&mut self, duration_since_last_refill: i64) {
        self.error_switch_stuck_high = duration_since_last_refill > self.config.max_duration_since_last_refill;
    }
    
    /// this function contains the main cycle of refill control
    pub fn execute(&mut self, tx_to_gpio: Sender<InternalCommand>, rx_from_gpio: Receiver<bool>) {
        let sleep_duration_one_sec = Duration::new(1, 0);
        let mut i = 0;
        let spin_sleeper = SpinSleeper::default();

        // retrieving data from SQL data base
        let mut sql_refill_data = self.get_sql_refill_data();
        println!(
            "Duration since last refill is {} seconds.",
            sql_refill_data.duration_since_last_refill
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

        // initial waiting time
        if self.config.initial_wait_interval > 0 {
            while i < self.config.initial_wait_interval {
                i = i + 1;
                spin_sleeper.sleep(sleep_duration_one_sec);
            }
        }
        sql_refill_data.duration_since_last_refill =
            sql_refill_data.duration_since_last_refill + self.config.initial_wait_interval;
        let mut tank_level_switch_position: bool;
        let mut tank_level_switch_invalid: bool = false;
        let mut tank_level_switch_position_error: bool = false;
        let mut tank_level_switch_position_stabilized: bool = true;
        // main control
        loop {
            i = 0;
            while i < self.config.check_interval {
                i = i + 1;
                spin_sleeper.sleep(sleep_duration_one_sec);
            }
            sql_refill_data.duration_since_last_refill =
                sql_refill_data.duration_since_last_refill + self.config.check_interval;
            
            self.check_error_condition_switch_stuck_high(sql_refill_data.duration_since_last_refill);
            
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
                    tank_level_switch_position_stabilized =
                        self.calc_tank_level_switch_position_stabilized(
                            tank_level_switch_position_stabilized,
                            tank_level_switch_position,
                            tank_level_switch_invalid,
                        );
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

            if !tank_level_switch_position_stabilized && !tank_level_switch_invalid {
                println!("Stabilized tank level switch position indicates low water level.");

                // execute checks before initiating refill
                let refill_check_volume_last_24h =
                    sql_refill_data.refill_volume_last_24h > self.config.max_refill_volume_24h;
                if refill_check_volume_last_24h {
                    println!(
                        "Refill volume of last 24h exceeds permissible refill volume per 24h of {}",
                        self.config.max_refill_volume_24h
                    )
                }
                let refill_check_count_last_24h =
                    sql_refill_data.refill_count_last_24h > self.config.max_refill_count_24h;
                if refill_check_count_last_24h {
                    println!(
                        "Refill count of last 24h exceeds permissible refill count per 24h of {}",
                        self.config.max_refill_count_24h
                    )
                }
                let refill_check_volume_last_hour =
                    sql_refill_data.refill_volume_last_hour > self.config.max_refill_volume_hour;
                if refill_check_volume_last_hour {
                    println!(
                        "Refill volume of last hour exceeds permissible refill volume per hour of {}",
                        self.config.max_refill_volume_hour
                    )
                }
                let refill_check_count_last_hour =
                    sql_refill_data.refill_count_last_hour > self.config.max_refill_count_hour;
                if refill_check_count_last_hour {
                    println!(
                        "Refill count of last 24h exceeds permissible refill count per hour of {}",
                        self.config.max_refill_count_hour
                    )
                }

                let refill_check_intervall_last_refill = sql_refill_data.duration_since_last_refill
                    < self.config.min_interval_last_refill;
                if refill_check_intervall_last_refill {
                    println!("Time interval since last refill is too small.");
                }
                else {
                    if !refill_check_volume_last_24h && !refill_check_count_last_24h &&
                        !refill_check_volume_last_hour && !refill_check_count_last_24h {
                        if !self.error_switch_stuck_low {
                            // conditions for allowing refill operation are given        
                            println!("Executing refill.");                                    
                        }
                    }
                }
            }
        }
    }
}
