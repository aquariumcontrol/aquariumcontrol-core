/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#![allow(non_snake_case)]
use chrono::Local;
use chrono::NaiveDateTime;
use log::{error, warn};
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::fs;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::Duration;

use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;
use crate::data_injection::DataInjectionTrait;
use crate::iir_filter::IIRFilter;
use crate::iir_filter::IIRFilterConfig;
use crate::proc_ext_req::ProcessExternalRequestTrait;
use crate::sql_interface::SqlInterface;
use crate::sql_interface_data::DataFrame;
use crate::sql_interface_data::SqlInterfaceData;

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
    const LOG_TARGET_DATA_LOGGER: &'static str = "data_logger";

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
                || config.output_file_name_ventilation_is_on.is_empty()
                || config.output_file_name_heating_is_on.is_empty()
                || config.output_file_name_ambient_temperature.is_empty()
                || config.output_file_name_humidity.is_empty()
                || config.output_file_name_refill_in_progress.is_empty()
            {
                panic!(
                    "{}: one of the output file names is empty.",
                    Self::LOG_TARGET_DATA_LOGGER
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
            let signal_string = match signal_type {
                AquariumSignal::WaterTemperature => format!("{:.1}", signal),
                AquariumSignal::pH => format!("{:.2}", signal),
                AquariumSignal::Conductivity => format!("{:.0}", signal),
                AquariumSignal::AmbientTemperature => format!("{:.1}", signal),
                AquariumSignal::AmbientHumidity => format!("{:.1}", signal),
                _ => signal.to_string(),
            };

            match fs::write(output_file_name, signal_string.clone()) {
                Ok(_) => {
                    /* do nothing */
                    false // indicate no error
                }
                Err(e) => {
                    if !inhibit_error_log {
                        error!(
                            target: Self::LOG_TARGET_DATA_LOGGER,
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
                            target: Self::LOG_TARGET_DATA_LOGGER,
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
                            target: Self::LOG_TARGET_DATA_LOGGER,
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
        data_injection: &mut impl DataInjectionTrait,
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
        let mut refill_channel_disconnected = false;
        let mut heating_channel_disconnected = false;
        let mut ventilation_channel_disconnected = false;
        let mut gpio_channel_disconnected = false;

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

            // get information if refill is in progress from refill thread
            if !refill_channel_disconnected {
                match rx_from_refill.try_recv() {
                    Ok(c) => {
                        refill_in_progress = c;
                        self.lock_error_output_state_refill_in_progress = self.output_state(
                            c,
                            "ON",
                            "OFF",
                            &self.config.output_file_name_refill_in_progress,
                            self.lock_error_output_state_refill_in_progress,
                        );
                    }
                    Err(e) => {
                        match e {
                            TryRecvError::Empty => { /* empty buffer - no action required */ }
                            TryRecvError::Disconnected => {
                                error!(
                                    target: Self::LOG_TARGET_DATA_LOGGER,
                                    "receiving command from refill thread failed ({e:?})"
                                );
                                refill_channel_disconnected = true;
                            }
                        }
                    }
                }
            }

            // get information if heater is on from heating thread
            if !heating_channel_disconnected {
                match rx_from_heating.try_recv() {
                    Ok(c) => {
                        heater_is_on = c;
                        self.lock_error_output_state_heating_is_on = self.output_state(
                            c,
                            "ON",
                            "OFF",
                            &self.config.output_file_name_heating_is_on,
                            self.lock_error_output_state_heating_is_on,
                        );
                    }
                    Err(e) => {
                        match e {
                            TryRecvError::Empty => { /* empty buffer - no action required */ }
                            TryRecvError::Disconnected => {
                                error!(
                                    target: Self::LOG_TARGET_DATA_LOGGER,
                                    "receiving command from heating thread failed ({e:?})"
                                );
                                heating_channel_disconnected = true;
                            }
                        }
                    }
                }
            }

            // get information if ventilation is on from ventilation thread
            if !ventilation_channel_disconnected {
                match rx_from_ventilation.try_recv() {
                    Ok(c) => {
                        ventilation_is_on = c;
                        self.lock_error_output_state_ventilation_is_on = self.output_state(
                            c,
                            "ON",
                            "OFF",
                            &self.config.output_file_name_ventilation_is_on,
                            self.lock_error_output_state_ventilation_is_on,
                        );
                    }
                    Err(e) => {
                        match e {
                            TryRecvError::Empty => { /* empty buffer - no action required */ }
                            TryRecvError::Disconnected => {
                                error!(
                                    target: Self::LOG_TARGET_DATA_LOGGER,
                                    "receiving command from ventilation thread failed ({e:?})"
                                );
                                ventilation_channel_disconnected = true;
                            }
                        }
                    }
                }
            }

            if !gpio_channel_disconnected {
                // send request for water temperature
                match tx_to_gpio.send(InternalCommand::RequestSignal(
                    AquariumSignal::WaterTemperature,
                )) {
                    Ok(_) => {
                        // now listen for response
                        water_temperature = match rx_from_gpio.recv() {
                            Ok(c) => {
                                self.lock_error_output_signal_water_temperature = self
                                    .output_signal(
                                        c,
                                        AquariumSignal::WaterTemperature,
                                        &self.config.output_file_name_water_temperature,
                                        self.lock_error_output_signal_water_temperature,
                                    );
                                Some(c)
                            }
                            Err(_) => {
                                gpio_channel_disconnected = true;
                                None
                            }
                        };
                    }
                    Err(e) => {
                        error!(
                            target: Self::LOG_TARGET_DATA_LOGGER,
                            "channel communication to GPIO handler when requesting water temperature failed ({e:?})"
                        );
                        gpio_channel_disconnected = true;
                    }
                }

                // send request for pH
                match tx_to_gpio.send(InternalCommand::RequestSignal(AquariumSignal::pH)) {
                    Ok(_) => {
                        // now listen for response
                        ph = match rx_from_gpio.recv() {
                            Ok(c) => {
                                self.lock_error_output_signal_pH = self.output_signal(
                                    c,
                                    AquariumSignal::pH,
                                    &self.config.output_file_name_pH,
                                    self.lock_error_output_signal_pH,
                                );
                                Some(c)
                            }
                            Err(_) => {
                                gpio_channel_disconnected = true;
                                None
                            }
                        };
                    }
                    Err(e) => {
                        error!(
                            target: Self::LOG_TARGET_DATA_LOGGER,
                            "channel communication to GPIO handler when requesting pH failed ({e:?})"
                        );
                        gpio_channel_disconnected = true;
                    }
                }

                // send request for conductivity
                match tx_to_gpio.send(InternalCommand::RequestSignal(AquariumSignal::Conductivity))
                {
                    Ok(_) => {
                        // now listen for response
                        conductivity = match rx_from_gpio.recv() {
                            Ok(c) => {
                                self.lock_error_output_signal_conductivity = self.output_signal(
                                    c,
                                    AquariumSignal::Conductivity,
                                    &self.config.output_file_name_conductivity,
                                    self.lock_error_output_signal_conductivity,
                                );
                                Some(c)
                            }
                            Err(_) => {
                                gpio_channel_disconnected = true;
                                None
                            }
                        };
                    }
                    Err(e) => {
                        error!(
                            target: Self::LOG_TARGET_DATA_LOGGER,
                            "channel communication to GPIO handler when requesting conductivity failed ({e:?})"
                        );
                        gpio_channel_disconnected = true;
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
                                self.lock_error_output_signal_ambient_temperature = self
                                    .output_signal(
                                        c,
                                        AquariumSignal::AmbientTemperature,
                                        &self.config.output_file_name_ambient_temperature,
                                        self.lock_error_output_signal_ambient_temperature,
                                    );
                                Some(c)
                            }
                            Err(_) => {
                                gpio_channel_disconnected = true;
                                None
                            }
                        };
                    }
                    Err(e) => {
                        error!(
                            target: Self::LOG_TARGET_DATA_LOGGER,
                            "channel communication to GPIO handler when requesting ambient temperature failed ({e:?})"
                        );
                        gpio_channel_disconnected = true;
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
                                self.lock_error_output_signal_humidity = self.output_signal(
                                    c,
                                    AquariumSignal::AmbientHumidity,
                                    &self.config.output_file_name_humidity,
                                    self.lock_error_output_signal_humidity,
                                );
                                Some(c)
                            }
                            Err(_) => {
                                gpio_channel_disconnected = true;
                                None
                            }
                        };
                    }
                    Err(e) => {
                        error!(
                            target: Self::LOG_TARGET_DATA_LOGGER,
                            "channel communication to GPIO handler when requesting ambient humidity failed ({e:?})"
                        );
                        gpio_channel_disconnected = true;
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
                                self.lock_error_output_state_tank_level_switch_position = self
                                    .output_state(
                                        c > 0.0,
                                        "HIGH",
                                        "LOW",
                                        &self.config.output_file_name_tank_level_switch_position,
                                        self.lock_error_output_state_tank_level_switch_position,
                                    );
                                Some(c > 0.0)
                            }
                            Err(_) => {
                                gpio_channel_disconnected = true;
                                None
                            }
                        };
                    }
                    Err(e) => {
                        error!(
                            target: Self::LOG_TARGET_DATA_LOGGER,
                            "channel communication to GPIO handler when requesting tank level switch position failed ({e:?})"
                        );
                        gpio_channel_disconnected = true;
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
                                self.lock_error_output_state_tank_level_switch_invalid = self
                                    .output_state(
                                        c > 0.0,
                                        "INVALID",
                                        "VALID",
                                        &self.config.output_file_name_tank_level_switch_invalid,
                                        self.lock_error_output_state_tank_level_switch_invalid,
                                    );
                                Some(c > 0.0)
                            }
                            Err(_) => {
                                gpio_channel_disconnected = true;
                                None
                            }
                        };
                    }
                    Err(e) => {
                        error!(
                            target: Self::LOG_TARGET_DATA_LOGGER,
                            "channel communication to GPIO handler when requesting tank level switch invalid failed ({e:?})"
                        );
                        gpio_channel_disconnected = true;
                    }
                }
            }

            // calculate filtered values and output them to (RAM disk) files
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
                        AquariumSignal::WaterTemperature,
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

            // get timestampe either from SQL server or application-generated
            let current_timestamp = match self.config.use_sql_timestamp {
                true => match SqlInterface::get_current_timestamp(&mut sql_interface_data.conn) {
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
                                target: Self::LOG_TARGET_DATA_LOGGER,
                                "could not retrieve timestamp from database: ({e:?}) - using application-generated timestamp."
                            );
                            self.lock_error_get_timestamp = true;
                        }
                        Local::now().naive_local()
                    }
                },
                false => Local::now().naive_local(),
            };

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
            //println!("DateFrame = {}", data_frame);
            data_injection.inject_data(data_frame, &mut sql_interface_data);
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_DATA_LOGGER,
                    "sending confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // this thread has channel connection to underlying threads (GpioHandler).
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
                                target: Self::LOG_TARGET_DATA_LOGGER,
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
                            target: Self::LOG_TARGET_DATA_LOGGER,
                            "receiving termination signal from signal handler failed ({e:?})"
                        );
                        self.lock_error_channel_receive_termination = true;
                    }
                    false
                }
            };
            spin_sleeper.sleep(sleep_duration_one_millisec);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use spin_sleep::SpinSleeper;
    use std::fs;
    use std::sync::mpsc;
    use std::sync::mpsc::TryRecvError;
    use std::thread::scope;
    use std::time::Duration;

    use crate::channel_content::AquariumSignal;
    use crate::channel_content::InternalCommand;
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::data_logger::DataInjectionTrait;
    use crate::data_logger::DataLogger;
    use crate::sql_interface::SqlInterface;
    use crate::sql_interface_data::DataFrame;
    use crate::sql_interface_data::SqlInterfaceData;

    pub struct MockDataInjection {
        pub data_frames: Vec<DataFrame>,
    }

    impl MockDataInjection {
        pub fn new() -> MockDataInjection {
            MockDataInjection {
                data_frames: Vec::new(),
            }
        }

        pub fn output(&self) {
            if self.data_frames.is_empty() {
                println!("MockDataInjection has not recorded any data frames.");
            }
            for (position, data_frame) in self.data_frames.iter().enumerate() {
                println!("Dataframe #{}: {}", position, data_frame);
            }
        }
    }

    impl DataInjectionTrait for MockDataInjection {
        fn inject_data(&mut self, data_frame: DataFrame, _: &mut SqlInterfaceData) {
            println!(
                "MockDataInjection.inject_data: recording data frame: {}",
                data_frame
            );

            // add data frame to vector
            self.data_frames.push(data_frame);
        }
    }

    #[test]
    pub fn test_data_log() {
        let spin_sleeper = SpinSleeper::default();
        let sleep_duration_8_seconds = Duration::from_millis(8000);
        let (tx_data_logger_to_gpio, rx_gpio_from_data_logger) = mpsc::channel();
        let (tx_gpio_to_data_logger, rx_data_logger_from_gpio) = mpsc::channel();
        let (tx_refill_to_data_logger, rx_data_logger_from_refill) = mpsc::channel();
        let (tx_heating_to_data_logger, rx_data_logger_from_heating) = mpsc::channel();
        let (tx_ventilation_to_data_logger, rx_data_logger_from_ventilation) = mpsc::channel();
        let (tx_data_logger_to_signal_handler, rx_signal_handler_from_data_logger) =
            mpsc::channel();
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler) =
            mpsc::channel();

        let mut mock_data_injection = MockDataInjection::new();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_data_logger.toml".to_string());
        let config2: ConfigData =
            read_config_file("/config/aquarium_control_test_data_logger.toml".to_string());
        let sql_interface = match SqlInterface::new(config.sql_interface.clone()) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not connect to SQL database: {e:?}");
            }
        };
        let sql_interface_data = SqlInterfaceData::new(sql_interface.get_connection());
        let mut data_logger = DataLogger::new(config.data_logger);
        scope(|scope| {
            scope.spawn(move || {
                // mock signal handler
                spin_sleeper.sleep(sleep_duration_8_seconds);
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Quit);
                rx_signal_handler_from_data_logger.recv().unwrap();
                let _ = tx_signal_handler_to_data_logger.send(InternalCommand::Terminate);
            });

            scope.spawn(move || {
                // mock refill control
                for i in 0..7 {
                    let _ = tx_refill_to_data_logger.send(i % 2 > 0);
                }
            });

            scope.spawn(move || {
                // mock heating
                for i in 0..7 {
                    let _ = tx_heating_to_data_logger.send(i % 2 > 0);
                }
            });

            scope.spawn(move || {
                // mock ventilation
                for i in 0..7 {
                    let _ = tx_ventilation_to_data_logger.send(i % 2 > 0);
                }
            });

            scope.spawn(move || {
                // mock gpio
                let mut toggle_bit_position = true;
                let mut toggle_bit_invalid = false;
                loop {
                    match rx_gpio_from_data_logger.try_recv() {
                        Ok(c) => {
                            let response_value = match c {
                                InternalCommand::RequestSignal(d) => {
                                    match d {
                                        AquariumSignal::WaterTemperature => 10.0,
                                        AquariumSignal::pH => 11.0,
                                        AquariumSignal::Conductivity => 12.0,
                                        AquariumSignal::AmbientTemperature => 13.0,
                                        AquariumSignal::AmbientHumidity => 14.0,
                                        AquariumSignal::TankLevelSwitchPosition => {
                                            toggle_bit_position = !toggle_bit_position;
                                            match toggle_bit_position { true => 1.0, false => 0.0 }
                                        },
                                        AquariumSignal::TankLevelSwitchInvalid => {
                                            toggle_bit_invalid = !toggle_bit_invalid;
                                            match toggle_bit_invalid { true => 1.0, false => 0.0 }
                                        },
                                    }
                                },
                                InternalCommand::Quit => {
                                    break;
                                }
                                _ => 19.0,
                            };
                            match tx_gpio_to_data_logger.send(response_value) {
                                Ok(()) => {}
                                Err(e) => {
                                    println!(
                                        "Error in trying to send mock response value back to data logger: {e:?}"
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!("Mock GpioHandler: Error in trying to receive command from data logger: {e:?}");
                                }
                            }
                        }
                    }
                }
            });

            scope.spawn(move || {
                // test object
                data_logger.execute(
                    &mut mock_data_injection,
                    sql_interface_data,
                    tx_data_logger_to_gpio.clone(),
                    rx_data_logger_from_gpio,
                    rx_data_logger_from_refill,
                    rx_data_logger_from_heating,
                    rx_data_logger_from_ventilation,
                    tx_data_logger_to_signal_handler,
                    rx_data_logger_from_signal_handler,
                );

                // signal end of test case execution to mock GPIO handler
                let _ = tx_data_logger_to_gpio.send(InternalCommand::Quit);

                println!("Printing recorded data:");
                mock_data_injection.output();

                let last_recorded_data_frame = mock_data_injection.data_frames.pop().unwrap();

                assert_eq!(last_recorded_data_frame.water_temperature, Some(10.0));
                assert_eq!(
                    last_recorded_data_frame.water_temperature_filtered,
                    Some(9.921875)
                );
                assert_eq!(last_recorded_data_frame.ph, Some(11.0));
                assert_eq!(last_recorded_data_frame.ph_filtered, Some(10.9140625));
                assert_eq!(last_recorded_data_frame.conductivity, Some(12.0));
                assert_eq!(
                    last_recorded_data_frame.conductivity_filtered,
                    Some(11.90625)
                );
                assert_eq!(last_recorded_data_frame.refill_in_progress, Some(false));
                assert_eq!(
                    last_recorded_data_frame.tank_level_switch_position,
                    Some(false)
                );
                assert_eq!(
                    last_recorded_data_frame.tank_level_switch_invalid,
                    Some(true)
                );
                assert_eq!(
                    last_recorded_data_frame.surface_ventilation_status,
                    Some(false)
                );
                assert_eq!(last_recorded_data_frame.ambient_temperature, Some(13.0));
                assert_eq!(last_recorded_data_frame.ambient_humidity, Some(14.0));
                assert_eq!(last_recorded_data_frame.heater_status, Some(false));

                assert_eq!(
                    fs::read_to_string(config2.data_logger.output_file_name_water_temperature)
                        .expect("Could not read back water temperature from disk.")
                        .parse::<f32>()
                        .unwrap(),
                    10.0,
                );
                assert_eq!(
                    fs::read_to_string(
                        config2
                            .data_logger
                            .output_file_name_water_temperature_filtered
                    )
                    .expect("Could not read back filtered water temperature from disk.")
                    .parse::<f32>()
                    .unwrap(),
                    9.9,
                );
                assert_eq!(
                    fs::read_to_string(config2.data_logger.output_file_name_pH)
                        .expect("Could not read back pH from disk.")
                        .parse::<f32>()
                        .unwrap(),
                    11.0,
                );
                assert_eq!(
                    fs::read_to_string(config2.data_logger.output_file_name_pH_filtered)
                        .expect("Could not read back filtered pH from disk.")
                        .parse::<f32>()
                        .unwrap(),
                    10.9,
                );
                assert_eq!(
                    fs::read_to_string(config2.data_logger.output_file_name_conductivity)
                        .expect("Could not read back conductivity from disk.")
                        .parse::<f32>()
                        .unwrap(),
                    12.0,
                );
                assert_eq!(
                    fs::read_to_string(config2.data_logger.output_file_name_ambient_temperature)
                        .expect("Could not read back ambient temperature from disk.")
                        .parse::<f32>()
                        .unwrap(),
                    13.0,
                );
                assert_eq!(
                    fs::read_to_string(config2.data_logger.output_file_name_humidity)
                        .expect("Could not read back ambient humidity from disk.")
                        .parse::<f32>()
                        .unwrap(),
                    14.0,
                );
                assert_eq!(
                    fs::read_to_string(config2.data_logger.output_file_name_refill_in_progress)
                        .expect("Could not read back refill in progress from disk."),
                    "OFF".to_string(),
                );
                assert_eq!(
                    fs::read_to_string(
                        config2
                            .data_logger
                            .output_file_name_tank_level_switch_position
                    )
                    .expect("Could not read back tank_level_switch_position from disk."),
                    "LOW".to_string(),
                );
                assert_eq!(
                    fs::read_to_string(
                        config2
                            .data_logger
                            .output_file_name_tank_level_switch_invalid
                    )
                    .expect("Could not read back tank level switch invalid from disk."),
                    "INVALID".to_string(),
                );
                assert_eq!(
                    fs::read_to_string(config2.data_logger.output_file_name_ventilation_is_on)
                        .expect("Could not read back ventilation status from disk."),
                    "OFF".to_string(),
                );
                assert_eq!(
                    fs::read_to_string(config2.data_logger.output_file_name_heating_is_on)
                        .expect("Could not read back heater status from disk."),
                    "OFF".to_string(),
                );
            });
        });
    }
}
