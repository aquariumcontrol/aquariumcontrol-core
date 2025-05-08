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

/// Communicates to Controllino thread via the channel to switch on the ventilation
/// # Arguments
/// * `mutex_device_scheduler_ventilation` - reference to clone of the mutex for avoiding parallel device actuation.
/// * `tx_to_controllino` - Sender part of channel for communication to Controllino
/// * `rx_from_controllino` - Receiver part of channel for communication from Controllino
/// * `tx_to_data_logger` - Sender part of channel for communication to data logger
fn switch_on_ventilation(
    mutex_device_scheduler_ventilation: &Arc<Mutex<i32>>,
    tx_to_controllino: &Sender<InternalCommand>,
    rx_from_controllino: &Receiver<bool>,
    mutex_blocked_during_actuation: &mut bool,
) {
    // reduce probability of wrong warning
    *mutex_blocked_during_actuation |=
        check_if_mutex_is_blocked(mutex_device_scheduler_ventilation);

    // avoid electrical overloads through parallel device actuation
    let mut mutex_data = mutex_device_scheduler_ventilation.lock().unwrap();
    match tx_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::Ventilation)) {
        Ok(_) => {
            // now listen for the response
            match rx_from_controllino.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_VENTILATION,
                        "receiving answer from Controllino when switching on ventilation failed ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_VENTILATION,
                "channel communication to Controllino when switching on ventilation ({e:?})"
            );
        }
    }
    *mutex_data += 1;
}

