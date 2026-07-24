/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages the aquarium's cooling system through a dedicated control thread.
//!
//! This module implements the `Ventilation` control thread, a long-running process
//! responsible for controlling the aquarium's water temperature in conjunction with
//! the heating control. It's a sophisticated controller that integrates sensor data,
//! scheduling, and external commands to provide robust and efficient evaporative cooling.
//!
//! ## Key Responsibilities
//!
//! - **Temperature Control**: Implements a hybrid control strategy:
//!   - **Hysteresis Control**: Switches the ventilation fans fully on when the temperature
//!     exceeds `switch_on_temperature` and fully off when it drops below `switch_off_temperature`.
//!   - **Proportional Control**: Within the hysteresis band, it uses a `SawToothProfile`
//!     to achieve a "PWM-like" effect, modulating the fans' on-time to provide
//!     smoother temperature regulation and prevent rapid on/off cycling.
//!
//! - **External Control**: Listens for `Start` and `Stop` commands from other parts of
//!   the system (e.g., a messaging interface), allowing ventilation to be manually
//!   inhibited or re-enabled.
//!
//! - **Scheduling**: Respects a system-wide schedule by querying the `ScheduleCheck`
//!   module. It uses a configurable strategy (`switch_on_when_out_of_schedule`) to
//!   determine its behavior when outside the allowed time window.
//!
//! - **Configurable Shutdown Behavior**: The `switch_on_when_terminating` flag in the
//!   configuration determines whether the fans should be left ON or OFF when the
//!   unwinding application shuts down.
//!
//! ## Key Components
//!
//! - **`Ventilation` Struct**: The core state machine for the thread. It holds the
//!   configuration and a set of log-suppression flags.
//!
//! - **`execute()` Method**: The main thread loop. This monolithic function orchestrates
//!   all the module's responsibilities, from reading sensor data to actuating the
//!   ventilation relay.
//!
//! - **`actuate_ventilation()`**: A private helper that encapsulates the channel communication
//!   required to send a command to the `RelayManager` and wait for its response.
//!
//! - **`conditional_ventilation_switch()`**: A private helper that encapsulates the logic
//!   for switching the ventilation ON or OFF only if its state has changed, reducing
//!   redundant commands.
//!
//! ## Design and Architecture
//!
//! - **Decoupled and Concurrent**: The `Ventilation` thread runs independently, communicating
//!   with other threads asynchronously via channels (for commands) and mutexes (for
//!   shared sensor data). This decouples the control logic from the hardware drivers
//!   and data sources.
//!
//! - **Stateful and Robust**: The thread maintains its own state (`ventilation_state`,
//!   `ventilation_inhibited`) and uses flags to manage log suppression and
//!   recover from transient errors.
//!
//! - **Data-Driven**: Its behavior is driven by a combination of real-time sensor data,
//!   pre-configured parameters from a TOML file, and dynamic setpoints fetched from
//!   a database.

#[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
use log::{info, warn};

