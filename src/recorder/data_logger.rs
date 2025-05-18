/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#![allow(non_snake_case)]
use chrono::{Local, NaiveDateTime};
use log::{error, warn};

#[cfg(feature = "debug_data_logger")]
use log::debug;

use mysql::PooledConn;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::fs;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_data::SqlInterfaceData;
use crate::recorder::data_injection::DataInjectionTrait;
use crate::recorder::recorder_data_frame::RecorderDataFrame;
use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
use crate::utilities::iir_filter::{IIRFilter, IIRFilterConfig};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

const LOG_TARGET_DATA_LOGGER: &str = "data_logger";
const MILLIS_PER_SEC: u64 = 1000;

/// Holds the configuration data for the regular data logging to SQL database.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct DataLoggerConfig {
    /// switch indicating if a self-generated timestamp or the database-generated timestamp shall be used
    use_sql_timestamp: bool,

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

    /// output file name (in ramdisk) for the current timestamp
    output_file_name_timestamp: String,

    /// output file name (in ramdisk) for current value of water temperature
    output_file_name_water_temperature: String,

    /// output file name (in ramdisk) for current value of water temperature filtered
    output_file_name_water_temperature_filtered: String,

    /// output file name (in ramdisk) for the current value of pH
    output_file_name_pH: String,

    /// output file name (in ramdisk) for the current value of pH filtered
    output_file_name_pH_filtered: String,

    /// output file name (in ramdisk) for the current value of conductivity
    output_file_name_conductivity: String,

    /// output file name (in ramdisk) for the current value of conductivity filtered
    output_file_name_conductivity_filtered: String,

    /// output file name (in ramdisk) for the current value of tank level switch position
    output_file_name_tank_level_switch_position: String,

    /// output file name (in ramdisk) for the current value of tank level switch invalid
    output_file_name_tank_level_switch_invalid: String,

    /// output file name (in ramdisk) for the current value of tank level switch position stabilized
    output_file_name_tank_level_switch_position_stabilized: String,

    /// output file name (in ramdisk) for the current value of ventilation
    output_file_name_ventilation_is_on: String,

    /// output file name (in ramdisk) for the current value of heating
    output_file_name_heating_is_on: String,

    /// output file name (in ramdisk) for the current value of ambient air temperature
    output_file_name_ambient_temperature: String,

    /// output file name (in ramdisk) for the current value of air humidity
    output_file_name_humidity: String,

    /// output file name (in ramdisk) for the current refill pump status
    output_file_name_refill_in_progress: String,

    /// output file name (in ramdisk) for the water temperature measured by DS18B20 sensor
    output_file_name_water_temperature_ds18b20: String,

    /// output file name (in ramdisk) for the ambient temperature measured by DS18B20 sensor
    output_file_name_ambient_temperature_ds18b20: String,
}

pub struct DataLoggerChannels<'a> {
    /// Sender part of the channel for communication to Refill
    pub tx_to_refill: &'a Sender<InternalCommand>,
    
    /// Receiver part of the channel for communication from Refill
    pub rx_from_refill: &'a Receiver<bool>,

    /// Sender part of the channel for communication to Heating
    pub tx_to_heating: &'a Sender<InternalCommand>,

    /// Receiver part of the channel for communication from Heating
    pub rx_from_heating: &'a Receiver<bool>,

    /// Sender part of the channel for communication to Ventilation
    pub tx_to_ventilation: &'a Sender<InternalCommand>,
    
    /// Receiver part of the channel for communication from Ventilation
    pub rx_from_ventilation: &'a Receiver<bool>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_to_signal_handler: &'a Sender<bool>,
    
    /// Receiver part of the channel for communication from the signal handler                
    pub rx_from_signal_handler: &'a Receiver<InternalCommand>,
}

pub struct DataLoggerMutexes {
    /// Mutex for water temperature from DS18B20 sensor
    pub mutex_water_temperature_ds18b20: Arc<Mutex<f32>>,
    
    /// Mutex for ambient temperature from DS18B20 sensor
    pub mutex_ambient_temperature_dsb18b20: Arc<Mutex<f32>>,
    
    /// Mutex for temperature from Atlas Scientific
    pub mutex_atlas_scientific_temperature: Arc<Mutex<f32>>,
    
    /// Mutex for pH from Atlas Scientific
    pub mutex_atlas_scientific_ph: Arc<Mutex<f32>>,
    
    /// Mutex for conductivity DS18B20 sensor
    pub mutex_atlas_scientific_conductivity: Arc<Mutex<f32>>,
    
    /// Mutex for ambient temperature
    pub mutex_ambient_temperature: Arc<Mutex<f32>>,
    
    /// Mutex for ambient humidity
    pub mutex_ambient_humidity: Arc<Mutex<f32>>,

    /// Mutex for tank level switch signals
    pub mutex_tank_level_switch_signals: Arc<Mutex<TankLevelSwitchSignals>>,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for regular data logging to SQL database
/// and writing to the file (in RAM disk).
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     data_logger[Data logger] --> atlas_scientific[AtlasScientific]
///     atlas_scientific --> data_logger
///     data_logger --> ambient[Ambient]
///     ambient --> data_logger
///     data_logger --> tank_level_switch[TankLevelSwitch]
///     tank_level_switch --> data_logger
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

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to create the timestamp
    lock_error_get_timestamp: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write refill in progress to disk
    lock_error_output_state_refill_in_progress: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write heating is on to disk
    lock_error_output_state_heating_is_on: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write ventilation is on to disk
    lock_error_output_state_ventilation_is_on: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write water temperature to disk
    lock_error_output_signal_water_temperature: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write pH to disk
    lock_error_output_signal_pH: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write conductivity to disk
    lock_error_output_signal_conductivity: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write ambient temperature to disk
    lock_error_output_signal_ambient_temperature: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write humidity to disk
    lock_error_output_signal_humidity: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write tank level switch position to disk
    lock_error_output_state_tank_level_switch_position: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write tank level switch invalid to disk
    lock_error_output_state_tank_level_switch_invalid: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write tank level switch position stabilized to disk
    lock_error_output_state_tank_level_switch_position_stabilized: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write temperature filtered to disk
    lock_error_output_signal_water_temperature_filtered: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write pH filtered to disk
    lock_error_output_signal_pH_filtered: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write conductivity filtered to disk
    lock_error_output_signal_conductivity_filtered: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write timestamp to disk
    lock_error_output_timestamp: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about having received an inapplicable command
    lock_warn_inapplicable_command: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    lock_error_channel_receive_termination: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write water temperature from DS18B20 sensor to disk
    lock_error_output_signal_water_temperature_ds18b20: bool,

    /// an inhibition flag to avoid flooding the log file with repeated messages about failure to write ambient temperature from DS18B20 sensor to disk
    lock_error_output_signal_ambient_temperature_ds18b20: bool,
}

