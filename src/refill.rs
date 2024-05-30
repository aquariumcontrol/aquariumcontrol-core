/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::Local;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::fmt;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::channel_content::AquariumDevice;
use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;
use crate::sql_interface_refill::SqlInterfaceRefill;

/// Holds the configuration data for the refill control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Clone)]
pub struct RefillConfig {
    /// control will only be active when the flag is set to true
    active: bool,

    /// time interval in seconds which need to pass after initialization of the application before refill control is becoming active
    initial_wait_interval: i64,

    /// interval for the main control in seconds    
    check_interval: i64,

    /// maximum number of permitted refill operations within last 24h
    /// - If this number is exceeded, then the refill control will not further refill even if water level sensor signal value is low.
    max_refill_count_24h: i64,

    /// maximum permitted refill volume within last 24h
    /// - If this number is exceeded, then the refill control will not further refill even if water level sensor signal value is low.
    max_refill_volume_24h: f64,

    /// maximum number of permitted refill operations within last hour
    /// - If this number is exceeded, then the refill control will not further refill even if water level sensor signal value is low.
    max_refill_count_hour: i64,

    /// maximum permitted refill volume within last hour
    /// - If this number is exceeded, then the refill control will not further refill even if water level sensor signal value is low.
    max_refill_volume_hour: f64,

    /// minimum time which has to pass after a refill operation before a subsequent refill operation is started
    min_interval_last_refill: i64,

    /// number of tank level switch position measurements which need to indicate a low level before the refill control assumes that the water level is low
    /// - Caution: Also consider the check_interval when calibrating this variable.
    tank_level_switch_position_high_stabilization_count: u32,

    /// time interval in seconds which need to pass before the application recognizes a potential tank level switch fault
    max_duration_since_last_refill: i64,

    /// flow of the refill pump in liters / second
    refill_pump_flow: f64,

    /// maximum permitted refill volume in one refill operation
    max_refill_volume: f64,

    /// refill volume which is added to the tank in a refill operation after the application recognizes tank level position high
    /// - Background: This volume shall avoid too frequent refill operations. Too high values may lead to unstable water level.
    /// It is reommended to calibrate a value which is a fraction of max_refill_volume.
    additional_refill_volume: f64,
}

/// Checks if there are new messages in the channel.
/// if the message contains the quit command, then the function returns true, otherwise false.
/// if the message contains the command to reset all errors, the function will set all error bits
/// to false and return false.
/// # Arguments
/// * `rx_from_signal_handler` - receiver part of channel for communication from signal handler
/// * `refill_errors` - mutable reference to the struct containing the error bits
fn check_pending_external_request(
    rx_from_signal_handler: &Receiver<InternalCommand>,
    refill_errors: &mut RefillErrors,
) -> bool {
    match rx_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Quit => true,
            InternalCommand::ResetAllErrors => {
                refill_errors.error_switch_stuck_high = false;
                refill_errors.error_switch_stuck_low = false;
                refill_errors.error_receive_tank_level_switch_position = false;
                refill_errors.error_sql_update_failed = false;
                false
            }
            _ => {
                println!("Refill: ignoring inapplicable command from signal handler.");
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

/// Trait for the execution of the refill once control has detected that water level is low.
/// This trait allows running the main control with a mock implementation for testing.
pub trait RefillExecutionTrait {
    fn execute_refill(
        &mut self,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
        tx_to_gpio: &Sender<InternalCommand>,
        rx_from_gpio: &Receiver<f32>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        tx_to_datalogger: &Sender<bool>,
        tank_level_switch_signals: &mut TankLevelSwitchSignals,
        refill_errors: &mut RefillErrors,
        config: &RefillConfig,
        sql_interface_refill: &mut SqlInterfaceRefill,
    ) -> bool;
}

/// Implementation of the RefillExecutionTrait which contains the code for executing the refill once
/// the main control has detected that the water level is low.
pub struct RefillExecution;

impl RefillExecutionTrait for RefillExecution {
    /// this function executes the refill operation
    fn execute_refill(
        &mut self,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
        tx_to_gpio: &Sender<InternalCommand>,
        rx_from_gpio: &Receiver<f32>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        tx_to_datalogger: &Sender<bool>,
        tank_level_switch_signals: &mut TankLevelSwitchSignals,
        refill_errors: &mut RefillErrors,
        config: &RefillConfig,
        sql_interface_refill: &mut SqlInterfaceRefill,
    ) -> bool {
        // inform data logger that refill is in progress
        println!("Refill: communicating to data logger thread...");
        match tx_to_datalogger.send(true) {
            Ok(_) => { /* no listening to response */ }
            Err(e) => {
                println!(
                "Refill: Error in channel communication to DataLogger when starting refill: {e:?}"
            );
            }
        }
        println!("Refill: executing refill...");
        let mut quit_command_received = false;
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
        let refill_increment = sleep_interval_seconds * config.refill_pump_flow;

        loop {
            spin_sleeper.sleep(sleep_duration_hundred_milli_sec);
            refill_time_seconds += sleep_interval_seconds;
            tank_level_switch_signals.update_tank_level_switch_signals(&tx_to_gpio, rx_from_gpio);
            if tank_level_switch_signals.tank_level_switch_position
                && additional_refill_volume_executed > config.additional_refill_volume
            {
                // water level changed to high
                break;
            }
            if tank_level_switch_signals.tank_level_switch_invalid {
                // switch signal is invalid
                refill_error_code = 1;
                break;
            }

            // calculate volume refilled in this operation
            refill_volume += refill_increment;
            if tank_level_switch_signals.tank_level_switch_position {
                additional_refill_volume_executed += refill_increment;
                println!(
                    "Executing additional refill at {}L, limit at {}L",
                    additional_refill_volume_executed, config.additional_refill_volume
                );
            }
            quit_command_received =
                check_pending_external_request(&rx_from_signal_handler, refill_errors);
            if quit_command_received {
                break;
            }
            println!(
                "Refill: current refill volume at {}L, limit={}L",
                refill_volume, config.max_refill_volume
            );
            if refill_volume > config.max_refill_volume {
                refill_errors.error_switch_stuck_low = true;
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

        // inform data logger that refill is finished
        match tx_to_datalogger.send(false) {
            Ok(_) => { /* no listening to response */ }
            Err(e) => {
                println!(
                "Refill: Error in channel communication to DataLogger when finishing refill: {e:?}"
            );
            }
        }

        // write result to data base
        match sql_interface_refill.insert_refill_event(
            Local::now().naive_local(),
            refill_time_seconds,
            refill_volume,
            refill_error_code,
        ) {
            Ok(_) => {}
            Err(e) => {
                println!(
                "Refill: error occurred when writing refill operation data to SQL database: {e:?}"
            );
                refill_errors.error_sql_update_failed = true;
            }
        }
        quit_command_received
    }
}

/// Contains the communication between Refill thread and GpioHandler thread via channels.
/// The struct holds attributes for the results and for error flags indicating unsuccessful communication.
pub struct TankLevelSwitchSignals {
    tank_level_switch_position: bool,
    tank_level_switch_invalid: bool,
    error_receive_tank_level_switch_position: bool,
    error_receive_tank_level_switch_invalid: bool,
}

impl TankLevelSwitchSignals {
    /// Executes the channel communication to update the tank level switch position signal
    /// as well as the tank level switch invalid signal.
    /// # Arguments
    /// * `tx_to_gpio` - sender part of the channel for communication to GPIO handler
    /// * `rx_from_gpio` - receiver part of the channel for communication from GPIO handler
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
                        self.error_receive_tank_level_switch_position = false;
                        c > 0.0
                    }
                    Err(e) => {
                        println!(
                            "Refill: Error in update_tank_level_switch_signals \
                            when reading channel content for tank_level_switch_position: {e:?}"
                        );
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
                    Ok(c) => {
                        self.error_receive_tank_level_switch_invalid = false;
                        c > 0.0
                    }
                    Err(e) => {
                        println!(
                            "Refill: Error in update_tank_level_switch_signals \
                            when reading channel content for tank_level_switch_invalid: {e:?}"
                        );
                        self.error_receive_tank_level_switch_invalid = true;
                        true
                    }
                };
            }
            Err(e) => {
                println!("Refill: Error in channel communication to GPIO handler when requesting tank level switch invalid: {e:?}");
            }
        }

        println!(
            "Refill: update_tank_level_switch_signals set struct variables \
                        tank_level_switch_position={} and tank_level_switch_invalid={}",
            match self.tank_level_switch_position {
                true => "high",
                false => "low",
            },
            match self.tank_level_switch_invalid {
                true => "invalid",
                false => "valid",
            }
        );
    }
}