use crate::database::thermal_set_value_updater_trait::ThermalSetValueUpdaterTrait;
use crate::utilities::channel_content::{ActuatorState, AquariumDevice, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::sawtooth_profile::SawToothProfile;

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::thermal::temperature_gradient_monitor::TemperatureGradientMonitorTrait;
use crate::thermal::ventilation_channels::VentilationChannels;
use crate::thermal::ventilation_config::VentilationConfig;
use crate::thermal::ventilation_mutexes::VentilationMutexes;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::check_mutex_access_duration::CheckMutexAccessDurationTrait;
use crate::utilities::logger::log_error_chain;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;
use crate::{perform_schedule_check, update_thermal_set_values};
#[cfg(all(not(test), not(feature = "test-utils"), target_os = "linux"))]
// only required in real-life execution
use nix::unistd::gettid;

const CYCLE_TIME_VENTILATION_MILLIS: u64 = 500;

const TIME_INCREMENT_VENTILATION_SECS: f32 = CYCLE_TIME_VENTILATION_MILLIS as f32 / 1000.0;

/// allow max. 10 milliseconds for mutex to be blocked by any other thread
const MAX_MUTEX_ACCESS_DURATION_MILLIS: u64 = 10;

/// Sends a command to the relay manager to actuate the ventilation and waits for a response.
///
/// This private helper function encapsulates the logic for sending a command
/// (either `SwitchOn` or `SwitchOff`) to the relay manager. It handles channel
/// communication, error logging, and updates the shared actuation mutex.
///
/// # Arguments
/// * `command` - The `InternalCommand` to send (`SwitchOn` or `SwitchOff`).
/// * `mutex_device_scheduler_ventilation` - A reference to the shared actuation mutex.
/// * `mutex_blocked_during_actuation` - A mutable flag to track if the actuation mutex was blocked.
/// * `ventilation_channels` - A mutable reference to the struct containing the channels.
/// * `is_multi_thread` - A boolean flag indicating if the Tokio runtime is running in multithreaded mode.
async fn actuate_ventilation(
    command: InternalCommand,
    mutex_device_scheduler_ventilation: &Arc<tokio::sync::Mutex<i32>>,
    mutex_blocked_during_actuation: &mut bool,
    ventilation_channels: &mut VentilationChannels,
    _is_multi_thread: bool,
) {
    // reduce the probability of wrong warning
    *mutex_blocked_during_actuation |= mutex_device_scheduler_ventilation.try_lock().is_err();

    let mut mutex_data = mutex_device_scheduler_ventilation.lock().await;

    // Use if let for cleaner error handling
    if let Err(e) = ventilation_channels.send_to_relay_manager(command.clone()) {
        let error_message = format!("Channel communication to relay manager for {command} failed.");
        log_error_chain(module_path!(), &error_message, e);
    } else {
        // now listen for the response
        if let Err(e) = ventilation_channels.receive_from_relay_manager().await {
            let error_message =
                format!("Receiving answer from relay manager for {command} failed.");
            log_error_chain(module_path!(), &error_message, e);
        }
    }

    *mutex_data = mutex_data.saturating_add(1);
}

/// The function commands the relay manager to switch on the ventilation device.
async fn switch_on_ventilation(
    mutex_device_scheduler_ventilation: &Arc<tokio::sync::Mutex<i32>>,
    mutex_blocked_during_actuation: &mut bool,
    ventilation_channels: &mut VentilationChannels,
    is_multi_thread: bool,
) {
    actuate_ventilation(
        InternalCommand::SwitchOn(AquariumDevice::Ventilation),
        mutex_device_scheduler_ventilation,
        mutex_blocked_during_actuation,
        ventilation_channels,
        is_multi_thread,
    )
    .await;
}

/// The function commands the relay manager to switch off the ventilation device.
async fn switch_off_ventilation(
    mutex_device_scheduler_ventilation: &Arc<tokio::sync::Mutex<i32>>,
    mutex_blocked_during_actuation: &mut bool,
    ventilation_channels: &mut VentilationChannels,
    is_multi_thread: bool,
) {
    actuate_ventilation(
        InternalCommand::SwitchOff(AquariumDevice::Ventilation),
        mutex_device_scheduler_ventilation,
        mutex_blocked_during_actuation,
        ventilation_channels,
        is_multi_thread,
    )
    .await;
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Holds the configuration and the implementation for the ventilation control.
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     atlas_scientific[Atlas Scientific] --> ventilation
///     ventilation --> relay_manager[Relay Manager]
///     relay_manager --> ventilation
///     ventilation --> signal_handler[Signal Handler]
///     signal_handler --> ventilation
///     ventilation --> data_logger[Data Logger]
///     data_logger --> ventilation
///     ventilation --> schedule_check[Schedule Check]
///     schedule_check --> ventilation
///     messaging[Messaging] --> ventilation
/// ```
pub struct Ventilation {
    pub config: VentilationConfig,

    /// inhibition flag to avoid flooding the log file with repeated messages to send request via the channel to schedule check
    lock_error_channel_send_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages to receive request via the channel from schedule check
    lock_error_channel_receive_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex
    pub lock_warn_max_mutex_access_duration: bool,

    /// for testing purposes: record when mutex access time is exceeded without resetting it
    pub mutex_access_duration_exceeded: bool,

    /// inhibition flag to avoid flooding the log file about failure to update the set values
    lock_error_ventilation_set_value_read_failure: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via the channel
    pub lock_warn_inapplicable_command_signal_handler: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about the channel being disconnected
    pub lock_error_channel_receive_termination: bool,

    /// Maximum permissible access duration for Mutex
    pub max_mutex_access_duration: Duration,

    /// The time the actuator was last turned on continuously.
    pub last_actuator_on_time: Option<Instant>,

    /// Inhibition flag to prevent log flooding for gradient monitoring.
    pub log_inhibition_gradient: bool,

    /// flag indicating that actuator has been disabled due to indication from temperature gradient monitor
    pub actuator_disabled: bool,
}

impl TemperatureGradientMonitorTrait for Ventilation {
    /// Performs the `max_time_actuator_effect_monitor` operation.
    fn max_time_actuator_effect_monitor(&self) -> u64 {
        self.config.max_time_actuator_effect_monitor
    }

    /// Performs the `monitor_state` operation.
    fn monitor_state(&mut self) -> (&mut Option<Instant>, &mut bool) {
        (
            &mut self.last_actuator_on_time,
            &mut self.log_inhibition_gradient,
        )
    }
}

impl Ventilation {
    /// Creates a new `Ventilation` control instance.
    ///
    /// This constructor initializes the ventilation control module with its specific
    /// configuration. It sets all internal "lock" flags to `false` by default; these
    /// flags are used during operation to prevent log files from being flooded with
    /// repeated error or warning messages.
    ///
    /// # Arguments
    /// * `config` - **Configuration data** for the ventilation control, loaded from a TOML file.
    ///   This includes parameters such as temperatures for switching on/off, and various
    ///   behavioral strategies.
    ///
    /// # Returns
    /// A new `Ventilation` struct, ready to manage the aquarium's ventilation system.
    pub fn new(config: VentilationConfig) -> Ventilation {
        Self {
            config,
            lock_error_channel_send_schedule_check: false,
            lock_error_channel_receive_schedule_check: false,
            lock_warn_max_mutex_access_duration: false,
            lock_error_ventilation_set_value_read_failure: false,
            lock_warn_inapplicable_command_signal_handler: false,
            mutex_access_duration_exceeded: false,
            lock_error_channel_receive_termination: false,
            max_mutex_access_duration: Duration::from_millis(MAX_MUTEX_ACCESS_DURATION_MILLIS),
            last_actuator_on_time: None,
            log_inhibition_gradient: false,
            actuator_disabled: false,
        }
    }

    /// Calculates a **normalized control deviation** based on the measured temperature and configured thresholds.
    ///
    /// This private helper function determines how far the `measured_value` deviates
    /// from the `switch_off_temperature` relative to the total temperature control range
    /// (`switch_on_temperature` to `switch_off_temperature`). The result is a float
    /// between `0.0` and `1.0`. A value of `0.0` means the temperature is at or below
    /// `switch_off_temperature`, and `1.0` means it's at or above `switch_on_temperature`.
    ///
    /// # Arguments
    /// * `measured_value` - The current measured temperature (`f32`).
    ///
    /// # Returns
    /// An `f32` representing the normalized control deviation, ranging from `0.0` to `1.0`.
    /// Returns `0.0` if `switch_on_temperature` is not greater than `switch_off_temperature`
    /// (i.e., `target_value_delta` is not positive), indicating an invalid configuration.
    fn calc_normalized_control_deviation(&self, measured_value: f32) -> f32 {
        let target_value_delta: f32 =
            self.config.switch_on_temperature - self.config.switch_off_temperature;
        let measured_value_delta: f32 = measured_value - self.config.switch_off_temperature;
        if target_value_delta > 0.0 {
            1.0 - measured_value_delta / target_value_delta
        } else {
            0.0 // default value when target values are not properly set up.
        }
    }

    /// Conditionally switches the ventilation device ON or OFF based on a boolean condition.
    ///
    /// This private helper function encapsulates the logic for changing the ventilation's
    /// state. It only triggers a switch if the `ventilation_state` is not already
    /// in the desired `condition`. It relies on `switch_on_ventilation` and `switch_off_ventilation`
    /// to interact with the relay manager.
    ///
    /// # Arguments
    /// * `condition` - A boolean value. If `true`, the function attempts to switch the ventilation ON;
    ///   if `false`, it attempts to switch it OFF.
    /// * `ventilation_state` - A mutable reference to the `ActuatorState` representing the
    ///   current state of the ventilation device, which will be updated by this function.
    /// * `mutex_device_scheduler_ventilation` - A shared mutex for coordinating device actuation.
    /// * `ventilation_channels` - A reference to the `VentilationChannels` struct for communication with the relay manager.
    /// * `mutex_was_blocked_during_actuation` - A mutable flag indicating if the device scheduler mutex was blocked.
    /// * `is_multi_thread` - A boolean flag indicating if the Tokio runtime is running in multithreaded mode.
    async fn conditional_ventilation_switch(
        condition: bool,
        ventilation_state: &mut ActuatorState,
        mutex_device_scheduler_ventilation: &Arc<tokio::sync::Mutex<i32>>,
        ventilation_channels: &mut VentilationChannels,
        mutex_was_blocked_during_actuation: &mut bool,
        is_multi_thread: bool,
    ) {
        match condition {
            true => {
                // switch on the ventilation if not already on
                if *ventilation_state != ActuatorState::On {
                    switch_on_ventilation(
                        mutex_device_scheduler_ventilation,
                        mutex_was_blocked_during_actuation,
                        ventilation_channels,
                        is_multi_thread,
                    )
                    .await;
                    *ventilation_state = ActuatorState::On;
                }
            }
            false => {
                // switch off the ventilation if not already off
                if *ventilation_state != ActuatorState::Off {
                    switch_off_ventilation(
                        mutex_device_scheduler_ventilation,
                        mutex_was_blocked_during_actuation,
                        ventilation_channels,
                        is_multi_thread,
                    )
                    .await;
                    *ventilation_state = ActuatorState::Off;
                }
            }
        }
    }

    /// Executes the main control loop for the ventilation module.
    ///
    /// This function runs continuously, managing the aquarium's ventilation system. It adjusts
    /// ventilation based on water temperature readings, a calculated sawtooth profile (for
    /// proportional control), and various external commands and schedule limitations.
    ///
    /// The loop periodically reads water temperature, checks schedule permissions, and updates
    /// the ventilation state via the relay manager. It is responsive to `Start`, `Stop`, and `Quit`
    /// commands from external channels. The function maintains a fixed cycle time and ensures
    /// a graceful shutdown upon receiving termination signals.
    ///
    /// # Arguments
    /// * `mutex_device_scheduler_ventilation` - An `Arc<Mutex<i32>>` used for coordinating
    ///   device actuation, preventing parallel operations, and tracking actuation counts.
    /// * `ventilation_channels` - A mutable reference to `VentilationChannels` struct containing all `mpsc`
    ///   channels for communication with other threads (e.g., relay manager, data logger, schedule checker, signal handler).
    /// * `ventilation_set_val_updater` - A mutable reference to an object implementing `ThermalSetValueUpdaterTrait`,
    ///   responsible for updating the set values dynamically by communicating with SQL database.
    /// * `ventilation_mutexes` - Struct containing the mutexes to access data from different threads.
    /// * `is_multi_thread` - A boolean indicating if the tokio runtime is running in multithreaded mode.
    ///
    /// # Returns
    /// This function does not return a value in the traditional sense, as it is designed
    /// to loop indefinitely. It will only break out of its loop and terminate when a `Quit`
    /// command is received from the signal handler, after which it performs final cleanup
    /// (switching the ventilation to its configured terminal state) and confirms shutdown.
    pub async fn execute(
        &mut self,
        mutex_device_scheduler_ventilation: Arc<tokio::sync::Mutex<i32>>,
        ventilation_channels: &mut VentilationChannels,
        ventilation_set_val_updater: &mut impl ThermalSetValueUpdaterTrait,
        ventilation_mutexes: VentilationMutexes,
        is_multi_thread: bool,
    ) {
        #[cfg(all(target_os = "linux", not(test), not(feature = "test-utils")))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid()).await;

        let cycle_time_duration = Duration::from_millis(CYCLE_TIME_VENTILATION_MILLIS);
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let mut saw_tooth_profile = SawToothProfile::new(&self.config.saw_tooth_profile_config);
        let mut measured_water_temperature = 0.0;
        let mut measurement_error_water_temperature: bool;
        let mut ventilation_state: ActuatorState = ActuatorState::Undefined;
        let mut schedule_check_result: bool;
        let mut ventilation_inhibited = false; // state of ventilation control determined by if the start/stop command has been received
        let mut lock_warn_cycle_time_exceeded = false;
        let mut start_time = Instant::now();
        let mut actuation_mutex_was_blocked_during_actuation: bool = false;
        let mut actuator_ineffective: bool = false;

        loop {
            let (quit_command_received, start_command_received, stop_command_received) = self
                .process_external_request(
                    &mut ventilation_channels.rx_ventilation_from_signal_handler,
                    Some(&mut ventilation_channels.rx_ventilation_from_messaging),
                );
            if quit_command_received {
                break;
            }
            if stop_command_received {
                #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                info!(
                    target: module_path!(),
                    "received Stop command. Inhibiting ventilation."
                );
                ventilation_inhibited = true;
            }
            if start_command_received {
                #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                info!(
                    target: module_path!(),
                    "received Start command. Restarting ventilation."
                );
                ventilation_inhibited = false;
            }

            if self.config.active {
                // read water temperature from mutex
                {
                    match ventilation_mutexes.mutex_sensor_manager_signals.lock() {
                        Ok(c) => {
                            measurement_error_water_temperature = false;
                            measured_water_temperature = c.water_temperature;
                        }
                        Err(_) => {
                            measurement_error_water_temperature = true;
                        }
                    };
                }

                // schedule check to see if actuation is allowed
                perform_schedule_check!(
                    ventilation_channels,
                    schedule_check_result,
                    self.lock_error_channel_send_schedule_check,
                    self.lock_error_channel_receive_schedule_check,
                    module_path!()
                );

                // check if set values need to be updated
                update_thermal_set_values!(
                    ventilation_set_val_updater,
                    self.config.switch_off_temperature,
                    self.config.switch_on_temperature,
                    self.lock_error_ventilation_set_value_read_failure,
                    module_path!(),
                    is_multi_thread
                );

                // execute gradient monitoring
                actuator_ineffective = self.temperature_gradient_monitor(
                    &ventilation_mutexes.mutex_temperature_gradient,
                    ventilation_state == ActuatorState::On,
                    false, // ventilation expects a negative gradient
                    #[cfg(all(not(test), not(feature = "test-utils")))]
                    // only required in real-life execution
                    "Ventilation",
                );
                if actuator_ineffective
                    && self.config.switch_off_when_no_effect
                    && ventilation_state == ActuatorState::On
                {
                    switch_off_ventilation(
                        &mutex_device_scheduler_ventilation,
                        &mut actuation_mutex_was_blocked_during_actuation,
                        ventilation_channels,
                        is_multi_thread,
                    )
                    .await;
                    ventilation_state = ActuatorState::Off;
                    self.actuator_disabled = true;
                    #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                    #[cfg(not(tarpaulin_include))]
                    warn!("[{}]: Disabled ventilation because temperature gradient monitor indicates ineffectiveness.", module_path!());
                }

                if schedule_check_result && !ventilation_inhibited {
                    // actuation is allowed. proceed with control.
                    if (measured_water_temperature >= self.config.switch_off_temperature)
                        && (measured_water_temperature <= self.config.switch_on_temperature)
                    {
                        // temperature is within (dynamic) control window
                        if (self.calc_normalized_control_deviation(measured_water_temperature)
                            < saw_tooth_profile.level_normalized)
                            && !measurement_error_water_temperature
                        {
                            // switch on ventilation if not already on
                            if ventilation_state != ActuatorState::On && !self.actuator_disabled {
                                switch_on_ventilation(
                                    &mutex_device_scheduler_ventilation,
                                    &mut actuation_mutex_was_blocked_during_actuation,
                                    ventilation_channels,
                                    is_multi_thread,
                                )
                                .await;
                                ventilation_state = ActuatorState::On;
                            }
                        } else {
                            // switch off ventilation if not already off
                            if ventilation_state != ActuatorState::Off {
                                switch_off_ventilation(
                                    &mutex_device_scheduler_ventilation,
                                    &mut actuation_mutex_was_blocked_during_actuation,
                                    ventilation_channels,
                                    is_multi_thread,
                                )
                                .await;
                                ventilation_state = ActuatorState::Off;
                            }
                        }
                    } else {
                        // The temperature is above or below (dynamic) control window
                        if measured_water_temperature > self.config.switch_on_temperature {
                            // switch on ventilation if not already on
                            if ventilation_state != ActuatorState::On && !self.actuator_disabled {
                                switch_on_ventilation(
                                    &mutex_device_scheduler_ventilation,
                                    &mut actuation_mutex_was_blocked_during_actuation,
                                    ventilation_channels,
                                    is_multi_thread,
                                )
                                .await;
                                ventilation_state = ActuatorState::On;
                            }
                        }
                        if measured_water_temperature < self.config.switch_off_temperature {
                            // switch off ventilation if not already off
                            if ventilation_state != ActuatorState::Off {
                                switch_off_ventilation(
                                    &mutex_device_scheduler_ventilation,
                                    &mut actuation_mutex_was_blocked_during_actuation,
                                    ventilation_channels,
                                    is_multi_thread,
                                )
                                .await;
                                ventilation_state = ActuatorState::Off;
                            }
                        }
                    }
                } else if ventilation_inhibited {
                    // An external request for stopping ventilation control has been received
                    Self::conditional_ventilation_switch(
                        self.config.switch_on_when_external_stop,
                        &mut ventilation_state,
                        &mutex_device_scheduler_ventilation,
                        ventilation_channels,
                        &mut actuation_mutex_was_blocked_during_actuation,
                        is_multi_thread,
                    )
                    .await;
                } else {
                    // schedule check requires stop of ventilation control
                    Self::conditional_ventilation_switch(
                        self.config.switch_on_when_out_of_schedule,
                        &mut ventilation_state,
                        &mutex_device_scheduler_ventilation,
                        ventilation_channels,
                        &mut actuation_mutex_was_blocked_during_actuation,
                        is_multi_thread,
                    )
                    .await;
                }
            }

            let instant_before_locking_mutex = Instant::now();
            let mut instant_after_locking_mutex = Instant::now(); // initialization is overwritten

            // Write signal to mutex
            {
                match ventilation_mutexes.mutex_ventilation_status.lock() {
                    Ok(mut ventilation_status) => {
                        instant_after_locking_mutex = Instant::now();
                        ventilation_status.actuator_switched_on = match ventilation_state {
                            ActuatorState::On => true,
                            ActuatorState::Off => false,
                            _ => false,
                        };
                        ventilation_status.actuator_disabled = self.actuator_disabled;
                        ventilation_status.actuator_ineffective = actuator_ineffective;
                    }
                    Err(_) => {
                        // Do nothing
                    }
                }
            }

            // check if access to mutex took too long
            self.check_mutex_access_duration(
                None,
                instant_after_locking_mutex,
                instant_before_locking_mutex,
            );

            saw_tooth_profile.execute_with(TIME_INCREMENT_VENTILATION_SECS);

            // Manage the loop's cycle time, sleeping or logging a warning as needed.
            let stop_time = Instant::now();
            let execution_duration = stop_time.duration_since(start_time);

            if execution_duration > cycle_time_duration {
                if !lock_warn_cycle_time_exceeded && !actuation_mutex_was_blocked_during_actuation {
                    #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                    #[cfg(not(tarpaulin_include))]
                    warn!(target: module_path!(),
                        "execution duration of {} milliseconds exceeds cycle time of {}",
                        execution_duration.as_millis(),
                        CYCLE_TIME_VENTILATION_MILLIS);
                    lock_warn_cycle_time_exceeded = true;
                }
            } else {
                let remaining_sleep_time = cycle_time_duration - execution_duration;
                tokio::time::sleep(remaining_sleep_time).await;
                lock_warn_cycle_time_exceeded = false;
            }

            // Reset the actuation block flag for the next cycle.
            actuation_mutex_was_blocked_during_actuation = false;

            // Reset the start time for the next cycle.
            start_time = Instant::now();
        }

        if self.config.switch_on_when_terminating {
            // switch on ventilation if not already on
            if ventilation_state != ActuatorState::On {
                switch_on_ventilation(
                    &mutex_device_scheduler_ventilation,
                    &mut actuation_mutex_was_blocked_during_actuation,
                    ventilation_channels,
                    is_multi_thread,
                )
                .await;
            }
        } else {
            // switch off ventilation if not already off
            if ventilation_state != ActuatorState::Off {
                switch_off_ventilation(
                    &mutex_device_scheduler_ventilation,
                    &mut actuation_mutex_was_blocked_during_actuation,
                    ventilation_channels,
                    is_multi_thread,
                )
                .await;
            }
        }

        ventilation_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads (relay manager, Atlas Scientific).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination_async(
            &mut ventilation_channels.rx_ventilation_from_signal_handler,
            sleep_duration_hundred_millis,
            module_path!(),
        )
        .await;
    }
}
