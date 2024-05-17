use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::channel_content::AquariumDevice;
use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;
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

        loop {
            spin_sleeper.sleep(sleep_duration_hundred_millis);
        }
    }
}
