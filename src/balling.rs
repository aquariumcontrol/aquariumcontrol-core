/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::Local;
use log::{error, warn};

#[cfg(feature = "debug_balling")]
use log::debug;

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::channel_content::{AquariumDevice, InternalCommand};
use crate::mineral_injection::MineralInjectionTrait;
use crate::proc_ext_req::ProcessExternalRequestTrait;
use crate::sql_interface_balling::{BallingSetVal, SqlInterfaceBalling};

const LOG_TARGET_BALLING: &str = "balling";

/// Holds the configuration data for the balling dosing control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct BallingConfig {
    /// control will only be active when the flag is set to true
    active: bool,

    /// pump #1 will only be used when the flag is set to true
    pump1_active: bool,

    /// pump #2 will only be used when the flag is set to true
    pump2_active: bool,

    /// pump #3 will only be used when the flag is set to true
    pump3_active: bool,

    /// pump #4 will only be used when the flag is set to true
    pump4_active: bool,

    /// interval for the main control in seconds
    schedule_check_interval: u32,

    /// inject Balling minerals with pump 1 after each period    
    dosing_interval_pump1: u32,

    /// inject Balling minerals with pump 2 after each period    
    dosing_interval_pump2: u32,

    /// inject Balling minerals with pump 3 after each period    
    dosing_interval_pump3: u32,

    /// inject Balling minerals with pump 4 after each period    
    dosing_interval_pump4: u32,

    /// wait for a fixed period between dosings from different pumps
    pub pump_switch_delay_millis: u32,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the Balling dosing control.
/// Thread communication of this component is as follows:
/// ```mermaid

/// graph LR
///     balling[Balling dosing control] --> signal_handler[Signal handler]
///     signal_handler --> balling
///     signal_handler --> mineral_injection[Mineral injection]
///     balling --> mineral_injection
///     mineral_injection --> controllino[Controllino]
///     controllino --> mineral_injection
///     balling --> schedule_check[Schedule check]
///     schedule_check --> balling
///	    messaging[Messaging] --> balling
/// ```
/// Communication channel to and from Controllino is forwarded to implementation of MineralInjectionTrait.
/// Signal handler is communicating both directly with balling as well with implementation of MineralInjectionTrait.

pub struct Balling {
    /// configuration data for Balling dosing control
    config: BallingConfig,

    /// SQL database interface for Balling dosing control
    sql_interface_balling: SqlInterfaceBalling,

    /// Count down in seconds until next Balling dosing for pump 1
    countdown_dosing_pump1: u32,

    /// Count down in seconds until next Balling dosing for pump 2
    countdown_dosing_pump2: u32,

    /// Count down in seconds until next Balling dosing for pump 3
    countdown_dosing_pump3: u32,

    /// Count down in seconds until next Balling dosing for pump 4
    countdown_dosing_pump4: u32,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via channel
    lock_error_channel_receive_termination_signal: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via channel
    lock_warn_inapplicable_command: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages to send request via channel to schedule check
    lock_error_channel_send_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages to receive request via channel from schedule check
    lock_error_channel_receive_schedule_check: bool,
}

impl ProcessExternalRequestTrait for Balling {}

impl Balling {
    /// Provides a struct of type Balling.
    /// # Arguments
    /// * `config` - configuration data for Balling dosing control
    /// * `sql_interface_balling` - specific SQL interface for Balling dosing control           
    pub fn new(config: BallingConfig, mut sql_interface_balling: SqlInterfaceBalling) -> Balling {
        if config.schedule_check_interval == 0 {
            panic!(
                "{}: schedule check interval is zero - please assign a value > 0",
                LOG_TARGET_BALLING
            );
        }

        let countdown_dosing_pump1 =
            Self::calc_countdown_next_dosing(&config, &mut sql_interface_balling, 1);
        let countdown_dosing_pump2 =
            Self::calc_countdown_next_dosing(&config, &mut sql_interface_balling, 2);
        let countdown_dosing_pump3 =
            Self::calc_countdown_next_dosing(&config, &mut sql_interface_balling, 3);
        let countdown_dosing_pump4 =
            Self::calc_countdown_next_dosing(&config, &mut sql_interface_balling, 4);

        Balling {
            config: config,
            sql_interface_balling: sql_interface_balling,
            countdown_dosing_pump1: countdown_dosing_pump1,
            countdown_dosing_pump2: countdown_dosing_pump2,
            countdown_dosing_pump3: countdown_dosing_pump3,
            countdown_dosing_pump4: countdown_dosing_pump4,
            lock_error_channel_receive_termination_signal: false,
            lock_warn_inapplicable_command: false,
            lock_error_channel_send_schedule_check: false,
            lock_error_channel_receive_schedule_check: false,
        }
    }