/// Contains the error flags for the refill component.
pub struct RefillErrors {
    error_switch_stuck_high: bool,
    error_switch_stuck_low: bool,
    error_receive_tank_level_switch_position: bool,
    error_sql_update_failed: bool,
}

/// Contains the configuration and the implementation for the refill control.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// Architecture of the component is as follows:
/// ```mermaid
/// graph LR
/// execute --> calc_tank_level_switch_position_stabilized;
/// execute --> get_sql_refill_data;
/// execute_refill --> check_pending_external_request;
/// execute --> check_error_condition_switch_stuck_high;
/// execute --> check_pending_external_request;
/// execute --> update_tank_level_switch_signals;
/// execute_refill --> update_tank_level_switch_signals;
/// execute --> check_volume_count_time_limits;
/// execute --> execute_refill;
/// ```
pub struct Refill {
    config: RefillConfig,
    sql_interface_refill: SqlInterfaceRefill,
    tank_level_switch_position_stabilized: bool,
    tank_level_switch_position_high_stabilization_counter: u32,
    refill_errors: RefillErrors,
}

/// Contains the data retrieved from SQL database.
/// It is also used to communicate data to be updated in SQL database.
struct RefillSqlData {
    duration_since_last_refill: i64,
    refill_volume_last_24h: f64,
    refill_count_last_24h: i64,
    refill_volume_last_hour: f64,
    refill_count_last_hour: i64,
}

impl fmt::Display for RefillSqlData {
    /// Function provides output of the struct values for debugging purposes.
    /// # Arguments
    /// * `f` - configuration for formatting    
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            " Refill: Duration since last refill is {} seconds.\n \
            Refill: volume of last 24h is {} L.\n Refill: Count of last 24h is {}.\n \
            Refill: volume of last hour is {} L.\n Refill: Count of last hour is {}.",
            self.duration_since_last_refill,
            self.refill_volume_last_24h,
            self.refill_count_last_24h,
            self.refill_volume_last_hour,
            self.refill_count_last_hour
        )
    }
}

impl Refill {
    /// Provides a struct of type Refill.
    /// # Arguments
    /// * `config` - configuration data for refill control
    /// * `sql_interface_refill` - specific SQL interface for refill
    pub fn new(config: RefillConfig, sql_interface_refill: SqlInterfaceRefill) -> Refill {
        Self {
            config: config,
            sql_interface_refill: sql_interface_refill,
            tank_level_switch_position_stabilized: true,
            tank_level_switch_position_high_stabilization_counter: 0,
            refill_errors: RefillErrors {
                error_switch_stuck_high: false,
                error_switch_stuck_low: false,
                error_receive_tank_level_switch_position: false,
                error_sql_update_failed: false,
            },
        }
    }

    /// Calculates a stabilized tank level switch position signal considering:
    /// - if invalid bit of signal is true, then use the current value of the stabilized position signal.
    /// - if the new signal is true (indicating high water level), then set the stabilized signal immediately to true (high).
    /// - if the the new signal is false (indicating low water level), then set increase the stabilization counter.
    /// - if the stabiliyation counter exceeds the configured limit, then set the counter to zero and set the stabilized
    /// signal to true indicating high water level.
    /// # Arguments
    /// * `tank_level_switch_signals` - struct containing the refill-relevant signal data
    fn calc_tank_level_switch_position_stabilized(
        &mut self,
        tank_level_switch_signals: &TankLevelSwitchSignals,
    ) {
        if !tank_level_switch_signals.tank_level_switch_invalid {
            // update stabilized signal only when new signal is not invalid
            self.tank_level_switch_position_stabilized =
                match tank_level_switch_signals.tank_level_switch_position {
                    true => {
                        self.tank_level_switch_position_high_stabilization_counter = 0;
                        true // set stabilized to true without delay
                    }
                    false => {
                        if self.tank_level_switch_position_high_stabilization_counter
                            < self
                                .config
                                .tank_level_switch_position_high_stabilization_count
                        {
                            self.tank_level_switch_position_high_stabilization_counter += 1;
                            true
                        } else {
                            false
                        }
                    }
                }
        }
    }

