/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Implements the driver for Atlas Scientific OEM sensors.
//!
//! This module provides the `AtlasScientificOem` struct, which implements the
//! `AtlasScientificDriver` trait. It handles the specific communication protocol
//! required by the industrial "OEM" modules from Atlas Scientific.
//!
//! ## Protocol Details
//!
//! Unlike the consumer-grade EZO sensors which use ASCII commands, the OEM modules
//! use a binary register-based protocol:
//!
//! 1.  **Wake Up**: The device must be explicitly woken up by writing to the
//!     `REG_WAKEUP` register.
//! 2.  **Trigger/Read**: To read data, the driver writes the address of the specific
//!     data register (e.g., `REG_DATA_MSB_PH`) to the device.
//! 3.  **Wait**: The driver waits for a specified processing time (`sleep_time_millis`).
//!     For OEM type devices, this value is much lower compared to EZO type devices.
//! 4.  **Response**: The driver reads a fixed number of bytes (typically 4 bytes for
//!     the measurement value in Big Endian format).
//!
//! ## Initialization
//!
//! The `device_init` method is critical for OEM sensors. It performs two steps:
//! 1.  **Wake Up**: Sends the wake-up command to ensure the device is active.
//! 2.  **Type Verification**: Reads the `REG_DEVICE_TYPE` register to ensure the
//!     connected hardware matches the configuration (e.g., ensuring a pH sensor
//!     is not connected to the address configured for a conductivity sensor).

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
use std::time::Duration;
use std::time::Instant;

#[allow(dead_code)]
/// numeric values used in communication with Atlas Scientific sensor units
pub mod atlas_scientific_oem_constants {
    /// Register for the device type (identical across all types)
    pub const REG_DEVICE_TYPE: u8 = 0x00;

    /// Temperature device type code
    pub const DEVICE_TYPE_TEMPERATURE: u8 = 0x05;

    /// pH device type code
    pub const DEVICE_TYPE_PH: u8 = 0x01;

    /// Conductivity device type code
    pub const DEVICE_TYPE_CONDUCTIVITY: u8 = 0x04;

    // The OEM devices return 2 byte for this request
    pub const DEVICE_TYPE_RESPONSE_LENGTH: usize = 2;

    /// Register for waking up the sensor
    pub const REG_WAKEUP: u8 = 0x06;

    /// Register for the firmware version (identical across all types)
    pub const REG_FIRMWARE_VERSION: u8 = 0x01;

    /// Register for the temperature Data (MSB)
    pub const REG_DATA_MSB_TEMPERATURE: u8 = 0x0E;

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

    /// Scaling factor for temperature (Value / 1000.0)
    pub const SCALE_FACTOR_TEMPERATURE: f32 = 1000.0;

    /// Scaling factor for pH (Value / 1000.0)
    pub const SCALE_FACTOR_PH: f32 = 1000.0;

    /// Scaling factor for conductivity (Value / 100.0)
    pub const SCALE_FACTOR_CONDUCTIVITY: f32 = 100.0;
}

#[derive(Debug)]
pub struct AtlasScientificOem {
    /// I2C address of the device
    pub i2c_address: u16,

    /// The expected device type code (e.g., 0x05 for Temperature).
    /// Used during initialization to verify the connected hardware.
    pub device_type: u8,

    /// Sleep time in milliseconds applied between writing the register address and reading the data.
    pub sleep_time_millis: u64,

    /// The register address where the measurement data starts (Most Significant Byte).
    pub register_data_msb: u8,

    /// The divisor used to convert the raw integer value to a float.
    pub scale_factor: f32,

    /// The number of bytes to read for a measurement.
    pub response_length: usize,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for water temperature
    pub lock_warn_max_mutex_access_duration_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for pH
    pub lock_warn_max_mutex_access_duration_ph: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for conductivity
    pub lock_warn_max_mutex_access_duration_conductivity: bool,

    // for testing purposes: record when mutex access time is exceeded without resetting it
    #[cfg(any(test, feature = "test-utils"))]
    pub mutex_temperature_access_duration_exceeded: bool,
    #[cfg(any(test, feature = "test-utils"))]
    pub mutex_ph_access_duration_exceeded: bool,
    #[cfg(any(test, feature = "test-utils"))]
    pub mutex_conductivity_access_duration_exceeded: bool,

    pub max_mutex_access_duration: Duration,
}

