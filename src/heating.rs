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

/// Trait for the execution of data transfer to SQL database after midnight has arrived.
/// This trait allows running the main control with a mock implementation for testing.
pub trait HeatingStatsDataTransferTrait {
    fn transfer_heating_stats(
        &self,
        data: HeatingStatsEntry,
        sql_interface_heating: &mut SqlInterfaceHeating,
    );
}

pub struct HeatingStatsDataTransfer;

impl HeatingStatsDataTransferTrait for HeatingStatsDataTransfer {
    fn transfer_heating_stats(
        &self,
        heating_stats_entry: HeatingStatsEntry,
        sql_interface_heating: &mut SqlInterfaceHeating,
    ) {
        println!("Heating: Transferring statistical data to SQL database.");
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
    /// regardless of measured temperature in order to avoid overheating
    /// of the fish tank or the heater itself.
    /// Background: If water level is too low, temperature sensor may be exposed to ambient air in stead of water.
    /// This condition results in wrong temperature reading and thus in wrong heater actuation.
    /// Furthermore, the exposing the heater to air instead of water and switching it on
    /// will not allow the heater to dissipate the heat and it therefore might suffer damage.
    stop_on_refill_error: bool,

    /// Number of seconds which need to pass with low water level before the heater is switched off
    stop_on_refill_error_delay: u32,

    /// Configuration of saw tooth profile for generation of low-frequency PWM signal
    saw_tooth_profile_config: SawToothProfileConfig,

    /// Electrical power consumption of heater in Watt
    heater_power: f32,
}

/// Checks if there are new messages in the channel.
/// If the message contains the quit command, then the function returns true, otherwise false.
/// # Arguments
/// * `rx_from_signal_handler` - receiver part of channel for communication from signal handler
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

/// Communicates to Controllino thread via channel to switch on the heater.
/// # Arguments
/// * `mutex_device_scheduler_heating` - mutex to avoid parallel actuation of device
/// * `tx_to_controllino` - sender part of channel for communication to Controllino
/// * `rx_from_controllino` - receiver part of channel for communication from Controllino
/// * `tx_to_datalogger` - sender part of channel for communication to data logger
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

/// Communicates to Controllino thread via channel to switch off the heater.
/// # Arguments
/// * `mutex_device_scheduler_heating` - mutex to avoid parallel actuation of device
/// * `tx_to_controllino` - sender part of channel for communication to Controllino
/// * `rx_from_controllino` - receiver part of channel for communication from Controllino
/// * `tx_to_datalogger` - sender part of channel for communication to data logger
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

/// Contains the configuration and the implementation for the refill control.
pub struct Heating {
    /// configuration data for heating control
    config: HeatingConfig,

    /// SQL interface for heating control
    sql_interface_heating: SqlInterfaceHeating,
}

impl Heating {
    /// Provides a struct of type Heating.
    /// # Arguments
    /// * `config` - configuration data for heating control
    /// * `sql_interface_heating` - specific SQL interface for heating control           
    pub fn new(config: HeatingConfig, sql_interface_heating: SqlInterfaceHeating) -> Heating {
        Self {
            config: config,
            sql_interface_heating: sql_interface_heating,
        }
    }

    /// Calculates a normalized controller deviation between measured value and target value in the range from zero to one.
    /// # Arguments
    /// * `measured_value` - measured temperature value    
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

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `mutex_device_scheduler_heating` - clone of the mutex for avoiding parallel device actuation.
    /// * `tx_to_gpio` - Sender part of channel for communication to GPIO handler
    /// * `rx_from_gpio` - Receiver part of channel for communication from GPIO handler
    /// * `tx_to_controllino` - Sender part of channel for communication to Controllino
    /// * `rx_from_controllino` - Receiver part of channel for communication from Controllino
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    /// * `tx_to_datalogger` - Sender part of channel for communication to data logger
    /// * `sql_interface` - Reference to generic SQL interface
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
        heating_stats_transfer: &mut impl HeatingStatsDataTransferTrait,
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let mut refill_error = false;
        let spin_sleeper = SpinSleeper::default();
        let mut saw_tooth_profile = SawToothProfile::new(&self.config.saw_tooth_profile_config);
        let mut measured_water_temperature = 0.0;
        let mut measurement_error_water_temperature: bool;
        let mut measured_ambient_temperature = 0.0;
        let mut measurement_error_ambient_temperature: bool;
        let mut loop_counter = 0;
        let mut heater_state: ActuatorState = ActuatorState::Undefined;

