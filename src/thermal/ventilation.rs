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

use crate::utilities::channel_content::{
    ActuatorState, AquariumDevice, AquariumSignal, InternalCommand,
};
use crate::utilities::common::check_if_mutex_is_blocked;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::sawtooth_profile::{SawToothProfile, SawToothProfileConfig};

const LOG_TARGET_VENTILATION: &str = "ventilation";

const CYCLE_TIME_VENTILATION_MILLIS: u64 = 500;

const TIME_INCREMENT_VENTILATION_SECS: f32 = CYCLE_TIME_VENTILATION_MILLIS as f32 / 1000.0;

/// Holds the configuration data for the ventilation control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct VentilationConfig {
    active: bool,
    /// The temperature at which the ventilation is switched on for 100% of the time
    /// This temperature must be higher than switch_off_temperature
    switch_on_temperature: f32,

    /// The temperature at which the ventilation is switched off for 100% of the time
    /// this temperature must be lower than switch_on_temperature
    switch_off_temperature: f32,

    /// A flag to determine the behavior of the control when the application is terminating
    /// If this flag is true, the application will switch on the ventilation when exiting.    
    switch_on_when_terminating: bool,

    /// The configuration of saw tooth profile for generation of low-frequency PWM signal
    saw_tooth_profile_config: SawToothProfileConfig,

    /// The strategy when schedule check does not allow operation:
    /// true=ventilation will be switched on; false=ventilation will be switched off
    switch_on_when_out_of_schedule: bool,

    /// The strategy when an external request for stopping ventilation control has been received:
    /// true=ventilation will be switched on; false=ventilation will be switched off
    switch_on_when_external_stop: bool,
}

/// Commands the relay manager to switch on the ventilation device.
///
/// This private helper function sends a `SwitchOn` command for `AquariumDevice::Ventilation`
/// to the relay manager thread. It then waits for an acknowledgment from the relay manager,
/// incrementing a shared mutex counter (`mutex_device_scheduler_ventilation`) to track actuation.
/// It also updates a flag (`mutex_blocked_during_actuation`) to help manage warnings if the
/// device scheduler mutex was blocked.
///
/// # Arguments
/// * `mutex_device_scheduler_ventilation` - A reference to an `Arc<Mutex<i32>>` used for
///   coordinating device actuation, preventing parallel operations, and tracking actuation counts.
/// * `tx_ventilation_to_relay_manager` - The sender channel for communicating the `SwitchOn` command to the relay manager.
/// * `rx_ventilation_from_relay_manager` - The receiver channel for getting a boolean acknowledgment back from the relay manager.
/// * `mutex_blocked_during_actuation` - A mutable boolean flag that is set to `true` if the
///   `mutex_device_scheduler_ventilation` was found to be blocked during the `check_if_mutex_is_blocked` call.
fn switch_on_ventilation(
    mutex_device_scheduler_ventilation: &Arc<Mutex<i32>>,
    tx_ventilation_to_relay_manager: &Sender<InternalCommand>,
    rx_ventilation_from_relay_manager: &Receiver<bool>,
    mutex_blocked_during_actuation: &mut bool,
) {
    // reduce probability of wrong warning
    *mutex_blocked_during_actuation |=
        check_if_mutex_is_blocked(mutex_device_scheduler_ventilation);

    // avoid electrical overloads through parallel device actuation
    let mut mutex_data = mutex_device_scheduler_ventilation.lock().unwrap();
    match tx_ventilation_to_relay_manager
        .send(InternalCommand::SwitchOn(AquariumDevice::Ventilation))
    {
        Ok(_) => {
            // now listen for the response
            match rx_ventilation_from_relay_manager.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_VENTILATION,
                        "receiving answer from relay manager when switching on ventilation failed ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_VENTILATION,
                "channel communication to relay manager when switching on ventilation ({e:?})"
            );
        }
    }
    *mutex_data += 1;
}