impl AtlasScientificOem {
    /// Creates a new instance of the OEM driver.
    ///
    /// # Arguments
    /// * `i2c_address` - The I2C address of the OEM module.
    /// * `device_type` - The expected device type code (e.g., 1 for pH, 5 for Temp).
    ///   Used during initialization to verify the hardware.
    /// * `sleep_time_millis` - The time to wait between writing the register address and reading the data.
    /// * `max_mutex_access_duration_millis` - Threshold for logging mutex contention warnings.
    /// * `register_data_msb` - The register address where the measurement data starts (Most Significant Byte).
    /// * `scale_factor` - The divisor used to convert the raw integer value to a float (e.g., 100.0 or 1000.0).
    /// * `response_length` - The number of bytes to read for a measurement (typically 4).
    ///
    /// [impl->swreq~sensors_read_data~1]
    pub fn new(
        i2c_address: u16,
        device_type: u8,
        sleep_time_millis: u64,
        max_mutex_access_duration_millis: u64,
        register_data_msb: u8,
        scale_factor: f32,
        response_length: usize,
    ) -> Self {
        AtlasScientificOem {
            i2c_address,
            device_type,
            sleep_time_millis,
            register_data_msb,
            scale_factor,
            response_length,
            lock_warn_max_mutex_access_duration_temperature: false,
            lock_warn_max_mutex_access_duration_ph: false,
            lock_warn_max_mutex_access_duration_conductivity: false,
            #[cfg(any(test, feature = "test-utils"))]
            mutex_temperature_access_duration_exceeded: false,
            #[cfg(any(test, feature = "test-utils"))]
            mutex_ph_access_duration_exceeded: false,
            #[cfg(any(test, feature = "test-utils"))]
            mutex_conductivity_access_duration_exceeded: false,
            max_mutex_access_duration: Duration::from_millis(max_mutex_access_duration_millis),
        }
    }