impl DataLogger {
    /// Returns a struct of type DataLogger
    /// # Arguments
    /// * `config` - configuration data for data logger
    pub fn new(config: DataLoggerConfig) -> DataLogger {
        let filter_water_temperature = IIRFilter::new_requires_initial(
            &config.filter_coefficient_water_temperature,
            0.0, // initialization value (measured value not available at this time)
            1.0, // 1 second time step
        );
        let filter_pH = IIRFilter::new_requires_initial(
            &config.filter_coefficient_pH,
            0.0, // initialization value (measured value not available at this time)
            1.0, // 1 second time step
        );
        let filter_conductivity = IIRFilter::new_requires_initial(
            &config.filter_coefficient_conductivity,
            0.0, // initialization value (measured value not available at this time)
            1.0, // 1 second time step
        );

        if config.output_to_disk {
            if config.output_file_name_timestamp.is_empty()
                || config.output_file_name_water_temperature.is_empty()
                || config
                    .output_file_name_water_temperature_filtered
                    .is_empty()
                || config.output_file_name_pH.is_empty()
                || config.output_file_name_pH_filtered.is_empty()
                || config.output_file_name_conductivity.is_empty()
                || config.output_file_name_conductivity_filtered.is_empty()
                || config
                    .output_file_name_tank_level_switch_position
                    .is_empty()
                || config.output_file_name_tank_level_switch_invalid.is_empty()
                || config
                    .output_file_name_tank_level_switch_position_stabilized
                    .is_empty()
                || config.output_file_name_ventilation_is_on.is_empty()
                || config.output_file_name_heating_is_on.is_empty()
                || config.output_file_name_ambient_temperature.is_empty()
                || config.output_file_name_humidity.is_empty()
                || config.output_file_name_refill_in_progress.is_empty()
                || config.output_file_name_water_temperature_ds18b20.is_empty()
                || config
                    .output_file_name_ambient_temperature_ds18b20
                    .is_empty()
            {
                panic!(
                    "{}: one of the output file names is empty.",
                    LOG_TARGET_DATA_LOGGER
                );
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
                fs::write(config.output_file_name_tank_level_switch_position_stabilized.clone(), "").expect(
                    "DataLogger: Could not reset content of output file for tank level switch position stabilized."
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
                fs::write(config.output_file_name_water_temperature_ds18b20.clone(), "").expect(
                    "DataLogger: Could not reset content of output file for water temperature from DS18B20 sensor.",
                );
                fs::write(config.output_file_name_ambient_temperature_ds18b20.clone(), "").expect(
                    "DataLogger: Could not reset content of output file for ambient air temperature from DS18B20 sensor.",
                );
            }
        }

        Self {
            config,
            filter_water_temperature,
            filter_pH,
            filter_conductivity,
            lock_error_get_timestamp: false,
            lock_error_output_signal_water_temperature: false,
            lock_error_output_state_refill_in_progress: false,
            lock_error_output_state_heating_is_on: false,
            lock_error_output_state_ventilation_is_on: false,
            lock_error_output_signal_pH: false,
            lock_error_output_signal_conductivity: false,
            lock_error_output_signal_ambient_temperature: false,
            lock_error_output_signal_humidity: false,
            lock_error_output_state_tank_level_switch_position: false,
            lock_error_output_state_tank_level_switch_invalid: false,
            lock_error_output_state_tank_level_switch_position_stabilized: false,
            lock_error_output_signal_water_temperature_filtered: false,
            lock_error_output_signal_pH_filtered: false,
            lock_error_output_signal_conductivity_filtered: false,
            lock_error_output_timestamp: false,
            lock_warn_inapplicable_command: false,
            lock_error_channel_receive_termination: false,
            lock_error_output_signal_water_temperature_ds18b20: false,
            lock_error_output_signal_ambient_temperature_ds18b20: false,
        }
    }

    /// Outputs non-binary signal to a file (in RAM disk).
    /// # Arguments
    /// * `signal` - signal which shall be converted to humanly readable format and written to file
    /// * `output_file_name` - name of the file where the value shall be written to    
    fn output_signal(
        &self,
        signal: f32,
        signal_type: AquariumSignal,
        output_file_name: &String,
        inhibit_error_log: bool,
    ) -> bool {
        if self.config.output_to_disk {
            let signal_string = signal_type.output_file_string(signal);

            match fs::write(output_file_name, signal_string.clone()) {
                Ok(_) => {
                    /* do nothing */
                    false // indicate no error
                }
                Err(e) => {
                    if !inhibit_error_log {
                        error!(
                            target: LOG_TARGET_DATA_LOGGER,
                            "output of value {} to {} failed: {e:?}",
                            signal_string, output_file_name
                        );
                    }
                    true // indicate error
                }
            }
        } else {
            false // no output = no error
        }
    }

    /// Outputs binary signal to a file (in RAM disk).
    /// # Arguments
    /// * `device_state` - signal which shall be converted to a humanly readable format and written to file
    /// * `state_description_true` - label to write to a file for true value
    /// * `state_description_false` - label to write to a file for false value
    /// * `output_file_name` - name of the file where the value shall be written to    
    fn output_state(
        &self,
        state: bool,
        state_description_true: &str,
        state_description_false: &str,
        output_file_name: &String,
        inhibit_error_log: bool,
    ) -> bool {
        if self.config.output_to_disk {
            let state_string = match state {
                true => state_description_true,
                false => state_description_false,
            };

            match fs::write(output_file_name, state_string) {
                Ok(_) => {
                    /* do nothing */
                    false // indicate no error
                }
                Err(e) => {
                    if !inhibit_error_log {
                        error!(
                            target: LOG_TARGET_DATA_LOGGER,
                            "output of value {} to {} failed: {e:?}",
                            state_string, output_file_name
                        );
                    }
                    true // indicate error
                }
            }
        } else {
            false // no output = no error
        }
    }

    /// Outputs string to a file (in RAM disk).
    /// # Arguments
    /// * `data` - string data to be written to file
    /// * `output_file_name` - name of the file where the value shall be written to    
    fn output_timestamp(
        &self,
        timestamp: NaiveDateTime,
        output_file_name: &String,
        inhibit_error_log: bool,
    ) -> bool {
        if self.config.output_to_disk {
            match fs::write(output_file_name, timestamp.to_string()) {
                Ok(_) => {
                    /* do nothing */
                    false // indicate no error
                }
                Err(e) => {
                    if !inhibit_error_log {
                        error!(
                            target: LOG_TARGET_DATA_LOGGER,
                            "output of value {} to {} failed: {e:?}",
                            timestamp,
                            output_file_name
                        );
                    }
                    true // indicate error
                }
            }
        } else {
            false // no output = no error
        }
    }

    /// Get information from refill/ventilation/heating thread
    /// # Arguments
    /// * `signal` - reference to variable for storing control status information of refill/ventilation/heating
    /// * `signal_name` - enum indicating which signal shall be measured
    /// * `tx_to_on_off_thread` - reference to sender part of the channel for communication to refill/ventilation/heating
    /// * `rx_from_on_off_thread` - reference to receiver part of the channel for communication from refill/ventilation/heating
    /// * `lock_error_output_state` - information if error has already occurred
    fn update_on_off_signal(
        &mut self,
        signal: &mut bool,
        refill_channel_disconnected: &mut bool,
        signal_name: AquariumSignal,
        tx_to_on_off_thread: &Sender<InternalCommand>,
        rx_from_on_off_thread: &Receiver<bool>,
        mut lock_error_output_state: bool,
    ) -> bool {
        if !*refill_channel_disconnected {
            #[cfg(feature = "debug_data_logger")]
            let start_time_refill = SystemTime::now();

            match tx_to_on_off_thread.send(InternalCommand::RequestSignal(signal_name.clone())) {
                Ok(_) => {
                    // now listen for a response
                    match rx_from_on_off_thread.recv() {
                        Ok(c) => {
                            *signal = c;
                            let output_file_name = match signal_name {
                                AquariumSignal::RefillPumpStatus => {
                                    &self.config.output_file_name_refill_in_progress
                                }
                                AquariumSignal::VentilationControlStatus => {
                                    &self.config.output_file_name_ventilation_is_on
                                }
                                AquariumSignal::HeatingControlStatus => {
                                    &self.config.output_file_name_heating_is_on
                                }
                                _ => {
                                    // invalid signal parameter
                                    return true;
                                }
                            };
                            lock_error_output_state = self.output_state(
                                c,
                                "ON",
                                "OFF",
                                output_file_name,
                                lock_error_output_state,
                            );
                        }
                        Err(e) => {
                            error!(
                                target: LOG_TARGET_DATA_LOGGER,
                                "receiving value for {} failed ({e:?})",
                                signal
                            );
                            *refill_channel_disconnected = true;
                        }
                    }
                }
                Err(e) => {
                    error!(
                        target: LOG_TARGET_DATA_LOGGER,
                        "channel communication for {} failed ({e:?})",
                        signal
                    );
                    *refill_channel_disconnected = true;
                }
            }
            #[cfg(feature = "debug_data_logger")]
            {
                let stop_time_refill = SystemTime::now();
                let execution_duration_refill_millis = stop_time_refill
                    .duration_since(start_time_refill)
                    .unwrap()
                    .as_millis() as u64;
                debug!(
                    target: LOG_TARGET_DATA_LOGGER,
                    "communication for {} took {} ms.",
                    signal_name,
                    execution_duration_refill_millis
                );
            }
        }
        lock_error_output_state
    }

    /// Get information from Ambient thread
    /// # Arguments
    /// * `mutex_ambient_temperature` - Mutex for ambient temperature
    /// * `mutex_humidity` - reference to receiver part of the channel for communication from Ambient
    /// * `ambient_temperature` - reference to variable for storing signal
    /// * `humidity` - reference to variable for storing signal
    fn update_ambient_signals(
        &mut self,
        mutex_ambient_temperature: Arc<Mutex<f32>>,
        mutex_humidity: Arc<Mutex<f32>>,
        ambient_temperature: &mut Option<f32>,
        humidity: &mut Option<f32>,
    ) {
        {
            // internal scope to limit lifetime of mutex lock
            *ambient_temperature = match mutex_ambient_temperature.lock() {
                Ok(c) => {
                    self.lock_error_output_signal_ambient_temperature = self.output_signal(
                        *c,
                        AquariumSignal::AmbientTemperature,
                        &self.config.output_file_name_ambient_temperature,
                        self.lock_error_output_signal_ambient_temperature,
                    );
                    Some(*c)
                }
                Err(_) => None,
            };
            *humidity = match mutex_humidity.lock() {
                Ok(c) => {
                    self.lock_error_output_signal_humidity = self.output_signal(
                        *c,
                        AquariumSignal::AmbientHumidity,
                        &self.config.output_file_name_humidity,
                        self.lock_error_output_signal_humidity,
                    );
                    Some(*c)
                }
                Err(_) => None,
            };
        }
    }

    /// Executes thread communication and filter calculation for all signals
    /// # Arguments
    /// * `data_logger_channels` - reference to container of channels for communication with other threads
    /// * `refill_in_progress` - reference to variable for storing refill in progress information
    /// * `refill_channel_disconnected` - reference to variable for storing health status of the channel
    /// * `heater_is_on` - reference to variable for storing heating control status information
    /// * `heating_channel_disconnected` - reference to variable for storing health status of the channel
    /// * `ventilation_is_on` - reference to variable for storing ventilation control status information
    /// * `ventilation_channel_disconnected` - reference to variable for storing health status of the channel
    fn update_signals(
        &mut self,
        data_logger_channels: &DataLoggerChannels,
        refill_status: &mut (bool, bool), // (in_progress, channel_disconnected)
        heater_status: &mut (bool, bool), // (is_on, channel_disconnected)
        ventilation_status: &mut (bool, bool), // (is_on, channel_disconnected)
    ) {
        // Extract the individual parameters from the tuples
        let (mut refill_in_progress, mut refill_channel_disconnected) = *refill_status;
        let (mut heater_is_on, mut heating_channel_disconnected) = *heater_status;
        let (mut ventilation_is_on, mut ventilation_channel_disconnected) = *ventilation_status;

        //*** communication with refill thread ***
        self.lock_error_output_state_refill_in_progress = self.update_on_off_signal(
            &mut refill_in_progress, &mut refill_channel_disconnected,
            AquariumSignal::RefillPumpStatus,
            data_logger_channels.tx_to_refill,
            data_logger_channels.rx_from_refill,
            self.lock_error_output_state_refill_in_progress,
        );
        *refill_status = (refill_in_progress, refill_channel_disconnected);

        //*** communication with heating thread ***
        self.lock_error_output_state_heating_is_on = self.update_on_off_signal(
            &mut heater_is_on,
            &mut heating_channel_disconnected,
            AquariumSignal::HeatingControlStatus,
            data_logger_channels.tx_to_heating,
            data_logger_channels.rx_from_heating,
            self.lock_error_output_state_heating_is_on,
        );
        *heater_status = (heater_is_on, heating_channel_disconnected);

        //*** communication with ventilation thread ***
        self.lock_error_output_state_ventilation_is_on = self.update_on_off_signal(
            &mut ventilation_is_on,
            &mut ventilation_channel_disconnected,
            AquariumSignal::VentilationControlStatus,
            data_logger_channels.tx_to_ventilation,
            data_logger_channels.rx_from_ventilation,
            self.lock_error_output_state_ventilation_is_on,
        );
        *ventilation_status = (ventilation_is_on, ventilation_channel_disconnected);
    }

    /// Read values of Atlas Scientific from Mutex and write them to RAM disk
    /// # Arguments
    /// * `mutex_temperature` - Mutex for water temperature
    /// * `mutex_ph` - Mutex for pH
    /// * `mutex_conductivity` - Mutex for conductivity
    /// * `temperature` - Reference to temperature variable
    /// * `ph` - Reference to pH variable
    /// * `conductivity` - Reference to conductivity variable
    fn update_atlas_scientific_signals(
        &mut self,
        mutex_temperature: Arc<Mutex<f32>>,
        mutex_ph: Arc<Mutex<f32>>,
        mutex_conductivity: Arc<Mutex<f32>>,
        temperature: &mut Option<f32>,
        ph: &mut Option<f32>,
        conductivity: &mut Option<f32>,
    ) {
        {
            // internal scope to limit lifetime of mutex lock
            *temperature = match mutex_temperature.lock() {
                Ok(c) => {
                    self.lock_error_output_signal_water_temperature = self.output_signal(
                        *c,
                        AquariumSignal::WaterTemperature,
                        &self.config.output_file_name_water_temperature,
                        self.lock_error_output_signal_water_temperature,
                    );
                    Some(*c)
                }
                Err(_) => None,
            };
            *ph = match mutex_ph.lock() {
                Ok(c) => {
                    self.lock_error_output_signal_pH = self.output_signal(
                        *c,
                        AquariumSignal::pH,
                        &self.config.output_file_name_pH,
                        self.lock_error_output_signal_pH,
                    );
                    Some(*c)
                }
                Err(_) => None,
            };
            *conductivity = match mutex_conductivity.lock() {
                Ok(c) => {
                    self.lock_error_output_signal_conductivity = self.output_signal(
                        *c,
                        AquariumSignal::Conductivity,
                        &self.config.output_file_name_conductivity,
                        self.lock_error_output_signal_conductivity,
                    );
                    Some(*c)
                }
                Err(_) => None,
            };
        }
    }

    /// Read values of DS18B20 sensor from Mutex and write them to RAM disk
    /// # Arguments
    /// * `mutex_water_temperature` - Mutex for water temperature
    /// * `mutex_ambient_temperature` - Mutex for ambient temperature
    /// * `water_temperature` - Reference to water temperature variable
    /// * `ambient_temperature` - Reference to ambient temperature variable
    fn update_ds18b20_signals(
        &mut self,
        mutex_water_temperature: Arc<Mutex<f32>>,
        mutex_ambient_temperature: Arc<Mutex<f32>>,
        water_temperature: &mut Option<f32>,
        ambient_temperature: &mut Option<f32>,
    ) {
        {
            // internal scope to limit lifetime of mutex lock
            *water_temperature = match mutex_water_temperature.lock() {
                Ok(c) => {
                    self.lock_error_output_signal_water_temperature_ds18b20 = self.output_signal(
                        *c,
                        AquariumSignal::WaterTemperature,
                        &self.config.output_file_name_water_temperature_ds18b20,
                        self.lock_error_output_signal_water_temperature_ds18b20,
                    );
                    Some(*c)
                }
                Err(_) => None,
            };
            *ambient_temperature = match mutex_ambient_temperature.lock() {
                Ok(c) => {
                    self.lock_error_output_signal_ambient_temperature_ds18b20 = self.output_signal(
                        *c,
                        AquariumSignal::AmbientTemperature,
                        &self.config.output_file_name_ambient_temperature_ds18b20,
                        self.lock_error_output_signal_ambient_temperature_ds18b20,
                    );
                    Some(*c)
                }
                Err(_) => None,
            };
        }
    }

    /// Read values of tank level switch from Mutex and write them to RAM disk
    /// # Arguments
    /// * `mutex_tank_level_switch_signals` - Mutex for tank level switch signals
    /// * `tank_level_switch_position_opt` - Reference to tank level switch position variable wrapped in Option
    /// * `tank_level_switch_position_stabilized_opt` - Reference to tank level switch position stabilized variable wrapped in Option
    /// * `tank_level_switch_invalid_opt` - Reference to tank level switch invalid variable wrapped in Option
    fn update_tank_level_switch_signals(
        &mut self,
        mutex_tank_level_switch_signals: &Arc<Mutex<TankLevelSwitchSignals>>,
        tank_level_switch_position_opt: &mut Option<bool>,
        tank_level_switch_position_stabilized_opt: &mut Option<bool>,
        tank_level_switch_invalid_opt: &mut Option<bool>,
    ) {
        {
            // internal scope to limit lifetime of mutex lock
            match mutex_tank_level_switch_signals.lock() {
                Ok(tank_level_switch_signals) => {
                    *tank_level_switch_position_opt =
                        Some(tank_level_switch_signals.tank_level_switch_position);
                    *tank_level_switch_position_stabilized_opt =
                        Some(tank_level_switch_signals.tank_level_switch_position_stabilized);
                    *tank_level_switch_invalid_opt =
                        Some(tank_level_switch_signals.tank_level_switch_invalid);

                    self.lock_error_output_state_tank_level_switch_position = self.output_state(
                        tank_level_switch_signals.tank_level_switch_position,
                        "HIGH",
                        "LOW",
                        &self.config.output_file_name_tank_level_switch_position,
                        self.lock_error_output_state_tank_level_switch_position,
                    );

                    self.lock_error_output_state_tank_level_switch_position_stabilized = self
                        .output_state(
                            tank_level_switch_signals.tank_level_switch_position_stabilized,
                            "HIGH",
                            "LOW",
                            &self
                                .config
                                .output_file_name_tank_level_switch_position_stabilized,
                            self.lock_error_output_state_tank_level_switch_position_stabilized,
                        );

                    self.lock_error_output_state_tank_level_switch_invalid = self.output_state(
                        tank_level_switch_signals.tank_level_switch_invalid,
                        "INVALID",
                        "VALID",
                        &self.config.output_file_name_tank_level_switch_invalid,
                        self.lock_error_output_state_tank_level_switch_invalid,
                    );
                }
                Err(_) => {
                    *tank_level_switch_position_opt = None;
                    *tank_level_switch_position_stabilized_opt = None;
                    *tank_level_switch_invalid_opt = None;
                }
            };
        }
    }

    /// Calculate filtered values and output them to (RAM disk) files
    /// # Arguments
    /// * `water_temperature` - measured water temperature wrapped in Option
    /// * `ph` - measured ph temperature wrapped in Option
    /// * `conductivity` - measured conductivity temperature wrapped in Option
    fn calculate_filtered_signals(
        &mut self,
        water_temperature: Option<f32>,
        ph: Option<f32>,
        conductivity: Option<f32>,
    ) -> (Option<f32>, Option<f32>, Option<f32>) {
        let water_temperature_filtered = match water_temperature {
            Some(c) => {
                let filtered_water_temperature =
                    self.filter_water_temperature.get_filtered_value(c);
                self.lock_error_output_signal_water_temperature_filtered = self.output_signal(
                    filtered_water_temperature,
                    AquariumSignal::WaterTemperature,
                    &self.config.output_file_name_water_temperature_filtered,
                    self.lock_error_output_signal_water_temperature_filtered,
                );
                Some(filtered_water_temperature)
            }
            None => None,
        };

        let pH_filtered = match ph {
            Some(c) => {
                let filtered_pH = self.filter_pH.get_filtered_value(c);
                self.lock_error_output_signal_pH_filtered = self.output_signal(
                    filtered_pH,
                    AquariumSignal::pH,
                    &self.config.output_file_name_pH_filtered,
                    self.lock_error_output_signal_pH_filtered,
                );
                Some(filtered_pH)
            }
            None => None,
        };

        let conductivity_filtered = match conductivity {
            Some(c) => {
                let filtered_conductivity = self.filter_conductivity.get_filtered_value(c);
                self.lock_error_output_signal_conductivity_filtered = self.output_signal(
                    filtered_conductivity,
                    AquariumSignal::Conductivity,
                    &self.config.output_file_name_conductivity_filtered,
                    self.lock_error_output_signal_conductivity_filtered,
                );
                Some(filtered_conductivity)
            }
            None => None,
        };

        (
            water_temperature_filtered,
            pH_filtered,
            conductivity_filtered,
        )
    }

    /// get timestamp either from SQL server or application-generated
    /// # Arguments    
    /// * `conn` - connection to SQL database
    fn update_timestamp(&mut self, conn: &mut PooledConn) -> NaiveDateTime {
        match self.config.use_sql_timestamp {
            true => match SqlInterface::get_current_timestamp(conn) {
                Ok(c) => {
                    self.lock_error_get_timestamp = false;
                    self.lock_error_output_timestamp = self.output_timestamp(
                        c,
                        &self.config.output_file_name_timestamp,
                        self.lock_error_output_timestamp,
                    );
                    c
                }
                Err(e) => {
                    if !self.lock_error_get_timestamp {
                        error!(
                            target: LOG_TARGET_DATA_LOGGER,
                            "could not retrieve timestamp from database: ({e:?}) - using application-generated timestamp."
                        );
                        self.lock_error_get_timestamp = true;
                    }
                    Local::now().naive_local()
                }
            },
            false => Local::now().naive_local(),
        }
    }

    /// Executes the main control in a loop until the signal handler commands quitting and termination.
    /// # Arguments
    /// * `data_injection` - Trait method for transferring data to database or mock data structure
    /// * `sql_interface_data` - Interface to SQL database for manipulation of data table
    /// * `data_logger_channels` - Struct containing the channels for communication with other threads
    /// 
    pub fn execute(
        &mut self,
        data_injection: &mut impl DataInjectionTrait,
        mut sql_interface_data: SqlInterfaceData,
        data_logger_channels: DataLoggerChannels,
        data_logger_mutexes: DataLoggerMutexes
    ) {
        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut i;

        // define and initialize with values which will be overwritten in the first iteration
        let mut water_temperature: Option<f32> = Some(10.0);
        let mut ph: Option<f32> = Some(0.0);
        let mut conductivity: Option<f32> = Some(0.0);
        let mut ambient_temperature: Option<f32> = Some(0.0);
        let mut ambient_humidity: Option<f32> = Some(0.0);
        let mut tank_level_switch_invalid: Option<bool> = Some(false);
        let mut tank_level_switch_position: Option<bool> = Some(true);
        let mut tank_level_switch_position_stabilized: Option<bool> = Some(true);
        let mut refill_in_progress: bool = false;
        let mut heater_is_on: bool = false;
        let mut ventilation_is_on: bool = false;
        let mut quit_command_received = false;
        let mut refill_channel_disconnected = false;
        let mut heating_channel_disconnected = false;
        let mut ventilation_channel_disconnected = false;
        let mut start_time = SystemTime::now();
        let mut lock_warn_cycle_time_exceeded = false;
        let mut cycle_time_exceeded: bool; // ensures that signal handler/messaging are polled at least once even if cycle time is exceeded
        let mut water_temperature_ds18b20: Option<f32> = Some(0.0);
        let mut ambient_temperature_ds18b20: Option<f32> = Some(0.0);

        loop {
            // update signals via channel communication
            let mut refill_status = (refill_in_progress, refill_channel_disconnected);
            let mut heater_status = (heater_is_on, heating_channel_disconnected);
            let mut ventilation_status = (ventilation_is_on, ventilation_channel_disconnected);
            self.update_signals(
                &data_logger_channels,
                &mut refill_status,
                &mut heater_status,
                &mut ventilation_status,
            );
            (refill_in_progress, refill_channel_disconnected) = refill_status;
            (heater_is_on, heating_channel_disconnected) = heater_status;
            (ventilation_is_on, ventilation_channel_disconnected) = ventilation_status;

            // update signals by accessing mutex for DS18B20 sensor signals
            self.update_ds18b20_signals(
                data_logger_mutexes.mutex_water_temperature_ds18b20.clone(),
                data_logger_mutexes.mutex_ambient_temperature_dsb18b20.clone(),
                &mut water_temperature_ds18b20,
                &mut ambient_temperature_ds18b20,
            );

            // update signals by accessing mutex for Atlas Scientific signals
            self.update_atlas_scientific_signals(
                data_logger_mutexes.mutex_atlas_scientific_temperature.clone(),
                data_logger_mutexes.mutex_atlas_scientific_ph.clone(),
                data_logger_mutexes.mutex_atlas_scientific_conductivity.clone(),
                &mut water_temperature,
                &mut ph,
                &mut conductivity,
            );

            // update signals by accessing mutex for ambient signal
            self.update_ambient_signals(
                data_logger_mutexes.mutex_ambient_temperature.clone(),
                data_logger_mutexes.mutex_ambient_humidity.clone(),
                &mut ambient_temperature,
                &mut ambient_humidity,
            );

            // update signals by accessing mutex for tank level switch signals
            self.update_tank_level_switch_signals(
                &data_logger_mutexes.mutex_tank_level_switch_signals,
                &mut tank_level_switch_position,
                &mut tank_level_switch_position_stabilized,
                &mut tank_level_switch_invalid,
            );

            let (water_temperature_filtered, pH_filtered, conductivity_filtered) =
                self.calculate_filtered_signals(water_temperature, ph, conductivity);

            let current_timestamp = self.update_timestamp(&mut sql_interface_data.conn);

            // all data received, let's construct a data frame
            let data_frame = RecorderDataFrame {
                timestamp: current_timestamp,
                water_temperature,
                water_temperature_filtered,
                ph,
                ph_filtered: pH_filtered,
                conductivity,
                conductivity_filtered,
                conductivity_compensated: None,
                refill_in_progress: Some(refill_in_progress),
                tank_level_switch_position,
                tank_level_switch_position_stabilized,
                tank_level_switch_invalid,
                surface_ventilation_status: Some(ventilation_is_on),
                ambient_temperature,
                ambient_humidity,
                heater_status: Some(heater_is_on),
                water_temperature_ds18b20,
                ambient_temperature_ds18b20,
            };

            data_injection.inject_data(data_frame, &mut sql_interface_data);

            let stop_time = SystemTime::now();
            let execution_duration_millis =
                stop_time.duration_since(start_time).unwrap().as_millis() as u64;

            // deduct the execution time of the thread from the target cycle time
            let remaining_sleep_time_millis: u64;
            if execution_duration_millis >= self.config.logging_interval * MILLIS_PER_SEC {
                if !lock_warn_cycle_time_exceeded {
                    warn!(target: LOG_TARGET_DATA_LOGGER,
                        "execution duration of {} milliseconds exceeds cycle time of {}",
                        execution_duration_millis,
                        self.config.logging_interval * MILLIS_PER_SEC);
                    lock_warn_cycle_time_exceeded = true;
                } else {
                    lock_warn_cycle_time_exceeded = false;
                }
                remaining_sleep_time_millis = 0;
                cycle_time_exceeded = true;
            } else {
                // underflow cannot happen because the else statement requires that first operand is higher than the second
                remaining_sleep_time_millis =
                    self.config.logging_interval * MILLIS_PER_SEC - execution_duration_millis;
                cycle_time_exceeded = false;
            }

            #[cfg(feature = "debug_data_logger")]
            debug!(
                target: LOG_TARGET_DATA_LOGGER,
                "inserted data into database, now idling for {} milliseconds",
                remaining_sleep_time_millis
            );

            // wait the remaining time period to complete the target cycle time
            i = 0;
            while i < remaining_sleep_time_millis || cycle_time_exceeded {
                i += 100;
                spin_sleeper.sleep(sleep_duration_hundred_millis);
                (quit_command_received, _, _) =
                    self.process_external_request(data_logger_channels.rx_from_signal_handler, None);
                if quit_command_received {
                    #[cfg(feature = "debug_data_logger")]
                    debug!(
                        target: LOG_TARGET_DATA_LOGGER,
                        "received QUIT command from signal handler"
                    );
                    break;
                }

                // once per second, retrieve signals and write them to the RAM disk
                if i % 1000 == 0 {
                    // update signals via channel communication
                    let mut refill_status = (refill_in_progress, refill_channel_disconnected);
                    let mut heater_status = (heater_is_on, heating_channel_disconnected);
                    let mut ventilation_status = (ventilation_is_on, ventilation_channel_disconnected);
                    self.update_signals(
                        &data_logger_channels,
                        &mut refill_status,
                        &mut heater_status,
                        &mut ventilation_status,
                    );
                    (refill_in_progress, refill_channel_disconnected) = refill_status;
                    (heater_is_on, heating_channel_disconnected) = heater_status;
                    (ventilation_is_on, ventilation_channel_disconnected) = ventilation_status;


                    // update signals by accessing mutex for DS18B20 sensor signals
                    self.update_ds18b20_signals(
                        data_logger_mutexes.mutex_water_temperature_ds18b20.clone(),
                        data_logger_mutexes.mutex_ambient_temperature_dsb18b20.clone(),
                        &mut water_temperature_ds18b20,
                        &mut ambient_temperature_ds18b20,
                    );

                    // update signals by accessing mutex for Atlas Scientific signals
                    self.update_atlas_scientific_signals(
                        data_logger_mutexes.mutex_atlas_scientific_temperature.clone(),
                        data_logger_mutexes.mutex_atlas_scientific_ph.clone(),
                        data_logger_mutexes.mutex_atlas_scientific_conductivity.clone(),
                        &mut water_temperature,
                        &mut ph,
                        &mut conductivity,
                    );

                    // update signals by accessing mutex for ambient signal
                    self.update_ambient_signals(
                        data_logger_mutexes.mutex_ambient_temperature.clone(),
                        data_logger_mutexes.mutex_ambient_humidity.clone(),
                        &mut ambient_temperature,
                        &mut ambient_humidity,
                    );

                    // update signals by accessing mutex for tank level switch signals
                    self.update_tank_level_switch_signals(
                        &data_logger_mutexes.mutex_tank_level_switch_signals,
                        &mut tank_level_switch_position,
                        &mut tank_level_switch_position_stabilized,
                        &mut tank_level_switch_invalid,
                    );
                }
                cycle_time_exceeded = false;
            }
            if quit_command_received {
                break;
            }

            #[cfg(feature = "debug_data_logger")]
            debug!(
                target: LOG_TARGET_DATA_LOGGER,
                "finished idling restarting loop"
            );

            start_time = SystemTime::now(); // start time for the next cycle
        }

        // application received request to terminate, that is why the loop was left.
        // answer to the thread which sent the request for termination, so that shutdown can proceed further.
        match data_logger_channels.tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_DATA_LOGGER,
                    "sending confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // This thread has channel connections to underlying threads.
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match data_logger_channels.rx_from_signal_handler.recv() {
                Ok(c) => match c {
                    InternalCommand::Terminate => {
                        self.lock_warn_inapplicable_command = false;
                        true
                    }
                    _ => {
                        if !self.lock_warn_inapplicable_command {
                            warn!(
                                target: LOG_TARGET_DATA_LOGGER,
                                "ignoring inapplicable command from signal handler ({})",
                                c
                            );
                            self.lock_warn_inapplicable_command = true;
                        }
                        false
                    }
                },
                Err(e) => {
                    if !self.lock_error_channel_receive_termination {
                        error!(
                            target: LOG_TARGET_DATA_LOGGER,
                            "receiving termination signal from signal handler failed ({e:?})"
                        );
                        self.lock_error_channel_receive_termination = true;
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
    use crate::database::{sql_interface::SqlInterface, sql_interface_data::SqlInterfaceData};
    use crate::recorder::data_logger::{DataLogger, DataLoggerChannels, DataLoggerConfig, DataLoggerMutexes};
    use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
    use crate::stubs::mock_data_injection::tests::MockDataInjection;
    use crate::stubs::mock_heating::tests::mock_heating;
    use crate::stubs::mock_refill::tests::mock_refill;
    use crate::stubs::mock_signal::tests::MockSignal;
    use crate::stubs::mock_ventilation::tests::mock_ventilation;
    use crate::stubs::test_command::tests::TestCommand;
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};
    use spin_sleep::SpinSleeper;
    use std::sync::{Arc, Mutex};
    use std::{fs, sync::mpsc, thread, time::Duration};

    fn assert_output_file_contents(
        data_logger_config: &DataLoggerConfig,
        target_water_temperature: f32,
        target_water_temperature_filtered: f32,
        target_water_ph: f32,
        target_water_ph_filtered: f32,
        target_water_conductivity: f32,
        target_water_conductivity_filtered: f32,
        _target_water_conductivity_compensated: f32,
        target_refill_in_progress: bool,
        target_tank_level_switch_position: bool,
        target_tank_level_switch_invalid: bool,
        target_tank_level_switch_position_stabilized: bool,
        target_surface_ventilation_status: bool,
        target_ambient_temperature: f32,
        target_ambient_humidity: f32,
        target_heater_status: bool,
        target_water_temperature_ds18b20: f32,
        target_ambient_temperature_ds18b20: f32,
    ) {
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_water_temperature)
                .expect("Could not read back water temperature from disk.")
                .parse::<f32>()
                .unwrap(),
            target_water_temperature,
        );
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_water_temperature_filtered)
                .expect("Could not read back filtered water temperature from disk.")
                .parse::<f32>()
                .unwrap(),
            target_water_temperature_filtered,
        );
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_pH)
                .expect("Could not read back pH from disk.")
                .parse::<f32>()
                .unwrap(),
            target_water_ph,
        );
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_pH_filtered)
                .expect("Could not read back filtered pH from disk.")
                .parse::<f32>()
                .unwrap(),
            target_water_ph_filtered,
        );
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_conductivity)
                .expect("Could not read back conductivity from disk.")
                .parse::<f32>()
                .unwrap(),
            target_water_conductivity,
        );
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_conductivity_filtered)
                .expect("Could not read back filtered conductivity from disk.")
                .parse::<f32>()
                .unwrap(),
            target_water_conductivity_filtered,
        );
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_ambient_temperature)
                .expect("Could not read back ambient temperature from disk.")
                .parse::<f32>()
                .unwrap(),
            target_ambient_temperature,
        );
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_humidity)
                .expect("Could not read back ambient humidity from disk.")
                .parse::<f32>()
                .unwrap(),
            target_ambient_humidity,
        );
        let target_refill_in_progress_string = match target_refill_in_progress {
            true => "ON".to_string(),
            false => "OFF".to_string(),
        };
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_refill_in_progress)
                .expect("Could not read back refill in progress from disk."),
            target_refill_in_progress_string,
        );
        let target_tank_level_switch_position_string = match target_tank_level_switch_position {
            true => "HIGH".to_string(),
            false => "LOW".to_string(),
        };
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_tank_level_switch_position)
                .expect("Could not read back tank_level_switch_position from disk."),
            target_tank_level_switch_position_string,
        );
        let target_tank_level_switch_invalid_string = match target_tank_level_switch_invalid {
            true => "INVALID".to_string(),
            false => "VALID".to_string(),
        };
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_tank_level_switch_invalid)
                .expect("Could not read back tank level switch invalid from disk."),
            target_tank_level_switch_invalid_string,
        );
        let target_tank_level_switch_position_stabilized_string =
            match target_tank_level_switch_position_stabilized {
                true => "HIGH".to_string(),
                false => "LOW".to_string(),
            };
        assert_eq!(
            fs::read_to_string(
                &data_logger_config.output_file_name_tank_level_switch_position_stabilized
            )
            .expect("Could not read back tank_level_switch_position from disk."),
            target_tank_level_switch_position_stabilized_string,
        );
        let target_surface_ventilation_status_string = match target_surface_ventilation_status {
            true => "ON".to_string(),
            false => "OFF".to_string(),
        };
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_ventilation_is_on)
                .expect("Could not read back ventilation status from disk."),
            target_surface_ventilation_status_string,
        );
        let target_heater_status_string = match target_heater_status {
            true => "ON".to_string(),
            false => "OFF".to_string(),
        };
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_heating_is_on)
                .expect("Could not read back heater status from disk."),
            target_heater_status_string,
        );
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_water_temperature_ds18b20)
                .expect("Could not read back DS18B20 water temperature from disk.")
                .parse::<f32>()
                .unwrap(),
            target_water_temperature_ds18b20,
        );
        assert_eq!(
            fs::read_to_string(&data_logger_config.output_file_name_ambient_temperature_ds18b20)
                .expect("Could not read back DS18B20 ambient temperature from disk.")
                .parse::<f32>()
                .unwrap(),
            target_ambient_temperature_ds18b20,
        );
    }

    #[test]
    // test case checks if DataLogger correctly processes all information:
    // - storage in SQL database using mock implementation
    // - writing output to filesystem
    // Test case uses test database #45.
    pub fn test_data_log() {
        // channels for communication with the test environment
        let (tx_refill_to_data_logger, rx_data_logger_from_refill) = mpsc::channel();
        let (tx_data_logger_to_refill, rx_refill_from_data_logger) = mpsc::channel();
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_data_logger_to_heating, rx_heating_from_data_logger) = mpsc::channel();
        let (tx_ventilation_to_data_logger, rx_data_logger_from_ventilation) = mpsc::channel();
        let (tx_data_logger_to_ventilation, rx_ventilation_from_data_logger) = mpsc::channel();
        let (tx_data_logger_to_signal_handler, rx_signal_handler_from_data_logger) =
            mpsc::channel();
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_ventilation, rx_ventilation_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_test_environment_to_ventilation, rx_ventilation_from_test_environment) =
            mpsc::channel();
        let (tx_test_environment_to_heating, rx_heating_from_test_environment) = mpsc::channel();
        let (tx_test_environment_to_refill, rx_refill_from_test_environment) = mpsc::channel();

        let mut mock_data_injection = MockDataInjection::new();

        let mut config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            45,
        );
        config.data_logger.logging_interval = 1;

        let mut config2: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            45,
        );
        config2.data_logger.logging_interval = 1;

        let sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_data = SqlInterfaceData::new(sql_interface.get_connection());
        let mut data_logger = DataLogger::new(config.data_logger);

        let mutex_atlas_scientific_temperature = Arc::new(Mutex::new(10.0));
        let mutex_atlas_scientific_temperature_clone_for_data_logger =
            mutex_atlas_scientific_temperature.clone();
        let mutex_atlas_scientific_ph = Arc::new(Mutex::new(11.0));
        let mutex_atlas_scientific_ph_clone_for_data_logger = mutex_atlas_scientific_ph.clone();
        let mutex_atlas_scientific_conductivity = Arc::new(Mutex::new(12.0));
        let mutex_atlas_scientific_conductivity_clone_for_data_logger =
            mutex_atlas_scientific_conductivity.clone();
        let mutex_ambient_temperature = Arc::new(Mutex::new(13.0));
        let mutex_ambient_temperature_clone_for_data_logger = mutex_ambient_temperature.clone();
        let mutex_ambient_humidity = Arc::new(Mutex::new(14.0));
        let mutex_ambient_humidity_clone_for_data_logger = mutex_ambient_humidity.clone();
        let mutex_ds18b20_water_temperature = Arc::new(Mutex::new(15.0));
        let mutex_ds18b20_water_temperature_clone_for_data_logger =
            mutex_ds18b20_water_temperature.clone();
        let mutex_ds18b20_ambient_temperature = Arc::new(Mutex::new(16.0));
        let mutex_ds18b20_ambient_temperature_clone_for_data_logger =
            mutex_ds18b20_ambient_temperature.clone();
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, false)));
        // thread for test environment and mock signal handler
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let spin_sleeper = SpinSleeper::default();
                let sleep_duration_1_second = Duration::from_millis(1000);
                let sleep_duration_100_millis = Duration::from_millis(100);
                spin_sleeper.sleep(sleep_duration_1_second);
                spin_sleeper.sleep(sleep_duration_100_millis);

                let _ = tx_test_environment_to_ventilation.send(TestCommand::UpdateSignal(
                    MockSignal::VentilationControlStatus(true),
                ));
                {
                    let mut water_temperature = mutex_atlas_scientific_temperature.lock().unwrap();
                    *water_temperature = 29.0;
                }

                spin_sleeper.sleep(sleep_duration_1_second);

                let _ = tx_test_environment_to_heating.send(TestCommand::UpdateSignal(
                    MockSignal::HeatingControlStatus(true),
                ));

                {
                    let mut ph = mutex_atlas_scientific_ph.lock().unwrap();
                    *ph = 7.0;
                }

                spin_sleeper.sleep(sleep_duration_1_second);

                let _ = tx_test_environment_to_refill.send(TestCommand::UpdateSignal(
                    MockSignal::RefillPumpStatus(true),
                ));

                {
                    let mut conductivity = mutex_atlas_scientific_conductivity.lock().unwrap();
                    *conductivity = 50000.0;
                }

                spin_sleeper.sleep(sleep_duration_1_second);

                {
                    let mut tank_level_switch_signals =
                        mutex_tank_level_switch_signals.lock().unwrap();
                    tank_level_switch_signals.tank_level_switch_invalid = true;
                }
                {
                    let mut ambient_temperature = mutex_ambient_temperature.lock().unwrap();
                    *ambient_temperature = 35.0;
                }

                spin_sleeper.sleep(sleep_duration_1_second);

                {
                    let mut tank_level_switch_signals =
                        mutex_tank_level_switch_signals.lock().unwrap();
                    tank_level_switch_signals.tank_level_switch_position = true;
                }
                {
                    let mut ambient_humidity = mutex_ambient_humidity.lock().unwrap();
                    *ambient_humidity = 66.0;
                }

                spin_sleeper.sleep(sleep_duration_1_second);

                {
                    let mut water_temperature_ds18b20 =
                        mutex_ds18b20_water_temperature.lock().unwrap();
                    *water_temperature_ds18b20 = 15.5;
                    let mut ambient_temperature_ds18b20 =
                        mutex_ds18b20_ambient_temperature.lock().unwrap();
                    *ambient_temperature_ds18b20 = 16.5;
                }
                {
                    let mut tank_level_switch_signals =
                        mutex_tank_level_switch_signals.lock().unwrap();
                    tank_level_switch_signals.tank_level_switch_position_stabilized = true;
                }

                spin_sleeper.sleep(sleep_duration_1_second);

                // cease operation of the test object
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Quit);
                rx_signal_handler_from_data_logger.recv().unwrap();

                // signal end of test case execution to mock threads
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);

                spin_sleeper.sleep(sleep_duration_1_second);
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for mock refill control
        let join_handle_mock_refill = thread::Builder::new()
            .name("mock_refill".to_string())
            .spawn(move || {
                mock_refill(
                    None,
                    None,
                    &rx_refill_from_signal_handler,
                    Some(&tx_refill_to_data_logger),
                    Some(&rx_refill_from_data_logger),
                    Some(&rx_refill_from_test_environment),
                    false, // initial refill pump status
                );
            })
            .unwrap();

        // thread for mock heating
        let join_handle_mock_heating = thread::Builder::new()
            .name("mock_heating".to_string())
            .spawn(move || {
                mock_heating(
                    None,
                    None,
                    &rx_heating_from_signal_handler,
                    Some(&tx_heating_to_data_logger),
                    Some(&rx_heating_from_data_logger),
                    Some(&rx_heating_from_test_environment),
                    false, // initial heating control status
                );
            })
            .unwrap();

        // thread for mock ventilation
        let join_handle_mock_ventilation = thread::Builder::new()
            .name("mock_ventilation".to_string())
            .spawn(move || {
                mock_ventilation(
                    &tx_ventilation_to_data_logger,
                    &rx_ventilation_from_data_logger,
                    &rx_ventilation_from_signal_handler,
                    Some(&rx_ventilation_from_test_environment),
                    false, // initial ventilation control status
                );
            })
            .unwrap();

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // give mock threads some time to establish
        let spin_sleeper = SpinSleeper::default();
        let sleep_duration_100_millis = Duration::from_millis(100);
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for test object and asserts
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                let data_logger_channels = DataLoggerChannels {
                    tx_to_refill: &tx_data_logger_to_refill,
                    rx_from_refill: &rx_data_logger_from_refill,
                    tx_to_heating: &tx_data_logger_to_heating,
                    rx_from_heating: &rx_data_logger_from_heating,
                    tx_to_ventilation: &tx_data_logger_to_ventilation,
                    rx_from_ventilation: &rx_data_logger_from_ventilation,
                    tx_to_signal_handler: &tx_data_logger_to_signal_handler,
                    rx_from_signal_handler: &rx_data_logger_from_signal_handler,
                };

                let data_logger_mutexes = DataLoggerMutexes {
                    mutex_water_temperature_ds18b20: mutex_ds18b20_water_temperature_clone_for_data_logger,
                    mutex_ambient_temperature_dsb18b20: mutex_ds18b20_ambient_temperature_clone_for_data_logger,
                    mutex_atlas_scientific_temperature: mutex_atlas_scientific_temperature_clone_for_data_logger,
                    mutex_atlas_scientific_ph: mutex_atlas_scientific_ph_clone_for_data_logger,
                    mutex_atlas_scientific_conductivity: mutex_atlas_scientific_conductivity_clone_for_data_logger,
                    mutex_ambient_temperature: mutex_ambient_temperature_clone_for_data_logger,
                    mutex_ambient_humidity: mutex_ambient_humidity_clone_for_data_logger,
                    mutex_tank_level_switch_signals,
                };
                
                data_logger.execute(
                    &mut mock_data_injection,
                    sql_interface_data,
                    data_logger_channels,
                    data_logger_mutexes,
                );

                println!("Printing recorded data:");
                mock_data_injection.output();

                // Dataframe #6
                let mut data_frame = mock_data_injection.data_frames.pop().unwrap();
                println!("Asserting data frame #6");
                data_frame.assert(
                    Some(29.0),      // target_water_temperature
                    Some(28.625),    // target_water_temperature_filtered
                    Some(7.0),       // target_water_ph
                    Some(7.0390625), // target_water_ph_filtered
                    Some(50_000.0),  // target_water_conductivity
                    Some(46875.656), // target_water_conductivity_filtered
                    None,            // target_water_conductivity_compensated
                    Some(true),      // target_refill_in_progress
                    Some(true),      // target_tank_level_switch_position
                    Some(true),      // target_tank_level_switch_position_stabilized
                    Some(false),     // target_tank_level_switch_invalid
                    Some(true),      // target_surface_ventilation_status
                    Some(35.0),      // target_ambient_temperature
                    Some(66.0),      // target_ambient_humidity
                    Some(true),      // target_heater_status
                    Some(15.5),      // target DS18B20 water temperature
                    Some(16.5),      // target DS18B20 ambient temperature
                );

                // Dataframe #5
                data_frame = mock_data_injection.data_frames.pop().unwrap();
                println!("Asserting data frame #5");
                data_frame.assert(
                    Some(29.0),      // target_water_temperature
                    Some(28.25),     // target_water_temperature_filtered
                    Some(7.0),       // target_water_ph
                    Some(7.078125),  // target_water_ph_filtered
                    Some(50_000.0),  // target_water_conductivity
                    Some(43751.313), // target_water_conductivity_filtered
                    None,            // target_water_conductivity_compensated
                    Some(true),      // target_refill_in_progress
                    Some(true),      // target_tank_level_switch_position
                    Some(true),      // target_tank_level_switch_position_stabilized
                    Some(false),     // target_tank_level_switch_invalid
                    Some(true),      // target_surface_ventilation_status
                    Some(35.0),      // target_ambient_temperature
                    Some(66.0),      // target_ambient_humidity
                    Some(true),      // target_heater_status
                    Some(15.0),      // target DS18B20 water temperature
                    Some(16.0),      // target DS18B20 ambient temperature
                );

                // Dataframe #4
                data_frame = mock_data_injection.data_frames.pop().unwrap();
                println!("Asserting data frame #4");
                data_frame.assert(
                    Some(29.0),      // target_water_temperature
                    Some(27.5),      // target_water_temperature_filtered
                    Some(7.0),       // target_water_ph
                    Some(7.15625),   // target_water_ph_filtered
                    Some(50_000.0),  // target_water_conductivity
                    Some(37502.625), // target_water_conductivity_filtered
                    None,            // target_water_conductivity_compensated
                    Some(true),      // target_refill_in_progress
                    Some(true),      // target_tank_level_switch_position
                    Some(true),      // target_tank_level_switch_position_stabilized
                    Some(false),     // target_tank_level_switch_invalid
                    Some(true),      // target_surface_ventilation_status
                    Some(35.0),      // target_ambient_temperature
                    Some(14.0),      // target_ambient_humidity
                    Some(true),      // target_heater_status
                    Some(15.0),      // target DS18B20 water temperature
                    Some(16.0),      // target DS18B20 ambient temperature
                );

                // Dataframe #3
                data_frame = mock_data_injection.data_frames.pop().unwrap();
                println!("Asserting data frame #3");
                data_frame.assert(
                    Some(29.0),     // target_water_temperature
                    Some(26.0),     // target_water_temperature_filtered
                    Some(7.0),      // target_water_ph
                    Some(7.3125),   // target_water_ph_filtered
                    Some(50_000.0), // target_water_conductivity
                    Some(25005.25), // target_water_conductivity_filtered
                    None,           // target_water_conductivity_compensated
                    Some(true),     // target_refill_in_progress
                    Some(true),     // target_tank_level_switch_position
                    Some(true),     // target_tank_level_switch_position_stabilized
                    Some(false),    // target_tank_level_switch_invalid
                    Some(true),     // target_surface_ventilation_status
                    Some(13.0),     // target_ambient_temperature
                    Some(14.0),     // target_ambient_humidity
                    Some(true),     // target_heater_status
                    Some(15.0),     // target DS18B20 water temperature
                    Some(16.0),     // target DS18B20 ambient temperature
                );

                // Dataframe #2
                data_frame = mock_data_injection.data_frames.pop().unwrap();
                println!("Asserting data frame #2");
                data_frame.assert(
                    Some(29.0),  // target_water_temperature
                    Some(23.0),  // target_water_temperature_filtered
                    Some(7.0),   // target_water_ph
                    Some(7.625), // target_water_ph_filtered
                    Some(12.0),  // target_water_conductivity
                    Some(10.5),  // target_water_conductivity_filtered
                    None,        // target_water_conductivity_compensated
                    Some(false), // target_refill_in_progress
                    Some(true),  // target_tank_level_switch_position
                    Some(true),  // target_tank_level_switch_position_stabilized
                    Some(false), // target_tank_level_switch_invalid
                    Some(true),  // target_surface_ventilation_status
                    Some(13.0),  // target_ambient_temperature
                    Some(14.0),  // target_ambient_humidity
                    Some(true),  // target_heater_status
                    Some(15.0),  // target DS18B20 water temperature
                    Some(16.0),  // target DS18B20 ambient temperature
                );

                // Dataframe #1
                data_frame = mock_data_injection.data_frames.pop().unwrap();
                println!("Asserting data frame #1");
                data_frame.assert(
                    Some(29.0),  // target_water_temperature
                    Some(17.0),  // target_water_temperature_filtered
                    Some(11.0),  // target_water_ph
                    Some(8.25),  // target_water_ph_filtered
                    Some(12.0),  // target_water_conductivity
                    Some(9.0),   // target_water_conductivity_filtered
                    None,        // target_water_conductivity_compensated
                    Some(false), // target_refill_in_progress
                    Some(true),  // target_tank_level_switch_position
                    Some(true),  // target_tank_level_switch_position_stabilized
                    Some(false), // target_tank_level_switch_invalid
                    Some(true),  // target_surface_ventilation_status
                    Some(13.0),  // target_ambient_temperature
                    Some(14.0),  // target_ambient_humidity
                    Some(false), // target_heater_status
                    Some(15.0),  // target DS18B20 water temperature
                    Some(16.0),  // target DS18B20 ambient temperature
                );

                // Dataframe #0
                data_frame = mock_data_injection.data_frames.pop().unwrap();
                println!("Asserting data frame #0");
                data_frame.assert(
                    Some(10.0),  // target_water_temperature
                    Some(5.0),   // target_water_temperature_filtered
                    Some(11.0),  // target_water_ph
                    Some(5.5),   // target_water_ph_filtered
                    Some(12.0),  // target_water_conductivity
                    Some(6.0),   // target_water_conductivity_filtered
                    None,        // target_water_conductivity_compensated
                    Some(false), // target_refill_in_progress
                    Some(true),  // target_tank_level_switch_position
                    Some(true),  // target_tank_level_switch_position_stabilized
                    Some(false), // target_tank_level_switch_invalid
                    Some(false), // target_surface_ventilation_status
                    Some(13.0),  // target_ambient_temperature
                    Some(14.0),  // target_ambient_humidity
                    Some(false), // target_heater_status
                    Some(15.0),  // target DS18B20 water temperature
                    Some(16.0),  // target DS18B20 ambient temperature
                );

                assert_output_file_contents(
                    &config2.data_logger, // data_logger_config,
                    29.0,                 // target_water_temperature
                    28.6,                 // target_water_temperature_filtered
                    7.0,                  // target_water_ph
                    7.04,                 // target_water_ph_filtered
                    50_000.0,             // target_water_conductivity
                    46876.0,              // target_water_conductivity_filtered
                    0.0,                  // target_water_conductivity_compensated
                    true,                 // target_refill_in_progress
                    true,                 // target_tank_level_switch_position
                    false,                // target_tank_level_switch_invalid
                    true,                 // target_tank_level_switch_position_stabilized
                    true,                 // target_surface_ventilation_status
                    35.0,                 // target_ambient_temperature
                    66.0,                 // target_ambient_humidity
                    true,                 // target_heater_status
                    15.5,                 // target DS18B20 water temperature
                    16.5,                 // target DS18B20 ambient temperature
                );
            })
            .unwrap();

        join_handle_mock_ventilation
            .join()
            .expect("Mock ventilation thread did not finish.");
        join_handle_mock_heating
            .join()
            .expect("Mock heating thread did not finish.");
        join_handle_mock_refill
            .join()
            .expect("Mock refill thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
        join_handle_test_object
            .join()
            .expect("Mock data logger thread did not finish.");
    }
}