        let mut duration_until_midnight =
            match self.sql_interface_heating.get_duration_until_midnight() {
                Ok(c) => c,
                Err(e) => {
                    println!("Heating: Could not get duration until midnight: {e:?}");
                    100000 // default value for time in seconds until midnight
                }
            };
        println!(
            "Heating: Duration until midnight is {} seconds.",
            duration_until_midnight
        );
        let mut heating_stats_opt = match self
            .sql_interface_heating
            .get_single_heating_stats_from_database()
        {
            Ok(c) => Some(c),
            Err(e) => {
                println!("Heating: Could not get heating stats from database:{e:?}");
                None
            }
        };
        match heating_stats_opt {
            Some(ref heating_stats) => {
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
            }
            None => {
                // do nothing
            }
        }

        loop {
            if check_pending_external_request(&rx_from_signal_handler) {
                break;
            }

            // run the temperature control only once per second
            if (loop_counter % 10 == 0) && (self.config.active) {
                // send request for water temperature
                match tx_to_gpio.send(InternalCommand::RequestSignal(
                    AquariumSignal::WaterTemperature,
                )) {
                    Ok(_) => {
                        measurement_error_water_temperature = false;
                        // now listen for response
                        measured_water_temperature = match rx_from_gpio.recv() {
                            Ok(c) => c,
                            Err(e) => {
                                println!(
                                    "Heating: Error when reading channel content for water temperature: {e:?}"
                                );
                                measurement_error_water_temperature = true;
                                measured_water_temperature // keep the old value
                            }
                        };
                    }
                    Err(e) => {
                        measurement_error_water_temperature = true;
                        println!(
                            "Heating: Error in channel communication to GPIO handler \
                            when requesting water temperature: {e:?}"
                        );
                    }
                }

                // send request for ambient temperature
                match tx_to_gpio.send(InternalCommand::RequestSignal(
                    AquariumSignal::AmbientTemperature,
                )) {
                    Ok(_) => {
                        measurement_error_ambient_temperature = false;
                        // now listen for response
                        measured_ambient_temperature = match rx_from_gpio.recv() {
                            Ok(c) => c,
                            Err(e) => {
                                println!(
                                    "Heating: Error when reading channel content for ambient temperature: {e:?}"
                                );
                                measurement_error_ambient_temperature = true;
                                measured_ambient_temperature // keep the old value
                            }
                        };
                    }
                    Err(e) => {
                        measurement_error_ambient_temperature = true;
                        println!(
                            "Heating: Error in channel communication to GPIO handler \
                            when requesting ambient temperature: {e:?}"
                        );
                    }
                }

                // main control happens here (comparison measured/target), actuation
                if (measured_water_temperature >= self.config.switch_on_temperature)
                    && (measured_water_temperature <= self.config.switch_off_temperature)
                {
                    if (self.calc_normalized_control_deviation(measured_water_temperature)
                        < saw_tooth_profile.level_normalized)
                        && !measurement_error_water_temperature
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
                } else {
                    if measured_water_temperature > self.config.switch_off_temperature {
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
                    if measured_water_temperature < self.config.switch_on_temperature {
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
                    }
                }

                saw_tooth_profile.execute();

                // one second closer to midnight
                duration_until_midnight -= 1;
                match heating_stats_opt {
                    Some(ref mut heating_stats) => {
                        // wrap temperatures depending on the status bits
                        let water_temperature_opt = match measurement_error_water_temperature {
                            true => None,
                            false => Some(measured_water_temperature as f64),
                        };
                        let ambient_temperature_opt = match measurement_error_ambient_temperature {
                            true => None,
                            false => Some(measured_ambient_temperature as f64),
                        };
                        // update the heating statistics with recently aquired data
                        heating_stats.update(
                            water_temperature_opt,
                            ambient_temperature_opt,
                            (self.config.heater_power / 3600.0) as f64,
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
                }
            }

            loop_counter += 1;

            spin_sleeper.sleep(sleep_duration_hundred_millis);
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
    use chrono::DateTime;
    use chrono::Local;
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::channel_content::AquariumDevice;
    use crate::channel_content::AquariumSignal;
    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::heating::Heating;
    use crate::heating::HeatingConfig;
    use crate::heating::HeatingStatsDataTransferTrait;
    use crate::heating::HeatingStatsEntry;
    use crate::sql_interface::SqlInterface;
    use crate::sql_interface_heating::SqlInterfaceHeating;

    pub struct HeatingStatsMockDataTransfer;

    impl HeatingStatsDataTransferTrait for HeatingStatsMockDataTransfer {
        fn transfer_heating_stats(
            &self,
            heating_stats_entry: HeatingStatsEntry,
            sql_interface_heating: &mut SqlInterfaceHeating,
        ) {
            println!("Heating: Transferring statistical mock data to SQL database.");
        }
    }

    fn prepare_heating_tests() -> (ConfigData, Heating, SqlInterface) {
        // prepare struct Heating for subsequent tests
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_heating.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_heating = SqlInterfaceHeating::new(sql_interface.get_connection());
        let heating_config = config.heating.clone();
        (
            config,
            Heating::new(heating_config, sql_interface_heating),
            sql_interface,
        )
    }

    fn test_execute_heating_receive_actuation_command_send_confirmation(
        rx_controllino_from_heating: &Receiver<InternalCommand>,
        tx_controllino_to_heating: &Sender<bool>,
    ) -> Option<InternalCommand> {
        let mut command_received = None;
        // receive command from test object
        match rx_controllino_from_heating.recv() {
            Ok(command) => {
                command_received = Some(command);
            }
            Err(e) => {
                panic!("Heating: Error when receiving actuation command from test object: {e:?}");
            }
        }

        // send confirmation back
        match tx_controllino_to_heating.send(true) {
            Ok(_) => {}
            Err(e) => {
                panic!("Heating: Error when sending back actuation command confirmation to test object: {e:?}");
            }
        }

        command_received
    }

    fn test_execute_heating_receive_information_for_datalogger(
        rx_datalogger_from_heating: &Receiver<bool>,
    ) {
        // receive information for data logger
        match rx_datalogger_from_heating.recv() {
            Ok(_) => {}
            Err(e) => {
                panic!("Heating: error when receiving information for data logger: {e:?}");
            }
        }
    }

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

    // checks if the Quit command is correctly processed
    // - application shall switch off heater before exiting
    #[test]
    pub fn test_heating_execute_with_pending_quit_terminate_command() {
        let sleep_duration_100_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        // channels for the test object. test environment only uses one of the channels
        let (_, mut heating, mut sql_interface) = prepare_heating_tests();
        let (tx_heating_to_gpio, _) = mpsc::channel();
        let (_, rx_heating_from_gpio) = mpsc::channel();
        let (tx_heating_to_datalogger, rx_datalogger_from_heating) = mpsc::channel();
        let (tx_heating_to_controllino, rx_controllino_from_heating) = mpsc::channel();
        let (tx_controllino_to_heating, rx_heating_from_controllino) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating) = mpsc::channel();

        scope(|scope| {
            let mutex_device_scheduler_heating = Arc::new(Mutex::new(0));

            let start_time = SystemTime::now();
            scope.spawn(move || {
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                let command_received_opt =
                    test_execute_heating_receive_actuation_command_send_confirmation(
                        &rx_controllino_from_heating,
                        &tx_controllino_to_heating,
                    );
                test_execute_heating_receive_information_for_datalogger(
                    &rx_datalogger_from_heating,
                );
                rx_signal_handler_from_heating.recv().unwrap();
                //spin_sleeper.sleep(sleep_duration_100_millisec);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);

                // check if test object sent the right command
                match command_received_opt {
                    Some(command_received) => {
                        match command_received {
                            InternalCommand::SwitchOff(AquariumDevice::Heater) => { /* Do nothing */
                            }
                            _ => {
                                panic!(
                                    "Heating: Did not receive SwitchOff command from test object."
                                );
                            }
                        }
                    }
                    _ => {
                        panic!("Heating: Did not receive any command from test object.");
                    }
                }
            });
            spin_sleeper.sleep(sleep_duration_100_millisec);
            scope.spawn(move || {
                let mut heating_stats_transfer = HeatingStatsMockDataTransfer;
                heating.execute(
                    mutex_device_scheduler_heating.clone(),
                    tx_heating_to_gpio,
                    rx_heating_from_gpio,
                    tx_heating_to_controllino,
                    rx_heating_from_controllino,
                    tx_heating_to_signal_handler,
                    rx_heating_from_signal_handler,
                    tx_heating_to_datalogger,
                    &mut sql_interface,
                    &mut heating_stats_transfer,
                );
                // check if heater actuation has taken place
                let mutex_data = mutex_device_scheduler_heating.lock().unwrap();
                assert_eq!(*mutex_data, 1);
            });
            let finish_time = SystemTime::now();
            let execution_duration = finish_time.duration_since(start_time).unwrap();
            all_asserts::assert_le!(execution_duration.as_millis(), 500);
            println!(
                "* [Heating] checking heating execution with pending quit/terminate command (time limit) succeeded."
            );
        });
    }
}
