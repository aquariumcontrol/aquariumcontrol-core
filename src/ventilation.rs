/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

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
                        println!("Ventilation: Error when trying to receive answer from Controllino when switching on ventilation: {e:?}");
                    }
                };
            }
            Err(e) => {
                println!("Ventilation: Error in channel communication to Controllino when switching on ventilation: {e:?}");
            }
        }
        match tx_to_datalogger.send(true) {
            Ok(_) => {}
            Err(e) => {
                println!("Ventilation: Error in channel communication to data logger when switching on ventilation: {e:?}");
            }
        }
        println!("Ventilation: Switched on ventilation.");

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
                        println!("Ventilation: Error when trying to receive answer from Controllino when switching off ventilation: {e:?}");
                    }
                };
            }
            Err(e) => {
                println!("Ventilation: Error in channel communication to Controllino when switching off ventilation: {e:?}");
            }
        }
        match tx_to_datalogger.send(false) {
            Ok(_) => {}
            Err(e) => {
                println!("Ventilation: Error in channel communication to data logger when switching off ventilation: {e:?}");
            }
        }
        println!("Ventilation: Switched off ventilation.");
        *mutex_data += 1;
    }
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Holds the configuration and the implementation for the ventilation control.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     ventilation[Heating control] --> gpio_handler[GPIO handler]
///     gpio_handler --> ventilation
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
}

impl ProcessExternalRequestTrait for Ventilation {}