    /// Retrieves duration since last dosing event from database and calculates
    /// # Arguments
    /// the remaining time in seconds until next dosing for a specific pump
    /// * `config` - reference to configuration data for Balling dosing control       
    /// * `sql_interface_balling` - reference to specific SQL interface for Balling dosing control           
    /// * `pump_id` - id of pump for which the count down shall be calculated
    fn calc_countdown_next_dosing(
        config: &BallingConfig,
        sql_interface_balling: &mut SqlInterfaceBalling,
        pump_id: i64,
    ) -> u32 {
        let target_dosing_interval = match pump_id {
            1 => config.dosing_interval_pump1,
            2 => config.dosing_interval_pump2,
            3 => config.dosing_interval_pump3,
            4 => config.dosing_interval_pump4,
            _ => {
                // Panic is happening at startup of system.
                panic!(
                    "{}: countdown calculation encountered invalid pump id = {}",
                    LOG_TARGET_BALLING, pump_id
                );
            }
        };
        if target_dosing_interval == 0 {
            panic!(
                "{}: Invalid dosing interval for pump #{} in configuration.",
                LOG_TARGET_BALLING, pump_id
            );
        }

        let duration_since_balling_dosing: u32 = match sql_interface_balling
            .get_duration_since_last_balling_dosing(pump_id)
        {
            Ok(duration_opt) => match duration_opt {
                Some(duration) => match duration.try_into() {
                    Ok(c) => c,
                    Err(e) => {
                        error!("{}: Reading last dosing event duration for pump {} from database ({e:?})",
                            LOG_TARGET_BALLING,
                            pump_id);
                        0
                    }
                },
                None => target_dosing_interval.into(),
            },
            Err(e) => {
                panic!(
                    "{}: Could not get duration since last Balling dosing event: {e:?}",
                    LOG_TARGET_BALLING
                );
            }
        };

        #[cfg(test)]
        println!(
            "{}, Duration since last dosing for pump {} is {} seconds.",
            LOG_TARGET_BALLING, pump_id, duration_since_balling_dosing
        );

        // limit subtraction to 0
        let countdown = match target_dosing_interval.checked_sub(duration_since_balling_dosing) {
            None => 0,
            Some(diff) => diff,
        };

        #[cfg(test)]
        println!(
            "{}: Countdown for next dosing of pump {} is {} seconds.",
            LOG_TARGET_BALLING, pump_id, countdown
        );

        countdown
    }

    /// Calculates target dosing duration based on set values read from database.
    /// # Arguments
    /// * `balling_setval` - struct of type BallingSetVal containing flow rate of pump and target dosing volume
    fn calc_target_dosing_duration_millis(balling_setval: &BallingSetVal) -> u32 {
        if balling_setval.dosing_speed > 0.0 && balling_setval.dosing_volume > 0.0 {
            let dosing_duration_seconds =
                balling_setval.dosing_volume / balling_setval.dosing_speed;
            (dosing_duration_seconds * 1000.0) as u32
        } else {
            0 // user set an invalid flow rate or invalid dosing volume -> no dosing
        }
    }

    /// Calculates actual dosing volume based on actual dosing duration and pump flow.
    /// # Arguments
    /// * `balling_setval` - struct of type BallingSetVal containing flow rate of pump and target dosing volume
    /// * `actual_dosing_duration_millis` - actual dosing duration in milliseconds
    fn calc_actual_dosing_volume(
        balling_setval: &BallingSetVal,
        actual_dosing_duration_millis: u32,
    ) -> f32 {
        if balling_setval.dosing_speed > 0.0 && actual_dosing_duration_millis > 0 {
            (actual_dosing_duration_millis as f32) / 1000.0 * balling_setval.dosing_speed
        } else {
            0.0 // user set an invalid flow rate or invalid dosing volume -> no dosing
        }
    }