/// Communicates to Controllino thread via the channel to switch off the ventilation
/// # Arguments
/// * `mutex_device_scheduler_ventilation` - reference to clone of the mutex for avoiding parallel device actuation.
/// * `tx_to_controllino` - Sender part of the channel for communication to Controllino
/// * `rx_from_controllino` - Receiver part of the channel for communication from Controllino
/// * `tx_to_data_logger` - Sender part of the channel for communication to data logger
fn switch_off_ventilation(
    mutex_device_scheduler_ventilation: &Arc<Mutex<i32>>,
    tx_to_controllino: &Sender<InternalCommand>,
    rx_from_controllino: &Receiver<bool>,
    mutex_blocked_during_actuation: &mut bool,
) {
    // reduce probability of wrong warning
    *mutex_blocked_during_actuation |=
        check_if_mutex_is_blocked(mutex_device_scheduler_ventilation);

    // avoid electrical overloads through parallel device actuation
    let mut mutex_data = mutex_device_scheduler_ventilation.lock().unwrap();
    match tx_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::Ventilation)) {
        Ok(_) => {
            // now listen for the response
            match rx_from_controllino.recv() {
                Ok(_) => {}
                Err(e) => {
                    error!(
                        target: LOG_TARGET_VENTILATION,
                        "receiving answer from Controllino when switching off ventilation failed ({e:?})"
                    );
                }
            };
        }
        Err(e) => {
            error!(
                target: LOG_TARGET_VENTILATION,
                "channel communication to Controllino when switching off ventilation failed ({e:?})"
            );
        }
    }
    *mutex_data += 1;
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Holds the configuration and the implementation for the ventilation control.
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     ventilation[Ventilation control] --> atlas_scientific[AtlasScientific]
///     atlas_scientific --> ventilation
///     ventilation --> controllino[Controllino]
///     controllino --> ventilation
///     ventilation --> signal_handler[Signal handler]
///     signal_handler --> ventilation
///     ventilation --> data_logger[Data logger]
///     data_logger --> ventilation
///     ventilation --> schedule_check[Schedule check]
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
    /// Provides a struct of type Ventilation.
    /// # Arguments
    /// * `config` - configuration data for Ventilation control
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

    /// Calculates a normalized controller deviation between measured value and target value in the range from zero to one.
    /// # Arguments
    /// * `measured_value` - measured temperature value    
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

    /// check if data logger sent a command and respond
    fn communicate_with_data_logger(
        &mut self,
        ventilation_state: &ActuatorState,
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
                            AquariumSignal::VentilationControlStatus => {
                                self.lock_error_invalid_signal_request_data_logger = false;
                                let response_to_data_logger = match ventilation_state {
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

    /// Executes the main control in a loop until the signal handler commands quitting and termination.
    /// # Arguments
    /// * `mutex_device_scheduler_ventilation` - clone of the mutex for avoiding parallel device actuation.
    /// * `tx_to_controllino` - Sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Receiver part of channel for communication from Controllino
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    /// * `tx_to_data_logger` - Sender part of channel for communication to data logger
    /// * `rx_from_data_logger` - Receiver part of channel for communication from data logger
    /// * `tx_to_schedule_check` - Sender part of channel for communication to schedule checker
    /// * `rx_from_schedule_check` - Receiver part of channel for communication from schedule checker                   
    /// * `mutex_water_temperature` - Mutex which contains water temperature                   
    pub fn execute(
        &mut self,
        mutex_device_scheduler_ventilation: Arc<Mutex<i32>>,
        tx_to_controllino: Sender<InternalCommand>,
        rx_from_controllino: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
        tx_to_data_logger: Sender<bool>,
        rx_from_data_logger: Receiver<InternalCommand>,
        tx_to_schedule_check: Sender<InternalCommand>,
        rx_from_schedule_check: Receiver<bool>,
        rx_from_messaging_opt: Option<Receiver<InternalCommand>>,
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
        let mut mutex_blocked_during_actuation: bool = false;

        loop {
            let (quit_command_received, start_command_received, stop_command_received) = self
                .process_external_request(&rx_from_signal_handler, rx_from_messaging_opt.as_ref());
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
                match tx_to_schedule_check.send(InternalCommand::ScheduleCheck) {
                    Ok(_) => {
                        self.lock_error_channel_send_schedule_check = false;
                        // now listen for the response
                        schedule_check_result = match rx_from_schedule_check.recv() {
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
                                    &tx_to_controllino,
                                    &rx_from_controllino,
                                    &mut mutex_blocked_during_actuation,
                                );
                                ventilation_state = ActuatorState::On;
                            }
                        } else {
                            // switch off ventilation if not already off
                            if ventilation_state != ActuatorState::Off {
                                switch_off_ventilation(
                                    &mutex_device_scheduler_ventilation,
                                    &tx_to_controllino,
                                    &rx_from_controllino,
                                    &mut mutex_blocked_during_actuation,
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
                                    &tx_to_controllino,
                                    &rx_from_controllino,
                                    &mut mutex_blocked_during_actuation,
                                );
                                ventilation_state = ActuatorState::On;
                            }
                        }
                        if measured_water_temperature < self.config.switch_off_temperature {
                            // switch off ventilation if not already off
                            if ventilation_state != ActuatorState::Off {
                                switch_off_ventilation(
                                    &mutex_device_scheduler_ventilation,
                                    &tx_to_controllino,
                                    &rx_from_controllino,
                                    &mut mutex_blocked_during_actuation,
                                );
                                ventilation_state = ActuatorState::Off;
                            }
                        }
                    }
                } else {
                    if ventilation_inhibited {
                        // An external request for stopping ventilation control has been received
                        match self.config.switch_on_when_external_stop {
                            true => {
                                // switch on the heater if not already on
                                if ventilation_state != ActuatorState::On {
                                    switch_on_ventilation(
                                        &mutex_device_scheduler_ventilation,
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                        &mut mutex_blocked_during_actuation,
                                    );
                                    ventilation_state = ActuatorState::On;
                                }
                            }
                            false => {
                                // switch off the heater if not already off
                                if ventilation_state != ActuatorState::Off {
                                    switch_off_ventilation(
                                        &mutex_device_scheduler_ventilation,
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                        &mut mutex_blocked_during_actuation,
                                    );
                                    ventilation_state = ActuatorState::Off;
                                }
                            }
                        }
                    } else {
                        // schedule check requires stop of ventilation control
                        match self.config.switch_on_when_out_of_schedule {
                            true => {
                                // switch on the heater if not already on
                                if ventilation_state != ActuatorState::On {
                                    switch_on_ventilation(
                                        &mutex_device_scheduler_ventilation,
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                        &mut mutex_blocked_during_actuation,
                                    );
                                    ventilation_state = ActuatorState::On;
                                }
                            }
                            false => {
                                // switch off the heater if not already off
                                if ventilation_state != ActuatorState::Off {
                                    switch_off_ventilation(
                                        &mutex_device_scheduler_ventilation,
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                        &mut mutex_blocked_during_actuation,
                                    );
                                    ventilation_state = ActuatorState::Off;
                                }
                            }
                        }
                    }
                }
            }

            self.communicate_with_data_logger(
                &ventilation_state,
                &tx_to_data_logger,
                &rx_from_data_logger,
            );

            saw_tooth_profile.execute_with(TIME_INCREMENT_VENTILATION_SECS);

            let stop_time = SystemTime::now();
            let execution_duration_millis =
                stop_time.duration_since(start_time).unwrap().as_millis() as u64;

            let remaining_sleep_time_millis: u64;
            if execution_duration_millis >= CYCLE_TIME_VENTILATION_MILLIS {
                if !lock_warn_cycle_time_exceeded && !!mutex_blocked_during_actuation {
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
            mutex_blocked_during_actuation = false; // reset for the next cycle

            let sleep_duration = Duration::from_millis(remaining_sleep_time_millis);
            spin_sleeper.sleep(sleep_duration);

            start_time = SystemTime::now(); // start time for the next cycle
        }

        if self.config.switch_on_when_terminating {
            // switch on ventilation if not already on
            if ventilation_state != ActuatorState::On {
                switch_on_ventilation(
                    &mutex_device_scheduler_ventilation,
                    &tx_to_controllino,
                    &rx_from_controllino,
                    &mut mutex_blocked_during_actuation,
                );
            }
        } else {
            // switch off ventilation if not already off
            if ventilation_state != ActuatorState::Off {
                switch_off_ventilation(
                    &mutex_device_scheduler_ventilation,
                    &tx_to_controllino,
                    &rx_from_controllino,
                    &mut mutex_blocked_during_actuation,
                );
            }
        }

        // The application received a request to terminate, that is why the loop was left.
        // Answer to the thread which sent the request for termination, so that shutdown can proceed further.
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_VENTILATION,
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // This thread has channel connections to underlying threads (Controllino, Atlas Scientific).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
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
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use crate::thermal::ventilation::{Ventilation, LOG_TARGET_VENTILATION};
    use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
    use crate::utilities::common::tests::update_min_max_actuation_duration;
    use crate::utilities::config::{read_config_file, ConfigData};

    use crate::stubs::mock_controllino::tests::mock_controllino;
    use crate::stubs::mock_data_logger::tests::mock_data_logger;
    use crate::stubs::mock_schedule_check::tests::mock_schedule_check;

    fn prepare_ventilation_tests() -> Ventilation {
        // prepare struct Ventilation for subsequent tests
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        Ventilation::new(config.ventilation)
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
        let (tx_ventilation_to_controllino, rx_controllino_from_ventilation) = mpsc::channel();
        let (tx_controllino_to_ventilation, rx_ventilation_from_controllino) = mpsc::channel();
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

        // thread for mock Controllino - includes assertions
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events = mock_controllino(
                    &tx_controllino_to_ventilation,
                    &rx_controllino_from_ventilation,
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
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of the test object
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();
                let tx_ventilation_to_controllino_for_test_case_finish =
                    tx_ventilation_to_controllino.clone();

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    tx_ventilation_to_controllino,
                    rx_ventilation_from_controllino,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_data_logger,
                    rx_ventilation_from_data_logger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    None,
                    mutex_water_temperature,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ =
                    tx_ventilation_to_controllino_for_test_case_finish.send(InternalCommand::Quit);
                println!("* [Ventilation] checking reaction to high temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
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

    // Check if the ventilation is switched off with low temperature and switched back on when exiting the application.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Check if the ventilation is switched off after sending Quit command and before sending Terminate command.
    #[test]
    pub fn test_ventilation_with_measured_temperature_low() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let mut ventilation = prepare_ventilation_tests();

        // Channels for the test object. The test environment only uses one of the channels
        let (tx_ventilation_to_data_logger, rx_data_logger_from_ventilation) = mpsc::channel();
        let (tx_data_logger_to_ventilation, rx_ventilation_from_data_logger) = mpsc::channel();
        let (tx_ventilation_to_controllino, rx_controllino_from_ventilation) = mpsc::channel();
        let (tx_controllino_to_ventilation, rx_ventilation_from_controllino) = mpsc::channel();
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

        // thread for mock Controllino - includes assertions
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events = mock_controllino(
                    &tx_controllino_to_ventilation,
                    &rx_controllino_from_ventilation,
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
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of the test object
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();
                let tx_ventilation_to_controllino_for_test_case_finish =
                    tx_ventilation_to_controllino.clone();

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    tx_ventilation_to_controllino,
                    rx_ventilation_from_controllino,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_data_logger,
                    rx_ventilation_from_data_logger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    None,
                    mutex_water_temperature,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ =
                    tx_ventilation_to_controllino_for_test_case_finish.send(InternalCommand::Quit);
                println!("* [Ventilation] checking reaction to low temperature.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
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
        let (tx_ventilation_to_controllino, rx_controllino_from_ventilation) = mpsc::channel();
        let (tx_controllino_to_ventilation, rx_ventilation_from_controllino) = mpsc::channel();
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

        // thread for mock Controllino - includes assertions
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events = mock_controllino(
                    &tx_controllino_to_ventilation,
                    &rx_controllino_from_ventilation,
                );
                println!(
                    "Mock Controllino received {} commands:",
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
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration = Duration::from_secs(58);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration);
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of the test object
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();
                let tx_ventilation_to_controllino_for_test_case_finish =
                    tx_ventilation_to_controllino.clone();

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    tx_ventilation_to_controllino,
                    rx_ventilation_from_controllino,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_data_logger,
                    rx_ventilation_from_data_logger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    None,
                    mutex_water_temperature,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ =
                    tx_ventilation_to_controllino_for_test_case_finish.send(InternalCommand::Quit);
                println!("* [Heating] checking reaction to medium temperature succeeded.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
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

    // Check if ventilation control is only actuating when quitting while schedule check provides a negative result.
    #[test]
    pub fn test_ventilation_block_by_schedule() {
        let sleep_duration_100_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let mut ventilation = prepare_ventilation_tests();

        // Channels for the test object. The test environment only uses one of the channels.
        let (tx_ventilation_to_data_logger, rx_data_logger_from_ventilation) = mpsc::channel();
        let (tx_data_logger_to_ventilation, rx_ventilation_from_data_logger) = mpsc::channel();
        let (tx_ventilation_to_controllino, rx_controllino_from_ventilation) = mpsc::channel();
        let (tx_controllino_to_ventilation, rx_ventilation_from_controllino) = mpsc::channel();
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

        // thread for mock Controllino - includes assertions
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events = mock_controllino(
                    &tx_controllino_to_ventilation,
                    &rx_controllino_from_ventilation,
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
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);
            })
            .unwrap();

        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of the test object
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();
                let tx_ventilation_to_controllino_for_test_case_finish =
                    tx_ventilation_to_controllino.clone();

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    tx_ventilation_to_controllino,
                    rx_ventilation_from_controllino,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_data_logger,
                    rx_ventilation_from_data_logger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    None,
                    mutex_water_temperature,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ =
                    tx_ventilation_to_controllino_for_test_case_finish.send(InternalCommand::Quit);

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
        let (tx_ventilation_to_controllino, rx_controllino_from_ventilation) = mpsc::channel();
        let (tx_controllino_to_ventilation, rx_ventilation_from_controllino) = mpsc::channel();
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

        // thread for mock Controllino - includes assertions
        let join_handle_mock_controllino = thread::Builder::new()
            .name("mock_controllino".to_string())
            .spawn(move || {
                let mut actuation_events = mock_controllino(
                    &tx_controllino_to_ventilation,
                    &rx_controllino_from_ventilation,
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
                // clone sender part of channels to mock threads so that we can terminate them after execution of the test object
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();
                let tx_ventilation_to_controllino_for_test_case_finish =
                    tx_ventilation_to_controllino.clone();

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    tx_ventilation_to_controllino,
                    rx_ventilation_from_controllino,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_data_logger,
                    rx_ventilation_from_data_logger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    Some(rx_ventilation_from_messaging),
                    mutex_water_temperature,
                );

                // send Quit signal to mock threads because the test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ =
                    tx_ventilation_to_controllino_for_test_case_finish.send(InternalCommand::Quit);

                println!("* [Ventilation] checking if messaging can block and restart actuation.");
            })
            .unwrap();

        join_handle_mock_schedule_check
            .join()
            .expect("Mock schedule check did not finish.");
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
