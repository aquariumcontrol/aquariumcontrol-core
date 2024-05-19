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
use crate::sql_interface::SqlInterface;
use crate::sql_interface_heating::{HeatingStatsEntry, SqlInterfaceHeating};

/// This struct holds the configuration data for the heating control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct HeatingConfig {
    active: bool,
    switch_on_temperature: f32,
    switch_off_temperature: f32,
    stop_on_refill_error: bool,
    stop_on_refill_error_delay: u32,
    saw_tooth_profile_config: SawToothProfileConfig,
}

/// function checks if there are new messages in the channel.
/// if the message contains the quit command, then the function returns true, otherwise false.
fn check_pending_external_request(rx_from_signal_handler: &Receiver<InternalCommand>) -> bool {
    match rx_from_signal_handler.try_recv() {
        Ok(c) => match c {
            InternalCommand::Quit => true,
            _ => {
                println!("Heating: ignoring inapplicable command from signal handler.");
                false
            }
        },
        Err(e) => {
            match e {
                TryRecvError::Empty => { /* empty buffer - no action required */ }
                TryRecvError::Disconnected => {
                    println!(
                        "Heating: Error in trying to receive command from signal handler: {e:?}"
                    );
                }
            }
            false
        }
    }
}

/// function communicates to Controllino thread via channel to switch on the heater
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
                        println!("Heating: Error when trying to receive answer from Controllino when switching on heater: {e:?}");
                    }
                };
            }
            Err(e) => {
                println!("Heating: Error in channel communication to Controllino when switching on heater: {e:?}");
            }
        }
        match tx_to_datalogger.send(true) {
            Ok(_) => {}
            Err(e) => {
                println!("Heating: Error in channel communication to data logger when switching on heater: {e:?}");
            }
        }
        println!("Heating: Switched on heater.");

        *mutex_data += 1;
    }
}

/// function communicates to Controllino thread via channel to switch off the heater
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
                        println!("Heating: Error when trying to receive answer from Controllino when switching off heater: {e:?}");
                    }
                };
            }
            Err(e) => {
                println!("Heating: Error in channel communication to Controllino when switching off heater: {e:?}");
            }
        }
        match tx_to_datalogger.send(false) {
            Ok(_) => {}
            Err(e) => {
                println!("Heating: Error in channel communication to data logger when switching off heater: {e:?}");
            }
        }
        println!("Heating: Switched off heater.");
        *mutex_data += 1;
    }
}

/// This struct contains the configuration and the implementation for the refill control.
pub struct Heating {
    config: HeatingConfig,
    sql_interface_heating: SqlInterfaceHeating,
}

impl Heating {
    pub fn new(config: HeatingConfig, sql_interface_heating: SqlInterfaceHeating) -> Heating {
        Self {
            config: config,
            sql_interface_heating: sql_interface_heating,
        }
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
        mutex_device_scheduler_heating: Arc<Mutex<i32>>,
        tx_to_gpio: Sender<InternalCommand>,
        rx_from_gpio: Receiver<f32>,
        tx_to_controllino: Sender<InternalCommand>,
        rx_from_controllino: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
        tx_to_datalogger: Sender<bool>,
        sql_interface: &mut SqlInterface,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let mut refill_error = false;
        let spin_sleeper = SpinSleeper::default();
        let mut saw_tooth_profile = SawToothProfile::new(&self.config.saw_tooth_profile_config);
        let mut measured_temperature = 0.0;
        let mut measurement_error: bool;
        let mut loop_counter = 0;
        let mut heater_state: ActuatorState = ActuatorState::Undefined;
        
        let duration_until_midnight = match self.sql_interface_heating.get_duration_until_midnight() {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get duration until midnight: {e:?}");
            }
        };
        println!(
            "Heating: Duration until midnight is {} seconds.",
            duration_until_midnight
        );
        let heating_stats = match self.sql_interface_heating.get_single_heating_stats_from_database() {
            Ok(c) => c,
            Err(e) => panic!("Could not get heating stats from database:{e:?}"),
        };
        println!(
            "Heating stats entry: Date={}, Energy={}, Ambient Average={}, Ambient Counter={}, \
            Water Average={}, Water Counter={}, Control         time = {}",
            heating_stats.date.to_string(),
            heating_stats.energy,
            heating_stats.ambient_temperature_average_value,
            heating_stats.ambient_temperature_average_counter,
            heating_stats.water_temperature_average_value,
            heating_stats.water_temperature_average_counter,
            heating_stats.heating_control_runtime
        );
                
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
                                    "Heating: Error when reading channel content for water temperature: {e:?}"
                                );
                                measurement_error = true;
                                measured_temperature // keep the old value
                            }
                        };
                    }
                    Err(e) => {
                        measurement_error = true;
                        println!(
                            "Heating: Error in channel communication to GPIO handler \
                            when requesting tank level switch invalid: {e:?}"
                        );
                    }
                }

                if (self.calc_normalized_control_deviation(measured_temperature)
                    < saw_tooth_profile.level_normalized)
                    && !measurement_error
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
                    // switch off heater if no already off
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

                saw_tooth_profile.execute();
            }

            loop_counter += 1;

            spin_sleeper.sleep(sleep_duration_hundred_millis);

            if check_pending_external_request(&rx_from_signal_handler) {
                break;
            }
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

        // @todo: this code needs to move to main control along with the
        // two subsequent sections.
        let today = match sql_interface.get_current_date() {
            Ok(c) => c,
            Err(_) => panic!("Could not get current date."),
        };

        // @todo: this is mock data for subsequent call to SQL.
        // This numbers need to be replaced with data aquired during operation.
        let heating_stats_entry = HeatingStatsEntry {
            date: today.into(),
            energy: 0.9,
            ambient_temperature_average_value: 22.0,
            ambient_temperature_average_counter: 72000,
            water_temperature_average_value: 25.0,
            water_temperature_average_counter: 78000,
            heating_control_runtime: 64000,
        };

        // @todo: temporarily, this call is placed here to avoid warnings.
        // with implementation of midnight timing and aquisition of data,
        // this call needs to move to the central control cycle.
        match self
            .sql_interface_heating
            .insert_heating_stats_entry(&heating_stats_entry)
        {
            Ok(_) => {}
            Err(e) => panic!("Could not insert heating statistics into database: {e:?}"),
        };

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        println!("Heating: Sending Quit confirmation to signal handler.");
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                println!(
                    "Heating: Error in trying to send quit confirmation back to signal handler: {e:?}"
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
                        println!("Heating: ignoring inapplicable command from signal handler.");
                        false
                    }
                },
                Err(e) => {
                    println!("Heating: error when trying to receive termination signal: {e:?}");
                    false
                }
            };
            spin_sleeper.sleep(sleep_duration_hundred_millis);
        }
    }
}

#[cfg(test)]
pub mod tests {

    // Check if the heater remains switched off during two cycles of the saw tooth profile.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_heating_with_measured_temperature_high() {}

    // Check if the heater is switched on and is not switched off during two cycles of the saw tooth profile.
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    // Check if the heater is switched off after sending Quit command and before sending Terminate command.
    #[test]
    pub fn test_heating_with_measured_temperature_low() {}

    // Check if the heater is switched on and off during two cycles of the saw tooth profile.
    // Duration of heater switched on shall equal duration of heater switched off (1% tolerance).
    // Also, implicitly test if the execute function terminates after receiving Quit and Terminate commands.
    #[test]
    pub fn test_heating_with_measured_temperature_50_percent() {}

    // Check if heating control is not acting when disabled (active=false in configuration file)
    #[test]
    pub fn test_heating_not_active() {}
}
