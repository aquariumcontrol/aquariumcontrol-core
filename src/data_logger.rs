/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#![allow(non_snake_case)]
use chrono::Local;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;
use std::fs;
use chrono::NaiveDateTime;

use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;
use crate::iir_filter::IIRFilter;
use crate::iir_filter::IIRFilterConfig;
use crate::proc_ext_req::ProcessExternalRequestTrait;
use crate::sql_interface_data::DataFrame;
use crate::sql_interface_data::SqlInterfaceData;
use crate::sql_interface::SqlInterface;

/// Holds the configuration data for the regular data logging to SQL database.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct DataLoggerConfig {
    /// interval of the data logger in seconds
    logging_interval: u64,

    /// filter coefficient for the filtered version of the water temperature signal
    filter_coefficient_water_temperature: IIRFilterConfig<()>,

    /// filter coefficient for the filtered version of the pH signal
    filter_coefficient_pH: IIRFilterConfig<()>,

    /// filter coefficient for the filtered version of the conductivity signal
    filter_coefficient_conductivity: IIRFilterConfig<()>,

    /// switch if output to file (in ramdisk) shall be written
    output_to_disk: bool,

    /// output file name (in ramdisk) for current timestamp
    output_file_name_timestamp: String,

    /// output file name (in ramdisk) for current value of water temperature
    output_file_name_water_temperature: String,

    /// output file name (in ramdisk) for current value of water temperature filtered
    output_file_name_water_temperature_filtered: String,

    /// output file name (in ramdisk) for current value of pH
    output_file_name_pH: String,

    /// output file name (in ramdisk) for current value of pH filtered
    output_file_name_pH_filtered: String,

    /// output file name (in ramdisk) for current value of conductivity
    output_file_name_conductivity: String,

    /// output file name (in ramdisk) for current value of conductivity filtered
    output_file_name_conductivity_filtered: String,

    /// output file name (in ramdisk) for current value of tank level switch position
    output_file_name_tank_level_switch_position: String,

    /// output file name (in ramdisk) for current value of tank level switch invalid
    output_file_name_tank_level_switch_invalid: String,

    /// output file name (in ramdisk) for current value of ventilation
    output_file_name_ventilation_is_on: String,

    /// output file name (in ramdisk) for current value of heating
    output_file_name_heating_is_on: String,

    /// output file name (in ramdisk) for current value of ambient air temperature
    output_file_name_ambient_temperature: String,

    /// output file name (in ramdisk) for current value of air humidity
    output_file_name_humidity: String,

    /// output file name (in ramdisk) for current refill pump status
    output_file_name_refill_in_progress: String,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for regular data logging to SQL database.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     data_logger[Data logger] --> gpio_handler[GPIO handler]
///     gpio_handler --> data_logger
///     refill[Refill control] --> data_logger
///     heating[Heating control] --> data_logger
///     ventilation[Ventilation control] --> data_logger
///     data_logger --> signal_handler[Signal handler]
///     signal_handler[Signal handler] --> data_logger
/// ```

pub struct DataLogger {
    /// configuration data for the data logger
    config: DataLoggerConfig,

    /// filter for the water temperature signal
    filter_water_temperature: IIRFilter,

    /// filter for the pH signal
    filter_pH: IIRFilter,

    /// filter for the conductivity signal
    filter_conductivity: IIRFilter,
}

