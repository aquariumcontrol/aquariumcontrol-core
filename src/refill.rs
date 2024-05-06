use chrono::Local;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::channel_content::AquariumDevice;
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
    refill_pump_flow: f64,
    max_refill_volume: f64,
    additional_refill_volume: f64,
}

/// This struct contains the configuration and the implementation for the refill control.
pub struct Refill {
    config: RefillConfig,
    sql_interface_refill: SqlInterfaceRefill,
    tank_level_switch_position: bool,
    tank_level_switch_invalid: bool,
    tank_level_switch_position_stabilized: bool,
    tank_level_switch_position_high_stabilization_counter: u32,
    error_switch_stuck_high: bool,
    error_switch_stuck_low: bool,
    error_receive_tank_level_switch_position: bool,
    error_sql_update_failed: bool,
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
            tank_level_switch_position: true,
            tank_level_switch_invalid: true,
            tank_level_switch_position_stabilized: true,
            tank_level_switch_position_high_stabilization_counter: 0,
            error_switch_stuck_high: false,
            error_switch_stuck_low: false,
            error_receive_tank_level_switch_position: false,
            error_sql_update_failed: false,
        }
    }

    /// function calculates a stabilized tank level switch position signal considering:
    /// - if invalid bit of signal is true, then use the current value of the stabilized position signal.
    /// - if the new signal is true (indicating high water level), then set the stabilized signal immediately to true (high).
    /// - if the the new signal is false (indicating low water level), then set increase the stabilization counter.
    /// - if the stabiliyation counter exceeds the configured limit, then set the counter to zero and set the stabilized
    /// signal to true indicating high water level.
    fn calc_tank_level_switch_position_stabilized(&mut self) {
        if !self.tank_level_switch_invalid {
            // update stabilized signal only when new signal is not invalid
            self.tank_level_switch_position_stabilized = match self.tank_level_switch_position {
                true => true, // set stabilized to true without delay
                false => {
                    if self.tank_level_switch_position_high_stabilization_counter
                        < self
                            .config
                            .tank_level_switch_position_high_stabilization_count
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
        self.error_switch_stuck_high =
            duration_since_last_refill > self.config.max_duration_since_last_refill;
    }

    /// function checks if there are new messages in the channel.
    /// if the message contains the quit command, then the function returns true, otherwise false.
    /// if the message contains the command to reset all errors, the function will set all error bits
    /// to false and return false.
    fn check_pending_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
    ) -> bool {
        match rx_from_signal_handler.try_recv() {
            Ok(c) => match c {
                InternalCommand::Quit => true,
                InternalCommand::ResetAllErrors => {
                    self.error_switch_stuck_high = false;
                    self.error_switch_stuck_low = false;
                    self.error_receive_tank_level_switch_position = false;
                    self.error_sql_update_failed = false;
                    false
                }
                _ => {
                    println!("Refill: ignoring not applicable command from signal handler.");
                    false
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        println!(
                            "Refill: Error in trying to receive command from signal handler: {e:?}"
                        );
                    }
                }
                false
            }
        }
    }

    /// function executes the channel communication to update the tank level switch position signal
    /// as well as the tank level switch invalid signal.
    fn update_tank_level_switch_signals(
        &mut self,
        tx_to_gpio: &Sender<InternalCommand>,
        rx_from_gpio: &Receiver<f32>,
    ) {
        // send request for tank level switch position
        match tx_to_gpio.send(InternalCommand::RequestSignal(
            AquariumSignal::TankLevelSwitchPosition,
        )) {
            Ok(_) => {
                //println!("Refill: Sent request for tank level switch position.");
                // now listen for response
                self.tank_level_switch_position = match rx_from_gpio.recv() {
                    Ok(c) => {
                        //println!("Refill: Received tank level switch position.");
                        c > 0.0
                    }
                    Err(_) => {
                        self.error_receive_tank_level_switch_position = true; // information is used in subsequent signal
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
                self.tank_level_switch_invalid = match rx_from_gpio.recv() {
                    Ok(c) => c > 0.0,
                    Err(_) => true,
                };
            }
            Err(e) => {
                println!("Refill: Error in channel communication to GPIO handler when requesting tank level switch invalid: {e:?}");
            }
        }

        // calculate the stabilized tank level signal to be used in main control loop
        self.calc_tank_level_switch_position_stabilized();
    }

    // this function checks if historic refill data does not exceed limitations in terms of count and volume.
    // the function returns a tuple of boolean variables
    fn check_volume_count_time_limits(&self, sql_refill_data: &RefillSqlData) -> bool {
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
        let refill_check_intervall_last_refill =
            sql_refill_data.duration_since_last_refill < self.config.min_interval_last_refill;
        if refill_check_intervall_last_refill {
            println!(
                "Time interval since last refill {} is too small (limit={}).",
                sql_refill_data.duration_since_last_refill,
                self.config.min_interval_last_refill
            );
        }
        refill_check_volume_last_24h
            | refill_check_count_last_24h
            | refill_check_volume_last_hour
            | refill_check_count_last_24h
            | refill_check_intervall_last_refill
    }

    /// this function executes the refill operation
    fn execute_refill(
        &mut self,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
        tx_to_gpio: &Sender<InternalCommand>,
        rx_from_gpio: &Receiver<f32>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
    ) -> bool {
        let mut termination_request_received = false;
        // command start of refill pump
        match tx_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::RefillPump)) {
            Ok(_) => {
                // now listen for response
                match rx_from_controllino.recv() {
                    Ok(_) => {}
                    Err(e) => {
                        println!("Refill: Error when trying to receive answer from Controllino when switching on refill pump: {e:?}")
                    }
                };
            }
            Err(e) => {
                println!("Refill: Error in channel communication to Controllino when switching on refill pump: {e:?}");
            }
        }
        println!("Refill: Sent request to switch on refill pump to Controllino thread.");

        //*** wait for condition to stop refilling ***
        let spin_sleeper = SpinSleeper::default();
        let sleep_interval_millis = 100;
        let sleep_duration_hundred_milli_sec = Duration::from_millis(sleep_interval_millis);
        let sleep_interval_seconds: f64 = (sleep_interval_millis as f64) / (1000 as f64);
        let mut refill_volume = 0.0;
        let mut refill_time_seconds = 0.0;
        let mut refill_error_code = 0;
        let mut additional_refill_volume_executed = 0.0;
        let refill_increment = sleep_interval_seconds * self.config.refill_pump_flow;

        loop {
            spin_sleeper.sleep(sleep_duration_hundred_milli_sec);
            refill_time_seconds += sleep_interval_seconds;
            self.update_tank_level_switch_signals(&tx_to_gpio, rx_from_gpio);
            if self.tank_level_switch_position
                && additional_refill_volume_executed > self.config.additional_refill_volume
            {
                // water level changed to high
                break;
            }
            if self.tank_level_switch_invalid {
                // switch signal is invalid
                refill_error_code = 1;
                break;
            }

            // calculate volume refilled in this operation
            refill_volume += refill_increment;
            if self.tank_level_switch_position {
                additional_refill_volume_executed += refill_increment;
                println!(
                    "Executing additional refill at {}L, limit at {}L",
                    additional_refill_volume_executed, self.config.additional_refill_volume
                );
            }
            termination_request_received =
                self.check_pending_external_request(&rx_from_signal_handler);
            if termination_request_received {
                break;
            }
            println!(
                "Refill: current refill volume at {}L, limit={}L",
                refill_volume, self.config.max_refill_volume
            );
            if refill_volume > self.config.max_refill_volume {
                self.error_switch_stuck_low = true;
                refill_error_code = 2;
                break;
            }
        }

        // command stop of refill pump
        match tx_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::RefillPump)) {
            Ok(_) => {
                // now listen for response
                match rx_from_controllino.recv() {
                    Ok(_) => {}
                    Err(e) => {
                        println!("Refill: Error when trying to receive answer from Controllino when switching off refill pump: {e:?}")
                    }
                };
            }
            Err(e) => {
                println!("Refill: Error in channel communication to Controllino when switching off refill pump: {e:?}");
            }
        }
        println!("Refill: Sent request to switch off refill pump to Controllino thread.");

        // write result to data base
        match self.sql_interface_refill.insert_refill_event(
            Local::now().naive_local(),
            refill_time_seconds,
            refill_volume,
            refill_error_code,
        ) {
            Ok(_) => {}
            Err(e) => {
                println!("Refill: error occurred when writing refill operation data to SQL database: {e:?}");
                self.error_sql_update_failed = true;
            }
        }
        termination_request_received
    }

    /// this function contains the main cycle of refill control
    pub fn execute(
        &mut self,
        mutex_device_scheduler_refill: Arc<Mutex<i32>>,
        tx_to_gpio: Sender<InternalCommand>,
        rx_from_gpio: Receiver<f32>,
        tx_to_controllino: Sender<InternalCommand>,
        rx_from_controllino: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
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

        let mut termination_request_received = false;

        // main control loop for refill
        loop {
            // check for external requests while waiting for the check interval to pass
            println!("Refill: executing main loop.");
            i = 0;
            while i < self.config.check_interval {
                i = i + 1;
                spin_sleeper.sleep(sleep_duration_one_sec);
                termination_request_received =
                    self.check_pending_external_request(&rx_from_signal_handler);
                if termination_request_received {
                    break; // exit inner loop
                }
            }
            if termination_request_received {
                break; // exit outer loop
            }

            // check when the last refill operation has taken place
            // if too much time has passed, issue an error condition - not impeding rest of operation
            sql_refill_data.duration_since_last_refill =
                sql_refill_data.duration_since_last_refill + self.config.check_interval;
            self.check_error_condition_switch_stuck_high(
                sql_refill_data.duration_since_last_refill,
            );

            // either communicate with real HW or with simulator to get updated signal information
            // this call will also update the stabilized tank level position.
            self.update_tank_level_switch_signals(&tx_to_gpio, &rx_from_gpio);

            if !self.tank_level_switch_position_stabilized && !self.tank_level_switch_invalid {
                println!("Stabilized tank level switch position indicates low water level.");

                // execute checks before initiating refill
                if !self.check_volume_count_time_limits(&sql_refill_data) {
                    if !self.error_switch_stuck_low && !self.error_sql_update_failed {
                        // conditions for allowing refill operation are given
                        println!("Executing refill.");
                        // inner scope to limit lifetime of unlocked mutex
                        {
                            let mut mutex_data = mutex_device_scheduler_refill.lock().unwrap();
                            // now we are given exclusive access to actuators
                            termination_request_received = self.execute_refill(
                                &tx_to_controllino,
                                &rx_from_controllino,
                                &tx_to_gpio,
                                &rx_from_gpio,
                                &rx_from_signal_handler,
                            );
                            *mutex_data += 1;
                        }
                        if termination_request_received {
                            break;
                        }
                    }
                }
            }
        }
        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                println!("Error in trying to send confirmation back to signal handler: {e:?}");
            }
        };
    }
}

