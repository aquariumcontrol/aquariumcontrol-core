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

#[cfg(not(test))]
use log::info;

use crate::database::thermal_set_value_updater_trait::ThermalSetValueUpdaterTrait;
use crate::utilities::channel_content::{ActuatorState, AquariumDevice, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::sawtooth_profile::SawToothProfile;
#[cfg(not(test))]
use log::warn;
use spin_sleep::SpinSleeper;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::thermal::temperature_gradient_monitor::TemperatureGradientMonitorTrait;
use crate::thermal::ventilation_channels::VentilationChannels;
use crate::thermal::ventilation_config::VentilationConfig;
use crate::thermal::ventilation_mutexes::VentilationMutexes;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::check_mutex_access_duration::CheckMutexAccessDurationTrait;
use crate::utilities::logger::log_error_chain;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;
use crate::{manage_cycle_time_thermal, perform_schedule_check, update_thermal_set_values};
#[cfg(all(not(test), target_os = "linux"))]
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
fn actuate_ventilation(
    command: InternalCommand,
    mutex_device_scheduler_ventilation: &Arc<Mutex<i32>>,
    mutex_blocked_during_actuation: &mut bool,
    ventilation_channels: &mut VentilationChannels,
) {
    // reduce the probability of wrong warning
    *mutex_blocked_during_actuation |= mutex_device_scheduler_ventilation.try_lock().is_err();

    // avoid electrical overloads through parallel device actuation
    let mut mutex_data = mutex_device_scheduler_ventilation.lock().unwrap();

    // Use if let for cleaner error handling
    if let Err(e) = ventilation_channels.send_to_relay_manager(command.clone()) {
        let error_message = format!("Channel communication to relay manager for {command} failed.");
        log_error_chain(module_path!(), &error_message, e);
    } else {
        // now listen for the response
        if let Err(e) = ventilation_channels.receive_from_relay_manager() {
            let error_message =
                format!("Receiving answer from relay manager for {command} failed.");
            log_error_chain(module_path!(), &error_message, e);
        }
    };
    *mutex_data = mutex_data.saturating_add(1);
}

/// The function commands the relay manager to switch on the ventilation device.
///
/// This function is a specific wrapper around `actuate_ventilation` that sends the `SwitchOn` command.
///
/// # Arguments
/// * `mutex_device_scheduler_ventilation` - A reference to an `Arc<Mutex<i32>>` used for
///   coordinating device actuation.
/// * `mutex_blocked_during_actuation` - A mutable boolean flag that is set if the
/// * `ventilation_channels` - A mutable reference to the struct containing the channels.
///   Actuation mutex was found to be blocked.
fn switch_on_ventilation(
    mutex_device_scheduler_ventilation: &Arc<Mutex<i32>>,
    mutex_blocked_during_actuation: &mut bool,
    ventilation_channels: &mut VentilationChannels,
) {
    actuate_ventilation(
        InternalCommand::SwitchOn(AquariumDevice::Ventilation),
        mutex_device_scheduler_ventilation,
        mutex_blocked_during_actuation,
        ventilation_channels,
    );
}

/// The function commands the relay manager to switch off the ventilation device.
///
/// This function is a specific wrapper around `actuate_ventilation` that sends the `SwitchOff` command.
///
/// # Arguments
/// * `mutex_device_scheduler_ventilation` - A reference to an `Arc<Mutex<i32>>` used for
///   coordinating device actuation.
/// * `mutex_blocked_during_actuation` - A mutable boolean flag that is set if the
/// * `ventilation_channels` - A mutable reference to the struct containing the channels.
///   Actuation mutex was found to be blocked.
fn switch_off_ventilation(
    mutex_device_scheduler_ventilation: &Arc<Mutex<i32>>,
    mutex_blocked_during_actuation: &mut bool,
    ventilation_channels: &mut VentilationChannels,
) {
    actuate_ventilation(
        InternalCommand::SwitchOff(AquariumDevice::Ventilation),
        mutex_device_scheduler_ventilation,
        mutex_blocked_during_actuation,
        ventilation_channels,
    );
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
    config: VentilationConfig,

    /// inhibition flag to avoid flooding the log file with repeated messages to send request via the channel to schedule check
    lock_error_channel_send_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages to receive request via the channel from schedule check
    lock_error_channel_receive_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex
    pub lock_warn_max_mutex_access_duration: bool,

    /// for testing purposes: record when mutex access time is exceeded without resetting it
    #[cfg(test)]
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
    fn max_time_actuator_effect_monitor(&self) -> u64 {
        self.config.max_time_actuator_effect_monitor
    }

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
            #[cfg(test)]
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
    fn conditional_ventilation_switch(
        condition: bool,
        ventilation_state: &mut ActuatorState,
        mutex_device_scheduler_ventilation: &Arc<Mutex<i32>>,
        ventilation_channels: &mut VentilationChannels,
        mutex_was_blocked_during_actuation: &mut bool,
    ) {
        match condition {
            true => {
                // switch on the ventilation if not already on
                if *ventilation_state != ActuatorState::On {
                    switch_on_ventilation(
                        mutex_device_scheduler_ventilation,
                        mutex_was_blocked_during_actuation,
                        ventilation_channels,
                    );
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
                    );
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
    /// * `ventilation_mutexes` - Struct containing the mutexes to access data from different threads.
    ///
    /// # Returns
    /// This function does not return a value in the traditional sense, as it is designed
    /// to loop indefinitely. It will only break out of its loop and terminate when a `Quit`
    /// command is received from the signal handler, after which it performs final cleanup
    /// (switching the ventilation to its configured terminal state) and confirms shutdown.
    pub fn execute(
        &mut self,
        mutex_device_scheduler_ventilation: Arc<Mutex<i32>>,
        ventilation_channels: &mut VentilationChannels,
        ventilation_set_val_updater: &mut impl ThermalSetValueUpdaterTrait,
        ventilation_mutexes: VentilationMutexes,
    ) {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let cycle_time_duration = Duration::from_millis(CYCLE_TIME_VENTILATION_MILLIS);
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
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
                    ventilation_channels
                        .rx_ventilation_from_messaging_opt
                        .as_mut(),
                );
            if quit_command_received {
                break;
            }
            if stop_command_received {
                #[cfg(not(test))]
                info!(
                    target: module_path!(),
                    "received Stop command. Inhibiting ventilation."
                );
                ventilation_inhibited = true;
            }
            if start_command_received {
                #[cfg(not(test))]
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
                    module_path!()
                );

                // execute gradient monitoring
                actuator_ineffective = self.temperature_gradient_monitor(
                    &ventilation_mutexes.mutex_temperature_gradient,
                    ventilation_state == ActuatorState::On,
                    false, // ventilation expects a negative gradient
                    #[cfg(not(test))]
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
                    );
                    ventilation_state = ActuatorState::Off;
                    self.actuator_disabled = true;
                    #[cfg(not(test))]
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
                                );
                                ventilation_state = ActuatorState::On;
                            }
                        } else {
                            // switch off ventilation if not already off
                            if ventilation_state != ActuatorState::Off {
                                switch_off_ventilation(
                                    &mutex_device_scheduler_ventilation,
                                    &mut actuation_mutex_was_blocked_during_actuation,
                                    ventilation_channels,
                                );
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
                                );
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
                                );
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
                    );
                } else {
                    // schedule check requires stop of ventilation control
                    Self::conditional_ventilation_switch(
                        self.config.switch_on_when_out_of_schedule,
                        &mut ventilation_state,
                        &mutex_device_scheduler_ventilation,
                        ventilation_channels,
                        &mut actuation_mutex_was_blocked_during_actuation,
                    );
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
            manage_cycle_time_thermal!(
                start_time,
                cycle_time_duration,
                CYCLE_TIME_VENTILATION_MILLIS,
                spin_sleeper,
                lock_warn_cycle_time_exceeded,
                actuation_mutex_was_blocked_during_actuation,
                module_path!()
            );
        }

        if self.config.switch_on_when_terminating {
            // switch on ventilation if not already on
            if ventilation_state != ActuatorState::On {
                switch_on_ventilation(
                    &mutex_device_scheduler_ventilation,
                    &mut actuation_mutex_was_blocked_during_actuation,
                    ventilation_channels,
                );
            }
        } else {
            // switch off ventilation if not already off
            if ventilation_state != ActuatorState::Off {
                switch_off_ventilation(
                    &mutex_device_scheduler_ventilation,
                    &mut actuation_mutex_was_blocked_during_actuation,
                    ventilation_channels,
                );
            }
        }

        ventilation_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads (relay manager, Atlas Scientific).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination(
            &mut ventilation_channels.rx_ventilation_from_signal_handler,
            sleep_duration_hundred_millis,
            module_path!(),
        );
    }
}