impl DataLogger {
    /// Returns a struct of type DataLogger
    /// # Arguments
    /// * `config` - configuration data for data logger
    pub fn new(config: DataLoggerConfig) -> DataLogger {
        let filter_water_temperature = IIRFilter::new_requires_initial(
            &config.filter_coefficient_water_temperature,
            0.0, // initialization value (mesured value not available at this time)
            1.0, // 1 second time step
        );
        let filter_pH = IIRFilter::new_requires_initial(
            &config.filter_coefficient_pH,
            0.0, // initialization value (mesured value not available at this time)
            1.0, // 1 second time step
        );
        let filter_conductivity = IIRFilter::new_requires_initial(
            &config.filter_coefficient_conductivity,
            0.0, // initialization value (mesured value not available at this time)
            1.0, // 1 second time step
        );

        if config.output_to_disk {
            if config.output_file_name_timestamp.is_empty()
                || config.output_file_name_water_temperature.is_empty()
                || config.output_file_name_water_temperature_filtered.is_empty()
                || config.output_file_name_pH.is_empty()
                || config.output_file_name_pH_filtered.is_empty()
                || config.output_file_name_conductivity.is_empty()
                || config.output_file_name_conductivity_filtered.is_empty()
                || config.output_file_name_tank_level_switch_position.is_empty()
                || config.output_file_name_tank_level_switch_invalid.is_empty()
                || config.output_file_name_ventilation_is_on.is_empty()
                || config.output_file_name_heating_is_on.is_empty()
                || config.output_file_name_ambient_temperature.is_empty()
                || config.output_file_name_humidity.is_empty()
                || config.output_file_name_refill_in_progress.is_empty()
            {
                panic!("DataLogger: one of the output file names is empty.");
            } else {
                fs::write(config.output_file_name_timestamp.clone(), "")
                    .expect("DataLogger: Could not reset content of output file for timestamp.");
                fs::write(config.output_file_name_water_temperature.clone(), "").expect(
                    "DataLogger: Could not reset content of output file for water temperature.",
                );
                fs::write(config.output_file_name_water_temperature_filtered.clone(), "").expect(
                    "DataLogger: Could not reset content of output file for filtered water temperature."
                );
                fs::write(config.output_file_name_pH.clone(), "")
                    .expect("DataLogger: Could not reset content of output file for pH value.");
                fs::write(config.output_file_name_pH_filtered.clone(), "").expect(
                    "DataLogger: Could not reset content of output file for filtered pH value.",
                );
                fs::write(config.output_file_name_conductivity.clone(), "")
                    .expect("DataLogger: Could not reset content of output file for conductivity.");
                fs::write(config.output_file_name_conductivity_filtered.clone(), "").expect(
                    "DataLogger: Could not reset content of output file for filtered conductivity.",
                );
                fs::write(config.output_file_name_tank_level_switch_position.clone(), "").expect(
                    "DataLogger: Could not reset content of output file for tank level switch position."
                );
                fs::write(config.output_file_name_tank_level_switch_invalid.clone(), "").expect(
                    "DataLogger: Could not reset content of output file for tank level switch invalid."
                );
                fs::write(config.output_file_name_ventilation_is_on.clone(), "").expect(
                    "DataLogger: Could not reset content of output file for ventilation status.",
                );
                fs::write(config.output_file_name_heating_is_on.clone(), "").expect(
                    "DataLogger: Could not reset content of output file for heating status.",
                );
                fs::write(config.output_file_name_ambient_temperature.clone(), "").expect(
                    "DataLogger: Could not reset content of output file for ambient air temperature."
                );
                fs::write(config.output_file_name_humidity.clone(), "")
                    .expect("DataLogger: Could not reset content of output file for humidity.");
                fs::write(config.output_file_name_refill_in_progress.clone(), "").expect(
                    "DataLogger: Could not reset content of output file for refill in progress.",
                );
            }
        }

        Self {
            config: config,
            filter_water_temperature: filter_water_temperature,
            filter_pH: filter_pH,
            filter_conductivity: filter_conductivity,
        }
    }

    /// Outputs non-binary signal to file (in RAM disk).
    /// # Arguments
    /// * `signal` - signal which shall be converted to human readable format and written to file
    /// * `output_file_name` - name of file where the value shall be written to    
    fn output_signal(&self, signal: f32, signal_type: AquariumSignal, output_file_name: &String) {
        if self.config.output_to_disk {
            let signal_string = match signal_type {
                AquariumSignal::WaterTemperature => format!("{:.1}", signal),
                AquariumSignal::pH => format!("{:.2}", signal),
                AquariumSignal::Conductivity => format!("{:.0}", signal),
                AquariumSignal::AmbientTemperature => format!("{:.1}", signal),
                AquariumSignal::AmbientHumidity => format!("{:.1}", signal),
                _ => signal.to_string(),
            };

            match fs::write(output_file_name, signal_string.clone()) {
                Ok(_) => { /* do nothing */ }
                Err(e) => {
                    println!(
                        "DataLogger: output of value {} to {} failed: {e:?}",
                        signal_string, output_file_name
                    );
                }
            }
        }
    }

