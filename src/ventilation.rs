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
use std::time::{Duration, SystemTime};

use crate::channel_content::ActuatorState;
use crate::channel_content::AquariumDevice;
use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;
use crate::proc_ext_req::ProcessExternalRequestTrait;
use crate::sawtooth_profile::SawToothProfile;
use crate::sawtooth_profile::SawToothProfileConfig;

const LOG_TARGET_VENTILATION: &str = "ventilation";

const CYCLE_TIME_VENTILATION_MILLIS: u64 = 10;

const TIME_INCREMENT_VENTILATION_MILLIS: f32 = CYCLE_TIME_VENTILATION_MILLIS as f32 / 1000.0;

/// Holds the configuration data for the ventilation control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct VentilationConfig {
    active: bool,
    /// temperature at which the ventilation is switched on for 100% of the time
    /// This temperature must be higher than switch_off_temperature
    switch_on_temperature: f32,

    /// temperature at which the ventilation is switched off for 100% of the time
    /// this temperature must be lower than switch_on_temperature
    switch_off_temperature: f32,

    /// flag to determine the behavior of the control when the application is terminating
    /// If this flag is true, the application will switch on the ventilation when exiting.    
    switch_on_when_terminating: bool,

    /// configuration of saw tooth profile for generation of low-frequency PWM signal
    saw_tooth_profile_config: SawToothProfileConfig,

    // strategy when schedule check does not allow operation:
    // true=ventilation will be switched on; false=ventilation will be switched off
    switch_on_when_out_of_schedule: bool,

    // strategy when external request for stopping ventilation control has been received:
    // true=ventilation will be switched on; false=ventilation will be switched off
    switch_on_when_external_stop: bool,
}

/// Communicates to Controllino thread via channel to switch on the ventilation
/// # Arguments
/// * `mutex_device_scheduler_ventilation` - reference to clone of the mutex for avoiding parallel device actuation.
/// * `tx_to_controllino` - Sender part of channel for communication to Controllino
/// * `rx_from_controllino` - Receiver part of channel for communication from Controllino
/// * `tx_to_datalogger` - Sender part of channel for communication to data logger
fn switch_on_ventilation(
    mutex_device_scheduler_ventilation: &Arc<Mutex<i32>>,
    tx_to_controllino: &Sender<InternalCommand>,
    rx_from_controllino: &Receiver<bool>,
    tx_to_datalogger: &Sender<bool>,
) {
    {
        // avoid electrical overloads through parallel device actuation
        let mut mutex_data = mutex_device_scheduler_ventilation.lock().unwrap();
        match tx_to_controllino.send(InternalCommand::SwitchOn(AquariumDevice::Ventilation)) {
            Ok(_) => {
                // now listen for response
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
        match tx_to_datalogger.send(true) {
            Ok(_) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_VENTILATION,
                    "channel communication to data logger failed when switching on ventilation ({e:?})"
                );
            }
        }

        *mutex_data += 1;
    }
}