    /// Parses the binary response from an OEM sensor.
    ///
    /// OEM sensors typically return data as a 32-bit unsigned integer in Big Endian format
    /// (MSB first). This function combines the bytes into a `u32` and then divides by
    /// the configured `scale_factor` to produce the final floating-point value.
    ///
    /// # Arguments
    /// * `signal` - The signal type (used for error reporting context).
    /// * `data` - The raw byte slice received from the I2C interface.
    ///
    /// # Returns
    /// * `Ok(AtlasScientificResultData)` - The parsed value wrapped in result data.
    /// * `Err(AtlasScientificError)` - If the data length is insufficient.
    fn parse_oem_response(
        &self,
        signal: &AquariumSignal,
        data: &[u8],
    ) -> Result<AtlasScientificResultData, AtlasScientificError> {
        if data.len() < self.response_length {
            return Err(AtlasScientificError::ResponseTooShort {
                location: module_path!().to_string(),
                signal: signal.clone(),
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
    /// Constructs and sends a measurement request to the I2C interface thread.
    ///
    /// For OEM sensors, this involves writing the target register address (`register_data_msb`)
    /// to the device. The `I2cRequest` is configured to write this single byte, wait
    /// for `sleep_time_millis`, and then read `response_length` bytes.
    ///
    /// # Arguments
    /// * `atlas_scientific_channels` - A mutable reference to the struct containing the channels.
    ///
    /// # Returns
    /// * `Ok(())` - If the request was successfully created and sent.
    /// * `Err(AtlasScientificError)` - If request creation or sending failed.
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

    /// Receives and processes the binary measurement response.
    ///
    /// This function:
    /// 1.  Receives the raw bytes from the I2C channel.
    /// 2.  Calls `parse_oem_response` to convert the Big Endian bytes into a float.
    /// 3.  Updates the shared `Mutex` with the result.
    /// 4.  Monitors mutex lock duration.
    ///
    /// # Arguments
    /// * `signal` - The `AquariumSignal` indicating which sensor's data is expected.
    /// * `atlas_scientific_channels` - A mutable reference to the struct containing the channels.
    /// * `mutex_water_temperature` - The shared `Mutex` for the water temperature result.
    /// * `mutex_ph` - The shared `Mutex` for the pH result.
    /// * `mutex_conductivity` - The shared `Mutex` for the conductivity result.
    ///
    /// # Returns
    /// * `Ok(AtlasScientificResultData)` - The successfully parsed measurement data.
    /// * `Err(AtlasScientificError)` - If communication, parsing, or mutex locking failed.
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
            .blocking_recv()
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
                        let result_data = self.parse_oem_response(
                            signal,
                            &i2c_response.read_buf[..i2c_response.length],
                        )?;
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

    /// Performs hardware initialization for the OEM sensor.
    ///
    /// This process involves:
    /// 1.  **Wake Up**: Sending `0x01` to the `REG_WAKEUP` (0x06) register to ensure the device is active.
    /// 2.  **Type Check**: Reading the `REG_DEVICE_TYPE` (0x00) register and comparing
    ///     it against the configured `device_type`.
    ///
    /// # Arguments
    /// * `atlas_scientific_channels` - Channels used to communicate with the I2C interface.
    /// * `signal` - The signal type associated with this driver instance.
    ///
    /// # Returns
    /// * `Ok(())` - If the device was successfully woken up and the device type matches.
    /// * `Err(AtlasScientificError)` - If I2C communication fails or the device type is incorrect.
    fn device_init(
        &self,
        atlas_scientific_channels: &mut AtlasScientificChannels,
        signal: &AquariumSignal,
    ) -> Result<(), AtlasScientificError> {
        // Step 1: Wake up the sensor - write 0x01 to the register
        let wake_command = vec![atlas_scientific_oem_constants::REG_WAKEUP, 0x01];

        let wake_request = I2cRequest::new(
            self.i2c_address,
            &wake_command,
            0, // no sleep time
            0, // We don't expect a response for the write operation
        )
        .map_err(|e| AtlasScientificError::SignalRequestCreationFailure {
            location: module_path!().to_string(),
            source: e,
        })?;

        atlas_scientific_channels
            .tx_atlas_scientific_to_i2c_interface
            .send(wake_request)
            .map_err(
                |e| AtlasScientificError::SendingRequestToI2cInterfaceFailed {
                    location: module_path!().to_string(),
                    source: e,
                },
            )?;

        let _response = atlas_scientific_channels
            .rx_atlas_scientific_from_i2c_interface
            .blocking_recv()
            .map_err(|_| AtlasScientificError::ChannelIsDisconnected {
                location: "AtlasScientificOem::receive_response".to_string(),
                source: AquaChannelError::Disconnected,
            })?;
        // no evaluation of response content required for step 1

        // Step 2: Check the sensor type
        let command_inform_type = vec![atlas_scientific_oem_constants::REG_DEVICE_TYPE];
        let request = I2cRequest::new(
            self.i2c_address,
            &command_inform_type,
            self.sleep_time_millis,
            atlas_scientific_oem_constants::DEVICE_TYPE_RESPONSE_LENGTH,
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
            )?;

        let response = atlas_scientific_channels
            .rx_atlas_scientific_from_i2c_interface
            .blocking_recv()
            .map_err(|_| AtlasScientificError::ChannelIsDisconnected {
                location: "AtlasScientificOem::receive_response".to_string(),
                source: AquaChannelError::Disconnected,
            })?;

        match response {
            Ok(i2c_response) => {
                if i2c_response.length > atlas_scientific_oem_constants::DEVICE_TYPE_RESPONSE_LENGTH
                {
                    #[cfg(test)]
                    println!("Firmware version = {}", i2c_response.read_buf[1]);
                    return Err(AtlasScientificError::ResponseTooLong {
                        location: module_path!().to_string(),
                        signal: signal.clone(),
                        expected_length: 1,
                        actual_length: i2c_response.length,
                    });
                }
                if i2c_response.length < atlas_scientific_oem_constants::DEVICE_TYPE_RESPONSE_LENGTH
                {
                    return Err(AtlasScientificError::ResponseTooShort {
                        location: module_path!().to_string(),
                        signal: signal.clone(),
                        expected_length: 1,
                        actual_length: i2c_response.length,
                    });
                }

                #[cfg(test)]
                println!("Firmware version = {}", i2c_response.read_buf[1]);

                if i2c_response.read_buf[0] != self.device_type {
                    return Err(AtlasScientificError::InvalidDeviceType {
                        location: module_path!().to_string(),
                        expected_value: self.device_type,
                        actual_value: i2c_response.read_buf[0],
                    });
                }

                #[cfg(test)]
                println!("Atlas Scientific OEM sensor type {} recognized.", signal);

                Ok(())
            }
            Err(i2c_error) => Err(AtlasScientificError::I2cCommunicationFailure {
                location: module_path!().to_string(),
                signal: signal.clone(),
                source: i2c_error,
            }),
        }
    }

    /// Retrieves the mutex access duration exceeded.
    ///
    /// # Arguments
    /// * `signal` - sensor type
    #[cfg(any(test, feature = "test-utils"))]
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
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub mod tests {
    use crate::launch::channels::channel;
    use crate::sensors::atlas_scientific::AtlasScientificResultData;
    use crate::sensors::atlas_scientific_channels::AtlasScientificChannels;
    use crate::sensors::atlas_scientific_driver::AtlasScientificDriver;
    use crate::sensors::atlas_scientific_error::AtlasScientificError;
    use crate::sensors::atlas_scientific_monitor_view::AtlasScientificMonitorView;
    use crate::sensors::atlas_scientific_oem::{
        atlas_scientific_oem_constants, AtlasScientificOem,
    };
    use crate::sensors::i2c_error::I2cError;
    use crate::sensors::i2c_interface::{I2cRequest, I2cResponse, I2cResult};
    use crate::utilities::channel_content::AquariumSignal;
    use std::sync::{Arc, Mutex};

    #[test]
    // Verifies parse_oem_response handles valid Big Endian u32 byte decoding and response length validation errors.
    /// [utest->swreq~sensors_read_data~1]
    fn test_parse_oem_response() {
        let oem = AtlasScientificOem::new(
            0x66,
            atlas_scientific_oem_constants::DEVICE_TYPE_TEMPERATURE,
            10,
            10,
            atlas_scientific_oem_constants::REG_DATA_MSB_TEMPERATURE,
            atlas_scientific_oem_constants::SCALE_FACTOR_TEMPERATURE,
            atlas_scientific_oem_constants::RESPONSE_LENGTH_TEMPERATURE,
        );

        // 1. ResponseTooShort error
        let short_data = [0u8; 2];
        let err = oem.parse_oem_response(&AquariumSignal::WaterTemperature, &short_data);
        assert!(matches!(
            err,
            Err(AtlasScientificError::ResponseTooShort { .. })
        ));

        // 2. Valid response parsing: 25000 / 1000.0 = 25.0
        // 25000 in u32 Big Endian = [0, 0, 0x61, 0xA8]
        let val_25000: u32 = 25000;
        let valid_data = val_25000.to_be_bytes();
        let res = oem
            .parse_oem_response(&AquariumSignal::WaterTemperature, &valid_data)
            .unwrap();
        assert_eq!(res.value, 25.0);
    }

    #[test]
    // Verifies send_request_to_i2c_interface sends request successfully and handles disconnected channel errors.
    /// [utest->swreq~sensors_read_data~1]
    fn test_send_request_to_i2c_interface() {
        let oem = AtlasScientificOem::new(
            0x66,
            atlas_scientific_oem_constants::DEVICE_TYPE_TEMPERATURE,
            10,
            10,
            atlas_scientific_oem_constants::REG_DATA_MSB_TEMPERATURE,
            atlas_scientific_oem_constants::SCALE_FACTOR_TEMPERATURE,
            atlas_scientific_oem_constants::RESPONSE_LENGTH_TEMPERATURE,
        );

        let (tx_i2c, rx_i2c) = channel::<I2cRequest>(10, false);
        let (_tx_i2c_res, rx_i2c_res) = channel(10, false);
        let (_tx_sig, rx_sig) = channel(10, false);
        let (tx_ack, _rx_ack) = channel(10, false);
        let (tx_mon, _rx_mon) = channel::<AtlasScientificMonitorView>(10, false);

        let mut channels = AtlasScientificChannels {
            tx_atlas_scientific_to_i2c_interface: tx_i2c,
            rx_atlas_scientific_from_i2c_interface: rx_i2c_res,
            tx_atlas_scientific_to_signal_handler: tx_ack,
            rx_atlas_scientific_from_signal_handler: rx_sig,
            tx_atlas_scientific_to_monitors: tx_mon,
        };

        // 1. Success path
        assert!(oem.send_request_to_i2c_interface(&mut channels).is_ok());

        // 2. Disconnected channel error path
        drop(rx_i2c);
        let err = oem.send_request_to_i2c_interface(&mut channels);
        assert!(matches!(
            err,
            Err(AtlasScientificError::SendingRequestToI2cInterfaceFailed { .. })
        ));
    }

    #[test]
    // Verifies receive_response_from_i2c_interface handles successful responses, disconnected channels, invalid signals, and I2C failures.
    /// [utest->swreq~sensors_read_data~1]
    fn test_receive_response_from_i2c_interface() {
        let mut oem = AtlasScientificOem::new(
            0x66,
            atlas_scientific_oem_constants::DEVICE_TYPE_TEMPERATURE,
            10,
            10,
            atlas_scientific_oem_constants::REG_DATA_MSB_TEMPERATURE,
            atlas_scientific_oem_constants::SCALE_FACTOR_TEMPERATURE,
            atlas_scientific_oem_constants::RESPONSE_LENGTH_TEMPERATURE,
        );

        let (tx_i2c, _rx_i2c) = channel(10, false);
        let (mut tx_i2c_res, rx_i2c_res) = channel::<I2cResult>(10, false);
        let (_tx_sig, rx_sig) = channel(10, false);
        let (tx_ack, _rx_ack) = channel(10, false);
        let (tx_mon, _rx_mon) = channel::<AtlasScientificMonitorView>(10, false);

        let mut channels = AtlasScientificChannels {
            tx_atlas_scientific_to_i2c_interface: tx_i2c,
            rx_atlas_scientific_from_i2c_interface: rx_i2c_res,
            tx_atlas_scientific_to_signal_handler: tx_ack,
            rx_atlas_scientific_from_signal_handler: rx_sig,
            tx_atlas_scientific_to_monitors: tx_mon,
        };

        let mutex_temp = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_cond = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));

        // 1. Success path for WaterTemperature
        let val_25000: u32 = 25000;
        let mut read_buf = [0u8; 64];
        read_buf[..4].copy_from_slice(&val_25000.to_be_bytes());
        let response_ok = I2cResponse {
            read_buf,
            length: 4,
        };
        tx_i2c_res.send(Ok(response_ok)).unwrap();

        let res = oem.receive_response_from_i2c_interface(
            &AquariumSignal::WaterTemperature,
            &mut channels,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );
        assert!(res.is_ok());
        assert_eq!(res.unwrap().value, 25.0);

        // 2. InvalidSignal error path
        tx_i2c_res
            .send(Ok(I2cResponse {
                read_buf: [0u8; 64],
                length: 4,
            }))
            .unwrap();
        let res_invalid = oem.receive_response_from_i2c_interface(
            &AquariumSignal::AmbientTemperature,
            &mut channels,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );
        assert!(matches!(
            res_invalid,
            Err(AtlasScientificError::InvalidSignal(..))
        ));

        // 3. I2cCommunicationFailure error path
        tx_i2c_res
            .send(Err(I2cError::WriteFailure {
                location: module_path!().to_string(),
            }))
            .unwrap();
        let res_i2c_err = oem.receive_response_from_i2c_interface(
            &AquariumSignal::WaterTemperature,
            &mut channels,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );
        assert!(matches!(
            res_i2c_err,
            Err(AtlasScientificError::I2cCommunicationFailure { .. })
        ));

        // 4. ChannelIsDisconnected error path
        drop(tx_i2c_res);
        let res_disc = oem.receive_response_from_i2c_interface(
            &AquariumSignal::WaterTemperature,
            &mut channels,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );
        assert!(matches!(
            res_disc,
            Err(AtlasScientificError::ChannelIsDisconnected { .. })
        ));
    }

    #[test]
    // Verifies device_init wake-up, device type verification, and length/device-type error paths.
    /// [utest->swreq~sensors_read_data~1]
    fn test_device_init() {
        let oem = AtlasScientificOem::new(
            0x66,
            atlas_scientific_oem_constants::DEVICE_TYPE_TEMPERATURE,
            10,
            10,
            atlas_scientific_oem_constants::REG_DATA_MSB_TEMPERATURE,
            atlas_scientific_oem_constants::SCALE_FACTOR_TEMPERATURE,
            atlas_scientific_oem_constants::RESPONSE_LENGTH_TEMPERATURE,
        );

        let (tx_i2c, _rx_i2c) = channel(10, false);
        let (mut tx_i2c_res, rx_i2c_res) = channel::<I2cResult>(10, false);
        let (_tx_sig, rx_sig) = channel(10, false);
        let (tx_ack, _rx_ack) = channel(10, false);
        let (tx_mon, _rx_mon) = channel::<AtlasScientificMonitorView>(10, false);

        let mut channels = AtlasScientificChannels {
            tx_atlas_scientific_to_i2c_interface: tx_i2c,
            rx_atlas_scientific_from_i2c_interface: rx_i2c_res,
            tx_atlas_scientific_to_signal_handler: tx_ack,
            rx_atlas_scientific_from_signal_handler: rx_sig,
            tx_atlas_scientific_to_monitors: tx_mon,
        };

        // 1. Success path: wake-up response OK, type check response OK
        tx_i2c_res
            .send(Ok(I2cResponse {
                read_buf: [0u8; 64],
                length: 0,
            }))
            .unwrap();
        let mut type_buf = [0u8; 64];
        type_buf[0] = atlas_scientific_oem_constants::DEVICE_TYPE_TEMPERATURE;
        type_buf[1] = 1; // firmware version
        tx_i2c_res
            .send(Ok(I2cResponse {
                read_buf: type_buf,
                length: 2,
            }))
            .unwrap();

        assert!(oem
            .device_init(&mut channels, &AquariumSignal::WaterTemperature)
            .is_ok());

        // 2. ResponseTooLong error path (length = 3 > 2)
        tx_i2c_res
            .send(Ok(I2cResponse {
                read_buf: [0u8; 64],
                length: 0,
            }))
            .unwrap();
        tx_i2c_res
            .send(Ok(I2cResponse {
                read_buf: [0u8; 64],
                length: 3,
            }))
            .unwrap();
        let res_long = oem.device_init(&mut channels, &AquariumSignal::WaterTemperature);
        assert!(matches!(
            res_long,
            Err(AtlasScientificError::ResponseTooLong { .. })
        ));

        // 3. ResponseTooShort error path (length = 1 < 2)
        tx_i2c_res
            .send(Ok(I2cResponse {
                read_buf: [0u8; 64],
                length: 0,
            }))
            .unwrap();
        tx_i2c_res
            .send(Ok(I2cResponse {
                read_buf: [0u8; 64],
                length: 1,
            }))
            .unwrap();
        let res_short = oem.device_init(&mut channels, &AquariumSignal::WaterTemperature);
        assert!(matches!(
            res_short,
            Err(AtlasScientificError::ResponseTooShort { .. })
        ));

        // 4. InvalidDeviceType error path (read_buf[0] != device_type)
        tx_i2c_res
            .send(Ok(I2cResponse {
                read_buf: [0u8; 64],
                length: 0,
            }))
            .unwrap();
        let mut wrong_type_buf = [0u8; 64];
        wrong_type_buf[0] = 0x99; // Unexpected device type code
        tx_i2c_res
            .send(Ok(I2cResponse {
                read_buf: wrong_type_buf,
                length: 2,
            }))
            .unwrap();
        let res_wrong = oem.device_init(&mut channels, &AquariumSignal::WaterTemperature);
        assert!(matches!(
            res_wrong,
            Err(AtlasScientificError::InvalidDeviceType { .. })
        ));

        // 5. I2cCommunicationFailure error path
        tx_i2c_res
            .send(Ok(I2cResponse {
                read_buf: [0u8; 64],
                length: 0,
            }))
            .unwrap();
        tx_i2c_res
            .send(Err(I2cError::WriteFailure {
                location: module_path!().to_string(),
            }))
            .unwrap();
        let res_i2c_err = oem.device_init(&mut channels, &AquariumSignal::WaterTemperature);
        assert!(matches!(
            res_i2c_err,
            Err(AtlasScientificError::I2cCommunicationFailure { .. })
        ));
    }

    #[test]
    // Verifies get_mutex_access_duration_exceeded reporting across temperature, pH, and conductivity signals.
    /// [utest->swreq~sensors_read_data~1]
    fn test_get_mutex_access_duration_exceeded() {
        let mut oem = AtlasScientificOem::new(
            0x66,
            atlas_scientific_oem_constants::DEVICE_TYPE_TEMPERATURE,
            10,
            10,
            atlas_scientific_oem_constants::REG_DATA_MSB_TEMPERATURE,
            atlas_scientific_oem_constants::SCALE_FACTOR_TEMPERATURE,
            atlas_scientific_oem_constants::RESPONSE_LENGTH_TEMPERATURE,
        );

        oem.mutex_temperature_access_duration_exceeded = true;
        oem.mutex_ph_access_duration_exceeded = true;
        oem.mutex_conductivity_access_duration_exceeded = true;

        assert!(oem.get_mutex_access_duration_exceeded(&AquariumSignal::WaterTemperature));
        assert!(oem.get_mutex_access_duration_exceeded(&AquariumSignal::pH));
        assert!(oem.get_mutex_access_duration_exceeded(&AquariumSignal::Conductivity));
    }
}
