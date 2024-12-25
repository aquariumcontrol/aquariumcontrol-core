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
use std::time::{Duration, SystemTime};

use crate::channel_content::{AquariumSignal, InternalCommand};
use crate::data_injection::DataInjectionTrait;
use crate::iir_filter::{IIRFilter, IIRFilterConfig};
use crate::proc_ext_req::ProcessExternalRequestTrait;
use crate::sql_interface::SqlInterface;
use crate::sql_interface_data::{DataFrame, SqlInterfaceData};

const LOG_TARGET_DATA_LOGGER: &str = "data_logger";
const MILLIS_PER_SEC: u64 = 1000;

/// Holds the configuration data for the regular data logging to SQL database.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct DataLoggerConfig {
    /// switch indicating if self-generated timestamp or data-base timestamp shall be used
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

    /// output file name (in ramdisk) for current value of tank level switch position stabilized
    output_file_name_tank_level_switch_position_stabilized: String,

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

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to create timestamp
    lock_error_get_timestamp: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write refill in progress to disk
    lock_error_output_state_refill_in_progress: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write heating is on to disk
    lock_error_output_state_heating_is_on: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write ventilation is on to disk
    lock_error_output_state_ventilation_is_on: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write water temperature to disk
    lock_error_output_signal_water_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write pH to disk
    lock_error_output_signal_pH: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write conductivity to disk
    lock_error_output_signal_conductivity: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write ambient temperature to disk
    lock_error_output_signal_ambient_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write humidity to disk
    lock_error_output_signal_humidity: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write tank level switch position to disk
    lock_error_output_state_tank_level_switch_position: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write tank level switch invalid to disk
    lock_error_output_state_tank_level_switch_invalid: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write tank level switch position stabilized to disk
    lock_error_output_state_tank_level_switch_position_stabilized: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write temperature filtered to disk
    lock_error_output_signal_water_temperature_filtered: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write pH filtered to disk
    lock_error_output_signal_pH_filtered: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write conductivity filtered to disk
    lock_error_output_signal_conductivity_filtered: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to write timestamp to disk
    lock_error_output_timestamp: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command
    lock_warn_inapplicable_command: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via channel
    lock_error_channel_receive_termination: bool,
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
            }
        }

        Self {
            config: config,
            filter_water_temperature: filter_water_temperature,
            filter_pH: filter_pH,
            filter_conductivity: filter_conductivity,
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
        }
    }

    /// Outputs non-binary signal to file (in RAM disk).
    /// # Arguments
    /// * `signal` - signal which shall be converted to human readable format and written to file
    /// * `output_file_name` - name of file where the value shall be written to    
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

    /// Outputs string to file (in RAM disk).
    /// # Arguments
    /// * `data` - string data to be written to file
    /// * `output_file_name` - name of file where the value shall be written to    
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

    /// get information from refill/ventilation/heating thread
    /// * `signal` - reference to variable for storing control status information of refill/ventilation/heating
    /// * `signal_name` - enum indicating which signal shall be measured
    /// * `tx_to_on_off_thread` - reference to sender part of channel for communication to refill/ventilation/heating
    /// * `rx_from_on_off_thread` - reference to receiver part of channel for communication from refill/ventilation/heating
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
                    // now listen for response
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

    /// get information from AtlasScientific thread
    /// * `water_temperature` - reference to variable for storing signal
    /// * `ph` - reference to variable for storing signal
    /// * `conductivity` - reference to variable for storing signal
    /// * `atlas_scientific_channel_disconnected` - reference to variable for storing health status of channel
    /// * `tx_to_atlas_scientific` - reference to sender part of channel for communication to Atlas Scientific
    /// * `rx_from_atlas_scientific` - reference to receiver part of channel for communication from Atlas Scientific
    fn update_atlas_scientific_signals(
        &mut self,
        water_temperature: &mut Option<f32>,
        ph: &mut Option<f32>,
        conductivity: &mut Option<f32>,
        atlas_scientific_channel_disconnected: &mut bool,
        tx_to_atlas_scientific: &Sender<InternalCommand>,
        rx_from_atlas_scientific: &Receiver<f32>,
    ) {
        if !*atlas_scientific_channel_disconnected {
            #[cfg(feature = "debug_data_logger")]
            let start_time_atlas_scientific = SystemTime::now();

            // send request for water temperature
            match tx_to_atlas_scientific.send(InternalCommand::RequestSignal(
                AquariumSignal::WaterTemperature,
            )) {
                Ok(_) => {
                    // now listen for response
                    *water_temperature = match rx_from_atlas_scientific.recv() {
                        Ok(c) => {
                            self.lock_error_output_signal_water_temperature = self.output_signal(
                                c,
                                AquariumSignal::WaterTemperature,
                                &self.config.output_file_name_water_temperature,
                                self.lock_error_output_signal_water_temperature,
                            );
                            Some(c)
                        }
                        Err(e) => {
                            error!(
                                target: LOG_TARGET_DATA_LOGGER,
                                "receiving value from Atlas Scientific thread for water temperature failed ({e:?})"
                            );
                            *atlas_scientific_channel_disconnected = true;
                            None
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: LOG_TARGET_DATA_LOGGER,
                        "channel communication to Atlas Scientific thread when requesting water temperature failed ({e:?})"
                    );
                    *atlas_scientific_channel_disconnected = true;
                }
            }

            // send request for pH
            match tx_to_atlas_scientific.send(InternalCommand::RequestSignal(AquariumSignal::pH)) {
                Ok(_) => {
                    // now listen for response
                    *ph = match rx_from_atlas_scientific.recv() {
                        Ok(c) => {
                            self.lock_error_output_signal_pH = self.output_signal(
                                c,
                                AquariumSignal::pH,
                                &self.config.output_file_name_pH,
                                self.lock_error_output_signal_pH,
                            );
                            Some(c)
                        }
                        Err(e) => {
                            error!(
                                target: LOG_TARGET_DATA_LOGGER,
                                "receiving value from Atlas Scientific thread for pH failed ({e:?})"
                            );
                            *atlas_scientific_channel_disconnected = true;
                            None
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: LOG_TARGET_DATA_LOGGER,
                        "channel communication to Atlas Scientific thread when requesting pH failed ({e:?})"
                    );
                    *atlas_scientific_channel_disconnected = true;
                }
            }

            // send request for conductivity
            match tx_to_atlas_scientific
                .send(InternalCommand::RequestSignal(AquariumSignal::Conductivity))
            {
                Ok(_) => {
                    // now listen for response
                    *conductivity = match rx_from_atlas_scientific.recv() {
                        Ok(c) => {
                            self.lock_error_output_signal_conductivity = self.output_signal(
                                c,
                                AquariumSignal::Conductivity,
                                &self.config.output_file_name_conductivity,
                                self.lock_error_output_signal_conductivity,
                            );
                            Some(c)
                        }
                        Err(e) => {
                            *atlas_scientific_channel_disconnected = true;
                            error!(
                                target: LOG_TARGET_DATA_LOGGER,
                                "receiving value from Atlas Scientific thread for conductivity failed ({e:?})"
                            );
                            None
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: LOG_TARGET_DATA_LOGGER,
                        "channel communication to Atlas Scientific thread when requesting conductivity failed ({e:?})"
                    );
                    *atlas_scientific_channel_disconnected = true;
                }
            }
            #[cfg(feature = "debug_data_logger")]
            {
                let stop_time_atlas_scientific = SystemTime::now();
                let execution_duration_atlas_scientific_millis = stop_time_atlas_scientific
                    .duration_since(start_time_atlas_scientific)
                    .unwrap()
                    .as_millis()
                    as u64;
                debug!(
                    target: LOG_TARGET_DATA_LOGGER,
                    "communication with atlas_scientific took {} ms.",
                    execution_duration_atlas_scientific_millis
                );
            }
        }
    }

    /// get information from Ambient thread
    /// # Arguments
    /// * `ambient_temperature` - reference to variable for storing signal
    /// * `ambient_humidity` - reference to variable for storing signal
    /// * `ambient_channel_disconnected` - reference to variable for storing health status of channel
    /// * `tx_to_ambient` - reference to sender part of channel for communication to Ambient
    /// * `rx_from_ambient` - reference to receiver part of channel for communication from Ambient
    fn update_ambient_signals(
        &mut self,
        ambient_temperature: &mut Option<f32>,
        ambient_humidity: &mut Option<f32>,
        ambient_channel_disconnected: &mut bool,
        tx_to_ambient: &Sender<InternalCommand>,
        rx_from_ambient: &Receiver<f32>,
    ) {
        if !*ambient_channel_disconnected {
            #[cfg(feature = "debug_data_logger")]
            let start_time_ambient = SystemTime::now();

            // send request for ambient temperature
            match tx_to_ambient.send(InternalCommand::RequestSignal(
                AquariumSignal::AmbientTemperature,
            )) {
                Ok(_) => {
                    // now listen for response
                    *ambient_temperature = match rx_from_ambient.recv() {
                        Ok(c) => {
                            self.lock_error_output_signal_ambient_temperature = self.output_signal(
                                c,
                                AquariumSignal::AmbientTemperature,
                                &self.config.output_file_name_ambient_temperature,
                                self.lock_error_output_signal_ambient_temperature,
                            );
                            Some(c)
                        }
                        Err(e) => {
                            error!(
                                target: LOG_TARGET_DATA_LOGGER,
                                "receiving value from Ambient thread for temperature failed ({e:?})"
                            );
                            *ambient_channel_disconnected = true;
                            None
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: LOG_TARGET_DATA_LOGGER,
                        "channel communication to Ambient thread when requesting ambient temperature failed ({e:?})"
                    );
                    *ambient_channel_disconnected = true;
                }
            }

            // send request for ambient humidity
            match tx_to_ambient.send(InternalCommand::RequestSignal(
                AquariumSignal::AmbientHumidity,
            )) {
                Ok(_) => {
                    // now listen for response
                    *ambient_humidity = match rx_from_ambient.recv() {
                        Ok(c) => {
                            self.lock_error_output_signal_humidity = self.output_signal(
                                c,
                                AquariumSignal::AmbientHumidity,
                                &self.config.output_file_name_humidity,
                                self.lock_error_output_signal_humidity,
                            );
                            Some(c)
                        }
                        Err(e) => {
                            error!(
                                target: LOG_TARGET_DATA_LOGGER,
                                "receiving value from Ambient thread for humidity failed ({e:?})"
                            );
                            *ambient_channel_disconnected = true;
                            None
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: LOG_TARGET_DATA_LOGGER,
                        "channel communication to Ambient thread when requesting ambient humidity failed ({e:?})"
                    );
                    *ambient_channel_disconnected = true;
                }
            }
            #[cfg(feature = "debug_data_logger")]
            {
                let stop_time_ambient = SystemTime::now();
                let execution_duration_ambient_millis = stop_time_ambient
                    .duration_since(start_time_ambient)
                    .unwrap()
                    .as_millis() as u64;
                debug!(
                    target: LOG_TARGET_DATA_LOGGER,
                    "communication with ambient took {} ms.",
                    execution_duration_ambient_millis
                );
            }
        }
    }

    /// get information from TankLevelSwitch thread
    /// # Arguments
    /// * `tank_level_switch_position` - reference to variable for storing signal
    /// * `tank_level_switch_invalid` - reference to variable for storing signal
    /// * `tank_level_switch_channel_disconnected` - reference to variable for storing health status of channel
    /// * `tx_to_tank_level_switch` - reference to sender part of channel for communication to Tank Level Switch
    /// * `rx_from_tank_level_switch` - reference to receiver part of channel for communication from Tank Level Switch
    fn update_tank_level_switch_signals(
        &mut self,
        tank_level_switch_position: &mut Option<bool>,
        tank_level_switch_invalid: &mut Option<bool>,
        tank_level_switch_channel_disconnected: &mut bool,
        tx_to_tank_level_switch: &Sender<InternalCommand>,
        rx_from_tank_level_switch: &Receiver<bool>,
    ) {
        if !*tank_level_switch_channel_disconnected {
            #[cfg(feature = "debug_data_logger")]
            let start_time_tank_level_switch = SystemTime::now();

            // send request for tank level switch position
            match tx_to_tank_level_switch.send(InternalCommand::RequestSignal(
                AquariumSignal::TankLevelSwitchPosition,
            )) {
                Ok(_) => {
                    // now listen for response
                    *tank_level_switch_position = match rx_from_tank_level_switch.recv() {
                        Ok(c) => {
                            self.lock_error_output_state_tank_level_switch_position = self
                                .output_state(
                                    c,
                                    "HIGH",
                                    "LOW",
                                    &self.config.output_file_name_tank_level_switch_position,
                                    self.lock_error_output_state_tank_level_switch_position,
                                );
                            Some(c)
                        }
                        Err(e) => {
                            error!(
                                target: LOG_TARGET_DATA_LOGGER,
                                "receiving value from Tank Level Switch thread for position failed ({e:?})"
                            );
                            *tank_level_switch_channel_disconnected = true;
                            None
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: LOG_TARGET_DATA_LOGGER,
                        "channel communication to tank level switch thread failed when requesting tank level switch position ({e:?})"
                    );
                    *tank_level_switch_channel_disconnected = true;
                }
            }

            // send request for tank level switch invalid
            match tx_to_tank_level_switch.send(InternalCommand::RequestSignal(
                AquariumSignal::TankLevelSwitchInvalid,
            )) {
                Ok(_) => {
                    // now listen for response
                    *tank_level_switch_invalid = match rx_from_tank_level_switch.recv() {
                        Ok(c) => {
                            self.lock_error_output_state_tank_level_switch_invalid = self
                                .output_state(
                                    c,
                                    "INVALID",
                                    "VALID",
                                    &self.config.output_file_name_tank_level_switch_invalid,
                                    self.lock_error_output_state_tank_level_switch_invalid,
                                );
                            Some(c)
                        }
                        Err(e) => {
                            error!(
                                target: LOG_TARGET_DATA_LOGGER,
                                "receiving value from Tank Level Switch thread for validity failed ({e:?})"
                            );
                            *tank_level_switch_channel_disconnected = true;
                            None
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: LOG_TARGET_DATA_LOGGER,
                        "channel communication to tank level switch thread failed when requesting tank level switch invalid  ({e:?})"
                    );
                    *tank_level_switch_channel_disconnected = true;
                }
            }

            // send request for tank level switch stabilized position
            match tx_to_tank_level_switch.send(InternalCommand::RequestSignal(
                AquariumSignal::TankLevelSwitchPositionStabilized,
            )) {
                Ok(_) => {
                    // now listen for response
                    match rx_from_tank_level_switch.recv() {
                        Ok(c) => {
                            self.lock_error_output_state_tank_level_switch_position_stabilized = self
                                .output_state(
                                    c,
                                    "HIGH",
                                    "LOW",
                                    &self
                                        .config
                                        .output_file_name_tank_level_switch_position_stabilized,
                                    self.lock_error_output_state_tank_level_switch_position_stabilized,
                                );
                            Some(c)
                        }
                        Err(e) => {
                            error!(
                                target: LOG_TARGET_DATA_LOGGER,
                                "receiving value from Tank Level Switch thread for position stabilized failed ({e:?})"
                            );
                            *tank_level_switch_channel_disconnected = true;
                            None
                        }
                    };
                }
                Err(e) => {
                    error!(
                        target: LOG_TARGET_DATA_LOGGER,
                        "channel communication to tank level switch thread failed when requesting tank level switch position stabilized ({e:?})"
                    );
                    *tank_level_switch_channel_disconnected = true;
                }
            }

            #[cfg(feature = "debug_data_logger")]
            {
                let stop_time_tank_level_switch = SystemTime::now();
                let execution_duration_tank_level_switch_millis = stop_time_tank_level_switch
                    .duration_since(start_time_tank_level_switch)
                    .unwrap()
                    .as_millis()
                    as u64;
                debug!(
                    target: LOG_TARGET_DATA_LOGGER,
                    "communication with tank_level_switch took {} ms.",
                    execution_duration_tank_level_switch_millis
                );
            }
        }
    }

    /// Executes thread communication and filter calculation for all signals
    /// # Arguments
    /// * `tx_to_ambient` - reference to sender part of channel for communication to Ambient
    /// * `rx_from_ambient` - reference to receiver part of channel for communication from Ambient
    /// * `tx_to_atlas_scientific` - reference to sender part of channel for communication to Atlas Scientific
    /// * `rx_from_atlas_scientific` - reference to receiver part of channel for communication from Atlas Scientific
    /// * `tx_to_tank_level_switch` - reference to sender part of channel for communication to Tank Level Switch
    /// * `rx_from_tank_level_switch` - reference to receiver part of channel for communication from Tank Level Switch
    /// * `tx_to_refill` - reference to sender part of channel for communication to Refill
    /// * `rx_from_refill` - reference to receiver part of channel for communication from Refill
    /// * `rx_from_heating` - reference to sender part of channel for communication to Heating
    /// * `rx_from_heating` - reference to receiver part of channel for communication from Heating
    /// * `tx_to_ventilation` - reference to sender part of channel for communication to Ventilation
    /// * `rx_from_ventilation` - reference to receiver part of channel for communication from Ventilation
    /// * `refill_in_progress` - reference to variable for storing refill in progress information
    /// * `refill_channel_disconnected` - reference to variable for storing health status of channel
    /// * `heater_is_on` - reference to variable for storing heating control status information
    /// * `heating_channel_disconnected` - reference to variable for storing health status of channel
    /// * `ventilation_is_on` - reference to variable for storing ventilation control status information
    /// * `ventilation_channel_disconnected` - reference to variable for storing health status of channel
    /// * `water_temperature` - reference to variable for storing signal
    /// * `ph` - reference to variable for storing signal
    /// * `conductivity` - reference to variable for storing signal
    /// * `atlas_scientific_channel_disconnected` - reference to variable for storing health status of channel
    /// * `ambient_temperature` - reference to variable for storing signal
    /// * `ambient_humidity` - reference to variable for storing signal
    /// * `ambient_channel_disconnected` - reference to variable for storing health status of channel
    /// * `tank_level_switch_position` - reference to variable for storing signal
    /// * `tank_level_switch_invalid` - reference to variable for storing signal
    /// * `tank_level_switch_channel_disconnected` - reference to variable for storing health status of channel
    fn update_signals(
        &mut self,
        tx_to_ambient: &Sender<InternalCommand>,
        rx_from_ambient: &Receiver<f32>,
        tx_to_atlas_scientific: &Sender<InternalCommand>,
        rx_from_atlas_scientific: &Receiver<f32>,
        tx_to_tank_level_switch: &Sender<InternalCommand>,
        rx_from_tank_level_switch: &Receiver<bool>,
        tx_to_refill: &Sender<InternalCommand>,
        rx_from_refill: &Receiver<bool>,
        tx_to_heating: &Sender<InternalCommand>,
        rx_from_heating: &Receiver<bool>,
        tx_to_ventilation: &Sender<InternalCommand>,
        rx_from_ventilation: &Receiver<bool>,
        refill_in_progress: &mut bool,
        refill_channel_disconnected: &mut bool,
        heater_is_on: &mut bool,
        heating_channel_disconnected: &mut bool,
        ventilation_is_on: &mut bool,
        ventilation_channel_disconnected: &mut bool,
        water_temperature: &mut Option<f32>,
        ph: &mut Option<f32>,
        conductivity: &mut Option<f32>,
        atlas_scientific_channel_disconnected: &mut bool,
        ambient_temperature: &mut Option<f32>,
        ambient_humidity: &mut Option<f32>,
        ambient_channel_disconnected: &mut bool,
        tank_level_switch_position: &mut Option<bool>,
        tank_level_switch_invalid: &mut Option<bool>,
        tank_level_switch_channel_disconnected: &mut bool,
    ) {
        //*** communication with refill thread ***
        self.lock_error_output_state_refill_in_progress = self.update_on_off_signal(
            refill_in_progress,
            refill_channel_disconnected,
            AquariumSignal::RefillPumpStatus,
            tx_to_refill,
            rx_from_refill,
            self.lock_error_output_state_refill_in_progress,
        );

        //*** communication with heating thread ***
        self.lock_error_output_state_heating_is_on = self.update_on_off_signal(
            heater_is_on,
            heating_channel_disconnected,
            AquariumSignal::HeatingControlStatus,
            tx_to_heating,
            rx_from_heating,
            self.lock_error_output_state_heating_is_on,
        );

        //*** communication with ventilation thread ***
        self.lock_error_output_state_ventilation_is_on = self.update_on_off_signal(
            ventilation_is_on,
            ventilation_channel_disconnected,
            AquariumSignal::VentilationControlStatus,
            tx_to_ventilation,
            rx_from_ventilation,
            self.lock_error_output_state_ventilation_is_on,
        );

        //*** communication with Atlas Scientific thread ***
        self.update_atlas_scientific_signals(
            water_temperature,
            ph,
            conductivity,
            atlas_scientific_channel_disconnected,
            tx_to_atlas_scientific,
            rx_from_atlas_scientific,
        );

        //*** communication with Ambient thread ***
        self.update_ambient_signals(
            ambient_temperature,
            ambient_humidity,
            ambient_channel_disconnected,
            tx_to_ambient,
            rx_from_ambient,
        );

        //*** communication with Tank Level Switch thread ***
        self.update_tank_level_switch_signals(
            tank_level_switch_position,
            tank_level_switch_invalid,
            tank_level_switch_channel_disconnected,
            tx_to_tank_level_switch,
            rx_from_tank_level_switch,
        );
    }

    /// calculate filtered values and output them to (RAM disk) files
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

    /// get timestampe either from SQL server or application-generated
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

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// # Arguments
    /// * `data_injection` - Trait method for transferring data to database or mock data structure
    /// * `sql_interface_data` - Interface to SQL database for manipulation of data table
    /// * `tx_to_ambient` - Sender part of channel for communication to Ambient
    /// * `rx_from_ambient` - Receiver part of channel for communication from Ambient
    /// * `tx_to_atlas_scientific` - Sender part of channel for communication to Atlas Scientific
    /// * `rx_from_atlas_scientific` - Receiver part of channel for communication from Atlas Scientific
    /// * `tx_to_tank_level_switch` - Sender part of channel for communication to Tank Level Switch
    /// * `rx_from_tank_level_switch` - Receiver part of channel for communication from Tank Level Switch
    /// * `tx_to_refill` - Sender part of channel for communication to Refill
    /// * `rx_from_refill` - Receiver part of channel for communication from Refill
    /// * `rx_from_heating` - Sender part of channel for communication to Heating
    /// * `rx_from_heating` - Receiver part of channel for communication from Heating
    /// * `tx_to_ventilation` - Sender part of channel for communication to Ventilation
    /// * `rx_from_ventilation` - Receiver part of channel for communication from Ventilation
    /// * `tx_to_signal_handler` - Sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of channel for communication from signal handler                
    pub fn execute(
        &mut self,
        data_injection: &mut impl DataInjectionTrait,
        mut sql_interface_data: SqlInterfaceData,
        tx_to_ambient: Sender<InternalCommand>,
        rx_from_ambient: Receiver<f32>,
        tx_to_atlas_scientific: Sender<InternalCommand>,
        rx_from_atlas_scientific: Receiver<f32>,
        tx_to_tank_level_switch: Sender<InternalCommand>,
        rx_from_tank_level_switch: Receiver<bool>,
        tx_to_refill: Sender<InternalCommand>,
        rx_from_refill: Receiver<bool>,
        tx_to_heating: Sender<InternalCommand>,
        rx_from_heating: Receiver<bool>,
        tx_to_ventilation: Sender<InternalCommand>,
        rx_from_ventilation: Receiver<bool>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        let sleep_duration_hundred_millisec = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut i;

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
        let mut refill_channel_disconnected = false;
        let mut heating_channel_disconnected = false;
        let mut ventilation_channel_disconnected = false;
        let mut tank_level_switch_channel_disconnected = false;
        let mut atlas_scientific_channel_disconnected = false;
        let mut ambient_channel_disconnected = false;
        let mut start_time = SystemTime::now();
        let mut lock_warn_cycle_time_exceeded = false;

        loop {
            self.update_signals(
                &tx_to_ambient,
                &rx_from_ambient,
                &tx_to_atlas_scientific,
                &rx_from_atlas_scientific,
                &tx_to_tank_level_switch,
                &rx_from_tank_level_switch,
                &tx_to_refill,
                &rx_from_refill,
                &tx_to_heating,
                &rx_from_heating,
                &tx_to_ventilation,
                &rx_from_ventilation,
                &mut refill_in_progress,
                &mut refill_channel_disconnected,
                &mut heater_is_on,
                &mut heating_channel_disconnected,
                &mut ventilation_is_on,
                &mut ventilation_channel_disconnected,
                &mut water_temperature,
                &mut ph,
                &mut conductivity,
                &mut atlas_scientific_channel_disconnected,
                &mut ambient_temperature,
                &mut ambient_humidity,
                &mut ambient_channel_disconnected,
                &mut tank_level_switch_position,
                &mut tank_level_switch_invalid,
                &mut tank_level_switch_channel_disconnected,
            );

            let (water_temperature_filtered, pH_filtered, conductivity_filtered) =
                self.calculate_filtered_signals(water_temperature, ph, conductivity);

            let current_timestamp = self.update_timestamp(&mut sql_interface_data.conn);

            // all data received, let's construct a data frame
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
            } else {
                remaining_sleep_time_millis =
                    self.config.logging_interval * MILLIS_PER_SEC - execution_duration_millis;
            }

            #[cfg(feature = "debug_data_logger")]
            debug!(
                target: LOG_TARGET_DATA_LOGGER,
                "inserted data into database, now idling for {} milliseconds",
                remaining_sleep_time_millis
            );

            // wait the remaining time period to complete the target cycle time
            i = 0;
            while i < remaining_sleep_time_millis {
                i += 100;
                spin_sleeper.sleep(sleep_duration_hundred_millisec);
                (quit_command_received, _, _) =
                    self.process_external_request(&rx_from_signal_handler, None);
                if quit_command_received {
                    #[cfg(feature = "debug_data_logger")]
                    debug!(
                        target: LOG_TARGET_DATA_LOGGER,
                        "received QUIT command from signal handler"
                    );
                    break;
                }

                // once per second, retrieve signals and write them to RAM-disk
                if i % 1000 == 0 {
                    self.update_signals(
                        &tx_to_ambient,
                        &rx_from_ambient,
                        &tx_to_atlas_scientific,
                        &rx_from_atlas_scientific,
                        &tx_to_tank_level_switch,
                        &rx_from_tank_level_switch,
                        &tx_to_refill,
                        &rx_from_refill,
                        &tx_to_heating,
                        &rx_from_heating,
                        &tx_to_ventilation,
                        &rx_from_ventilation,
                        &mut refill_in_progress,
                        &mut refill_channel_disconnected,
                        &mut heater_is_on,
                        &mut heating_channel_disconnected,
                        &mut ventilation_is_on,
                        &mut ventilation_channel_disconnected,
                        &mut water_temperature,
                        &mut ph,
                        &mut conductivity,
                        &mut atlas_scientific_channel_disconnected,
                        &mut ambient_temperature,
                        &mut ambient_humidity,
                        &mut ambient_channel_disconnected,
                        &mut tank_level_switch_position,
                        &mut tank_level_switch_invalid,
                        &mut tank_level_switch_channel_disconnected,
                    );
                }
            }
            if quit_command_received {
                break;
            }

            #[cfg(feature = "debug_data_logger")]
            debug!(
                target: LOG_TARGET_DATA_LOGGER,
                "finished idling restarting loop"
            );

            start_time = SystemTime::now(); // start time for next cycle
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_DATA_LOGGER,
                    "sending confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // this thread has channel connection to underlying threads.
        // These threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by signal_handler crate.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match rx_from_signal_handler.recv() {
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
            spin_sleeper.sleep(sleep_duration_hundred_millisec);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use spin_sleep::SpinSleeper;
    use std::{fs, sync::mpsc, thread, time::Duration};

    use crate::channel_content::InternalCommand;
    use crate::config::{read_config_file, ConfigData};
    use crate::data_logger::{DataLogger, DataLoggerConfig};
    use crate::mock::tests::{
        mock_ambient, mock_atlas_scientific, mock_heating, mock_refill, mock_tank_level_switch,
        mock_ventilation,
    };
    use crate::mock::tests::{MockDataInjection, MockSignal, TestCommand};
    use crate::sql_interface::SqlInterface;
    use crate::sql_interface_data::SqlInterfaceData;

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
    }

    #[test]
    // test case checks if DataLogger correctly processes all information:
    // - storage in SQL database using mock implementation
    // - writing output to filesystem
    // Test case does not require any communication with database.
    pub fn test_data_log() {
        // channels for communication with test environment
        let (tx_data_logger_to_ambient, rx_ambient_from_data_logger) = mpsc::channel();
        let (tx_ambient_to_data_logger, rx_data_logger_from_ambient) = mpsc::channel();
        let (tx_data_logger_to_atlas_scientific, rx_atlas_scientific_from_data_logger) =
            mpsc::channel();
        let (tx_atlas_scientific_to_data_logger, rx_data_logger_from_atlas_scientific) =
            mpsc::channel();
        let (tx_data_logger_to_tank_level_switch, rx_tank_level_switch_from_data_logger) =
            mpsc::channel();
        let (tx_tank_level_switch_to_data_logger, rx_data_logger_from_tank_level_switch) =
            mpsc::channel();
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
        let (tx_test_environment_to_tank_level_switch, rx_tank_level_switch_from_test_environment) =
            mpsc::channel();
        let (tx_test_environment_to_ventilation, rx_ventilation_from_test_environment) =
            mpsc::channel();
        let (tx_test_environment_to_heating, rx_heating_from_test_environment) = mpsc::channel();
        let (tx_test_environment_to_refill, rx_refill_from_test_environment) = mpsc::channel();
        let (tx_test_environment_to_atlas_scientific, rx_atlas_scientific_from_test_environment) =
            mpsc::channel();
        let (tx_test_environment_to_ambient, rx_ambient_from_test_environment) = mpsc::channel();

        let tx_signal_handler_to_tank_level_switch = tx_data_logger_to_tank_level_switch.clone();
        let tx_signal_handler_to_ambient = tx_data_logger_to_ambient.clone();
        let tx_signal_handler_to_atlas_scientific = tx_data_logger_to_atlas_scientific.clone();

        let mut mock_data_injection = MockDataInjection::new();

        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.data_logger.logging_interval = 1;
        let mut config2: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config2.data_logger.logging_interval = 1;
        let sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_data = SqlInterfaceData::new(sql_interface.get_connection());
        let mut data_logger = DataLogger::new(config.data_logger);

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
                let _ = tx_test_environment_to_atlas_scientific.send(TestCommand::UpdateSignal(
                    MockSignal::WaterTemperature(29.0),
                ));

                spin_sleeper.sleep(sleep_duration_1_second);

                let _ = tx_test_environment_to_heating.send(TestCommand::UpdateSignal(
                    MockSignal::HeatingControlStatus(true),
                ));
                let _ = tx_test_environment_to_atlas_scientific
                    .send(TestCommand::UpdateSignal(MockSignal::pH(7.0)));

                spin_sleeper.sleep(sleep_duration_1_second);

                let _ = tx_test_environment_to_refill.send(TestCommand::UpdateSignal(
                    MockSignal::RefillPumpStatus(true),
                ));
                let _ = tx_test_environment_to_atlas_scientific
                    .send(TestCommand::UpdateSignal(MockSignal::Conductivity(50000.0)));

                spin_sleeper.sleep(sleep_duration_1_second);

                let _ = tx_test_environment_to_tank_level_switch.send(TestCommand::UpdateSignal(
                    MockSignal::TankLevelSwitchPosition(false),
                ));
                let _ = tx_test_environment_to_ambient.send(TestCommand::UpdateSignal(
                    MockSignal::AmbientTemperature(35.0),
                ));

                spin_sleeper.sleep(sleep_duration_1_second);

                let _ = tx_test_environment_to_tank_level_switch.send(TestCommand::UpdateSignal(
                    MockSignal::TankLevelSwitchInvalid(true),
                ));
                let _ = tx_test_environment_to_ambient
                    .send(TestCommand::UpdateSignal(MockSignal::AmbientHumidity(66.0)));

                spin_sleeper.sleep(sleep_duration_1_second);
                spin_sleeper.sleep(sleep_duration_1_second);

                // cease operation of test object
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Quit);
                rx_signal_handler_from_data_logger.recv().unwrap();

                // signal end of test case execution to mock threads
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_ambient.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_atlas_scientific.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_ventilation.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
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

        // thread for mock TankLevelSwitch
        let join_handle_mock_tank_level_switch = thread::Builder::new()
            .name("mock_tank_level_switch".to_string())
            .spawn(move || {
                mock_tank_level_switch(
                    &tx_tank_level_switch_to_data_logger,
                    &rx_tank_level_switch_from_data_logger,
                    Some(&rx_tank_level_switch_from_test_environment),
                    Some(&tx_tank_level_switch_to_data_logger),
                    Some(&rx_tank_level_switch_from_data_logger),
                    true,  // initial tank level switch position
                    false, // initial tank level switch invalid
                    true,  // initial tank level switch position stabilized
                );
            })
            .unwrap();

        // thread for mock Atlas Scientific
        let join_handle_mock_atlas_scientific = thread::Builder::new()
            .name("mock_atlas_scientific".to_string())
            .spawn(move || {
                mock_atlas_scientific(
                    &tx_atlas_scientific_to_data_logger,
                    &rx_atlas_scientific_from_data_logger,
                    Some(&rx_atlas_scientific_from_test_environment),
                    10.0, // initial water temperature
                    12.0, // initial conductivity
                    11.0, // initial pH
                );
            })
            .unwrap();

        // thread for mock Ambient
        let join_handle_mock_ambient = thread::Builder::new()
            .name("mock_ambient".to_string())
            .spawn(move || {
                mock_ambient(
                    &tx_ambient_to_data_logger,
                    &rx_ambient_from_data_logger,
                    Some(&rx_ambient_from_test_environment),
                    13.0, // initial ambient temperature
                    14.0, // initial ambient humidity
                );
            })
            .unwrap();

        // give mock threads some time to establish
        let spin_sleeper = SpinSleeper::default();
        let sleep_duration_100_millis = Duration::from_millis(100);
        spin_sleeper.sleep(sleep_duration_100_millis);

        // thread for test object and asserts
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                data_logger.execute(
                    &mut mock_data_injection,
                    sql_interface_data,
                    tx_data_logger_to_ambient,
                    rx_data_logger_from_ambient,
                    tx_data_logger_to_atlas_scientific,
                    rx_data_logger_from_atlas_scientific,
                    tx_data_logger_to_tank_level_switch,
                    rx_data_logger_from_tank_level_switch,
                    tx_data_logger_to_refill,
                    rx_data_logger_from_refill,
                    tx_data_logger_to_heating,
                    rx_data_logger_from_heating,
                    tx_data_logger_to_ventilation,
                    rx_data_logger_from_ventilation,
                    tx_data_logger_to_signal_handler,
                    rx_data_logger_from_signal_handler,
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
                    Some(false),     // target_tank_level_switch_position
                    Some(true),      // target_tank_level_switch_invalid
                    Some(true),      // target_surface_ventilation_status
                    Some(35.0),      // target_ambient_temperature
                    Some(66.0),      // target_ambient_humidity
                    Some(true),      // target_heater_status
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
                    Some(false),     // target_tank_level_switch_position
                    Some(true),      // target_tank_level_switch_invalid
                    Some(true),      // target_surface_ventilation_status
                    Some(35.0),      // target_ambient_temperature
                    Some(66.0),      // target_ambient_humidity
                    Some(true),      // target_heater_status
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
                    Some(false),     // target_tank_level_switch_position
                    Some(false),     // target_tank_level_switch_invalid
                    Some(true),      // target_surface_ventilation_status
                    Some(35.0),      // target_ambient_temperature
                    Some(14.0),      // target_ambient_humidity
                    Some(true),      // target_heater_status
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
                    Some(false),    // target_tank_level_switch_invalid
                    Some(true),     // target_surface_ventilation_status
                    Some(13.0),     // target_ambient_temperature
                    Some(14.0),     // target_ambient_humidity
                    Some(true),     // target_heater_status
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
                    Some(false), // target_tank_level_switch_invalid
                    Some(true),  // target_surface_ventilation_status
                    Some(13.0),  // target_ambient_temperature
                    Some(14.0),  // target_ambient_humidity
                    Some(true),  // target_heater_status
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
                    Some(false), // target_tank_level_switch_invalid
                    Some(true),  // target_surface_ventilation_status
                    Some(13.0),  // target_ambient_temperature
                    Some(14.0),  // target_ambient_humidity
                    Some(false), // target_heater_status
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
                    Some(false), // target_tank_level_switch_invalid
                    Some(false), // target_surface_ventilation_status
                    Some(13.0),  // target_ambient_temperature
                    Some(14.0),  // target_ambient_humidity
                    Some(false), // target_heater_status
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
                    false,                // target_tank_level_switch_position
                    true,                 // target_tank_level_switch_invalid
                    true,                 // target_tank_level_switch_position_stabilized
                    true,                 // target_surface_ventilation_status
                    35.0,                 // target_ambient_temperature
                    66.0,                 // target_ambient_humidity
                    true,                 // target_heater_status
                );
            })
            .unwrap();

        join_handle_mock_tank_level_switch
            .join()
            .expect("Mock tank level switch thread did not finish.");
        join_handle_mock_ventilation
            .join()
            .expect("Mock ventilation thread did not finish.");
        join_handle_mock_heating
            .join()
            .expect("Mock heating thread did not finish.");
        join_handle_mock_atlas_scientific
            .join()
            .expect("Mock Atlas Scientific thread did not finish.");
        join_handle_mock_ambient
            .join()
            .expect("Mock Ambient thread did not finish.");
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