/// Commands the relay manager to switch off the ventilation device.
///
/// This private helper function sends a `SwitchOff` command for `AquariumDevice::Ventilation`
/// to the relay manager thread. It then waits for an acknowledgment from the relay manager,
/// incrementing a shared mutex counter (`mutex_device_scheduler_ventilation`) to track actuation.
/// It also updates a flag (`mutex_blocked_during_actuation`) to help manage warnings if the
/// device scheduler mutex was blocked.
///
/// # Arguments
/// * `mutex_device_scheduler_ventilation` - A reference to an `Arc<Mutex<i32>>` used for
///   coordinating device actuation, preventing parallel operations, and tracking actuation counts.
/// * `tx_ventilation_to_relay_manager` - The sender channel for communicating the `SwitchOff` command to the relay manager.
/// * `rx_ventilation_from_relay_manager` - The receiver channel for getting a boolean acknowledgment back from the relay manager.
/// * `mutex_blocked_during_actuation` - A mutable boolean flag that is set to `true` if the
///   `mutex_device_scheduler_ventilation` was found to be blocked during the `check_if_mutex_is_blocked` call.
fn switch_off_ventilation(
    mutex_device_scheduler_ventilation: &Arc<Mutex<i32>>,
    tx_ventilation_to_relay_manager: &Sender<InternalCommand>,
    rx_ventilation_from_relay_manager: &Receiver<bool>,
    mutex_blocked_during_actuation: &mut bool,
) {
    // reduce probability of wrong warning
    *mutex_blocked_during_actuation |=
        check_if_mutex_is_blocked(mutex_device_scheduler_ventilation);

    // avoid electrical overloads through parallel device actuation
    let mut mutex_data = mutex_device_scheduler_ventilation.lock().unwrap();
    match tx_ventilation_to_relay_manager
        .send(InternalCommand::SwitchOff(AquariumDevice::Ventilation))
    {
        Ok(_) => {
            // now listen for the response
            match rx_ventilation_from_relay_manager.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_VENTILATION,
                        "receiving answer from relay manager when switching off ventilation failed ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_VENTILATION,
                "channel communication to relay manager when switching off ventilation failed ({e:?})"
            );
        }
    }
    *mutex_data += 1;
}

/// Struct collects the channels for communication between Ventilation and other components.
pub struct VentilationChannels {
    /// Sender part of the channel for communication to relay manager
    pub tx_ventilation_to_relay_manager: Sender<InternalCommand>,

    /// Receiver part of the channel for communication from the relay manager
    pub rx_ventilation_from_relay_manager: Receiver<bool>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_ventilation_to_signal_handler: Sender<bool>,

    /// Receiver part of the channel for communication from the signal handler                
    pub rx_ventilation_from_signal_handler: Receiver<InternalCommand>,

    /// Sender part of the channel for communication to the data logger
    pub tx_ventilation_to_data_logger: Sender<bool>,

    /// Receiver part of the channel for communication from the data logger
    pub rx_ventilation_from_data_logger: Receiver<InternalCommand>,

    /// Sender part of the channel for communication to the schedule checker
    pub tx_ventilation_to_schedule_check: Sender<InternalCommand>,

    /// Receiver part of the channel for communication from the schedule checker                   
    pub rx_ventilation_from_schedule_check: Receiver<bool>,

    /// Receiver part of the channel for communication from the messaging                   
    pub rx_ventilation_from_messaging_opt: Option<Receiver<InternalCommand>>,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Holds the configuration and the implementation for the ventilation control.
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     ventilation[Ventilation Control] --> atlas_scientific[Atlas Scientific]
///     atlas_scientific --> ventilation
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

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    lock_error_channel_receive_termination_signal: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via the channel
    lock_warn_inapplicable_command: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages to send request via the channel to schedule check
    lock_error_channel_send_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages to receive request via the channel from schedule check
    lock_error_channel_receive_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive command from data logger
    lock_error_channel_receive_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command from data logger via the channel
    lock_warn_inapplicable_command_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received a request for invalid signal from the data logger
    lock_error_invalid_signal_request_data_logger: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send response to data logger
    lock_error_send_to_data_logger: bool,
}