#[cfg(test)]
pub mod tests {
    use crate::launch::channels::{AquaReceiver, AquaSender, Channels};
    use crate::mocks::mock_relay_manager::tests::mock_relay_manager;
    use crate::mocks::mock_schedule_check::tests::mock_schedule_check;
    use crate::mocks::mock_ventilation::tests::MockSqlInterfaceVentilationSetVals;
    use crate::sensors::sensor_manager::SensorManagerSignals;
    use crate::thermal::thermal_control_status::ThermalControlStatus;
    use crate::thermal::ventilation::Ventilation;
    use crate::thermal::ventilation_mutexes::VentilationMutexes;
    use crate::utilities::channel_content::{ActuatorState, AquariumDevice, InternalCommand};
    use crate::utilities::config::{read_config_file, ConfigData};
    use crate::utilities::sawtooth_profile::SawToothProfileConfig;
    use crate::utilities::signal_handler::SignalHandlerChannels;
    use all_asserts::{assert_ge, assert_le};
    use spin_sleep::SpinSleeper;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    // Prepares and returns a new `Ventilation` struct for use in test cases.
    //
    // This private helper function simplifies test setup by loading the generic
    // test configuration file and then creating a `Ventilation` instance from its
    // specific `ventilation` section.
    //
    // # Returns
    // A freshly initialized `Ventilation` struct, ready for testing.
    fn prepare_ventilation_tests(
        saw_tooth_profile_config_opt: Option<SawToothProfileConfig>,
    ) -> (ConfigData, Ventilation) {
        // prepare struct Ventilation for subsequent tests
        let mut config_for_ventilation: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        if let Some(saw_tooth_profile_config) = saw_tooth_profile_config_opt {
            config.ventilation.saw_tooth_profile_config = saw_tooth_profile_config.clone();
            config_for_ventilation.ventilation.saw_tooth_profile_config = saw_tooth_profile_config;
        }
        (config, Ventilation::new(config_for_ventilation.ventilation))
    }