impl Ventilation {
    /// Provides a struct of type Ventilation.
    /// # Arguments
    /// * `config` - configuration data for Ventilation control
    pub fn new(config: VentilationConfig) -> Ventilation {
        Self { config: config }
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
    /// * `tx_to_gpio` - Sender part of channel for communication to GPIO handler
    /// * `rx_from_gpio` - Receiver part of channel for communication from GPIO handler
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
        tx_to_gpio: Sender<InternalCommand>,
        rx_from_gpio: Receiver<f32>,
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
        let mut loop_counter = 0;
        let mut ventilation_state: ActuatorState = ActuatorState::Undefined;
        let mut schedule_check_result: bool = false;
        let mut ventilation_inhibited = false; // state of ventilation control determined by if start/stop command has been received

        loop {
            // run the temperature control only once per second
            if (loop_counter % 10 == 0) && (self.config.active) {
                // send request for temperature
                match tx_to_gpio.send(InternalCommand::RequestSignal(
                    AquariumSignal::WaterTemperature,
                )) {
                    Ok(_) => {
                        measurement_error = false;
                        // now listen for response
                        measured_temperature = match rx_from_gpio.recv() {
                            Ok(c) => c,
                            Err(e) => {
                                println!(
                                    "Ventilation: Error when reading channel content for water temperature: {e:?}"
                                );
                                measurement_error = true;
                                measured_temperature // keep the old value
                            }
                        };
                    }
                    Err(e) => {
                        measurement_error = true;
                        println!(
                            "Ventilation: Error in channel communication to GPIO handler \
                            when requesting water temperature: {e:?}"
                        );
                    }
                }

                // schedule check to see if actuation is allowed
                match tx_to_schedule_check.send(InternalCommand::ScheduleCheck) {
                    Ok(_) => {
                        // now listen for response
                        schedule_check_result = match rx_from_schedule_check.recv() {
                            Ok(c) => c,
                            Err(e) => {
                                println!(
                                    "Ventilation: Error when reading channel content for schedule check: {e:?}"
                                );
                                false // assume we are not allowed to actuate when communication is not successful
                            }
                        };
                    }
                    Err(e) => {
                        println!(
                            "Ventilation: Error in channel communication to schedule check: {e:?}"
                        );
                    }
                }

                if schedule_check_result && !ventilation_inhibited {
                    // actuation is allowed. proceed with control.
                    println!("Ventilation: control is active");
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
                            println!("Ventilation: Measured temperature is high (greater than upper bound of control window).");
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
                            println!("Ventilation: Measured temperature is low (smaller than lower bound of control window).");
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
                        println!("Ventilation: Control is inhibited by external request.");
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
                        println!("Ventilation: Control is inhibited by schedule check.");
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
                saw_tooth_profile.execute();
            }

            loop_counter += 1;

            spin_sleeper.sleep(sleep_duration_hundred_millis);

            let (quit_command_received, start_command_received, stop_command_received) =
                self.process_external_request(&rx_from_signal_handler, &rx_from_messaging);
            if quit_command_received {
                break;
            }
            if stop_command_received {
                println!("Ventilation: main loop: received Stop command. Inhibiting ventilation.");
                ventilation_inhibited = true;
            }
            if start_command_received {
                println!("Ventilation: main loop: received Start command. Restarting ventilation.");
                ventilation_inhibited = false;
            }
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
        println!("Ventilation: Sending Quit confirmation to signal handler.");
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                println!(
                    "Ventilation: Error in trying to send quit confirmation back to signal handler: {e:?}"
                );
            }
        };

        // this thread has channel connection to underlying threads (Controllino, GpioHandler).
        // These threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by signal_handler crate.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match rx_from_signal_handler.recv() {
                Ok(c) => match c {
                    InternalCommand::Terminate => true,
                    _ => {
                        println!("Ventilation: ignoring inapplicable command from signal handler.");
                        false
                    }
                },
                Err(e) => {
                    println!("Ventilation: error when trying to receive termination signal: {e:?}");
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
    use std::sync::mpsc;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::Duration;

    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::ventilation::Ventilation;
    use crate::channel_content::AquariumDevice;

    fn prepare_ventilation_tests(short_ramp: bool) -> Ventilation {
        // prepare struct Ventilation for subsequent tests
        let config: ConfigData;
        if short_ramp {
            config = read_config_file("/config/aquarium_control_test_ventilation_secondary.toml".to_string());
        }
        else {
            config = read_config_file("/config/aquarium_control_test_ventilation.toml".to_string());            
        }           
        Ventilation::new(config.ventilation)
    }

    // send via channel a value to the test object to indicate if actuation is permitted.
    pub fn mock_schedule_check(
        tx_schedule_check_to_ventilation: &Sender<bool>,
        rx_schedule_check_from_ventilation: &Receiver<InternalCommand>,
        value: bool,
    ) -> bool {
        match rx_schedule_check_from_ventilation.try_recv() {
            Ok(c) => match c {
                InternalCommand::ScheduleCheck => {
                    // allow control to actuate
                    let _ = tx_schedule_check_to_ventilation.send(value);
                }
                InternalCommand::Quit => {
                    // test case has finished
                    return true;
                }
                _ => {
                    panic!("Ventilation: test object sent invalid command.");
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        println!(
                            "Ventilation: Error in trying to receive command from test object: {e:?}"
                        );
                    }
                }
            }
        }
        false
    }

    pub fn run_main_cycle_with_stimuli(
        iterations: u32,
        rx_gpio_from_ventilation: &Receiver<InternalCommand>,
        tx_gpio_to_ventilation: &Sender<f32>,
        temperature_stimuli: f32,
    ) {
        for i in 0..iterations {
            println!(
                "run_main_cycle_with_stimuli: run_main_cycle_with_sensor_signals running loop iteration #{}",
                i
            );
            rx_gpio_from_ventilation.recv().unwrap();
            let _ = tx_gpio_to_ventilation.send(temperature_stimuli);
        }
    }

    pub fn test_receive_information_for_datalogger(
        rx_datalogger_from_control: &Receiver<bool>,
    ) -> bool {
        // receive information for data logger
        match rx_datalogger_from_control.recv() {
            Ok(c) => c,
            Err(e) => {
                panic!("test_receive_information_for_datalogger: error when receiving information for data logger: {e:?}");
            }
        }
    }

    pub fn test_receive_actuation_command_send_confirmation(
        rx_controllino_from_control: &Receiver<InternalCommand>,
        tx_controllino_to_control: &Sender<bool>,
    ) -> Option<InternalCommand> {
        let command_received: Option<InternalCommand>;

        // receive command from test object
        match rx_controllino_from_control.recv() {
            Ok(command) => {
                command_received = Some(command);
            }
            Err(e) => {
                panic!(
                    "test_receive_actuation_command_send_confirmation: Error when receiving actuation command from test object: {e:?}"
                );
            }
        }

        // send confirmation back
        match tx_controllino_to_control.send(true) {
            Ok(_) => {}
            Err(e) => {
                panic!("test_receive_actuation_command_send_confirmation: Error when sending back actuation command confirmation to test object: {e:?}");
            }
        }

        command_received
    }

    // Check if the ventilation is switched on when temperature is high.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_ventilation_with_measured_temperature_high() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let mut ventilation = prepare_ventilation_tests(false);

        // channels for the test object. test environment only uses one of the channels
        let (tx_ventilation_to_gpio, rx_gpio_from_ventilation) = mpsc::channel();
        let (tx_gpio_to_ventilation, rx_ventilation_from_gpio) = mpsc::channel();
        let (tx_ventilation_to_datalogger, rx_datalogger_from_ventilation) = mpsc::channel();
        let (tx_ventilation_to_controllino, rx_controllino_from_ventilation) = mpsc::channel();
        let (tx_controllino_to_ventilation, rx_ventilation_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_ventilation, rx_ventilation_from_signal_handler) = mpsc::channel();
        let (tx_ventilation_to_signal_handler, rx_signal_handler_from_ventilation) = mpsc::channel();
        let (tx_schedule_check_to_ventilation, rx_ventilation_from_schedule_check) = mpsc::channel();
        let (tx_ventilation_to_schedule_check, rx_schedule_check_from_ventilation) = mpsc::channel();
        let (_tx_ventilation_to_messaging, rx_ventilation_from_messaging) = mpsc::channel();

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_ventilation,
                    &rx_schedule_check_from_ventilation,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

            // thread for mock GPIO and mock signal handler
            scope.spawn(move || {
                run_main_cycle_with_stimuli(
                    10,
                    &rx_gpio_from_ventilation,
                    &tx_gpio_to_ventilation,
                    30.0, // hot temperature to force switching off heater
                );
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);
            });