    /// Executes the dosing for a specific pump including
    /// - reading set values from data base
    /// - invoking mineral injection
    /// - writing result back to data base
    /// # Arguments
    /// * `pump_id` - id of the pump to be actuated.
    /// * `pump_device` - value of enum AquariumDevice corresponding to the pump id
    /// * `mineral_injection` - refence to struct implementing MineralInjectionTrait
    /// * `tx_to_controllino` - sender part of channel communication to send command to Controllino
    /// * `rx_from_controllino` - receiver part of channel communication to receive confirmation back from Controllino
    fn execute_dosing(
        &mut self,
        pump_id: u32,
        pump_device: AquariumDevice,
        mineral_injection: &mut impl MineralInjectionTrait,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        tx_to_controllino: &Sender<InternalCommand>,
        rx_from_controllino: &Receiver<bool>,
    ) -> bool {
        let mut quit_command_received: bool = false;
        let actual_dosing_duration_millis: u32;

        match self
            .sql_interface_balling
            .get_single_balling_setval_from_database(pump_id.into())
        {
            Ok(balling_setval) => {
                let dosing_duration_millis =
                    Self::calc_target_dosing_duration_millis(&balling_setval);

                #[cfg(test)]
                println!(
                    "{}: Calculated target dosing duration of {} ms for pump #{}",
                    LOG_TARGET_BALLING, dosing_duration_millis, pump_id
                );

                if dosing_duration_millis > 0 {
                    (quit_command_received, actual_dosing_duration_millis) = mineral_injection
                        .inject_mineral(
                            tx_to_controllino,
                            rx_from_controllino,
                            rx_from_signal_handler,
                            pump_device,
                            dosing_duration_millis,
                        );
                    let actual_dosing_volume = Self::calc_actual_dosing_volume(
                        &balling_setval,
                        actual_dosing_duration_millis,
                    );
                    _ = self.sql_interface_balling.insert_balling_event(
                        Local::now().naive_local(),
                        pump_id.into(),
                        actual_dosing_volume.into(),
                    );
                }
            }
            Err(e) => {
                error!(
                    target: LOG_TARGET_BALLING,
                    "encountered error when reading set values of pump #{}: {e:?}",
                    pump_id
                );
            }
        }
        quit_command_received
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `mutex_device_scheduler_balling` - clone of the mutex for avoiding parallel device actuation.
    /// * `tx_to_controllino` - sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - receiver part of channel for communication from Controllino
    /// * `tx_to_signal_handler` - sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - receiver part of channel for communication from signal handler                
    /// * `tx_to_schedule_check` - sender part of channel for communication to schedule checker
    /// * `rx_from_schedule_check` - receiver part of channel for communication from schedule checker
    /// * `rx_from_messaging_opt` - receiver part of channel communication wrapped in Option from Messaging
    /// * `mineral_injection`- reference to struct implementing MineralInjectionTrait
    pub fn execute(
        &mut self,
        mutex_device_scheduler_balling: Arc<Mutex<i32>>,
        tx_to_controllino: Sender<InternalCommand>,
        rx_from_controllino: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
        tx_to_schedule_check: Sender<InternalCommand>,
        rx_from_schedule_check: Receiver<bool>,
        rx_from_messaging_opt: Option<Receiver<InternalCommand>>,
        mineral_injection: &mut impl MineralInjectionTrait,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100); // used to reduce frequency of channel polling
        let sleep_duration_pump_switch = Duration::from_millis(self.config.pump_switch_delay_millis.into()); // wait between actuation of pumps
        let spin_sleeper = SpinSleeper::default();
        let mut loop_counter = 0; // used to reduce the frequency of DB requests
        let mut quit_command_received: bool; // request to end the application has been received
        let mut stop_command_received: bool; // request to (temporarily) stop dosing has been received
        let mut start_command_received: bool; // request to (temporarily) stop dosing has been received
        let mut dosing_inhibited: bool = false; // state of dosing control determined if start/stop command has been received
        let mut schedule_check_result = false; // flag indicating if operation of Balling is permitted at this time of day

        loop {
            if self.config.active
                && (loop_counter % (self.config.schedule_check_interval * 10) == 0)
            {
                // this code only executes when .toml file is configure accordingly (.active)
                // and the monitoring period has passed (usually every 60secs)

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
                                    error!(target: LOG_TARGET_BALLING,
                                        "Error when reading channel content for schedule check ({e:?})",
                                    );
                                    self.lock_error_channel_receive_schedule_check = true;
                                }
                                false // assume we are not allowed to actuate when communication is not successful
                            }
                        };
                        self.lock_error_channel_send_schedule_check = false;
                    }
                    Err(e) => {
                        if !self.lock_error_channel_send_schedule_check {
                            error!(target: LOG_TARGET_BALLING,
                                "Error in channel communication to schedule check: {e:?}"
                            );
                            self.lock_error_channel_send_schedule_check = true;
                        }
                    }
                }

                // debug information
                #[cfg(feature = "debug_balling")]
                {
                    // output countdowns every 10 seconds
                    debug!(
                        target: LOG_TARGET_BALLING,
                        "countdown 1 = {}, countdown 2 = {}, countdown 3 = {}, countdown 4 = {}",
                        self.countdown_dosing_pump1,
                        self.countdown_dosing_pump2,
                        self.countdown_dosing_pump3,
                        self.countdown_dosing_pump4,
                    );
                }

                //*** Peristaltic pump 1 ***
                if self.countdown_dosing_pump1 == 0 && self.config.pump1_active {
                    if schedule_check_result && !dosing_inhibited {
                        // inner scope to limit lifetime of unlocked mutex
                        {
                            let mut mutex_data = mutex_device_scheduler_balling.lock().unwrap();
                            quit_command_received = self.execute_dosing(
                                1,
                                AquariumDevice::PeristalticPump1,
                                mineral_injection,
                                &rx_from_signal_handler,
                                &tx_to_controllino,
                                &rx_from_controllino,
                            );
                            *mutex_data += 1;
                        }
                        if quit_command_received {
                            break; // Quit command received during dosing
                        }
                        self.countdown_dosing_pump1 = self.config.dosing_interval_pump1;
                    }
                } else {
                    if self.countdown_dosing_pump1 > 0 {
                        self.countdown_dosing_pump1 = self
                            .countdown_dosing_pump1
                            .saturating_sub(self.config.schedule_check_interval);
                    }
                }
                
                spin_sleeper.sleep(sleep_duration_pump_switch);
                
                //*** Peristaltic pump 2 ***
                if self.countdown_dosing_pump2 == 0 && self.config.pump2_active {
                    if schedule_check_result && !dosing_inhibited {
                        // inner scope to limit lifetime of unlocked mutex
                        {
                            let mut mutex_data = mutex_device_scheduler_balling.lock().unwrap();
                            quit_command_received = self.execute_dosing(
                                2,
                                AquariumDevice::PeristalticPump2,
                                mineral_injection,
                                &rx_from_signal_handler,
                                &tx_to_controllino,
                                &rx_from_controllino,
                            );
                            *mutex_data += 1;
                        }
                        if quit_command_received {
                            break; // Quit command received during dosing
                        }
                        self.countdown_dosing_pump2 = self.config.dosing_interval_pump2;
                    }
                } else {
                    if self.countdown_dosing_pump2 > 0 {
                        self.countdown_dosing_pump2 = self
                            .countdown_dosing_pump2
                            .saturating_sub(self.config.schedule_check_interval);
                    }
                }
                
                spin_sleeper.sleep(sleep_duration_pump_switch);
                
                //*** Peristaltic pump 3 ***
                if self.countdown_dosing_pump3 == 0 && self.config.pump3_active {
                    if schedule_check_result && !dosing_inhibited {
                        // inner scope to limit lifetime of unlocked mutex
                        {
                            let mut mutex_data = mutex_device_scheduler_balling.lock().unwrap();
                            quit_command_received = self.execute_dosing(
                                3,
                                AquariumDevice::PeristalticPump3,
                                mineral_injection,
                                &rx_from_signal_handler,
                                &tx_to_controllino,
                                &rx_from_controllino,
                            );
                            *mutex_data += 1;
                        }
                        if quit_command_received {
                            break; // Quit command received during dosing
                        }
                        self.countdown_dosing_pump3 = self.config.dosing_interval_pump3;
                    }
                } else {
                    if self.countdown_dosing_pump3 > 0 {
                        self.countdown_dosing_pump3 = self
                            .countdown_dosing_pump3
                            .saturating_sub(self.config.schedule_check_interval);
                    }
                }
                
                spin_sleeper.sleep(sleep_duration_pump_switch);                
                
                //*** Peristaltic pump 4 ***
                if self.countdown_dosing_pump4 == 0 && self.config.pump4_active {
                    if schedule_check_result && !dosing_inhibited {
                        // inner scope to limit lifetime of unlocked mutex
                        {
                            let mut mutex_data = mutex_device_scheduler_balling.lock().unwrap();
                            quit_command_received = self.execute_dosing(
                                4,
                                AquariumDevice::PeristalticPump4,
                                mineral_injection,
                                &rx_from_signal_handler,
                                &tx_to_controllino,
                                &rx_from_controllino,
                            );
                            *mutex_data += 1;
                        }
                        if quit_command_received {
                            break; // Quit command received during dosing
                        }
                        self.countdown_dosing_pump4 = self.config.dosing_interval_pump4;
                    }
                } else {
                    if self.countdown_dosing_pump4 > 0 {
                        self.countdown_dosing_pump4 = self
                            .countdown_dosing_pump4
                            .saturating_sub(self.config.schedule_check_interval);
                    }
                }
            }

            (
                quit_command_received,
                start_command_received,
                stop_command_received,
            ) = self
                .process_external_request(&rx_from_signal_handler, rx_from_messaging_opt.as_ref());
            if quit_command_received {
                #[cfg(test)]
                println!("{}: received Quit command. Exiting...", LOG_TARGET_BALLING);

                break;
            }
            if stop_command_received {
                #[cfg(test)]
                println!(
                    "{}: received Stop command. Inhibiting dosing.",
                    LOG_TARGET_BALLING
                );

                dosing_inhibited = true;
            }
            if start_command_received {
                #[cfg(test)]
                println!(
                    "{}: received Start command. (Re-)starting dosing.",
                    LOG_TARGET_BALLING
                );

                dosing_inhibited = false;
            }
            spin_sleeper.sleep(sleep_duration_hundred_millis);

            loop_counter += 1;
        }
        // Application received request to terminate, that is why the loop was left.
        // Answer to thread which sent the request for termination, so that shutdown can proceed further.
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_BALLING,
                    "Error in trying to send quit confirmation back to signal handler: {e:?}"
                );
            }
        };

        // This thread has channel connection to underlying threads (Controllino).
        // These threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by signal_handler crate.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match rx_from_signal_handler.recv() {
                Ok(c) => {
                    self.lock_error_channel_receive_termination_signal = false;
                    match c {
                        InternalCommand::Terminate => {
                            self.lock_warn_inapplicable_command = false;
                            true
                        }
                        _ => {
                            if !self.lock_warn_inapplicable_command {
                                warn!(
                                    target: LOG_TARGET_BALLING,
                                    "ignoring inapplicable command {} from signal handler.", c);
                                self.lock_warn_inapplicable_command = true;
                            }
                            false
                        }
                    }
                }
                Err(e) => {
                    if !self.lock_error_channel_receive_termination_signal {
                        error!(target: LOG_TARGET_BALLING,
                            "error when trying to receive termination signal. ({e:?})");
                        self.lock_error_channel_receive_termination_signal = true;
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
    use spin_sleep::SpinSleeper;
    use std::fmt;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::Duration;

    use crate::balling::{Balling, LOG_TARGET_BALLING};
    use crate::channel_content::{AquariumDevice, InternalCommand};
    use crate::config::{read_config_file, read_config_file_with_test_database, ConfigData};
    use crate::mineral_injection::MineralInjectionTrait;
    use crate::sql_interface::SqlInterface;
    use crate::sql_interface_balling::{BallingSetVal, SqlInterfaceBalling};

    use crate::mock::tests::mock_schedule_check;

    pub struct MockMineralInjection {
        pump1_engagement_durations: Vec<u32>,
        pump2_engagement_durations: Vec<u32>,
        pump3_engagement_durations: Vec<u32>,
        pump4_engagement_durations: Vec<u32>,
    }

    impl MockMineralInjection {
        pub fn new() -> MockMineralInjection {
            MockMineralInjection {
                pump1_engagement_durations: Vec::new(),
                pump2_engagement_durations: Vec::new(),
                pump3_engagement_durations: Vec::new(),
                pump4_engagement_durations: Vec::new(),
            }
        }
    }

    impl MineralInjectionTrait for MockMineralInjection {
        fn inject_mineral(
            &mut self,
            _tx_to_controllino: &Sender<InternalCommand>,
            _rx_from_controllino: &Receiver<bool>,
            _rx_from_signal_handler: &Receiver<InternalCommand>,
            mineral_pump: AquariumDevice,
            dosing_duration_millis: u32,
        ) -> (bool, u32) {
            // do nothing except for recording the dosing duration for the specific pump
            match mineral_pump {
                AquariumDevice::PeristalticPump1 => {
                    self.pump1_engagement_durations.push(dosing_duration_millis);
                }
                AquariumDevice::PeristalticPump2 => {
                    self.pump2_engagement_durations.push(dosing_duration_millis);
                }
                AquariumDevice::PeristalticPump3 => {
                    self.pump3_engagement_durations.push(dosing_duration_millis);
                }
                AquariumDevice::PeristalticPump4 => {
                    self.pump4_engagement_durations.push(dosing_duration_millis);
                }
                _ => {
                    panic!("inject_mineral (mock) invoked with invalid device.");
                }
            }
            println!(
                "inject_mineral (mock): recording {}ms engagement for {}",
                dosing_duration_millis, mineral_pump,
            );
            (false, dosing_duration_millis)
        }
    }

    fn read_balling_config() -> ConfigData {
        read_config_file("/config/aquarium_control_test_generic.toml".to_string())
    }

    fn read_balling_config_with_database(db_number: u32) -> ConfigData {
        read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            db_number            
        )
    }

    fn output_vector(id: u32, v: &Vec<u32>) -> String {
        let mut output: String = "Pump #".into();
        if v.is_empty() {
            output += &id.to_string();
            output += " has not been engaged.";
        } else {
            output += &id.to_string();
            output += " engagement duration(s) (ms): ";
            for duration in v {
                output += &duration.to_string();
                output += " ";
            }
        }
        output
    }

    impl fmt::Display for MockMineralInjection {
        /// Provides output of the struct debugging purposes.
        /// # Arguments
        /// * `f` - configuration for formatting
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(
                f,
                "{}\n{}\n{}\n{}",
                output_vector(1, &self.pump1_engagement_durations),
                output_vector(2, &self.pump2_engagement_durations),
                output_vector(3, &self.pump3_engagement_durations),
                output_vector(4, &self.pump4_engagement_durations)
            )
        }
    }

    #[test]
    #[should_panic]
    // This test case checks if the configuration file contains a valid value for the dosing interval.
    // It does not operate on any data from SQL database.
    pub fn test_balling_initialization_invalid_intervals() {
        let mut config: ConfigData = read_balling_config();

        config.balling.dosing_interval_pump1 = 0;
        config.balling.dosing_interval_pump2 = 0;
        config.balling.dosing_interval_pump3 = 0;
        config.balling.dosing_interval_pump4 = 0;

        println!("Testing with database {}", config.sql_interface.db_name);
        let sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_balling = SqlInterfaceBalling::new(sql_interface.get_connection());

        // create test object
        let _ = Balling::new(config.balling, sql_interface_balling);
    }

    #[test]
    // When starting, the application checks when the last dosings took place by analyzing data from SQL database.
    // As prerequisite for this test case, an empty empty dosing log is required.
    // Test cases uses test database #01.
    pub fn test_balling_initialization_empty_dosing_log() {
        let config: ConfigData = read_balling_config_with_database(1);

        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_balling = SqlInterfaceBalling::new(sql_interface.get_connection());

        // empty the Balling dosing log table
        match SqlInterface::truncate_table(&mut sql_interface, "ballingdosinglog".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        // create test object
        let balling = Balling::new(config.balling, sql_interface_balling);

        // check if the countdown calculation was correct
        assert_eq!(balling.countdown_dosing_pump1, 0);
        assert_eq!(balling.countdown_dosing_pump2, 0);
        assert_eq!(balling.countdown_dosing_pump3, 0);
        assert_eq!(balling.countdown_dosing_pump4, 0);
    }

    // helper function to prepare for test cases which require a filled dosing log
    // This function inserts dosing events for each of the four pumps.
    // The time of the dosing events is set to
    // - 100 seconds in the past for pump #1
    // - 200 seconds in the past for pump #2
    // - 300 seconds in the past for pump #3
    // - 400 seconds in the past for pump #4
    fn insert_past_dosing_events(
        sql_interface: &mut SqlInterface,
        sql_interface_balling: &mut SqlInterfaceBalling,
    ) {
        // empty the Balling dosing log table
        match SqlInterface::truncate_table(sql_interface, "ballingdosinglog".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }
        let naive_datetime_100secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 100);
        let naive_datetime_200secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 200);
        let naive_datetime_300secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 300);
        let naive_datetime_400secs_ago = SqlInterface::get_naive_timestamp_from_past(0, 400);

        // insert balling dosing events
        match sql_interface_balling.insert_balling_event(naive_datetime_100secs_ago, 1, 0.5) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not insert Balling dosing event for pump #1 into database: {e:?}")
            }
        };
        match sql_interface_balling.insert_balling_event(naive_datetime_200secs_ago, 2, 0.5) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not insert Balling dosing event for pump #2 into database: {e:?}")
            }
        };
        match sql_interface_balling.insert_balling_event(naive_datetime_300secs_ago, 3, 0.5) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not insert Balling dosing event for pump #3 into database: {e:?}")
            }
        };
        match sql_interface_balling.insert_balling_event(naive_datetime_400secs_ago, 4, 0.5) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not insert Balling dosing event for pump #4 into database: {e:?}")
            }
        };
    }

    #[test]
    // Test case checks if the countdowns for each pump are calculated correctly using
    // a dosing log in SQL database which has entries for all pumps.
    // Test case uses test database #02.
    pub fn test_balling_initialization_full_dosing_log() {
        let config: ConfigData = read_balling_config_with_database(2);

        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let mut sql_interface_balling = SqlInterfaceBalling::new(sql_interface.get_connection());

        insert_past_dosing_events(&mut sql_interface, &mut sql_interface_balling);

        // create test object
        let balling = Balling::new(config.balling, sql_interface_balling);

        // check if the countdown calculation was correct:
        // the countdown depends on the pump specific dosing interval and the timing of the last dosing event.
        assert_eq!(balling.countdown_dosing_pump1, 100);
        assert_eq!(balling.countdown_dosing_pump2, 0);
        assert_eq!(balling.countdown_dosing_pump3, 0);
        assert_eq!(balling.countdown_dosing_pump4, 0);
    }

    // inserts a set of pump configurations to the database
    fn insert_pump_configurations(
        sql_interface: &mut SqlInterface,
        sql_interface_balling: &mut SqlInterfaceBalling,
        pump1: bool,
        pump2: bool,
        pump3: bool,
        pump4: bool,
    ) {
        // empty the Balling pump configuration table
        match SqlInterface::truncate_table(sql_interface, "ballingsetvals".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }
        if pump1 {
            match sql_interface_balling.insert_pump_configuration(1, 4.0, 1.0, "pump1".to_string())
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: Could not insert pump 1 configuration ({e:?})",
                        LOG_TARGET_BALLING,
                    );
                }
            }
        }
        if pump2 {
            match sql_interface_balling.insert_pump_configuration(2, 4.0, 1.0, "pump2".to_string())
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: Could not insert pump 2 configuration ({e:?})",
                        LOG_TARGET_BALLING,
                    );
                }
            }
        }
        if pump3 {
            match sql_interface_balling.insert_pump_configuration(3, 4.0, 1.0, "pump3".to_string())
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: Could not insert pump 3 configuration ({e:?})",
                        LOG_TARGET_BALLING,
                    );
                }
            }
        }
        if pump4 {
            match sql_interface_balling.insert_pump_configuration(4, 4.0, 1.0, "pump4".to_string())
            {
                Ok(()) => { /* do nothing */ }
                Err(e) => {
                    panic!(
                        "{}: Could not insert pump 4 configuration ({e:?})",
                        LOG_TARGET_BALLING,
                    );
                }
            }
        }
    }

    // This function executes the test object and test environment in separate threads.
    // The test case depends on the configuration and reference values provided for asserts.
    pub fn test_balling_dosing(
        test_db_number: u32,
        schedule_allows: bool,
        execution_count_reference: i32,
        duration_millis: u64,
        pump1_engagement_durations_reference: usize,
        pump2_engagement_durations_reference: usize,
        pump3_engagement_durations_reference: usize,
        pump4_engagement_durations_reference: usize,
        schedule_check_interval: u32,
        dosing_intervals: Option<(u32, u32, u32, u32)>,
    ) {
        let sleep_duration_test_environment = Duration::from_millis(duration_millis);
        let spin_sleeper_test_environment = SpinSleeper::default();
        let mut config: ConfigData =
            read_balling_config_with_database(test_db_number);

        config.balling.pump1_active = pump1_engagement_durations_reference > 0;
        config.balling.pump2_active = pump2_engagement_durations_reference > 0;
        config.balling.pump3_active = pump3_engagement_durations_reference > 0;
        config.balling.pump4_active = pump4_engagement_durations_reference > 0;
        config.balling.schedule_check_interval = schedule_check_interval;

        match dosing_intervals {
            Some((
                dosing_interval_pump1,
                dosing_interval_pump2,
                dosing_interval_pump3,
                dosing_interval_pump4,
            )) => {
                config.balling.dosing_interval_pump1 = dosing_interval_pump1;
                config.balling.dosing_interval_pump2 = dosing_interval_pump2;
                config.balling.dosing_interval_pump3 = dosing_interval_pump3;
                config.balling.dosing_interval_pump4 = dosing_interval_pump4;
            }
            None => { /* do nothing */ }
        }

        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let mut sql_interface_balling = SqlInterfaceBalling::new(sql_interface.get_connection());

        insert_pump_configurations(
            &mut sql_interface,
            &mut sql_interface_balling,
            pump1_engagement_durations_reference > 0,
            pump2_engagement_durations_reference > 0,
            pump3_engagement_durations_reference > 0,
            pump4_engagement_durations_reference > 0,
        );
        insert_past_dosing_events(&mut sql_interface, &mut sql_interface_balling);

        let (tx_balling_to_controllino, _rx_controllino_from_balling) = mpsc::channel();
        let (_tx_controllino_to_balling, rx_balling_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_balling, rx_balling_from_signal_handler) = mpsc::channel();
        let (tx_balling_to_signal_handler, rx_signal_handler_from_balling) = mpsc::channel();
        let (tx_schedule_check_to_balling, rx_balling_from_schedule_check) = mpsc::channel();
        let (tx_balling_to_schedule_check, rx_schedule_check_from_balling) = mpsc::channel();

        let mutex_device_scheduler = Arc::new(Mutex::new(0));
        let mutex_device_scheduler_environment = mutex_device_scheduler.clone();

        // create test object
        let mut balling = Balling::new(config.balling, sql_interface_balling);

        let mut mock_mineral_injection = MockMineralInjection::new();

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_balling,
                    &rx_schedule_check_from_balling,
                    None,
                    schedule_allows,
                )
            });

            // thread for rest of test environment
            scope.spawn(move || {
                let execution_count: i32;
                spin_sleeper_test_environment.sleep(sleep_duration_test_environment);

                // internal scope to for releasing mutex after read
                {
                    execution_count = *mutex_device_scheduler_environment.lock().unwrap();
                }

                println!("test_balling_dosing: execution_count={}", execution_count);
                assert_eq!(execution_count, execution_count_reference);

                // now we can send the quit command
                let _ = tx_signal_handler_to_balling.send(InternalCommand::Quit);
                rx_signal_handler_from_balling.recv().unwrap();
                println!(
                    "test_balling_dosing: Received confirmation of Quit command from test object."
                );
                let _ = tx_signal_handler_to_balling.send(InternalCommand::Terminate);
            });

            // thread for test object
            scope.spawn(move || {
                let tx_balling_to_schedule_check_for_test_case_finish =
                    tx_balling_to_schedule_check.clone();

                balling.execute(
                    mutex_device_scheduler,
                    tx_balling_to_controllino,
                    rx_balling_from_controllino,
                    tx_balling_to_signal_handler,
                    rx_balling_from_signal_handler,
                    tx_balling_to_schedule_check,
                    rx_balling_from_schedule_check,
                    None,
                    &mut mock_mineral_injection,
                );

                let _ =
                    tx_balling_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

                println!("{}", mock_mineral_injection);

                assert_eq!(
                    mock_mineral_injection.pump1_engagement_durations.len(),
                    pump1_engagement_durations_reference
                );
                assert_eq!(
                    mock_mineral_injection.pump2_engagement_durations.len(),
                    pump2_engagement_durations_reference
                );
                assert_eq!(
                    mock_mineral_injection.pump3_engagement_durations.len(),
                    pump3_engagement_durations_reference
                );
                assert_eq!(
                    mock_mineral_injection.pump4_engagement_durations.len(),
                    pump4_engagement_durations_reference
                );
            });
        });
    }

    #[test]
    // The test case executes the test object with the following boundaries:
    // - pump #1 is active
    // - schedule checker is allowing operation
    // Test case uses test database #03.
    pub fn test_balling_dosing_schedule_allows_single_dosing() {
        test_balling_dosing(
            3,                        // index for determining test data base
            true,                     // schedule allows
            1,                        // execution count
            10000,                    // maximum time in milliseconds for test execution
            1,                        // number of dosings for pump #1
            0,                        // number of dosings for pump #2
            0,                        // number of dosings for pump #3
            0,                        // number of dosings for pump #4
            60,                       // schedule check interval
            Some((50, 200, 100, 50)), // dosing intervals
        );
    }

    #[test]
    // The test case executes the test object with the following boundaries:
    // - all pumps are active
    // - schedule checker is not allowing operation
    // Test case uses test database #04.
    pub fn test_balling_dosing_schedule_forbids() {
        test_balling_dosing(
            4,     // index for determining test data base
            false, // schedule does not allow
            0,     // execution count
            10000, // maximum time in milliseconds for test execution
            0,     // number of dosings for pump #1
            0,     // number of dosings for pump #2
            0,     // number of dosings for pump #3
            0,     // number of dosings for pump #4
            60,    // schedule check interval
            None,
        );
    }

    #[test]
    // The test case executes the test object with the following boundaries:
    // - all pumps are active
    // - schedule checker is allowing operation
    // - dosing intervals are short so that lots of dosing events are triggered
    // Test case uses test database #05.
    pub fn test_balling_dosing_schedule_allows_multiple_dosings() {
        test_balling_dosing(
            5,                  // index for determining test data base
            true,               // schedule allows
            14,                 // execution count
            35000,              // maximum time in milliseconds for test execution
            5,                  // number of dosings for pump #1
            4,                  // number of dosings for pump #2
            3,                  // number of dosings for pump #3
            2,                  // number of dosings for pump #4
            1,                  // schedule check interval
            Some((3, 5, 7, 9)), // dosing intervals
        );
    }

    #[test]
    // Test case checks if the target dosing duration is calculated correctly
    // considering only valid input values.
    // The test case does not require any communication with SQL database.
    pub fn test_calc_target_dosing_duration_millis_happy_case() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 5.0,
            dosing_volume: 5.0,
            label: "test_calc_target_dosing_duration_millis_happy_case".to_string(),
        };

        assert_eq!(
            Balling::calc_target_dosing_duration_millis(&balling_setval),
            1000
        );
    }

    #[test]
    // Test case checks if the target dosing duration is calculated correctly
    // using zero dosing speed as input.
    // The test case does not require any communication with SQL database.
    pub fn test_calc_target_dosing_duration_millis_zero_dosing_speed() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 0.0,
            dosing_volume: 5.0,
            label: "test_calc_target_dosing_duration_millis_zero_dosing_speed".to_string(),
        };

        assert_eq!(
            Balling::calc_target_dosing_duration_millis(&balling_setval),
            0
        );
    }

    #[test]
    // Test case checks if the target dosing duration is calculated correctly
    // using negative dosing speed as input.
    // The test case does not require any communication with SQL database.
    pub fn test_calc_target_dosing_duration_millis_negative_dosing_speed() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: -5.0,
            dosing_volume: 5.0,
            label: "test_calc_target_dosing_duration_millis_negative_dosing_speed".to_string(),
        };

        assert_eq!(
            Balling::calc_target_dosing_duration_millis(&balling_setval),
            0
        );
    }

    #[test]
    // Test case checks if the target dosing duration is calculated correctly
    // using zero dosing volume as input.
    // The test case does not require any communication with SQL database.
    pub fn test_calc_target_dosing_duration_millis_zero_dosing_volume() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 5.0,
            dosing_volume: 0.0,
            label: "test_calc_target_dosing_duration_millis_zero_dosing_volume".to_string(),
        };

        assert_eq!(
            Balling::calc_target_dosing_duration_millis(&balling_setval),
            0
        );
    }

    #[test]
    // Test case checks if the target dosing duration is calculated correctly
    // using negative dosing volume as input.
    // The test case does not require any communication with SQL database.
    pub fn test_calc_target_dosing_duration_millis_negative_dosing_volume() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 5.0,
            dosing_volume: -5.0,
            label: "test_calc_target_dosing_duration_millis_negative_dosing_volume".to_string(),
        };

        assert_eq!(
            Balling::calc_target_dosing_duration_millis(&balling_setval),
            0
        );
    }

    #[test]
    // Test case checks if the actual dosing duration is calculated correctly
    // considering only valid input values.
    // The test case does not require any communication with SQL database.
    pub fn test_calc_actual_dosing_volume_happy_case() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 5.0,
            dosing_volume: 5.0,
            label: "test_calc_actual_dosing_volume_happy_case".to_string(),
        };

        assert_eq!(
            Balling::calc_actual_dosing_volume(&balling_setval, 1000),
            5.0
        );
    }

    #[test]
    // Test case checks if the actual dosing duration is calculated correctly
    // using zero dosing speed as input.
    // The test case does not require any communication with SQL database.
    pub fn test_calc_actual_dosing_volume_zero_dosing_speed() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 0.0,
            dosing_volume: 5.0,
            label: "test_calc_actual_dosing_volume_zero_dosing_speed".to_string(),
        };

        assert_eq!(
            Balling::calc_actual_dosing_volume(&balling_setval, 1000),
            0.0
        );
    }

    #[test]
    // Test case checks if the actual dosing duration is calculated correctly
    // using negative dosing speed as input.
    // The test case does not require any communication with SQL database.
    pub fn test_calc_actual_dosing_volume_negative_dosing_speed() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 0.0,
            dosing_volume: 5.0,
            label: "test_calc_actual_dosing_volume_negative_dosing_speed".to_string(),
        };

        assert_eq!(
            Balling::calc_actual_dosing_volume(&balling_setval, 1000),
            0.0
        );
    }

    #[test]
    // Test case checks if the actual dosing duration is calculated correctly
    // using zero actual dosing duration as input.
    // The test case does not require any communication with SQL database.
    pub fn test_calc_actual_dosing_volume_zero_actual_dosing_duration() {
        let balling_setval = BallingSetVal {
            pump_id: 1,
            dosing_speed: 5.0,
            dosing_volume: 5.0,
            label: "test_calc_actual_dosing_volume_zero_actual_dosing_duration".to_string(),
        };

        assert_eq!(Balling::calc_actual_dosing_volume(&balling_setval, 0), 0.0);
    }

    #[test]
    // Test case runs Balling dosing control and triggers inhibition by sending stop message.
    // After verification that Balling dosing is stopped,
    // test case sends start message and verifies if Balling dosing control
    // resumes operation.
    // Test case uses test database #06.
    pub fn test_messaging_stops_starts_balling_dosing() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let sleep_duration_7_secs = Duration::from_secs(7);
        let sleep_duration_2_secs = Duration::from_secs(2);
        let spin_sleeper = SpinSleeper::default();

        let mut config: ConfigData = read_balling_config_with_database(6);

        config.balling.pump2_active = false;
        config.balling.pump3_active = false;
        config.balling.pump4_active = false;
        config.balling.schedule_check_interval = 1;
        config.balling.dosing_interval_pump1 = 5;

        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let mut sql_interface_balling = SqlInterfaceBalling::new(sql_interface.get_connection());

        // empty the Balling dosing log table
        match SqlInterface::truncate_table(&mut sql_interface, "ballingdosinglog".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        // insert a valid pump configuration for pump #1
        insert_pump_configurations(
            &mut sql_interface,
            &mut sql_interface_balling,
            true,
            false,
            false,
            false,
        );

        // create test objects
        let mut balling = Balling::new(config.balling, sql_interface_balling);

        let (tx_balling_to_controllino, _rx_controllino_from_balling) = mpsc::channel();
        let (_tx_controllino_to_balling, rx_balling_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_balling, rx_balling_from_signal_handler) = mpsc::channel();
        let (tx_balling_to_signal_handler, rx_signal_handler_from_balling) = mpsc::channel();
        let (tx_schedule_check_to_balling, rx_balling_from_schedule_check) = mpsc::channel();
        let (tx_balling_to_schedule_check, rx_schedule_check_from_balling) = mpsc::channel();
        let (tx_messaging_to_balling, rx_balling_from_messaging) = mpsc::channel();

        let tx_balling_to_schedule_check_for_test_case_finish =
            tx_balling_to_schedule_check.clone();

        let mutex_device_scheduler = Arc::new(Mutex::new(0));

        let mut mock_mineral_injection = MockMineralInjection::new();

        scope(|scope| {
            // thread for mock schedule check: runs until it receives Quit command
            // thread for schedule check
            scope.spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_balling,
                    &rx_schedule_check_from_balling,
                    None,
                    true,
                )
            });

            let mutex_device_scheduler_balling_test_environment = mutex_device_scheduler.clone();

            // thread IPC messaging - this scope controls execution of the test case
            scope.spawn(move || {
                // wait for 2 seconds to make sure test object can receive message
                for _ in 0..20 {
                    spin_sleeper.sleep(sleep_duration_100_millisec);
                }

                let actuation_count_0 = *mutex_device_scheduler_balling_test_environment
                    .lock()
                    .unwrap();
                // check initial amount of actuations
                assert_eq!(actuation_count_0, 1);

                // sending message requesting stop of Balling dosing control
                match tx_messaging_to_balling.send(InternalCommand::Stop) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending stop command to test object ({e:?})",
                            LOG_TARGET_BALLING
                        );
                    }
                }

                // wait for 7 seconds. If test object does not follow stop request,
                // then dosings will happen in this period.
                spin_sleeper.sleep(sleep_duration_7_secs);

                let actuation_count_1 = *mutex_device_scheduler_balling_test_environment
                    .lock()
                    .unwrap();
                // check amount of actuations (after stop request)
                assert_eq!(actuation_count_1, 1);

                // sending message requesting restart of Balling dosing control
                match tx_messaging_to_balling.send(InternalCommand::Start) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending start command to test object ({e:?})",
                            LOG_TARGET_BALLING
                        );
                    }
                }

                // wait for 2 seconds. If test object follows start request,
                // then dosings will happen in this period.
                spin_sleeper.sleep(sleep_duration_2_secs);

                let actuation_count_2 = *mutex_device_scheduler_balling_test_environment
                    .lock()
                    .unwrap();
                // check amount of actuations (after start request)
                assert_eq!(actuation_count_2, 2);

                // requesting Balling dosing control to quit
                let _ = tx_signal_handler_to_balling.send(InternalCommand::Quit);
                rx_signal_handler_from_balling.recv().unwrap();

                let actuation_count_3 = *mutex_device_scheduler_balling_test_environment
                    .lock()
                    .unwrap();
                // check amount of actuations (after Quit command)
                assert_eq!(actuation_count_3, 2);
                let _ = tx_signal_handler_to_balling.send(InternalCommand::Terminate);
            });

            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object and assertions
            scope.spawn(move || {
                balling.execute(
                    mutex_device_scheduler,
                    tx_balling_to_controllino,
                    rx_balling_from_controllino,
                    tx_balling_to_signal_handler,
                    rx_balling_from_signal_handler,
                    tx_balling_to_schedule_check,
                    rx_balling_from_schedule_check,
                    Some(rx_balling_from_messaging),
                    &mut mock_mineral_injection,
                );
                println!("MockMineralInjection:\n{}", mock_mineral_injection);

                assert_eq!(mock_mineral_injection.pump1_engagement_durations.len(), 2);
                assert_eq!(mock_mineral_injection.pump2_engagement_durations.len(), 0);
                assert_eq!(mock_mineral_injection.pump3_engagement_durations.len(), 0);
                assert_eq!(mock_mineral_injection.pump4_engagement_durations.len(), 0);

                assert_eq!(
                    mock_mineral_injection
                        .pump1_engagement_durations
                        .pop()
                        .unwrap(),
                    250
                );
                assert_eq!(
                    mock_mineral_injection
                        .pump1_engagement_durations
                        .pop()
                        .unwrap(),
                    250
                );

                let _ =
                    tx_balling_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            });

            println!(
                "* [Messaging] checking if messaging can stop and start balling dosing succeeded."
            );
        });
    }
}