#[cfg(test)]
pub mod tests {
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::refill::Refill;
    use crate::sql_interface::SqlInterface;
    use crate::sql_interface_refill::SqlInterfaceRefill;

    fn prepare_refill_tests() -> Refill {
        // prepare struct Refill for subsequent tests
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_refill.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface = match SqlInterface::new(config.sql_interface) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let mut sql_interface_refill = SqlInterfaceRefill::new(sql_interface.get_connection());
        Refill::new(config.refill, sql_interface_refill)
    }

    #[test]
    pub fn test_calc_tank_level_switch_position_stabilized_inhibition_false_true_when_invalid() {
        //*** test if position signal is not updating when invalid signal is true ***
        let mut refill = prepare_refill_tests();
        // check transition false -> true
        refill.tank_level_switch_invalid = true;
        refill.tank_level_switch_position = true;
        refill.tank_level_switch_position_stabilized = false;
        for _ in 0..10 {
            refill.calc_tank_level_switch_position_stabilized();
            assert_eq!(refill.tank_level_switch_position_stabilized, false);
        }
        println!("* checking inhibition of change false -> true for stabilized tank level position when invalid bit is true succeeded.");
    }

    #[test]
    pub fn test_calc_tank_level_switch_position_stabilized_inhibition_true_false_when_invalid() {
        //*** test if position signal is not updating when invalid signal is true ***
        let mut refill = prepare_refill_tests();
        // check transition true -> false
        refill.tank_level_switch_invalid = true;
        refill.tank_level_switch_position = false;
        refill.tank_level_switch_position_stabilized = true;
        for _ in 0..10 {
            refill.calc_tank_level_switch_position_stabilized();
            assert_eq!(refill.tank_level_switch_position_stabilized, true);
        }
        println!("* checking inhibition of change true -> false for stabilized tank level position when invalid bit is true succeeded.");
    }

