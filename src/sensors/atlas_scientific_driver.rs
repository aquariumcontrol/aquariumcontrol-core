use crate::sensors::atlas_scientific::{AtlasScientificResult, AtlasScientificResultData};
use crate::sensors::atlas_scientific_channels::AtlasScientificChannels;
use crate::sensors::atlas_scientific_error::AtlasScientificError;
use crate::utilities::channel_content::AquariumSignal;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

/// Abstract strategy for interacting with an Atlas Scientific sensor.
/// Allows for different sensor types ("oem", "ezo") to be integrated.
pub trait AtlasScientificDriver: Send + Sync + Debug {
    fn send_request_to_i2c_interface(
        &self,
        atlas_scientific_channels: &mut AtlasScientificChannels,
    ) -> Result<(), AtlasScientificError>;

    fn receive_response_from_i2c_interface(
        &mut self,
        signal: &AquariumSignal,
        atlas_scientific_channels: &mut AtlasScientificChannels,
        mutex_water_temperature: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_ph: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_conductivity: &Arc<Mutex<AtlasScientificResultData>>,
    ) -> AtlasScientificResult;

    #[cfg(test)]
    fn get_mutex_access_duration_exceeded(&self, signal: &AquariumSignal) -> bool;
}
