#![allow(non_snake_case)]
use chrono::Local;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;
use crate::iir_filter::IIRFilter;
use crate::iir_filter::IIRFilterConfig;
use crate::sql_interface_data::DataFrame;
use crate::sql_interface_data::SqlInterfaceData;

/// This struct holds the configuration data for the regular data logging to SQL database.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct DataLoggerConfig {
    logging_interval: u64,
    filter_coefficient_water_temperature: IIRFilterConfig<()>,
    filter_coefficient_pH: IIRFilterConfig<()>,
    filter_coefficient_conductivity: IIRFilterConfig<()>,
}

/// This struct contains the configuration and the implementation for regular data logging to SQL database.
pub struct DataLogger {
    sleep_duration: Duration,
    spin_sleeper: SpinSleeper,
    filter_water_temperature: IIRFilter,
    filter_pH: IIRFilter,
    filter_conductivity: IIRFilter,
}

impl DataLogger {
    pub fn new(config: DataLoggerConfig) -> DataLogger {
        let logging_interval_secs = config.logging_interval;
        Self {
            sleep_duration: Duration::new(logging_interval_secs, 0),
            spin_sleeper: SpinSleeper::default(),
            filter_water_temperature: IIRFilter::new_requires_initial(
                &config.filter_coefficient_water_temperature,
                0.0, // initialization value (mesured value not available at this time)
                1.0, // 1 second time step
            ),
            filter_pH: IIRFilter::new_requires_initial(
                &config.filter_coefficient_pH,
                0.0, // initialization value (mesured value not available at this time)
                1.0, // 1 second time step
            ),
            filter_conductivity: IIRFilter::new_requires_initial(
                &config.filter_coefficient_conductivity,
                0.0, // initialization value (mesured value not available at this time)
                1.0, // 1 second time step
            ),
        }
    }

    pub fn execute(
        &mut self,
        mut sql_interface_data: SqlInterfaceData,
        tx_to_gpio: Sender<InternalCommand>,
        rx_from_gpio: Receiver<f32>,
    ) {
        // define and initialize with value which will be overwritten in first iteration
        let mut water_temperature: Option<f32> = Some(0.0);
        let mut ph: Option<f32> = Some(0.0);
        let mut conductivity: Option<f32> = Some(0.0);
        let mut ambient_temperature: Option<f32> = Some(0.0);
        let mut ambient_humidity: Option<f32> = Some(0.0);
        let mut tank_level_switch_invalid: Option<bool> = Some(false);
        let mut tank_level_switch_position: Option<bool> = Some(false);

        loop {
            self.spin_sleeper.sleep(self.sleep_duration);

            // send request for water temperature
            match tx_to_gpio.send(InternalCommand::RequestSignal(
                AquariumSignal::WaterTemperature,
            )) {
                Ok(_) => {
                    // now listen for response
                    water_temperature = match rx_from_gpio.recv() {
                        Ok(c) => Some(c),
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
                        Ok(c) => Some(c),
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
                        Ok(c) => Some(c),
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
                        Ok(c) => Some(c),
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
                        Ok(c) => Some(c),
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
                        Ok(c) => Some(c > 0.0),
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
                        Ok(c) => Some(c > 0.0),
                        Err(_) => None,
                    };
                }
                Err(e) => {
                    println!("DataLogger: Error in channel communication to GPIO handler when requesting tank level switch invalid: {e:?}");
                }
            }

            // calculate filtered values
            let water_temperature_filtered = match water_temperature {
                Some(c) => Some(self.filter_water_temperature.get_filtered_value(c)),
                None => None,
            };
            let pH_filtered = match ph {
                Some(c) => Some(self.filter_pH.get_filtered_value(c)),
                None => None,
            };
            let conductivity_filtered = match conductivity {
                Some(c) => Some(self.filter_conductivity.get_filtered_value(c)),
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
                refill_in_progress: None,
                tank_level_switch_position: tank_level_switch_position,
                tank_level_switch_invalid: tank_level_switch_invalid,
                surface_ventilation_status: None,
                ambient_temperature: ambient_temperature,
                ambient_humidity: ambient_humidity,
                heater_status: None,
            };
            match sql_interface_data.write_data_frame_to_database(&data_frame) {
                Ok(()) => {}
                Err(e) => {
                    println!("DataLogger: error occurred when writing to SQL database: {e:?}")
                }
            }
        }
    }
}
