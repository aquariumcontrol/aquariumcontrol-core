use crate::launch::channels::AquaChannelError;
use crate::sensors::atlas_scientific::{AtlasScientific, AtlasScientificEzo};
use crate::sensors::atlas_scientific::{AtlasScientificResult, AtlasScientificResultData};
use crate::sensors::atlas_scientific_channels::AtlasScientificChannels;
use crate::sensors::atlas_scientific_driver::AtlasScientificDriver;
use crate::sensors::atlas_scientific_error::AtlasScientificError;
use crate::sensors::atlas_scientific_error::AtlasScientificError::SignalRequestCreationFailure;
use crate::sensors::i2c_interface::I2cRequest;
use crate::utilities::channel_content::AquariumSignal;
use crate::utilities::check_mutex_access_duration::CheckMutexAccessDurationTrait;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[allow(dead_code)]
/// numeric values used in communication with Atlas Scientific sensor units
pub(crate) mod atlas_scientific_oem_constants {
}


pub struct AtlasScientificOem {
    pub i2c_address: u16,

    pub sleep_time_millis: u64,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for water temperature
    pub lock_warn_max_mutex_access_duration_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for pH
    pub lock_warn_max_mutex_access_duration_ph: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for conductivity
    pub lock_warn_max_mutex_access_duration_conductivity: bool,

    // for testing purposes: record when mutex access time is exceeded without resetting it
    #[cfg(test)]
    pub mutex_temperature_access_duration_exceeded: bool,
    #[cfg(test)]
    pub mutex_ph_access_duration_exceeded: bool,
    #[cfg(test)]
    pub mutex_conductivity_access_duration_exceeded: bool,

    pub max_mutex_access_duration: Duration,
}

impl AtlasScientificOem {
    pub fn new(
        i2c_address: u16,
        sleep_time_millis: u64,
        max_mutex_access_duration_millis: u64,
    ) -> Self {
        AtlasScientificOem {
            i2c_address,
            sleep_time_millis,
            lock_warn_max_mutex_access_duration_temperature: false,
            lock_warn_max_mutex_access_duration_ph: false,
            lock_warn_max_mutex_access_duration_conductivity: false,
            #[cfg(test)]
            mutex_temperature_access_duration_exceeded: false,
            #[cfg(test)]
            mutex_ph_access_duration_exceeded: false,
            #[cfg(test)]
            mutex_conductivity_access_duration_exceeded: false,
            max_mutex_access_duration: Duration::from_millis(max_mutex_access_duration_millis),
        }
    }
}

impl AtlasScientificDriver for AtlasScientificOem {
    fn send_request_to_i2c_interface(&self, atlas_scientific_channels: &mut AtlasScientificChannels) -> Result<(), AtlasScientificError> {
        todo!()
    }

    fn receive_response_from_i2c_interface(&mut self, signal: &AquariumSignal, atlas_scientific_channels: &mut AtlasScientificChannels, mutex_water_temperature: &Arc<Mutex<AtlasScientificResultData>>, mutex_ph: &Arc<Mutex<AtlasScientificResultData>>, mutex_conductivity: &Arc<Mutex<AtlasScientificResultData>>) -> AtlasScientificResult {
        todo!()
    }

    #[cfg(test)]
    fn get_mutex_access_duration_exceeded(&self, signal: &AquariumSignal) -> bool {
        match signal {
            AquariumSignal::WaterTemperature => self.mutex_temperature_access_duration_exceeded,
            AquariumSignal::pH => self.mutex_ph_access_duration_exceeded,
            AquariumSignal::Conductivity => self.mutex_conductivity_access_duration_exceeded,
            _ => panic!("Signal type not supported."),
        }
    }
}