    /// Outputs binary signal to file (in RAM disk).
    /// # Arguments
    /// * `device_state` - signal which shall be converted to human readable format and written to file
    /// * `state_description_true` - label to write to file for true value
    /// * `state_description_false` - label to write to file for false value
    /// * `output_file_name` - name of file where the value shall be written to    
    fn output_state(
        &self,
        state: bool,
        state_description_true: &str,
        state_description_false: &str,
        output_file_name: &String,
    ) {
        if self.config.output_to_disk {
            let state_string = match state {
                true => state_description_true,
                false => state_description_false,
            };

            match fs::write(output_file_name, state_string) {
                Ok(_) => { /* do nothing */ }
                Err(e) => {
                    println!(
                        "DataLogger: output of value {} to {} failed: {e:?}",
                        state_string, output_file_name
                    );
                }
            }
        }
    }

    /// Outputs string to file (in RAM disk).
    /// # Arguments
    /// * `data` - string data to be written to file
    /// * `output_file_name` - name of file where the value shall be written to    
    fn output_timestamp(
        &self,
        timestamp: NaiveDateTime,
        output_file_name: &String,
    ) {
        if self.config.output_to_disk {

            match fs::write(output_file_name, timestamp.to_string()) {
                Ok(_) => { /* do nothing */ }
                Err(e) => {
                    println!(
                        "DataLogger: output of value {} to {} failed: {e:?}",
                        timestamp, output_file_name
                    );
                }
            }
        }
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `mutex_device_scheduler_balling` - clone of the mutex for avoiding parallel device actuation.
    /// * `tx_to_gpio` - Sender part of channel for communication to Controllino
    /// * `rx_from_gpio` - Receiver part of channel for communication from Controllino
    /// * `rx_from_refill` - Receiver part of channel for communication from refill thread
    /// * `rx_from_heating` - Receiver part of channel for communication from heating thread
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    pub fn execute(
        &mut self,
        mut sql_interface_data: SqlInterfaceData,
        tx_to_gpio: Sender<InternalCommand>,
        rx_from_gpio: Receiver<f32>,
        rx_from_refill: Receiver<bool>,
        rx_from_heating: Receiver<bool>,
        rx_from_ventilation: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        let sleep_duration_one_millisec = Duration::from_millis(1);
        let mut i;
        let spin_sleeper = SpinSleeper::default();

        // define and initialize with value which will be overwritten in first iteration
        let mut water_temperature: Option<f32> = Some(0.0);
        let mut ph: Option<f32> = Some(0.0);
        let mut conductivity: Option<f32> = Some(0.0);
        let mut ambient_temperature: Option<f32> = Some(0.0);
        let mut ambient_humidity: Option<f32> = Some(0.0);
        let mut tank_level_switch_invalid: Option<bool> = Some(false);
        let mut tank_level_switch_position: Option<bool> = Some(false);
        let mut refill_in_progress: bool = false;
        let mut heater_is_on: bool = false;
        let mut ventilation_is_on: bool = false;
        let mut quit_command_received = false;

        loop {
            i = 0;
            while i < self.config.logging_interval * 1000 {
                i = i + 1;
                spin_sleeper.sleep(sleep_duration_one_millisec);
                (quit_command_received, _, _) =
                    self.process_external_request(&rx_from_signal_handler, None);
                if quit_command_received {
                    break;
                }
            }
            if quit_command_received {
                break;
            }

            match SqlInterface::get_current_timestamp(&mut sql_interface_data.conn) {
                Ok(c) => {
                    self.output_timestamp(c, &self.config.output_file_name_timestamp);
                },
                Err(e) => {
                    println!("DataLogger: Could not retrieve timestamp from database: {e:?}");
                }
            }
            
            // get information if refill is in progress from refill thread
            match rx_from_refill.try_recv() {
                Ok(c) => {
                    refill_in_progress = c;
                    self.output_state(c, "ON", "OFF", &self.config.output_file_name_refill_in_progress);
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("DataLogger: Error in trying to receive command from refill thread: {e:?}");
                        }
                    }
                }
            }

            // get information if heater is on from heating thread
            match rx_from_heating.try_recv() {
                Ok(c) => {
                    heater_is_on = c;
                    self.output_state(c, "ON", "OFF", &self.config.output_file_name_heating_is_on);
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("DataLogger: Error in trying to receive command from heating thread: {e:?}");
                        }
                    }
                }
            }

            // get information if ventilation is on from ventilation thread
            match rx_from_ventilation.try_recv() {
                Ok(c) => {
                    ventilation_is_on = c;
                    self.output_state(c, "ON", "OFF", &self.config.output_file_name_ventilation_is_on);
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("DataLogger: Error in trying to receive command from ventilation thread: {e:?}");
                        }
                    }
                }
            }

            // send request for water temperature
            match tx_to_gpio.send(InternalCommand::RequestSignal(
                AquariumSignal::WaterTemperature,
            )) {
                Ok(_) => {
                    // now listen for response
                    water_temperature = match rx_from_gpio.recv() {
                        Ok(c) => {
                            self.output_signal(
                                c,
                                AquariumSignal::WaterTemperature,
                                &self.config.output_file_name_water_temperature,
                            );
                            Some(c)
                        }
                        Err(_) => None,
                    };
                }
                Err(e) => {
                    println!("DataLogger: Error in channel communication to GPIO handler when requesting water temperature: {e:?}");
                }
            }

            // send request for pH
            match tx_to_gpio.send(InternalCommand::RequestSignal(AquariumSignal::pH)) {
                Ok(_) => {
                    // now listen for response
                    ph = match rx_from_gpio.recv() {
                        Ok(c) => {
                            self.output_signal(c, AquariumSignal::pH, &self.config.output_file_name_pH);
                            Some(c)
                        }
                        Err(_) => None,
                    };
                }
                Err(e) => {
                    println!("DataLogger: Error in channel communication to GPIO handler when requesting pH: {e:?}");
                }
            }

            // send request for conductivity
            match tx_to_gpio.send(InternalCommand::RequestSignal(AquariumSignal::Conductivity)) {
                Ok(_) => {
                    // now listen for response
                    conductivity = match rx_from_gpio.recv() {
                        Ok(c) => {
                            self.output_signal(
                                c,
                                AquariumSignal::Conductivity,
                                &self.config.output_file_name_conductivity,
                            );
                            Some(c)
                        }
                        Err(_) => None,
                    };
                }
                Err(e) => {
                    println!("DataLogger: Error in channel communication to GPIO handler when requesting conductivity: {e:?}");
                }
            }

            // send request for ambient temperature
            match tx_to_gpio.send(InternalCommand::RequestSignal(
                AquariumSignal::AmbientTemperature,
            )) {
                Ok(_) => {
                    // now listen for response
                    ambient_temperature = match rx_from_gpio.recv() {
                        Ok(c) => {
                            self.output_signal(
                                c,
                                AquariumSignal::AmbientTemperature,
                                &self.config.output_file_name_ambient_temperature,
                            );
                            Some(c)
                        }
                        Err(_) => None,
                    };
                }
                Err(e) => {
                    println!("DataLogger: Error in channel communication to GPIO handler when requesting ambient temperature: {e:?}");
                }
            }

            // send request for ambient humidity
            match tx_to_gpio.send(InternalCommand::RequestSignal(
                AquariumSignal::AmbientHumidity,
            )) {
                Ok(_) => {
                    // now listen for response
                    ambient_humidity = match rx_from_gpio.recv() {
                        Ok(c) => {
                            self.output_signal(
                                c,
                                AquariumSignal::AmbientHumidity,
                                &self.config.output_file_name_humidity,
                            );
                            Some(c)
                        }
                        Err(_) => None,
                    };
                }
                Err(e) => {
                    println!("DataLogger: Error in channel communication to GPIO handler when requesting ambient humidity: {e:?}");
                }
            }

            // send request for tank level switch position
            match tx_to_gpio.send(InternalCommand::RequestSignal(
                AquariumSignal::TankLevelSwitchPosition,
            )) {
                Ok(_) => {
                    // now listen for response
                    tank_level_switch_position = match rx_from_gpio.recv() {
                        Ok(c) => {
                            self.output_state(
                                c > 0.0,
                                "HIGH",
                                "LOW",
                                &self.config.output_file_name_tank_level_switch_position,
                            );
                            Some(c > 0.0)
                        }
                        Err(_) => None,
                    };
                }
                Err(e) => {
                    println!("DataLogger: Error in channel communication to GPIO handler when requesting tank level switch position: {e:?}");
                }
            }

            // send request for tank level switch invalid
            match tx_to_gpio.send(InternalCommand::RequestSignal(
                AquariumSignal::TankLevelSwitchInvalid,
            )) {
                Ok(_) => {
                    // now listen for response
                    tank_level_switch_invalid = match rx_from_gpio.recv() {
                        Ok(c) => {
                            self.output_state(
                                c > 0.0,
                                "INVALID",
                                "VALID",
                                &self.config.output_file_name_tank_level_switch_position,
                            );
                            Some(c > 0.0)
                        }
                        Err(_) => None,
                    };
                }
                Err(e) => {
                    println!("DataLogger: Error in channel communication to GPIO handler when requesting tank level switch invalid: {e:?}");
                }
            }

            // calculate filtered values and output them to (RAM disk) files
            let water_temperature_filtered = match water_temperature {
                Some(c) => {
                    let filtered_water_temperature =
                        self.filter_water_temperature.get_filtered_value(c);
                    self.output_signal(
                        filtered_water_temperature,
                        AquariumSignal::WaterTemperature,
                        &self.config.output_file_name_water_temperature_filtered,
                    );
                    Some(filtered_water_temperature)
                }
                None => None,
            };
            let pH_filtered = match ph {
                Some(c) => {
                    let filtered_pH = self.filter_pH.get_filtered_value(c);
                    self.output_signal(
                        filtered_pH,
                        AquariumSignal::WaterTemperature,
                        &self.config.output_file_name_pH_filtered,
                    );
                    Some(filtered_pH)
                }
                None => None,
            };
            let conductivity_filtered = match conductivity {
                Some(c) => {
                    let filtered_conductivity = self.filter_conductivity.get_filtered_value(c);
                    self.output_signal(
                        filtered_conductivity,
                        AquariumSignal::Conductivity,
                        &self.config.output_file_name_conductivity_filtered,
                    );
                    Some(filtered_conductivity)
                }
                None => None,
            };

            // all data received, let's construct a data frame
            let current_timestamp = Local::now().naive_local();
            let data_frame = DataFrame {
                timestamp: current_timestamp,
                water_temperature: water_temperature,
                water_temperature_filtered: water_temperature_filtered,
                ph: ph,
                ph_filtered: pH_filtered,
                conductivity: conductivity,
                conductivity_filtered: conductivity_filtered,
                conductivity_compensated: None,
                refill_in_progress: Some(refill_in_progress),
                tank_level_switch_position: tank_level_switch_position,
                tank_level_switch_invalid: tank_level_switch_invalid,
                surface_ventilation_status: Some(ventilation_is_on),
                ambient_temperature: ambient_temperature,
                ambient_humidity: ambient_humidity,
                heater_status: Some(heater_is_on),
            };
            println!("DateFrame = {}", data_frame);
            match sql_interface_data.write_data_frame_to_database(&data_frame) {
                Ok(()) => {}
                Err(e) => {
                    println!("DataLogger: error occurred when writing to SQL database: {e:?}")
                }
            };
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                println!("DataLogger: Error in trying to send confirmation back to signal handler: {e:?}");
            }
        };

        // this thread has channel connection to underlying threads (GpioHandler).
        // These threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by signal_handler crate.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match rx_from_signal_handler.recv() {
                Ok(c) => match c {
                    InternalCommand::Terminate => true,
                    _ => {
                        println!(
                            "DataLogger: ignoring not applicable command from signal handler."
                        );
                        false
                    }
                },
                Err(e) => {
                    println!("DataLogger: error when trying to receive termination signal: {e:?}");
                    false
                }
            };
            spin_sleeper.sleep(sleep_duration_one_millisec);
        }
    }
}
