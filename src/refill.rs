/* Copyright 2024 Uwe Martinuuu

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::{error, warn};

#[cfg(not(test))]
use log::info;

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::fmt;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::channel_content::{AquariumSignal, InternalCommand};
use crate::data_logger_communication::DataLoggerCommunicationTrait;
use crate::proc_ext_req::ProcessExternalRequestTrait;
use crate::sql_interface_refill::DatabaseInterfaceRefillTrait;
use crate::water_injection::WaterInjectionTrait;

/// Holds the configuration data for the refill control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Clone)]
pub struct RefillConfig {
    /// control will only be active when the flag is set to true
    active: bool,

    /// time interval in seconds which need to pass after initialization of the application before refill control is becoming active
    pub initial_wait_interval: u64,

    /// interval for the main control in seconds    
    pub check_interval: u64,

    /// maximum number of permitted refill operations within last 24h
    /// - If this number is exceeded, then the refill control will not further refill even if water level sensor signal value is low.
    max_refill_count_24h: u64,

    /// maximum permitted refill volume within last 24h
    /// - If this number is exceeded, then the refill control will not further refill even if water level sensor signal value is low.
    max_refill_volume_24h: f64,

    /// maximum number of permitted refill operations within last hour
    /// - If this number is exceeded, then the refill control will not further refill even if water level sensor signal value is low.
    max_refill_count_hour: u64,

    /// maximum permitted refill volume within last hour
    /// - If this number is exceeded, then the refill control will not further refill even if water level sensor signal value is low.
    max_refill_volume_hour: f64,

    /// minimum time which has to pass after a refill operation before a subsequent refill operation is started
    min_interval_last_refill: u64,

    /// time interval in seconds which need to pass before the application recognizes a potential tank level switch fault
    max_duration_since_last_refill: u64,

    /// flow of the refill pump in liters / second
    pub refill_pump_flow: f64,

    /// maximum permitted refill volume in one refill operation
    pub max_refill_volume: f64,

    /// refill volume which is added to the tank in a refill operation after the application recognizes tank level position high
    /// - Background: This volume shall avoid too frequent refill operations. Too high values may lead to unstable water level.
    /// It is reommended to calibrate a value which is a fraction of max_refill_volume.
    pub additional_refill_volume: f64,
}

const LOG_TARGET_REFILL: &str = "refill";

const MILLIS_PER_SEC: u64 = 1000;

/// Contains the error flags for the refill component.
pub struct RefillErrors {
    pub error_switch_stuck_high: bool,
    pub error_switch_stuck_low: bool,
    pub error_receive_tank_level_switch_position: bool,
    pub error_sql_update_failed: bool,
    pub error_sql_get_historic_data_failed: bool,
}

impl RefillErrors {
    /// Resets all error bits of the struct.
    pub fn reset_all_errors(&mut self) {
        self.error_switch_stuck_high = false;
        self.error_switch_stuck_low = false;
        self.error_receive_tank_level_switch_position = false;
        self.error_sql_update_failed = false;
        self.error_sql_get_historic_data_failed = false;
    }

    #[cfg(test)]
    pub fn has_error(&mut self) -> bool {
        self.error_switch_stuck_high
            || self.error_switch_stuck_low
            || self.error_receive_tank_level_switch_position
            || self.error_sql_update_failed
            || self.error_sql_get_historic_data_failed
    }
}

impl fmt::Display for RefillErrors {
    /// Function provides output of RefillErrors for development purposes
    /// # Arguments
    /// * `f` - configuration for formatting
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
            "error_switch_stuck_high: {}, error_switch_stuck_low: {}, error_receive_tank_level_switch_position: {},  error_sql_update_failed: {}, error_sql_get_historic_data_failed: {}",
            self.error_switch_stuck_high,
            self.error_switch_stuck_low,
            self.error_receive_tank_level_switch_position,
            self.error_sql_update_failed,
            self.error_sql_get_historic_data_failed,
        )
    }
}

/// Contains the configuration and the implementation for the refill control.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// Thread communication  of this component is as follows:
/// ```mermaid
/// graph LR
/// refill[Refill control] --> tank_level_switch[TankLevelSwitchxx§]
/// refill --> water_injection[Water injection]
/// water_injection --> tank_level_switch
/// tank_level_switch --> refill
/// tank_level_switch --> water_injection
/// water_injection --> controllino[Controllino]
/// controllino --> water_injection
/// refill --> signal_handler[Signal handler]
/// signal_handler --> refill
/// signal_handler --> water_injection
/// refill --> schedule_check[Schedule check]
/// schedule_check --> refill
/// refill --> data_logger[Data logger]
/// data_logger --> refill
/// messaging[Messaging] --> refill
/// ```
/// Communication channel to and from Controllino is forwarded to implementation of WaterInjectionTrait.
/// Communication channel to and from data logger is forwarded to implementation of WaterInjectionTrait.
/// Signal handler is communicating both directly with refill as well with implementation of WaterInjectionTrait.
/// Tank level switch calculation is communicating both directly with refill as well with implementation of WaterInjectionTrait.

/// Architecture of the component is as follows:
/// ```mermaid
/// graph LR
/// execute --> calc_tank_level_switch_position_stabilized;
/// execute --> get_sql_refill_data;
/// execute_refill --> process_external_request;
/// execute --> check_error_condition_switch_stuck_high;
/// execute --> process_external_request;
/// execute --> update_tank_level_switch_signals;
/// execute_refill --> update_tank_level_switch_signals;
/// execute --> check_volume_count_time_limits;
/// execute --> execute_refill;
/// ```
pub struct Refill {
    /// configuration of refill control
    /// Attribute is public becaused access from within test cases.
    pub config: RefillConfig,

    /// interface to SQL database
    /// Attribute is public becaused access from within test cases.
    pub sql_interface_refill: Box<dyn DatabaseInterfaceRefillTrait + Sync + Send>,

    /// struct of type RefillErrors which includes flags for different kind of errors
    pub refill_errors: RefillErrors,

    /// inhibition flag to avoid flooding the log file with repeated messages about stop command being received
    lock_info_stop_command_received: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about start command being received
    lock_info_start_command_received: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about refill volume check of last 24h
    lock_info_refill_check_volume_last_24h: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about refill count check of last 24h
    lock_info_refill_check_count_last_24h: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about refill volume check of last hour
    lock_info_refill_check_volume_last_hour: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about refill count check of last hour
    lock_info_refill_check_count_last_hour: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about refill interval check
    lock_info_refill_check_interval_last_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read duration since last refill
    lock_error_read_duration_since_last_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to check if refill log table is empty
    lock_error_check_empty: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read refill volume of last 24h from SQL database
    lock_error_read_refill_volume_last_24h: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read refill count of last 24h from SQL database
    lock_error_read_refill_count_last_24h: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read refill volume of last hour from SQL database
    lock_error_read_refill_volume_last_hour: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read refill count of last hour from SQL database
    lock_error_read_refill_count_last_hour: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to request to schedule check via channel
    lock_error_channel_send_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive from schedule check via channel
    lock_error_channel_receive_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received not applicable command from signal handler
    lock_warn_not_applicable_command: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive from signal handler via channel
    lock_error_channel_receive_termination_command: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure of sending request for tank level switch position via channel
    lock_error_channel_send_tank_level_switch_position: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure of receiving tank level switch position via channel
    lock_error_channel_receive_tank_level_switch_position: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure of sending request for tank level switch invalid via channel
    lock_error_channel_send_tank_level_switch_invalid: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure of receiving tank level switch invalid via channel
    lock_error_channel_receive_tank_level_switch_invalid: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure of sending request for tank level switch position stabilized via channel
    lock_error_channel_send_tank_level_switch_position_stabilized: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure of receiving tank level switch position stabilized via channel
    lock_error_channel_receive_tank_level_switch_position_stabilized: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive command from data logger
    pub lock_error_channel_receive_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command from data logger via channel
    pub lock_warn_inapplicable_command_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having recevied request for invalid signal from data logger
    pub lock_error_invalid_signal_request_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failuire to send response to data logger
    pub lock_error_send_to_data_logger: bool,
}

/// Contains the data retrieved from SQL database.
/// It is also used to communicate data to be updated in SQL database.
struct RefillSqlData {
    duration_since_last_refill: u64,
    refill_volume_last_24h: f64,
    refill_count_last_24h: u64,
    refill_volume_last_hour: f64,
    refill_count_last_hour: u64,
}

impl RefillSqlData {
    /// Initializer using all zero data.
    pub fn new() -> RefillSqlData {
        RefillSqlData {
            duration_since_last_refill: 0,
            refill_volume_last_24h: 0.0,
            refill_count_last_24h: 0,
            refill_volume_last_hour: 0.0,
            refill_count_last_hour: 0,
        }
    }
}

impl fmt::Display for RefillSqlData {
    /// Function provides output of the struct values for debugging purposes.
    /// # Arguments
    /// * `f` - configuration for formatting    
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            " {}: Duration since last refill is {} seconds.\n \
            Refill: volume of last 24h is {} L.\n Refill: Count of last 24h is {}.\n \
            Refill: volume of last hour is {} L.\n Refill: Count of last hour is {}.",
            LOG_TARGET_REFILL,
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
    pub fn new(
        config: RefillConfig,
        sql_interface_refill: Box<dyn DatabaseInterfaceRefillTrait + Sync + Send>,
    ) -> Refill {
        Self {
            config: config,
            sql_interface_refill: sql_interface_refill,
            refill_errors: RefillErrors {
                error_switch_stuck_high: false,
                error_switch_stuck_low: false,
                error_receive_tank_level_switch_position: false,
                error_sql_update_failed: false,
                error_sql_get_historic_data_failed: false,
            },
            lock_info_stop_command_received: false,
            lock_info_start_command_received: false,
            lock_info_refill_check_volume_last_24h: false,
            lock_info_refill_check_count_last_24h: false,
            lock_info_refill_check_volume_last_hour: false,
            lock_info_refill_check_count_last_hour: false,
            lock_info_refill_check_interval_last_refill: false,
            lock_error_read_duration_since_last_refill: false,
            lock_error_check_empty: false,
            lock_error_read_refill_volume_last_24h: false,
            lock_error_read_refill_count_last_24h: false,
            lock_error_read_refill_volume_last_hour: false,
            lock_error_read_refill_count_last_hour: false,
            lock_error_channel_send_schedule_check: false,
            lock_error_channel_receive_schedule_check: false,
            lock_warn_not_applicable_command: false,
            lock_error_channel_receive_termination_command: false,
            lock_error_channel_receive_tank_level_switch_invalid: false,
            lock_error_channel_receive_tank_level_switch_position: false,
            lock_error_channel_receive_tank_level_switch_position_stabilized: false,
            lock_error_channel_send_tank_level_switch_invalid: false,
            lock_error_channel_send_tank_level_switch_position: false,
            lock_error_channel_send_tank_level_switch_position_stabilized: false,
            lock_error_channel_receive_data_logger: false,
            lock_warn_inapplicable_command_data_logger: false,
            lock_error_invalid_signal_request_data_logger: false,
            lock_error_send_to_data_logger: false,
        }
    }

    #[cfg(test)]
    /// Provides a struct of type Refill with RefillErrors as additional parameter for testing
    /// # Arguments
    /// * `config` - configuration data for refill control
    /// * `sql_interface_refill` - specific SQL interface for refill
    /// * `refill_errors` - struct containing the error bits
    pub fn new_with_errors(
        config: RefillConfig,
        sql_interface_refill: Box<dyn DatabaseInterfaceRefillTrait + Sync + Send>,
        refill_errors: RefillErrors,
    ) -> Refill {
        Self {
            config: config,
            sql_interface_refill: sql_interface_refill,
            refill_errors: refill_errors,
            lock_info_stop_command_received: false,
            lock_info_start_command_received: false,
            lock_info_refill_check_volume_last_24h: false,
            lock_info_refill_check_count_last_24h: false,
            lock_info_refill_check_volume_last_hour: false,
            lock_info_refill_check_count_last_hour: false,
            lock_info_refill_check_interval_last_refill: false,
            lock_error_read_duration_since_last_refill: false,
            lock_error_check_empty: false,
            lock_error_read_refill_volume_last_24h: false,
            lock_error_read_refill_count_last_24h: false,
            lock_error_read_refill_volume_last_hour: false,
            lock_error_read_refill_count_last_hour: false,
            lock_error_channel_send_schedule_check: false,
            lock_error_channel_receive_schedule_check: false,
            lock_warn_not_applicable_command: false,
            lock_error_channel_receive_termination_command: false,
            lock_error_channel_receive_tank_level_switch_invalid: false,
            lock_error_channel_receive_tank_level_switch_position: false,
            lock_error_channel_receive_tank_level_switch_position_stabilized: false,
            lock_error_channel_send_tank_level_switch_invalid: false,
            lock_error_channel_send_tank_level_switch_position: false,
            lock_error_channel_send_tank_level_switch_position_stabilized: false,
            lock_error_channel_receive_data_logger: false,
            lock_warn_inapplicable_command_data_logger: false,
            lock_error_invalid_signal_request_data_logger: false,
            lock_error_send_to_data_logger: false,
        }
    }

    /// Retrieves the refill-related information from SQL database:
    /// - duration since last refill in seconds
    /// - volume refilled in last 24h
    /// - number of refill operations in last 24h
    /// - volume refilled in the last hour
    /// - number of refill operations in the last hour
    fn get_sql_refill_data(&mut self, refill_sql_data: &mut RefillSqlData) {
        match self.sql_interface_refill.check_empty() {
            Ok(0) => {
                self.lock_error_check_empty = false;
                refill_sql_data.duration_since_last_refill =
                    self.config.min_interval_last_refill + 1;
            }
            Ok(_) => {
                self.lock_error_check_empty = false;
                match self.sql_interface_refill.get_duration_since_last_refill() {
                    Ok(c) => {
                        self.lock_error_read_duration_since_last_refill = false;
                        refill_sql_data.duration_since_last_refill = c;
                    }
                    #[cfg(not(test))]
                    Err(e) => {
                        if !self.lock_error_read_duration_since_last_refill {
                            #[cfg(not(test))]
                            error!(
                                target: LOG_TARGET_REFILL,
                                "could not get duration since last refill event ({e:?})"
                            );
                            self.lock_error_read_duration_since_last_refill = true;
                        }
                        self.refill_errors.error_sql_get_historic_data_failed = true;
                    }
                    #[cfg(test)]
                    Err(_) => {
                        self.lock_error_read_duration_since_last_refill = true;
                        self.refill_errors.error_sql_get_historic_data_failed = true;
                    }
                }
            }
            #[cfg(not(test))]
            Err(e) => {
                if !self.lock_error_check_empty {
                    #[cfg(not(test))]
                    error!(
                        target: LOG_TARGET_REFILL,
                        "could not check if refill table is empty or not ({e:?})"
                    );
                    self.lock_error_check_empty = true;
                }
                self.refill_errors.error_sql_get_historic_data_failed = true;
            }
            #[cfg(test)]
            Err(_) => {
                self.lock_error_check_empty = true;
                self.refill_errors.error_sql_get_historic_data_failed = true;
            }
        };

        match self.sql_interface_refill.get_refill_volume_of_last_24h() {
            Ok(c) => {
                self.lock_error_read_refill_volume_last_24h = false;
                refill_sql_data.refill_volume_last_24h = c;
            }
            #[cfg(not(test))]
            Err(e) => {
                if !self.lock_error_read_refill_volume_last_24h {
                    #[cfg(not(test))]
                    error!(
                        target: LOG_TARGET_REFILL,
                        "could not get refill volume of last 24 hours ({e:?})");
                    self.lock_error_read_refill_volume_last_24h = true;
                }
                self.refill_errors.error_sql_get_historic_data_failed = true;
            }
            #[cfg(test)]
            Err(_) => {
                self.lock_error_read_refill_volume_last_24h = true;
                self.refill_errors.error_sql_get_historic_data_failed = true;
            }
        };

        match self.sql_interface_refill.get_refill_count_of_last_24h() {
            Ok(c) => {
                self.lock_error_read_refill_count_last_24h = false;
                refill_sql_data.refill_count_last_24h = c;
            }
            #[cfg(not(test))]
            Err(e) => {
                if !self.lock_error_read_refill_count_last_24h {
                    #[cfg(not(test))]
                    error!(
                        target: LOG_TARGET_REFILL,
                        "could not get refill count of last 24 hours: {e:?}"
                    );
                    self.lock_error_read_refill_count_last_24h = true;
                }
                self.refill_errors.error_sql_get_historic_data_failed = true;
            }
            #[cfg(test)]
            Err(_) => {
                self.lock_error_read_refill_count_last_24h = true;
                self.refill_errors.error_sql_get_historic_data_failed = true;
            }
        };

        match self.sql_interface_refill.get_refill_volume_of_last_hour() {
            Ok(c) => {
                self.lock_error_read_refill_volume_last_hour = false;
                refill_sql_data.refill_volume_last_hour = c;
            }
            #[cfg(not(test))]
            Err(e) => {
                if !self.lock_error_read_refill_volume_last_hour {
                    #[cfg(not(test))]
                    error!(
                        target: LOG_TARGET_REFILL,
                        "Could not get refill volume of last hour: {e:?}"
                    );
                    self.lock_error_read_refill_volume_last_hour = true;
                }
                self.refill_errors.error_sql_get_historic_data_failed = true;
            }
            #[cfg(test)]
            Err(_) => {
                self.lock_error_read_refill_volume_last_hour = true;
                self.refill_errors.error_sql_get_historic_data_failed = true;
            }
        };

        match self.sql_interface_refill.get_refill_count_of_last_hour() {
            Ok(c) => {
                self.lock_error_read_refill_count_last_hour = false;
                refill_sql_data.refill_count_last_hour = c;
            }
            #[cfg(not(test))]
            Err(e) => {
                if !self.lock_error_read_refill_count_last_hour {
                    error!(
                        target: LOG_TARGET_REFILL,
                        "Could not get refill count of last hour: {e:?}"
                    );
                    self.lock_error_read_refill_count_last_hour = true;
                }
                self.refill_errors.error_sql_get_historic_data_failed = true;
            }
            #[cfg(test)]
            Err(_) => {
                self.lock_error_read_refill_count_last_hour = true;
                self.refill_errors.error_sql_get_historic_data_failed = true;
            }
        };
    }

    /// Checks if the duration since the last refill operation eceeds a configurable limit.
    /// If yes, then corresponding error flag is set.
    /// # Arguments
    /// * `duration_since_last_refill` - duration in seconds since last refill operation
    fn check_error_condition_switch_stuck_high(&mut self, duration_since_last_refill: u64) {
        self.refill_errors.error_switch_stuck_high =
            duration_since_last_refill > self.config.max_duration_since_last_refill;
    }

    /// Calculates the inhibition flag depending on the commands received.
    /// # Arguments
    /// * `current_state` - current value of inhibition flag
    /// * `start_command_received` - flag indicating that start command has been received
    /// * `stop_command_received` - flag indicating that stop command has been received
    fn calc_inhibition_flag(
        &mut self,
        current_state: bool,
        start_command_received: bool,
        stop_command_received: bool,
    ) -> bool {
        // initialize return value with current state
        let mut new_state: bool = current_state;

        if stop_command_received {
            if !self.lock_info_stop_command_received {
                #[cfg(not(test))]
                info!(
                    target: LOG_TARGET_REFILL,
                    "received Stop command. Inhibiting refill control."
                );

                self.lock_info_stop_command_received = true;
            }
            new_state = true;
        } else {
            self.lock_info_stop_command_received = false;
        }
        // start command has higher priority than stop command
        // therefore it is evaluated after stop command
        if start_command_received {
            if !self.lock_info_start_command_received {
                #[cfg(not(test))]
                info!(
                    target: LOG_TARGET_REFILL,
                    "received Start command. Restarting refill control."
                );

                self.lock_info_start_command_received = true;
            } else {
                self.lock_info_start_command_received = true;
            }
            new_state = false;
        }
        new_state
    }

    // Checks if historic refill data does not exceed limitations in terms of count and volume.
    // The function returns a tuple of boolean variables.
    /// # Arguments
    /// * `sql_refill_data` - reference to data retrieved from SQL database    
    fn check_volume_count_time_limits(&mut self, sql_refill_data: &RefillSqlData) -> bool {
        let refill_check_volume_last_24h =
            sql_refill_data.refill_volume_last_24h > self.config.max_refill_volume_24h;

        if refill_check_volume_last_24h {
            if !self.lock_info_refill_check_volume_last_24h {
                #[cfg(not(test))]
                info!(
                    target: LOG_TARGET_REFILL,
                    "refill volume of last 24h exceeds permissible refill volume per 24h of {} L",
                    self.config.max_refill_volume_24h
                );

                self.lock_info_refill_check_volume_last_24h = true;
            }
        } else {
            self.lock_info_refill_check_volume_last_24h = false;
        }

        let refill_check_count_last_24h =
            sql_refill_data.refill_count_last_24h > self.config.max_refill_count_24h;
        if refill_check_count_last_24h {
            if !self.lock_info_refill_check_count_last_24h {
                #[cfg(not(test))]
                info!(
                    target: LOG_TARGET_REFILL,
                    "refill count of last 24h exceeds permissible refill count per 24h of {} L",
                    self.config.max_refill_count_24h
                );

                self.lock_info_refill_check_count_last_24h = true;
            }
        } else {
            self.lock_info_refill_check_count_last_24h = false;
        }

        let refill_check_volume_last_hour =
            sql_refill_data.refill_volume_last_hour > self.config.max_refill_volume_hour;
        if refill_check_volume_last_hour {
            if !self.lock_info_refill_check_volume_last_hour {
                #[cfg(not(test))]
                info!(
                    target: LOG_TARGET_REFILL,
                    "refill volume of last hour exceeds permissible refill volume per hour of {}",
                    self.config.max_refill_volume_hour
                );

                self.lock_info_refill_check_volume_last_hour = true;
            }
        } else {
            self.lock_info_refill_check_volume_last_hour = false;
        }

        let refill_check_count_last_hour =
            sql_refill_data.refill_count_last_hour > self.config.max_refill_count_hour;
        if refill_check_count_last_hour {
            if !self.lock_info_refill_check_count_last_hour {
                #[cfg(not(test))]
                info!(
                    target: LOG_TARGET_REFILL,
                    "refill count of last 24h exceeds permissible refill count per hour of {}",
                    self.config.max_refill_count_hour
                );

                self.lock_info_refill_check_count_last_hour = true;
            }
        } else {
            self.lock_info_refill_check_count_last_hour = false;
        }

        let refill_check_interval_last_refill =
            sql_refill_data.duration_since_last_refill < self.config.min_interval_last_refill;
        if refill_check_interval_last_refill {
            if !self.lock_info_refill_check_interval_last_refill {
                #[cfg(not(test))]
                info!(
                    target: LOG_TARGET_REFILL,
                    "time interval since last refill {} is too small (limit={}).",
                    sql_refill_data.duration_since_last_refill, self.config.min_interval_last_refill
                );

                self.lock_info_refill_check_interval_last_refill = true;
            }
        } else {
            self.lock_info_refill_check_interval_last_refill = false;
        }
        refill_check_volume_last_24h
            | refill_check_count_last_24h
            | refill_check_volume_last_hour
            | refill_check_count_last_hour
            | refill_check_interval_last_refill
    }

    // Channel communication with tank level switch calculation
    // The function returns a tuple of boolean variables.
    /// # Arguments
    /// * `tx_to_tank_level_switch` - reference to sender part of channel
    /// * `rx_from_tank_level_switch` - reference to receiver part of channel
    pub fn get_tank_level_switch_signals(
        &mut self,
        tx_to_tank_level_switch: &Sender<InternalCommand>,
        rx_from_tank_level_switch: &Receiver<bool>,
    ) -> (bool, bool, bool) {
        // send request for tank level switch position
        let tank_level_switch_position = match tx_to_tank_level_switch.send(
            InternalCommand::RequestSignal(AquariumSignal::TankLevelSwitchPosition),
        ) {
            Ok(_) => {
                self.lock_error_channel_send_tank_level_switch_position = false;
                // Sent request for tank level switch position.
                // now listen for response
                match rx_from_tank_level_switch.recv() {
                    Ok(c) => {
                        self.lock_error_channel_receive_tank_level_switch_position = false;
                        c
                    }
                    Err(e) => {
                        if !self.lock_error_channel_receive_tank_level_switch_position {
                            error!(
                                target: LOG_TARGET_REFILL,
                                "receiving tank level switch position failed ({e:?})"
                            );
                            self.lock_error_channel_receive_tank_level_switch_position = true;
                        }
                        true // default value in case of error
                    }
                }
            }
            Err(e) => {
                if !self.lock_error_channel_send_tank_level_switch_position {
                    error!(
                        target: LOG_TARGET_REFILL,
                        "channel communication for tank level switch position failed ({e:?})"
                    );
                    self.lock_error_channel_send_tank_level_switch_position = true;
                }
                true // default value in case of error
            }
        };

        // send request for tank level switch invalid
        let tank_level_switch_invalid = match tx_to_tank_level_switch.send(
            InternalCommand::RequestSignal(AquariumSignal::TankLevelSwitchInvalid),
        ) {
            Ok(_) => {
                self.lock_error_channel_send_tank_level_switch_invalid = false;
                // Sent request for tank level switch position.
                // now listen for response
                match rx_from_tank_level_switch.recv() {
                    Ok(c) => {
                        self.lock_error_channel_receive_tank_level_switch_invalid = false;
                        c
                    }
                    Err(e) => {
                        if !self.lock_error_channel_receive_tank_level_switch_invalid {
                            error!(
                                target: LOG_TARGET_REFILL,
                                "receiving tank level switch invalid failed ({e:?})"
                            );
                            self.lock_error_channel_receive_tank_level_switch_invalid = true;
                        }
                        true // default value in case of error
                    }
                }
            }
            Err(e) => {
                if !self.lock_error_channel_send_tank_level_switch_invalid {
                    error!(
                        target: LOG_TARGET_REFILL,
                        "channel communication for tank level switch invalid failed ({e:?})"
                    );
                    self.lock_error_channel_send_tank_level_switch_invalid = true;
                }
                true // default value in case of error
            }
        };

        // send request for tank level switch position stabilized
        let tank_level_switch_position_stabilized = match tx_to_tank_level_switch.send(
            InternalCommand::RequestSignal(AquariumSignal::TankLevelSwitchPositionStabilized),
        ) {
            Ok(_) => {
                self.lock_error_channel_send_tank_level_switch_position_stabilized = false;
                // Sent request for tank level switch position stabilized.
                // now listen for response
                match rx_from_tank_level_switch.recv() {
                    Ok(c) => {
                        self.lock_error_channel_receive_tank_level_switch_position_stabilized =
                            false;
                        c
                    }
                    Err(e) => {
                        if !self.lock_error_channel_receive_tank_level_switch_position_stabilized {
                            error!(
                                target: LOG_TARGET_REFILL,
                                "receiving tank level switch position failed ({e:?})"
                            );
                            self.lock_error_channel_receive_tank_level_switch_position_stabilized =
                                true;
                        }
                        true // default value in case of error
                    }
                }
            }
            Err(e) => {
                if !self.lock_error_channel_send_tank_level_switch_position_stabilized {
                    error!(
                        target: LOG_TARGET_REFILL,
                        "channel communication for tank level switch position stabilized failed ({e:?})"
                    );
                    self.lock_error_channel_send_tank_level_switch_position_stabilized = true;
                }
                true // default value in case of error
            }
        };

        (
            tank_level_switch_position,
            tank_level_switch_invalid,
            tank_level_switch_position_stabilized,
        )
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `mutex_device_scheduler_refill` - clone of the mutex for avoiding parallel device actuation.
    /// * `tx_to_tank_level_switch` - Sender part of channel for communication to tank level switch calculation
    /// * `rx_from_tank_level_switch` - Receiver part of channel for communication from tank level switch calculation
    /// * `tx_to_controllino` - Sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Receiver part of channel for communication from Controllino
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    /// * `tx_to_schedule_check` - Sender part of channel for communication to schedule checker
    /// * `rx_from_schedule_check` - Receiver part of channel for communication from schedule checker               
    /// * `tx_to_data_logger` - Sender part of channel for communication to data logger
    /// * `rx_from_data_logger` - Receiver part of channel for communication from data logger
    /// * `water_injection` - Reference to struct implementing WaterInjectionTrait
    pub fn execute(
        &mut self,
        mutex_device_scheduler_refill: Arc<Mutex<i32>>,
        tx_to_tank_level_switch: Sender<InternalCommand>,
        rx_from_tank_level_switch: Receiver<bool>,
        tx_to_controllino: Sender<InternalCommand>,
        rx_from_controllino: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
        tx_to_schedule_check: Sender<InternalCommand>,
        rx_from_schedule_check: Receiver<bool>,
        tx_to_data_logger: Sender<bool>,
        rx_from_data_logger: Receiver<InternalCommand>,
        rx_from_messaging_opt: Option<Receiver<InternalCommand>>,
        water_injection: &mut impl WaterInjectionTrait,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let mut i = 0;
        let spin_sleeper = SpinSleeper::default();
        let mut schedule_check_result: bool = false;
        let mut tank_level_switch_invalid: bool;
        let mut tank_level_switch_position_stabilized: bool;
        let mut lock_warn_cycle_time_exceeded: bool = false;
        let mut cycle_time_exceeded: bool = false; // ensures that signal handler/messaging are polled at least once even if cycle time is exceeded

        // retrieving data from SQL data base
        let mut sql_refill_data = RefillSqlData::new();
        self.get_sql_refill_data(&mut sql_refill_data);

        let mut refill_inhibited = false;
        let mut quit_command_received = false; // request to end the application has been received
        let mut start_command_received; // request to (re-)start feed has been received
        let mut stop_command_received; // request to (temporarily) stop feed has been received

        // initial waiting time
        if self.config.initial_wait_interval > 0 {
            while i < self.config.initial_wait_interval * 10 {
                i += 1;

                self.communicate_with_data_logger(&tx_to_data_logger, &rx_from_data_logger, false);

                (
                    quit_command_received,
                    start_command_received,
                    stop_command_received,
                ) = self.process_external_request(
                    &rx_from_signal_handler,
                    rx_from_messaging_opt.as_ref(),
                );
                refill_inhibited = self.calc_inhibition_flag(
                    refill_inhibited,
                    start_command_received,
                    stop_command_received,
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

        let mut start_time = SystemTime::now();

        // main control loop for refill
        loop {
            // check if either during main control loop or during refill execution termination requests has been received
            if quit_command_received {
                break; // exit outer loop
            }

            if self.config.active {
                // check when the last refill operation has taken place
                // if too much time has passed, issue an error condition - not impeding rest of operation
                self.check_error_condition_switch_stuck_high(
                    sql_refill_data.duration_since_last_refill,
                );

                if !quit_command_received && !refill_inhibited {
                    (
                        _,
                        tank_level_switch_invalid,
                        tank_level_switch_position_stabilized,
                    ) = self.get_tank_level_switch_signals(
                        &tx_to_tank_level_switch,
                        &rx_from_tank_level_switch,
                    );
                    self.get_sql_refill_data(&mut sql_refill_data);

                    // execute checks before initiating refill
                    if !self.check_volume_count_time_limits(&sql_refill_data) {
                        if !self.refill_errors.error_switch_stuck_low
                            && !self.refill_errors.error_sql_update_failed
                            && !self.refill_errors.error_sql_get_historic_data_failed
                        {
                            // schedule check to see if actuation is allowed
                            match tx_to_schedule_check.send(InternalCommand::ScheduleCheck) {
                                Ok(_) => {
                                    // now listen for response
                                    schedule_check_result = match rx_from_schedule_check.recv() {
                                        Ok(c) => {
                                            self.lock_error_channel_receive_schedule_check = false;
                                            c
                                        }
                                        Err(e) => {
                                            if !self.lock_error_channel_receive_schedule_check {
                                                error!(
                                                    target: LOG_TARGET_REFILL,
                                                    "reading channel content for schedule check failed ({e:?})"
                                                );
                                                self.lock_error_channel_receive_schedule_check =
                                                    false;
                                            }
                                            true // assume we are allowed to actuate when communication is not successful
                                        }
                                    };
                                    self.lock_error_channel_send_schedule_check = false;
                                }
                                Err(e) => {
                                    if !self.lock_error_channel_send_schedule_check {
                                        error!(
                                            target: LOG_TARGET_REFILL,
                                            "channel communication to schedule check failed ({e:?})"
                                        );
                                        self.lock_error_channel_send_schedule_check = true;
                                    }
                                }
                            }

                            if schedule_check_result {
                                if !tank_level_switch_position_stabilized
                                    && !tank_level_switch_invalid
                                {
                                    // conditions for allowing refill operation are given
                                    let mut mutex_data =
                                        mutex_device_scheduler_refill.lock().unwrap();
                                    // now we are given exclusive access to actuators

                                    quit_command_received = water_injection.inject_water(
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                        &tx_to_tank_level_switch,
                                        &rx_from_tank_level_switch,
                                        &rx_from_signal_handler,
                                        &tx_to_data_logger,
                                        &rx_from_data_logger,
                                        &mut self.refill_errors,
                                        &mut self.sql_interface_refill,
                                    );
                                    *mutex_data += 1;
                                }
                            }
                        }
                    }
                }
            }

            let stop_time = SystemTime::now();
            let execution_duration_millis =
                stop_time.duration_since(start_time).unwrap().as_millis() as u64;
            let cycle_time_refill_millis: u64 = self.config.check_interval * MILLIS_PER_SEC;

            let remaining_sleep_time_millis: u64;
            if execution_duration_millis >= cycle_time_refill_millis {
                if !lock_warn_cycle_time_exceeded {
                    warn!(target: LOG_TARGET_REFILL,
                        "execution duration of {} milliseconds exceeds cycle time of {}",
                        execution_duration_millis,
                        cycle_time_refill_millis
                    );
                    lock_warn_cycle_time_exceeded = true;
                } else {
                    lock_warn_cycle_time_exceeded = false;
                }
                remaining_sleep_time_millis = 0;
                cycle_time_exceeded = true;
            } else {
                remaining_sleep_time_millis = cycle_time_refill_millis - execution_duration_millis;
            }

            // skip further waiting if quit command has been received during refilling
            if !quit_command_received {
                // wait for configured time period before rechecking the sensor signal
                // check for external requests while waiting
                i = 0;
                while i < remaining_sleep_time_millis || cycle_time_exceeded {
                    i += 1;

                    self.communicate_with_data_logger(
                        &tx_to_data_logger,
                        &rx_from_data_logger,
                        false,
                    );

                    (
                        quit_command_received,
                        start_command_received,
                        stop_command_received,
                    ) = self.process_external_request(
                        &rx_from_signal_handler,
                        rx_from_messaging_opt.as_ref(),
                    );
                    refill_inhibited = self.calc_inhibition_flag(
                        refill_inhibited,
                        start_command_received,
                        stop_command_received,
                    );
                    if quit_command_received {
                        break;
                    }
                    if !cycle_time_exceeded {
                        spin_sleeper.sleep(sleep_duration_hundred_millis);
                    }
                    cycle_time_exceeded = false;
                }
            }

            start_time = SystemTime::now();

            sql_refill_data.duration_since_last_refill =
                sql_refill_data.duration_since_last_refill + self.config.check_interval;
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        #[cfg(test)]
        println!(
            "{}: sending Quit confirmation to signal handler.",
            LOG_TARGET_REFILL
        );

        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!("sending quit confirmation back to signal handler failed ({e:?})");
            }
        };

        // this thread has channel connection to underlying threads (Controllino, Tank level switch).
        // These threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by signal_handler crate.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match rx_from_signal_handler.recv() {
                Ok(c) => {
                    self.lock_error_channel_receive_termination_command = false;
                    match c {
                        InternalCommand::Terminate => {
                            self.lock_warn_not_applicable_command = false;
                            true
                        }
                        _ => {
                            if !self.lock_warn_not_applicable_command {
                                warn!(
                                    target: LOG_TARGET_REFILL,
                                    "ignoring not applicable command {} from signal handler",
                                    c
                                );
                                self.lock_warn_not_applicable_command = true;
                            }
                            false
                        }
                    }
                }
                Err(e) => {
                    if !self.lock_error_channel_receive_termination_command {
                        error!(
                            target: LOG_TARGET_REFILL,
                            "receiving termination signal from signal handler failed ({e:?})"
                        );
                        self.lock_error_channel_receive_termination_command = true;
                    }
                    false
                }
            };
            spin_sleeper.sleep(sleep_duration_hundred_millis);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use all_asserts::{assert_ge, assert_le};
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, SystemTime};

    use crate::channel_content::InternalCommand;
    use crate::config::{read_config_file, read_config_file_with_test_database, ConfigData};
    use crate::proc_ext_req::ProcessExternalRequestTrait;
    use crate::refill::{Refill, RefillErrors};
    use crate::sql_interface::SqlInterface;
    use crate::sql_interface_refill::SqlInterfaceRefill;

    use crate::mock::tests::MockSignal::{
        TankLevelSwitchPosition, TankLevelSwitchPositionStabilized,
    };
    use crate::mock::tests::MockSqlInterfaceRefill;
    use crate::mock::tests::{
        mock_data_logger, mock_schedule_check, mock_tank_level_switch, MockSignal,
        MockWaterInjection, TestCommand,
    };
    use crate::refill::LOG_TARGET_REFILL;

    pub fn prepare_refill_tests(
        refill_errors: Option<RefillErrors>,
        initial_wait_interval: u64,
        check_interval: u64,
        test_db_number_opt: Option<u32>,
    ) -> (ConfigData, Refill, SqlInterface) {
        let config_file_name = "/config/aquarium_control_test_generic.toml";

        // prepare struct Refill for subsequent tests
        let mut config: ConfigData = match test_db_number_opt {
            Some(test_db_number) => {
                read_config_file_with_test_database(config_file_name.to_string(), test_db_number)
            }
            None => read_config_file(config_file_name.to_string()),
        };

        config.refill.initial_wait_interval = initial_wait_interval;
        config.refill.check_interval = check_interval;

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
            match refill_errors.is_some() {
                true => Refill::new_with_errors(
                    refill_config,
                    Box::new(sql_interface_refill),
                    refill_errors.unwrap(),
                ),
                false => Refill::new(refill_config, Box::new(sql_interface_refill)),
            },
            sql_interface,
        )
    }

    pub fn prepare_refill_tests_with_mock_database_interface(
        refill_errors: Option<RefillErrors>,
        initial_wait_interval: u64,
        check_interval: u64,
        test_db_number: u32,
    ) -> (ConfigData, Refill) {
        let config_file_name = "/config/aquarium_control_test_generic.toml";

        // prepare struct Refill for subsequent tests
        let mut config: ConfigData =
            read_config_file_with_test_database(config_file_name.to_string(), test_db_number);

        config.refill.initial_wait_interval = initial_wait_interval;
        config.refill.check_interval = check_interval;

        let sql_interface_refill = MockSqlInterfaceRefill {};

        let refill_config = config.refill.clone();
        (
            config,
            match refill_errors.is_some() {
                true => Refill::new_with_errors(
                    refill_config,
                    Box::new(sql_interface_refill),
                    refill_errors.unwrap(),
                ),
                false => Refill::new(refill_config, Box::new(sql_interface_refill)),
            },
        )
    }

    #[test]
    // Test case checks the implementation of ProcessExternalRequestTrait.
    // The test case does not require any communication with SQL database.
    pub fn test_refill_process_external_request_empty_channel() {
        let refill_errors = RefillErrors {
            error_switch_stuck_high: false,
            error_switch_stuck_low: false,
            error_receive_tank_level_switch_position: false,
            error_sql_update_failed: false,
            error_sql_get_historic_data_failed: false,
        };
        let (_, mut refill, _) = prepare_refill_tests(Some(refill_errors), 2, 60, None);

        let (_, rx_refill_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();

        let (_, rx_refill_from_messaging): (Sender<InternalCommand>, Receiver<InternalCommand>) =
            mpsc::channel();

        let (quit_command_received, start_command_received, stop_command_received) = refill
            .process_external_request(
                &rx_refill_from_signal_handler,
                Some(&rx_refill_from_messaging),
            );

        assert_eq!(quit_command_received, false);
        assert_eq!(start_command_received, false);
        assert_eq!(stop_command_received, false);
    }

    #[test]
    // This test case executes multiple iterations of the loop inside the .execute function with triggering a refill.
    // Test case uses test database #20.
    pub fn test_refill_trigger_water_injection() {
        let sleep_duration_50_millisec = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut refill, mut sql_interface) = prepare_refill_tests(None, 1, 1, Some(20));
        let (tx_refill_to_tank_level_switch, rx_tank_level_switch_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_data_logger, rx_data_logger_from_refill) = mpsc::channel::<bool>();
        let (tx_data_logger_to_refill, rx_refill_from_data_logger) =
            mpsc::channel::<InternalCommand>();
        let (tx_tank_level_switch_to_refill, rx_refill_from_tank_level_switch) =
            mpsc::channel::<bool>();
        let (tx_refill_to_controllino, _rx_controllino_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (_tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel::<bool>();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel::<bool>();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel::<bool>();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();
        let (
            tx_mock_water_injection_to_test_environment,
            rx_test_environment_from_mock_water_injection,
        ) = mpsc::channel();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mutex_device_scheduler = Arc::new(Mutex::new(0));

        let mut mock_water_injection =
            MockWaterInjection::new(Some(tx_mock_water_injection_to_test_environment));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_refill,
                    &rx_tank_level_switch_from_refill,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    false,
                    false,
                    false,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_refill,
                    &rx_data_logger_from_refill,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_secs = Duration::from_secs(2);
                let spin_sleeper = SpinSleeper::default();

                // wait for test object to trigger water injection
                let _ = rx_test_environment_from_mock_water_injection
                    .recv()
                    .unwrap();

                // request update of tank level position signals to mock tank level switch
                let _ = tx_test_environment_to_tank_level_switch
                    .send(TestCommand::UpdateSignal(TankLevelSwitchPosition(true)));
                let _ = tx_test_environment_to_tank_level_switch.send(TestCommand::UpdateSignal(
                    TankLevelSwitchPositionStabilized(true),
                ));

                let start_time = SystemTime::now();
                spin_sleeper.sleep(sleep_duration_two_secs);

                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();

                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);
                let finish_time = SystemTime::now();
                let finalization_duration_millis =
                    finish_time.duration_since(start_time).unwrap().as_millis();

                assert_ge!(finalization_duration_millis, 2000 as u128);
                assert_le!(finalization_duration_millis, 2150 as u128);

                // wait for test object to finish so that mock threads receive
                // termination request before end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive call for signal update
        spin_sleeper.sleep(sleep_duration_50_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();
                let tx_refill_to_tank_level_switch_for_test_case_finish =
                    tx_refill_to_tank_level_switch.clone();

                refill.execute(
                    mutex_device_scheduler,
                    tx_refill_to_tank_level_switch,
                    rx_refill_from_tank_level_switch,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_data_logger,
                    rx_refill_from_data_logger,
                    None,
                    &mut mock_water_injection,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_refill_to_tank_level_switch_for_test_case_finish.send(InternalCommand::Quit);
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // this test case executes one iteration of the loop inside the .execute function with triggering a refill
    // It checks the timing of initial wait period and main control interval.
    // Test case uses test database #21.
    pub fn test_refill_initial_waiting_time() {
        let sleep_duration_50_millisec = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();

        let (config, mut refill, mut sql_interface) = prepare_refill_tests(None, 2, 60, Some(21));

        let (tx_refill_to_tank_level_switch, rx_tank_level_switch_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_data_logger, rx_data_logger_from_refill) = mpsc::channel::<bool>();
        let (tx_data_logger_to_refill, rx_refill_from_data_logger) =
            mpsc::channel::<InternalCommand>();
        let (tx_tank_level_switch_to_refill, rx_refill_from_tank_level_switch) =
            mpsc::channel::<bool>();
        let (tx_refill_to_controllino, _) = mpsc::channel::<InternalCommand>();
        let (_tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel::<bool>();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel::<bool>();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel::<bool>();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();
        let (
            tx_mock_water_injection_to_test_environment,
            rx_test_environment_from_mock_water_injection,
        ) = mpsc::channel();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mutex_device_scheduler = Arc::new(Mutex::new(0));

        let mut mock_water_injection =
            MockWaterInjection::new(Some(tx_mock_water_injection_to_test_environment));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_refill,
                    &rx_tank_level_switch_from_refill,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    false,
                    false,
                    false,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_refill,
                    &rx_data_logger_from_refill,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let start_time = SystemTime::now();

                // wait for test object to trigger water injection
                let _ = rx_test_environment_from_mock_water_injection
                    .recv()
                    .unwrap();

                // record time after test object triggered water injection
                let initial_waiting_time_passed = SystemTime::now();

                // now we can send the quit command
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();

                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);
                let finish_time = SystemTime::now();
                let initial_wait_duration_millis = initial_waiting_time_passed
                    .duration_since(start_time)
                    .unwrap()
                    .as_millis();
                let finalization_duration_millis = finish_time
                    .duration_since(initial_waiting_time_passed)
                    .unwrap()
                    .as_millis();

                assert_le!(
                    initial_wait_duration_millis,
                    (config.refill.initial_wait_interval * 1000 + 200) as u128
                );
                assert_ge!(
                    initial_wait_duration_millis,
                    (config.refill.initial_wait_interval * 1000) as u128
                );
                assert_le!(finalization_duration_millis, 150 as u128);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive call for signal update
        spin_sleeper.sleep(sleep_duration_50_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();
                let tx_refill_to_tank_level_switch_for_test_case_finish =
                    tx_refill_to_tank_level_switch.clone();

                refill.execute(
                    mutex_device_scheduler,
                    tx_refill_to_tank_level_switch,
                    rx_refill_from_tank_level_switch,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_data_logger,
                    rx_refill_from_data_logger,
                    None,
                    &mut mock_water_injection,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_refill_to_tank_level_switch_for_test_case_finish.send(InternalCommand::Quit);
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // this test case executes multiple iterations of the loop of the .execute function with error flag error_switch_stuck_low set
    // during the execution of the test case, the error is reset.
    // refill control shall resum operation upon having reset the error.
    // Test case uses test database #22.
    pub fn test_refill_conditions_historic_data_error_switch_stuck_low() {
        let sleep_duration_50_millisec = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill, mut sql_interface) = prepare_refill_tests(None, 1, 1, Some(22));

        // set the error
        refill.refill_errors.error_switch_stuck_low = true;

        let (tx_refill_to_tank_level_switch, rx_tank_level_switch_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_data_logger, rx_data_logger_from_refill) = mpsc::channel::<bool>();
        let (tx_data_logger_to_refill, rx_refill_from_data_logger) =
            mpsc::channel::<InternalCommand>();
        let (tx_tank_level_switch_to_refill, rx_refill_from_tank_level_switch) =
            mpsc::channel::<bool>();
        let (tx_refill_to_controllino, _rx_controllino_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (_tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel::<bool>();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel::<bool>();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel::<bool>();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (tx_messaging_to_refill, rx_refill_from_messaging) = mpsc::channel::<InternalCommand>();
        let (tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();
        let (
            tx_mock_water_injection_to_test_environment,
            rx_test_environment_from_mock_water_injection,
        ) = mpsc::channel();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mutex_device_scheduler = Arc::new(Mutex::new(0));

        let mut mock_water_injection =
            MockWaterInjection::new(Some(tx_mock_water_injection_to_test_environment));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_refill,
                    &rx_tank_level_switch_from_refill,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    false,
                    false,
                    false,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_refill,
                    &rx_data_logger_from_refill,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_secs = Duration::from_secs(2);
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                spin_sleeper.sleep(sleep_duration_two_secs);

                for _ in 0..999 {
                    assert_eq!(
                        rx_test_environment_from_mock_water_injection.try_recv(),
                        Err(TryRecvError::Empty)
                    );
                    spin_sleeper.sleep(sleep_duration_two_millis);
                }

                // reset errors
                println!("resetting errors...");
                match tx_messaging_to_refill.send(InternalCommand::ResetAllErrors) {
                    Ok(_) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "test_refill_conditions_historic_data_error_recent_refill_event:\
                        error when trying to reset refill errors from previous test runs: {e:?}"
                        );
                    }
                }

                // wait for test object to trigger water injection
                println!("waiting for water injection...");
                let _ = rx_test_environment_from_mock_water_injection
                    .recv()
                    .unwrap();

                spin_sleeper.sleep(sleep_duration_two_secs);

                // request update of tank level position signals to mock tank level switch
                println!("updating tank level switch...");
                let _ = tx_test_environment_to_tank_level_switch
                    .send(TestCommand::UpdateSignal(TankLevelSwitchPosition(true)));
                let _ = tx_test_environment_to_tank_level_switch.send(TestCommand::UpdateSignal(
                    TankLevelSwitchPositionStabilized(true),
                ));

                spin_sleeper.sleep(sleep_duration_two_secs);

                println!("sending quit signal to test object...");
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();

                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);

                // wait for test object to finish so that mock threads receive
                // termination request before end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive call for signal update
        spin_sleeper.sleep(sleep_duration_50_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();
                let tx_refill_to_tank_level_switch_for_test_case_finish =
                    tx_refill_to_tank_level_switch.clone();

                refill.execute(
                    mutex_device_scheduler,
                    tx_refill_to_tank_level_switch,
                    rx_refill_from_tank_level_switch,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_data_logger,
                    rx_refill_from_data_logger,
                    Some(rx_refill_from_messaging),
                    &mut mock_water_injection,
                );

                // check if there was no water injection
                assert_eq!(mock_water_injection.injection_recorder.is_empty(), false);

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_refill_to_tank_level_switch_for_test_case_finish.send(InternalCommand::Quit);
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // This test case executes multiple iterations of the loop inside .execute function with error flag error_sql_update_failed
    // The refill control shall not initiate a refill operation.
    // Test case uses test database #23.
    pub fn test_refill_conditions_error_sql_update_failed() {
        let sleep_duration_50_millisec = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill, mut sql_interface) = prepare_refill_tests(None, 1, 1, Some(23));

        // set the error
        refill.refill_errors.error_sql_update_failed = true;

        let (tx_refill_to_tank_level_switch, rx_tank_level_switch_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_data_logger, rx_data_logger_from_refill) = mpsc::channel::<bool>();
        let (tx_data_logger_to_refill, rx_refill_from_data_logger) =
            mpsc::channel::<InternalCommand>();
        let (tx_tank_level_switch_to_refill, rx_refill_from_tank_level_switch) =
            mpsc::channel::<bool>();
        let (tx_refill_to_controllino, _rx_controllino_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (_tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel::<bool>();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel::<bool>();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel::<bool>();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();
        let (
            tx_mock_water_injection_to_test_environment,
            rx_test_environment_from_mock_water_injection,
        ) = mpsc::channel();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mutex_device_scheduler = Arc::new(Mutex::new(0));

        let mut mock_water_injection =
            MockWaterInjection::new(Some(tx_mock_water_injection_to_test_environment));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_refill,
                    &rx_tank_level_switch_from_refill,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    false,
                    false,
                    false,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_refill,
                    &rx_data_logger_from_refill,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        // thread for mock Test environment
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_secs = Duration::from_secs(2);
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                spin_sleeper.sleep(sleep_duration_two_secs);

                for _ in 0..999 {
                    assert_eq!(
                        rx_test_environment_from_mock_water_injection.try_recv(),
                        Err(TryRecvError::Empty)
                    );
                    spin_sleeper.sleep(sleep_duration_two_millis);
                }

                println!("sending quit signal to test object...");
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();

                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);

                // wait for test object to finish so that mock threads receive
                // termination request before end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive call for signal update
        spin_sleeper.sleep(sleep_duration_50_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();
                let tx_refill_to_tank_level_switch_for_test_case_finish =
                    tx_refill_to_tank_level_switch.clone();

                refill.execute(
                    mutex_device_scheduler,
                    tx_refill_to_tank_level_switch,
                    rx_refill_from_tank_level_switch,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_data_logger,
                    rx_refill_from_data_logger,
                    None,
                    &mut mock_water_injection,
                );

                // check if there was no water injection
                assert_eq!(mock_water_injection.injection_recorder.is_empty(), true);

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_refill_to_tank_level_switch_for_test_case_finish.send(InternalCommand::Quit);

                spin_sleeper.sleep(sleep_duration_two_millis);
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // This test case checks the process_external_request implementation if it correctly
    // processes the ResetAllErrors command.
    // The test case checks the struct attribute after having executed the function.
    // Test case uses test database #24.
    pub fn test_refill_process_external_request_reset_all_errors_command() {
        // set the errors
        let refill_errors = RefillErrors {
            error_switch_stuck_high: true,
            error_switch_stuck_low: true,
            error_receive_tank_level_switch_position: true,
            error_sql_update_failed: true,
            error_sql_get_historic_data_failed: true,
        };

        let (_, mut refill, mut sql_interface) =
            prepare_refill_tests(Some(refill_errors), 2, 60, Some(24));

        let sleep_duration_50_millisec = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();

        let (tx_refill_to_tank_level_switch, rx_tank_level_switch_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_data_logger, rx_data_logger_from_refill) = mpsc::channel::<bool>();
        let (tx_data_logger_to_refill, rx_refill_from_data_logger) =
            mpsc::channel::<InternalCommand>();
        let (tx_tank_level_switch_to_refill, rx_refill_from_tank_level_switch) =
            mpsc::channel::<bool>();
        let (tx_refill_to_controllino, _rx_controllino_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (_tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel::<bool>();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel::<bool>();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel::<bool>();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (tx_messaging_to_refill, rx_refill_from_messaging) = mpsc::channel::<InternalCommand>();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();
        let (
            tx_mock_water_injection_to_test_environment,
            _rx_test_environment_from_mock_water_injection,
        ) = mpsc::channel();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mutex_device_scheduler = Arc::new(Mutex::new(0));

        let mut mock_water_injection =
            MockWaterInjection::new(Some(tx_mock_water_injection_to_test_environment));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_refill,
                    &rx_tank_level_switch_from_refill,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    true,
                    false,
                    true,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_refill,
                    &rx_data_logger_from_refill,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_secs = Duration::from_secs(2);
                let spin_sleeper = SpinSleeper::default();

                spin_sleeper.sleep(sleep_duration_two_secs);
                assert_eq!(
                    tx_messaging_to_refill.send(InternalCommand::ResetAllErrors),
                    Ok(())
                );
                spin_sleeper.sleep(sleep_duration_two_secs);

                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();

                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);

                // wait for test object to finish so that mock threads receive
                // termination request before end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive call for signal update
        spin_sleeper.sleep(sleep_duration_50_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                // check if errors have been set
                assert_eq!(refill.refill_errors.has_error(), true);

                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();
                let tx_refill_to_tank_level_switch_for_test_case_finish =
                    tx_refill_to_tank_level_switch.clone();

                refill.execute(
                    mutex_device_scheduler,
                    tx_refill_to_tank_level_switch,
                    rx_refill_from_tank_level_switch,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_data_logger,
                    rx_refill_from_data_logger,
                    Some(rx_refill_from_messaging),
                    &mut mock_water_injection,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_refill_to_tank_level_switch_for_test_case_finish.send(InternalCommand::Quit);

                println!("{}", refill.refill_errors);

                // check if errors have been reset
                assert_eq!(refill.refill_errors.has_error(), false);

                spin_sleeper.sleep(sleep_duration_two_millis);
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Executes loop of the .execute function manipulating the sensor signals only and giving a negative schedule check.
    // Checks if the test object correctly processes the negative schedule check result.
    // Test case uses test database #25.
    pub fn test_refill_blocked_by_schedule() {
        let sleep_duration_50_millisec = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill, mut sql_interface) = prepare_refill_tests(None, 1, 1, Some(25));

        let (tx_refill_to_tank_level_switch, rx_tank_level_switch_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_data_logger, rx_data_logger_from_refill) = mpsc::channel::<bool>();
        let (tx_data_logger_to_refill, rx_refill_from_data_logger) =
            mpsc::channel::<InternalCommand>();
        let (tx_tank_level_switch_to_refill, rx_refill_from_tank_level_switch) =
            mpsc::channel::<bool>();
        let (tx_refill_to_controllino, _rx_controllino_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (_tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel::<bool>();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel::<bool>();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel::<bool>();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();
        let (
            tx_mock_water_injection_to_test_environment,
            rx_test_environment_from_mock_water_injection,
        ) = mpsc::channel();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mutex_device_scheduler = Arc::new(Mutex::new(0));

        let mut mock_water_injection =
            MockWaterInjection::new(Some(tx_mock_water_injection_to_test_environment));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    None,
                    false,
                );
            })
            .unwrap();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_refill,
                    &rx_tank_level_switch_from_refill,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    false,
                    false,
                    false,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_refill,
                    &rx_data_logger_from_refill,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_secs = Duration::from_secs(2);
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                spin_sleeper.sleep(sleep_duration_two_secs);

                for _ in 0..999 {
                    assert_eq!(
                        rx_test_environment_from_mock_water_injection.try_recv(),
                        Err(TryRecvError::Empty)
                    );
                    spin_sleeper.sleep(sleep_duration_two_millis);
                }

                println!("sending quit signal to test object...");
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();

                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);

                // wait for test object to finish so that mock threads receive
                // termination request before end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive call for signal update
        spin_sleeper.sleep(sleep_duration_50_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();
                let tx_refill_to_tank_level_switch_for_test_case_finish =
                    tx_refill_to_tank_level_switch.clone();

                refill.execute(
                    mutex_device_scheduler,
                    tx_refill_to_tank_level_switch,
                    rx_refill_from_tank_level_switch,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_data_logger,
                    rx_refill_from_data_logger,
                    None,
                    &mut mock_water_injection,
                );

                // check if there was no water injection
                assert_eq!(mock_water_injection.injection_recorder.is_empty(), true);

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_refill_to_tank_level_switch_for_test_case_finish.send(InternalCommand::Quit);

                spin_sleeper.sleep(sleep_duration_two_millis);
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case executes loop of the .execute function manipulating the sensor signals only
    // and giving a positive schedule check.
    // Test case checks if the test object correctly processes the inhibition requested via channel.
    // Test case uses test database #26.
    pub fn test_messaging_stops_starts_refill() {
        let sleep_duration_50_millisec = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill, mut sql_interface) = prepare_refill_tests(None, 10, 60, Some(26));

        let (tx_refill_to_tank_level_switch, rx_tank_level_switch_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_data_logger, rx_data_logger_from_refill) = mpsc::channel::<bool>();
        let (tx_data_logger_to_refill, rx_refill_from_data_logger) =
            mpsc::channel::<InternalCommand>();
        let (tx_tank_level_switch_to_refill, rx_refill_from_tank_level_switch) =
            mpsc::channel::<bool>();
        let (tx_refill_to_controllino, _rx_controllino_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (_tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel::<bool>();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel::<bool>();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel::<bool>();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (tx_messaging_to_refill, rx_refill_from_messaging) = mpsc::channel::<InternalCommand>();
        let (tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();
        let (
            tx_mock_water_injection_to_test_environment,
            rx_test_environment_from_mock_water_injection,
        ) = mpsc::channel();

        // empty the refill table
        match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let mutex_device_scheduler = Arc::new(Mutex::new(0));

        let mut mock_water_injection =
            MockWaterInjection::new(Some(tx_mock_water_injection_to_test_environment));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_refill,
                    &rx_tank_level_switch_from_refill,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    true,
                    false,
                    true,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_refill,
                    &rx_data_logger_from_refill,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_one_sec = Duration::from_secs(1);
                let sleep_duration_two_secs = Duration::from_secs(2);
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                spin_sleeper.sleep(sleep_duration_one_sec);

                match tx_messaging_to_refill.send(InternalCommand::Stop) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: could not send stop command to refill ({e:?})",
                            LOG_TARGET_REFILL
                        );
                    }
                }

                spin_sleeper.sleep(sleep_duration_one_sec);

                let _ = tx_test_environment_to_tank_level_switch.send(TestCommand::UpdateSignal(
                    MockSignal::TankLevelSwitchPosition(false),
                ));
                let _ = tx_test_environment_to_tank_level_switch.send(TestCommand::UpdateSignal(
                    MockSignal::TankLevelSwitchPositionStabilized(false),
                ));

                // expect to see no event
                for _ in 0..999 {
                    assert_eq!(
                        rx_test_environment_from_mock_water_injection.try_recv(),
                        Err(TryRecvError::Empty)
                    );
                    spin_sleeper.sleep(sleep_duration_two_millis);
                }

                spin_sleeper.sleep(sleep_duration_one_sec);

                match tx_messaging_to_refill.send(InternalCommand::Start) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: could not send start command to refill ({e:?})",
                            LOG_TARGET_REFILL
                        );
                    }
                }

                // expect to see one event
                assert_eq!(
                    rx_test_environment_from_mock_water_injection.recv(),
                    Ok(TestCommand::InformWaterInjectionEvent)
                );

                println!("sending quit signal to test object...");
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();

                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);

                // wait for test object to finish so that mock threads receive
                // termination request before end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive call for signal update
        spin_sleeper.sleep(sleep_duration_50_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();
                let tx_refill_to_tank_level_switch_for_test_case_finish =
                    tx_refill_to_tank_level_switch.clone();

                refill.execute(
                    mutex_device_scheduler,
                    tx_refill_to_tank_level_switch,
                    rx_refill_from_tank_level_switch,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_data_logger,
                    rx_refill_from_data_logger,
                    Some(rx_refill_from_messaging),
                    &mut mock_water_injection,
                );

                // check if there was only one water injection
                assert_eq!(mock_water_injection.injection_recorder.len(), 1);

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_refill_to_tank_level_switch_for_test_case_finish.send(InternalCommand::Quit);

                spin_sleeper.sleep(sleep_duration_two_millis);
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // This test case executes multiple iterations of the loop inside .execute function
    // using mock version of SQL interface which returns only errors.
    // The refill control shall not initiate a refill operation.
    // Test case uses test database #27.
    pub fn test_refill_conditions_error_get_historic_data_failed() {
        let sleep_duration_50_millisec = Duration::from_millis(50);
        let spin_sleeper = SpinSleeper::default();
        let (_, mut refill) = prepare_refill_tests_with_mock_database_interface(None, 1, 1, 27);
        // this test case does not use the SQL database, so no conflict arises
        // when concurrently executing other test cases on the same DB.

        let (tx_refill_to_tank_level_switch, rx_tank_level_switch_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_data_logger, rx_data_logger_from_refill) = mpsc::channel::<bool>();
        let (tx_data_logger_to_refill, rx_refill_from_data_logger) =
            mpsc::channel::<InternalCommand>();
        let (tx_tank_level_switch_to_refill, rx_refill_from_tank_level_switch) =
            mpsc::channel::<bool>();
        let (tx_refill_to_controllino, _rx_controllino_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (_tx_controllino_to_refill, rx_refill_from_controllino) = mpsc::channel::<bool>();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel::<InternalCommand>();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill) = mpsc::channel::<bool>();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check) = mpsc::channel::<bool>();
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill) =
            mpsc::channel::<InternalCommand>();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();
        let (
            tx_mock_water_injection_to_test_environment,
            rx_test_environment_from_mock_water_injection,
        ) = mpsc::channel();

        let mutex_device_scheduler = Arc::new(Mutex::new(0));

        let mut mock_water_injection =
            MockWaterInjection::new(Some(tx_mock_water_injection_to_test_environment));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_refill,
                    &rx_schedule_check_from_refill,
                    None,
                    true,
                );
            })
            .unwrap();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_refill,
                    &rx_tank_level_switch_from_refill,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    false,
                    false,
                    false,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_refill,
                    &rx_data_logger_from_refill,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        // thread for mock Test environment
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_two_secs = Duration::from_secs(2);
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                spin_sleeper.sleep(sleep_duration_two_secs);

                for _ in 0..999 {
                    assert_eq!(
                        rx_test_environment_from_mock_water_injection.try_recv(),
                        Err(TryRecvError::Empty)
                    );
                    spin_sleeper.sleep(sleep_duration_two_millis);
                }

                println!("sending quit signal to test object...");
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                rx_signal_handler_from_refill.recv().unwrap();

                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);

                // wait for test object to finish so that mock threads receive
                // termination request before end of this (test environment) thread
                spin_sleeper.sleep(sleep_duration_two_secs);
            })
            .unwrap();

        // give the test environment a little time to trigger the receive call for signal update
        spin_sleeper.sleep(sleep_duration_50_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let sleep_duration_two_millis = Duration::from_millis(2);
                let spin_sleeper = SpinSleeper::default();

                let tx_refill_to_schedule_check_for_test_case_finish =
                    tx_refill_to_schedule_check.clone();
                let tx_refill_to_tank_level_switch_for_test_case_finish =
                    tx_refill_to_tank_level_switch.clone();

                refill.execute(
                    mutex_device_scheduler,
                    tx_refill_to_tank_level_switch,
                    rx_refill_from_tank_level_switch,
                    tx_refill_to_controllino,
                    rx_refill_from_controllino,
                    tx_refill_to_signal_handler,
                    rx_refill_from_signal_handler,
                    tx_refill_to_schedule_check,
                    rx_refill_from_schedule_check,
                    tx_refill_to_data_logger,
                    rx_refill_from_data_logger,
                    None,
                    &mut mock_water_injection,
                );

                // check if there was no water injection
                assert_eq!(mock_water_injection.injection_recorder.is_empty(), true);

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_refill_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_refill_to_tank_level_switch_for_test_case_finish.send(InternalCommand::Quit);

                spin_sleeper.sleep(sleep_duration_two_millis);
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }
}