    // Creates a thread that simulates a test environment, managing test duration and sending shutdown signals.
    //
    // This private helper function is used in integration tests to control the lifecycle of the
    // main test object (e.g., `Ventilation` module). It sleeps for a specified duration,
    // simulating the active test run, and then sends a `Quit` command followed by a `Terminate`
    // command to the test object, prompting its graceful shutdown.
    // If the mutex is provided as optional parameter, then the function will intermittently block
    // the mutex.
    //
    // # Arguments
    // * `duration_seconds` - The duration (in seconds) that the test environment should "run" before initiating shutdown.
    // * `tx_signal_handler_to_ventilation` - The sender channel used by this mock environment to send commands to the `Ventilation` test object, simulating the signal handler.
    // * `rx_signal_handler_from_ventilation` - The receiver channel for this mock environment to receive acknowledgments back from the `Ventilation` test object.
    // * `mutex_ventilation_status_opt` - Mutex for ventilation actuator status wrapped in Option
    //
    // # Returns
    // A `thread::JoinHandle<()>` which can be used by the main test thread to wait for this mock environment thread's completion.
    //
    // # Panics
    // This function will panic if:
    // - The new thread cannot be spawned.
    // - It fails to send the `Quit` or `Terminate` command.
    // - It fails to receive the acknowledgment from the `Ventilation` test object.
    fn create_test_environment(
        duration_seconds: u64,
        mut signal_handler_channels: SignalHandlerChannels,
        mutex_ventilation_status_opt: Option<Arc<Mutex<ThermalControlStatus>>>,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                if mutex_ventilation_status_opt.is_some() {
                    // caller wants the test environment to intermittently block the mutex
                    let mutex_ventilation_status = mutex_ventilation_status_opt.unwrap();
                    let sleep_duration_one_second = Duration::from_secs(1);
                    let spin_sleeper = SpinSleeper::default();
                    for i in 0..10 {
                        if i % 2 == 0 {
                            spin_sleeper.sleep(sleep_duration_one_second);
                        } else {
                            // block mutex on purpose
                            {
                                match mutex_ventilation_status.lock() {
                                    Ok(_) => {
                                        spin_sleeper.sleep(sleep_duration_one_second);
                                    }
                                    Err(_) => {
                                        // Do nothing
                                    }
                                }
                            }
                        }
                    }
                } else {
                    let sleep_duration_ten_seconds = Duration::from_secs(duration_seconds);
                    let spin_sleeper = SpinSleeper::default();
                    spin_sleeper.sleep(sleep_duration_ten_seconds);
                }
                let _ = signal_handler_channels
                    .tx_signal_handler_to_ventilation
                    .send(InternalCommand::Quit);
                signal_handler_channels
                    .rx_signal_handler_from_ventilation
                    .recv()
                    .unwrap();
                let _ = signal_handler_channels
                    .tx_signal_handler_to_ventilation
                    .send(InternalCommand::Terminate);
            })
            .unwrap()
    }

    // Check if the ventilation is switched on when the temperature is high.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_ventilation_with_measured_temperature_high_without_blocking_mutex() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut ventilation) = prepare_ventilation_tests(None);

        // replacement value is used for initializing mutex
        config.sensor_manager.replacement_value_water_temperature = 30.0;

        let mut channels = Channels::new_for_test();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                    &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex needed for water temperature
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = create_mock_relay_manager_for_ventilation_on(
            channels.relay_manager.tx_relay_manager_to_ventilation,
            channels.relay_manager.rx_relay_manager_from_ventilation,
        );

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            10,
            channels.signal_handler,
            None, // no blocking of mutex in this test case
        );

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                    .ventilation
                    .tx_ventilation_to_schedule_check
                    .clone();
                let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                    channels.ventilation.tx_ventilation_to_relay_manager.clone();

                let mut ventilation_set_value_updater =
                    MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

                let ventilation_mutexes = VentilationMutexes {
                    mutex_sensor_manager_signals,
                    mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    &mut channels.ventilation,
                    &mut ventilation_set_value_updater,
                    ventilation_mutexes,
                );

                assert_eq!(ventilation.mutex_access_duration_exceeded, false);

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_relay_manager_for_test_case_finish
                    .send(InternalCommand::Quit);
                println!("* [Ventilation] checking reaction to high temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
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

    // Creates a mock Relay Manager thread for `test_ventilation_with_measured_temperature_low`.
    //
    // This private helper function spawns a new thread that acts as a mock for the `RelayManager`.
    // It's specifically configured for the `test_ventilation_with_measured_temperature_low` test case,
    // expecting a precise sequence of actuation commands from the `Ventilation` test object:
    // 1.  An `InternalCommand::SwitchOff(AquariumDevice::Ventilation)` command.
    // 2.  An `InternalCommand::SwitchOn(AquariumDevice::Ventilation)` command.
    //
    // The mock then asserts that these commands are received in the correct order and type.
    //
    // # Arguments
    // * `tx_relay_manager_to_ventilation` - The sender channel for this mock to send acknowledgments back to the `Ventilation` test object.
    // * `rx_relay_manager_from_ventilation` - The receiver channel for this mock to get actuation commands from the `Ventilation` test object.
    //
    // # Returns
    // A `thread::JoinHandle<()>` which can be used by the main test thread to wait for this mock thread's completion.
    //
    // # Panics
    // This function will panic if:
    // - The new thread cannot be spawned.
    // - The expected commands are not received in the correct order or type.
    // - Any other unexpected command is received.
    fn create_mock_relay_manager(
        mut tx_relay_manager_to_ventilation: AquaSender<bool>,
        mut rx_relay_manager_from_ventilation: AquaReceiver<InternalCommand>,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut tx_relay_manager_to_ventilation,
                    &mut rx_relay_manager_from_ventilation,
                );
                assert_eq!(actuation_events.len(), 2);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Ventilation)
                );
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Ventilation)
                );
                mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::On);
            })
            .unwrap()
    }

    // Check if the ventilation is switched off with low temperature and then switched on during application termination (if configured).
    // This test also implicitly verifies that the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_ventilation_with_measured_temperature_low() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut ventilation) = prepare_ventilation_tests(None);

        // replacement value is used for initializing mutex
        config.sensor_manager.replacement_value_water_temperature = 20.0;

        let mut channels = Channels::new_for_test();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                    &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex needed for water temperature
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = create_mock_relay_manager(
            channels.relay_manager.tx_relay_manager_to_ventilation,
            channels.relay_manager.rx_relay_manager_from_ventilation,
        );

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            58,
            channels.signal_handler,
            None, // no blocking of mutex in this test case
        );

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                    .ventilation
                    .tx_ventilation_to_schedule_check
                    .clone();
                let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                    channels.ventilation.tx_ventilation_to_relay_manager.clone();

                let mut ventilation_set_value_updater =
                    MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

                let ventilation_mutexes = VentilationMutexes {
                    mutex_sensor_manager_signals,
                    mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    &mut channels.ventilation,
                    &mut ventilation_set_value_updater,
                    ventilation_mutexes,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_relay_manager_for_test_case_finish
                    .send(InternalCommand::Quit);
                println!("* [Ventilation] checking reaction to low temperature.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
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

    // Check if the ventilation is switched on and off during two cycles of the saw tooth profile.
    // Duration of ventilation switched on shall equal the duration of ventilation switched off (1% tolerance).
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_ventilation_with_measured_temperature_50_percent() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut ventilation) =
            prepare_ventilation_tests(Some(SawToothProfileConfig::new(6, 1, 6, 1)));

        println!(
            "Temperature control range is {} deg C to {} deg C.",
            config.ventilation.switch_off_temperature, config.ventilation.switch_on_temperature
        );
        // replacement value is used for initializing mutex
        config.sensor_manager.replacement_value_water_temperature = 25.0;
        let mut channels = Channels::new_for_test();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                    &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex needed for water temperature
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_ventilation,
                    &mut channels.relay_manager.rx_relay_manager_from_ventilation,
                );
                println!(
                    "Mock relay manager received {} commands:",
                    actuation_events.len()
                );
                for actuation_event in actuation_events.clone() {
                    println!("{}", actuation_event);
                }
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Ventilation)
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
                    .as_millis();

                println!(
                    "second_last_actuation_duration={}",
                    second_last_actuation_duration
                );

                min_actuation_duration = min_actuation_duration.min(second_last_actuation_duration);
                max_actuation_duration = max_actuation_duration.max(second_last_actuation_duration);

                let third_last_actuation_duration = third_last_actuation_event
                    .time
                    .duration_since(fourth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "third_last_actuation_duration={}",
                    third_last_actuation_duration
                );

                min_actuation_duration = min_actuation_duration.min(third_last_actuation_duration);
                max_actuation_duration = max_actuation_duration.max(third_last_actuation_duration);

                let fourth_last_actuation_duration = fourth_last_actuation_event
                    .time
                    .duration_since(fifth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "fourth_last_actuation_duration={}",
                    fourth_last_actuation_duration
                );

                min_actuation_duration = min_actuation_duration.min(fourth_last_actuation_duration);
                max_actuation_duration = max_actuation_duration.max(fourth_last_actuation_duration);

                let fifth_last_actuation_duration = fifth_last_actuation_event
                    .time
                    .duration_since(sixth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "fifth_last_actuation_duration={}",
                    fifth_last_actuation_duration
                );

                min_actuation_duration = min_actuation_duration.min(fifth_last_actuation_duration);
                max_actuation_duration = max_actuation_duration.max(fifth_last_actuation_duration);

                let sixth_last_actuation_duration = sixth_last_actuation_event
                    .time
                    .duration_since(seventh_last_actuation_event.time)
                    .as_millis();

                println!(
                    "sixth_last_actuation_duration={}",
                    sixth_last_actuation_duration
                );

                min_actuation_duration = min_actuation_duration.min(sixth_last_actuation_duration);
                max_actuation_duration = max_actuation_duration.max(sixth_last_actuation_duration);

                let delta_min_max_duration = max_actuation_duration - min_actuation_duration;

                assert_le!(second_last_actuation_duration, 6600);
                assert_ge!(second_last_actuation_duration, 6400);
                assert_le!(third_last_actuation_duration, 6600);
                assert_ge!(third_last_actuation_duration, 6400);
                assert_le!(fourth_last_actuation_duration, 6600);
                assert_ge!(fourth_last_actuation_duration, 6400);
                assert_le!(fifth_last_actuation_duration, 6600);
                assert_ge!(fifth_last_actuation_duration, 6400);
                assert_le!(sixth_last_actuation_duration, 6600);
                assert_ge!(sixth_last_actuation_duration, 6400);

                println!("delta_min_max_duration={}", delta_min_max_duration);
                assert_le!(delta_min_max_duration, 50);
                mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::On);
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            58,
            channels.signal_handler,
            None, // no blocking of mutex in this test case
        );

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                    .ventilation
                    .tx_ventilation_to_schedule_check
                    .clone();
                let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                    channels.ventilation.tx_ventilation_to_relay_manager.clone();

                let mut ventilation_set_value_updater =
                    MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

                let ventilation_mutexes = VentilationMutexes {
                    mutex_sensor_manager_signals,
                    mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    &mut channels.ventilation,
                    &mut ventilation_set_value_updater,
                    ventilation_mutexes,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_relay_manager_for_test_case_finish
                    .send(InternalCommand::Quit);
                println!("* [Ventilation] checking reaction to medium temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
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

    // Check if the ventilation is switched on for most of the time if the temperature is high within the interval.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_ventilation_with_measured_temperature_high_within_interval() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut ventilation) =
            prepare_ventilation_tests(Some(SawToothProfileConfig::new(6, 1, 6, 1)));

        // replacement value is used for initializing mutex
        config.sensor_manager.replacement_value_water_temperature = 27.5;
        let mut channels = Channels::new_for_test();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                    &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex needed for water temperature
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_ventilation,
                    &mut channels.relay_manager.rx_relay_manager_from_ventilation,
                );
                println!(
                    "Mock relay manager received {} commands:",
                    actuation_events.len()
                );
                for actuation_event in actuation_events.clone() {
                    println!("{}", actuation_event);
                }
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Ventilation)
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

                let second_last_actuation_duration = second_last_actuation_event
                    .time
                    .duration_since(third_last_actuation_event.time)
                    .as_millis();

                println!(
                    "second_last_actuation_duration={}, Command={}",
                    second_last_actuation_duration, second_last_actuation_event.command
                );

                let third_last_actuation_duration = third_last_actuation_event
                    .time
                    .duration_since(fourth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "third_last_actuation_duration={}, Command={}",
                    third_last_actuation_duration, third_last_actuation_event.command
                );

                let fourth_last_actuation_duration = fourth_last_actuation_event
                    .time
                    .duration_since(fifth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "fourth_last_actuation_duration={}, Command={}",
                    fourth_last_actuation_duration, fourth_last_actuation_event.command
                );

                let fifth_last_actuation_duration = fifth_last_actuation_event
                    .time
                    .duration_since(sixth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "fifth_last_actuation_duration={}, Command={}",
                    fifth_last_actuation_duration, fifth_last_actuation_event.command
                );

                let sixth_last_actuation_duration = sixth_last_actuation_event
                    .time
                    .duration_since(seventh_last_actuation_event.time)
                    .as_millis();

                println!(
                    "sixth_last_actuation_duration={}, Command={}",
                    sixth_last_actuation_duration, sixth_last_actuation_event.command
                );

                assert_le!(second_last_actuation_duration, 1100);
                assert_ge!(second_last_actuation_duration, 900);
                assert_le!(third_last_actuation_duration, 12150);
                assert_ge!(third_last_actuation_duration, 11950);
                assert_le!(fourth_last_actuation_duration, 1100);
                assert_ge!(fourth_last_actuation_duration, 900);
                assert_le!(fifth_last_actuation_duration, 12160);
                assert_ge!(fifth_last_actuation_duration, 11950);
                assert_le!(sixth_last_actuation_duration, 1100);
                assert_ge!(sixth_last_actuation_duration, 900);

                mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::On);
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            58,
            channels.signal_handler,
            None, // no blocking of mutex in this test case
        );

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                    .ventilation
                    .tx_ventilation_to_schedule_check
                    .clone();
                let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                    channels.ventilation.tx_ventilation_to_relay_manager.clone();

                let mut ventilation_set_value_updater =
                    MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

                let ventilation_mutexes = VentilationMutexes {
                    mutex_sensor_manager_signals,
                    mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    &mut channels.ventilation,
                    &mut ventilation_set_value_updater,
                    ventilation_mutexes,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_relay_manager_for_test_case_finish
                    .send(InternalCommand::Quit);
                println!("* [Ventilation] checking reaction to medium temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
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

    // Check if the ventilation is switched off for most of the time if the temperature is low within the interval.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_ventilation_with_measured_temperature_low_within_interval() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut ventilation) =
            prepare_ventilation_tests(Some(SawToothProfileConfig::new(6, 1, 6, 1)));

        // replacement value is used for initializing mutex
        config.sensor_manager.replacement_value_water_temperature = 22.5;
        let mut channels = Channels::new_for_test();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                    &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex needed for water temperature
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_ventilation,
                    &mut channels.relay_manager.rx_relay_manager_from_ventilation,
                );
                println!(
                    "Mock relay manager received {} commands:",
                    actuation_events.len()
                );
                for actuation_event in actuation_events.clone() {
                    println!("{}", actuation_event);
                }
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Ventilation)
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

                let second_last_actuation_duration = second_last_actuation_event
                    .time
                    .duration_since(third_last_actuation_event.time)
                    .as_millis();

                println!(
                    "second_last_actuation_duration={}, Command={}",
                    second_last_actuation_duration, second_last_actuation_event.command
                );

                let third_last_actuation_duration = third_last_actuation_event
                    .time
                    .duration_since(fourth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "third_last_actuation_duration={}, Command={}",
                    third_last_actuation_duration, third_last_actuation_event.command
                );

                let fourth_last_actuation_duration = fourth_last_actuation_event
                    .time
                    .duration_since(fifth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "fourth_last_actuation_duration={}, Command={}",
                    fourth_last_actuation_duration, fourth_last_actuation_event.command
                );

                let fifth_last_actuation_duration = fifth_last_actuation_event
                    .time
                    .duration_since(sixth_last_actuation_event.time)
                    .as_millis();

                println!(
                    "fifth_last_actuation_duration={}, Command={}",
                    fifth_last_actuation_duration, fifth_last_actuation_event.command
                );

                let sixth_last_actuation_duration = sixth_last_actuation_event
                    .time
                    .duration_since(seventh_last_actuation_event.time)
                    .as_millis();

                println!(
                    "sixth_last_actuation_duration={}, Command={}",
                    sixth_last_actuation_duration, sixth_last_actuation_event.command
                );

                /*
                assert_le!(second_last_actuation_duration, 1100);
                assert_ge!(second_last_actuation_duration, 900);
                assert_le!(third_last_actuation_duration, 12150);
                assert_ge!(third_last_actuation_duration, 11950);
                assert_le!(fourth_last_actuation_duration, 1100);
                assert_ge!(fourth_last_actuation_duration, 900);
                assert_le!(fifth_last_actuation_duration, 12150);
                assert_ge!(fifth_last_actuation_duration, 11950);
                assert_le!(sixth_last_actuation_duration, 1100);
                assert_ge!(sixth_last_actuation_duration, 900);
                */
                mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::On);
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            58,
            channels.signal_handler,
            None, // no blocking of mutex in this test case
        );

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                    .ventilation
                    .tx_ventilation_to_schedule_check
                    .clone();
                let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                    channels.ventilation.tx_ventilation_to_relay_manager.clone();

                let mut ventilation_set_value_updater =
                    MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

                let ventilation_mutexes = VentilationMutexes {
                    mutex_sensor_manager_signals,
                    mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    &mut channels.ventilation,
                    &mut ventilation_set_value_updater,
                    ventilation_mutexes,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_relay_manager_for_test_case_finish
                    .send(InternalCommand::Quit);
                println!("* [Ventilation] checking reaction to medium temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
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

    // Check if ventilation control is only actuating when quitting while schedule check provides a negative result.
    #[test]
    pub fn test_ventilation_block_by_schedule() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut ventilation) = prepare_ventilation_tests(None);

        // replacement value is used for initializing mutex
        config.sensor_manager.replacement_value_water_temperature = 30.0;

        let mut channels = Channels::new_for_test();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                    &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                    None,
                    false,
                );
            })
            .unwrap();

        // Mutex needed for water temperature
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = create_mock_relay_manager(
            channels.relay_manager.tx_relay_manager_to_ventilation,
            channels.relay_manager.rx_relay_manager_from_ventilation,
        );

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            10,
            channels.signal_handler,
            None, // no blocking of mutex in this test case
        );

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                    .ventilation
                    .tx_ventilation_to_schedule_check
                    .clone();
                let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                    channels.ventilation.tx_ventilation_to_relay_manager.clone();

                let mut ventilation_set_value_updater =
                    MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

                let ventilation_mutexes = VentilationMutexes {
                    mutex_sensor_manager_signals,
                    mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    &mut channels.ventilation,
                    &mut ventilation_set_value_updater,
                    ventilation_mutexes,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_relay_manager_for_test_case_finish
                    .send(InternalCommand::Quit);

                let actuation_count = *mutex_device_scheduler_ventilation.lock().unwrap();

                // check if ventilation is only actuated once (when quitting)
                assert_eq!(actuation_count, 2);

                println!("* [Ventilation] checking if schedule checker can block actuation.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
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

    // Test case runs ventilation control and triggers inhibition by sending the stop message via the channel.
    // After verification that ventilation is stopped,
    // The test case sends the start message via the channel and verifies if ventilation control
    // resumes operation.
    #[test]
    pub fn test_messaging_stops_starts_ventilation() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

        // replacement value is used for initialization of mutex
        config.sensor_manager.replacement_value_water_temperature = 30.0;

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));

        let mut ventilation = Ventilation::new(config.ventilation);

        let mut channels = Channels::new_for_test();

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));
        let mutex_device_scheduler_test_environment = mutex_device_scheduler_ventilation.clone();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                    &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                    None,
                    true,
                );
            })
            .unwrap();

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_ventilation,
                    &mut channels.relay_manager.rx_relay_manager_from_ventilation,
                );
                println!("actuation_events:");
                for actuation_event in &actuation_events {
                    println!("{}", actuation_event);
                }
                assert_eq!(actuation_events.len(), 5);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Ventilation)
                );
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Ventilation)
                );
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Ventilation)
                );
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOff(AquariumDevice::Ventilation)
                );
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Ventilation)
                );
                mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::On);
            })
            .unwrap();

        // thread for test environment
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_100_millis = Duration::from_millis(100);
                let spin_sleeper = SpinSleeper::default();

                // initial wait so that ventilation control can switch on
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millis);
                }
                // check the initial state of mutex: should be already switched on
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 1);

                // sending a message requesting stop of ventilation control
                match channels
                    .messaging
                    .tx_messaging_to_ventilation
                    .send(InternalCommand::Stop)
                {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending stop command to test object ({e:?})",
                            module_path!()
                        );
                    }
                }

                // wait for 1 second: Ventilation control will switch off
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millis);
                }
                // check if ventilation is actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 2);

                // sending a message requesting restart of ventilation control
                match channels
                    .messaging
                    .tx_messaging_to_ventilation
                    .send(InternalCommand::Start)
                {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending start command to test object ({e:?})",
                            module_path!()
                        );
                    }
                }

                // Wait for 1 second. Ventilation control will switch on
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millis);
                }
                // check if ventilation is actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 3);

                // sending a message requesting stop of ventilation control
                match channels
                    .messaging
                    .tx_messaging_to_ventilation
                    .send(InternalCommand::Stop)
                {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending stop command to test object ({e:?})",
                            module_path!()
                        );
                    }
                }

                // wait for 1 second: Ventilation control will switch off
                for _ in 0..10 {
                    spin_sleeper.sleep(sleep_duration_100_millis);
                }
                // check if ventilation is actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 4);

                // requesting ventilation control to quit
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_ventilation
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_ventilation
                    .recv()
                    .unwrap();

                // check if ventilation is actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 5);
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_ventilation
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                    .ventilation
                    .tx_ventilation_to_schedule_check
                    .clone();
                let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                    channels.ventilation.tx_ventilation_to_relay_manager.clone();

                let mut ventilation_set_value_updater =
                    MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

                let ventilation_mutexes = VentilationMutexes {
                    mutex_sensor_manager_signals,
                    mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    &mut channels.ventilation,
                    &mut ventilation_set_value_updater,
                    ventilation_mutexes,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_relay_manager_for_test_case_finish
                    .send(InternalCommand::Quit);

                println!("* [Ventilation] checking if messaging can block and restart actuation.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
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

    // Check if the ventilation is switched on when the temperature is high.
    // After a limited time period, increase the set values and observe if the heater is switched off.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Test case uses test database #52.
    #[test]
    pub fn test_ventilation_with_decreased_set_values() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut ventilation) = prepare_ventilation_tests(None);

        // replacement value is used for initializing mutex
        config.sensor_manager.replacement_value_water_temperature = 30.0;

        let mut channels = Channels::new_for_test();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                    &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex needed for water temperature
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_ventilation,
                    &mut channels.relay_manager.rx_relay_manager_from_ventilation,
                );
                assert_eq!(actuation_events.len(), 3);
                let actuation_event_3 = actuation_events.pop().unwrap();
                let actuation_event_2 = actuation_events.pop().unwrap();
                let actuation_event_1 = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event_1.command,
                    InternalCommand::SwitchOn(AquariumDevice::Ventilation)
                );
                assert_eq!(
                    actuation_event_2.command,
                    InternalCommand::SwitchOff(AquariumDevice::Ventilation)
                );
                assert_eq!(
                    actuation_event_3.command,
                    InternalCommand::SwitchOn(AquariumDevice::Ventilation)
                );
                mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::On);
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            12,
            channels.signal_handler,
            None, // no blocking of mutex in this test case
        );

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                    .ventilation
                    .tx_ventilation_to_schedule_check
                    .clone();
                let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                    channels.ventilation.tx_ventilation_to_relay_manager.clone();

                // set up the set value updater to actively manipulate the set values
                let mut ventilation_set_value_updater = MockSqlInterfaceVentilationSetVals::new(
                    true,
                    Some(30.0),
                    Some(40.0),
                    Some(Duration::from_secs(10)),
                );

                let ventilation_mutexes = VentilationMutexes {
                    mutex_sensor_manager_signals,
                    mutex_ventilation_status: Arc::new(Mutex::new(ThermalControlStatus::default())),
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    &mut channels.ventilation,
                    &mut ventilation_set_value_updater,
                    ventilation_mutexes,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_relay_manager_for_test_case_finish
                    .send(InternalCommand::Quit);
                println!("* [Ventilation] checking reaction to high temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
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

    // helper function to avoid warning about duplicated code.
    fn create_mock_relay_manager_for_ventilation_on(
        mut tx_relay_manager_to_ventilation: AquaSender<bool>,
        mut rx_relay_manager_from_ventilation: AquaReceiver<InternalCommand>,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (mut actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut tx_relay_manager_to_ventilation,
                    &mut rx_relay_manager_from_ventilation,
                );
                assert_eq!(actuation_events.len(), 1);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Ventilation)
                );
                mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::On);
            })
            .unwrap()
    }

    // Check if the ventilation is switched on when the temperature is high.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_ventilation_with_measured_temperature_high_with_blocking_mutex() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut ventilation) = prepare_ventilation_tests(None);

        // replacement value is used for initializing mutex
        config.sensor_manager.replacement_value_water_temperature = 30.0;

        let mut channels = Channels::new_for_test();

        let mutex_ventilation_status = Arc::new(Mutex::new(ThermalControlStatus::default()));
        let mutex_ventilation_status_clone_for_test_environment = mutex_ventilation_status.clone();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                    &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex needed for water temperature
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = create_mock_relay_manager_for_ventilation_on(
            channels.relay_manager.tx_relay_manager_to_ventilation,
            channels.relay_manager.rx_relay_manager_from_ventilation,
        );

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            10,
            channels.signal_handler,
            Some(mutex_ventilation_status_clone_for_test_environment), // blocking of mutex in this test case
        );

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                    .ventilation
                    .tx_ventilation_to_schedule_check
                    .clone();
                let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                    channels.ventilation.tx_ventilation_to_relay_manager.clone();

                let mut ventilation_set_value_updater =
                    MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

                let ventilation_mutexes = VentilationMutexes {
                    mutex_sensor_manager_signals,
                    mutex_ventilation_status,
                    mutex_temperature_gradient: Arc::new(Mutex::new(0.0)),
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    &mut channels.ventilation,
                    &mut ventilation_set_value_updater,
                    ventilation_mutexes,
                );

                assert_eq!(ventilation.mutex_access_duration_exceeded, true);

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_relay_manager_for_test_case_finish
                    .send(InternalCommand::Quit);
                println!("* [Ventilation] checking reaction to high temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
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
    pub fn test_ventilation_gradient_monitoring_disables_actuator() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut ventilation) = prepare_ventilation_tests(None);

        // Configure gradient monitoring to be very aggressive
        config.ventilation.max_time_actuator_effect_monitor = 2;
        config.ventilation.switch_off_when_no_effect = true;
        config.ventilation.switch_on_when_terminating = false;
        ventilation.config = config.ventilation;

        // replacement value is used for initializing mutex - needs to start with a high value
        config.sensor_manager.replacement_value_water_temperature = 30.0;

        let mut channels = Channels::new_for_test();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                    &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex needed for water temperature
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

        // Mutex for ventilation status to check assertions
        let mutex_ventilation_status = Arc::new(Mutex::new(ThermalControlStatus::default()));
        let mutex_ventilation_status_clone = mutex_ventilation_status.clone();

        // Mutex for temperature gradient
        let mutex_temperature_gradient = Arc::new(Mutex::new(-0.1)); // Start with cooling gradient
        let mutex_temperature_gradient_clone = mutex_temperature_gradient.clone();

        // thread for mock relay manager
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_ventilation,
                    &mut channels.relay_manager.rx_relay_manager_from_ventilation,
                );
                // Expect On, Off
                assert_eq!(actuation_events.len(), 2);
                assert_eq!(
                    actuation_events[0].command,
                    InternalCommand::SwitchOn(AquariumDevice::Ventilation)
                );
                assert_eq!(
                    actuation_events[1].command,
                    InternalCommand::SwitchOff(AquariumDevice::Ventilation)
                );

                mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::Off);
            })
            .unwrap();

        // thread for controlling test phases
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_phase = Duration::from_secs(5);
                let spin_sleeper = SpinSleeper::default();

                // Phase 1: High temp, good gradient. Wait 5s.
                spin_sleeper.sleep(sleep_duration_phase);
                {
                    let status = mutex_ventilation_status_clone.lock().unwrap();
                    assert!(
                        status.actuator_switched_on,
                        "Phase 1: Ventilation should be ON"
                    );
                }

                // Phase 2: Constant temp (gradient indicates that actuator is ineffective).
                {
                    let mut gradient = mutex_temperature_gradient_clone.lock().unwrap();
                    *gradient = 0.0;
                }
                // Wait 5s (monitor timeout is 2s)
                spin_sleeper.sleep(sleep_duration_phase);
                {
                    let status = mutex_ventilation_status_clone.lock().unwrap();
                    assert!(
                        !status.actuator_switched_on,
                        "Phase 2: Ventilation should be OFF due to gradient monitor"
                    );
                }

                // Terminate
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_ventilation
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_ventilation
                    .recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_ventilation
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                    .ventilation
                    .tx_ventilation_to_schedule_check
                    .clone();
                let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                    channels.ventilation.tx_ventilation_to_relay_manager.clone();

                let mut ventilation_set_value_updater =
                    MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

                let ventilation_mutexes = VentilationMutexes {
                    mutex_sensor_manager_signals,
                    mutex_ventilation_status,
                    mutex_temperature_gradient,
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    &mut channels.ventilation,
                    &mut ventilation_set_value_updater,
                    ventilation_mutexes,
                );

                // send Quit signal to mock threads
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_relay_manager_for_test_case_finish
                    .send(InternalCommand::Quit);
                println!("* [Ventilation] checking gradient monitoring succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check failed");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager failed");
        join_handle_test_environment
            .join()
            .expect("Test environment failed");
        join_handle_test_object.join().expect("Test object failed");
    }

    #[test]
    pub fn test_ventilation_reset_errors_enables_actuator() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let (mut config, mut ventilation) = prepare_ventilation_tests(None);

        // Start in disabled state
        ventilation.actuator_disabled = true;

        // replacement value is used for initializing mutex - needs to start with a high value
        config.sensor_manager.replacement_value_water_temperature = 30.0;
        config.ventilation.switch_off_when_no_effect = true;
        config.ventilation.switch_on_when_terminating = false;
        ventilation.config = config.ventilation;

        let mut channels = Channels::new_for_test();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &mut channels.schedule_check.tx_schedule_check_to_ventilation,
                    &mut channels.schedule_check.rx_schedule_check_from_ventilation,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex needed for water temperature
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

        // Mutex for ventilation status to check assertions
        let mutex_ventilation_status = Arc::new(Mutex::new(ThermalControlStatus::default()));
        let mutex_ventilation_status_clone = mutex_ventilation_status.clone();

        // Mutex for temperature gradient
        let mutex_temperature_gradient = Arc::new(Mutex::new(0.1)); // Start with positive gradient
        let mutex_temperature_gradient_clone = mutex_temperature_gradient.clone();

        // thread for mock relay manager
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let (actuation_events, mock_actuator_states) = mock_relay_manager(
                    &mut channels.relay_manager.tx_relay_manager_to_ventilation,
                    &mut channels.relay_manager.rx_relay_manager_from_ventilation,
                );
                // Expect SwitchOn (after reset), SwitchOff (at termination)
                assert_eq!(actuation_events.len(), 2);
                assert_eq!(
                    actuation_events[0].command,
                    InternalCommand::SwitchOn(AquariumDevice::Ventilation)
                );
                assert_eq!(
                    actuation_events[1].command,
                    InternalCommand::SwitchOff(AquariumDevice::Ventilation)
                );

                mock_actuator_states.check_terminal_condition_ventilation(ActuatorState::Off);
            })
            .unwrap();

        // thread for controlling test phases
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_phase = Duration::from_secs(5);
                let spin_sleeper = SpinSleeper::default();

                // Phase 1: Disabled state. Wait 5s.
                spin_sleeper.sleep(sleep_duration_phase);
                {
                    let status = mutex_ventilation_status_clone.lock().unwrap();
                    assert!(
                        !status.actuator_switched_on,
                        "Phase 1: Ventilation should be OFF (disabled)"
                    );
                }

                // Send ResetAllErrors command
                channels
                    .messaging
                    .tx_messaging_to_ventilation
                    .send(InternalCommand::ResetAllErrors)
                    .unwrap();

                // Phase 2: Enabled state. Wait 5s.
                spin_sleeper.sleep(sleep_duration_phase);
                {
                    let status = mutex_ventilation_status_clone.lock().unwrap();
                    assert!(
                        status.actuator_switched_on,
                        "Phase 2: Ventilation should be ON (enabled)"
                    );
                }

                // Terminate
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_ventilation
                    .send(InternalCommand::Quit);
                channels
                    .signal_handler
                    .rx_signal_handler_from_ventilation
                    .recv()
                    .unwrap();
                let _ = channels
                    .signal_handler
                    .tx_signal_handler_to_ventilation
                    .send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let mut tx_ventilation_to_schedule_check_for_test_case_finish = channels
                    .ventilation
                    .tx_ventilation_to_schedule_check
                    .clone();
                let mut tx_ventilation_to_relay_manager_for_test_case_finish =
                    channels.ventilation.tx_ventilation_to_relay_manager.clone();

                let mut ventilation_set_value_updater =
                    MockSqlInterfaceVentilationSetVals::new(false, None, None, None);

                let ventilation_mutexes = VentilationMutexes {
                    mutex_sensor_manager_signals,
                    mutex_ventilation_status,
                    mutex_temperature_gradient: mutex_temperature_gradient_clone,
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    &mut channels.ventilation,
                    &mut ventilation_set_value_updater,
                    ventilation_mutexes,
                );

                // Add a small delay to allow mock_relay_manager to process the final command
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(Duration::from_millis(100));

                // send Quit signal to mock threads
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_relay_manager_for_test_case_finish
                    .send(InternalCommand::Quit);
                println!("* [Ventilation] checking reset errors enables actuator succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check failed");
        join_handle_mock_relay_manager
            .join()
            .expect("Mock relay manager failed");
        join_handle_test_environment
            .join()
            .expect("Test environment failed");
        join_handle_test_object.join().expect("Test object failed");
    }
}
