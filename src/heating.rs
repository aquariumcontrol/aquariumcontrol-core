/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::{error, info, warn};
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

/// Trait for the execution of data transfer to SQL database after midnight has arrived.
/// This trait allows running the main control with a mock implementation for testing.
pub trait HeatingStatsDataTransferTrait {
    fn transfer_heating_stats(
        &mut self,
        data: HeatingStatsEntry,
        sql_interface_heating: &mut SqlInterfaceHeating,
        sql_interface: &mut SqlInterface,
    );
}

pub struct HeatingStatsDataTransfer;

impl HeatingStatsDataTransferTrait for HeatingStatsDataTransfer {
    fn transfer_heating_stats(
        &mut self,
        mut heating_stats_entry: HeatingStatsEntry,
        sql_interface_heating: &mut SqlInterfaceHeating,
        sql_interface: &mut SqlInterface,
    ) {
        // two subsequent sections.
        let today = match sql_interface.get_current_date() {
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
/// * `tx_to_datalogger` - sender part of channel for communication to data logger
fn switch_on_heater(
    mutex_device_scheduler_heating: &Arc<Mutex<i32>>,
    tx_to_controllino: &Sender<InternalCommand>,
    rx_from_controllino: &Receiver<bool>,
    tx_to_datalogger: &Sender<bool>,
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
        match tx_to_datalogger.send(true) {
            Ok(_) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_HEATING,
                    "channel communication to data logger when switching on heater failed ({e:?})"
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
/// * `tx_to_datalogger` - sender part of channel for communication to data logger
fn switch_off_heater(
    mutex_device_scheduler_heating: &Arc<Mutex<i32>>,
    tx_to_controllino: &Sender<InternalCommand>,
    rx_from_controllino: &Receiver<bool>,
    tx_to_datalogger: &Sender<bool>,
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
        match tx_to_datalogger.send(false) {
            Ok(_) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_HEATING,
                    "channel communication to data logger when switching off heater failed ({e:?})"
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
///     heating --> schedule_check[Schedule check]
///     schedule_check --> heating
///     messaging[Messaging] --> heating
/// ```

pub struct Heating {
    /// configuration data for heating control
    config: HeatingConfig,

    /// SQL interface for heating control
    sql_interface_heating: SqlInterfaceHeating,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive water temperature via channel from GPIO handler
    lock_error_channel_receive_water_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send request for water temperature via channel to GPIO handler    
    lock_error_channel_send_water_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive ambient temperature via channel from GPIO handler
    lock_error_channel_receive_ambient_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send request for ambient temperature via channel to GPIO handler    
    lock_error_channel_send_ambient_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive tank level switch position via channel from GPIO handler
    lock_error_channel_receive_tank_level_switch_position: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send request for tank level switch position via channel to GPIO handler    
    lock_error_channel_send_tank_level_switch_position: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive tank level switch invalid via channel from GPIO handler
    lock_error_channel_receive_tank_level_switch_invalid: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send request for tank level switch invalid via channel to GPIO handler    
    lock_error_channel_send_tank_level_switch_invalid: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive information from schedule check
    lock_error_channel_receive_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via channel
    lock_error_channel_receive_termination_signal: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via channel
    lock_warn_inapplicable_command: bool,
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
            lock_error_channel_receive_schedule_check: false,
            lock_error_channel_receive_termination_signal: false,
            lock_warn_inapplicable_command: false,
        }
    }

    /// Calculates a normalized controller deviation between measured value and target value in the range from zero to one.
    /// # Arguments
    /// * `measured_value` - measured temperature value    
    fn calc_normalized_control_deviation(&self, measured_value: f32) -> f32 {
        let target_value_delta =
            self.config.switch_off_temperature - self.config.switch_on_temperature;
        let measured_value_delta = measured_value - self.config.switch_on_temperature;
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

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `mutex_device_scheduler_heating` - clone of the mutex for avoiding parallel device actuation.
    /// * `tx_to_gpio` - Sender part of channel for communication to GPIO handler
    /// * `rx_from_gpio` - Receiver part of channel for communication from GPIO handler
    /// * `tx_to_controllino` - Sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Receiver part of channel for communication from Controllino
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    /// * `tx_to_datalogger` - Sender part of channel for communication to data logger
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
        rx_from_tank_level_switch: Receiver<f32>,
        tx_to_controllino: Sender<InternalCommand>,
        rx_from_controllino: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
        tx_to_datalogger: Sender<bool>,
        tx_to_schedule_check: Sender<InternalCommand>,
        rx_from_schedule_check: Receiver<bool>,
        rx_from_messaging: Receiver<InternalCommand>,
        sql_interface: &mut SqlInterface,
        heating_stats_transfer: &mut impl HeatingStatsDataTransferTrait,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let mut _refill_error = false;
        let spin_sleeper = SpinSleeper::default();
        let mut saw_tooth_profile = SawToothProfile::new(&self.config.saw_tooth_profile_config);
        let mut measured_water_temperature = 0.0;
        let mut measurement_error_water_temperature: bool;
        let mut measured_ambient_temperature = 0.0;
        let mut measurement_error_ambient_temperature: bool;
        let mut measured_tank_level_switch_position = 0.0;
        let mut measured_tank_level_switch_invalid = 0.0;
        let mut loop_counter = 0;
        let mut heater_state: ActuatorState = ActuatorState::Undefined;
        let mut schedule_check_result: bool = false;
        let mut duration_until_midnight = self.get_duration_until_midnight();
        let mut tank_level_low_counter = 0;
        let mut energy_increment: f64 = 0.0;
        let mut heating_inhibited: bool = false; // state of heating control determined by if start/stop command has been received

        info!(
            target: LOG_TARGET_HEATING,
            "duration until midnight is {} seconds.",
            duration_until_midnight
        );
        let mut heating_stats_opt = match self
            .sql_interface_heating
            .get_single_heating_stats_from_database()
        {
            Ok(c) => Some(c),
            Err(e) => {
                info!(
                    target: LOG_TARGET_HEATING,
                    "could not get existing heating stats entry for today from database ({e:?}) - creating new one for today"
                );
                Some(HeatingStatsEntry::new())
            }
        };

        loop {
            let (
                quit_command_received,  // request to end the application has been received
                start_command_received, // request to (re-)start heating has been received
                stop_command_received,  // request to (temporarily) stop heating has been received
            ) = self.process_external_request(&rx_from_signal_handler, Some(&rx_from_messaging));
            if quit_command_received {
                break;
            }
            if stop_command_received {
                info!(
                    target: LOG_TARGET_HEATING,
                    "received Stop command. inhibiting heating."
                );
                heating_inhibited = true;
            }
            if start_command_received {
                info!("received Start command. restarting heating.");
                heating_inhibited = false;
            }

            // run the temperature control only once per second
            if (loop_counter % 10 == 0) && (self.config.active) {
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
                                        "reading ambient temperature from GPIO handler failed ({e:?})"
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
                                "channel communication to GPIO handler for reading ambient temperature failed ({e:?})"
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
                                        "reading tank level switch position from GPIO handler failed ({e:?})"
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
                                "channel communication to GPIO handler for reading tank level switch position failed ({e:?})"
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
                                        "reading tank level switch invalid from GPIO handler failed ({e:?})"
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
                                "channel communication to GPIO handler for reading tank level switch invalid failed ({e:?})"
                            );
                            self.lock_error_channel_send_tank_level_switch_invalid = true;
                        }
                    }
                }

                // check if tank level switch hints to potentially low water level
                if (measured_tank_level_switch_invalid > 0.0)
                    || (measured_tank_level_switch_position < 0.5)
                {
                    tank_level_low_counter += 1;
                } else {
                    tank_level_low_counter = 0;
                }

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
                        error!(
                            target: LOG_TARGET_HEATING,
                            "channel communication to schedule check failed ({e:?})"
                        );
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
                            &tx_to_datalogger,
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
                                        &tx_to_datalogger,
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
                                        &tx_to_datalogger,
                                    );
                                    heater_state = ActuatorState::Off;
                                }
                            }
                        } else {
                            if measured_water_temperature > self.config.switch_off_temperature {
                                // switch off heater if not already off
                                if heater_state != ActuatorState::Off {
                                    switch_off_heater(
                                        &mutex_device_scheduler_heating,
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                        &tx_to_datalogger,
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
                                        &tx_to_datalogger,
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
                                            &tx_to_datalogger,
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
                                            &tx_to_datalogger,
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
                                            &tx_to_datalogger,
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
                                            &tx_to_datalogger,
                                        );
                                        heater_state = ActuatorState::Off;
                                    }
                                }
                            }
                        }
                    }
                }

                saw_tooth_profile.execute();

                // one second closer to midnight
                duration_until_midnight -= 1;
                match heating_stats_opt {
                    Some(ref mut heating_stats) => {
                        // wrap temperatures depending on the status bits
                        let water_temperature_opt = match measurement_error_water_temperature {
                            true => None,
                            false => Some(measured_water_temperature as f64),
                        };
                        let ambient_temperature_opt = match measurement_error_ambient_temperature {
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
                        sql_interface,
                    );
                    // reset heating stats
                    heating_stats_opt = Some(HeatingStatsEntry::new());
                    duration_until_midnight = self.get_duration_until_midnight();
                }
            }

            loop_counter += 1;

            spin_sleeper.sleep(sleep_duration_hundred_millis);
        }

        // switch off heater if not already off
        if heater_state != ActuatorState::Off {
            switch_off_heater(
                &mutex_device_scheduler_heating,
                &tx_to_controllino,
                &rx_from_controllino,
                &tx_to_datalogger,
            );
        }

        // update the database with the heating stats data recorded
        // so far for this date
        if heating_stats_opt.is_some() {
            heating_stats_transfer.transfer_heating_stats(
                heating_stats_opt.clone().unwrap(),
                &mut self.sql_interface_heating,
                sql_interface,
            );
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {
                info!(
                    target: LOG_TARGET_HEATING,
                    "sent quit confirmation to signal handler.");
            }
            Err(e) => {
                error!(
                    target: LOG_TARGET_HEATING,
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // this thread has channel connection to underlying threads (Controllino, GpioHandler).
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
            spin_sleeper.sleep(sleep_duration_hundred_millis);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use chrono::Local;
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::{Duration, SystemTime};

    use crate::channel_content::AquariumDevice;
    use crate::channel_content::AquariumSignal;
    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::heating::{Heating, HeatingStatsDataTransferTrait};
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
            _sql_interface: &mut SqlInterface,
        ) {
            self.heating_stats_entry_previous = self.heating_stats_entry.clone();
            self.heating_stats_entry = heating_stats_entry.clone();
            println!(
                "Heating: Received command to transfer mock data to SQL database: {}",
                heating_stats_entry
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
    ) -> (ConfigData, Heating, SqlInterface) {
        // prepare struct Heating for subsequent tests
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_heating.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_heating_midnight_calculator = Box::new(
            SqlInterfaceHeatingMockMidnightCalculator::new(seconds_to_midnight),
        );
        let sql_interface_heating = SqlInterfaceHeating::new(
            sql_interface.get_connection(),
            sql_interface_heating_midnight_calculator,
        );
        let heating_config = config.heating.clone();
        (
            config,
            Heating::new(heating_config, sql_interface_heating),
            sql_interface,
        )
    }

    fn test_heating_receive_actuation_command_send_confirmation(
        rx_controllino_from_heating: &Receiver<InternalCommand>,
        tx_controllino_to_heating: &Sender<bool>,
    ) -> Option<InternalCommand> {
        let command_received: Option<InternalCommand>;

        // receive command from test object
        match rx_controllino_from_heating.recv() {
            Ok(command) => {
                println!(
                    "test_heating_receive_actuation_command_send_confirmation: received {}",
                    command
                );
                command_received = Some(command);
            }
            Err(e) => {
                panic!("Heating: Error when receiving actuation command from test object: {e:?}");
            }
        }

        // send confirmation back
        match tx_controllino_to_heating.send(true) {
            Ok(_) => {}
            Err(e) => {
                panic!("Heating: Error when sending back actuation command confirmation to test object: {e:?}");
            }
        }

        command_received
    }

    fn run_main_cycle_with_stimuli(
        iterations: u32,
        rx_gpio_from_heating: &Receiver<InternalCommand>,
        tx_gpio_to_heating: &Sender<f32>,
        water_temperature_stimuli: f32,
        ambient_temperature_stimuli: f32,
        tank_level_switch_position: f32,
        tank_level_switch_invalid: f32,
    ) {
        for i in 0..iterations {
            println!(
                "Heating: run_main_cycle_with_sensor_signals running loop iteration #{}",
                i
            );
            for _ in 0..4 {
                // execute channel communication 4 times per iteration
                match rx_gpio_from_heating.recv() {
                    Ok(c) => match c {
                        InternalCommand::RequestSignal(s) => match s {
                            AquariumSignal::WaterTemperature => {
                                let _ = tx_gpio_to_heating.send(water_temperature_stimuli);
                            }
                            AquariumSignal::AmbientTemperature => {
                                let _ = tx_gpio_to_heating.send(ambient_temperature_stimuli);
                            }
                            AquariumSignal::TankLevelSwitchPosition => {
                                let _ = tx_gpio_to_heating.send(tank_level_switch_position);
                            }
                            AquariumSignal::TankLevelSwitchInvalid => {
                                let _ = tx_gpio_to_heating.send(tank_level_switch_invalid);
                            }
                            _ => {
                                panic!("Received invalid signal request from test object.");
                            }
                        },
                        _ => {
                            panic!("Received invalid command from test object.");
                        }
                    },
                    _ => {
                        panic!("Error in communication with test object.");
                    }
                }
            }
        }
        println!("run_main_cycle_with_stimuli finished.");
    }

    fn test_heating_receive_information_for_datalogger(
        rx_datalogger_from_heating: &Receiver<bool>,
    ) {
        // receive information for data logger
        match rx_datalogger_from_heating.recv() {
            Ok(_) => {}
            Err(e) => {
                panic!("Heating: error when receiving information for data logger: {e:?}");
            }
        }
    }

    // send via channel a value to the test object to indicate if actuation is permitted.
    fn mock_schedule_check(
        tx_schedule_check_to_heating: &Sender<bool>,
        rx_schedule_check_from_heating: &Receiver<InternalCommand>,
        value: bool,
    ) -> bool {
        match rx_schedule_check_from_heating.try_recv() {
            Ok(c) => match c {
                InternalCommand::ScheduleCheck => {
                    // allow control to actuate
                    let _ = tx_schedule_check_to_heating.send(value);
                }
                InternalCommand::Quit => {
                    // test case has finished
                    return true;
                }
                _ => {
                    panic!("Heating: test object sent invalid command.");
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        println!(
                            "Heating: Error in trying to receive command from test object: {e:?}"
                        );
                    }
                }
            }
        }
        false
    }

    // Check if the heater remains switched off with high temperature.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_heating_with_measured_temperature_high() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating, mut sql_interface) =
            prepare_heating_tests_mock_midnight_calculator(3600);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_gpio, rx_gpio_from_heating) = mpsc::channel();
        let (tx_gpio_to_heating, rx_heating_from_gpio) = mpsc::channel();
        let (tx_heating_to_datalogger, rx_datalogger_from_heating) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_heating_to_messaging, rx_heating_from_messaging) = mpsc::channel();

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

            // thread for mock GPIO and mock signal handler
            scope.spawn(move || {
                run_main_cycle_with_stimuli(
                    10,
                    &rx_gpio_from_heating,
                    &tx_gpio_to_heating,
                    30.0, // hot temperature to force switching off heater
                    24.0,
                    1.0,
                    0.0,
                );
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            });

            // thread for mock Controllino and mock data logger
            scope.spawn(move || {
                let command_received = test_heating_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_heating,
                    &tx_controllino_to_heating,
                );
                assert_eq!(
                    command_received,
                    Some(InternalCommand::SwitchOff(AquariumDevice::Heater))
                );
                test_heating_receive_information_for_datalogger(&rx_datalogger_from_heating);
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_gpio,
                    rx_heating_from_gpio,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_datalogger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging,
                    &mut sql_interface,
                    &mut heating_stats_transfer,
                );
                println!("execution of test object finished.");
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            });
            println!("* [Heating] checking reaction to high temperature.");
        });
    }

    // Check if the heater is switched on and is not switched off with low temperature.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Check if the heater is switched off after sending Quit command and before sending Terminate command.
    #[test]
    pub fn test_heating_with_measured_temperature_low() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating, mut sql_interface) =
            prepare_heating_tests_mock_midnight_calculator(3600);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_gpio, rx_gpio_from_heating) = mpsc::channel();
        let (tx_gpio_to_heating, rx_heating_from_gpio) = mpsc::channel();
        let (tx_heating_to_datalogger, rx_datalogger_from_heating) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_heating_to_messaging, rx_heating_from_messaging) = mpsc::channel();

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

            // thread for mock GPIO and mock signal handler
            scope.spawn(move || {
                run_main_cycle_with_stimuli(
                    10,
                    &rx_gpio_from_heating,
                    &tx_gpio_to_heating,
                    20.0, // cold temperature to force switching on heater
                    24.0,
                    1.0,
                    0.0,
                );
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            });

            // thread for mock Controllino and mock data logger
            scope.spawn(move || {
                let command_received = test_heating_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_heating,
                    &tx_controllino_to_heating,
                );
                assert_eq!(
                    command_received,
                    Some(InternalCommand::SwitchOn(AquariumDevice::Heater))
                );
                test_heating_receive_information_for_datalogger(&rx_datalogger_from_heating);
                // check if heater is switched off when exiting application
                let command_received = test_heating_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_heating,
                    &tx_controllino_to_heating,
                );
                assert_eq!(
                    command_received,
                    Some(InternalCommand::SwitchOff(AquariumDevice::Heater))
                );
                test_heating_receive_information_for_datalogger(&rx_datalogger_from_heating);
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_gpio,
                    rx_heating_from_gpio,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_datalogger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging,
                    &mut sql_interface,
                    &mut heating_stats_transfer,
                );
                println!("execution of test object finished.");
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            });
            println!("* [Heating] checking reaction to low temperature.");
        });
    }

    // Check if the heater is switched on and off during two cycles of the saw tooth profile.
    // Duration of heater switched on shall equal duration of heater switched off (1% tolerance).
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_heating_with_measured_temperature_50_percent() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating, mut sql_interface) =
            prepare_heating_tests_mock_midnight_calculator(3600);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_gpio, rx_gpio_from_heating) = mpsc::channel();
        let (tx_gpio_to_heating, rx_heating_from_gpio) = mpsc::channel();
        let (tx_heating_to_datalogger, rx_datalogger_from_heating) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_heating_to_messaging, rx_heating_from_messaging) = mpsc::channel();

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

            // thread for mock GPIO and mock signal handler
            scope.spawn(move || {
                run_main_cycle_with_stimuli(
                    20,
                    &rx_gpio_from_heating,
                    &tx_gpio_to_heating,
                    25.0, // medium temperature to force switching on and off of heater
                    24.0,
                    1.0,
                    0.0,
                );
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            });

            // thread for mock Controllino and mock data logger
            scope.spawn(move || {
                for _ in 0..2 {
                    let command_received = test_heating_receive_actuation_command_send_confirmation(
                        &rx_controllino_from_heating,
                        &tx_controllino_to_heating,
                    );
                    assert_eq!(
                        command_received,
                        Some(InternalCommand::SwitchOff(AquariumDevice::Heater))
                    );
                    test_heating_receive_information_for_datalogger(&rx_datalogger_from_heating);
                    let command_received = test_heating_receive_actuation_command_send_confirmation(
                        &rx_controllino_from_heating,
                        &tx_controllino_to_heating,
                    );
                    assert_eq!(
                        command_received,
                        Some(InternalCommand::SwitchOn(AquariumDevice::Heater))
                    );
                    test_heating_receive_information_for_datalogger(&rx_datalogger_from_heating);
                }
                // check if heater is switched off when exiting application
                let command_received = test_heating_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_heating,
                    &tx_controllino_to_heating,
                );
                assert_eq!(
                    command_received,
                    Some(InternalCommand::SwitchOff(AquariumDevice::Heater))
                );
                test_heating_receive_information_for_datalogger(&rx_datalogger_from_heating);
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_gpio,
                    rx_heating_from_gpio,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_datalogger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging,
                    &mut sql_interface,
                    &mut heating_stats_transfer,
                );
                println!("execution of test object finished.");
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            });
            println!("* [Heating] checking reaction to low temperature.");
        });
    }

    // checks if the Quit command is correctly processed
    // - application shall switch off heater before exiting
    #[test]
    pub fn test_heating_with_pending_quit_terminate_command() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating, mut sql_interface) = prepare_heating_tests_mock_midnight_calculator(3);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_gpio, _) = mpsc::channel();
        let (_, rx_heating_from_gpio) = mpsc::channel();
        let (tx_heating_to_datalogger, rx_datalogger_from_heating) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_messaging_to_heating, rx_heating_from_messaging) = mpsc::channel();

        scope(|scope| {
            let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

            let start_time = SystemTime::now();
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            // thread for test environment
            scope.spawn(move || {
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                let command_received_opt = test_heating_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_heating,
                    &tx_controllino_to_heating,
                );
                test_heating_receive_information_for_datalogger(&rx_datalogger_from_heating);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);

                // check if test object sent the right command
                match command_received_opt {
                    Some(command_received) => {
                        match command_received {
                            InternalCommand::SwitchOff(AquariumDevice::Heater) => { /* Do nothing */
                            }
                            _ => {
                                panic!(
                                    "Heating: Did not receive SwitchOff command from test object."
                                );
                            }
                        }
                    }
                    _ => {
                        panic!("Heating: Did not receive any command from test object.");
                    }
                }
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_gpio,
                    rx_heating_from_gpio,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_datalogger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging,
                    &mut sql_interface,
                    &mut heating_stats_transfer,
                );

                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

                // check if heater actuation has taken place
                let mutex_data = mutex_device_scheduler_heating.lock().unwrap();
                assert_eq!(*mutex_data, 1);
            });
            let finish_time = SystemTime::now();
            let execution_duration = finish_time.duration_since(start_time).unwrap();
            all_asserts::assert_le!(execution_duration.as_millis(), 500);
            println!(
                "* [Heating] checking heating execution with pending quit/terminate command (time limit) succeeded."
            );
        });
    }

    // checks if waiting for midnight and statistical calculations are computed correctly
    #[test]
    pub fn test_heating_wait_for_midnight_calc_stats_heater_on() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating, mut sql_interface) = prepare_heating_tests_mock_midnight_calculator(3);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_gpio, rx_gpio_from_heating) = mpsc::channel();
        let (tx_gpio_to_heating, rx_heating_from_gpio) = mpsc::channel();
        let (tx_heating_to_datalogger, rx_datalogger_from_heating) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_messaging_to_heating, rx_heating_from_messaging) = mpsc::channel();

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

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

            // thread for mock GPIO and mock signal handler
            scope.spawn(move || {
                run_main_cycle_with_stimuli(
                    10,
                    &rx_gpio_from_heating,
                    &tx_gpio_to_heating,
                    20.0, // very cold temperature to force switching on heater
                    24.0,
                    1.0,
                    0.0,
                );
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            });

            // thread for mock Controllino
            scope.spawn(move || {
                for _ in 0..1 {
                    _ = test_heating_receive_actuation_command_send_confirmation(
                        &rx_controllino_from_heating,
                        &tx_controllino_to_heating,
                    );
                    test_heating_receive_information_for_datalogger(&rx_datalogger_from_heating);
                }
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_gpio,
                    rx_heating_from_gpio,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_datalogger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging,
                    &mut sql_interface,
                    &mut heating_stats_transfer,
                );
                println!("execution of test object finished.");
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

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
            });
            println!("* [Heating] checking heating waiting for midnight and stats calculation with heater on succeeded.");
        });
    }
    // checks if waiting for midnight and statistical calculations are computed correctly
    #[test]
    pub fn test_heating_wait_for_midnight_calc_stats_heater_off() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating, mut sql_interface) = prepare_heating_tests_mock_midnight_calculator(3);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_gpio, rx_gpio_from_heating) = mpsc::channel();
        let (tx_gpio_to_heating, rx_heating_from_gpio) = mpsc::channel();
        let (tx_heating_to_datalogger, rx_datalogger_from_heating) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_messaging_to_heating, rx_heating_from_messaging) = mpsc::channel();

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

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

            // thread for mock GPIO and mock signal handler
            scope.spawn(move || {
                run_main_cycle_with_stimuli(
                    10,
                    &rx_gpio_from_heating,
                    &tx_gpio_to_heating,
                    30.0, // very hot temperature to force switching off heater
                    24.0,
                    1.0,
                    0.0,
                );
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            });

            // thread for mock Controllino
            scope.spawn(move || {
                for _ in 0..1 {
                    _ = test_heating_receive_actuation_command_send_confirmation(
                        &rx_controllino_from_heating,
                        &tx_controllino_to_heating,
                    );
                    test_heating_receive_information_for_datalogger(&rx_datalogger_from_heating);
                }
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_gpio,
                    rx_heating_from_gpio,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_datalogger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging,
                    &mut sql_interface,
                    &mut heating_stats_transfer,
                );
                println!("execution of test object finished.");
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

                println!(
                    "test_heating_wait_for_midnight_calc_stats_heater_off (previous): {}",
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
                    "test_heating_wait_for_midnight_calc_stats_heater_off (current): {}",
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
            });
            println!("* [Heating] checking heating waiting for midnight and stats calculation with heater off succeeded.");
        });
    }

    // Checks if negative schedule checker result is processed correctly
    #[test]
    pub fn test_heating_blocked_by_schedule() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating, mut sql_interface) = prepare_heating_tests_mock_midnight_calculator(3);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_gpio, rx_gpio_from_heating) = mpsc::channel();
        let (tx_gpio_to_heating, rx_heating_from_gpio) = mpsc::channel();
        let (tx_heating_to_datalogger, rx_datalogger_from_heating) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_messaging_to_heating, rx_heating_from_messaging) = mpsc::channel();

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
                    false,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

            // thread for mock GPIO
            scope.spawn(move || {
                run_main_cycle_with_stimuli(
                    10,
                    &rx_gpio_from_heating,
                    &tx_gpio_to_heating,
                    15.0,
                    20.0,
                    1.0,
                    0.0,
                );
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            });

            // thread for mock Controllino
            scope.spawn(move || {
                _ = test_heating_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_heating,
                    &tx_controllino_to_heating,
                );
                test_heating_receive_information_for_datalogger(&rx_datalogger_from_heating);
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_gpio,
                    rx_heating_from_gpio,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_datalogger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging,
                    &mut sql_interface,
                    &mut heating_stats_transfer,
                );

                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

                let actuation_count = *mutex_device_scheduler_heating.lock().unwrap();

                // check if heater is only actuated once (when quitting)
                assert_eq!(actuation_count, 1);
            });
            println!("* [Heating] checking if schedule checker can block actuation.");
        });
    }

    // checks if component protection is executed
    #[test]
    pub fn test_heating_component_protection_when_tank_level_low() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating, mut sql_interface) = prepare_heating_tests_mock_midnight_calculator(3);

        // channels for the test object. test environment only uses one of the channels
        let (tx_heating_to_gpio, rx_gpio_from_heating) = mpsc::channel();
        let (tx_gpio_to_heating, rx_heating_from_gpio) = mpsc::channel();
        let (tx_heating_to_datalogger, rx_datalogger_from_heating) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_messaging_to_heating, rx_heating_from_messaging) = mpsc::channel();

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

            // thread for mock GPIO and mock signal handler
            scope.spawn(move || {
                run_main_cycle_with_stimuli(
                    5,
                    &rx_gpio_from_heating,
                    &tx_gpio_to_heating,
                    20.0, // low temperature to force switching on heater
                    24.0,
                    0.0, // low tank level
                    1.0, // tank level valid
                );
                run_main_cycle_with_stimuli(
                    10,
                    &rx_gpio_from_heating,
                    &tx_gpio_to_heating,
                    20.0, // low temperature to force switching on heater
                    24.0,
                    0.0, // low tank level
                    1.0, // tank level valid
                );
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            });

            // thread for mock Controllino
            scope.spawn(move || {
                let mut heating_command;
                heating_command = test_heating_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_heating,
                    &tx_controllino_to_heating,
                );
                test_heating_receive_information_for_datalogger(&rx_datalogger_from_heating);
                match heating_command {
                    Some(command) => {
                        if command != InternalCommand::SwitchOn(AquariumDevice::Heater) {
                            panic!("test_heating_component_protection_when_tank_level_low: received invalid combination of command and device from test object.");
                        }
                    },
                    None => {
                        panic!("test_heating_component_protection_when_tank_level_low: received no command from test object");
                    }
                }
                heating_command = test_heating_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_heating,
                    &tx_controllino_to_heating,
                );
                test_heating_receive_information_for_datalogger(&rx_datalogger_from_heating);
                match heating_command {
                    Some(command) => {
                        if command != InternalCommand::SwitchOff(AquariumDevice::Heater) {
                            panic!("test_heating_component_protection_when_tank_level_low: received invalid combination of command and device from test object.");
                        }
                    },
                    None => {
                        panic!("test_heating_component_protection_when_tank_level_low: received no command from test object");
                    }
                }
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_gpio,
                    rx_heating_from_gpio,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_datalogger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging,
                    &mut sql_interface,
                    &mut heating_stats_transfer,
                );
                println!("execution of test object finished.");
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking heater component protection succeeded.");
            });
        });
    }
}