    #[test]
    pub fn test_calc_tank_level_switch_position_stabilized_false_true() {
        //*** test if position signal is updating without delay when invalid signal is false ***
        let mut refill = prepare_refill_tests();
        // check transition true -> false
        refill.tank_level_switch_invalid = false;
        refill.tank_level_switch_position = true;
        refill.tank_level_switch_position_stabilized = false;
        refill.calc_tank_level_switch_position_stabilized();
        assert_eq!(refill.tank_level_switch_position_stabilized, true);
        println!("* checking immediate change false -> true for stabilized tank level position when invalid bit is false succeeded.");
    }

    #[test]
    pub fn test_calc_tank_level_switch_position_stabilized_true_false() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_refill.toml".to_string());
        //*** test if position signal is updating with delay when invalid signal is false ***
        let mut refill = prepare_refill_tests();
        // check transition true -> false
        refill.tank_level_switch_invalid = false;
        refill.tank_level_switch_position = false;
        refill.tank_level_switch_position_stabilized = true;
        for _ in 0..config
            .refill
            .tank_level_switch_position_high_stabilization_count
        {
            refill.calc_tank_level_switch_position_stabilized();
            assert_eq!(refill.tank_level_switch_position_stabilized, true);
        }
        refill.calc_tank_level_switch_position_stabilized();
        assert_eq!(refill.tank_level_switch_position_stabilized, false);
        println!("* checking delayed change true -> false for stabilized tank level position when invalid bit is false succeeded.");
    }
}