impl ProcessExternalRequestTrait for Ventilation {}

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
            lock_error_channel_receive_termination_signal: false,
            lock_warn_inapplicable_command: false,
            lock_error_channel_send_schedule_check: false,
            lock_error_channel_receive_schedule_check: false,
            lock_error_channel_receive_data_logger: false,
            lock_warn_inapplicable_command_data_logger: false,
            lock_error_invalid_signal_request_data_logger: false,
            lock_error_send_to_data_logger: false,
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
            measured_value_delta / target_value_delta
        } else {
            0.0 // default value when target values are not properly set up.
        }
    }

    /// Checks if the data logger sent a `RequestSignal` command and responds with the current ventilation status.
    ///
    /// This private helper function handles incoming requests from the `DataLogger` module.
    /// If a `RequestSignal` for `AquariumSignal::VentilationControlStatus` is received,
    /// it sends the current boolean state of the ventilation (`true` if ON, `false` if OFF)
    /// back to the data logger. Inapplicable commands are logged as warnings.
    ///
    /// # Arguments
    /// * `ventilation_state` - A reference to the current `ActuatorState` of the ventilation device.
    /// * `tx_ventilation_to_data_logger` - The sender channel for sending the boolean status to the data logger.
    /// * `rx_ventilation_from_data_logger` - The receiver channel for getting `InternalCommand` requests from the data logger.
    fn communicate_with_data_logger(
        &mut self,
        ventilation_state: &ActuatorState,
        tx_ventilation_to_data_logger: &Sender<bool>,
        rx_ventilation_from_data_logger: &Receiver<InternalCommand>,
    ) {
        match rx_ventilation_from_data_logger.try_recv() {
            Ok(c) => {
                self.lock_error_channel_receive_data_logger = false;
                match c {
                    InternalCommand::RequestSignal(d) => {
                        self.lock_warn_inapplicable_command_data_logger = false;
                        match d {
                            AquariumSignal::VentilationControlStatus => {
                                self.lock_error_invalid_signal_request_data_logger = false;
                                let response_to_data_logger =
                                    matches!(ventilation_state, ActuatorState::On);

                                match tx_ventilation_to_data_logger.send(response_to_data_logger) {
                                    Ok(_) => {
                                        self.lock_error_send_to_data_logger = false;
                                    }
                                    Err(e) => {
                                        if !self.lock_error_send_to_data_logger {
                                            error!(
                                                target: LOG_TARGET_VENTILATION,
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
                                        target: LOG_TARGET_VENTILATION,
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
                                target: LOG_TARGET_VENTILATION,
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
                                target: LOG_TARGET_VENTILATION,
                                "receiving command from data logger thread failed ({e:?})"
                            );
                            self.lock_error_channel_receive_data_logger = true;
                        }
                    }
                }
            }
        };
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
        ventilation_channels: &VentilationChannels,
        mutex_was_blocked_during_actuation: &mut bool,
    ) {
        match condition {
            true => {
                // switch on the ventilation if not already on
                if *ventilation_state != ActuatorState::On {
                    switch_on_ventilation(
                        mutex_device_scheduler_ventilation,
                        &ventilation_channels.tx_ventilation_to_relay_manager,
                        &ventilation_channels.rx_ventilation_from_relay_manager,
                        mutex_was_blocked_during_actuation,
                    );
                    *ventilation_state = ActuatorState::On;
                }
            }
            false => {
                // switch off the ventilation if not already off
                if *ventilation_state != ActuatorState::Off {
                    switch_off_ventilation(
                        mutex_device_scheduler_ventilation,
                        &ventilation_channels.tx_ventilation_to_relay_manager,
                        &ventilation_channels.rx_ventilation_from_relay_manager,
                        mutex_was_blocked_during_actuation,
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
    /// * `ventilation_channels` - A `VentilationChannels` struct containing all `mpsc`
    ///   channels for communication with other threads (e.g., relay manager, data logger, schedule checker, signal handler).
    /// * `mutex_water_temperature` - An `Arc<Mutex<f32>>` from which the latest water temperature
    ///   reading will be retrieved.
    pub fn execute(
        &mut self,
        mutex_device_scheduler_ventilation: Arc<Mutex<i32>>,
        ventilation_channels: VentilationChannels,
        mutex_water_temperature: Arc<Mutex<f32>>,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut saw_tooth_profile = SawToothProfile::new(&self.config.saw_tooth_profile_config);
        let mut measured_water_temperature = 0.0;
        let mut measurement_error_water_temperature: bool;
        let mut ventilation_state: ActuatorState = ActuatorState::Undefined;
        let mut schedule_check_result: bool = false;
        let mut ventilation_inhibited = false; // state of ventilation control determined by if the start/stop command has been received
        let mut lock_warn_cycle_time_exceeded = false;
        let mut start_time = SystemTime::now();
        let mut mutex_was_blocked_during_actuation: bool = false;

        loop {
            let (quit_command_received, start_command_received, stop_command_received) = self
                .process_external_request(
                    &ventilation_channels.rx_ventilation_from_signal_handler,
                    ventilation_channels
                        .rx_ventilation_from_messaging_opt
                        .as_ref(),
                );
            if quit_command_received {
                break;
            }
            if stop_command_received {
                #[cfg(not(test))]
                info!(
                    target: LOG_TARGET_VENTILATION,
                    "received Stop command. Inhibiting ventilation."
                );
                ventilation_inhibited = true;
            }
            if start_command_received {
                #[cfg(not(test))]
                info!(
                    target: LOG_TARGET_VENTILATION,
                    "received Start command. Restarting ventilation."
                );
                ventilation_inhibited = false;
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
                            measured_water_temperature
                        }
                    };
                }

                // schedule check to see if actuation is allowed
                match ventilation_channels
                    .tx_ventilation_to_schedule_check
                    .send(InternalCommand::ScheduleCheck)
                {
                    Ok(_) => {
                        self.lock_error_channel_send_schedule_check = false;
                        // now listen for the response
                        schedule_check_result = match ventilation_channels
                            .rx_ventilation_from_schedule_check
                            .recv()
                        {
                            Ok(c) => {
                                self.lock_error_channel_receive_schedule_check = false;
                                c
                            }
                            Err(e) => {
                                if !self.lock_error_channel_receive_schedule_check {
                                    error!(
                                        target: LOG_TARGET_VENTILATION,
                                        "reading channel content for schedule check failed ({e:?})"
                                    );
                                    self.lock_error_channel_receive_schedule_check = true;
                                }
                                true // assume we are allowed to actuate when communication is not successful
                            }
                        };
                    }
                    Err(e) => {
                        if !self.lock_error_channel_send_schedule_check {
                            error!(
                                target: LOG_TARGET_VENTILATION,
                                "channel communication to schedule check failed ({e:?})"
                            );
                            self.lock_error_channel_send_schedule_check = true;
                        }
                    }
                }

                if schedule_check_result && !ventilation_inhibited {
                    // actuation is allowed. proceed with control.
                    if (measured_water_temperature >= self.config.switch_off_temperature)
                        && (measured_water_temperature <= self.config.switch_on_temperature)
                    {
                        // temperature is within (dynamic) control window
                        if (self.calc_normalized_control_deviation(measured_water_temperature)
                            > saw_tooth_profile.level_normalized)
                            && !measurement_error_water_temperature
                        {
                            // switch on ventilation if not already on
                            if ventilation_state != ActuatorState::On {
                                switch_on_ventilation(
                                    &mutex_device_scheduler_ventilation,
                                    &ventilation_channels.tx_ventilation_to_relay_manager,
                                    &ventilation_channels.rx_ventilation_from_relay_manager,
                                    &mut mutex_was_blocked_during_actuation,
                                );
                                ventilation_state = ActuatorState::On;
                            }
                        } else {
                            // switch off ventilation if not already off
                            if ventilation_state != ActuatorState::Off {
                                switch_off_ventilation(
                                    &mutex_device_scheduler_ventilation,
                                    &ventilation_channels.tx_ventilation_to_relay_manager,
                                    &ventilation_channels.rx_ventilation_from_relay_manager,
                                    &mut mutex_was_blocked_during_actuation,
                                );
                                ventilation_state = ActuatorState::Off;
                            }
                        }
                    } else {
                        // The temperature is above or below (dynamic) control window
                        if measured_water_temperature > self.config.switch_on_temperature {
                            // switch on ventilation if not already on
                            if ventilation_state != ActuatorState::On {
                                switch_on_ventilation(
                                    &mutex_device_scheduler_ventilation,
                                    &ventilation_channels.tx_ventilation_to_relay_manager,
                                    &ventilation_channels.rx_ventilation_from_relay_manager,
                                    &mut mutex_was_blocked_during_actuation,
                                );
                                ventilation_state = ActuatorState::On;
                            }
                        }
                        if measured_water_temperature < self.config.switch_off_temperature {
                            // switch off ventilation if not already off
                            if ventilation_state != ActuatorState::Off {
                                switch_off_ventilation(
                                    &mutex_device_scheduler_ventilation,
                                    &ventilation_channels.tx_ventilation_to_relay_manager,
                                    &ventilation_channels.rx_ventilation_from_relay_manager,
                                    &mut mutex_was_blocked_during_actuation,
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
                        &ventilation_channels,
                        &mut mutex_was_blocked_during_actuation,
                    );
                } else {
                    // schedule check requires stop of ventilation control
                    Self::conditional_ventilation_switch(
                        self.config.switch_on_when_out_of_schedule,
                        &mut ventilation_state,
                        &mutex_device_scheduler_ventilation,
                        &ventilation_channels,
                        &mut mutex_was_blocked_during_actuation,
                    );
                }
            }

            self.communicate_with_data_logger(
                &ventilation_state,
                &ventilation_channels.tx_ventilation_to_data_logger,
                &ventilation_channels.rx_ventilation_from_data_logger,
            );

            saw_tooth_profile.execute_with(TIME_INCREMENT_VENTILATION_SECS);

            let stop_time = SystemTime::now();
            let execution_duration_millis =
                stop_time.duration_since(start_time).unwrap().as_millis() as u64;

            let remaining_sleep_time_millis: u64;
            if execution_duration_millis >= CYCLE_TIME_VENTILATION_MILLIS {
                if !lock_warn_cycle_time_exceeded && !mutex_was_blocked_during_actuation {
                    warn!(target: LOG_TARGET_VENTILATION,
                        "execution duration of {} milliseconds exceeds cycle time of {}",
                        execution_duration_millis,
                        CYCLE_TIME_VENTILATION_MILLIS);
                    lock_warn_cycle_time_exceeded = true;
                } else {
                    lock_warn_cycle_time_exceeded = false;
                }
                remaining_sleep_time_millis = 0;
            } else {
                remaining_sleep_time_millis =
                    CYCLE_TIME_VENTILATION_MILLIS - execution_duration_millis;
            }
            mutex_was_blocked_during_actuation = false; // reset for the next cycle

            let sleep_duration = Duration::from_millis(remaining_sleep_time_millis);
            spin_sleeper.sleep(sleep_duration);

            start_time = SystemTime::now(); // start time for the next cycle
        }

        if self.config.switch_on_when_terminating {
            // switch on ventilation if not already on
            if ventilation_state != ActuatorState::On {
                switch_on_ventilation(
                    &mutex_device_scheduler_ventilation,
                    &ventilation_channels.tx_ventilation_to_relay_manager,
                    &ventilation_channels.rx_ventilation_from_relay_manager,
                    &mut mutex_was_blocked_during_actuation,
                );
            }
        } else {
            // switch off ventilation if not already off
            if ventilation_state != ActuatorState::Off {
                switch_off_ventilation(
                    &mutex_device_scheduler_ventilation,
                    &ventilation_channels.tx_ventilation_to_relay_manager,
                    &ventilation_channels.rx_ventilation_from_relay_manager,
                    &mut mutex_was_blocked_during_actuation,
                );
            }
        }

        // The application received a request to terminate, that is why the loop was left.
        // Answer to the thread which sent the request for termination, so that shutdown can proceed further.
        match ventilation_channels
            .tx_ventilation_to_signal_handler
            .send(true)
        {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_VENTILATION,
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // This thread has channel connections to underlying threads (relay manager, Atlas Scientific).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match ventilation_channels
                .rx_ventilation_from_signal_handler
                .recv()
            {
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
                                    target: LOG_TARGET_VENTILATION,
                                    "ignoring inapplicable command ({}) from signal handler.",
                                    c
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
                            target: LOG_TARGET_VENTILATION,
                            "receiving termination signal failed ({e:?})"
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
    use all_asserts::{assert_ge, assert_le};
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use crate::thermal::ventilation::{Ventilation, VentilationChannels, LOG_TARGET_VENTILATION};
    use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
    use crate::utilities::common::tests::update_min_max_actuation_duration;
    use crate::utilities::config::{read_config_file, ConfigData};

    use crate::stubs::mock_data_logger::tests::mock_data_logger;
    use crate::stubs::mock_relay_manager::tests::mock_relay_manager;
    use crate::stubs::mock_schedule_check::tests::mock_schedule_check;

    // Prepares and returns a new `Ventilation` struct for use in test cases.
    //
    // This private helper function simplifies test setup by loading the generic
    // test configuration file and then creating a `Ventilation` instance from its
    // specific `ventilation` section.
    //
    // # Returns
    // A freshly initialized `Ventilation` struct, ready for testing.
    fn prepare_ventilation_tests() -> Ventilation {
        // prepare struct Ventilation for subsequent tests
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        Ventilation::new(config.ventilation)
    }

    // Creates a thread that simulates a test environment, managing test duration and sending shutdown signals.
    //
    // This private helper function is used in integration tests to control the lifecycle of the
    // main test object (e.g., `Ventilation` module). It sleeps for a specified duration,
    // simulating the active test run, and then sends a `Quit` command followed by a `Terminate`
    // command to the test object, prompting its graceful shutdown.
    //
    // # Arguments
    // * `duration_seconds` - The duration (in seconds) the test environment should "run" before initiating shutdown.
    // * `tx_signal_handler_to_ventilation` - The sender channel used by this mock environment to send commands to the `Ventilation` test object, simulating the signal handler.
    // * `rx_signal_handler_from_ventilation` - The receiver channel for this mock environment to receive acknowledgments back from the `Ventilation` test object.
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
        tx_signal_handler_to_ventilation: Sender<InternalCommand>,
        rx_signal_handler_from_ventilation: Receiver<bool>,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(duration_seconds);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);
            })
            .unwrap()
    }

    // Check if the ventilation is switched on when the temperature is high.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_ventilation_with_measured_temperature_high() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let mut ventilation = prepare_ventilation_tests();

        // Channels for the test object. The test environment only uses one of the channels
        let (tx_ventilation_to_data_logger, rx_data_logger_from_ventilation) = mpsc::channel();
        let (tx_data_logger_to_ventilation, rx_ventilation_from_data_logger) = mpsc::channel();
        let (tx_ventilation_to_relay_manager, rx_relay_manager_from_ventilation) = mpsc::channel();
        let (tx_relay_manager_to_ventilation, rx_ventilation_from_relay_manager) = mpsc::channel();
        let (tx_signal_handler_to_ventilation, rx_ventilation_from_signal_handler) =
            mpsc::channel();
        let (tx_ventilation_to_signal_handler, rx_signal_handler_from_ventilation) =
            mpsc::channel();
        let (tx_schedule_check_to_ventilation, rx_ventilation_from_schedule_check) =
            mpsc::channel();
        let (tx_ventilation_to_schedule_check, rx_schedule_check_from_ventilation) =
            mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_ventilation,
                    &rx_schedule_check_from_ventilation,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_water_temperature = Arc::new(Mutex::new(30.0));

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_ventilation,
                    &rx_data_logger_from_ventilation,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_ventilation,
                    &rx_relay_manager_from_ventilation,
                );
                assert_eq!(actuation_events.len(), 1);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Ventilation)
                );
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            10,
            tx_signal_handler_to_ventilation,
            rx_signal_handler_from_ventilation,
        );

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();
                let tx_ventilation_to_relay_manager_for_test_case_finish =
                    tx_ventilation_to_relay_manager.clone();

                let ventilation_channels = VentilationChannels {
                    tx_ventilation_to_relay_manager,
                    rx_ventilation_from_relay_manager,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_data_logger,
                    rx_ventilation_from_data_logger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    rx_ventilation_from_messaging_opt: None,
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    ventilation_channels,
                    mutex_water_temperature,
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
        tx_relay_manager_to_ventilation: Sender<bool>,
        rx_relay_manager_from_ventilation: Receiver<InternalCommand>,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_ventilation,
                    &rx_relay_manager_from_ventilation,
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
            })
            .unwrap()
    }

    // Check if the ventilation is switched off with low temperature and then switched on during application termination (if configured).
    // This test also implicitly verifies that the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_ventilation_with_measured_temperature_low() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let mut ventilation = prepare_ventilation_tests();

        // Channels for the test object. The test environment only uses one of the channels
        let (tx_ventilation_to_data_logger, rx_data_logger_from_ventilation) = mpsc::channel();
        let (tx_data_logger_to_ventilation, rx_ventilation_from_data_logger) = mpsc::channel();
        let (tx_ventilation_to_relay_manager, rx_relay_manager_from_ventilation) = mpsc::channel();
        let (tx_relay_manager_to_ventilation, rx_ventilation_from_relay_manager) = mpsc::channel();
        let (tx_signal_handler_to_ventilation, rx_ventilation_from_signal_handler) =
            mpsc::channel();
        let (tx_ventilation_to_signal_handler, rx_signal_handler_from_ventilation) =
            mpsc::channel();
        let (tx_schedule_check_to_ventilation, rx_ventilation_from_schedule_check) =
            mpsc::channel();
        let (tx_ventilation_to_schedule_check, rx_schedule_check_from_ventilation) =
            mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_ventilation,
                    &rx_schedule_check_from_ventilation,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_water_temperature = Arc::new(Mutex::new(20.0));

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_ventilation,
                    &rx_data_logger_from_ventilation,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = create_mock_relay_manager(
            tx_relay_manager_to_ventilation,
            rx_relay_manager_from_ventilation,
        );

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            58,
            tx_signal_handler_to_ventilation,
            rx_signal_handler_from_ventilation,
        );

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();
                let tx_ventilation_to_relay_manager_for_test_case_finish =
                    tx_ventilation_to_relay_manager.clone();

                let ventilation_channels = VentilationChannels {
                    tx_ventilation_to_relay_manager,
                    rx_ventilation_from_relay_manager,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_data_logger,
                    rx_ventilation_from_data_logger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    rx_ventilation_from_messaging_opt: None,
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    ventilation_channels,
                    mutex_water_temperature,
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

    // Check if the ventilation is switched on and off during two cycles of the saw tooth profile.
    // Duration of ventilation switched on shall equal the duration of ventilation switched off (1% tolerance).
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_ventilation_with_measured_temperature_50_percent() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let mut ventilation = prepare_ventilation_tests();

        // Channels for the test object. The test environment only uses one of the channels.
        let (tx_ventilation_to_data_logger, rx_data_logger_from_ventilation) = mpsc::channel();
        let (tx_data_logger_to_ventilation, rx_ventilation_from_data_logger) = mpsc::channel();
        let (tx_ventilation_to_relay_manager, rx_relay_manager_from_ventilation) = mpsc::channel();
        let (tx_relay_manager_to_ventilation, rx_ventilation_from_relay_manager) = mpsc::channel();
        let (tx_signal_handler_to_ventilation, rx_ventilation_from_signal_handler) =
            mpsc::channel();
        let (tx_ventilation_to_signal_handler, rx_signal_handler_from_ventilation) =
            mpsc::channel();
        let (tx_schedule_check_to_ventilation, rx_ventilation_from_schedule_check) =
            mpsc::channel();
        let (tx_ventilation_to_schedule_check, rx_schedule_check_from_ventilation) =
            mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_ventilation,
                    &rx_schedule_check_from_ventilation,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_water_temperature = Arc::new(Mutex::new(25.0));

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_ventilation,
                    &rx_data_logger_from_ventilation,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_ventilation,
                    &rx_relay_manager_from_ventilation,
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
                        assert_le!(second_last_actuation_duration, 7600);
                        assert_ge!(second_last_actuation_duration, 5500);
                        assert_le!(third_last_actuation_duration, 7600);
                        assert_ge!(third_last_actuation_duration, 5500);
                        assert_le!(fourth_last_actuation_duration, 7600);
                        assert_ge!(fourth_last_actuation_duration, 5500);
                        assert_le!(fifth_last_actuation_duration, 7600);
                        assert_ge!(fifth_last_actuation_duration, 5500);
                        assert_le!(sixth_last_actuation_duration, 7600);
                        assert_ge!(sixth_last_actuation_duration, 5500);
                    }
                    else {
                        assert_le!(second_last_actuation_duration, 7600);
                        assert_ge!(second_last_actuation_duration, 5000);
                        assert_le!(third_last_actuation_duration, 7600);
                        assert_ge!(third_last_actuation_duration, 5000);
                        assert_le!(fourth_last_actuation_duration, 7600);
                        assert_ge!(fourth_last_actuation_duration, 5000);
                        assert_le!(fifth_last_actuation_duration, 7600);
                        assert_ge!(fifth_last_actuation_duration, 5000);
                        assert_le!(sixth_last_actuation_duration, 7600);
                        assert_ge!(sixth_last_actuation_duration, 5000);
                    }
                }
                assert_le!(delta_min_max_duration, 50);
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            58,
            tx_signal_handler_to_ventilation,
            rx_signal_handler_from_ventilation,
        );

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();
                let tx_ventilation_to_relay_manager_for_test_case_finish =
                    tx_ventilation_to_relay_manager.clone();

                let ventilation_channels = VentilationChannels {
                    tx_ventilation_to_relay_manager,
                    rx_ventilation_from_relay_manager,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_data_logger,
                    rx_ventilation_from_data_logger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    rx_ventilation_from_messaging_opt: None,
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    ventilation_channels,
                    mutex_water_temperature,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_relay_manager_for_test_case_finish
                    .send(InternalCommand::Quit);
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

    // Check if ventilation control is only actuating when quitting while schedule check provides a negative result.
    #[test]
    pub fn test_ventilation_block_by_schedule() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let mut ventilation = prepare_ventilation_tests();

        // Channels for the test object. The test environment only uses one of the channels.
        let (tx_ventilation_to_data_logger, rx_data_logger_from_ventilation) = mpsc::channel();
        let (tx_data_logger_to_ventilation, rx_ventilation_from_data_logger) = mpsc::channel();
        let (tx_ventilation_to_relay_manager, rx_relay_manager_from_ventilation) = mpsc::channel();
        let (tx_relay_manager_to_ventilation, rx_ventilation_from_relay_manager) = mpsc::channel();
        let (tx_signal_handler_to_ventilation, rx_ventilation_from_signal_handler) =
            mpsc::channel();
        let (tx_ventilation_to_signal_handler, rx_signal_handler_from_ventilation) =
            mpsc::channel();
        let (tx_schedule_check_to_ventilation, rx_ventilation_from_schedule_check) =
            mpsc::channel();
        let (tx_ventilation_to_schedule_check, rx_schedule_check_from_ventilation) =
            mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_ventilation,
                    &rx_schedule_check_from_ventilation,
                    None,
                    false,
                );
            })
            .unwrap();

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_water_temperature = Arc::new(Mutex::new(30.0));

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_ventilation,
                    &rx_data_logger_from_ventilation,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = create_mock_relay_manager(
            tx_relay_manager_to_ventilation,
            rx_relay_manager_from_ventilation,
        );

        // thread for controlling duration of test run
        let join_handle_test_environment = create_test_environment(
            10,
            tx_signal_handler_to_ventilation,
            rx_signal_handler_from_ventilation,
        );

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();
                let tx_ventilation_to_relay_manager_for_test_case_finish =
                    tx_ventilation_to_relay_manager.clone();

                let ventilation_channels = VentilationChannels {
                    tx_ventilation_to_relay_manager,
                    rx_ventilation_from_relay_manager,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_data_logger,
                    rx_ventilation_from_data_logger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    rx_ventilation_from_messaging_opt: None,
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    ventilation_channels,
                    mutex_water_temperature,
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

    // Test case runs ventilation control and triggers inhibition by sending the stop message via the channel.
    // After verification that ventilation is stopped,
    // The test case sends the start message via the channel and verifies if ventilation control
    // resumes operation.
    #[test]
    pub fn test_messaging_stops_starts_ventilation() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        let mut ventilation = Ventilation::new(config.ventilation);

        let (tx_ventilation_to_data_logger, rx_data_logger_from_ventilation) = mpsc::channel();
        let (tx_data_logger_to_ventilation, rx_ventilation_from_data_logger) = mpsc::channel();
        let (tx_ventilation_to_relay_manager, rx_relay_manager_from_ventilation) = mpsc::channel();
        let (tx_relay_manager_to_ventilation, rx_ventilation_from_relay_manager) = mpsc::channel();
        let (tx_ventilation_to_signal_handler, rx_signal_handler_from_ventilation) =
            mpsc::channel();
        let (tx_schedule_check_to_ventilation, rx_ventilation_from_schedule_check) =
            mpsc::channel();
        let (tx_ventilation_to_schedule_check, rx_schedule_check_from_ventilation) =
            mpsc::channel();
        let (tx_messaging_to_ventilation, rx_ventilation_from_messaging) = mpsc::channel();
        let (tx_signal_handler_to_ventilation, rx_ventilation_from_signal_handler) =
            mpsc::channel();
        let (_tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));
        let mutex_device_scheduler_test_environment = mutex_device_scheduler_ventilation.clone();

        // thread for mock schedule check
        let join_handle_mock_schedule_check = thread::Builder::new()
            .name("mock_schedule_check".to_string())
            .spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_ventilation,
                    &rx_schedule_check_from_ventilation,
                    None,
                    true,
                );
            })
            .unwrap();

        // Mutex for water temperature normally coming from Atlas Scientific
        let mutex_water_temperature = Arc::new(Mutex::new(30.0));

        // thread for mock Datalogger
        let join_handle_mock_data_logger = thread::Builder::new()
            .name("mock_data_logger".to_string())
            .spawn(move || {
                mock_data_logger(
                    &tx_data_logger_to_ventilation,
                    &rx_data_logger_from_ventilation,
                    &rx_data_logger_from_signal_handler,
                );
            })
            .unwrap();

        // thread for mock relay manager - includes assertions
        let join_handle_mock_relay_manager = thread::Builder::new()
            .name("mock_relay_manager".to_string())
            .spawn(move || {
                let mut actuation_events = mock_relay_manager(
                    &tx_relay_manager_to_ventilation,
                    &rx_relay_manager_from_ventilation,
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
                match tx_messaging_to_ventilation.send(InternalCommand::Stop) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending stop command to test object ({e:?})",
                            LOG_TARGET_VENTILATION
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
                match tx_messaging_to_ventilation.send(InternalCommand::Start) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending start command to test object ({e:?})",
                            LOG_TARGET_VENTILATION
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
                match tx_messaging_to_ventilation.send(InternalCommand::Stop) {
                    Ok(()) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "{}: error when sending stop command to test object ({e:?})",
                            LOG_TARGET_VENTILATION
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
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();

                // check if ventilation is actuated
                assert_eq!(*mutex_device_scheduler_test_environment.lock().unwrap(), 5);
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of the channels to mock threads so that we can terminate them after execution of the test object
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();
                let tx_ventilation_to_relay_manager_for_test_case_finish =
                    tx_ventilation_to_relay_manager.clone();

                let ventilation_channels = VentilationChannels {
                    tx_ventilation_to_relay_manager,
                    rx_ventilation_from_relay_manager,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_data_logger,
                    rx_ventilation_from_data_logger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    rx_ventilation_from_messaging_opt: Some(rx_ventilation_from_messaging),
                };

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    ventilation_channels,
                    mutex_water_temperature,
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
