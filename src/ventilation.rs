/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::channel_content::ActuatorState;
use crate::channel_content::AquariumDevice;
use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;
use crate::sawtooth_profile::SawToothProfile;
use crate::sawtooth_profile::SawToothProfileConfig;

/// This struct holds the configuration data for the ventilation control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct VentilationConfig {
    active: bool,
    switch_on_temperature: f32,
    switch_off_temperature: f32,
    switch_on_ventilation_when_terminating: bool,
    saw_tooth_profile_config: SawToothProfileConfig,
}

/// function checks if there are new messages in the channel.
/// if the message contains the quit command, then the function returns true, otherwise false.
fn check_pending_external_request(rx_from_signal_handler: &Receiver<InternalCommand>) -> bool {
    match rx_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Quit => true,
            _ => {
                println!("Ventilation: ignoring inapplicable command from signal handler.");
                false
            }
        },
        Err(e) => {
            match e {
                TryRecvError::Empty => { /* empty buffer - no action required */ }
                TryRecvError::Disconnected => {
                    println!(
                        "Ventilation: Error in trying to receive command from signal handler: {e:?}"
                    );
                }
            }
            false
        }
    }
}

/// function communicates to Controllino thread via channel to switch on the ventilation
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

/// function communicates to Controllino thread via channel to switch off the ventilation
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

pub struct Ventilation {
    config: VentilationConfig,
}

impl Ventilation {
    pub fn new(config: VentilationConfig) -> Ventilation {
        Self { config: config }
    }

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
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut saw_tooth_profile = SawToothProfile::new(&self.config.saw_tooth_profile_config);
        let mut measured_temperature = 0.0;
        let mut measurement_error: bool;
        let mut loop_counter = 0;
        let mut ventilation_state: ActuatorState = ActuatorState::Undefined;

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
                            when requesting tank level switch invalid: {e:?}"
                        );
                    }
                }

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
                    // switch off ventilation if no already off
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

                saw_tooth_profile.execute();
            }

            loop_counter += 1;

            spin_sleeper.sleep(sleep_duration_hundred_millis);

            if check_pending_external_request(&rx_from_signal_handler) {
                break;
            }
        }

        if self.config.switch_on_ventilation_when_terminating {
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

    // Check if the ventilation remains switched off during two cycles of the saw tooth profile.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_ventilation_with_measured_temperature_high() {}

    // Check if the ventilation is switched on and is not switched off during two cycles of the saw tooth profile.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Check if the ventilation is switched off after sending Quit command and before sending Terminate command.
    #[test]
    pub fn test_ventilation_with_measured_temperature_low() {}

    // Check if the ventilation is switched on and off during two cycles of the saw tooth profile.
    // Duration of ventilation switched on shall equal duration of ventilation switched off (1% tolerance).
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_ventilation_with_measured_temperature_50_percent() {}

    // Check if ventilation control is not acting when disabled (active=false in configuration file)
    #[test]
    pub fn test_ventilation_not_active() {}
}
