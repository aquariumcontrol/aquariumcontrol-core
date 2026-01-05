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
    pub const REG_DATA_MSB_PH: u8 = 0x16;

    /// Register for the conductivity Data (MSB)
    pub const REG_DATA_MSB_CONDUCTIVITY: u8 = 0x18;

    /// The OEM temperature sensor returns 4 bytes (MSB, High, Low, LSB)
    pub const RESPONSE_LENGTH_TEMPERATURE: usize = 4;

    /// The OEM pH sensor returns 4 bytes (MSB, High, Low, LSB)
    pub const RESPONSE_LENGTH_PH: usize = 4;

    /// The OEM conductivity sensor returns 4 bytes (MSB, High, Low, LSB)
    pub const RESPONSE_LENGTH_CONDUCTIVITY: usize = 4;

    /// Scaling factor for temperature (Value / 100.0)
    pub const SCALE_FACTOR_TEMPERATURE: f32 = 100.0;

    /// Scaling factor for pH (Value / 100.0)
    pub const SCALE_FACTOR_PH: f32 = 1000.0;

    /// Scaling factor for conductivity (Value / 100.0)
    pub const SCALE_FACTOR_CONDUCTIVITY: f32 = 1000.0;
}

#[derive(Debug)]
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
                    AquariumSignal::Conductivity
                    | AquariumSignal::WaterTemperature
                    | AquariumSignal::pH => {
                        let result_data =
                            self.parse_oem_response(&i2c_response.read_buf[..i2c_response.length])?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::channels::{AquaSender, Channels};
    use crate::sensors::atlas_scientific::atlas_scientific_constants;
    use crate::sensors::i2c_interface::{I2cResponse, I2cResult};
    use crate::utilities::config::{read_config_file, ConfigData};
    use std::collections::HashMap;
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    use crate::sensors::i2c_interface::I2cInterface;
    
    /// Helper function to set up a standard test environment for receive tests.
    fn setup_receive_test() -> (
        HashMap<AquariumSignal, AtlasScientificOem>,
        AtlasScientificChannels,
        AquaSender<I2cResult>,
        Arc<Mutex<AtlasScientificResultData>>,
        Arc<Mutex<AtlasScientificResultData>>,
        Arc<Mutex<AtlasScientificResultData>>,
    ) {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let mut drivers = HashMap::new();
        drivers.insert(
            AquariumSignal::WaterTemperature,
            AtlasScientificOem::new(
                config.atlas_scientific.address_atlas_scientific_temperature,
                config.atlas_scientific.sleep_time_millis_temperature,
                atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
                atlas_scientific_oem_constants::REG_DATA_MSB_TEMPERATURE,
                atlas_scientific_oem_constants::SCALE_FACTOR_TEMPERATURE,
                atlas_scientific_oem_constants::RESPONSE_LENGTH_TEMPERATURE,
            ),
        );
        drivers.insert(
            AquariumSignal::pH,
            AtlasScientificOem::new(
                config.atlas_scientific.address_atlas_scientific_ph,
                config.atlas_scientific.sleep_time_millis_ph,
                atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
                atlas_scientific_oem_constants::REG_DATA_MSB_PH,
                atlas_scientific_oem_constants::SCALE_FACTOR_PH,
                atlas_scientific_oem_constants::RESPONSE_LENGTH_PH,
            ),
        );
        drivers.insert(
            AquariumSignal::Conductivity,
            AtlasScientificOem::new(
                config
                    .atlas_scientific
                    .address_atlas_scientific_conductivity,
                config.atlas_scientific.sleep_time_millis_conductivity,
                atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
                atlas_scientific_oem_constants::REG_DATA_MSB_CONDUCTIVITY,
                atlas_scientific_oem_constants::SCALE_FACTOR_CONDUCTIVITY,
                atlas_scientific_oem_constants::RESPONSE_LENGTH_CONDUCTIVITY,
            ),
        );

        let channels = Channels::new_for_test();

        // Initialize mutexes with a known default value (0.0)
        let mutex_temp = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_cond = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));

        (
            drivers,
            channels.atlas_scientific,
            channels.i2c_interface.tx_i2c_interface_to_atlas_scientific,
            mutex_temp,
            mutex_ph,
            mutex_cond,
        )
    }

    #[test]
    fn test_conductivity_reading() {
        let (
            mut drivers,
            mut atlas_scientific_channels,
            mut tx_i2c_interface_to_atlas_scientific,
            mutex_temp,
            mutex_ph,
            mutex_cond,
        ) = setup_receive_test();

        let driver = drivers.get_mut(&AquariumSignal::Conductivity).unwrap();

        // Mock Data: 1413 uS/cm. 1413 * 100 = 141.300.
        // 1413000 in Hex is 0x00158EA8
        // Big Endian bytes: [0x00, 0x15, 0x8E, 0xA8]4]
        let mut mock_data = [0u8; 4];
        mock_data[0] = 0x00;
        mock_data[1] = 0x15;
        mock_data[2] = 0x8F;
        mock_data[3] = 0x88;
        let i2c_response = I2cResponse::new(&mock_data);
        tx_i2c_interface_to_atlas_scientific
            .send(Ok(i2c_response))
            .unwrap();

        // Process Response
        driver
            .receive_response_from_i2c_interface(
                &AquariumSignal::Conductivity,
                &mut atlas_scientific_channels,
                &mutex_temp,
                &mutex_ph,
                &mutex_cond,
            )
            .expect("Failed to process response");

        // --- Step 3: Check Result ---
        let result = mutex_cond.lock().unwrap();
        // 141300 / 100.0 = 1413.0
        println!(
            "test_conductivity_reading: Result has value {}",
            result.value
        );
        assert!((result.value - 1413.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_ph_reading() {
        let (
            mut drivers,
            mut atlas_scientific_channels,
            mut tx_i2c_interface_to_atlas_scientific,
            mutex_temp,
            mutex_ph,
            mutex_cond,
        ) = setup_receive_test();

        let driver = drivers.get_mut(&AquariumSignal::pH).unwrap();

        let mut mock_data = [0u8; 4];
        mock_data[0] = 0x00;
        mock_data[1] = 0x00;
        mock_data[2] = 0x1B;
        mock_data[3] = 0x58;
        let i2c_response = I2cResponse::new(&mock_data);
        tx_i2c_interface_to_atlas_scientific
            .send(Ok(i2c_response))
            .unwrap();

        // Process Response
        driver
            .receive_response_from_i2c_interface(
                &AquariumSignal::pH,
                &mut atlas_scientific_channels,
                &mutex_temp,
                &mutex_ph,
                &mutex_cond,
            )
            .expect("Failed to process response");

        // --- Step 3: Check Result ---
        let result = mutex_ph.lock().unwrap();
        println!("test_ph_reading: Result has value {}", result.value);
        // 7000 / 1000.0 = 7.0
        assert!((result.value - 7.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_temperature_reading() {
        let (
            mut drivers,
            mut atlas_scientific_channels,
            mut tx_i2c_interface_to_atlas_scientific,
            mutex_temp,
            mutex_ph,
            mutex_cond,
        ) = setup_receive_test();

        let driver = drivers.get_mut(&AquariumSignal::WaterTemperature).unwrap();

        // Mock Data: 25.432 C. 25.432 * 100 = 2543.2 -> 2543 (integer)
        // 2543 in Hex is 0x09EF
        // Big Endian bytes: [0x00, 0x00, 0x09, 0xEF]
        let mut mock_data = [0u8; 4];
        mock_data[0] = 0x00;
        mock_data[1] = 0x00;
        mock_data[2] = 0x09;
        mock_data[3] = 0xEF;
        let i2c_response = I2cResponse::new(&mock_data);
        tx_i2c_interface_to_atlas_scientific
            .send(Ok(i2c_response))
            .unwrap();

        // Process Response
        driver
            .receive_response_from_i2c_interface(
                &AquariumSignal::WaterTemperature,
                &mut atlas_scientific_channels,
                &mutex_temp,
                &mutex_ph,
                &mutex_cond,
            )
            .expect("Failed to process response");

        // --- Step 3: Check Result ---
        let result = mutex_temp.lock().unwrap();
        println!(
            "test_temperature_reading: Result has value {}",
            result.value
        );
        // 2543 / 100.0 = 25.43
        assert!((result.value - 25.43).abs() < 0.0001);
    }

    #[test]
    fn test_short_response_error() {
        let (
            mut drivers,
            mut atlas_scientific_channels,
            mut tx_i2c_interface_to_atlas_scientific,
            mutex_temp,
            mutex_ph,
            mutex_cond,
        ) = setup_receive_test();

        // Simulate sending a response that is too short (e.g., 3 byte instead of 4)
        let mock_data = [0u8; 3];
        let i2c_response = I2cResponse::new(&mock_data);
        tx_i2c_interface_to_atlas_scientific
            .send(Ok(i2c_response))
            .unwrap();

        let driver = drivers.get_mut(&AquariumSignal::WaterTemperature).unwrap();

        let result = driver.receive_response_from_i2c_interface(
            &AquariumSignal::WaterTemperature,
            &mut atlas_scientific_channels,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );

        match result {
            Ok(_c) => {
                panic!("Test case is expecting error.");
            }
            Err(e) => {
                matches!(
                    e,
                    AtlasScientificError::ResponseTooShort {
                        location: _,
                        expected_length:
                            atlas_scientific_oem_constants::RESPONSE_LENGTH_TEMPERATURE,
                        actual_length: 3,
                        signal: AquariumSignal::WaterTemperature
                    }
                );
            }
        }
    }

    #[test]
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[ignore]
    // read from real Atlas Scientific hardware (OEM) combined in one test case to avoid race conditions:
    // - pH sensor
    // - conductivity sensor
    // - temperature
    pub fn test_atlas_scientific_oem_read_sensors() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let mut i2c_interface = I2cInterface::new(config.i2c_interface).unwrap();

        // --- pH Sensor ---
        let cmd_ph = [atlas_scientific_oem_constants::REG_DATA_MSB_PH];
        let request_ph = I2cRequest::new(
            config.atlas_scientific.address_atlas_scientific_ph,
            &cmd_ph,
            config.atlas_scientific.sleep_time_millis_ph,
            atlas_scientific_oem_constants::RESPONSE_LENGTH_PH,
        )
        .unwrap();

        match i2c_interface.get_response_from_i2c(request_ph) {
            Ok(response) => {
                if response.length == atlas_scientific_oem_constants::RESPONSE_LENGTH_PH {
                    let data = &response.read_buf[..response.length];
                    let value_u32 = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                    let value = value_u32 as f32 / atlas_scientific_oem_constants::SCALE_FACTOR_PH;
                    println!("Measured pH: {:.2}", value);
                } else {
                    println!("Invalid response length for pH: {}", response.length);
                }
            }
            Err(e) => {
                println!("Failed to read from pH sensor: {:?}", e);
            }
        }

        // --- Temperature Sensor ---
        let cmd_temp = [atlas_scientific_oem_constants::REG_DATA_MSB_TEMPERATURE];
        let request_temperature = I2cRequest::new(
            config.atlas_scientific.address_atlas_scientific_temperature,
            &cmd_temp,
            config.atlas_scientific.sleep_time_millis_temperature,
            atlas_scientific_oem_constants::RESPONSE_LENGTH_TEMPERATURE,
        )
        .unwrap();

        match i2c_interface.get_response_from_i2c(request_temperature) {
            Ok(response) => {
                if response.length == atlas_scientific_oem_constants::RESPONSE_LENGTH_TEMPERATURE {
                    let data = &response.read_buf[..response.length];
                    let value_u32 = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                    let value =
                        value_u32 as f32 / atlas_scientific_oem_constants::SCALE_FACTOR_TEMPERATURE;
                    println!("Measured Temperature: {:.2}", value);
                } else {
                    println!(
                        "Invalid response length for Temperature: {}",
                        response.length
                    );
                }
            }
            Err(e) => {
                println!("Failed to read from Temperature sensor: {:?}", e);
            }
        }

        // --- Conductivity Sensor ---
        let cmd_cond = [atlas_scientific_oem_constants::REG_DATA_MSB_CONDUCTIVITY];
        let request_conductivity = I2cRequest::new(
            config
                .atlas_scientific
                .address_atlas_scientific_conductivity,
            &cmd_cond,
            config.atlas_scientific.sleep_time_millis_conductivity,
            atlas_scientific_oem_constants::RESPONSE_LENGTH_CONDUCTIVITY,
        )
        .unwrap();

        match i2c_interface.get_response_from_i2c(request_conductivity) {
            Ok(response) => {
                if response.length == atlas_scientific_oem_constants::RESPONSE_LENGTH_CONDUCTIVITY {
                    let data = &response.read_buf[..response.length];
                    let value_u32 = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                    let value = value_u32 as f32
                        / atlas_scientific_oem_constants::SCALE_FACTOR_CONDUCTIVITY;
                    println!("Measured Conductivity: {:.2}", value);
                } else {
                    println!(
                        "Invalid response length for Conductivity: {}",
                        response.length
                    );
                }
            }
            Err(e) => {
                println!("Failed to read from Conductivity sensor: {:?}", e);
            }
        }
    }
}
