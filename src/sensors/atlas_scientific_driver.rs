/* Copyright 2026 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the abstraction layer for Atlas Scientific sensor drivers.
//!
//! This module contains the `AtlasScientificDriver` trait, which serves as the interface
//! for the Strategy pattern used by the `AtlasScientific` thread. By defining a common
//! interface for sensor interaction, the system can support different hardware protocols
//! (specifically the consumer-grade "EZO" circuits and the industrial "OEM" modules)
//! interchangeably without modifying the core scheduling and data management logic.
//!
//! ## Key Responsibilities
//!
//! Implementations of this trait are responsible for:
//!
//! 1.  **Protocol Encapsulation**: Hiding the details of the I2C command structure (e.g.,
//!     ASCII commands for EZO vs. binary register access for OEM).
//! 2.  **Request Generation**: Constructing the appropriate `I2cRequest` to trigger a measurement.
//! 3.  **Response Parsing**: Interpreting the raw bytes received from the I2C interface
//!     and converting them into a normalized `f32` value.
//! 4.  **Initialization**: Performing any necessary hardware setup (e.g., waking up OEM sensors).
//!
//! ## Usage
//!
//! The `AtlasScientific` struct holds a collection of `Box<dyn AtlasScientificDriver>`,
//! keyed by the sensor signal type. During the execution loop, it delegates the actual
//! work of sending requests and processing responses to the specific driver instance
//! associated with the current sensor being polled.

use crate::sensors::atlas_scientific::{AtlasScientificResult, AtlasScientificResultData};
use crate::sensors::atlas_scientific_channels::AtlasScientificChannels;
use crate::sensors::atlas_scientific_error::AtlasScientificError;
use crate::utilities::channel_content::AquariumSignal;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

/// Abstract strategy for interacting with an Atlas Scientific sensor.
/// Allows for different sensor types ("oem", "ezo") to be integrated.
pub trait AtlasScientificDriver: Send + Sync + Debug {
    /// Constructs and sends a measurement request to the I2C interface thread.
    ///
    /// This function encapsulates the logic for creating the specific I2C command required
    /// by the sensor hardware (e.g., ASCII 'R' for EZO sensors or register address access
    /// for OEM sensors). It packages this command into an `I2cRequest` and sends it via
    /// the provided channel to the `I2cInterface` thread.
    ///
    /// # Arguments
    /// * `atlas_scientific_channels` - A mutable reference to the struct containing the
    ///   communication channels. Specifically, the `tx_atlas_scientific_to_i2c_interface`
    ///   sender is used.
    ///
    /// # Returns
    /// * `Ok(())` - If the request was successfully created and sent to the channel.
    /// * `Err(AtlasScientificError)` - If the request could not be created (e.g., invalid
    ///   parameters) or if the channel send failed (e.g., the I2C thread is disconnected).
    fn send_request_to_i2c_interface(
        &self,
        atlas_scientific_channels: &mut AtlasScientificChannels,
    ) -> Result<(), AtlasScientificError>;

    /// Receives the raw response from the I2C interface, parses it, and updates the shared state.
    ///
    /// This function performs the second half of the measurement cycle. It waits for (or checks)
    /// the response channel from the `I2cInterface`. Upon receiving data, it parses the
    /// raw bytes according to the specific sensor protocol (e.g., parsing an ASCII string
    /// for EZO or decoding Big Endian bytes for OEM).
    ///
    /// If parsing is successful, the result is written to the appropriate `Mutex` based on
    /// the provided `signal`.
    ///
    /// # Arguments
    /// * `signal` - The type of sensor signal currently being processed (e.g., pH, Temperature).
    ///   This is used to determine how to parse the data and which mutex to update.
    /// * `atlas_scientific_channels` - A mutable reference to the struct containing the
    ///   communication channels. Specifically, the `rx_atlas_scientific_from_i2c_interface`
    ///   receiver is used.
    /// * `mutex_water_temperature` - The shared mutex for water temperature data.
    /// * `mutex_ph` - The shared mutex for pH data.
    /// * `mutex_conductivity` - The shared mutex for conductivity data.
    ///
    /// # Returns
    /// * `Ok(AtlasScientificResultData)` - The successfully parsed measurement data.
    /// * `Err(AtlasScientificError)` - If communication failed, the channel was empty,
    ///   parsing failed, or the mutex could not be locked.
    fn receive_response_from_i2c_interface(
        &mut self,
        signal: &AquariumSignal,
        atlas_scientific_channels: &mut AtlasScientificChannels,
        mutex_water_temperature: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_ph: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_conductivity: &Arc<Mutex<AtlasScientificResultData>>,
    ) -> AtlasScientificResult;

    #[cfg(any(all(target_os = "linux", feature = "target_hw"), test))]
    /// Performs hardware-specific initialization for the sensor.
    ///
    /// This function is called once during the startup phase. It allows the driver to
    /// perform any necessary setup steps, such as waking up the device (required for OEM
    /// sensors) or verifying the device type.
    ///
    /// # Arguments
    /// * `atlas_scientific_channels` - Channels used to communicate with the I2C interface
    ///   to perform initialization transactions.
    /// * `signal` - The signal type associated with this driver instance.
    ///
    /// # Returns
    /// * `Ok(())` - If initialization was successful or if no initialization is required.
    /// * `Err(AtlasScientificError)` - If the device failed to initialize or responded incorrectly.
    fn device_init(
        &self,
        atlas_scientific_channels: &mut AtlasScientificChannels,
        signal: &AquariumSignal,
    ) -> Result<(), AtlasScientificError>;

    #[cfg(test)]
    /// Helper function for testing mutex contention logic.
    ///
    /// This function allows unit tests to verify if the driver correctly detected and
    /// flagged a situation where acquiring the shared mutex took longer than the
    /// configured threshold.
    ///
    /// # Arguments
    /// * `signal` - The signal type to check the flag for.
    ///
    /// # Returns
    /// * `true` - If the mutex access duration exceeded the limit during the last operation.
    /// * `false` - Otherwise.
    fn get_mutex_access_duration_exceeded(&self, signal: &AquariumSignal) -> bool;
}
