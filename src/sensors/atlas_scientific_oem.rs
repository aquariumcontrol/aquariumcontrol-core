use crate::launch::channels::AquaChannelError;
use crate::sensors::atlas_scientific::AtlasScientific;
use crate::sensors::atlas_scientific::{AtlasScientificResult, AtlasScientificResultData};
use crate::sensors::atlas_scientific_channels::AtlasScientificChannels;
use crate::sensors::atlas_scientific_driver::AtlasScientificDriver;
use crate::sensors::atlas_scientific_error::AtlasScientificError;
use crate::sensors::i2c_interface::I2cRequest;
use crate::utilities::channel_content::AquariumSignal;
use crate::utilities::check_mutex_access_duration::CheckMutexAccessDurationTrait;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[allow(dead_code)]
/// numeric values used in communication with Atlas Scientific sensor units
pub(crate) mod atlas_scientific_oem_constants {
    /// Register for the temperature Data (MSB)
    pub const REG_DATA_MSB_TEMPERATURE: u8 = 0x18;

    /// Register for the pH Data (MSB)
    pub const REG_DATA_MSB_PH: u8 = 0x18;

    /// Register for the conductivity Data (MSB)
    pub const REG_DATA_MSB_CONDUCTIVITY: u8 = 0x18;

    /// The OEM conductivity sensor returns 4 bytes (MSB, High, Low, LSB)
    pub const RESPONSE_LENGTH_TEMPERATURE: usize = 4;
    pub const RESPONSE_LENGTH_PH: usize = 4;
    pub const RESPONSE_LENGTH_CONDUCTIVITY: usize = 4;

    /// Scaling factor for temperature (Value / 100.0)
    pub const SCALE_FACTOR_TEMPERATURE: f32 = 100.0;

    /// Scaling factor for pH (Value / 100.0)
    pub const SCALE_FACTOR_PH: f32 = 100.0;

    /// Scaling factor for conductivity (Value / 100.0)
    pub const SCALE_FACTOR_CONDUCTIVITY: f32 = 100.0;
}

pub struct AtlasScientificOem {
    pub i2c_address: u16,

    pub sleep_time_millis: u64,

    pub register_data_msb: u8,

    pub scale_factor: f32,

    pub response_length: usize,

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
        register_data_msb: u8,
        scale_factor: f32,
        response_length: usize,
    ) -> Self {
        AtlasScientificOem {
            i2c_address,
            sleep_time_millis,
            register_data_msb,
            scale_factor,
            response_length,
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

    /// Helper to parse the binary response from the OEM sensor
    fn parse_oem_response(
        &self,
        data: &[u8],
    ) -> Result<AtlasScientificResultData, AtlasScientificError> {
        if data.len() < self.response_length {
            return Err(AtlasScientificError::ResponseTooShort {
                location: module_path!().to_string(),
                signal: AquariumSignal::Conductivity,
                expected_length: self.response_length,
                actual_length: data.len(),
            });
        }

        // Convert 4 bytes to u32 (Big Endian)
        // See oem-ec.py: (data[0] << 24) | (data[1] << 16) | ...
        let value_u32 = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);

        // Convert to float and scale
        let value_f32 = value_u32 as f32 / self.scale_factor;

        Ok(AtlasScientificResultData::new(value_f32))
    }
}

impl AtlasScientificDriver for AtlasScientificOem {
    fn send_request_to_i2c_interface(
        &self,
        atlas_scientific_channels: &mut AtlasScientificChannels,
    ) -> Result<(), AtlasScientificError> {
        // OEM Protocol: Write Register Address -> Wait -> Read Bytes
        let command = vec![self.register_data_msb];

        let request = I2cRequest::new(
            self.i2c_address,
            &command,
            self.sleep_time_millis,
            self.response_length,
        )
        .map_err(|e| AtlasScientificError::SignalRequestCreationFailure {
            location: module_path!().to_string(),
            source: e,
        })?;

        atlas_scientific_channels
            .tx_atlas_scientific_to_i2c_interface
            .send(request)
            .map_err(
                |e| AtlasScientificError::SendingRequestToI2cInterfaceFailed {
                    location: module_path!().to_string(),
                    source: e,
                },
            )
    }

    fn receive_response_from_i2c_interface(
        &mut self,
        signal: &AquariumSignal,
        atlas_scientific_channels: &mut AtlasScientificChannels,
        mutex_water_temperature: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_ph: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_conductivity: &Arc<Mutex<AtlasScientificResultData>>,
    ) -> AtlasScientificResult {
        // 1. Get the response from the channel
        let response = atlas_scientific_channels
            .rx_atlas_scientific_from_i2c_interface
            .recv()
            .map_err(|_| AtlasScientificError::ChannelIsDisconnected {
                location: "AtlasScientificOem::receive_response".to_string(),
                source: AquaChannelError::Disconnected,
            })?;

        match response {
            Ok(i2c_response) => {
                // 2. Parse the binary data
                match signal {
                    AquariumSignal::Conductivity => {
                        let result_data = self.parse_oem_response(&i2c_response.read_buf)?;
                        let instant_before_locking_mutex = Instant::now();
                        let result_data_clone = result_data.clone();

                        // 3. Write the result to mutex
                        let instant_after_locking_mutex = AtlasScientific::write_result_to_mutex(
                            signal,
                            result_data,
                            mutex_water_temperature,
                            mutex_ph,
                            mutex_conductivity,
                        )?;

                        // Check if access to mutex took too long.
                        self.check_mutex_access_duration(
                            Some(signal),
                            instant_after_locking_mutex,
                            instant_before_locking_mutex,
                        );

                        // The final result of this arm is the successfully parsed data.
                        Ok(result_data_clone)
                    }
                    _ => Err(AtlasScientificError::InvalidSignal(
                        module_path!().to_string(),
                        signal.clone(),
                    )),
                }
            }
            Err(i2c_error) => Err(AtlasScientificError::I2cCommunicationFailure {
                location: module_path!().to_string(),
                signal: signal.clone(),
                source: i2c_error,
            }),
        }
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