/// Communicates to Controllino thread via channel to switch off the ventilation
/// # Arguments
/// * `mutex_device_scheduler_ventilation` - reference to clone of the mutex for avoiding parallel device actuation.
/// * `tx_to_controllino` - Sender part of channel for communication to Controllino
/// * `rx_from_controllino` - Receiver part of channel for communication from Controllino
/// * `tx_to_datalogger` - Sender part of channel for communication to data logger
fn switch_off_ventilation(
    mutex_device_scheduler_ventilation: &Arc<Mutex<i32>>,
    tx_to_controllino: &Sender<InternalCommand>,
    rx_from_controllino: &Receiver<bool>,
    tx_to_datalogger: &Sender<bool>,
) {
    {
        // avoid electrical overloads through parallel device actuation
        let mut mutex_data = mutex_device_scheduler_ventilation.lock().unwrap();
        match tx_to_controllino.send(InternalCommand::SwitchOff(AquariumDevice::Ventilation)) {
            Ok(_) => {
                // now listen for response
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
        match tx_to_datalogger.send(false) {
            Ok(_) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_VENTILATION,
                    "channel communication to data logger when switching off ventilation failed ({e:?})"
                );
            }
        }
        *mutex_data += 1;
    }
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
///     ventilation --> schedule_check[Schedule check]
///     schedule_check --> ventilation
///     messaging[Messaging] --> ventilation
/// ```

pub struct Ventilation {
    config: VentilationConfig,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via channel
    lock_error_channel_receive_termination_signal: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via channel
    lock_warn_inapplicable_command: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages to send request via channel to schedule check
    lock_error_channel_send_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages to receive request via channel from schedule check
    lock_error_channel_receive_schedule_check: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages to send request via channel to Atlas Scientific thread
    lock_error_channel_send_water_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages to receive request via channel from Atlas Scientific thread
    lock_error_channel_receive_water_temperature: bool,
}

impl ProcessExternalRequestTrait for Ventilation {}

impl Ventilation {
    /// Provides a struct of type Ventilation.
    /// # Arguments
    /// * `config` - configuration data for Ventilation control
    pub fn new(config: VentilationConfig) -> Ventilation {
        Self {
            config: config,
            lock_error_channel_receive_termination_signal: false,
            lock_warn_inapplicable_command: false,
            lock_error_channel_send_schedule_check: false,
            lock_error_channel_receive_schedule_check: false,
            lock_error_channel_send_water_temperature: false,
            lock_error_channel_receive_water_temperature: false,
        }
    }

    /// Calculates a normalized controller deviation between measured value and target value in the range from zero to one.
    /// # Arguments
    /// * `measured_value` - measured temperature value    
    fn calc_normalized_control_deviation(&self, measured_value: f32) -> f32 {
        let target_value_delta =
            self.config.switch_on_temperature - self.config.switch_off_temperature;
        let measured_value_delta = measured_value - self.config.switch_off_temperature;
        if target_value_delta > 0.0 {
            measured_value_delta / target_value_delta
        } else {
            0.0 // default value when target values are not properly set up.
        }
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `mutex_device_scheduler_ventilation` - clone of the mutex for avoiding parallel device actuation.
    /// * `tx_to_atlas_scientific` - Sender part of channel for communication to Atlas Scientific thread
    /// * `rx_from_atlas_scientific` - Receiver part of channel for communication from Atlas Scientific thread
    /// * `tx_to_controllino` - Sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Receiver part of channel for communication from Controllino
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    /// * `tx_to_datalogger` - Sender part of channel for communication to data logger
    /// * `tx_to_schedule_check` - Sender part of channel for communication to schedule checker
    /// * `rx_from_schedule_check` - Receiver part of channel for communication from schedule checker                   
    pub fn execute(
        &mut self,
        mutex_device_scheduler_ventilation: Arc<Mutex<i32>>,
        tx_to_atlas_scientific: Sender<InternalCommand>,
        rx_from_atlas_scientific: Receiver<f32>,
        tx_to_controllino: Sender<InternalCommand>,
        rx_from_controllino: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
        tx_to_datalogger: Sender<bool>,
        tx_to_schedule_check: Sender<InternalCommand>,
        rx_from_schedule_check: Receiver<bool>,
        rx_from_messaging: Receiver<InternalCommand>,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut saw_tooth_profile = SawToothProfile::new(&self.config.saw_tooth_profile_config);
        let mut measured_temperature = 0.0;
        let mut measurement_error: bool;
        let mut ventilation_state: ActuatorState = ActuatorState::Undefined;
        let mut schedule_check_result: bool = false;
        let mut ventilation_inhibited = false; // state of ventilation control determined by if start/stop command has been received
        let mut lock_warn_cycle_time_exceeded = false;

        loop {
            let start_time = SystemTime::now();

            let (quit_command_received, start_command_received, stop_command_received) =
                self.process_external_request(&rx_from_signal_handler, Some(&rx_from_messaging));
            if quit_command_received {
                break;
            }
            if stop_command_received {
                info!(
                    target: LOG_TARGET_VENTILATION,
                    "received Stop command. Inhibiting ventilation."
                );
                ventilation_inhibited = true;
            }
            if start_command_received {
                info!(
                    target: LOG_TARGET_VENTILATION,
                    "received Start command. Restarting ventilation."
                );
                ventilation_inhibited = false;
            }

            if self.config.active {
                // send request for temperature
                match tx_to_atlas_scientific.send(InternalCommand::RequestSignal(
                    AquariumSignal::WaterTemperature,
                )) {
                    Ok(_) => {
                        self.lock_error_channel_send_water_temperature = false;
                        measurement_error = false;
                        // now listen for response
                        measured_temperature = match rx_from_atlas_scientific.recv() {
                            Ok(c) => {
                                self.lock_error_channel_receive_water_temperature = false;
                                c
                            }
                            Err(e) => {
                                if !self.lock_error_channel_receive_water_temperature {
                                    error!(
                                        target: LOG_TARGET_VENTILATION,
                                        "reading channel content for water temperature failed ({e:?})"
                                    );
                                    self.lock_error_channel_receive_water_temperature = true;
                                }
                                measurement_error = true;
                                measured_temperature // keep the old value
                            }
                        };
                    }
                    Err(e) => {
                        measurement_error = true;
                        if !self.lock_error_channel_send_water_temperature {
                            error!(
                                target: LOG_TARGET_VENTILATION,
                                "channel communication to Atlas Scientific thread when requesting water temperature failed ({e:?})"
                            );
                            self.lock_error_channel_send_water_temperature = true;
                        }
                    }
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
                    if (measured_temperature >= self.config.switch_off_temperature)
                        && (measured_temperature <= self.config.switch_on_temperature)
                    {
                        if (self.calc_normalized_control_deviation(measured_temperature)
                            > saw_tooth_profile.level_normalized)
                            && !measurement_error
                        {
                            // switch on ventilation if not already on
                            if ventilation_state != ActuatorState::On {
                                switch_on_ventilation(
                                    &mutex_device_scheduler_ventilation,
                                    &tx_to_controllino,
                                    &rx_from_controllino,
                                    &tx_to_datalogger,
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
                                    &tx_to_datalogger,
                                );
                                ventilation_state = ActuatorState::Off;
                            }
                        }
                    } else {
                        if measured_temperature > self.config.switch_on_temperature {
                            // switch on ventilation if not already on
                            if ventilation_state != ActuatorState::On {
                                switch_on_ventilation(
                                    &mutex_device_scheduler_ventilation,
                                    &tx_to_controllino,
                                    &rx_from_controllino,
                                    &tx_to_datalogger,
                                );
                                ventilation_state = ActuatorState::On;
                            }
                        }
                        if measured_temperature < self.config.switch_off_temperature {
                            // switch off ventilation if not already off
                            if ventilation_state != ActuatorState::Off {
                                switch_off_ventilation(
                                    &mutex_device_scheduler_ventilation,
                                    &tx_to_controllino,
                                    &rx_from_controllino,
                                    &tx_to_datalogger,
                                );
                                ventilation_state = ActuatorState::Off;
                            }
                        }
                    }
                } else {
                    if ventilation_inhibited {
                        // external request for stopping ventilation control has been received
                        match self.config.switch_on_when_external_stop {
                            true => {
                                // switch on heater if not already on
                                if ventilation_state != ActuatorState::On {
                                    switch_on_ventilation(
                                        &mutex_device_scheduler_ventilation,
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                        &tx_to_datalogger,
                                    );
                                    ventilation_state = ActuatorState::On;
                                }
                            }
                            false => {
                                // switch off heater if not already off
                                if ventilation_state != ActuatorState::Off {
                                    switch_off_ventilation(
                                        &mutex_device_scheduler_ventilation,
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                        &tx_to_datalogger,
                                    );
                                    ventilation_state = ActuatorState::Off;
                                }
                            }
                        }
                    } else {
                        // schedule check requires stop of ventilation control
                        match self.config.switch_on_when_out_of_schedule {
                            true => {
                                // switch on heater if not already on
                                if ventilation_state != ActuatorState::On {
                                    switch_on_ventilation(
                                        &mutex_device_scheduler_ventilation,
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                        &tx_to_datalogger,
                                    );
                                    ventilation_state = ActuatorState::On;
                                }
                            }
                            false => {
                                // switch off heater if not already off
                                if ventilation_state != ActuatorState::Off {
                                    switch_off_ventilation(
                                        &mutex_device_scheduler_ventilation,
                                        &tx_to_controllino,
                                        &rx_from_controllino,
                                        &tx_to_datalogger,
                                    );
                                    ventilation_state = ActuatorState::Off;
                                }
                            }
                        }
                    }
                }
            }

            saw_tooth_profile.execute_with(TIME_INCREMENT_VENTILATION_MILLIS);

            let stop_time = SystemTime::now();
            let execution_duration_millis =
                stop_time.duration_since(start_time).unwrap().as_millis() as u64;

            let remaining_sleep_time_millis: u64;
            if execution_duration_millis >= CYCLE_TIME_VENTILATION_MILLIS {
                if !lock_warn_cycle_time_exceeded {
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
            let sleep_duration = Duration::from_millis(remaining_sleep_time_millis);
            spin_sleeper.sleep(sleep_duration);
        }

        if self.config.switch_on_when_terminating {
            // switch on ventilation if not already on
            if ventilation_state != ActuatorState::On {
                switch_on_ventilation(
                    &mutex_device_scheduler_ventilation,
                    &tx_to_controllino,
                    &rx_from_controllino,
                    &tx_to_datalogger,
                );
            }
        } else {
            // switch off ventilation if not already off
            if ventilation_state != ActuatorState::Off {
                switch_off_ventilation(
                    &mutex_device_scheduler_ventilation,
                    &tx_to_controllino,
                    &rx_from_controllino,
                    &tx_to_datalogger,
                );
            }
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_VENTILATION,
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // this thread has channel connection to underlying threads (Controllino, Atlas Scientific).
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
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::Duration;

    use crate::channel_content::AquariumDevice;
    use crate::channel_content::InternalCommand;
    use crate::common::tests::update_min_max_actuation_duration;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::mock::tests::mock_atlas_scientific;
    use crate::mock::tests::mock_controllino;
    use crate::mock::tests::mock_datalogger;
    use crate::mock::tests::mock_schedule_check;
    use crate::mock::tests::ActuationEvent;
    use crate::ventilation::Ventilation;

    fn prepare_ventilation_tests(short_ramp: bool) -> Ventilation {
        // prepare struct Ventilation for subsequent tests
        let config: ConfigData;
        if short_ramp {
            config = read_config_file(
                "/config/aquarium_control_test_ventilation_secondary.toml".to_string(),
            );
        } else {
            config = read_config_file("/config/aquarium_control_test_ventilation.toml".to_string());
        }
        Ventilation::new(config.ventilation)
    }

    // Check if the ventilation is switched on when temperature is high.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_ventilation_with_measured_temperature_high() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let mut ventilation = prepare_ventilation_tests(false);

        // channels for the test object. test environment only uses one of the channels
        let (tx_ventilation_to_atlas_scientific, rx_atlas_scientific_from_ventilation) =
            mpsc::channel();
        let (tx_atlas_scientific_to_ventilation, rx_ventilation_from_atlas_scientific) =
            mpsc::channel();
        let (tx_ventilation_to_datalogger, rx_datalogger_from_ventilation) = mpsc::channel();
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
        let (_tx_ventilation_to_messaging, rx_ventilation_from_messaging) = mpsc::channel();

        scope(|scope| {
            // thread for mock schedule check
            scope.spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_ventilation,
                    &rx_schedule_check_from_ventilation,
                    true,
                );
            });

            // thread for mock AtlasScientific
            scope.spawn(move || {
                mock_atlas_scientific(
                    &tx_atlas_scientific_to_ventilation,
                    &rx_atlas_scientific_from_ventilation,
                    30.0,
                    50000.0,
                    7.0,
                );
            });

            // thread for mock Datalogger
            scope.spawn(move || {
                mock_datalogger(&rx_datalogger_from_ventilation);
            });

            let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

            // thread for mock Controllino - includes assertions
            scope.spawn(move || {
                let mut actuation_events = mock_controllino(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
                );
                assert_eq!(actuation_events.len(), 1);
                let actuation_event = actuation_events.pop().unwrap();
                assert_eq!(
                    actuation_event.command,
                    InternalCommand::SwitchOn(AquariumDevice::Ventilation)
                );
            });

            // thread for controlling duration of test run
            scope.spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);
            });

            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();
                let tx_ventilation_to_atlas_scientific_for_test_case_finish =
                    tx_ventilation_to_atlas_scientific.clone();
                let tx_ventilation_to_controllino_for_test_case_finish =
                    tx_ventilation_to_controllino.clone();

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    tx_ventilation_to_atlas_scientific,
                    rx_ventilation_from_atlas_scientific,
                    tx_ventilation_to_controllino,
                    rx_ventilation_from_controllino,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_datalogger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    rx_ventilation_from_messaging,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_atlas_scientific_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ =
                    tx_ventilation_to_controllino_for_test_case_finish.send(InternalCommand::Quit);
                println!("* [Ventilation] checking reaction to high temperature succeeded.");
            });
        });
    }

    // Check if the ventilation is switched off with low temperature and switched back on when exiting the application.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Check if the ventilation is switched off after sending Quit command and before sending Terminate command.
    #[test]
    pub fn test_ventilation_with_measured_temperature_low() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let mut ventilation = prepare_ventilation_tests(false);

        // channels for the test object. test environment only uses one of the channels
        let (tx_ventilation_to_atlas_scientific, rx_atlas_scientific_from_ventilation) =
            mpsc::channel();
        let (tx_atlas_scientific_to_ventilation, rx_ventilation_from_atlas_scientific) =
            mpsc::channel();
        let (tx_ventilation_to_datalogger, rx_datalogger_from_ventilation) = mpsc::channel();
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
        let (_tx_ventilation_to_messaging, rx_ventilation_from_messaging) = mpsc::channel();

        scope(|scope| {
            // thread for mock schedule check
            scope.spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_ventilation,
                    &rx_schedule_check_from_ventilation,
                    true,
                );
            });

            // thread for mock AtlasScientific
            scope.spawn(move || {
                mock_atlas_scientific(
                    &tx_atlas_scientific_to_ventilation,
                    &rx_atlas_scientific_from_ventilation,
                    20.0,
                    50000.0,
                    7.0,
                );
            });

            // thread for mock Datalogger
            scope.spawn(move || {
                mock_datalogger(&rx_datalogger_from_ventilation);
            });

            let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

            // thread for mock Controllino - includes assertions
            scope.spawn(move || {
                let mut actuation_events = mock_controllino(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
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
            });

            // thread for controlling duration of test run
            scope.spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);
            });

            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();
                let tx_ventilation_to_atlas_scientific_for_test_case_finish =
                    tx_ventilation_to_atlas_scientific.clone();
                let tx_ventilation_to_controllino_for_test_case_finish =
                    tx_ventilation_to_controllino.clone();

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    tx_ventilation_to_atlas_scientific,
                    rx_ventilation_from_atlas_scientific,
                    tx_ventilation_to_controllino,
                    rx_ventilation_from_controllino,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_datalogger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    rx_ventilation_from_messaging,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_atlas_scientific_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ =
                    tx_ventilation_to_controllino_for_test_case_finish.send(InternalCommand::Quit);
                println!("* [Ventilation] checking reaction to low temperature.");
            });
        });
    }

    // Check if the ventilation is switched on and off during two cycles of the saw tooth profile.
    // Duration of ventilation switched on shall equal duration of ventilation switched off (1% tolerance).
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_ventilation_with_measured_temperature_50_percent() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let mut ventilation = prepare_ventilation_tests(true);

        // channels for the test object. test environment only uses one of the channels
        let (tx_ventilation_to_atlas_scientific, rx_atlas_scientific_from_ventilation) =
            mpsc::channel();
        let (tx_atlas_scientific_to_ventilation, rx_ventilation_from_atlas_scientific) =
            mpsc::channel();
        let (tx_ventilation_to_datalogger, rx_datalogger_from_ventilation) = mpsc::channel();
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
        let (_tx_ventilation_to_messaging, rx_ventilation_from_messaging) = mpsc::channel();

        scope(|scope| {
            // thread for mock schedule check
            scope.spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_ventilation,
                    &rx_schedule_check_from_ventilation,
                    true,
                );
            });

            // thread for mock AtlasScientific
            scope.spawn(move || {
                mock_atlas_scientific(
                    &tx_atlas_scientific_to_ventilation,
                    &rx_atlas_scientific_from_ventilation,
                    25.0,
                    50000.0,
                    7.0,
                );
            });

            // thread for mock Datalogger
            scope.spawn(move || {
                mock_datalogger(&rx_datalogger_from_ventilation);
            });

            let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

            // thread for mock Controllino - includes assertions
            scope.spawn(move || {
                let mut actuation_events = mock_controllino(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
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

                assert_le!(second_last_actuation_duration, 8080);
                assert_ge!(second_last_actuation_duration, 7800);
                assert_le!(third_last_actuation_duration, 8080);
                assert_ge!(third_last_actuation_duration, 7800);
                assert_le!(fourth_last_actuation_duration, 8080);
                assert_ge!(fourth_last_actuation_duration, 7800);
                assert_le!(fifth_last_actuation_duration, 8080);
                assert_ge!(fifth_last_actuation_duration, 7800);
                assert_le!(sixth_last_actuation_duration, 8080);
                assert_ge!(sixth_last_actuation_duration, 7800);
                assert_le!(delta_min_max_duration, 100);
            });

            // thread for controlling duration of test run
            scope.spawn(move || {
                let sleep_duration = Duration::from_secs(59);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration);
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);
            });

            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();
                let tx_ventilation_to_atlas_scientific_for_test_case_finish =
                    tx_ventilation_to_atlas_scientific.clone();
                let tx_ventilation_to_controllino_for_test_case_finish =
                    tx_ventilation_to_controllino.clone();

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    tx_ventilation_to_atlas_scientific,
                    rx_ventilation_from_atlas_scientific,
                    tx_ventilation_to_controllino,
                    rx_ventilation_from_controllino,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_datalogger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    rx_ventilation_from_messaging,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_atlas_scientific_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ =
                    tx_ventilation_to_controllino_for_test_case_finish.send(InternalCommand::Quit);
                println!("* [Heating] checking reaction to medium temperature succeeded.");
            });
        });
    }

    // Check if ventilation control is only actuating when quitting while schedule check provides negative result
    #[test]
    pub fn test_ventilation_block_by_schedule() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let mut ventilation = prepare_ventilation_tests(false);

        // channels for the test object. test environment only uses one of the channels
        let (tx_ventilation_to_atlas_scientific, rx_atlas_scientific_from_ventilation) =
            mpsc::channel();
        let (tx_atlas_scientific_to_ventilation, rx_ventilation_from_atlas_scientific) =
            mpsc::channel();
        let (tx_ventilation_to_datalogger, rx_datalogger_from_ventilation) = mpsc::channel();
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
        let (_tx_messaging_to_ventilation, rx_ventilation_from_messaging) = mpsc::channel();

        scope(|scope| {
            // thread for mock schedule check
            scope.spawn(move || {
                mock_schedule_check(
                    &tx_schedule_check_to_ventilation,
                    &rx_schedule_check_from_ventilation,
                    false,
                );
            });

            // thread for mock AtlasScientific
            scope.spawn(move || {
                mock_atlas_scientific(
                    &tx_atlas_scientific_to_ventilation,
                    &rx_atlas_scientific_from_ventilation,
                    30.0,
                    50000.0,
                    7.0,
                );
            });

            // thread for mock Datalogger
            scope.spawn(move || {
                mock_datalogger(&rx_datalogger_from_ventilation);
            });

            let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

            // thread for mock Controllino - includes assertions
            scope.spawn(move || {
                let mut actuation_events = mock_controllino(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
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
            });

            // thread for controlling duration of test run
            scope.spawn(move || {
                let sleep_duration_ten_seconds = Duration::from_secs(10);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_ten_seconds);
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);
            });

            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of test object
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();
                let tx_ventilation_to_atlas_scientific_for_test_case_finish =
                    tx_ventilation_to_atlas_scientific.clone();
                let tx_ventilation_to_controllino_for_test_case_finish =
                    tx_ventilation_to_controllino.clone();

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    tx_ventilation_to_atlas_scientific,
                    rx_ventilation_from_atlas_scientific,
                    tx_ventilation_to_controllino,
                    rx_ventilation_from_controllino,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_datalogger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    rx_ventilation_from_messaging,
                );

                // send Quit signal to mock threads because test object has terminated
                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ = tx_ventilation_to_atlas_scientific_for_test_case_finish
                    .send(InternalCommand::Quit);
                let _ =
                    tx_ventilation_to_controllino_for_test_case_finish.send(InternalCommand::Quit);

                let actuation_count = *mutex_device_scheduler_ventilation.lock().unwrap();

                // check if ventilation is only actuated once (when quitting)
                assert_eq!(actuation_count, 2);
            });
            println!("* [Ventilation] checking if schedule checker can block actuation.");
        });
    }
}
