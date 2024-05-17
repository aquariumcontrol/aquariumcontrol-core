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
use crate::sql_interface_heating::SqlInterfaceHeating;

/// This struct holds the configuration data for the heating control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct HeatingConfig {
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
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let mut refill_error = false;
        let spin_sleeper = SpinSleeper::default();
        let mut saw_tooth_profile = SawToothProfile::new(&self.config.saw_tooth_profile_config);
        let mut measured_temperature = 0.0;
        let mut measurement_error: bool = false;
        let mut loop_counter = 0;
        let mut heater_state: ActuatorState = ActuatorState::Undefined;

        loop {
            // run the temperature control only once per second
            if loop_counter % 10 == 0 {
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
                        match tx_to_controllino
                            .send(InternalCommand::SwitchOn(AquariumDevice::Heater))
                        {
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
                        println!(
                            "Heating: Sent request to switch on heater to Controllino thread."
                        );
                        heater_state = ActuatorState::On;
                    }
                } else {
                    // switch off heater
                    if heater_state != ActuatorState::Off {
                        match tx_to_controllino
                            .send(InternalCommand::SwitchOff(AquariumDevice::Heater))
                        {
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
                        println!(
                            "Heating: Sent request to switch off heater to Controllino thread."
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
        }
    }
}
