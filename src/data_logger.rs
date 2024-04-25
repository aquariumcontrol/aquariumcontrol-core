use chrono::Local;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;
use crate::sql_interface;
use crate::sql_interface_data::DataFrame;
use crate::sql_interface_data::SqlInterfaceData;

#[derive(Deserialize)]
pub struct DataLoggerConfig {
    logging_interval: u64,
}

pub struct DataLogger {
    config: DataLoggerConfig,
    sleep_duration: Duration,
    spin_sleeper: SpinSleeper,
}

pub mod DataLoggerReplacementValues {
    pub const REPLACEMENT_VALUE_WATER_TEMPERATURE: f32 = 0.0;   
    pub const REPLACEMENT_VALUE_PH: f32 = 0.0;   
    pub const REPLACEMENT_VALUE_CONDUCTIVITY: f32 = 0.0;   
}

impl DataLogger {
    pub fn new(config: DataLoggerConfig) -> DataLogger {
        let logging_interval_secs = config.logging_interval;
        DataLogger {
            config: config,
            sleep_duration: Duration::new(logging_interval_secs, 0),
            spin_sleeper: SpinSleeper::default(),
        }
    }

    pub fn execute(
        &self,
        mut sql_interface_data: SqlInterfaceData,
        tx_to_gpio: Sender<InternalCommand>,
        rx_from_gpio: Receiver<f32>,
    ) {
        let mut water_temperature: f32 = 0.0;
        let mut conductivity: f32 = 0.0;
        let mut pH: f32 = 0.0;

        loop {
            self.spin_sleeper.sleep(self.sleep_duration);
            match tx_to_gpio.send(InternalCommand::RequestSignal(
                AquariumSignal::WaterTemperature,
            )) {
                Ok(_) => {
                    // now listen for response
                    water_temperature = match rx_from_gpio.recv() {
                        Ok(c) => c,
                        Err(_) => DataLoggerReplacementValues::REPLACEMENT_VALUE_WATER_TEMPERATURE,
                    };
                }
                Err(e) => {
                    println!("DataLogger: Error in channel communication to GPIO handler when requesting water temperature: {e:?}");
                }
            }
            match tx_to_gpio.send(InternalCommand::RequestSignal(AquariumSignal::pH)) {
                Ok(_) => {
                    // now listen for response
                    pH = match rx_from_gpio.recv() {
                        Ok(c) => c,
                        Err(_) => DataLoggerReplacementValues::REPLACEMENT_VALUE_PH,
                    };
                }
                Err(e) => {
                    println!("DataLogger: Error in channel communication to GPIO handler when requesting pH: {e:?}");
                }
            }
            match tx_to_gpio.send(InternalCommand::RequestSignal(AquariumSignal::Conductivity)) {
                Ok(_) => {
                    // now listen for response
                    conductivity = match rx_from_gpio.recv() {
                        Ok(c) => c,
                        Err(_) => DataLoggerReplacementValues::REPLACEMENT_VALUE_CONDUCTIVITY,
                    };
                }
                Err(e) => {
                    println!("DataLogger: Error in channel communication to GPIO handler when requesting conductivity: {e:?}");
                }
            }
            // all data received, let's construct a data frame
            let current_timestamp = Local::now().naive_local();
            let data_frame = DataFrame {
                timestamp: current_timestamp,
                water_temperature: water_temperature,
                water_temperature_filtered: -1.0,
                ph: pH,
                ph_filtered: -1.0,
                conductivity: conductivity,
                conductivity_filtered: -1.0,
                conductivity_compensated: -1.0,
                refill_in_progress: false,
                main_tank_level_switch_status: false,
                surface_ventilation_status: false,
                ambient_temperature: -1.0,
                ambient_humidity: -1.0,
                heater_status: false,
            };
            sql_interface_data.write_data_frame_to_database(&data_frame);
        }
    }
}