            // thread for mock Controllino and mock data logger
            scope.spawn(move || {
                let command_received = test_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
                );
                assert_eq!(command_received, Some(InternalCommand::SwitchOn(AquariumDevice::Ventilation)));
                _ = test_receive_information_for_datalogger(&rx_datalogger_from_ventilation);
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    tx_ventilation_to_gpio,
                    rx_ventilation_from_gpio,
                    tx_ventilation_to_controllino,
                    rx_ventilation_from_controllino,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_datalogger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    rx_ventilation_from_messaging,
                );
                println!("execution of test object finished.");
                let _ =
                    tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            });
            println!("* [Ventilation] checking reaction to high temperature.");
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
        let (tx_ventilation_to_gpio, rx_gpio_from_ventilation) = mpsc::channel();
        let (tx_gpio_to_ventilation, rx_ventilation_from_gpio) = mpsc::channel();
        let (tx_ventilation_to_datalogger, rx_datalogger_from_ventilation) = mpsc::channel();
        let (tx_ventilation_to_controllino, rx_controllino_from_ventilation) = mpsc::channel();
        let (tx_controllino_to_ventilation, rx_ventilation_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_ventilation, rx_ventilation_from_signal_handler) = mpsc::channel();
        let (tx_ventilation_to_signal_handler, rx_signal_handler_from_ventilation) = mpsc::channel();
        let (tx_schedule_check_to_ventilation, rx_ventilation_from_schedule_check) = mpsc::channel();
        let (tx_ventilation_to_schedule_check, rx_schedule_check_from_ventilation) = mpsc::channel();
        let (_tx_ventilation_to_messaging, rx_ventilation_from_messaging) = mpsc::channel();

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_ventilation,
                    &rx_schedule_check_from_ventilation,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

            // thread for mock GPIO and mock signal handler
            scope.spawn(move || {
                run_main_cycle_with_stimuli(
                    10,
                    &rx_gpio_from_ventilation,
                    &tx_gpio_to_ventilation,
                    20.0, // hot temperature to force switching off heater
                );
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);
            });

            // thread for mock Controllino and mock data logger
            scope.spawn(move || {
                let command_received = test_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
                );
                assert_eq!(command_received, Some(InternalCommand::SwitchOff(AquariumDevice::Ventilation)));
                _ = test_receive_information_for_datalogger(&rx_datalogger_from_ventilation);
                
                // check if ventilation is switched back on when exiting the application
                let command_received = test_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
                );
                assert_eq!(command_received, Some(InternalCommand::SwitchOn(AquariumDevice::Ventilation)));
                _ = test_receive_information_for_datalogger(&rx_datalogger_from_ventilation);
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    tx_ventilation_to_gpio,
                    rx_ventilation_from_gpio,
                    tx_ventilation_to_controllino,
                    rx_ventilation_from_controllino,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_datalogger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    rx_ventilation_from_messaging,
                );
                println!("execution of test object finished.");
                let _ =
                    tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            });
            println!("* [Ventilation] checking reaction to low temperature.");
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
        let (tx_ventilation_to_gpio, rx_gpio_from_ventilation) = mpsc::channel();
        let (tx_gpio_to_ventilation, rx_ventilation_from_gpio) = mpsc::channel();
        let (tx_ventilation_to_datalogger, rx_datalogger_from_ventilation) = mpsc::channel();
        let (tx_ventilation_to_controllino, rx_controllino_from_ventilation) = mpsc::channel();
        let (tx_controllino_to_ventilation, rx_ventilation_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_ventilation, rx_ventilation_from_signal_handler) = mpsc::channel();
        let (tx_ventilation_to_signal_handler, rx_signal_handler_from_ventilation) = mpsc::channel();
        let (tx_schedule_check_to_ventilation, rx_ventilation_from_schedule_check) = mpsc::channel();
        let (tx_ventilation_to_schedule_check, rx_schedule_check_from_ventilation) = mpsc::channel();
        let (_tx_ventilation_to_messaging, rx_ventilation_from_messaging) = mpsc::channel();

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_ventilation,
                    &rx_schedule_check_from_ventilation,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

            // thread for mock GPIO and mock signal handler
            scope.spawn(move || {
                run_main_cycle_with_stimuli(
                    20,
                    &rx_gpio_from_ventilation,
                    &tx_gpio_to_ventilation,
                    26.5, // hot temperature to force switching off heater
                );
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);
            });

            // thread for mock Controllino and mock Datalogger
            scope.spawn(move || {
               for _ in 0..2 {
                    let command_received = test_receive_actuation_command_send_confirmation(
                            &rx_controllino_from_ventilation,
                            &tx_controllino_to_ventilation,
                        );
                    assert_eq!(command_received, Some(InternalCommand::SwitchOn(AquariumDevice::Ventilation)));
                    test_receive_information_for_datalogger(&rx_datalogger_from_ventilation);
                    let command_received = test_receive_actuation_command_send_confirmation(
                            &rx_controllino_from_ventilation,
                            &tx_controllino_to_ventilation,
                        );
                    assert_eq!(command_received, Some(InternalCommand::SwitchOff(AquariumDevice::Ventilation)));
                    test_receive_information_for_datalogger(&rx_datalogger_from_ventilation);
                }

                // check if ventilation is switched back on when exiting the application
                let command_received = test_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
                );
                assert_eq!(command_received, Some(InternalCommand::SwitchOn(AquariumDevice::Ventilation)));
                _ = test_receive_information_for_datalogger(&rx_datalogger_from_ventilation);
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    tx_ventilation_to_gpio,
                    rx_ventilation_from_gpio,
                    tx_ventilation_to_controllino,
                    rx_ventilation_from_controllino,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_datalogger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    rx_ventilation_from_messaging,
                );
                println!("execution of test object finished.");
                let _ =
                    tx_ventilation_to_schedule_check_for_test_case_finish.send(InternalCommand::Quit);
            });
            println!("* [Ventilation] checking reaction to low temperature.");
        });                                
    }

    // Check if ventilation control is only actuating when quitting while schedule check provides negative result
    #[test]
    pub fn test_ventilation_block_by_schedule() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        let mut ventilation = prepare_ventilation_tests(false);

        // channels for the test object. test environment only uses one of the channels
        let (tx_ventilation_to_gpio, rx_gpio_from_ventilation) = mpsc::channel();
        let (tx_gpio_to_ventilation, rx_ventilation_from_gpio) = mpsc::channel();
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
        let (_, rx_ventilation_from_messaging) = mpsc::channel();

        scope(|scope| {
            // thread for schedule check
            scope.spawn(move || loop {
                if mock_schedule_check(
                    &tx_schedule_check_to_ventilation,
                    &rx_schedule_check_from_ventilation,
                    true,
                ) {
                    break;
                }
                spin_sleeper.sleep(sleep_duration_100_millisec);
            });

            let mutex_device_scheduler_ventilation = Arc::new(Mutex::new(0));

            // thread for mock GPIO
            scope.spawn(move || {
                run_main_cycle_with_stimuli(
                    4,
                    &rx_gpio_from_ventilation,
                    &tx_gpio_to_ventilation,
                    26.5,
                );
                run_main_cycle_with_stimuli(
                    4,
                    &rx_gpio_from_ventilation,
                    &tx_gpio_to_ventilation,
                    30.0,
                );
                run_main_cycle_with_stimuli(
                    4,
                    &rx_gpio_from_ventilation,
                    &tx_gpio_to_ventilation,
                    0.0,
                );
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                rx_signal_handler_from_ventilation.recv().unwrap();
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Terminate);
            });

            // thread for mock Controllino
            scope.spawn(move || {
                _ = test_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
                );
                _ = test_receive_information_for_datalogger(&rx_datalogger_from_ventilation);
                _ = test_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
                );
                _ = test_receive_information_for_datalogger(&rx_datalogger_from_ventilation);
                _ = test_receive_actuation_command_send_confirmation(
                    &rx_controllino_from_ventilation,
                    &tx_controllino_to_ventilation,
                );
                _ = test_receive_information_for_datalogger(&rx_datalogger_from_ventilation);
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);

            // thread for test object
            scope.spawn(move || {
                let tx_ventilation_to_schedule_check_for_test_case_finish =
                    tx_ventilation_to_schedule_check.clone();

                ventilation.execute(
                    mutex_device_scheduler_ventilation.clone(),
                    tx_ventilation_to_gpio,
                    rx_ventilation_from_gpio,
                    tx_ventilation_to_controllino,
                    rx_ventilation_from_controllino,
                    tx_ventilation_to_signal_handler,
                    rx_ventilation_from_signal_handler,
                    tx_ventilation_to_datalogger,
                    tx_ventilation_to_schedule_check,
                    rx_ventilation_from_schedule_check,
                    rx_ventilation_from_messaging,
                );

                let _ = tx_ventilation_to_schedule_check_for_test_case_finish
                    .send(InternalCommand::Quit);

                let actuation_count = *mutex_device_scheduler_ventilation.lock().unwrap();

                // check if ventilation is only actuated once (when quitting)
                assert_eq!(actuation_count, 3);
            });
            println!("* [Ventilation] checking if schedule checker can block actuation.");
        });
    }
}