    /// Retrieves the refill-related information from SQL database:
    /// - duration since last refill in seconds
    /// - volume refilled in last 24h
    /// - number of refill operations in last 24h
    /// - volume refilled in the last hour
    /// - number of refill operations in the last hour
    fn get_sql_refill_data(&mut self) -> RefillSqlData {
        let duration_since_last_refill = match self.sql_interface_refill.check_empty() {
            Ok(0) => self.config.min_interval_last_refill + 1,
            Ok(_) => match self.sql_interface_refill.get_duration_since_last_refill() {
                Ok(c) => c,
                Err(e) => {
                    panic!("Could not get duration since last refill event: {e:?}");
                }
            },
            Err(e) => {
                panic!("Could not check if refill table is empty or not: {e:?}");
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

    /// Checks if the duration since the last refill operation eceeds a configurable limit.
    /// If yes, then corresponding error flag is set.
    /// # Arguments
    /// * `duration_since_last_refill` - duration in seconds since last refill operation
    fn check_error_condition_switch_stuck_high(&mut self, duration_since_last_refill: i64) {
        self.refill_errors.error_switch_stuck_high =
            duration_since_last_refill > self.config.max_duration_since_last_refill;
    }

    // Checks if historic refill data does not exceed limitations in terms of count and volume.
    // The function returns a tuple of boolean variables.
    /// # Arguments
    /// * `sql_refill_data` - reference to data retrieved from SQL database    
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
                sql_refill_data.duration_since_last_refill, self.config.min_interval_last_refill
            );
        }
        refill_check_volume_last_24h
            | refill_check_count_last_24h
            | refill_check_volume_last_hour
            | refill_check_count_last_hour
            | refill_check_intervall_last_refill
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `mutex_device_scheduler_refill` - clone of the mutex for avoiding parallel device actuation.
    /// * `tx_to_gpio` - Sender part of channel for communication to GPIO handler
    /// * `rx_from_gpio` - Receiver part of channel for communication from GPIO handler
    /// * `tx_to_controllino` - Sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Receiver part of channel for communication from Controllino
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    /// * `tx_to_schedule_check` - Sender part of channel for communication to schedule checker
    /// * `rx_from_schedule_check` - Receiver part of channel for communication from schedule checker               
    /// * `tx_to_data_logger` - Sender part of channel for communication to data logger
    /// * `refill_execution` - Struct implementing the RefillExecutionTrait    
    pub fn execute(
        &mut self,
        mutex_device_scheduler_refill: Arc<Mutex<i32>>,
        tx_to_gpio: Sender<InternalCommand>,
        rx_from_gpio: Receiver<f32>,
        tx_to_controllino: Sender<InternalCommand>,
        rx_from_controllino: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
        tx_to_schedule_check: Sender<InternalCommand>,
        rx_from_schedule_check: Receiver<bool>,
        tx_to_datalogger: Sender<bool>,
        refill_execution: &mut impl RefillExecutionTrait,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let mut i = 0;
        let spin_sleeper = SpinSleeper::default();
        let mut tank_level_switch_signals = TankLevelSwitchSignals {
            tank_level_switch_position: true,
            tank_level_switch_invalid: true,
            error_receive_tank_level_switch_position: false,
            error_receive_tank_level_switch_invalid: false,
        };
        let mut schedule_check_result: bool = false;
        
        // retrieving data from SQL data base
        let mut sql_refill_data = self.get_sql_refill_data();
        println!("{}", sql_refill_data);

        let mut quit_command_received = false;

        // initial waiting time
        if self.config.initial_wait_interval > 0 {
            while i < self.config.initial_wait_interval * 10 {
                i = i + 1;
                quit_command_received = check_pending_external_request(
                    &rx_from_signal_handler,
                    &mut self.refill_errors,
                );
                if quit_command_received {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_hundred_millis);
            }
        }

        // frequent update required for calculation of error condition switch stuck high
        sql_refill_data.duration_since_last_refill =
            sql_refill_data.duration_since_last_refill + self.config.initial_wait_interval;

        // main control loop for refill
        loop {
            // check if either during main control loop or during refill execution termination requests has been received
            if quit_command_received {
                break; // exit outer loop
            }

            if self.config.active {
                // check for external requests while waiting for the check interval to pass
                println!("Refill: executing main loop.");

                // frequent update required for calculation of error condition switch stuck high
                sql_refill_data.duration_since_last_refill =
                    sql_refill_data.duration_since_last_refill + self.config.check_interval;

                // check when the last refill operation has taken place
                // if too much time has passed, issue an error condition - not impeding rest of operation
                self.check_error_condition_switch_stuck_high(
                    sql_refill_data.duration_since_last_refill,
                );

                // either communicate with real HW or with simulator to get updated signal information
                // this call will also update the stabilized tank level position.
                tank_level_switch_signals
                    .update_tank_level_switch_signals(&tx_to_gpio, &rx_from_gpio);

                self.calc_tank_level_switch_position_stabilized(&tank_level_switch_signals);

                println!(
                    "self.tank_level_switch_position_stabilized={}",
                    self.tank_level_switch_position_stabilized
                );
                println!(
                    "tank_level_switch_signals.tank_level_switch_invalid={}",
                    tank_level_switch_signals.tank_level_switch_invalid
                );
                if !quit_command_received
                    && !self.tank_level_switch_position_stabilized
                    && !tank_level_switch_signals.tank_level_switch_invalid
                {
                    println!(
                        "Refill: Stabilized tank level switch position indicates low water level."
                    );
                    sql_refill_data = self.get_sql_refill_data();

                    // execute checks before initiating refill
                    if !self.check_volume_count_time_limits(&sql_refill_data) {
                        if !self.refill_errors.error_switch_stuck_low
                            && !self.refill_errors.error_sql_update_failed
                        {
                            // schedule check to see if actuation is allowed
                            println!("Refill: initiating communication to schedule checker.");
                            match tx_to_schedule_check.send(InternalCommand::ScheduleCheck) {
                                Ok(_) => {
                                    // now listen for response
                                    schedule_check_result = match rx_from_schedule_check.recv() {
                                        Ok(c) => c,
                                        Err(e) => {
                                            println!(
                                                "Refill: Error when reading channel content for schedule check: {e:?}"
                                            );
                                            false // assume we are not allowed to actuate when communication is not successful
                                        }
                                    };
                                }
                                Err(e) => {
                                    println!(
                                        "Refill: Error in channel communication to schedule check: {e:?}"
                                    );
                                }
                            }                            
                            println!("Refill: finished communication to schedule checker.");
                                        
                            if schedule_check_result {
                                // conditions for allowing refill operation are given
                                println!("Refill: Executing refill.");
                                // inner scope to limit lifetime of unlocked mutex
                                {
                                    let mut mutex_data = mutex_device_scheduler_refill.lock().unwrap();
                                    // now we are given exclusive access to actuators
                                    quit_command_received = refill_execution.execute_refill(
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                        &tx_to_gpio,
                                        &rx_from_gpio,
                                        &rx_from_signal_handler,
                                        &tx_to_datalogger,
                                        &mut tank_level_switch_signals,
                                        &mut self.refill_errors,
                                        &self.config,
                                        &mut self.sql_interface_refill,
                                    );
                                    *mutex_data += 1;
                                }
                            }
                        }
                    }
                }
            }

            // wait for configured time period before rechecking the sensor signal
            i = 0;
            while i < self.config.check_interval * 10 {
                i = i + 1;
                quit_command_received = check_pending_external_request(
                    &rx_from_signal_handler,
                    &mut self.refill_errors,
                );
                if quit_command_received {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_hundred_millis);
            }
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        println!("Refill: Sending Quit confirmation to signal handler.");
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                println!(
                    "Refill: Error in trying to send quit confirmation back to signal handler: {e:?}"
                );
            }
        };

        // this thread has channel connection to underlying threads (Controllino, GpioHandler).
        // These threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by signal_handler crate.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match rx_from_signal_handler.recv() {
                Ok(c) => match c {
                    InternalCommand::Terminate => true,
                    _ => {
                        println!("Refill: ignoring inapplicable command from signal handler.");
                        false
                    }
                },
                Err(e) => {
                    println!("Refill: error when trying to receive termination signal: {e:?}");
                    false
                }
            };
            spin_sleeper.sleep(sleep_duration_hundred_millis);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use chrono::DateTime;
    use chrono::Local;
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::channel_content::AquariumSignal;
    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::refill::check_pending_external_request;
    use crate::refill::Refill;
    use crate::refill::RefillConfig;
    use crate::refill::RefillErrors;
    use crate::refill::RefillExecution;
    use crate::refill::RefillExecutionTrait;
    use crate::refill::TankLevelSwitchSignals;
    use crate::sql_interface::SqlInterface;
    use crate::sql_interface_refill::SqlInterfaceRefill;

    trait Output {
        fn output(&self) -> String;
    }

    impl Output for SystemTime {
        fn output(&self) -> String {
            let datetime: DateTime<Local> = (*self).into();
            let duration_since_epoch = self
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards");
            datetime.format("%d/%m/%Y %T").to_string()
                + " "
                + &duration_since_epoch.as_millis().to_string()
        }
    }
    
