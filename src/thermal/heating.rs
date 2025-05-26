/* Copyright 2024 Uwe Martin

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

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_heating::{HeatingStatsEntry, SqlInterfaceHeating};
use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
use crate::utilities::channel_content::{
    ActuatorState, AquariumDevice, AquariumSignal, InternalCommand,
};
use crate::utilities::common::check_if_mutex_is_blocked;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::sawtooth_profile::{SawToothProfile, SawToothProfileConfig};

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

/// Implements the `HeatingStatsDataTransferTrait` for transferring heating statistics to the SQL database.
///
/// This struct serves as the concrete component for persisting daily heating statistics
/// into the configured SQL database.
pub struct HeatingStatsDataTransfer;

impl HeatingStatsDataTransferTrait for HeatingStatsDataTransfer {
    /// Transfers the provided daily heating statistics entry to the SQL database.
    ///
    /// This implementation updates the `date` of the `heating_stats_entry` to the
    /// **current database date** (ensuring consistency) and then attempts to insert
    /// or update this record in the database. Errors during database operations are logged,
    /// but the function proceeds without panicking to avoid disrupting the main application flow.
    ///
    /// # Arguments
    /// * `heating_stats_entry` - The **`HeatingStatsEntry`** containing the statistics for the day.
    ///   This is passed by value, meaning ownership is transferred to this function.
    /// * `sql_interface_heating` - A mutable reference to the **`SqlInterfaceHeating`** instance,
    ///   used for database interaction.
    fn transfer_heating_stats(
        &mut self,
        mut heating_stats_entry: HeatingStatsEntry,
        sql_interface_heating: &mut SqlInterfaceHeating,
    ) {
        // two further sections.
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
    /// regardless of measured temperature to avoid overheating
    /// of the fish tank or the heater itself.
    /// Background: If the water level is too low, the temperature sensor may be exposed to ambient air instead of water.
    /// This condition results in a wrong temperature reading and thus in the wrong actuation of the heater.
    /// Furthermore, exposing the heater to air instead of water and switching it on
    /// will not allow the heater to dissipate the heat, and it therefore might suffer damage.
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

    // strategy when the external request for stopping heating control has been received:
    // true=heating will be switched on; false=heating will be switched off
    switch_on_when_external_stop: bool,
}

/// Commands the relay manager to switch on the heater.
///
/// This private helper function sends a `SwitchOn` command for `AquariumDevice::Heater`
/// to the relay manager thread. It then waits for an acknowledgment from the relay manager,
/// incrementing a shared mutex counter (`mutex_device_scheduler_heating`) to track actuation.
/// It also updates a flag (`mutex_blocked_during_actuation`) to help manage warnings if the
/// device scheduler mutex was blocked.
///
/// # Arguments
/// * `mutex_device_scheduler_heating` - A reference to an `Arc<Mutex<i32>>` used for
///   coordinating device actuation, preventing parallel operations, and tracking actuation counts.
/// * `tx_heating_to_relay_manager` - The **sender channel** for communicating the `SwitchOn` command to the relay manager.
/// * `rx_heating_from_relay_manager` - The **receiver channel** for getting a boolean acknowledgment back from the relay manager.
/// * `mutex_blocked_during_actuation` - A **mutable boolean flag** that's set to `true` if the
///   `mutex_device_scheduler_heating` was found to be blocked during the `check_if_mutex_is_blocked` call.
fn switch_on_heater(
    mutex_device_scheduler_heating: &Arc<Mutex<i32>>,
    tx_heating_to_relay_manager: &Sender<InternalCommand>,
    rx_heating_from_relay_manager: &Receiver<bool>,
    mutex_blocked_during_actuation: &mut bool,
) {
    {
        // reduce probability of wrong warning
        *mutex_blocked_during_actuation |=
            check_if_mutex_is_blocked(mutex_device_scheduler_heating);

        // avoid electrical overloads through parallel device actuation
        let mut mutex_data = mutex_device_scheduler_heating.lock().unwrap();
        match tx_heating_to_relay_manager.send(InternalCommand::SwitchOn(AquariumDevice::Heater)) {
            Ok(_) => {
                // now listen for the response
                match rx_heating_from_relay_manager.recv() {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_HEATING,
                            "receiving answer from relay manager when switching on heater failed ({e:?})"
                        );
                    }
                };
            }
            Err(e) => {
                error!(
                    target: LOG_TARGET_HEATING,
                    "channel communication to relay manager when switching on heater failed ({e:?})"
                );
            }
        }

        *mutex_data = mutex_data.saturating_add(1);
    }
}

/// Commands the relay manager to switch off the heater.
///
/// This private helper function sends a `SwitchOff` command for `AquariumDevice::Heater`
/// to the relay manager thread. It then waits for an acknowledgment from the relay manager,
/// incrementing a shared mutex counter (`mutex_device_scheduler_heating`) to track actuation.
/// It also updates a flag (`mutex_blocked_during_actuation`) to help manage warnings if the
/// device scheduler mutex was blocked.
///
/// # Arguments
/// * `mutex_device_scheduler_heating` - A reference to an `Arc<Mutex<i32>>` used for
///   coordinating device actuation, preventing parallel operations, and tracking actuation counts.
/// * `tx_heating_to_relay_manager` - The **sender channel** for communicating the `SwitchOff` command to the relay manager.
/// * `rx_heating_from_relay_manager` - The **receiver channel** for getting a boolean acknowledgment back from the relay manager.
/// * `mutex_blocked_during_actuation` - A **mutable boolean flag** that's set to `true` if the
///   `mutex_device_scheduler_heating` was found to be blocked during the `check_if_mutex_is_blocked` call.
fn switch_off_heater(
    mutex_device_scheduler_heating: &Arc<Mutex<i32>>,
    tx_heating_to_relay_manager: &Sender<InternalCommand>,
    rx_heating_from_relay_manager: &Receiver<bool>,
    mutex_blocked_during_actuation: &mut bool,
) {
    {
        // reduce probability of wrong warning
        *mutex_blocked_during_actuation |=
            check_if_mutex_is_blocked(mutex_device_scheduler_heating);

        // avoid electrical overloads through parallel device actuation
        let mut mutex_data = mutex_device_scheduler_heating.lock().unwrap();
        match tx_heating_to_relay_manager.send(InternalCommand::SwitchOff(AquariumDevice::Heater)) {
            Ok(_) => {
                // now listen for the response
                match rx_heating_from_relay_manager.recv() {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            target: LOG_TARGET_HEATING,
                            "receiving answer from relay manager when switching off the heater failed ({e:?})"
                        );
                    }
                };
            }
            Err(e) => {
                error!(
                    target: LOG_TARGET_HEATING,
                    "channel communication to relay manager when switching off the heater failed ({e:?})"
                );
            }
        }
        *mutex_data = mutex_data.saturating_add(1);
    }
}

/// Struct collects the channels for communication to relay manager, signal handler, data logger, schedule check, and messaging.
pub struct HeatingChannels {
    /// Sender part of the channel for communication to the relay manager
    pub tx_heating_to_relay_manager: Sender<InternalCommand>,

    /// Receiver part of the channel for communication from the relay manager
    pub rx_heating_from_relay_manager: Receiver<bool>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_heating_to_signal_handler: Sender<bool>,

    /// Receiver part of the channel for communication from the signal handler                
    pub rx_heating_from_signal_handler: Receiver<InternalCommand>,

    /// Sender part of the channel for communication to the data logger
    pub tx_heating_to_data_logger: Sender<bool>,

    /// Receiver part of the channel for communication from the data logger
    pub rx_heating_from_data_logger: Receiver<InternalCommand>,

    /// Sender part of the channel for communication to the schedule checker
    pub tx_heating_to_schedule_check: Sender<InternalCommand>,

    /// Receiver part of the channel for communication from the schedule checker               
    pub rx_heating_from_schedule_check: Receiver<bool>,

    /// Receiver part of the channel for communication from the messaging
    pub rx_heating_from_messaging_opt: Option<Receiver<InternalCommand>>,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the heating control.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     heating --> ambient[Ambient]
///     ambient --> heating
///     heating --> tank_level_switch[Tank Level Switch]
///     tank_level_switch --> heating
///     heating --> relay_manager[Relay Manager]
///     relay_manager --> heating
///     heating --> signal_handler[Signal Handler]
///     signal_handler --> heating
///     heating --> data_logger[Data Logger]
///     data_logger --> heating
///     heating --> schedule_check[Schedule Check]
///     schedule_check --> heating
///     messaging[Messaging] --> heating
/// ```
pub struct Heating {
    /// configuration data for heating control
    config: HeatingConfig,

    /// SQL interface for heating control
    sql_interface_heating: SqlInterfaceHeating,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive information from schedule check
    lock_error_channel_receive_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send information to schedule check
    lock_error_channel_send_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    lock_error_channel_receive_termination_signal: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via the channel
    lock_warn_inapplicable_command: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive command from data logger
    lock_error_channel_receive_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command from data logger via the channel
    lock_warn_inapplicable_command_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received the request for invalid signal from data logger
    lock_error_invalid_signal_request_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send response to data logger
    lock_error_send_to_data_logger: bool,
}

impl ProcessExternalRequestTrait for Heating {}

impl Heating {
    /// Creates a new `Heating` control instance.
    ///
    /// This constructor initializes the heating control module with its specific
    /// configuration and a dedicated SQL interface. It also sets up all internal
    /// "lock" flags to `false` by default; these flags are crucial during operation
    /// to prevent log files from being flooded with repeated error or warning messages
    /// stemming from channel communication or invalid requests.
    ///
    /// # Arguments
    /// * `config` - **Configuration data** for the heating control, loaded from a TOML file.
    ///   This includes parameters such as target temperatures, safety features, and
    ///   behavioral strategies.
    /// * `sql_interface_heating` - The **specific SQL interface** for heating-related database operations.
    ///   This instance is moved into the `Heating` struct.
    ///
    /// # Returns
    /// A new **`Heating` struct**, ready to manage the aquarium's heating system.
    pub fn new(config: HeatingConfig, sql_interface_heating: SqlInterfaceHeating) -> Heating {
        Self {
            config,
            sql_interface_heating,
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

    /// Calculates a **normalized control deviation** based on the measured temperature and configured heating thresholds.
    ///
    /// This private helper function determines how far the `measured_value` deviates
    /// from the `switch_on_temperature` relative to the total temperature control range
    /// (`switch_on_temperature` to `switch_off_temperature`). The result is a float
    /// between `0.0` and `1.0`. A value of `0.0` means the temperature is at or below
    /// `switch_on_temperature` (full heating needed), and `1.0` means it's at or above
    /// `switch_off_temperature` (no heating needed).
    ///
    /// # Arguments
    /// * `measured_value` - The current measured temperature (`f32`).
    ///
    /// # Returns
    /// An `f32` representing the normalized control deviation, ranging from `0.0` to `1.0`.
    /// Returns `0.0` if `switch_off_temperature` is not greater than `switch_on_temperature`
    /// (i.e., `target_value_delta` is not positive), indicating an invalid configuration.
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

    /// Retrieves the duration in seconds until the next midnight from the database or provides a default fallback on error.
    ///
    /// This private helper function acts as a robust wrapper around the `sql_interface_heating`'s
    /// `get_duration_until_midnight` method. If the database call is successful, it returns
    /// the precise duration. If an error occurs during the database interaction, it logs the error
    /// and returns a default value of 24 hours (86400 seconds), ensuring the control logic
    /// always has a valid time-until-midnight value.
    ///
    /// # Returns
    /// An `i64` representing the number of seconds remaining until the next midnight.
    /// Returns `24 * 3600` (86400) seconds as a fallback if the database retrieval fails.
    fn get_duration_until_midnight(&mut self) -> i64 {
        self.sql_interface_heating
            .get_duration_until_midnight()
            .unwrap_or_else(|e| {
                error!(
                    target: LOG_TARGET_HEATING,
                    "could not get duration until midnight ({e:?})"
                );
                24 * 3600 // default value for time in seconds until midnight
            })
    }

    /// Checks if the data logger sent a `RequestSignal` command and responds with the current heater status.
    ///
    /// This private helper function handles incoming requests from the `DataLogger` module.
    /// If a `RequestSignal` for `AquariumSignal::HeatingControlStatus` is received,
    /// it sends the current boolean state of the heater (`true` if ON, `false` if OFF)
    /// back to the data logger. Inapplicable commands are logged as warnings.
    ///
    /// # Arguments
    /// * `heater_state` - A reference to the current `ActuatorState` of the heater.
    /// * `tx_heating_to_data_logger` - The sender channel for sending the boolean status to the data logger.
    /// * `rx_heating_from_data_logger` - The receiver channel for getting `InternalCommand` requests from the data logger.
    fn communicate_with_data_logger(
        &mut self,
        heater_state: &ActuatorState,
        tx_heating_to_data_logger: &Sender<bool>,
        rx_heating_from_data_logger: &Receiver<InternalCommand>,
    ) {
        match rx_heating_from_data_logger.try_recv() {
            Ok(c) => {
                self.lock_error_channel_receive_data_logger = false;
                match c {
                    InternalCommand::RequestSignal(d) => {
                        self.lock_warn_inapplicable_command_data_logger = false;
                        match d {
                            AquariumSignal::HeatingControlStatus => {
                                self.lock_error_invalid_signal_request_data_logger = false;
                                let response_to_data_logger =
                                    matches!(heater_state, ActuatorState::On);

                                match tx_heating_to_data_logger.send(response_to_data_logger) {
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

    /// Executes the **main control loop for the heating module**.
    ///
    /// This function runs continuously, managing the aquarium's heating system until a termination
    /// signal is received. It adjusts heater operation based on **water temperature** and 
    /// **tank level switch signals** adhering to the limitations imposed by a **schedule**.
    /// The **ambient temperature** signal is considered for logging daily statistical data.
    /// It also processes external `Start`/`Stop` commands, applies a
    /// **sawtooth profile for proportional control**, and **logs daily heating statistics**.
    ///
    /// **Key Operations:**
    /// - **Sensor Data Acquisition**: Reads current water temperature, ambient temperature, and tank level switch states from shared mutexes.
    /// - **Safety Interlocks**: Implements a critical safety feature to turn off the heater if the water level is too low for a configured duration, preventing damage.
    /// - **Schedule & External Control**: Checks with the `ScheduleCheck` module and responds to `Start`/`Stop` commands to inhibit or enable heating.
    /// - **Proportional Control**: When within the active temperature range, uses a `SawToothProfile` to achieve a "PWM-like" heating effect.
    /// - **Energy Logging**: Periodically updates and transfers daily heating statistics (energy consumption, average temperatures) to the SQL database.
    /// - **Cycle Management**: Maintains a fixed cycle time, sleeping as necessary, and warning if execution duration exceeds the cycle limit.
    /// - **Graceful Shutdown**: Responds to `Quit` and `Terminate` commands from the signal handler, ensuring a safe shutdown of the heater and final data logging.
    ///
    /// # Arguments
    /// * `mutex_device_scheduler_heating` - A clone of the `Arc<Mutex<i32>>` used to coordinate device
    ///   actuation across the application, preventing conflicts and tracking activity counts.
    /// * `heating_channels` - A struct containing all sender and receiver channels for communication
    ///   with modules like `RelayManager`, `SignalHandler`, `DataLogger`, `ScheduleCheck`, and `Messaging`.
    /// * `heating_stats_transfer` - A mutable reference to an object implementing `HeatingStatsDataTransferTrait`,
    ///   responsible for persisting daily heating statistics to the database.
    /// * `mutex_water_temperature` - The `Arc<Mutex<f32>>` holding the latest measured water temperature.
    /// * `mutex_ambient_temperature` - The `Arc<Mutex<f32>>` holding the latest measured ambient temperature.
    /// * `mutex_tank_level_switch_signals` - The `Arc<Mutex<TankLevelSwitchSignals>>` containing the latest
    pub fn execute(
        &mut self,
        mutex_device_scheduler_heating: Arc<Mutex<i32>>,
        heating_channels: HeatingChannels,
        heating_stats_transfer: &mut impl HeatingStatsDataTransferTrait,
        mutex_water_temperature: Arc<Mutex<f32>>,
        mutex_ambient_temperature: Arc<Mutex<f32>>,
        mutex_tank_level_switch_signals: Arc<Mutex<TankLevelSwitchSignals>>,
    ) {
        let _refill_error = false;
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
        let mut heating_inhibited: bool = false; // state of heating control determined by if the start/stop command has been received
        let mut lock_warn_cycle_time_exceeded: bool = false;
        let mut mutex_was_blocked_during_actuation: bool = false;

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
                quit_command_received,  // the request to end the application has been received
                start_command_received, // the request to (re-)start heating has been received
                stop_command_received, // the request to (temporarily) stop heating has been received
            ) = self.process_external_request(
                &heating_channels.rx_heating_from_signal_handler,
                heating_channels.rx_heating_from_messaging_opt.as_ref(),
            );
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
                // read water temperature from mutex
                {
                    measured_water_temperature = match mutex_water_temperature.lock() {
                        Ok(c) => {
                            measurement_error_water_temperature = false;
                            *c
                        }
                        Err(_) => {
                            measurement_error_water_temperature = true;
                            measured_water_temperature // use previous value
                        }
                    };
                }

                // read ambient temperature from mutex
                {
                    measured_ambient_temperature = match mutex_ambient_temperature.lock() {
                        Ok(c) => {
                            measurement_error_ambient_temperature = false;
                            *c
                        }
                        Err(_) => {
                            measurement_error_ambient_temperature = true;
                            measured_ambient_temperature // use previous value
                        }
                    }
                }

                // read tank level switch position and invalid bit from mutex
                {
                    (
                        measured_tank_level_switch_position,
                        measured_tank_level_switch_invalid,
                    ) = match mutex_tank_level_switch_signals.lock() {
                        Ok(c) => (c.tank_level_switch_position, c.tank_level_switch_invalid),
                        Err(_) => {
                            (
                                measured_tank_level_switch_position,
                                measured_tank_level_switch_invalid,
                            ) // use previous values
                        }
                    }
                }

                // check if the tank-level switch hints to potentially low water level
                if (measured_tank_level_switch_invalid) || (!measured_tank_level_switch_position) {
                    tank_level_low_counter += 1;
                } else {
                    tank_level_low_counter = 0;
                }

                // schedule check to see if actuation is allowed
                match heating_channels
                    .tx_heating_to_schedule_check
                    .send(InternalCommand::ScheduleCheck)
                {
                    Ok(_) => {
                        self.lock_error_channel_send_schedule_check = false;
                        // now listen for the response
                        schedule_check_result =
                            match heating_channels.rx_heating_from_schedule_check.recv() {
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
                    // switch off the heater if not already off to protect against overheating of the device
                    if heater_state != ActuatorState::Off {
                        switch_off_heater(
                            &mutex_device_scheduler_heating,
                            &heating_channels.tx_heating_to_relay_manager,
                            &heating_channels.rx_heating_from_relay_manager,
                            &mut mutex_was_blocked_during_actuation,
                        );
                        heater_state = ActuatorState::Off;
                    }
                } else {
                    // component protection allows operation of the heater
                    if schedule_check_result && !heating_inhibited {
                        // Schedule check said that actuation is allowed.
                        // Also, there is no inhibition from external request.
                        // The main control happens here (comparison measured/target, actuation).
                        if (measured_water_temperature >= self.config.switch_on_temperature)
                            && (measured_water_temperature <= self.config.switch_off_temperature)
                        {
                            // temperature is inside (dynamic) control window
                            if (self.calc_normalized_control_deviation(measured_water_temperature)
                                < saw_tooth_profile.level_normalized)
                                && !measurement_error_water_temperature
                            {
                                // switch on the heater if not already on
                                if heater_state != ActuatorState::On {
                                    switch_on_heater(
                                        &mutex_device_scheduler_heating,
                                        &heating_channels.tx_heating_to_relay_manager,
                                        &heating_channels.rx_heating_from_relay_manager,
                                        &mut mutex_was_blocked_during_actuation,
                                    );
                                    heater_state = ActuatorState::On;
                                }
                            } else {
                                // switch off the heater if not already off
                                if heater_state != ActuatorState::Off {
                                    switch_off_heater(
                                        &mutex_device_scheduler_heating,
                                        &heating_channels.tx_heating_to_relay_manager,
                                        &heating_channels.rx_heating_from_relay_manager,
                                        &mut mutex_was_blocked_during_actuation,
                                    );
                                    heater_state = ActuatorState::Off;
                                }
                            }
                        } else {
                            // the temperature is either above or below (dynamic) control window
                            if measured_water_temperature > self.config.switch_off_temperature {
                                // switch off the heater if not already off
                                if heater_state != ActuatorState::Off {
                                    switch_off_heater(
                                        &mutex_device_scheduler_heating,
                                        &heating_channels.tx_heating_to_relay_manager,
                                        &heating_channels.rx_heating_from_relay_manager,
                                        &mut mutex_was_blocked_during_actuation,
                                    );
                                    heater_state = ActuatorState::Off;
                                }
                            }
                            if measured_water_temperature < self.config.switch_on_temperature {
                                // switch on the heater if not already on
                                if heater_state != ActuatorState::On {
                                    switch_on_heater(
                                        &mutex_device_scheduler_heating,
                                        &heating_channels.tx_heating_to_relay_manager,
                                        &heating_channels.rx_heating_from_relay_manager,
                                        &mut mutex_was_blocked_during_actuation,
                                    );
                                    heater_state = ActuatorState::On;
                                }
                            }
                        }
                    } else if heating_inhibited {
                        // the external request to stop heating control has been received
                        match self.config.switch_on_when_external_stop {
                            true => {
                                // switch on the heater if not already on
                                if heater_state != ActuatorState::On {
                                    switch_on_heater(
                                        &mutex_device_scheduler_heating,
                                        &heating_channels.tx_heating_to_relay_manager,
                                        &heating_channels.rx_heating_from_relay_manager,
                                        &mut mutex_was_blocked_during_actuation,
                                    );
                                    heater_state = ActuatorState::On;
                                }
                            }
                            false => {
                                // switch off the heater if not already off
                                if heater_state != ActuatorState::Off {
                                    switch_off_heater(
                                        &mutex_device_scheduler_heating,
                                        &heating_channels.tx_heating_to_relay_manager,
                                        &heating_channels.rx_heating_from_relay_manager,
                                        &mut mutex_was_blocked_during_actuation,
                                    );
                                    heater_state = ActuatorState::Off;
                                }
                            }
                        }
                    } else {
                        // schedule check said that we are not allowed to actuate
                        match self.config.switch_on_when_out_of_schedule {
                            true => {
                                // switch on the heater if not already on
                                if heater_state != ActuatorState::On {
                                    switch_on_heater(
                                        &mutex_device_scheduler_heating,
                                        &heating_channels.tx_heating_to_relay_manager,
                                        &heating_channels.rx_heating_from_relay_manager,
                                        &mut mutex_was_blocked_during_actuation,
                                    );
                                    heater_state = ActuatorState::On;
                                }
                            }
                            false => {
                                // switch off the heater if not already off
                                if heater_state != ActuatorState::Off {
                                    switch_off_heater(
                                        &mutex_device_scheduler_heating,
                                        &heating_channels.tx_heating_to_relay_manager,
                                        &heating_channels.rx_heating_from_relay_manager,
                                        &mut mutex_was_blocked_during_actuation,
                                    );
                                    heater_state = ActuatorState::Off;
                                }
                            }
                        }
                    }
                }

                saw_tooth_profile.execute_with(TIME_INCREMENT_HEATING_SECS);

                // the update of midnight counter only runs every second
                if loop_counter % CYCLE_TIME_HEATING_DIVISOR == 0 {
                    // one second closer to midnight
                    duration_until_midnight = duration_until_midnight.saturating_sub(1);
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
                            // update the heating statistics with recently acquired data
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
                &heating_channels.tx_heating_to_data_logger,
                &heating_channels.rx_heating_from_data_logger,
            );

            loop_counter += 1;

            // avoid overflow
            if loop_counter == 1_000_000 {
                loop_counter = 0;
            }

            let stop_time = SystemTime::now();
            let execution_duration_millis =
                stop_time.duration_since(start_time).unwrap().as_millis() as u64;

            let remaining_sleep_time_millis: u64;
            if execution_duration_millis >= CYCLE_TIME_HEATING_MILLIS {
                if !lock_warn_cycle_time_exceeded && !mutex_was_blocked_during_actuation {
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
            mutex_was_blocked_during_actuation = false; // reset for the next cycle

            let sleep_duration = Duration::from_millis(remaining_sleep_time_millis);
            spin_sleeper.sleep(sleep_duration);

            start_time = SystemTime::now(); // start time for the next cycle
        }

        // switch off the heater if not already off
        if heater_state != ActuatorState::Off {
            switch_off_heater(
                &mutex_device_scheduler_heating,
                &heating_channels.tx_heating_to_relay_manager,
                &heating_channels.rx_heating_from_relay_manager,
                &mut mutex_was_blocked_during_actuation,
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
        // answer to the thread which sent the request for termination, so that shutdown can proceed further.
        match heating_channels.tx_heating_to_signal_handler.send(true) {
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

        // This thread has channel connections to underlying threads (relay manager, AtlasScientific, TankLevelSwitch).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received =
                match heating_channels.rx_heating_from_signal_handler.recv() {
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
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;
    use std::time::{Duration, SystemTime};

    use crate::database::sql_interface_heating::{
        HeatingStatsEntry, SqlInterfaceHeating, SqlInterfaceHeatingMidnightCalculatorTrait,
    };
    use crate::database::{sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError};
    use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
    use crate::stubs::mock_data_logger::tests::mock_data_logger;
    use crate::stubs::mock_relay_manager::tests::mock_relay_manager;
    use crate::stubs::mock_schedule_check::tests::mock_schedule_check;
    use crate::stubs::mock_tank_level_switch::tests::mock_tank_level_switch;

    use crate::thermal::heating::{
        Heating, HeatingChannels, HeatingStatsDataTransferTrait, LOG_TARGET_HEATING,
    };
    use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
    use crate::utilities::common::tests::update_min_max_actuation_duration;
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};
    use crate::utilities::sawtooth_profile::SawToothProfileConfig;

    pub struct HeatingStatsMockDataTransfer {
        heating_stats_entry_previous: HeatingStatsEntry,
        heating_stats_entry: HeatingStatsEntry,
    }

    impl HeatingStatsMockDataTransfer {
        // Creates a new `HeatingStatsMockDataTransfer` instance.
        //
        // This constructor initializes the mock data transfer object for testing heating statistics.
        // It sets up two `HeatingStatsEntry` instances, both initialized with default (zero) values
        // for the current date, to simulate the previous and current day's statistics.
        //
        // # Returns
        // A new `HeatingStatsMockDataTransfer` struct.
        pub fn new() -> HeatingStatsMockDataTransfer {
            Self {
                heating_stats_entry_previous: HeatingStatsEntry::new(),
                heating_stats_entry: HeatingStatsEntry::new(),
            }
        }
    }

    impl HeatingStatsDataTransferTrait for HeatingStatsMockDataTransfer {
        // Transfers the provided daily heating statistics entry to the SQL database.
        //
        // This implementation updates the `date` of the `heating_stats_entry` to the
        // current database date (ensuring consistency) and then attempts to insert
        // or update this record in the database. Errors during database operations are logged.
        //
        // # Arguments
        // * `heating_stats_entry` - The `HeatingStatsEntry` containing the statistics for the day.
        //   This is passed by value, meaning ownership is transferred to this function.
        // * `sql_interface_heating` - A mutable reference to the `SqlInterfaceHeating` instance,
        //   used for database interaction.
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
        // Creates a new `SqlInterfaceHeatingMockMidnightCalculator` instance.
        //
        // This constructor initializes the mock calculator, setting the specific
        // number of seconds until midnight that it will consistently return during tests.
        // This allows for predictable simulation of time progression in testing scenarios.
        //
        // # Arguments
        // * `seconds_to_midnight` - The fixed `i64` value representing the number of
        //   seconds until midnight that this mock will provide.
        //
        // # Returns
        // A new `SqlInterfaceHeatingMockMidnightCalculator` struct.
        pub fn new(seconds_to_midnight: i64) -> SqlInterfaceHeatingMockMidnightCalculator {
            Self {
                seconds_to_midnight,
            }
        }
    }

    impl SqlInterfaceHeatingMidnightCalculatorTrait for SqlInterfaceHeatingMockMidnightCalculator {
        // Provides a predefined duration until midnight for testing heating statistics transfer.
        //
        // This mock implementation of `get_duration_until_midnight` always returns a
        // fixed `i64` value, `self.seconds_to_midnight`. This allows tests to simulate
        // various durations until midnight without actually waiting for real-world time to pass,
        // enabling deterministic and fast testing of the heating statistics transfer logic.
        //
        // # Returns
        // An `Ok(i64)` containing the predefined number of seconds until midnight.
        // This function will always succeed (`Ok`) as it does not interact with a real database.
        fn get_duration_until_midnight(&mut self) -> Result<i64, SqlInterfaceError> {
            Ok(self.seconds_to_midnight) // seconds to midnight after which heating stats are inserted into the database
                                         // for testing, we do not want to wait until midnight
        }
    }

    // Prepares a `Heating` control instance with a mock midnight calculator and a clean test database.
    //
    // This helper function is designed exclusively for testing the `Heating` module. It sets up
    // a controlled environment by:
    // 1.  Loading a generic test configuration and overriding specific settings (e.g., `stop_on_refill_error_delay`).
    // 2.  Optionally applying a custom `SawToothProfileConfig` for precise control over heating patterns.
    // 3.  Connecting to and **truncating a dedicated test database** to ensure a clean state for heating statistics.
    // 4.  Injecting a **`SqlInterfaceHeatingMockMidnightCalculator`** to return a predefined `seconds_to_midnight`,
    //     allowing deterministic testing of daily statistic transfers without real-time dependency.
    //
    // # Arguments
    // * `seconds_to_midnight` - The fixed number of seconds (as `i64`) the mock midnight calculator will return.
    //   This directly controls when the `Heating` module attempts to transfer daily statistics.
    // * `test_db_number` - A unique `u32` identifier (e.g., `7`, `8`, `9`) used to select a specific
    //   test database (`aquarium_test_XX`) from the configuration.
    // * `saw_tooth_profile_config_opt` - An `Option<SawToothProfileConfig>`. If `Some`, this custom
    //   sawtooth profile configuration will override the one loaded from the TOML file for the `Heating` module.
    //
    // # Returns
    // A tuple `(ConfigData, Heating)`:
    // - The `ConfigData` struct, containing the loaded and modified configuration.
    // - The fully initialized `Heating` struct, ready for test execution.
    //
    // # Panics
    // This function will panic if:
    // - `read_config_file_with_test_database` fails (e.g., file not found, invalid DB number).
    // - It fails to connect to the SQL database.
    // - It fails to truncate the `heatingstats` table in the database.
    // - It encounters any issue during the initialization of the `SqlInterfaceHeating` or `Heating` structs.
    pub fn prepare_heating_tests_mock_midnight_calculator(
        seconds_to_midnight: i64,
        test_db_number: u32,
        saw_tooth_profile_config_opt: Option<SawToothProfileConfig>,
    ) -> (ConfigData, Heating) {
        // prepare struct Heating for further tests
        let mut config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            test_db_number,
        );
        config.heating.stop_on_refill_error_delay = 2;

        match saw_tooth_profile_config_opt {
            Some(saw_tooth_profile_config) => {
                config.heating.saw_tooth_profile_config = saw_tooth_profile_config;
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

    #[test]
    // Check if the heater remains switched off with a high temperature.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Test case uses test database #11.
    pub fn test_heating_with_measured_temperature_high() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3600, 11, None);

        // Channels for the test object. The test environment only uses one of the channels
        let (tx_heating_to_tank_level_switch, rx_tank_level_switch_from_heating) = mpsc::channel();
        let (tx_tank_level_switch_to_heating, _rx_heating_from_tank_level_switch) = mpsc::channel();
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_relay_manager, rx_relay_manager_from_heating) = mpsc::channel();
        let (tx_relay_manager_to_heating, rx_heating_from_relay_manager) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

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

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_water_temperature = Arc::new(Mutex::new(30.0));

        // Mutex for ambient temperature normally coming from Ambient
        let mutex_ambient_temperature = Arc::new(Mutex::new(24.0));

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

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_heating,
                    &rx_relay_manager_from_heating,
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
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_tank_level_switch_for_test_case_finish =
                    tx_heating_to_tank_level_switch.clone();
                let tx_heating_to_relay_manager_for_test_case_finish =
                    tx_heating_to_relay_manager.clone();

                let heating_channels = HeatingChannels {
                    tx_heating_to_relay_manager,
                    rx_heating_from_relay_manager,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging_opt: None,
                };

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    heating_channels,
                    &mut heating_stats_transfer,
                    mutex_water_temperature,
                    mutex_ambient_temperature,
                    mutex_tank_level_switch_signals,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_tank_level_switch_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking reaction to high temperature succeeded.");
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
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Check if the heater is switched on and is not switched off with low temperature.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Check if the heater is switched off after sending Quit command and before sending Terminate command.
    // Test case uses test database #12.
    pub fn test_heating_with_measured_temperature_low() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3600, 12, None);

        // Channels for the test object. The test environment only uses one of the channels
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_relay_manager, rx_relay_manager_from_heating) = mpsc::channel();
        let (tx_relay_manager_to_heating, rx_heating_from_relay_manager) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

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

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_water_temperature = Arc::new(Mutex::new(20.0));

        // Mutex for ambient temperature normally coming from Ambient
        let mutex_ambient_temperature = Arc::new(Mutex::new(24.0));

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

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_heating,
                    &rx_relay_manager_from_heating,
                );
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
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_relay_manager_for_test_case_finish =
                    tx_heating_to_relay_manager.clone();

                let heating_channels = HeatingChannels {
                    tx_heating_to_relay_manager,
                    rx_heating_from_relay_manager,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging_opt: None,
                };

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    heating_channels,
                    &mut heating_stats_transfer,
                    mutex_water_temperature,
                    mutex_ambient_temperature,
                    mutex_tank_level_switch_signals,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking reaction to low temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Check if the heater is switched on and off during two cycles of the saw tooth profile.
    // Duration of the heater switched on shall equal the duration of the heater switched off (1% tolerance).
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Test case uses test database #13.
    pub fn test_heating_with_measured_temperature_50_percent() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(
            3600,
            13,
            Some(SawToothProfileConfig::new(6, 1, 6, 1)),
        );

        // Channels for the test object. The test environment only uses one of the channels
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_relay_manager, rx_relay_manager_from_heating) = mpsc::channel();
        let (tx_relay_manager_to_heating, rx_heating_from_relay_manager) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

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

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_water_temperature = Arc::new(Mutex::new(25.0));

        // Mutex for ambient temperature normally coming from Ambient
        let mutex_ambient_temperature = Arc::new(Mutex::new(24.0));

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

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_heating,
                    &rx_relay_manager_from_heating,
                );
                println!(
                    "Mock relay manager received {} commands:",
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
                // hence, platform-specific asserts are necessary
                cfg_if::cfg_if! {
                    if #[cfg(target_os = "linux")] {
                        assert_le!(second_last_actuation_duration, 7100);
                        assert_ge!(second_last_actuation_duration, 6400);
                        assert_le!(third_last_actuation_duration, 7100);
                        assert_ge!(third_last_actuation_duration, 6400);
                        assert_le!(fourth_last_actuation_duration, 7100);
                        assert_ge!(fourth_last_actuation_duration, 6400);
                        assert_le!(fifth_last_actuation_duration, 7100);
                        assert_ge!(fifth_last_actuation_duration, 6400);
                        assert_le!(sixth_last_actuation_duration, 7100);
                        assert_ge!(sixth_last_actuation_duration, 6400);
                }
                    else {
                        assert_le!(second_last_actuation_duration, 7400);
                        assert_ge!(second_last_actuation_duration, 6400);
                        assert_le!(third_last_actuation_duration, 7400);
                        assert_ge!(third_last_actuation_duration, 6400);
                        assert_le!(fourth_last_actuation_duration, 7400);
                        assert_ge!(fourth_last_actuation_duration, 6400);
                        assert_le!(fifth_last_actuation_duration, 7400);
                        assert_ge!(fifth_last_actuation_duration, 6400);
                        assert_le!(sixth_last_actuation_duration, 7400);
                        assert_ge!(sixth_last_actuation_duration, 6400);
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
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_relay_manager_for_test_case_finish =
                    tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();

                let heating_channels = HeatingChannels {
                    tx_heating_to_relay_manager,
                    rx_heating_from_relay_manager,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging_opt: None,
                };

                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    heating_channels,
                    &mut heating_stats_transfer,
                    mutex_water_temperature,
                    mutex_ambient_temperature,
                    mutex_tank_level_switch_signals,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking reaction to medium temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case checks if the Quit command is correctly processed
    // - application shall switch off the heater before exiting
    // Test case uses test database #14.
    pub fn test_heating_with_pending_quit_terminate_command() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3, 14, None);

        // Channels for the test object. The test environment only uses one of the channels
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_relay_manager, rx_relay_manager_from_heating) = mpsc::channel();
        let (tx_relay_manager_to_heating, rx_heating_from_relay_manager) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        let start_time = SystemTime::now();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

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

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_water_temperature = Arc::new(Mutex::new(25.0));

        // Mutex for ambient temperature normally coming from Ambient
        let mutex_ambient_temperature = Arc::new(Mutex::new(24.0));

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

        // thread for mock relay manager
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_heating,
                    &rx_relay_manager_from_heating,
                );
                println!(
                    "Mock relay manager received {} commands.",
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

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_relay_manager_for_test_case_finish =
                    tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();

                let heating_channels = HeatingChannels {
                    tx_heating_to_relay_manager,
                    rx_heating_from_relay_manager,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging_opt: None
                };

                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    heating_channels,
                    &mut heating_stats_transfer,
                    mutex_water_temperature,
                    mutex_ambient_temperature,
                    mutex_tank_level_switch_signals,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                // check if heater actuation has taken place
                let mutex_data = mutex_device_scheduler_heating.lock().unwrap();
                assert_eq!(*mutex_data, 1);
            let finish_time = SystemTime::now();
            let execution_duration = finish_time.duration_since(start_time).unwrap();
            assert_le!(execution_duration.as_millis(), 650);
            println!(
                "* [Heating] checking heating execution with pending quit/terminate command (time limit) succeeded."
            );
        })
        .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case checks if waiting for midnight and statistical calculations are computed correctly.
    // Test case uses test database #15.
    pub fn test_heating_wait_for_midnight_calc_stats_heater_on() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3, 15, None);

        // Channels for the test object. The test environment only uses one of the channels
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_relay_manager, rx_relay_manager_from_heating) = mpsc::channel();
        let (tx_relay_manager_to_heating, rx_heating_from_relay_manager) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // get initial data for calculation of reference value
        let initial_heating_stats_entry = heating
            .sql_interface_heating
            .get_single_heating_stats_from_database()
            .unwrap_or_else(|_| {
                // database responded with an empty response - let's create an empty entry
                HeatingStatsEntry {
                    date: Local::now().date_naive(),
                    energy: 0.0,
                    ambient_temperature_average_value: 0.0,
                    ambient_temperature_average_counter: 0,
                    water_temperature_average_value: 0.0,
                    water_temperature_average_counter: 0,
                    heating_control_runtime: 0,
                }
            });

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

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_water_temperature = Arc::new(Mutex::new(20.0));

        // Mutex for ambient temperature normally coming from Ambient
        let mutex_ambient_temperature = Arc::new(Mutex::new(24.0));

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

        // thread for mock relay manager
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                mock_relay_manager(&tx_relay_manager_to_heating, &rx_relay_manager_from_heating);
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

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_relay_manager_for_test_case_finish =
                    tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();

                let heating_channels = HeatingChannels {
                    tx_heating_to_relay_manager,
                    rx_heating_from_relay_manager,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging_opt: None
                };

                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    heating_channels,
                    &mut heating_stats_transfer,
                    mutex_water_temperature,
                    mutex_ambient_temperature,
                    mutex_tank_level_switch_signals,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

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
                assert_le!(
                    0.16666666,
                    heating_stats_transfer.heating_stats_entry_previous.energy
                );
                assert_ge!(
                    0.16666667,
                    heating_stats_transfer.heating_stats_entry_previous.energy
                );
                assert_le!(
                    23.999,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_value
                );
                assert_ge!(
                    24.001,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_value
                );
                assert_le!(
                    19.999,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_value
                );
                assert_ge!(
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
                assert_le!(0.0555, heating_stats_transfer.heating_stats_entry.energy);
                assert_ge!(0.0556, heating_stats_transfer.heating_stats_entry.energy);
                assert_le!(
                    23.999,
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_value
                );
                assert_ge!(
                    24.001,
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_value
                );
                assert_le!(
                    19.999,
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_value
                );
                assert_ge!(
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
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case checks if waiting for midnight and statistical calculations are computed correctly
    // Test case uses test database #16.
    pub fn test_heating_wait_for_midnight_calc_stats_heater_off() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3, 16, None);

        // Channels for the test object. The test environment only uses one of the channels
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_relay_manager, rx_relay_manager_from_heating) = mpsc::channel();
        let (tx_relay_manager_to_heating, rx_heating_from_relay_manager) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        // get initial data for calculation of reference value
        let initial_heating_stats_entry = heating
            .sql_interface_heating
            .get_single_heating_stats_from_database()
            .unwrap_or_else(|_| {
                // database responded with an empty response - let's create an empty entry
                HeatingStatsEntry {
                    date: Local::now().date_naive(),
                    energy: 0.0,
                    ambient_temperature_average_value: 0.0,
                    ambient_temperature_average_counter: 0,
                    water_temperature_average_value: 0.0,
                    water_temperature_average_counter: 0,
                    heating_control_runtime: 0,
                }
            });

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

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_water_temperature = Arc::new(Mutex::new(30.0));

        // Mutex for ambient temperature normally coming from Ambient
        let mutex_ambient_temperature = Arc::new(Mutex::new(24.0));

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

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

        // thread for mock relay manager
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                mock_relay_manager(&tx_relay_manager_to_heating, &rx_relay_manager_from_heating);
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

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_relay_manager_for_test_case_finish =
                    tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();

                let heating_channels = HeatingChannels {
                    tx_heating_to_relay_manager,
                    rx_heating_from_relay_manager,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging_opt: None
                };

                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    heating_channels,
                    &mut heating_stats_transfer,
                    mutex_water_temperature,
                    mutex_ambient_temperature,
                    mutex_tank_level_switch_signals,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

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
                assert_le!(
                    -0.001,
                    heating_stats_transfer.heating_stats_entry_previous.energy
                );
                assert_ge!(
                    0.001,
                    heating_stats_transfer.heating_stats_entry_previous.energy
                );
                assert_le!(
                    23.999,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_value
                );
                assert_ge!(
                    24.001,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .ambient_temperature_average_value
                );
                assert_le!(
                    29.999,
                    heating_stats_transfer
                        .heating_stats_entry_previous
                        .water_temperature_average_value
                );
                assert_ge!(
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
                assert_le!(-0.001, heating_stats_transfer.heating_stats_entry.energy);
                assert_ge!(0.001, heating_stats_transfer.heating_stats_entry.energy);
                assert_le!(
                    23.999,
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_value
                );
                assert_ge!(
                    24.001,
                    heating_stats_transfer
                        .heating_stats_entry
                        .ambient_temperature_average_value
                );
                assert_le!(
                    29.999,
                    heating_stats_transfer
                        .heating_stats_entry
                        .water_temperature_average_value
                );
                assert_ge!(
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
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case checks if a negative schedule checker result is processed correctly.
    // Test case uses test database #17.
    pub fn test_heating_blocked_by_schedule() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3600, 17, None);

        // Channels for the test object. The test environment only uses one of the channels
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_relay_manager, rx_relay_manager_from_heating) = mpsc::channel();
        let (tx_relay_manager_to_heating, rx_heating_from_relay_manager) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_heating,
                    &rx_schedule_check_from_heating,
                    None,
                    false,
                );
            })
            .unwrap();

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_water_temperature = Arc::new(Mutex::new(20.0));

        // Mutex for ambient temperature normally coming from Ambient
        let mutex_ambient_temperature = Arc::new(Mutex::new(24.0));

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

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_heating,
                    &rx_relay_manager_from_heating,
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
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                rx_signal_handler_from_heating.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
            })
            .unwrap();

        // make sure all mock threads are running when instantiating the test object
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_relay_manager_for_test_case_finish =
                    tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();

                let heating_channels = HeatingChannels {
                    tx_heating_to_relay_manager,
                    rx_heating_from_relay_manager,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging_opt: None,
                };

                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    heating_channels,
                    &mut heating_stats_transfer,
                    mutex_water_temperature,
                    mutex_ambient_temperature,
                    mutex_tank_level_switch_signals,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);
                println!("* [Heating] checking if schedule checker can block actuation.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case checks if component protection is executed.
    // Test case uses test database #18.
    pub fn test_heating_component_protection_when_tank_level_low() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3, 18, None);

        // Channels for the test object. The test environment only uses one of the channels
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_heating_to_relay_manager, rx_relay_manager_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_relay_manager_to_heating, rx_heating_from_relay_manager) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
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

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_water_temperature = Arc::new(Mutex::new(20.0));

        // Mutex for ambient temperature normally coming from Ambient
        let mutex_ambient_temperature = Arc::new(Mutex::new(24.0));

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));

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

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_heating,
                    &rx_relay_manager_from_heating,
                );
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
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_relay_manager_for_test_case_finish =
                    tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();

                let heating_channels = HeatingChannels {
                    tx_heating_to_relay_manager,
                    rx_heating_from_relay_manager,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging_opt: None,
                };

                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    heating_channels,
                    &mut heating_stats_transfer,
                    mutex_water_temperature,
                    mutex_ambient_temperature,
                    mutex_tank_level_switch_signals,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking if schedule checker can block actuation.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }

    #[test]
    // Test case runs heating control and triggers inhibition by sending a stop message via the channel.
    // After verification that heating is stopped,
    // The test case sends the start message via the channel and verifies if heating control
    // resumes operation.
    // Test case uses test database #19.
    pub fn test_messaging_stops_starts_heating() {
        let (_, mut heating) = prepare_heating_tests_mock_midnight_calculator(3600, 19, None);

        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_heating_to_relay_manager, rx_relay_manager_from_heating) = mpsc::channel();
        let (tx_relay_manager_to_heating, rx_heating_from_relay_manager) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check) = mpsc::channel();
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating) = mpsc::channel();
        let (tx_messaging_to_heating, rx_heating_from_messaging) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
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

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_water_temperature = Arc::new(Mutex::new(20.0));

        // Mutex for ambient temperature normally coming from Ambient
        let mutex_ambient_temperature = Arc::new(Mutex::new(24.0));

        // Mutex for tank level switch signals
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

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

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_heating,
                    &rx_relay_manager_from_heating,
                );
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
                let sleep_duration_100_millis = Duration::from_millis(100);
                let spin_sleeper = SpinSleeper::default();

                // initial wait so that heating control can switch on
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millis);
                }
                // check the initial state of mutex: should be already switched on
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 1);

                // sending a message requesting stop of heating control
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
                    spin_sleeper.sleep(sleep_duration_100_millis);
                }
                // check if heating is actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 2);

                // sending a message requesting restart of heating control
                match tx_messaging_to_heating.send(InternalCommand::Start) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending start command to test object ({e:?})",
                            LOG_TARGET_HEATING
                        );
                    }
                }

                // Wait for 1 second. Heating control will switch on
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millis);
                }
                // check if heating is actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 3);

                // sending a message requesting stop of heating control
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
                    spin_sleeper.sleep(sleep_duration_100_millis);
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

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let tx_heating_to_schedule_check_for_test_case_finish =
                    tx_heating_to_schedule_check.clone();
                let tx_heating_to_relay_manager_for_test_case_finish =
                    tx_heating_to_relay_manager.clone();

                let mut heating_stats_transfer = HeatingStatsMockDataTransfer::new();

                let heating_channels = HeatingChannels {
                    tx_heating_to_relay_manager,
                    rx_heating_from_relay_manager,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_data_logger,
                    rx_heating_from_data_logger,
                    tx_heating_to_schedule_check,
                    rx_heating_from_schedule_check,
                    rx_heating_from_messaging_opt: Some(rx_heating_from_messaging),
                };

                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    heating_channels,
                    &mut heating_stats_transfer,
                    mutex_water_temperature,
                    mutex_ambient_temperature,
                    mutex_tank_level_switch_signals,
                );
                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_heating_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
                let _ =
                    tx_heating_to_relay_manager_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Heating] checking if messaging can block and restart actuation.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
        join_handle_mock_data_logger
            .join()
            .expect("Mock Data Logger thread did not finish.");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
    }
}
