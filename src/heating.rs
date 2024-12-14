/* Copyrigh 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::{error, warn};

#[cfg(not(test))]
use log::info;

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::channel_content::ActuatorState;
use crate::channel_content::AquariumDevice;
use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;
use crate::proc_ext_req::ProcessExternalRequestTrait;
use crate::sawtooth_profile::SawToothProfile;
use crate::sawtooth_profile::SawToothProfileConfig;
use crate::sql_interface::SqlInterface;
use crate::sql_interface_heating::{HeatingStatsEntry, SqlInterfaceHeating};

const LOG_TARGET_HEATING: &str = "heating";

const CYCLE_TIME_HEATING_MILLIS: u64 = 500;

const CYCLE_TIME_HEATING_DIVISOR: u64 = 1000 / CYCLE_TIME_HEATING_MILLIS;

const TIME_INCREMENT_HEATING_SECS: f32 = CYCLE_TIME_HEATING_MILLIS as f32 / 1000.0;

/// Trait for the execution of data transfer to SQL database after midnight has arrived.
/// This trait allows running the main control with a mock implementation for testing.
pub trait HeatingStatsDataTransferTrait {
    fn transfer_heating_stats(
        &mut self,
        data: HeatingStatsEntry,
        sql_interface_heating: &mut SqlInterfaceHeating,
    );
}

pub struct HeatingStatsDataTransfer;

impl HeatingStatsDataTransferTrait for HeatingStatsDataTransfer {
    fn transfer_heating_stats(
        &mut self,
        mut heating_stats_entry: HeatingStatsEntry,
        sql_interface_heating: &mut SqlInterfaceHeating,
    ) {
        // two subsequent sections.
        let today = match SqlInterface::get_current_date(&mut sql_interface_heating.conn) {
            Ok(c) => c,
            Err(e) => {
                error!(
                    target: LOG_TARGET_HEATING,
                    "could not get current date from SQL database ({e:?}) - not inserting heating stats into database"
                );
                return;
            }
        };

        heating_stats_entry.date = today;

        match sql_interface_heating.insert_heating_stats_entry(&heating_stats_entry) {
            Ok(_) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_HEATING,
                    "could not insert heating statistics into database: {e:?}"
                );
            }
        };
    }
}

/// Holds the configuration data for the heating control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Clone)]
pub struct HeatingConfig {
    /// Flag to determine if application shall communicate with hardware directly or via TCP with a simulator    
    pub use_simulator: bool,

    /// Control will only be active when the flag is set to true.
    active: bool,

    /// Temperature at which the heater is switched on for 100% of the time.
    /// This temperature must be lower than switch_off_temperature.
    switch_on_temperature: f32,

    /// Temperature at which the heater is switched off for 100% of the time.
    /// This temperature must be higher than switch_on_temperature.
    switch_off_temperature: f32,

    /// Flag to determine behavior in case the refill control has a problem
    /// and the water level stays low for longer periods of time.
    /// If this flag is true, then the heating control will switch off the heater
    /// regardless of measured temperature in order to avoid overheating
    /// of the fish tank or the heater itself.
    /// Background: If water level is too low, temperature sensor may be exposed to ambient air in stead of water.
    /// This condition results in wrong temperature reading and thus in wrong heater actuation.
    /// Furthermore, the exposing the heater to air instead of water and switching it on
    /// will not allow the heater to dissipate the heat and it therefore might suffer damage.
    stop_on_refill_error: bool,

    /// Number of seconds which need to pass with low water level before the heater is switched off
    stop_on_refill_error_delay: u32,

    /// Configuration of saw tooth profile for generation of low-frequency PWM signal
    saw_tooth_profile_config: SawToothProfileConfig,

    /// Electrical power consumption of heater in Watt
    heater_power: f32,

    // strategy when schedule check does not allow operation:
    // true=heating will be switched on; false=heating will be switched off
    switch_on_when_out_of_schedule: bool,

    // strategy when external request for stopping heating control has been received:
    // true=heating will be switched on; false=heating will be switched off
    switch_on_when_external_stop: bool,
}

/// Communicates to Controllino thread via channel to switch on the heater.
/// # Arguments
/// * `mutex_device_scheduler_heating` - mutex to avoid parallel actuation of device
/// * `tx_to_controllino` - sender part of channel for communication to Controllino
/// * `rx_from_controllino` - receiver part of channel for communication from Controllino
/// * `tx_to_data_logger` - sender part of channel for communication to data logger
fn switch_on_heater(
    mutex_device_scheduler_heating: &Arc<Mutex<i32>>,
    tx_to_controllino: &Sender<InternalCommand>,
    rx_from_controllino: &Receiver<bool>,
) {
    {
        // avoid electrical overloads through parallel device actuation
        let mut mutex_data = mutex_device_scheduler_heating.lock().unwrap();
        match tx_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::Heater)) {
            Ok(_) => {
                // now listen for response
                match rx_from_controllino.recv() {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_HEATING,
                            "receiving answer from Controllino when switching on heater failed ({e:?})"
                        );
                    }
                };
            }
            Err(e) => {
                error!(
                    target: LOG_TARGET_HEATING,
                    "channel communication to Controllino when switching on heater failed ({e:?})"
                );
            }
        }

        *mutex_data += 1;
    }
}

/// Communicates to Controllino thread via channel to switch off the heater.
/// # Arguments
/// * `mutex_device_scheduler_heating` - mutex to avoid parallel actuation of device
/// * `tx_to_controllino` - sender part of channel for communication to Controllino
/// * `rx_from_controllino` - receiver part of channel for communication from Controllino
/// * `tx_to_data_logger` - sender part of channel for communication to data logger
fn switch_off_heater(
    mutex_device_scheduler_heating: &Arc<Mutex<i32>>,
    tx_to_controllino: &Sender<InternalCommand>,
    rx_from_controllino: &Receiver<bool>,
) {
    {
        // avoid electrical overloads through parallel device actuation
        let mut mutex_data = mutex_device_scheduler_heating.lock().unwrap();
        match tx_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::Heater)) {
            Ok(_) => {
                // now listen for response
                match rx_from_controllino.recv() {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_HEATING,
                            "receiving answer from Controllino when switching off heater failed ({e:?})"
                        );
                    }
                };
            }
            Err(e) => {
                error!(
                    target: LOG_TARGET_HEATING,
                    "channel communication to Controllino when switching off heater failed ({e:?})"
                );
            }
        }
        *mutex_data += 1;
    }
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the heating control.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     heating[Heating control] --> atlas_scientific[AtlasScientific]
///     atlas_scientific --> heating
///     heating --> ambient[Ambient]
///     ambient --> heating
///     heating --> tank_level_switch[TankLevelSwitch]
///     tank_level_switch --> heating
///     heating --> controllino[Controllino]
///     controllino --> heating
///     heating --> signal_handler[Signal handler]
///     signal_handler --> heating
///     heating --> data_logger[Data logger]
///     data_logger --> heating
///     heating --> schedule_check[Schedule check]
///     schedule_check --> heating
///     messaging[Messaging] --> heating
/// ```

pub struct Heating {
    /// configuration data for heating control
    config: HeatingConfig,

    /// SQL interface for heating control
    sql_interface_heating: SqlInterfaceHeating,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive water temperature via channel from Atlas Scientific thread
    lock_error_channel_receive_water_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send request for water temperature via channel to Atlas Scientific thread
    lock_error_channel_send_water_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive ambient temperature via channel from Ambient thread
    lock_error_channel_receive_ambient_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send request for ambient temperature via channel to Ambient thread
    lock_error_channel_send_ambient_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive tank level switch position via channel from TankLevelSwitch thread
    lock_error_channel_receive_tank_level_switch_position: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send request for tank level switch position via channel to TankLevelSwitch thread
    lock_error_channel_send_tank_level_switch_position: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive tank level switch invalid via channel from TankLevelSwitch thread
    lock_error_channel_receive_tank_level_switch_invalid: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send request for tank level switch invalid via channel to TankLevelSwitch thread
    lock_error_channel_send_tank_level_switch_invalid: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive information from schedule check
    lock_error_channel_receive_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send information to schedule check
    lock_error_channel_send_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via channel
    lock_error_channel_receive_termination_signal: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via channel
    lock_warn_inapplicable_command: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive command from data logger
    lock_error_channel_receive_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command from data logger via channel
    lock_warn_inapplicable_command_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having recevied request for invalid signal from data logger
    lock_error_invalid_signal_request_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failuire to send response to data logger
    lock_error_send_to_data_logger: bool,
}

impl ProcessExternalRequestTrait for Heating {}

impl Heating {
    /// Provides a struct of type Heating.
    /// # Arguments
    /// * `config` - configuration data for heating control
    /// * `sql_interface_heating` - specific SQL interface for heating control           
    pub fn new(config: HeatingConfig, sql_interface_heating: SqlInterfaceHeating) -> Heating {
        Self {
            config: config,
            sql_interface_heating: sql_interface_heating,
            lock_error_channel_receive_water_temperature: false,
            lock_error_channel_send_water_temperature: false,
            lock_error_channel_receive_ambient_temperature: false,
            lock_error_channel_send_ambient_temperature: false,
            lock_error_channel_receive_tank_level_switch_position: false,
            lock_error_channel_send_tank_level_switch_position: false,
            lock_error_channel_receive_tank_level_switch_invalid: false,
            lock_error_channel_send_tank_level_switch_invalid: false,
            lock_error_channel_send_schedule_check: false,
            lock_error_channel_receive_schedule_check: false,
            lock_error_channel_receive_termination_signal: false,
            lock_warn_inapplicable_command: false,
            lock_error_channel_receive_data_logger: false,
            lock_warn_inapplicable_command_data_logger: false,
            lock_error_invalid_signal_request_data_logger: false,
            lock_error_send_to_data_logger: false,
        }
    }

    /// Calculates a normalized controller deviation between measured value and target value in the range from zero to one.
    /// # Arguments
    /// * `measured_value` - measured temperature value    
    fn calc_normalized_control_deviation(&self, measured_value: f32) -> f32 {
        let target_value_delta: f32 =
            self.config.switch_off_temperature - self.config.switch_on_temperature;
        let measured_value_delta: f32 = measured_value - self.config.switch_on_temperature;
        if target_value_delta > 0.0 {
            measured_value_delta / target_value_delta
        } else {
            0.0 // default value when target values are not properly set up.
        }
    }

    /// wraps function from sql_interface_heating and provides alternate value in case of error
    fn get_duration_until_midnight(&mut self) -> i64 {
        match self.sql_interface_heating.get_duration_until_midnight() {
            Ok(c) => c,
            Err(e) => {
                error!(
                    target: LOG_TARGET_HEATING,
                    "could not get duration until midnight ({e:?})"
                );
                24 * 3600 // default value for time in seconds until midnight
            }
        }
    }

    /// check if data logger sent command and respond
    fn communicate_with_data_logger(
        &mut self,
        heater_state: &ActuatorState,
        tx_to_data_logger: &Sender<bool>,
        rx_from_data_logger: &Receiver<InternalCommand>,
    ) {
        match rx_from_data_logger.try_recv() {
            Ok(c) => {
                self.lock_error_channel_receive_data_logger = false;
                match c {
                    InternalCommand::RequestSignal(d) => {
                        self.lock_warn_inapplicable_command_data_logger = false;
                        match d {
                            AquariumSignal::HeatingControlStatus => {
                                self.lock_error_invalid_signal_request_data_logger = false;
                                let response_to_data_logger = match heater_state {
                                    ActuatorState::On => true,
                                    _ => false,
                                };
                                match tx_to_data_logger.send(response_to_data_logger) {
                                    Ok(_) => {
                                        self.lock_error_send_to_data_logger = false;
                                    }
                                    Err(e) => {
                                        if !self.lock_error_send_to_data_logger {
                                            error!(
                                                target: LOG_TARGET_HEATING,
                                                "channel communication to data logger failed ({e:?})"
                                            );
                                            self.lock_error_send_to_data_logger = true;
                                        }
                                    }
                                };
                            }
                            _ => {
                                if !self.lock_error_invalid_signal_request_data_logger {
                                    error!(
                                        target: LOG_TARGET_HEATING,
                                        "internal error: received request for invalid signal {} from data logger",
                                        d
                                    );
                                    self.lock_error_invalid_signal_request_data_logger = true;
                                }
                            }
                        }
                    }
                    _ => {
                        if !self.lock_warn_inapplicable_command_data_logger {
                            warn!(
                                target: LOG_TARGET_HEATING,
                                "ignoring inapplicable command from data logger thread."
                            );
                            self.lock_warn_inapplicable_command_data_logger = true;
                        }
                    }
                }
            }
            Err(e) => {
                match e {
                    TryRecvError::Empty => {
                        /* empty buffer - no action required */
                        self.lock_error_channel_receive_data_logger = false;
                    }
                    TryRecvError::Disconnected => {
                        if !self.lock_error_channel_receive_data_logger {
                            error!(
                                target: LOG_TARGET_HEATING,
                                "receiving command from data logger thread failed ({e:?})"
                            );
                            self.lock_error_channel_receive_data_logger = true;
                        }
                    }
                }
            }
        };
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `mutex_device_scheduler_heating` - clone of the mutex for avoiding parallel device actuation.
    /// * `tx_to_ambient` - Sender part of channel for communication to Ambient thread
    /// * `rx_from_ambient` - Receiver part of channel for communication from Ambient thread
    /// * `tx_to_atlas_scientific` - Sender part of channel for communication to Atlas Scientific thread
    /// * `rx_from_atlas_scientific` - Receiver part of channel for communication from Atlas Scientific thread
    /// * `tx_to_tank_level_switch` - Sender part of channel for communication to tank level switch thread
    /// * `rx_from_tank_level_switch` - Receiver part of channel for communication from tank level switch thread
    /// * `tx_to_controllino` - Sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Receiver part of channel for communication from Controllino
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    /// * `tx_to_data_logger` - Sender part of channel for communication to data logger
    /// * `rx_from_data_logger` - Receiver part of channel for communication from data logger
    /// * `tx_to_schedule_check` - Sender part of channel for communication to schedule checker
    /// * `rx_from_schedule_check` - Receiver part of channel for communication from schedule checker               
    /// * `sql_interface` - Reference to generic SQL interface
    /// * `heating_stats_transfer` - generic struct for transfer of statistical data to database
    pub fn execute(
        &mut self,
        mutex_device_scheduler_heating: Arc<Mutex<i32>>,
        tx_to_ambient: Sender<InternalCommand>,
        rx_from_ambient: Receiver<f32>,
        tx_to_atlas_scientific: Sender<InternalCommand>,
        rx_from_atlas_scientific: Receiver<f32>,
        tx_to_tank_level_switch: Sender<InternalCommand>,
        rx_from_tank_level_switch: Receiver<bool>,
        tx_to_controllino: Sender<InternalCommand>,
        rx_from_controllino: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
        tx_to_data_logger: Sender<bool>,
        rx_from_data_logger: Receiver<InternalCommand>,
        tx_to_schedule_check: Sender<InternalCommand>,
        rx_from_schedule_check: Receiver<bool>,
        rx_from_messaging_opt: Option<Receiver<InternalCommand>>,
        heating_stats_transfer: &mut impl HeatingStatsDataTransferTrait,
    ) {
        let mut _refill_error = false;
        let spin_sleeper = SpinSleeper::default();
        let mut saw_tooth_profile = SawToothProfile::new(&self.config.saw_tooth_profile_config);
        let mut measured_water_temperature = 0.0;
        let mut measurement_error_water_temperature: bool;
        let mut measured_ambient_temperature = 0.0;
        let mut measurement_error_ambient_temperature: bool;
        let mut measured_tank_level_switch_position = true;
        let mut measured_tank_level_switch_invalid = false;
        let mut loop_counter = 0;
        let mut heater_state: ActuatorState = ActuatorState::Undefined;
        let mut schedule_check_result: bool = false;
        let mut duration_until_midnight = self.get_duration_until_midnight();
        let mut tank_level_low_counter = 0;
        let mut energy_increment: f64 = 0.0;
        let mut heating_inhibited: bool = false; // state of heating control determined by if start/stop command has been received
        let mut lock_warn_cycle_time_exceeded: bool = false;

        let mut heating_stats_opt = match self
            .sql_interface_heating
            .get_single_heating_stats_from_database()
        {
            Ok(c) => Some(c),
            Err(_) => {
                #[cfg(test)]
                println!(
                    "{}: could not get existing heating stats entry for today from database - creating new one for today",
                    LOG_TARGET_HEATING
                );

                Some(HeatingStatsEntry::new())
            }
        };

        let mut start_time = SystemTime::now();
        loop {
            let (
                quit_command_received,  // request to end the application has been received
                start_command_received, // request to (re-)start heating has been received
                stop_command_received,  // request to (temporarily) stop heating has been received
            ) = self
                .process_external_request(&rx_from_signal_handler, rx_from_messaging_opt.as_ref());
            if quit_command_received {
                break;
            }
            if stop_command_received {
                #[cfg(not(test))]
                info!(
                    target: LOG_TARGET_HEATING,
                    "received Stop command. inhibiting heating."
                );

                heating_inhibited = true;
            }
            if start_command_received {
                #[cfg(not(test))]
                info!(
                    target: LOG_TARGET_HEATING,
                    "received Start command. restarting heating."
                );

                heating_inhibited = false;
            }

            if self.config.active {
                // send request for water temperature
                match tx_to_atlas_scientific.send(InternalCommand::RequestSignal(
                    AquariumSignal::WaterTemperature,
                )) {
                    Ok(_) => {
                        self.lock_error_channel_send_water_temperature = false;
                        measurement_error_water_temperature = false;
                        // now listen for response
                        measured_water_temperature = match rx_from_atlas_scientific.recv() {
                            Ok(c) => {
                                self.lock_error_channel_receive_water_temperature = false;
                                c
                            }
                            Err(e) => {
                                if !self.lock_error_channel_receive_water_temperature {
                                    error!(
                                        target: LOG_TARGET_HEATING,
                                        "receiving water temperature from Atlas Scientific thread failed ({e:?})"
                                    );
                                    self.lock_error_channel_receive_water_temperature = true;
                                }
                                measurement_error_water_temperature = true;
                                measured_water_temperature // keep the old value
                            }
                        };
                    }
                    Err(e) => {
                        measurement_error_water_temperature = true;
                        if !self.lock_error_channel_send_water_temperature {
                            error!(
                                target: LOG_TARGET_HEATING,
                                "channel communication to Atlas Scientific thread for requesting water temperature failed ({e:?})"
                            );
                            self.lock_error_channel_send_water_temperature = true;
                        }
                    }
                }

                // send request for ambient temperature
                match tx_to_ambient.send(InternalCommand::RequestSignal(
                    AquariumSignal::AmbientTemperature,
                )) {
                    Ok(_) => {
                        self.lock_error_channel_send_ambient_temperature = false;
                        measurement_error_ambient_temperature = false;
                        // now listen for response
                        measured_ambient_temperature = match rx_from_ambient.recv() {
                            Ok(c) => {
                                self.lock_error_channel_receive_ambient_temperature = true;
                                c
                            }
                            Err(e) => {
                                if !self.lock_error_channel_receive_ambient_temperature {
                                    error!(
                                        target: LOG_TARGET_HEATING,
                                        "reading ambient temperature failed ({e:?})"
                                    );
                                    self.lock_error_channel_receive_ambient_temperature = true;
                                }
                                measurement_error_ambient_temperature = true;
                                measured_ambient_temperature // keep the old value
                            }
                        };
                    }
                    Err(e) => {
                        measurement_error_ambient_temperature = true;
                        if !self.lock_error_channel_send_water_temperature {
                            error!(
                                target: LOG_TARGET_HEATING,
                                "channel communication to Ambient thread for reading ambient temperature failed ({e:?})"
                            );
                            self.lock_error_channel_send_water_temperature = true;
                        }
                    }
                }

                // send request for water level switch position
                match tx_to_tank_level_switch.send(InternalCommand::RequestSignal(
                    AquariumSignal::TankLevelSwitchPosition,
                )) {
                    Ok(_) => {
                        self.lock_error_channel_send_tank_level_switch_position = false;
                        // now listen for response
                        match rx_from_tank_level_switch.recv() {
                            Ok(c) => {
                                self.lock_error_channel_receive_tank_level_switch_position = false;
                                measured_tank_level_switch_position = c
                            }
                            Err(e) => {
                                if !self.lock_error_channel_receive_tank_level_switch_position {
                                    error!(
                                        target: LOG_TARGET_HEATING,
                                        "reading tank level switch position failed ({e:?})"
                                    );
                                    self.lock_error_channel_receive_tank_level_switch_position =
                                        true;
                                }
                            }
                        };
                    }
                    Err(e) => {
                        if !self.lock_error_channel_send_tank_level_switch_position {
                            error!(
                                target: LOG_TARGET_HEATING,
                                "channel communication to tank level switch thread for reading tank level switch position failed ({e:?})"
                            );
                            self.lock_error_channel_send_tank_level_switch_position = true;
                        }
                    }
                }

                // send request for water level switch invalid
                match tx_to_tank_level_switch.send(InternalCommand::RequestSignal(
                    AquariumSignal::TankLevelSwitchInvalid,
                )) {
                    Ok(_) => {
                        self.lock_error_channel_send_tank_level_switch_invalid = false;
                        // now listen for response
                        match rx_from_tank_level_switch.recv() {
                            Ok(c) => {
                                self.lock_error_channel_receive_tank_level_switch_invalid = false;
                                measured_tank_level_switch_invalid = c
                            }
                            Err(e) => {
                                if !self.lock_error_channel_receive_tank_level_switch_invalid {
                                    error!(
                                        target: LOG_TARGET_HEATING,
                                        "reading tank level switch invalid failed ({e:?})"
                                    );
                                    self.lock_error_channel_receive_tank_level_switch_invalid =
                                        true;
                                }
                            }
                        };
                    }
                    Err(e) => {
                        if !self.lock_error_channel_send_tank_level_switch_invalid {
                            error!(
                                target: LOG_TARGET_HEATING,
                                "channel communication to tank level switch thread for reading tank level switch invalid failed ({e:?})"
                            );
                            self.lock_error_channel_send_tank_level_switch_invalid = true;
                        }
                    }
                }

                // check if tank level switch hints to potentially low water level
                if (measured_tank_level_switch_invalid) || (!measured_tank_level_switch_position) {
                    tank_level_low_counter += 1;
                } else {
                    tank_level_low_counter = 0;
                }

                // schedule check to see if actuation is allowed
                match tx_to_schedule_check.send(InternalCommand::ScheduleCheck) {
                    Ok(_) => {
                        self.lock_error_channel_send_schedule_check = false;
                        // now listen for response
                        schedule_check_result = match rx_from_schedule_check.recv() {
                            Ok(c) => {
                                self.lock_error_channel_receive_schedule_check = false;
                                c
                            }
                            Err(e) => {
                                if !self.lock_error_channel_receive_schedule_check {
                                    error!(
                                        target: LOG_TARGET_HEATING,
                                        "receiving from schedule check failed ({e:?})"
                                    );
                                    self.lock_error_channel_receive_schedule_check = true;
                                }
                                false // assume we are not allowed to actuate when communication is not successful
                            }
                        };
                    }
                    Err(e) => {
                        if !self.lock_error_channel_send_schedule_check {
                            error!(
                                target: LOG_TARGET_HEATING,
                                "channel communication to schedule check failed ({e:?})"
                            );
                            self.lock_error_channel_send_schedule_check = true;
                        }
                    }
                }

                if self.config.stop_on_refill_error
                    && (tank_level_low_counter > self.config.stop_on_refill_error_delay)
                {
                    // tank level switch position has not been available or indicating low water level for too long time
                    // switch off the heater if not already off in order to protect against overheating of the device
                    if heater_state != ActuatorState::Off {
                        switch_off_heater(
                            &mutex_device_scheduler_heating,
                            &tx_to_controllino,
                            &rx_from_controllino,
                        );
                        heater_state = ActuatorState::Off;
                    }
                } else {
                    // component protection allows operation of the heater
                    if schedule_check_result && !heating_inhibited {
                        // Schedule check said that actuation is allowed.
                        // Also, there is no inhibition from external request.
                        // Main control happens here (comparison measured/target, actuation).
                        if (measured_water_temperature >= self.config.switch_on_temperature)
                            && (measured_water_temperature <= self.config.switch_off_temperature)
                        {
                            // temperature is inside (dynamic) control window
                            if (self.calc_normalized_control_deviation(measured_water_temperature)
                                < saw_tooth_profile.level_normalized)
                                && !measurement_error_water_temperature
                            {
                                // switch on heater if not already on
                                if heater_state != ActuatorState::On {
                                    switch_on_heater(
                                        &mutex_device_scheduler_heating,
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                    );
                                    heater_state = ActuatorState::On;
                                }
                            } else {
                                // switch off heater if not already off
                                if heater_state != ActuatorState::Off {
                                    switch_off_heater(
                                        &mutex_device_scheduler_heating,
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                    );
                                    heater_state = ActuatorState::Off;
                                }
                            }
                        } else {
                            // temperature is either above or below (dynamic) control window
                            if measured_water_temperature > self.config.switch_off_temperature {
                                // switch off heater if not already off
                                if heater_state != ActuatorState::Off {
                                    switch_off_heater(
                                        &mutex_device_scheduler_heating,
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                    );
                                    heater_state = ActuatorState::Off;
                                }
                            }
                            if measured_water_temperature < self.config.switch_on_temperature {
                                // switch on heater if not already on
                                if heater_state != ActuatorState::On {
                                    switch_on_heater(
                                        &mutex_device_scheduler_heating,
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                    );
                                    heater_state = ActuatorState::On;
                                }
                            }
                        }
                    } else {
                        if heating_inhibited {
                            // external request to stop heating control has been received
                            match self.config.switch_on_when_external_stop {
                                true => {
                                    // switch on heater if not already on
                                    if heater_state != ActuatorState::On {
                                        switch_on_heater(
                                            &mutex_device_scheduler_heating,
                                            &tx_to_controllino,
                                            &rx_from_controllino,
                                        );
                                        heater_state = ActuatorState::On;
                                    }
                                }
                                false => {
                                    // switch off heater if not already off
                                    if heater_state != ActuatorState::Off {
                                        switch_off_heater(
                                            &mutex_device_scheduler_heating,
                                            &tx_to_controllino,
                                            &rx_from_controllino,
                                        );
                                        heater_state = ActuatorState::Off;
                                    }
                                }
                            }
                        } else {
                            // schedule check said that we not allowed actuate
                            match self.config.switch_on_when_out_of_schedule {
                                true => {
                                    // switch on heater if not already on
                                    if heater_state != ActuatorState::On {
                                        switch_on_heater(
                                            &mutex_device_scheduler_heating,
                                            &tx_to_controllino,
                                            &rx_from_controllino,
                                        );
                                        heater_state = ActuatorState::On;
                                    }
                                }
                                false => {
                                    // switch off heater if not already off
                                    if heater_state != ActuatorState::Off {
                                        switch_off_heater(
                                            &mutex_device_scheduler_heating,
                                            &tx_to_controllino,
                                            &rx_from_controllino,
                                        );
                                        heater_state = ActuatorState::Off;
                                    }
                                }
                            }
                        }
                    }
                }

                saw_tooth_profile.execute_with(TIME_INCREMENT_HEATING_SECS);

                // update of midnight counter only runs every second
                if loop_counter % CYCLE_TIME_HEATING_DIVISOR == 0 {
                    // one second closer to midnight
                    duration_until_midnight -= 1;
                    match heating_stats_opt {
                        Some(ref mut heating_stats) => {
                            // wrap temperatures depending on the status bits
                            let water_temperature_opt = match measurement_error_water_temperature {
                                true => None,
                                false => Some(measured_water_temperature as f64),
                            };
                            let ambient_temperature_opt =
                                match measurement_error_ambient_temperature {
                                    true => None,
                                    false => Some(measured_ambient_temperature as f64),
                                };
                            // update the heating statistics with recently aquired data
                            if heater_state == ActuatorState::On {
                                energy_increment = (self.config.heater_power / 3600.0) as f64;
                            }
                            heating_stats.update(
                                water_temperature_opt,
                                ambient_temperature_opt,
                                energy_increment,
                            );
                        }
                        None => {
                            // do nothing, heating stats or not mandatory to run heating control
                        }
                    }

                    // check if midnight has arrived
                    if duration_until_midnight <= 0 && heating_stats_opt.is_some() {
                        heating_stats_transfer.transfer_heating_stats(
                            heating_stats_opt.clone().unwrap(),
                            &mut self.sql_interface_heating,
                        );
                        // reset heating stats
                        heating_stats_opt = Some(HeatingStatsEntry::new());
                        duration_until_midnight = self.get_duration_until_midnight();
                    }
                }
            }

            self.communicate_with_data_logger(
                &heater_state,
                &tx_to_data_logger,
                &rx_from_data_logger,
            );

            loop_counter += 1;

            let stop_time = SystemTime::now();
            let execution_duration_millis =
                stop_time.duration_since(start_time).unwrap().as_millis() as u64;

            let remaining_sleep_time_millis: u64;
            if execution_duration_millis >= CYCLE_TIME_HEATING_MILLIS {
                if !lock_warn_cycle_time_exceeded {
                    warn!(target: LOG_TARGET_HEATING,
                        "execution duration of {} milliseconds exceeds cycle time of {}",
                        execution_duration_millis,
                        CYCLE_TIME_HEATING_MILLIS);
                    lock_warn_cycle_time_exceeded = true;
                } else {
                    lock_warn_cycle_time_exceeded = false;
                }
                remaining_sleep_time_millis = 0;
            } else {
                remaining_sleep_time_millis = CYCLE_TIME_HEATING_MILLIS - execution_duration_millis;
            }
            let sleep_duration = Duration::from_millis(remaining_sleep_time_millis);
            spin_sleeper.sleep(sleep_duration);

            start_time = SystemTime::now(); // start time for next cycle
        }

        // switch off heater if not already off
        if heater_state != ActuatorState::Off {
            switch_off_heater(
                &mutex_device_scheduler_heating,
                &tx_to_controllino,
                &rx_from_controllino,
            );
        }

        // update the database with the heating stats data recorded
        // so far for this date
        if heating_stats_opt.is_some() {
            heating_stats_transfer.transfer_heating_stats(
                heating_stats_opt.clone().unwrap(),
                &mut self.sql_interface_heating,
            );
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {
                #[cfg(test)]
                println!(
                    "{}: sent quit confirmation to signal handler.",
                    LOG_TARGET_HEATING
                );
            }
            Err(e) => {
                error!(
                    target: LOG_TARGET_HEATING,
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // this thread has channel connection to underlying threads (Controllino, AtlasScientific, TankLevelSwitch).
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
                                    target: LOG_TARGET_HEATING,
                                    "ignoring not applicable command from signal handler (c)"
                                );
                                self.lock_warn_inapplicable_command = true;
                            }
                            false
                        }
                    }
                }
                Err(e) => {
                    if !self.lock_error_channel_receive_termination_signal {
                        error!(
                            target: LOG_TARGET_HEATING,
                            "receiving termination signal from signal handler failed ({e:?})"
                        );
                        self.lock_error_channel_receive_termination_signal = true;
                    }
                    false
                }
            };
            let sleep_duration = Duration::from_millis(CYCLE_TIME_HEATING_MILLIS);
            spin_sleeper.sleep(sleep_duration);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use all_asserts::{assert_ge, assert_le};
    use chrono::Local;
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, SystemTime};

    use crate::channel_content::AquariumDevice;
    use crate::channel_content::InternalCommand;
    use crate::common::tests::update_min_max_actuation_duration;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::heating::LOG_TARGET_HEATING;
    use crate::heating::{Heating, HeatingStatsDataTransferTrait};
    use crate::mock::tests::mock_ambient;
    use crate::mock::tests::mock_atlas_scientific;
    use crate::mock::tests::mock_controllino;
    use crate::mock::tests::mock_data_logger;
    use crate::mock::tests::mock_schedule_check;
    use crate::mock::tests::mock_tank_level_switch;
    use crate::sql_interface::{SqlInterface, SqlInterfaceError};
    use crate::sql_interface_heating::{
        HeatingStatsEntry, SqlInterfaceHeating, SqlInterfaceHeatingMidnightCalculatorTrait,
    };

    pub struct HeatingStatsMockDataTransfer {
        heating_stats_entry_previous: HeatingStatsEntry,
        heating_stats_entry: HeatingStatsEntry,
    }

    impl HeatingStatsMockDataTransfer {
        pub fn new() -> HeatingStatsMockDataTransfer {
            Self {
                heating_stats_entry_previous: HeatingStatsEntry::new(),
                heating_stats_entry: HeatingStatsEntry::new(),
            }
        }
    }

    impl HeatingStatsDataTransferTrait for HeatingStatsMockDataTransfer {
        fn transfer_heating_stats(
            &mut self,
            heating_stats_entry: HeatingStatsEntry,
            _sql_interface_heating: &mut SqlInterfaceHeating,
        ) {
            self.heating_stats_entry_previous = self.heating_stats_entry.clone();
            self.heating_stats_entry = heating_stats_entry.clone();
            println!(
                "Heating: Received command to transfer mock data to SQL database: {}",
                heating_stats_entry,
            );
        }
    }

    pub struct SqlInterfaceHeatingMockMidnightCalculator {
        seconds_to_midnight: i64,
    }

    impl SqlInterfaceHeatingMockMidnightCalculator {
        pub fn new(seconds_to_midnight: i64) -> SqlInterfaceHeatingMockMidnightCalculator {
            Self {
                seconds_to_midnight,
            }
        }
    }

    impl SqlInterfaceHeatingMidnightCalculatorTrait for SqlInterfaceHeatingMockMidnightCalculator {
        fn get_duration_until_midnight(&mut self) -> Result<i64, SqlInterfaceError> {
            Ok(self.seconds_to_midnight) // seconds to midnight after which heating stats are inserted into database
                                         // for testing, we do not want to wait until midnight
        }
    }

    pub fn prepare_heating_tests_mock_midnight_calculator(
        seconds_to_midnight: i64,
    ) -> (ConfigData, Heating) {
        // prepare struct Heating for subsequent tests
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_heating.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);

        let mut sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };

        // empty the heating stats table
        match SqlInterface::truncate_table(&mut sql_interface, "heatingstats".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }

        let sql_interface_heating_midnight_calculator = Box::new(
            SqlInterfaceHeatingMockMidnightCalculator::new(seconds_to_midnight),
        );
        let sql_interface_heating = SqlInterfaceHeating::new(
            sql_interface.get_connection(),
            sql_interface_heating_midnight_calculator,
        );
        println!(
            "prepare_heating_tests_mock_midnight_calculator: SwitchOff temperature = {}, SwitchOn temperature = {}",
            config.heating.switch_on_temperature, config.heating.switch_off_temperature
        );
        let heating_config = config.heating.clone();
        (config, Heating::new(heating_config, sql_interface_heating))
    }

    // Check if the heater remains switched off with high temperature.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_heating_with_measured_temperature_high() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3600);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_ambient, rx_ambient_from_heating) = mpsc::channel();
        let (tx_ambient_to_heating, rx_heating_from_ambient) = mpsc::channel();
        let (tx_heating_to_atlas_scientific, rx_atlas_scientific_from_heating) = mpsc::channel();
        let (tx_atlas_scientific_to_heating, rx_heating_from_atlas_scientific) = mpsc::channel();
        let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating) = mpsc::channel();
        let (tx_tank_level_switch_to_heating, rx_heating_from_tank_level_switch) = mpsc::channel();
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
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
                    &tx_tank_level_switch_to_heating,
                    &rx_tank_level_switch_from_heating,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    true,
                    false,
                    true,
                );
            })
            .unwrap();

        // thread for mock AtlasScientific
        let join_handle_mock_atlas_scientific = thread::Builder::new()
            .name("mock_atlas_scientific".to_string())
            .spawn(move || {
                mock_atlas_scientific(
                    &tx_atlas_scientific_to_heating,
                    &rx_atlas_scientific_from_heating,
                    None,
                    30.0,
                    50000.0,
                    7.0,
                );
            })
            .unwrap();

        // thread for mock Ambient
        let join_handle_mock_ambient = thread::Builder::new()
            .name("mock_ambient".to_string())
            .spawn(move || {
                mock_ambient(
                    &tx_ambient_to_heating,
                    &rx_ambient_from_heating,
                    None,
                    24.0,
                    60.0,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_heating,
                    &rx_data_logger_from_heating,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock Controllino - includes assertions
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events =
                    mock_controllino(&tx_controllino_to_heating, &rx_controllino_from_heating);
                assert_eq!(actuation_events.len(), 1);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_atlas_scientific_for_test_case_finish =
                    tx_heating_to_atlas_scientific.clone();
                let tx_heating_to_ambient_for_test_case_finish = tx_heating_to_ambient.clone();
                let tx_heating_to_tank_level_switch_for_test_case_finish =
                    tx_heating_to_tank_level_switch.clone();
                let tx_heating_to_controllino_for_test_case_finish =
                    tx_heating_to_controllino.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_ambient,
                    rx_heating_from_ambient,
                    tx_heating_to_atlas_scientific,
                    rx_heating_from_atlas_scientific,
                    tx_heating_to_tank_level_switch,
                    rx_heating_from_tank_level_switch,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    None,
                    &mut heating_stats_transfer,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_atlas_scientific_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_ambient_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_tank_level_switch_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_heating_to_controllino_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking reaction to high temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_atlas_scientific
            .join()
            .expect("Mock Atlas Scientific did not finish.");
        join_handle_mock_ambient
            .join()
            .expect("Mock Ambient did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_controllino
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    // Check if the heater is switched on and is not switched off with low temperature.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Check if the heater is switched off after sending Quit command and before sending Terminate command.
    #[test]
    pub fn test_heating_with_measured_temperature_low() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3600);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_ambient, rx_ambient_from_heating) = mpsc::channel();
        let (tx_ambient_to_heating, rx_heating_from_ambient) = mpsc::channel();
        let (tx_heating_to_atlas_scientific, rx_atlas_scientific_from_heating) = mpsc::channel();
        let (tx_atlas_scientific_to_heating, rx_heating_from_atlas_scientific) = mpsc::channel();
        let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating) = mpsc::channel();
        let (tx_tank_level_switch_to_heating, rx_heating_from_tank_level_switch) = mpsc::channel();
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
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
                    &tx_tank_level_switch_to_heating,
                    &rx_tank_level_switch_from_heating,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    true,
                    false,
                    true,
                );
            })
            .unwrap();

        // thread for mock AtlasScientific
        let join_handle_mock_atlas_scientific = thread::Builder::new()
            .name("mock_atlas_scientific".to_string())
            .spawn(move || {
                mock_atlas_scientific(
                    &tx_atlas_scientific_to_heating,
                    &rx_atlas_scientific_from_heating,
                    None,
                    20.0,
                    50000.0,
                    7.0,
                );
            })
            .unwrap();

        // thread for mock Ambient
        let join_handle_mock_ambient = thread::Builder::new()
            .name("mock_ambient".to_string())
            .spawn(move || {
                mock_ambient(
                    &tx_ambient_to_heating,
                    &rx_ambient_from_heating,
                    None,
                    24.0,
                    60.0,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_heating,
                    &rx_data_logger_from_heating,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock Controllino - includes assertions
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events =
                    mock_controllino(&tx_controllino_to_heating, &rx_controllino_from_heating);
                assert_eq!(actuation_events.len(), 2);
                let last_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    last_actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
                let first_actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    first_actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Heater)
                );
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_atlas_scientific_for_test_case_finish =
                    tx_heating_to_atlas_scientific.clone();
                let tx_heating_to_ambient_for_test_case_finish = tx_heating_to_ambient.clone();
                let tx_heating_to_tank_level_switch_for_test_case_finish =
                    tx_heating_to_tank_level_switch.clone();
                let tx_heating_to_controllino_for_test_case_finish =
                    tx_heating_to_controllino.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_ambient,
                    rx_heating_from_ambient,
                    tx_heating_to_atlas_scientific,
                    rx_heating_from_atlas_scientific,
                    tx_heating_to_tank_level_switch,
                    rx_heating_from_tank_level_switch,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    None,
                    &mut heating_stats_transfer,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_atlas_scientific_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_ambient_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_tank_level_switch_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_heating_to_controllino_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking reaction to low temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_atlas_scientific
            .join()
            .expect("Mock Atlas Scientific did not finish.");
        join_handle_mock_ambient
            .join()
            .expect("Mock Ambient did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_controllino
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    // Check if the heater is switched on and off during two cycles of the saw tooth profile.
    // Duration of heater switched on shall equal duration of heater switched off (1% tolerance).
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_heating_with_measured_temperature_50_percent() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3600);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_ambient, rx_ambient_from_heating) = mpsc::channel();
        let (tx_ambient_to_heating, rx_heating_from_ambient) = mpsc::channel();
        let (tx_heating_to_atlas_scientific, rx_atlas_scientific_from_heating) = mpsc::channel();
        let (tx_atlas_scientific_to_heating, rx_heating_from_atlas_scientific) = mpsc::channel();
        let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating) = mpsc::channel();
        let (tx_tank_level_switch_to_heating, rx_heating_from_tank_level_switch) = mpsc::channel();
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
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
                    &tx_tank_level_switch_to_heating,
                    &rx_tank_level_switch_from_heating,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    true,
                    false,
                    true,
                );
            })
            .unwrap();

        // thread for mock AtlasScientific
        // thread for mock AtlasScientific
        let join_handle_mock_atlas_scientific = thread::Builder::new()
            .name("mock_atlas_scientific".to_string())
            .spawn(move || {
                mock_atlas_scientific(
                    &tx_atlas_scientific_to_heating,
                    &rx_atlas_scientific_from_heating,
                    None,
                    25.0,
                    50000.0,
                    7.0,
                );
            })
            .unwrap();

        // thread for mock Ambient
        let join_handle_mock_ambient = thread::Builder::new()
            .name("mock_ambient".to_string())
            .spawn(move || {
                mock_ambient(
                    &tx_ambient_to_heating,
                    &rx_ambient_from_heating,
                    None,
                    24.0,
                    60.0,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_heating,
                    &rx_data_logger_from_heating,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock Controllino - includes assertions
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events =
                    mock_controllino(&tx_controllino_to_heating, &rx_controllino_from_heating);
                println!(
                    "Mock Controllino received {} commands:",
                    actuation_events.len()
                );
                for actuation_event in actuation_events.clone() {
                    println!("{}", actuation_event);
                }
                // (first and) last actuation events need to be ignored
                let _last_actuation_event = actuation_events.pop().unwrap();
                let second_last_actuation_event = actuation_events.pop().unwrap();
                let third_last_actuation_event = actuation_events.pop().unwrap();
                let fourth_last_actuation_event = actuation_events.pop().unwrap();
                let fifth_last_actuation_event = actuation_events.pop().unwrap();
                let sixth_last_actuation_event = actuation_events.pop().unwrap();
                let seventh_last_actuation_event = actuation_events.pop().unwrap();

                let mut min_actuation_duration: u128 = 100000;
                let mut max_actuation_duration: u128 = 0;

                let second_last_actuation_duration = second_last_actuation_event
                    .time
                    .duration_since(third_last_actuation_event.time)
                    .unwrap()
                    .as_millis();
                (min_actuation_duration, max_actuation_duration) =
                    update_min_max_actuation_duration(
                        min_actuation_duration,
                        max_actuation_duration,
                        second_last_actuation_duration,
                    );
                println!(
                    "second_last_actuation_duration={}",
                    second_last_actuation_duration
                );

                let third_last_actuation_duration = third_last_actuation_event
                    .time
                    .duration_since(fourth_last_actuation_event.time)
                    .unwrap()
                    .as_millis();
                (min_actuation_duration, max_actuation_duration) =
                    update_min_max_actuation_duration(
                        min_actuation_duration,
                        max_actuation_duration,
                        second_last_actuation_duration,
                    );
                println!(
                    "third_last_actuation_duration={}",
                    third_last_actuation_duration
                );

                let fourth_last_actuation_duration = fourth_last_actuation_event
                    .time
                    .duration_since(fifth_last_actuation_event.time)
                    .unwrap()
                    .as_millis();
                (min_actuation_duration, max_actuation_duration) =
                    update_min_max_actuation_duration(
                        min_actuation_duration,
                        max_actuation_duration,
                        second_last_actuation_duration,
                    );
                println!(
                    "fourth_last_actuation_duration={}",
                    fourth_last_actuation_duration
                );

                let fifth_last_actuation_duration = fifth_last_actuation_event
                    .time
                    .duration_since(sixth_last_actuation_event.time)
                    .unwrap()
                    .as_millis();
                (min_actuation_duration, max_actuation_duration) =
                    update_min_max_actuation_duration(
                        min_actuation_duration,
                        max_actuation_duration,
                        second_last_actuation_duration,
                    );
                println!(
                    "fifth_last_actuation_duration={}",
                    fifth_last_actuation_duration
                );

                let sixth_last_actuation_duration = sixth_last_actuation_event
                    .time
                    .duration_since(seventh_last_actuation_event.time)
                    .unwrap()
                    .as_millis();
                (min_actuation_duration, max_actuation_duration) =
                    update_min_max_actuation_duration(
                        min_actuation_duration,
                        max_actuation_duration,
                        second_last_actuation_duration,
                    );
                println!(
                    "sixth_last_actuation_duration={}",
                    sixth_last_actuation_duration
                );

                let delta_min_max_duration = max_actuation_duration - min_actuation_duration;

                // execution timing differs between platforms.
                // hence, platform specific asserts are necessary
                cfg_if::cfg_if! {
                    if #[cfg(target_os = "linux")] {
                        assert_le!(second_last_actuation_duration, 7100);
                        assert_ge!(second_last_actuation_duration, 6500);
                        assert_le!(third_last_actuation_duration, 7100);
                        assert_ge!(third_last_actuation_duration, 6500);
                        assert_le!(fourth_last_actuation_duration, 7100);
                        assert_ge!(fourth_last_actuation_duration, 6500);
                        assert_le!(fifth_last_actuation_duration, 7100);
                        assert_ge!(fifth_last_actuation_duration, 6500);
                        assert_le!(sixth_last_actuation_duration, 7100);
                        assert_ge!(sixth_last_actuation_duration, 6500);
                }
                    else {
                        assert_le!(second_last_actuation_duration, 7400);
                        assert_ge!(second_last_actuation_duration, 6500);
                        assert_le!(third_last_actuation_duration, 7400);
                        assert_ge!(third_last_actuation_duration, 6500);
                        assert_le!(fourth_last_actuation_duration, 7400);
                        assert_ge!(fourth_last_actuation_duration, 6500);
                        assert_le!(fifth_last_actuation_duration, 7400);
                        assert_ge!(fifth_last_actuation_duration, 6500);
                        assert_le!(sixth_last_actuation_duration, 7400);
                        assert_ge!(sixth_last_actuation_duration, 6500);
                    }
                }
                assert_le!(delta_min_max_duration, 50);
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration = Duration::from_secs(59);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_atlas_scientific_for_test_case_finish =
                    tx_heating_to_atlas_scientific.clone();
                let tx_heating_to_ambient_for_test_case_finish = tx_heating_to_ambient.clone();
                let tx_heating_to_tank_level_switch_for_test_case_finish =
                    tx_heating_to_tank_level_switch.clone();
                let tx_heating_to_controllino_for_test_case_finish =
                    tx_heating_to_controllino.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_ambient,
                    rx_heating_from_ambient,
                    tx_heating_to_atlas_scientific,
                    rx_heating_from_atlas_scientific,
                    tx_heating_to_tank_level_switch,
                    rx_heating_from_tank_level_switch,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    None,
                    &mut heating_stats_transfer,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_atlas_scientific_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_ambient_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_tank_level_switch_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_heating_to_controllino_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking reaction to medium temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_atlas_scientific
            .join()
            .expect("Mock Atlas Scientific did not finish.");
        join_handle_mock_ambient
            .join()
            .expect("Mock Ambient did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_controllino
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    // checks if the Quit command is correctly processed
    // - application shall switch off heater before exiting
    #[test]
    pub fn test_heating_with_pending_quit_terminate_command() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_ambient, rx_ambient_from_heating) = mpsc::channel();
        let (tx_ambient_to_heating, rx_heating_from_ambient) = mpsc::channel();
        let (tx_heating_to_atlas_scientific, rx_atlas_scientific_from_heating) = mpsc::channel();
        let (tx_atlas_scientific_to_heating, rx_heating_from_atlas_scientific) = mpsc::channel();
        let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating) = mpsc::channel();
        let (tx_tank_level_switch_to_heating, rx_heating_from_tank_level_switch) = mpsc::channel();
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        let start_time = SystemTime::now();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
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
                    &tx_tank_level_switch_to_heating,
                    &rx_tank_level_switch_from_heating,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    true,
                    false,
                    true,
                );
            })
            .unwrap();

        // thread for mock AtlasScientific
        let join_handle_mock_atlas_scientific = thread::Builder::new()
            .name("mock_atlas_scientific".to_string())
            .spawn(move || {
                mock_atlas_scientific(
                    &tx_atlas_scientific_to_heating,
                    &rx_atlas_scientific_from_heating,
                    None,
                    25.0,
                    50000.0,
                    7.0,
                );
            })
            .unwrap();

        // thread for mock Ambient
        let join_handle_mock_ambient = thread::Builder::new()
            .name("mock_ambient".to_string())
            .spawn(move || {
                mock_ambient(
                    &tx_ambient_to_heating,
                    &rx_ambient_from_heating,
                    None,
                    24.0,
                    60.0,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_heating,
                    &rx_data_logger_from_heating,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock Controllino
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events =
                    mock_controllino(&tx_controllino_to_heating, &rx_controllino_from_heating);
                println!(
                    "Mock Controllino received {} commands.",
                    actuation_events.len()
                );
                assert_eq!(actuation_events.len(), 1);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_atlas_scientific_for_test_case_finish =
                    tx_heating_to_atlas_scientific.clone();
                let tx_heating_to_ambient_for_test_case_finish = tx_heating_to_ambient.clone();
                let tx_heating_to_tank_level_switch_for_test_case_finish =
                    tx_heating_to_tank_level_switch.clone();
                let tx_heating_to_controllino_for_test_case_finish =
                    tx_heating_to_controllino.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_ambient,
                    rx_heating_from_ambient,
                    tx_heating_to_atlas_scientific,
                    rx_heating_from_atlas_scientific,
                    tx_heating_to_tank_level_switch,
                    rx_heating_from_tank_level_switch,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    None,
                    &mut heating_stats_transfer,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_atlas_scientific_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_ambient_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_tank_level_switch_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_heating_to_controllino_for_test_case_finish.send(InternalCommand::Quit);

                // check if heater actuation has taken place
                let mutex_data = mutex_device_scheduler_heating.lock().unwrap();
                assert_eq!(*mutex_data, 1);
            let finish_time = SystemTime::now();
            let execution_duration = finish_time.duration_since(start_time).unwrap();
            all_asserts::assert_le!(execution_duration.as_millis(), 650);
            println!(
                "* [Heating] checking heating execution with pending quit/terminate command (time limit) succeeded."
            );
        })
        .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_atlas_scientific
            .join()
            .expect("Mock Atlas Scientific did not finish.");
        join_handle_mock_ambient
            .join()
            .expect("Mock Ambient did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_controllino
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    // checks if waiting for midnight and statistical calculations are computed correctly
    #[test]
    pub fn test_heating_wait_for_midnight_calc_stats_heater_on() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_ambient, rx_ambient_from_heating) = mpsc::channel();
        let (tx_ambient_to_heating, rx_heating_from_ambient) = mpsc::channel();
        let (tx_heating_to_atlas_scientific, rx_atlas_scientific_from_heating) = mpsc::channel();
        let (tx_atlas_scientific_to_heating, rx_heating_from_atlas_scientific) = mpsc::channel();
        let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating) = mpsc::channel();
        let (tx_tank_level_switch_to_heating, rx_heating_from_tank_level_switch) = mpsc::channel();
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        // get initial data for calculation of reference value
        let initial_heating_stats_entry = match heating
            .sql_interface_heating
            .get_single_heating_stats_from_database()
        {
            Ok(c) => c,
            Err(_) => {
                // database responded with empty response - let's create empty entry
                HeatingStatsEntry {
                    date: Local::now().date_naive(),
                    energy: 0.0,
                    ambient_temperature_average_value: 0.0,
                    ambient_temperature_average_counter: 0,
                    water_temperature_average_value: 0.0,
                    water_temperature_average_counter: 0,
                    heating_control_runtime: 0,
                }
            }
        };

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
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
                    &tx_tank_level_switch_to_heating,
                    &rx_tank_level_switch_from_heating,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    true,
                    false,
                    true,
                );
            })
            .unwrap();

        // thread for mock AtlasScientific
        let join_handle_mock_atlas_scientific = thread::Builder::new()
            .name("mock_atlas_scientific".to_string())
            .spawn(move || {
                mock_atlas_scientific(
                    &tx_atlas_scientific_to_heating,
                    &rx_atlas_scientific_from_heating,
                    None,
                    20.0,
                    50000.0,
                    7.0,
                );
            })
            .unwrap();

        // thread for mock Ambient
        let join_handle_mock_ambient = thread::Builder::new()
            .name("mock_ambient".to_string())
            .spawn(move || {
                mock_ambient(
                    &tx_ambient_to_heating,
                    &rx_ambient_from_heating,
                    None,
                    24.0,
                    60.0,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_heating,
                    &rx_data_logger_from_heating,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock Controllino
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                mock_controllino(&tx_controllino_to_heating, &rx_controllino_from_heating);
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration = Duration::from_secs(4);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_atlas_scientific_for_test_case_finish =
                    tx_heating_to_atlas_scientific.clone();
                let tx_heating_to_ambient_for_test_case_finish = tx_heating_to_ambient.clone();
                let tx_heating_to_tank_level_switch_for_test_case_finish =
                    tx_heating_to_tank_level_switch.clone();
                let tx_heating_to_controllino_for_test_case_finish =
                    tx_heating_to_controllino.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_ambient,
                    rx_heating_from_ambient,
                    tx_heating_to_atlas_scientific,
                    rx_heating_from_atlas_scientific,
                    tx_heating_to_tank_level_switch,
                    rx_heating_from_tank_level_switch,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    None,
                    &mut heating_stats_transfer,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_atlas_scientific_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_ambient_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_tank_level_switch_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_heating_to_controllino_for_test_case_finish.send(InternalCommand::Quit);

                println!(
                    "test_heating_wait_for_midnight_calc_stats_heater_on (previous): {}",
                    heating_stats_transfer.heating_stats_entry_previous,
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .heating_control_runtime
                        - initial_heating_stats_entry.heating_control_runtime,
                    3
                );
                all_asserts::assert_le!(
                    0.16666666,
                    heating_stats_transfer.heating_stats_entry_previous.energy
                );
                all_asserts::assert_ge!(
                    0.16666667,
                    heating_stats_transfer.heating_stats_entry_previous.energy
                );
                all_asserts::assert_le!(
                    23.999,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_value
                );
                all_asserts::assert_ge!(
                    24.001,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_value
                );
                all_asserts::assert_le!(
                    19.999,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_value
                );
                all_asserts::assert_ge!(
                    20.001,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_value
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_counter
                        - initial_heating_stats_entry.ambient_temperature_average_counter,
                    3
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_counter
                        - initial_heating_stats_entry.water_temperature_average_counter,
                    3
                );
                println!(
                    "test_heating_wait_for_midnight_calc_stats_heater_on (current): {}",
                    heating_stats_transfer.heating_stats_entry,
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .heating_control_runtime
                        - initial_heating_stats_entry.heating_control_runtime,
                    1
                );
                all_asserts::assert_le!(0.0555, heating_stats_transfer.heating_stats_entry.energy);
                all_asserts::assert_ge!(0.0556, heating_stats_transfer.heating_stats_entry.energy);
                all_asserts::assert_le!(
                    23.999,
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_value
                );
                all_asserts::assert_ge!(
                    24.001,
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_value
                );
                all_asserts::assert_le!(
                    19.999,
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_value
                );
                all_asserts::assert_ge!(
                    20.001,
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_value
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_counter
                        - initial_heating_stats_entry.ambient_temperature_average_counter,
                    1
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_counter
                        - initial_heating_stats_entry.water_temperature_average_counter,
                    1
                );
                println!("* [Heating] checking heating waiting for midnight and stats calculation with heater on succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_atlas_scientific
            .join()
            .expect("Mock Atlas Scientific did not finish.");
        join_handle_mock_ambient
            .join()
            .expect("Mock Ambient did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_controllino
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    // checks if waiting for midnight and statistical calculations are computed correctly
    #[test]
    pub fn test_heating_wait_for_midnight_calc_stats_heater_off() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_ambient, rx_ambient_from_heating) = mpsc::channel();
        let (tx_ambient_to_heating, rx_heating_from_ambient) = mpsc::channel();
        let (tx_heating_to_atlas_scientific, rx_atlas_scientific_from_heating) = mpsc::channel();
        let (tx_atlas_scientific_to_heating, rx_heating_from_atlas_scientific) = mpsc::channel();
        let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating) = mpsc::channel();
        let (tx_tank_level_switch_to_heating, rx_heating_from_tank_level_switch) = mpsc::channel();
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        // get initial data for calculation of reference value
        let initial_heating_stats_entry = match heating
            .sql_interface_heating
            .get_single_heating_stats_from_database()
        {
            Ok(c) => c,
            Err(_) => {
                // database responded with empty response - let's create empty entry
                HeatingStatsEntry {
                    date: Local::now().date_naive(),
                    energy: 0.0,
                    ambient_temperature_average_value: 0.0,
                    ambient_temperature_average_counter: 0,
                    water_temperature_average_value: 0.0,
                    water_temperature_average_counter: 0,
                    heating_control_runtime: 0,
                }
            }
        };

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
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
                    &tx_tank_level_switch_to_heating,
                    &rx_tank_level_switch_from_heating,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    true,
                    false,
                    true,
                );
            })
            .unwrap();

        // thread for mock AtlasScientific
        let join_handle_mock_atlas_scientific = thread::Builder::new()
            .name("mock_atlas_scientific".to_string())
            .spawn(move || {
                mock_atlas_scientific(
                    &tx_atlas_scientific_to_heating,
                    &rx_atlas_scientific_from_heating,
                    None,
                    30.0,
                    50000.0,
                    7.0,
                );
            })
            .unwrap();

        // thread for mock Ambient
        let join_handle_mock_ambient = thread::Builder::new()
            .name("mock_ambient".to_string())
            .spawn(move || {
                mock_ambient(
                    &tx_ambient_to_heating,
                    &rx_ambient_from_heating,
                    None,
                    24.0,
                    60.0,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_heating,
                    &rx_data_logger_from_heating,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock Controllino
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                mock_controllino(&tx_controllino_to_heating, &rx_controllino_from_heating);
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration = Duration::from_secs(4);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_atlas_scientific_for_test_case_finish =
                    tx_heating_to_atlas_scientific.clone();
                let tx_heating_to_ambient_for_test_case_finish = tx_heating_to_ambient.clone();
                let tx_heating_to_tank_level_switch_for_test_case_finish =
                    tx_heating_to_tank_level_switch.clone();
                let tx_heating_to_controllino_for_test_case_finish =
                    tx_heating_to_controllino.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_ambient,
                    rx_heating_from_ambient,
                    tx_heating_to_atlas_scientific,
                    rx_heating_from_atlas_scientific,
                    tx_heating_to_tank_level_switch,
                    rx_heating_from_tank_level_switch,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    None,
                    &mut heating_stats_transfer,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_atlas_scientific_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_ambient_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_tank_level_switch_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_heating_to_controllino_for_test_case_finish.send(InternalCommand::Quit);

                println!(
                    "test_heating_wait_for_midnight_calc_stats_heater_on (previous): {}",
                    heating_stats_transfer.heating_stats_entry_previous,
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .heating_control_runtime
                        - initial_heating_stats_entry.heating_control_runtime,
                    3
                );
                all_asserts::assert_le!(
                    -0.001,
                    heating_stats_transfer.heating_stats_entry_previous.energy
                );
                all_asserts::assert_ge!(
                    0.001,
                    heating_stats_transfer.heating_stats_entry_previous.energy
                );
                all_asserts::assert_le!(
                    23.999,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_value
                );
                all_asserts::assert_ge!(
                    24.001,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_value
                );
                all_asserts::assert_le!(
                    29.999,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_value
                );
                all_asserts::assert_ge!(
                    30.001,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_value
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_counter
                        - initial_heating_stats_entry.ambient_temperature_average_counter,
                    3
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_counter
                        - initial_heating_stats_entry.water_temperature_average_counter,
                    3
                );
                println!(
                    "test_heating_wait_for_midnight_calc_stats_heater_on (current): {}",
                    heating_stats_transfer.heating_stats_entry,
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .heating_control_runtime
                        - initial_heating_stats_entry.heating_control_runtime,
                    1
                );
                all_asserts::assert_le!(-0.001, heating_stats_transfer.heating_stats_entry.energy);
                all_asserts::assert_ge!(0.001, heating_stats_transfer.heating_stats_entry.energy);
                all_asserts::assert_le!(
                    23.999,
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_value
                );
                all_asserts::assert_ge!(
                    24.001,
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_value
                );
                all_asserts::assert_le!(
                    29.999,
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_value
                );
                all_asserts::assert_ge!(
                    30.001,
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_value
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_counter
                        - initial_heating_stats_entry.ambient_temperature_average_counter,
                    1
                );
                assert_eq!(
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_counter
                        - initial_heating_stats_entry.water_temperature_average_counter,
                    1
                );
                println!("* [Heating] checking heating waiting for midnight and stats calculation with heater on succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_atlas_scientific
            .join()
            .expect("Mock Atlas Scientific did not finish.");
        join_handle_mock_ambient
            .join()
            .expect("Mock Ambient did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_controllino
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    // Checks if negative schedule checker result is processed correctly
    #[test]
    pub fn test_heating_blocked_by_schedule() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3600);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_ambient, rx_ambient_from_heating) = mpsc::channel();
        let (tx_ambient_to_heating, rx_heating_from_ambient) = mpsc::channel();
        let (tx_heating_to_atlas_scientific, rx_atlas_scientific_from_heating) = mpsc::channel();
        let (tx_atlas_scientific_to_heating, rx_heating_from_atlas_scientific) = mpsc::channel();
        let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating) = mpsc::channel();
        let (tx_tank_level_switch_to_heating, rx_heating_from_tank_level_switch) = mpsc::channel();
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
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
                    &tx_tank_level_switch_to_heating,
                    &rx_tank_level_switch_from_heating,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    true,
                    false,
                    true,
                );
            })
            .unwrap();

        // thread for mock AtlasScientific
        let join_handle_mock_atlas_scientific = thread::Builder::new()
            .name("mock_atlas_scientific".to_string())
            .spawn(move || {
                mock_atlas_scientific(
                    &tx_atlas_scientific_to_heating,
                    &rx_atlas_scientific_from_heating,
                    None,
                    20.0,
                    50000.0,
                    7.0,
                );
            })
            .unwrap();

        // thread for mock Ambient
        let join_handle_mock_ambient = thread::Builder::new()
            .name("mock_ambient".to_string())
            .spawn(move || {
                mock_ambient(
                    &tx_ambient_to_heating,
                    &rx_ambient_from_heating,
                    None,
                    24.0,
                    60.0,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_heating,
                    &rx_data_logger_from_heating,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock Controllino - includes assertions
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events =
                    mock_controllino(&tx_controllino_to_heating, &rx_controllino_from_heating);
                assert_eq!(actuation_events.len(), 1);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_atlas_scientific_for_test_case_finish =
                    tx_heating_to_atlas_scientific.clone();
                let tx_heating_to_ambient_for_test_case_finish = tx_heating_to_ambient.clone();
                let tx_heating_to_tank_level_switch_for_test_case_finish =
                    tx_heating_to_tank_level_switch.clone();
                let tx_heating_to_controllino_for_test_case_finish =
                    tx_heating_to_controllino.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_ambient,
                    rx_heating_from_ambient,
                    tx_heating_to_atlas_scientific,
                    rx_heating_from_atlas_scientific,
                    tx_heating_to_tank_level_switch,
                    rx_heating_from_tank_level_switch,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    None,
                    &mut heating_stats_transfer,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_atlas_scientific_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_ambient_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_tank_level_switch_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_heating_to_controllino_for_test_case_finish.send(InternalCommand::Quit);
                println!("* [Heating] checking if schedule checker can block actuation.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_atlas_scientific
            .join()
            .expect("Mock Atlas Scientific did not finish.");
        join_handle_mock_ambient
            .join()
            .expect("Mock Ambient did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_controllino
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    // checks if component protection is executed
    #[test]
    pub fn test_heating_component_protection_when_tank_level_low() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_ambient, rx_ambient_from_heating) = mpsc::channel();
        let (tx_ambient_to_heating, rx_heating_from_ambient) = mpsc::channel();
        let (tx_heating_to_atlas_scientific, rx_atlas_scientific_from_heating) = mpsc::channel();
        let (tx_atlas_scientific_to_heating, rx_heating_from_atlas_scientific) = mpsc::channel();
        let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating) = mpsc::channel();
        let (tx_tank_level_switch_to_heating, rx_heating_from_tank_level_switch) = mpsc::channel();
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
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
                    &tx_tank_level_switch_to_heating,
                    &rx_tank_level_switch_from_heating,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    false,
                    false,
                    false,
                );
            })
            .unwrap();

        // thread for mock AtlasScientific
        let join_handle_mock_atlas_scientific = thread::Builder::new()
            .name("mock_atlas_scientific".to_string())
            .spawn(move || {
                mock_atlas_scientific(
                    &tx_atlas_scientific_to_heating,
                    &rx_atlas_scientific_from_heating,
                    None,
                    20.0,
                    50000.0,
                    7.0,
                );
            })
            .unwrap();

        // thread for mock Ambient
        let join_handle_mock_ambient = thread::Builder::new()
            .name("mock_ambient".to_string())
            .spawn(move || {
                mock_ambient(
                    &tx_ambient_to_heating,
                    &rx_ambient_from_heating,
                    None,
                    24.0,
                    60.0,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_heating,
                    &rx_data_logger_from_heating,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

        // thread for mock Controllino - includes assertions
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events =
                    mock_controllino(&tx_controllino_to_heating, &rx_controllino_from_heating);
                assert_eq!(actuation_events.len(), 2);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Heater)
                );
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millisec);

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_atlas_scientific_for_test_case_finish =
                    tx_heating_to_atlas_scientific.clone();
                let tx_heating_to_ambient_for_test_case_finish = tx_heating_to_ambient.clone();
                let tx_heating_to_tank_level_switch_for_test_case_finish =
                    tx_heating_to_tank_level_switch.clone();
                let tx_heating_to_controllino_for_test_case_finish =
                    tx_heating_to_controllino.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_ambient,
                    rx_heating_from_ambient,
                    tx_heating_to_atlas_scientific,
                    rx_heating_from_atlas_scientific,
                    tx_heating_to_tank_level_switch,
                    rx_heating_from_tank_level_switch,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    None,
                    &mut heating_stats_transfer,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_atlas_scientific_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_ambient_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_tank_level_switch_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_heating_to_controllino_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking if schedule checker can block actuation.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_atlas_scientific
            .join()
            .expect("Mock Atlas Scientific did not finish.");
        join_handle_mock_ambient
            .join()
            .expect("Mock Ambient did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_controllino
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    // Test case runs heating control and triggers inhibition by sending stop message via channel.
    // After verification that heating is stopped,
    // test case sends start message via channel and verifies if heating control
    // resumes operation.
    #[test]
    pub fn test_messaging_stops_starts_heating() {
        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3600);

        let (tx_heating_to_atlas_scientific, rx_atlas_scientific_from_heating) = mpsc::channel();
        let (tx_atlas_scientific_to_heating, rx_heating_from_atlas_scientific) = mpsc::channel();
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (tx_messaging_to_heating, rx_heating_from_messaging) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (tx_heating_to_ambient, rx_ambient_from_heating) = mpsc::channel();
        let (tx_ambient_to_heating, rx_heating_from_ambient) = mpsc::channel();
        let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating) = mpsc::channel();
        let (tx_tank_level_switch_to_heating, rx_heating_from_tank_level_switch) = mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));
        let mutex_device_scheduler_test_environment = mutex_device_scheduler_heating.clone();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
                    None,
                    true,
                );
            })
            .unwrap();

        // thread for mock AtlasScientific
        let join_handle_mock_atlas_scientific = thread::Builder::new()
            .name("mock_atlas_scientific".to_string())
            .spawn(move || {
                mock_atlas_scientific(
                    &tx_atlas_scientific_to_heating,
                    &rx_atlas_scientific_from_heating,
                    None,
                    20.0,
                    50000.0,
                    7.0,
                );
            })
            .unwrap();

        // thread for mock Ambient
        let join_handle_mock_ambient = thread::Builder::new()
            .name("mock_ambient".to_string())
            .spawn(move || {
                mock_ambient(
                    &tx_ambient_to_heating,
                    &rx_ambient_from_heating,
                    None,
                    24.0,
                    60.0,
                );
            })
            .unwrap();

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_heating,
                    &rx_data_logger_from_heating,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_heating,
                    &rx_tank_level_switch_from_heating,
                    Some(&rx_tank_level_switch_from_test_environment),
                    None,
                    None,
                    true,
                    false,
                    true,
                );
            })
            .unwrap();

        // thread for mock Controllino - includes assertions
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events =
                    mock_controllino(&tx_controllino_to_heating, &rx_controllino_from_heating);
                println!("actuation_events:");
                for actuation_event in &actuation_events {
                    println!("{}", actuation_event);
                }
                assert_eq!(actuation_events.len(), 4);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Heater)
                );
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Heater)
                );
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Heater)
                );
            })
            .unwrap();

        // thread for test environment
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_100_millisec = Duration::from_millis(100);
                let spin_sleeper = SpinSleeper::default();

                // initial wait so that heating control can switch on
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millisec);
                }
                // check initial state of mutex: should be already switched on
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 1);

                // sending message requesting stop of heating control
                match tx_messaging_to_heating.send(InternalCommand::Stop) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending stop command to test object ({e:?})",
                            LOG_TARGET_HEATING
                        );
                    }
                }

                // wait for 1 second: heating control will switch off
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millisec);
                }
                // check if heating is actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 2);

                // sending message requesting restart of heating control
                match tx_messaging_to_heating.send(InternalCommand::Start) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending start command to test object ({e:?})",
                            LOG_TARGET_HEATING
                        );
                    }
                }

                // wait for 1 second. Heating control will switch on
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millisec);
                }
                // check if heating is actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 3);

                // sending message requesting stop of heating control
                match tx_messaging_to_heating.send(InternalCommand::Stop) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending stop command to test object ({e:?})",
                            LOG_TARGET_HEATING
                        );
                    }
                }

                // wait for 1 second: Heating control will switch off
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millisec);
                }
                // check if heating is actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 4);

                // requesting heating control to quit
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();

                // check if heating is not actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 4);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_atlas_scientific_for_test_case_finish =
                    tx_heating_to_atlas_scientific.clone();
                let tx_heating_to_controllino_for_test_case_finish =
                    tx_heating_to_controllino.clone();
                let tx_heating_to_ambient_for_test_case_finish = tx_heating_to_ambient.clone();
                let tx_heating_to_tank_level_switch_for_test_case_finish =
                    tx_heating_to_tank_level_switch.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();

                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_ambient,
                    rx_heating_from_ambient,
                    tx_heating_to_atlas_scientific,
                    rx_heating_from_atlas_scientific,
                    tx_heating_to_tank_level_switch,
                    rx_heating_from_tank_level_switch,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    Some(rx_heating_from_messaging),
                    &mut heating_stats_transfer,
                );
                // send Quit signal to mock threads because test object has terminated
                let _ = tx_heating_to_ambient_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_tank_level_switch_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_atlas_scientific_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_controllino_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking if messaging can block and restart actuation.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_atlas_scientific
            .join()
            .expect("Mock Atlas Scientific did not finish.");
        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch did not finish.");
        join_handle_mock_ambient
            .join()
            .expect("Mock Ambient did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_controllino
            .join()
            .expect("Mock Controllino thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }
}