    // send via channel a value to the test object to indicate if actuation is permitted.
    fn mock_schedule_check(
        tx_schedule_check_to_refill: &Sender<bool>,
        rx_schedule_check_from_refill: &Receiver<InternalCommand>,
        value: bool
    ) -> bool {
        match rx_schedule_check_from_refill.try_recv() {
            Ok(c) => match c {
                InternalCommand::ScheduleCheck => {
                    // allow control to actuate
                    let _ = tx_schedule_check_to_refill.send(value);
                }
                InternalCommand::Quit => {
                    // test case has finished
                    return true;
                }
                _ => {
                    panic!("Refill: test object sent invalid command.");
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        println!(
                            "Refill: Error in trying to receive command from test object: {e:?}"
                        );
                    }
                }
            }
        }
        false
    }

    fn prepare_refill_tests() -> (ConfigData, Refill, SqlInterface) {
        // prepare struct Refill for subsequent tests
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_refill.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_refill = SqlInterfaceRefill::new(sql_interface.get_connection());
        let refill_config = config.refill.clone();
        (
            config,
            Refill::new(refill_config, sql_interface_refill),
            sql_interface,
        )
    }

    fn prepare_refill_tests_secondary() -> (ConfigData, Refill, SqlInterface) {
        // prepare struct Refill for subsequent tests
        // parallel usage of database leads to conflicts
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_refill_secondary.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_refill = SqlInterfaceRefill::new(sql_interface.get_connection());
        let refill_config = config.refill.clone();
        (
            config,
            Refill::new(refill_config, sql_interface_refill),
            sql_interface,
        )
    }

    fn prepare_refill_tests_tertiary() -> (ConfigData, Refill, SqlInterface) {
        // prepare struct Refill for subsequent tests
        // parallel usage of database leads to conflicts
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_refill_tertiary.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_refill = SqlInterfaceRefill::new(sql_interface.get_connection());
        let refill_config = config.refill.clone();
        (
            config,
            Refill::new(refill_config, sql_interface_refill),
            sql_interface,
        )
    }

    struct RefillTestExecutionVoid;

    impl RefillExecutionTrait for RefillTestExecutionVoid {
        fn execute_refill(
            &mut self,
            _tx_to_controllino: &Sender<InternalCommand>,
            _rx_from_controllino: &Receiver<bool>,
            _tx_to_gpio: &Sender<InternalCommand>,
            _rx_from_gpio: &Receiver<f32>,
            _rx_from_signal_handler: &Receiver<InternalCommand>,
            _tx_to_datalogger: &Sender<bool>,
            _tank_level_switch_signals: &mut TankLevelSwitchSignals,
            _refill_errors: &mut RefillErrors,
            _config: &RefillConfig,
            _sql_interface_refill: &mut SqlInterfaceRefill,
        ) -> bool {
            // do nothing
            println!("Refill: execute_refill *VOID* has been called.");
            false // simulate that no Quit command has been received
        }
    }

    #[test]
    pub fn test_refill_calc_tank_level_switch_position_stabilized_inhibition_false_true_when_invalid(
    ) {
        //*** test if position signal is not updating when invalid signal is true ***
        let (_, mut refill, _) = prepare_refill_tests();
        // check transition false -> true
        let tank_level_switch_signals = TankLevelSwitchSignals {
            tank_level_switch_position: true,
            tank_level_switch_invalid: true,
            error_receive_tank_level_switch_position: false,
            error_receive_tank_level_switch_invalid: false,
        };
        refill.tank_level_switch_position_stabilized = false;
        for _ in 0..10 {
            refill.calc_tank_level_switch_position_stabilized(&tank_level_switch_signals);
            assert_eq!(refill.tank_level_switch_position_stabilized, false);
        }
        println!("* [Refill] checking inhibition of change false -> true for stabilized tank level position when invalid bit is true succeeded.");
    }

    #[test]
    pub fn test_refill_calc_tank_level_switch_position_stabilized_inhibition_true_false_when_invalid(
    ) {
        //*** test if position signal is not updating when invalid signal is true ***
        let (_, mut refill, _) = prepare_refill_tests();
        // check transition true -> false
        let tank_level_switch_signals = TankLevelSwitchSignals {
            tank_level_switch_position: true,
            tank_level_switch_invalid: true,
            error_receive_tank_level_switch_position: false,
            error_receive_tank_level_switch_invalid: false,
        };
        refill.tank_level_switch_position_stabilized = true;
        for _ in 0..10 {
            refill.calc_tank_level_switch_position_stabilized(&tank_level_switch_signals);
            assert_eq!(refill.tank_level_switch_position_stabilized, true);
        }
        println!("* [Refill] checking inhibition of change true -> false for stabilized tank level position when invalid bit is true succeeded.");
    }

    #[test]
    pub fn test_refill_calc_tank_level_switch_position_stabilized_false_true() {
        //*** test if position signal is updating without delay when invalid signal is false ***
        let (_, mut refill, _) = prepare_refill_tests();
        // check transition true -> false
        let tank_level_switch_signals = TankLevelSwitchSignals {
            tank_level_switch_position: true,
            tank_level_switch_invalid: false,
            error_receive_tank_level_switch_position: false,
            error_receive_tank_level_switch_invalid: false,
        };
        refill.tank_level_switch_position_stabilized = false;
        refill.calc_tank_level_switch_position_stabilized(&tank_level_switch_signals);
        assert_eq!(refill.tank_level_switch_position_stabilized, true);
        println!("* [Refill] checking immediate change false -> true for stabilized tank level position when invalid bit is false succeeded.");
    }

    #[test]
    pub fn test_refill_calc_tank_level_switch_position_stabilized_true_false() {
        //*** test if position signal is updating with delay when invalid signal is false ***
        let (config, mut refill, _) = prepare_refill_tests();
        // check transition true -> false
        let tank_level_switch_signals = TankLevelSwitchSignals {
            tank_level_switch_position: false,
            tank_level_switch_invalid: false,
            error_receive_tank_level_switch_position: false,
            error_receive_tank_level_switch_invalid: false,
        };
        refill.tank_level_switch_position_stabilized = true;
        for _ in 0..config
            .refill
            .tank_level_switch_position_high_stabilization_count
        {
            refill.calc_tank_level_switch_position_stabilized(&tank_level_switch_signals);
            assert_eq!(refill.tank_level_switch_position_stabilized, true);
        }
        refill.calc_tank_level_switch_position_stabilized(&tank_level_switch_signals);
        assert_eq!(refill.tank_level_switch_position_stabilized, false);
        println!("* [Refill] checking delayed change true -> false for stabilized tank level position when invalid bit is false succeeded.");
    }

    #[test]
    pub fn test_refill_calc_tank_level_switch_position_stabilized_toggle_input() {
        //*** test if position signal is updating with delay when invalid signal is false ***
        let (config, mut refill, _) = prepare_refill_tests();
        // check transition true -> false
        let mut tank_level_switch_signals = TankLevelSwitchSignals {
            tank_level_switch_position: false,
            tank_level_switch_invalid: false,
            error_receive_tank_level_switch_position: false,
            error_receive_tank_level_switch_invalid: false,
        };
        refill.tank_level_switch_position_stabilized = true;
        for i in 0..config
            .refill
            .tank_level_switch_position_high_stabilization_count
            * 4
        {
            // toggle the input
            tank_level_switch_signals.tank_level_switch_position = (i % 2) > 0;
            refill.calc_tank_level_switch_position_stabilized(&tank_level_switch_signals);
            assert_eq!(refill.tank_level_switch_position_stabilized, true);
        }
        println!("* [Refill] checking no change true -> false for stabilized tank level position when toggling input succeeded.");
    }

    #[test]
    pub fn test_refill_check_pending_external_request_empty_channel() {
        let mut refill_errors = RefillErrors {
            error_switch_stuck_high: false,
            error_switch_stuck_low: false,
            error_receive_tank_level_switch_position: false,
            error_sql_update_failed: false,
        };
        let (_, rx_refill_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();

        assert_eq!(
            check_pending_external_request(&rx_refill_from_signal_handler, &mut refill_errors,),
            false
        );

        println!("* [Refill] checking false result of check_pending_external_request when channel is empty succeeded.");
    }

    #[test]
    // this test case checks the .check_pending_external_request method if it correctly processes the ResetAllErrors command
    // the test case checks the struct attribute after having executed the function
    pub fn test_refill_check_pending_external_request_reset_all_errors_command() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();

        let mut refill_errors = RefillErrors {
            error_switch_stuck_high: true,
            error_switch_stuck_low: true,
            error_receive_tank_level_switch_position: true,
            error_sql_update_failed: true,
        };

        scope(|scope| {            
            scope.spawn(move || {
                let _ = tx_signal_handler_to_refill.send(InternalCommand::ResetAllErrors);
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);
            scope.spawn(move || {
                assert_eq!(
                    check_pending_external_request(
                        &rx_refill_from_signal_handler,
                        &mut refill_errors,
                        ),
                    false);
                println!("* checking true result of check_pending_external_request when channel contains Quit command succeeded.");
                assert_eq!(refill_errors.error_switch_stuck_high, false);
                assert_eq!(refill_errors.error_switch_stuck_low, false);
                assert_eq!(refill_errors.error_receive_tank_level_switch_position, false);
                assert_eq!(refill_errors.error_sql_update_failed, false);
                println!("* checking reset of all error flags succeeded.");
            });
        });
    }

    #[test]
    // this test case checks the .check_pending_external_request method if it correctly processes the Quit command
    pub fn test_refill_check_pending_external_request_quit_command() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let mut refill_errors = RefillErrors {
            error_switch_stuck_high: true,
            error_switch_stuck_low: true,
            error_receive_tank_level_switch_position: true,
            error_sql_update_failed: true,
        };

        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            scope.spawn(move || {
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);
            scope.spawn(move || {
                assert_eq!(
                    check_pending_external_request(
                        &rx_refill_from_signal_handler,
                        &mut refill_errors,
                    ), true);
                println!("* [Refill] checking true result of check_pending_external_request when channel contains Quit command succeeded.");
            });
        });
    }

    #[test]
    // this test case checks if the .execute functions terminates without delays
    // when a quit/termination request is sent after starting the function
    pub fn test_refill_execute_with_pending_quit_terminate_command() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        // channels for the test object. test environment only uses one of the channels
        let (_, mut refill, _) = prepare_refill_tests();
        let (tx_refill_to_gpio, _) = mpsc::channel();
        let (_, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_datalogger, _) = mpsc::channel();
        let (tx_refill_to_controllino, _) = mpsc::channel();
        let (_, rx_refill_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_refill_to_signal_handler, _) = mpsc::channel();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) = mpsc::channel();

        scope(|scope| {
            let mutex_device_scheduler = Arc::new(Mutex::new(0));

            let start_time = SystemTime::now();
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });
            
            // thread for rest of test environmane
            scope.spawn(move || {
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                spin_sleeper.sleep(sleep_duration_100_millisec);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);
            });
            
            spin_sleeper.sleep(sleep_duration_100_millisec);
            
            // thread for test object
            scope.spawn(move || {
                let mut refill_execution = RefillExecution;
                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();
                    
                refill.execute(
                    mutex_device_scheduler.clone(),
                    tx_refill_to_gpio,
                    rx_refill_from_gpio,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_datalogger,
                    &mut refill_execution,
                );
                
                let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                
                // check if no refill operation has been executed
                let mutex_data = mutex_device_scheduler.lock().unwrap();
                assert_eq!(*mutex_data, 0);
            });
            let finish_time = SystemTime::now();
            let execution_duration = finish_time.duration_since(start_time).unwrap();
            all_asserts::assert_le!(execution_duration.as_millis(), 500);
            println!(
                "* [Refill] checking execution with pending quit/terminate command (time limit) succeeded."
            );
        });
    }

    // helper function for test case test_update_tank_level_switch_signals
    // the test case uses different combination of stimuli in each iteration of the loop
    // this function provides the stimuli depending on the index of the iteration
    fn response_values_for_update_tank_level_switch_signals(index: i32) -> (bool, bool) {
        match index {
            0 => (false, false),
            1 => (false, true),
            2 => (true, false),
            3 => (true, true),
            _ => {
                panic!("response_values_for_update_tank_level_switch_signals called with incorrect parameter.");
            }
        }
    }

    #[test]
    // this test case checks the function update_tank_level_switch_signals
    // the test case provides values via channel and checks if the test object sets the
    // attributes correctly.
    pub fn test_update_tank_level_switch_signals() {
        let sleep_duration_200_millisec = Duration::from_millis(200);
        let spin_sleeper = SpinSleeper::default();
        let mut tank_level_switch_signals = TankLevelSwitchSignals {
            tank_level_switch_position: false,
            tank_level_switch_invalid: false,
            error_receive_tank_level_switch_position: false,
            error_receive_tank_level_switch_invalid: false,
        };
        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        scope(|scope| {
            // thread for test environment
            scope.spawn(move || {
                for i in 0..4 {
                    let (target_response_switch_position, target_response_switch_invalid) =
                        response_values_for_update_tank_level_switch_signals(i);
                    for _ in 0..2 {
                        let response_to_be_sent_back = match rx_gpio_from_refill.recv().unwrap() {
                            InternalCommand::RequestSignal(s) => match s {
                                AquariumSignal::TankLevelSwitchPosition => {
                                    match target_response_switch_position {
                                        false => 0.0,
                                        true => 1.0,
                                    }
                                }
                                AquariumSignal::TankLevelSwitchInvalid => {
                                    match target_response_switch_invalid {
                                        false => 0.0,
                                        true => 1.0,
                                    }
                                }
                                _ => panic!(
                                    "update_tank_level_switch_signals requested incorrect signal."
                                ),
                            },
                            _ => panic!("update_tank_level_switch_signals issued incorrect command."),
                        };
                        let _ = tx_gpio_to_refill.send(response_to_be_sent_back);
                        println!(
                            "Refill: test_update_tank_level_switch_signals - sending back {} to application.",
                            response_to_be_sent_back);
                    }
                }
            });
            spin_sleeper.sleep(sleep_duration_200_millisec);

            // thread for test object
            scope.spawn(move || {
                for i in 0..4 {
                    tank_level_switch_signals
                        .update_tank_level_switch_signals(&tx_refill_to_gpio, &rx_refill_from_gpio);
                    let (target_response_switch_position, target_response_switch_invalid) =
                        response_values_for_update_tank_level_switch_signals(i);
                    println!(
                        "... testing update_tank_level_switch_signals with expected values \
                        target_response_switch_position={} and target_response_switch_invalid={}",
                        match target_response_switch_position {
                            true => "high",
                            false => "low",
                        },
                        match target_response_switch_invalid {
                            true => "invalid",
                            false => "valid",
                        }
                    );
                    assert_eq!(
                        tank_level_switch_signals.tank_level_switch_position,
                        target_response_switch_position
                    );
                    assert_eq!(
                        tank_level_switch_signals.tank_level_switch_invalid,
                        target_response_switch_invalid
                    );
                    spin_sleeper.sleep(sleep_duration_200_millisec);
                }
            });
        });
    }

    #[test]
    // this test case executes one iteration of the loop of the .execute function without triggering a refill
    // it checks the timing of initial wait period and main control interval
    pub fn test_execute_without_refill() {
        let sleep_duration_50_millisec = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (config, mut refill, _) = prepare_refill_tests();
        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, _) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, _) = mpsc::channel();
        let (_, rx_refill_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) = mpsc::channel();
        let mutex_device_scheduler = Arc::new(Mutex::new(0));

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_50_millisec);
            });
            
            // thread for rest of test environment
            scope.spawn(move || {
                let start_time = SystemTime::now();
                println!(
                    "test_execute_without_refill: start_time={}",
                    start_time.output()
                );
                // when reaching main control cycle, test object will request signal updates
                rx_gpio_from_refill.recv().unwrap();
                // record time after test object sent command via channel
                let initial_waiting_passed_time = SystemTime::now();
                println!(
                    "test_execute_without_refill: initial_waiting_passed_time={}",
                    initial_waiting_passed_time.output()
                );
                let _ = tx_gpio_to_refill.send(1.0);
                rx_gpio_from_refill.recv().unwrap();
                let _ = tx_gpio_to_refill.send(0.0);
                // from here onwards, test object will continue through main cycle and
                // request signal updates one more time
                rx_gpio_from_refill.recv().unwrap();
                let _ = tx_gpio_to_refill.send(1.0);
                rx_gpio_from_refill.recv().unwrap();
                let _ = tx_gpio_to_refill.send(0.0);
                // now we can send the quit command                
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);
                let finish_time = SystemTime::now();
                println!(
                    "test_execute_without_refill: finish_time={}",
                    finish_time.output()
                );
                let initial_wait_duration_millis = initial_waiting_passed_time
                    .duration_since(start_time)
                    .unwrap()
                    .as_millis();
                let check_interval_duration_millis = finish_time
                    .duration_since(initial_waiting_passed_time)
                    .unwrap()
                    .as_millis();
                println!(
                    "test_execute_without_refill: initial_wait_duration={}ms, check_interval_duration={}ms",
                    initial_wait_duration_millis,
                    check_interval_duration_millis
                );
                all_asserts::assert_le!(
                    initial_wait_duration_millis,
                    (config.refill.initial_wait_interval * 1000 + 200) as u128
                );
                all_asserts::assert_ge!(
                    initial_wait_duration_millis,
                    (config.refill.initial_wait_interval * 1000 - 100) as u128
                );
                all_asserts::assert_le!(
                    check_interval_duration_millis,
                    (config.refill.check_interval * 1000 + 200) as u128
                );
                all_asserts::assert_ge!(
                    check_interval_duration_millis,
                    (config.refill.check_interval * 1000 - 100) as u128
                );
            });
            // give the test environment a little time to trigger the receive call for signal update
            spin_sleeper.sleep(sleep_duration_50_millisec);

            // thread for test object
            scope.spawn(move || {
                let mut refill_test_execution_void = RefillTestExecutionVoid;
                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler.clone(),
                    tx_refill_to_gpio,
                    rx_refill_from_gpio,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_datalogger,
                    &mut refill_test_execution_void,
                );
                
                let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                
                // check if no refill operation has been executed
                let mutex_data = mutex_device_scheduler.lock().unwrap();
                assert_eq!(*mutex_data, 0);
            });
        });
    }

    fn run_main_cycle_with_sensor_signals(
        iterations: u32,
        rx_gpio_from_refill: &Receiver<InternalCommand>,
        tx_gpio_to_refill: &Sender<f32>,
        signal_position: bool,
        signal_invalid: bool,
    ) {
        for i in 0..iterations {
            println!(
                "Refill: run_main_cycle_with_sensor_signals running loop iteration #{}",
                i
            );
            rx_gpio_from_refill.recv().unwrap();
            let _ = tx_gpio_to_refill.send(match signal_position {
                true => 1.0,
                false => 0.0,
            });
            rx_gpio_from_refill.recv().unwrap();
            let _ = tx_gpio_to_refill.send(match signal_invalid {
                true => 1.0,
                false => 0.0,
            });
        }
    }

    #[test]
    // this test case executes multiple iterations of the loop of the .execute function manipulating the sensor signals only
    // it checks if the test object correctly processes the conditions for refill execution
    pub fn test_execute_conditions_sensor_signals() {
        let sleep_duration_50_millisec = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill, mut sql_interface) = prepare_refill_tests_secondary();
        // accelerate test case execution
        refill
            .config
            .tank_level_switch_position_high_stabilization_count = 2;
        refill.config.check_interval = 1;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, _) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, _) = mpsc::channel();
        let (_, rx_refill_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) = mpsc::channel();
        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_device_scheduler_environment = mutex_device_scheduler.clone();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_50_millisec);
            });
            
            // thread for rest of test environment
            scope.spawn(move || {
                let execution_count1: i32;
                let execution_count2: i32;
                let execution_count3: i32;

                // when reaching main control cycle, test object will request signal updates
                // test 1: answer with tank level switch high and valid
                run_main_cycle_with_sensor_signals(
                    refill
                        .config
                        .tank_level_switch_position_high_stabilization_count
                        * 2,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    true,
                    false,
                );
                {
                    execution_count1 = *mutex_device_scheduler_environment.lock().unwrap();
                }
                // test 2: answer with tank level switch low and invalid
                run_main_cycle_with_sensor_signals(
                    refill
                        .config
                        .tank_level_switch_position_high_stabilization_count
                        * 2,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    true,
                    false,
                );
                {
                    execution_count2 = *mutex_device_scheduler_environment.lock().unwrap();
                }
                // test 3: answer with tank level switch low and valid
                run_main_cycle_with_sensor_signals(
                    refill
                        .config
                        .tank_level_switch_position_high_stabilization_count
                        * 2,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );
                {
                    execution_count3 = *mutex_device_scheduler_environment.lock().unwrap();
                }

                // now we can send the quit command
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);

                // test object thread will end. We can assert now.
                assert_eq!(execution_count1, 0); // check if no refill operation has been executed
                assert_eq!(execution_count2, 0); // check if no refill operation has been executed
                assert_eq!(execution_count3, 1); // check if one refill operation has been executed
            });
            // give the test environment a little time to trigger the receive call for signal update
            spin_sleeper.sleep(sleep_duration_50_millisec);

            // thread for test object
            scope.spawn(move || {
                let mut refill_test_execution_void = RefillTestExecutionVoid;
                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler.clone(),
                    tx_refill_to_gpio,
                    rx_refill_from_gpio,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_datalogger,
                    &mut refill_test_execution_void,
                );
                
                let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);                
            });
        });
    }

    #[test]
    // this test case executes multiple iterations of the loop of the .execute function with error flag error_switch_stuck_low set
    pub fn test_execute_conditions_historic_data_error_switch_stuck_low() {
        let sleep_duration_50_millisec = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill, mut sql_interface) = prepare_refill_tests_secondary();
        // accelerate test case execution
        refill
            .config
            .tank_level_switch_position_high_stabilization_count = 2;
        refill.config.check_interval = 1;
        refill.refill_errors.error_switch_stuck_low = true;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, _) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, _) = mpsc::channel();
        let (_, rx_refill_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) = mpsc::channel();

        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_device_scheduler_environment = mutex_device_scheduler.clone();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Refill: test_execute_conditions_historic_data could not prepare test case: {e:?}")
            }
        }

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_50_millisec);
            });
            
            // thread for rest of test environment
            scope.spawn(move || {
                let execution_count1: i32;
                let execution_count2: i32;

                // when reaching main control cycle, test object will request signal updates
                // this test will run with error flag enabled
                println!("Refill test_execute_conditions_historic_data_error_switch_stuck_low: running check #1");
                run_main_cycle_with_sensor_signals(
                    refill
                        .config
                        .tank_level_switch_position_high_stabilization_count + 2,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );
                // give test object some time to process the signals sent via channels
                spin_sleeper.sleep(sleep_duration_50_millisec);
                {
                    execution_count1 = *mutex_device_scheduler_environment.lock().unwrap();
                }

                let _ = tx_signal_handler_to_refill.send(InternalCommand::ResetAllErrors);

                // no error conditions - trigger refill execution
                println!("Refill test_execute_conditions_historic_data_error_switch_stuck_low: running check #2");
                run_main_cycle_with_sensor_signals(
                    1,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );
                // give test object some time to process the signals sent via channels
                spin_sleeper.sleep(sleep_duration_50_millisec);
                {
                    execution_count2 = *mutex_device_scheduler_environment.lock().unwrap();
                }

                // now we can send the quit command
                println!("test_execute_conditions_historic_data_error_switch_stuck_low: sending quit command to test object.");
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);

                // test object thread will end. We can assert now.
                assert_eq!(execution_count1, 0); // check if no refill operation has been executed
                assert_eq!(execution_count2, 1); // check if one refill operation has been executed
            });
            // give the test environment a little time to trigger the receive call for signal update
            spin_sleeper.sleep(sleep_duration_50_millisec);

            // thread for test object
            scope.spawn(move || {
                let mut refill_test_execution_void = RefillTestExecutionVoid;
                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler.clone(),
                    tx_refill_to_gpio,
                    rx_refill_from_gpio,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_datalogger,
                    &mut refill_test_execution_void,
                );
                println!("test_execute_conditions_historic_data_error_switch_stuck_low: test object finished execution.");
                
                let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);                                
            });
        });
    }

    #[test]
    // this test case executes multiple iterations of the loop of the .execute function with error flag error_sql_update_failed
    pub fn test_execute_conditions_historic_data_error_sql_update_failed() {
        let sleep_duration_50_millisec = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill, mut sql_interface) = prepare_refill_tests();
        // accelerate test case execution
        refill
            .config
            .tank_level_switch_position_high_stabilization_count = 2;
        refill.config.check_interval = 1;
        refill.refill_errors.error_sql_update_failed = true;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, _) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, _) = mpsc::channel();
        let (_, rx_refill_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) = mpsc::channel();

        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_device_scheduler_environment = mutex_device_scheduler.clone();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Refill: test_execute_conditions_historic_data could not prepare test case: {e:?}")
            }
        }

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_50_millisec);
            });
            
            // thread for rest of test environment
            scope.spawn(move || {
                let execution_count1: i32;
                let execution_count2: i32;

                // when reaching main control cycle, test object will request signal updates
                // this test will run with error flag enabled
                println!("Refill test_execute_conditions_historic_data_error_sql_update_failed: running check #1");
                run_main_cycle_with_sensor_signals(
                    refill
                        .config
                        .tank_level_switch_position_high_stabilization_count + 2,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );
                // give test object some time to process the signals sent via channels
                spin_sleeper.sleep(sleep_duration_50_millisec);
                {
                    execution_count1 = *mutex_device_scheduler_environment.lock().unwrap();
                }

                let _ = tx_signal_handler_to_refill.send(InternalCommand::ResetAllErrors);

                // no error conditions - trigger refill execution
                println!("Refill test_execute_conditions_historic_data_error_sql_update_failed: running check #2");
                run_main_cycle_with_sensor_signals(
                    1,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );
                // give test object some time to process the signals sent via channels
                spin_sleeper.sleep(sleep_duration_50_millisec);
                {
                    execution_count2 = *mutex_device_scheduler_environment.lock().unwrap();
                }

                // now we can send the quit command
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);

                // test object thread will end. We can assert now.
                assert_eq!(execution_count1, 0); // check if no refill operation has been executed
                assert_eq!(execution_count2, 1); // check if one refill operation has been executed
            });
            // give the test environment a little time to trigger the receive call for signal update
            spin_sleeper.sleep(sleep_duration_50_millisec);

            // thread for test object
            scope.spawn(move || {
                let mut refill_test_execution_void = RefillTestExecutionVoid;
                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler.clone(),
                    tx_refill_to_gpio,
                    rx_refill_from_gpio,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_datalogger,
                    &mut refill_test_execution_void,
                );
                
                let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);                
            });
        });
    }

    #[test]
    // this test case executes multiple iterations of the loop of the .execute function with error flag error_sql_update_failed
    pub fn test_execute_conditions_historic_data_error_recent_refill_event() {
        let sleep_duration_50_millisec = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill, mut sql_interface) = prepare_refill_tests();

        // accelerate test case execution
        refill
            .config
            .tank_level_switch_position_high_stabilization_count = 2;
        refill.config.check_interval = 1;
        refill.refill_errors.error_sql_update_failed = true;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, _) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, _) = mpsc::channel();
        let (_, rx_refill_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) = mpsc::channel();

        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_device_scheduler_environment = mutex_device_scheduler.clone();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Refill: test_execute_conditions_historic_data_error_recent_refill_event could not prepare test case: {e:?}")
            }
        }

        // insert a recent refill event into the refill table
        let naive_datetime_some_time_ago = SqlInterface::get_naive_timestamp_from_past(
            0,
            refill.config.min_interval_last_refill - refill.config.check_interval * 2,
        );
        match refill.sql_interface_refill.insert_refill_event(
            naive_datetime_some_time_ago,
            5.0,
            0.25,
            -2,
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!(
                    "Refill: test_execute_conditions_historic_data \
                            Could not insert refill event in data base for test preparation: {e:?}"
                );
            }
        }

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_50_millisec);
            });
            
            // thread for rest of test environment
            scope.spawn(move || {
                let execution_count1: i32;
                let execution_count2: i32;

                // when reaching main control cycle, test object will request signal updates
                // this test will run with error flag enabled
                println!("Refill test_execute_conditions_historic_data_error_recent_refill_event: running check #1");
                run_main_cycle_with_sensor_signals(
                    refill
                        .config
                        .tank_level_switch_position_high_stabilization_count + 2,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );
                // give test object some time to process the signals sent via channels
                spin_sleeper.sleep(sleep_duration_50_millisec);
                {
                    execution_count1 = *mutex_device_scheduler_environment.lock().unwrap();
                }

                let _ = tx_signal_handler_to_refill.send(InternalCommand::ResetAllErrors);

                // empty the refill table
                match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
                    Ok(_) => {}
                    Err(e) => {
                        panic!("Refill: test_execute_conditions_historic_data could not prepare test case: {e:?}")
                    }
                }
                // give SQL database some time to process the command

                spin_sleeper.sleep(sleep_duration_50_millisec);
                // no error conditions - trigger refill execution
                println!("Refill test_execute_conditions_historic_data_error_recent_refill_event: running check #2");
                run_main_cycle_with_sensor_signals(
                    1,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );
                // give test object some time to process the signals sent via channels
                spin_sleeper.sleep(sleep_duration_50_millisec);
                {
                    execution_count2 = *mutex_device_scheduler_environment.lock().unwrap();
                }

                // now we can send the quit command
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);

                // test object thread will end. We can assert now.
                assert_eq!(execution_count1, 0); // check if no refill operation has been executed
                assert_eq!(execution_count2, 1); // check if one refill operation has been executed
            });
            // give the test environment a little time to trigger the receive call for signal update
            spin_sleeper.sleep(sleep_duration_50_millisec);

            // thread for test object
            scope.spawn(move || {
                let mut refill_test_execution_void = RefillTestExecutionVoid;
                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler.clone(),
                    tx_refill_to_gpio,
                    rx_refill_from_gpio,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_datalogger,
                    &mut refill_test_execution_void,
                );
                
                let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);                
            });
        });
    }

    fn test_execute_refill_receive_actuation_command_send_confirmation(
        rx_controllino_from_refill: &Receiver<InternalCommand>,
        tx_controllino_to_refill: &Sender<bool>,
    ) {
        // receive command from test object
        match rx_controllino_from_refill.recv() {
            Ok(_) => {}
            Err(e) => {
                panic!("Refill: Error when receiving actuation command from test object: {e:?}");
            }
        }

        // send confirmation back
        match tx_controllino_to_refill.send(true) {
            Ok(_) => {}
            Err(e) => {
                panic!("Refill: Error when sending back actuation command confirmation to test object: {e:?}");
            }
        }
    }

    fn test_execute_refill_receive_information_for_datalogger(
        rx_datalogger_from_refill: &Receiver<bool>,
    ) {
        // receive information for data logger
        match rx_datalogger_from_refill.recv() {
            Ok(_) => {}
            Err(e) => {
                panic!("Refill: error when receiving information for data logger: {e:?}");
            }
        }
    }

    #[test]
    // this test case executes execute_refill function and immediately sets the sensor signal to high
    pub fn test_execute_refill_immediate_switch_high() {
        let (_, mut refill, _) = prepare_refill_tests();
        let mut refill_execution = RefillExecution;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (_tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_execute_refill_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                let target_number_iterations = (refill.config.additional_refill_volume
                    / refill.config.refill_pump_flow)
                    as u32
                    * 10
                    + 1;
                // indicate sensor signal high immediately
                run_main_cycle_with_sensor_signals(
                    target_number_iterations,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    true,
                    false,
                );

                test_execute_refill_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };
                
                let result = refill_execution.execute_refill(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, false);
            });
        });
    }

    #[test]
    // this test case executes execute_refill function and immediately sets the sensor signal to high
    // Additionally, the Quit signal is sent.
    // Expected behavior: immediate stop of additional refill operation.
    pub fn test_execute_refill_immediate_switch_high_and_quit_signal() {
        let (_, mut refill, _) = prepare_refill_tests();
        let mut refill_execution = RefillExecution;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_execute_refill_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                // send Quit signal
                match tx_signal_handler_to_refill.send(InternalCommand::Quit) {
                    Ok(_) => {},
                    Err(e) => {
                        panic!("Refill: test_execute_refill_delayed_switch_high_with_invalid_true_and_quit_signal\
                            Error when sending Quit command to test object: {e:?}");
                    }
                }

                // leave switch high
                run_main_cycle_with_sensor_signals(
                    1,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );

                test_execute_refill_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );
                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };

                let result = refill_execution.execute_refill(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, true);
            });
        });
    }

    #[test]
    // this test case executes execute_refill function and immediately sets the sensor signal to high
    // additionally the test case also sets the invalid bit.
    // Additional refill shall be aborted.
    pub fn test_execute_refill_immediate_switch_high_with_invalid_true() {
        let (_, mut refill, _) = prepare_refill_tests();
        let mut refill_execution = RefillExecution;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (_tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_execute_refill_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                let target_number_iterations = 1;
                // indicate sensor signal high immediately
                run_main_cycle_with_sensor_signals(
                    target_number_iterations,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    true,
                    true,
                );

                test_execute_refill_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };

                let result = refill_execution.execute_refill(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, false);
            });
        });
    }

    #[test]
    // this test case executes execute_refill function and sets the sensor signal to high after defined number of iterations
    pub fn test_execute_refill_delayed_switch_high() {
        let (_, mut refill, _) = prepare_refill_tests();
        let mut refill_execution = RefillExecution;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (_tx_signal_handler_from_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_execute_refill_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                let target_number_iterations_main_fill =
                    (0.15 / refill.config.refill_pump_flow) as u32 * 10 + 1;
                // leave switch at low for some time
                run_main_cycle_with_sensor_signals(
                    target_number_iterations_main_fill,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );
                // tank is full. Indicate switch at high position
                let target_number_iterations_additional_fill =
                    (refill.config.additional_refill_volume / refill.config.refill_pump_flow)
                        as u32
                        * 10
                        + 1;
                run_main_cycle_with_sensor_signals(
                    target_number_iterations_additional_fill,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    true,
                    false,
                );

                test_execute_refill_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };

                let result = refill_execution.execute_refill(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, false);
            });
        });
    }

    #[test]
    // this test case executes execute_refill function with simulation of level switch stuck at low position
    pub fn test_execute_refill_switch_stuck_low() {
        let (_, mut refill, _) = prepare_refill_tests();
        let mut refill_execution = RefillExecution;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (_tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_execute_refill_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                // Switch is stuck at low position. Check max limitation of refill
                let target_number_iterations_additional_fill =
                    (refill.config.max_refill_volume / refill.config.refill_pump_flow) as u32 * 10;
                run_main_cycle_with_sensor_signals(
                    target_number_iterations_additional_fill,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );

                test_execute_refill_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };

                let result = refill_execution.execute_refill(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, false);
            });
        });
    }

    #[test]
    // this test case executes execute_refill function and sets the sensor signal to high after defined number of iterations
    pub fn test_execute_refill_delayed_switch_high_and_quit_signal() {
        let (_, mut refill, _) = prepare_refill_tests_tertiary();
        let mut refill_execution = RefillExecution;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_execute_refill_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                let target_number_iterations_main_fill =
                    (0.15 / refill.config.refill_pump_flow) as u32 * 10 + 1;
                // leave switch at low for some time
                run_main_cycle_with_sensor_signals(
                    target_number_iterations_main_fill,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );

                println!("Refill: test_execute_refill_delayed_switch_high_and_quit_signal: sending Quit command...");
                // send Quit signal
                match tx_signal_handler_to_refill.send(InternalCommand::Quit) {
                    Ok(_) => {},
                    Err(e) => {
                        panic!("Refill: test_execute_refill_delayed_switch_high_and_quit_signal\
                            Error when sending Quit command to test object: {e:?}");
                    }
                }

                test_execute_refill_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );
                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };

                let result = refill_execution.execute_refill(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, true);
            });
        });
    }

    #[test]
    // this test case executes execute_refill function and sets the sensor invalid signal to true after defined number of iterations
    pub fn test_execute_refill_delayed_switch_high_with_invalid_true() {
        let (_, mut refill, _) = prepare_refill_tests();
        let mut refill_execution = RefillExecution;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (_tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_execute_refill_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                let target_number_iterations_main_fill =
                    (0.15 / refill.config.refill_pump_flow) as u32 * 10 + 1;
                // leave switch at low for some time
                run_main_cycle_with_sensor_signals(
                    target_number_iterations_main_fill,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );
                // Now let's simulate invalid sensor signal while position signal stays low.
                // Expected behavior: Refill operation shall abort immediately.
                run_main_cycle_with_sensor_signals(
                    1,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    true,
                );

                test_execute_refill_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_refill,
                    &tx_controllino_to_refill,
                );

                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };

                let result = refill_execution.execute_refill(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, false);
            });
        });
    }

    #[test]
    // this test case executes execute_refill function and sets the sensor invalid signal to true after defined number of iterations
    // Additionally, the test case sends the Quit signal.
    // Expected behavior is that refill operation ceases immediately after receiving Quit signal.
    pub fn test_execute_refill_delayed_switch_high_with_invalid_true_and_quit_signal() {
        let (_, mut refill, _) = prepare_refill_tests();
        let mut refill_execution = RefillExecution;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, rx_datalogger_from_refill) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, rx_controllino_from_refill) = mpsc::channel();
        let (tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();

        scope(|scope| {
            // this scope is providing the test environment
            scope.spawn(move || {
                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
                test_execute_refill_receive_actuation_command_send_confirmation(&rx_controllino_from_refill, &tx_controllino_to_refill);

                let target_number_iterations_main_fill = (0.15 / refill.config.refill_pump_flow) as u32 * 10 + 1;
                // leave switch at low for some time
                run_main_cycle_with_sensor_signals(
                    target_number_iterations_main_fill,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );

                // send Quit signal
                match tx_signal_handler_to_refill.send(InternalCommand::Quit) {
                    Ok(_) => {},
                    Err(e) => {
                        panic!("Refill: test_execute_refill_delayed_switch_high_with_invalid_true_and_quit_signal\
                            Error when sending Quit command to test object: {e:?}");
                    }
                }

                test_execute_refill_receive_actuation_command_send_confirmation(&rx_controllino_from_refill, &tx_controllino_to_refill);

                test_execute_refill_receive_information_for_datalogger(&rx_datalogger_from_refill);
            });

            // this scope is executing the test object
            scope.spawn(move || {
                let mut tank_level_switch_signals = TankLevelSwitchSignals {
                    tank_level_switch_position: false,
                    tank_level_switch_invalid: false,
                    error_receive_tank_level_switch_position: false,
                    error_receive_tank_level_switch_invalid: false,
                };

                let result = refill_execution.execute_refill(
                    &tx_refill_to_controllino,
                    &rx_refill_from_controllino,
                    &tx_refill_to_gpio,
                    &rx_refill_from_gpio,
                    &rx_refill_from_signal_handler,
                    &tx_refill_to_datalogger,
                    &mut tank_level_switch_signals,
                    &mut refill.refill_errors,
                    &refill.config,
                    &mut refill.sql_interface_refill,
                );

                assert_eq!(result, true);
            });
        });
    }
    
    #[test]
    // Executes loop of the .execute function manipulating the sensor signals only and not giving a negative schedule check.
    // Checks if the test object correctly processes the negative schedule check result.
    pub fn test_refill_blocked_by_schedule() {
        let sleep_duration_50_millisec = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill, mut sql_interface) = prepare_refill_tests_secondary();
        // accelerate test case execution
        refill
            .config
            .tank_level_switch_position_high_stabilization_count = 2;
        refill.config.check_interval = 1;

        let (tx_refill_to_gpio, rx_gpio_from_refill) = mpsc::channel();
        let (tx_refill_to_datalogger, _) = mpsc::channel();
        let (tx_gpio_to_refill, rx_refill_from_gpio) = mpsc::channel();
        let (tx_refill_to_controllino, _) = mpsc::channel();
        let (_, rx_refill_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) = mpsc::channel();
        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_device_scheduler_environment = mutex_device_scheduler.clone();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    false,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_50_millisec);
            });
            
            // thread for rest of test environment
            scope.spawn(move || {
                let execution_count: i32;

                // test with tank level switch low and valid
                run_main_cycle_with_sensor_signals(
                    refill
                        .config
                        .tank_level_switch_position_high_stabilization_count
                        * 2,
                    &rx_gpio_from_refill,
                    &tx_gpio_to_refill,
                    false,
                    false,
                );
                {
                    execution_count = *mutex_device_scheduler_environment.lock().unwrap();
                }

                // now we can send the quit command
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);

                // test object thread will end. We can assert now.
                assert_eq!(execution_count, 0); // check if no refill operation has been executed
            });
            // give the test environment a little time to trigger the receive call for signal update
            spin_sleeper.sleep(sleep_duration_50_millisec);

            // thread for test object
            scope.spawn(move || {
                let mut refill_test_execution_void = RefillTestExecutionVoid;
                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();

                refill.execute(
                    mutex_device_scheduler.clone(),
                    tx_refill_to_gpio,
                    rx_refill_from_gpio,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_datalogger,
                    &mut refill_test_execution_void,
                );
                
                let _ = tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);                
            });
        });
    }    
}
