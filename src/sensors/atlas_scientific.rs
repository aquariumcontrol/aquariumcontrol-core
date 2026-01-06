/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Implements the sensor management thread for Atlas Scientific I2C devices.
//!
//! This module provides the `AtlasScientific` struct, which runs as a dedicated, long-running
//! thread. Its primary responsibility is to periodically and sequentially measure water
//! temperature, pH, and conductivity. It acts as a client to the `I2cInterface` thread,
//! sending measurement requests and processing the responses. The final, parsed sensor
//! values are stored in shared `Mutex`-protected data structures, making them available
//! to the rest of the application (e.g., the `DataLogger`).
//!
//! ## Key Components
//!
//! - **`AtlasScientific` Struct**: The core state machine for the thread. It holds the
//!   configuration, a number of stateful boolean "lock" flags for log
//!   suppression, and the current state of the measurement cycle.
//!
//! - **`execute()` Method**: The main thread loop. It implements a sequential measurement
//!   state machine:
//!   1.  **Wait**: Waits for the configured `measurement_interval` to elapse.
//!   2.  **Send**: Sends a request for the *next* sensor in the sequence (Temperature -> pH -> Conductivity)
//!       to the `I2cInterface` thread.
//!   3.  **Receive**: Polls the response channel from the `I2cInterface`.
//!   4.  **Process**: If a response is received, it is parsed and validated. The result is
//!       written to the appropriate shared `Mutex`.
//!   5.  **Cycle**: The state machine advances to the next sensor and returns to the **Wait** step.
//!
//! - **`check_response()` Method**: A critical private helper that parses and validates the
//!   proprietary 8-byte response from an Atlas Scientific sensor, checking magic numbers
//!   and ensuring the payload is a valid number.
//!
//! ## Design and Architecture
//!
//! This module is designed for robust, decoupled, and sequential sensor reading.
//!
//! - **Sequential Measurement**: The module does not attempt to read all sensors at once.
//!   This serialized approach simplifies the logic and avoids potential conflicts on the
//!   I2C bus if it were shared.
//!
//! - **Decoupling via I2C Interface**: The `AtlasScientific` thread does not perform
//!   hardware I/O directly. It is a client of the `I2cInterface` thread. This decouples
//!   the sensor-specific protocol logic from the low-level bus management, making both
//!   modules simpler and more testable.
//!
//! - **Shared Data via Mutexes**: The module's "outputs" are the `Arc<Mutex<...>>` data
//!   structures it updates. This allows other threads to safely access the latest sensor.
//!
//! - **Robust Error Handling**:
//!   - **Log Flood Prevention**: The use of `lock_...` flags prevents a single, persistent
//!     issue (like a disconnected channel or a consistently slow mutex) from filling up
//!     disk space with repetitive logs.
//!   - **Mutex Access Monitoring**: The module actively monitors how long it takes to
//!     acquire a lock on a shared mutex. If the wait time exceeds a threshold, it logs a
//!     warning, which can help diagnose performance bottlenecks or deadlocks elsewhere
//!     in the application.
//!
//! ### Atlas Scientific 8-Byte Protocol
//!
//! The `check_response` function implements the parsing of a fixed-size 8-byte packet:
//!
//! | Byte   | Name        | Description                                        |
//! | -------| :---------- | -------------------------------------------------- |
//! | `[0]`  | `MagicStart`| A status byte, expected to be `1` for success.     |
//! | `[1-6]`| `Payload`   | A 6-byte ASCII string representing the float value.|
//! | `[7]`  | `MagicEnd`  | A null terminator, expected to be `0`.             |

#![allow(non_snake_case)]

#[cfg(all(not(test), target_os = "linux"))]
use log::info;
use std::collections::HashMap;
use std::fmt;

use crate::sensors::atlas_scientific_channels::AtlasScientificChannels;
use crate::sensors::atlas_scientific_config::AtlasScientificConfig;
use crate::sensors::atlas_scientific_driver::AtlasScientificDriver;
use crate::sensors::atlas_scientific_error::AtlasScientificError;
#[cfg(any(all(feature = "target_hw", target_os = "linux"), test))]
use crate::sensors::atlas_scientific_ezo::AtlasScientificEzo;
#[cfg(any(all(feature = "target_hw", target_os = "linux"), test))]
use crate::sensors::atlas_scientific_oem::atlas_scientific_oem_constants;
#[cfg(any(all(feature = "target_hw", target_os = "linux"), test))]
use crate::sensors::atlas_scientific_oem::AtlasScientificOem;
use crate::simulator::get_resp_sim::GetResponseFromSimulatorTrait;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::channel_content::AquariumSignal;
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;
#[cfg(all(not(test), target_os = "linux"))]
use nix::unistd::gettid;
use spin_sleep::SpinSleeper;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// max loops for receiving when the quit command has been received
#[allow(unused)] // used in conditionally compiled code
pub const RECEIVE_COUNTER_MAX: u32 = 10;

#[derive(Clone, Debug)]
pub struct AtlasScientificResultData {
    /// The measurement value
    pub value: f32,

    /// Bit indicating if the value is valid or not
    #[allow(unused)]
    invalid: bool,

    /// The instant when the measurement result was recorded
    #[allow(unused)]
    measurement_instant: Instant,
}

impl AtlasScientificResultData {
    pub fn new(value: f32) -> Self {
        AtlasScientificResultData {
            value,
            invalid: false,
            measurement_instant: Instant::now(),
        }
    }
}

impl fmt::Display for AtlasScientificResultData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Write the floating-point sensor value directly to the formatter.
        write!(f, "{}", self.value)
    }
}

#[allow(unused)] // used in conditionally compiled code
pub type AtlasScientificResult = Result<AtlasScientificResultData, AtlasScientificError>;

/// Device-type-independent constants for AtlasScientific
pub mod atlas_scientific_constants {

    #[cfg(any(all(feature = "target_hw", target_os = "linux"), test))]
    /// allow max. 10 milliseconds for mutex to be blocked by any other thread
    pub const MAX_MUTEX_ACCESS_DURATION_MILLIS: u64 = 10;

    #[cfg(any(all(feature = "target_hw", target_os = "linux"), test))]
    pub const SENSOR_TYPE_OEM: &str = "oem";

    #[cfg(any(all(feature = "target_hw", target_os = "linux"), test))]
    pub const SENSOR_TYPE_EZO: &str = "ezo";
}

/// Contains the communication Atlas Scientific sensors via I2C.
/// Alternatively, TCP communication is used when configured to run with the simulator.
/// The struct holds attributes for the results and for error flags indicating unsuccessful communication.
/// Sensor data is read periodically and stored.
/// Requests from data logger, ventilation, and heating control are answered with last measured data.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// Creates the channels and sets up threads.
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     atlas_scientific --> signal_handler[SignalHandler]
///     signal_handler --> atlas_scientific
///     atlas_scientific -.-> data_logger[DataLogger]
/// ```
#[derive(Debug)]
pub struct AtlasScientific {
    /// configuration data for AtlasScientific
    #[allow(unused)] // used in conditionally compiled code
    config: AtlasScientificConfig,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    #[allow(unused)] // used in conditionally compiled code
    pub(crate) lock_error_channel_receive_termination: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via the channel from the signal handler thread
    #[allow(unused)] // used in conditionally compiled code
    pub(crate) lock_warn_inapplicable_command_signal_handler: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about to send request via the channel to I2cInterface thread
    lock_error_channel_send_i2c_interface: bool,

    /// instant when the last measurement took place
    #[allow(unused)] // used in conditionally compiled code
    pub last_measurement_instant: Instant,

    /// duration of the measurement interval calculated from the configuration data
    #[allow(unused)] // used in conditionally compiled code
    pub measurement_interval: Duration,

    /// Drivers for each connected sensor, keyed by the signal type.
    drivers: HashMap<AquariumSignal, Box<dyn AtlasScientificDriver>>,

    lock_error_driver_not_found: HashMap<AquariumSignal, bool>,
}

impl ProcessExternalRequestTrait for AtlasScientific {}

impl GetResponseFromSimulatorTrait for AtlasScientific {}

impl AtlasScientific {
    #[cfg(any(all(feature = "target_hw", target_os = "linux"), test))]
    pub fn create_drivers(
        config: &AtlasScientificConfig,
    ) -> Result<HashMap<AquariumSignal, Box<dyn AtlasScientificDriver>>, AtlasScientificError> {
        let mut drivers: HashMap<AquariumSignal, Box<dyn AtlasScientificDriver>> = HashMap::new();

        let driver_temperature: Box<dyn AtlasScientificDriver> =
            match config.sensor_type_temperature.as_str() {
                atlas_scientific_constants::SENSOR_TYPE_OEM => Box::new(AtlasScientificOem::new(
                    config.address_atlas_scientific_temperature,
                    atlas_scientific_oem_constants::REG_TYPE_TEMPERATURE,
                    config.sleep_time_millis_temperature,
                    atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
                    atlas_scientific_oem_constants::REG_DATA_MSB_TEMPERATURE,
                    atlas_scientific_oem_constants::SCALE_FACTOR_TEMPERATURE,
                    atlas_scientific_oem_constants::RESPONSE_LENGTH_TEMPERATURE,
                )),
                atlas_scientific_constants::SENSOR_TYPE_EZO => Box::new(AtlasScientificEzo::new(
                    config.address_atlas_scientific_temperature,
                    config.sleep_time_millis_temperature,
                    atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
                )),
                _ => {
                    return Err(AtlasScientificError::InvalidSensorType {
                        location: module_path!().to_string(),
                        signal: AquariumSignal::WaterTemperature,
                        sensor_type: config.sensor_type_temperature.clone(),
                    })
                }
            };
        drivers.insert(AquariumSignal::WaterTemperature, driver_temperature);

        let driver_ph: Box<dyn AtlasScientificDriver> = match config.sensor_type_ph.as_str() {
            atlas_scientific_constants::SENSOR_TYPE_OEM => Box::new(AtlasScientificOem::new(
                config.address_atlas_scientific_ph,
                atlas_scientific_oem_constants::REG_TYPE_PH,
                config.sleep_time_millis_ph,
                atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
                atlas_scientific_oem_constants::REG_DATA_MSB_PH,
                atlas_scientific_oem_constants::SCALE_FACTOR_PH,
                atlas_scientific_oem_constants::RESPONSE_LENGTH_PH,
            )),
            atlas_scientific_constants::SENSOR_TYPE_EZO => Box::new(AtlasScientificEzo::new(
                config.address_atlas_scientific_ph,
                config.sleep_time_millis_ph,
                atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
            )),
            _ => {
                return Err(AtlasScientificError::InvalidSensorType {
                    location: module_path!().to_string(),
                    signal: AquariumSignal::pH,
                    sensor_type: config.sensor_type_ph.clone(),
                })
            }
        };
        drivers.insert(AquariumSignal::pH, driver_ph);

        let driver_conductivity: Box<dyn AtlasScientificDriver> =
            match config.sensor_type_conductivity.as_str() {
                atlas_scientific_constants::SENSOR_TYPE_OEM => Box::new(AtlasScientificOem::new(
                    config.address_atlas_scientific_conductivity,
                    atlas_scientific_oem_constants::REG_TYPE_CONDUCTIVITY,
                    config.sleep_time_millis_conductivity,
                    atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
                    atlas_scientific_oem_constants::REG_DATA_MSB_CONDUCTIVITY,
                    atlas_scientific_oem_constants::SCALE_FACTOR_CONDUCTIVITY,
                    atlas_scientific_oem_constants::RESPONSE_LENGTH_CONDUCTIVITY,
                )),
                atlas_scientific_constants::SENSOR_TYPE_EZO => Box::new(AtlasScientificEzo::new(
                    config.address_atlas_scientific_conductivity,
                    config.sleep_time_millis_conductivity,
                    atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
                )),
                _ => {
                    return Err(AtlasScientificError::InvalidSensorType {
                        location: module_path!().to_string(),
                        signal: AquariumSignal::Conductivity,
                        sensor_type: config.sensor_type_conductivity.clone(),
                    })
                }
            };
        drivers.insert(AquariumSignal::Conductivity, driver_conductivity);
        Ok(drivers)
    }

    /// Creates a new `AtlasScientific` control instance.
    ///
    /// This constructor initializes the Atlas Scientific sensor module. It takes the
    /// configuration, instantiates the measurement interval, and sets up internal
    /// state, including lock flags used to prevent log flooding from recurring
    /// errors or warnings.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the Atlas Scientific module, specifying I2C
    ///   addresses, sleep times, and the measurement interval.
    /// * `drivers` - a hashmap containing the driver for each signal
    ///
    /// # Returns
    /// It returns a new `AtlasScientific` struct, which is ready to manage sensor data acquisition.
    #[allow(unused)] // used in conditionally compiled code
    pub fn new(
        config: AtlasScientificConfig,
        drivers: HashMap<AquariumSignal, Box<dyn AtlasScientificDriver>>,
    ) -> AtlasScientific {
        let measurement_interval = Duration::from_millis(config.measurement_interval_millis);

        AtlasScientific {
            config,
            lock_error_channel_receive_termination: false,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_send_i2c_interface: false,
            last_measurement_instant: Instant::now(),
            measurement_interval,
            drivers,
            lock_error_driver_not_found: HashMap::new(),
        }
    }

    /// Atomically writes a sensor measurement result to the appropriate shared mutex.
    ///
    /// This helper function directs an `AtlasScientificResultData` to its corresponding
    /// shared `Mutex` based on the provided `signal`. It centralizes the
    /// mutex selection and locking logic.
    ///
    /// # Arguments
    /// * `signal` - The `AquariumSignal` that determines which mutex to update.
    /// * `result` - The `AtlasScientificResultData` to be written.
    /// * `mutex_water_temperature` - The shared mutex for the water temperature result.
    /// * `mutex_ph` - The shared mutex for the pH sensor result.
    /// * `mutex_conductivity` - The shared mutex for the conductivity sensor result.
    ///
    /// # Returns
    /// A `Result` containing the `Instant` at which the lock was released on success.
    ///
    /// # Errors
    /// Returns `Err(AtlasScientificError::CouldNotLockMutex)` if the target mutex is
    /// poisoned and cannot be locked.
    #[allow(unused)] // used in conditionally compiled code
    pub(crate) fn write_result_to_mutex(
        signal: &AquariumSignal,
        result: AtlasScientificResultData,
        mutex_water_temperature: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_ph: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_conductivity: &Arc<Mutex<AtlasScientificResultData>>,
    ) -> Result<Instant, AtlasScientificError> {
        let target_mutex = match signal {
            AquariumSignal::WaterTemperature => mutex_water_temperature,
            AquariumSignal::pH => mutex_ph,
            AquariumSignal::Conductivity => mutex_conductivity,
            // For unsupported signals, do nothing and return success, matching original behavior.
            _ => return Ok(Instant::now()),
        };

        match target_mutex.lock() {
            Ok(mut guard) => *guard = result,
            Err(_) => {
                return Err(AtlasScientificError::CouldNotLockMutex {
                    location: module_path!().to_string(),
                    signal: signal.clone(),
                });
            }
        }
        Ok(Instant::now())
    }

    /// Helper function for retrieving the driver from the hash map
    fn get_driver(
        &mut self,
        signal: &AquariumSignal,
    ) -> Result<&mut Box<dyn AtlasScientificDriver>, AtlasScientificError> {
        match self.drivers.get_mut(signal) {
            Some(driver) => Ok(driver),
            None => Err(AtlasScientificError::DriverNotFound {
                location: module_path!().to_string(),
                signal: signal.clone(),
            }),
        }
    }

    /// Iterates over all registered drivers and verifies their device type.
    /// This requires the I2cInterface thread to be running.
    #[cfg(any(all(target_os = "linux", feature = "target_hw"), test))]
    pub fn verify_hardware(
        &self,
        channels: &mut AtlasScientificChannels,
    ) -> Result<(), AtlasScientificError> {
        for (signal, driver) in &self.drivers {
            // We verify the driver.
            // Note: check_device_type is defined in the Trait and takes &self
            driver.check_device_type(channels, signal)?;
        }
        Ok(())
    }

    /// Executes the main control loop for the Atlas Scientific sensor module.
    ///
    /// This function runs continuously, managing the periodic and sequential measurement of
    /// water temperature, pH, and conductivity. It interacts with an I2C interface
    /// thread by sending requests for one sensor at a time and then processing the response.
    ///
    /// The collected data (or any resulting error) is written to a shared `Mutex`,
    /// making it available to other application threads. The loop respects the configured
    /// `measurement_interval` between cycles and remains responsive to `Quit` and
    /// `Terminate` commands from the signal handler for a graceful shutdown.
    ///
    /// # Arguments
    /// * `atlas_scientific_channels` - A mutable reference to the struct containing all `mpsc` channels necessary for
    ///   inter-thread communication with the `I2CInterface` and the `SignalHandler`.
    /// * `mutex_atlas_scientific_water_temperature` - The shared `Mutex` for storing the water
    ///   temperature result.
    /// * `mutex_atlas_scientific_ph` - The shared `Mutex` for storing the pH result.
    /// * `mutex_atlas_scientific_conductivity` - The shared `Mutex` for storing the conductivity result.
    #[allow(unused)] // used in conditionally compiled code
    pub fn execute(
        &mut self,
        atlas_scientific_channels: &mut AtlasScientificChannels,
        mutex_atlas_scientific_water_temperature: Arc<Mutex<AtlasScientificResultData>>,
        mutex_atlas_scientific_ph: Arc<Mutex<AtlasScientificResultData>>,
        mutex_atlas_scientific_conductivity: Arc<Mutex<AtlasScientificResultData>>,
    ) {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let sleep_duration_ten_millis = Duration::from_millis(10);
        let sleep_duration_one_millis = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        let mut state_send_receive: bool = false;
        let mut channel_is_disconnected: bool = false;
        let mut receive_counter: u32 = 0;

        let mut signal = AquariumSignal::WaterTemperature;

        loop {
            let (
                quit_command_received, // the request to end the application has been received
                _,
                _,
            ) = self.process_external_request(
                &mut atlas_scientific_channels.rx_atlas_scientific_from_signal_handler,
                None,
            );

            if quit_command_received
                && (!state_send_receive || receive_counter > RECEIVE_COUNTER_MAX)
            {
                // quit command has been received, and there is no pending measurement
                // no further waiting for I2cInterface is required
                break;
            } else if quit_command_received {
                break;
            }

            // Check if data acquisition is enabled.
            if self.config.active && !channel_is_disconnected {
                let duration_since_last_measurement =
                    Instant::now().duration_since(self.last_measurement_instant);
                if !state_send_receive
                    && duration_since_last_measurement > self.measurement_interval
                {
                    #[cfg(test)]
                    println!(
                        "{}: Sending to i2c interface request for {}",
                        module_path!(),
                        signal
                    );

                    match self.get_driver(&signal) {
                        Ok(driver) => {
                            let send_result =
                                driver.send_request_to_i2c_interface(atlas_scientific_channels);
                            state_send_receive = match send_result {
                                Ok(()) => {
                                    self.lock_error_channel_send_i2c_interface = false;
                                    true
                                }
                                Err(e) => {
                                    if !matches!(
                                        e,
                                        AtlasScientificError::SendingRequestToI2cInterfaceFailed { .. }
                                    ) || !self.lock_error_channel_send_i2c_interface
                                    {
                                        log_error_chain(
                                            module_path!(),
                                            "Error in communication with I2C thread.",
                                            e,
                                        );
                                    }
                                    false
                                }
                            };
                        }
                        Err(e) => {
                            if !self
                                .lock_error_driver_not_found
                                .get(&signal)
                                .unwrap_or(&false)
                            {
                                log_error_chain(
                                    module_path!(),
                                    "Internal error when trying to send signal request: ",
                                    e,
                                );
                                self.lock_error_driver_not_found
                                    .insert(signal.clone(), true);
                            }
                        }
                    };

                    #[cfg(test)]
                    println!(
                        "{}: Sent to i2c interface request for {}",
                        module_path!(),
                        signal
                    );
                } else if state_send_receive {
                    // read from the channel if the measurement result is available from I2cInterface thread
                    match self.get_driver(&signal) {
                        Ok(driver) => {
                            match driver.receive_response_from_i2c_interface(
                                &signal,
                                atlas_scientific_channels,
                                &mutex_atlas_scientific_water_temperature,
                                &mutex_atlas_scientific_ph,
                                &mutex_atlas_scientific_conductivity,
                            ) {
                                Ok(_) => {
                                    state_send_receive = false; // allow sending again
                                }
                                Err(e) => {
                                    if matches!(e, AtlasScientificError::ChannelIsEmpty(_)) {
                                        state_send_receive = true; // receive again
                                    } else if matches!(
                                        e,
                                        AtlasScientificError::ChannelIsDisconnected {
                                            location: _,
                                            source: _
                                        }
                                    ) {
                                        channel_is_disconnected = true;
                                    } else {
                                        log_error_chain(
                                            module_path!(),
                                            "Encountered error trying to receive from I2C",
                                            e,
                                        );
                                        state_send_receive = false; // allow sending again
                                    }
                                }
                            }

                            if !state_send_receive {
                                // data could be received successfully
                                receive_counter = 0;
                                self.last_measurement_instant = Instant::now();
                                signal = signal.get_next_atlas_scientific_signal();
                            } else {
                                receive_counter = receive_counter.saturating_add(1);
                            }
                        }
                        Err(e) => {
                            if !self
                                .lock_error_driver_not_found
                                .get(&signal)
                                .unwrap_or(&false)
                            {
                                log_error_chain(
                                    module_path!(),
                                    "Internal error when trying to send signal request: ",
                                    e,
                                );
                                self.lock_error_driver_not_found
                                    .insert(signal.clone(), true);
                            }
                        }
                    };
                }
            }

            spin_sleeper.sleep(sleep_duration_ten_millis);
        }

        atlas_scientific_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads.
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination(
            &mut atlas_scientific_channels.rx_atlas_scientific_from_signal_handler,
            sleep_duration_one_millis,
            module_path!(),
        );
    }
}

#[cfg(test)]
pub mod tests {
    use crate::launch::channels::Channels;
    use crate::mocks::mock_i2c_interface::tests::MockI2cInterface;
    use crate::sensors::atlas_scientific::{
        atlas_scientific_constants, AtlasScientific, AtlasScientificError,
        AtlasScientificResultData,
    };
    use crate::sensors::atlas_scientific_ezo::atlas_scientific_ezo_constants;
    use crate::sensors::i2c_interface::I2cResponse;
    use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
    use crate::utilities::config::{read_config_file, ConfigData};
    use spin_sleep::SpinSleeper;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    // Test function receive_response_from_i2c_interface
    pub fn test_atlas_scientific_interfaces_with_blocking_mutexes() {
        let mut stimuli_array_temperature = [0u8; 64];
        let message_temperature: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
            atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
            '2' as u8,
            '3' as u8,
            atlas_scientific_ezo_constants::DECIMAL_POINT,
            '1' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
        ];
        stimuli_array_temperature[..message_temperature.len()]
            .copy_from_slice(&message_temperature);
        let stimuli_i2c_response_temperature = I2cResponse {
            read_buf: stimuli_array_temperature,
            length: atlas_scientific_ezo_constants::MESSAGE_SIZE,
        };

        let mut stimuli_array_conductivity = [0u8; 64];
        let message_conductivity: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
            atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
            '4' as u8,
            '8' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
        ];
        stimuli_array_conductivity[..message_conductivity.len()]
            .copy_from_slice(&message_conductivity);
        let stimuli_i2c_response_conductivity = I2cResponse {
            read_buf: stimuli_array_conductivity,
            length: atlas_scientific_ezo_constants::MESSAGE_SIZE,
        };

        let mut stimuli_array_ph = [0u8; 64];
        let message_ph: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
            atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
            '7' as u8,
            atlas_scientific_ezo_constants::DECIMAL_POINT,
            '2' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
        ];
        stimuli_array_ph[..message_ph.len()].copy_from_slice(&message_ph);
        let stimuli_i2c_response_ph = I2cResponse {
            read_buf: stimuli_array_ph,
            length: atlas_scientific_ezo_constants::MESSAGE_SIZE,
        };

        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.atlas_scientific.sensor_type_temperature =
            atlas_scientific_constants::SENSOR_TYPE_EZO.to_string();
        config.atlas_scientific.sensor_type_ph =
            atlas_scientific_constants::SENSOR_TYPE_EZO.to_string();
        config.atlas_scientific.sensor_type_conductivity =
            atlas_scientific_constants::SENSOR_TYPE_EZO.to_string();

        let measurement_interval_millis_for_test_environment =
            config.atlas_scientific.measurement_interval_millis;

        let mut mock_i2c_interface = MockI2cInterface::new(
            &config.atlas_scientific,
            stimuli_i2c_response_temperature.clone(),
            stimuli_i2c_response_ph.clone(),
            stimuli_i2c_response_conductivity.clone(),
        );

        let drivers = AtlasScientific::create_drivers(&config.atlas_scientific).unwrap();
        let mut atlas_scientific = AtlasScientific::new(config.atlas_scientific, drivers);

        let mut channels = Channels::new_for_test();

        // Mutexes for Atlas Scientific signals
        let mutex_atlas_scientific_temperature =
            Arc::new(Mutex::new(AtlasScientificResultData::new(
                config.sensor_manager.replacement_value_water_temperature,
            )));
        let mutex_atlas_scientific_temperature_clone_for_asserts =
            mutex_atlas_scientific_temperature.clone();
        let mutex_atlas_scientific_temperature_clone_for_test_environment =
            mutex_atlas_scientific_temperature.clone();

        let mutex_atlas_scientific_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(
            config.sensor_manager.replacement_value_ph,
        )));
        let mutex_atlas_scientific_ph_clone_for_asserts = mutex_atlas_scientific_ph.clone();
        let mutex_atlas_scientific_ph_clone_for_test_environment =
            mutex_atlas_scientific_ph.clone();

        let mutex_atlas_scientific_conductivity = Arc::new(Mutex::new(
            AtlasScientificResultData::new(config.sensor_manager.replacement_value_conductivity),
        ));
        let mutex_atlas_scientific_conductivity_clone_for_asserts =
            mutex_atlas_scientific_conductivity.clone();
        let mutex_atlas_scientific_conductivity_clone_for_test_environment =
            mutex_atlas_scientific_conductivity.clone();

        // thread for test environment (incl. signal handler)
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_time =
                    Duration::from_millis(measurement_interval_millis_for_test_environment + 1000);
                let spin_sleeper = SpinSleeper::default();

                // lock the mutexes and wait for triggering the detection
                match mutex_atlas_scientific_temperature_clone_for_test_environment.lock() {
                    Ok(_guard) => {
                        // binding is necessary to avoid drop
                        spin_sleeper.sleep(sleep_time);
                    }
                    Err(e) => {
                        panic!("mutex_atlas_scientific_temperature lock poisoned: {:?}", e);
                    }
                };
                match mutex_atlas_scientific_ph_clone_for_test_environment.lock() {
                    Ok(_guard) => {
                        // binding is necessary to avoid drop
                        spin_sleeper.sleep(sleep_time);
                    }
                    Err(e) => {
                        panic!("mutex_atlas_scientific_ph lock poisoned: {:?}", e);
                    }
                };
                match mutex_atlas_scientific_conductivity_clone_for_test_environment.lock() {
                    Ok(_guard) => {
                        // binding is necessary to avoid drop
                        spin_sleeper.sleep(sleep_time);
                    }
                    Err(e) => {
                        panic!("mutex_atlas_scientific_conductivity lock poisoned: {:?}", e);
                    }
                };

                channels
                    .signal_handler
                    .tx_signal_handler_to_atlas_scientific
                    .send(InternalCommand::Quit)
                    .unwrap();
                channels
                    .signal_handler
                    .tx_signal_handler_to_i2c_interface
                    .send(InternalCommand::Quit)
                    .unwrap();
                channels
                    .signal_handler
                    .tx_signal_handler_to_atlas_scientific
                    .send(InternalCommand::Terminate)
                    .unwrap();
            })
            .unwrap();

        // thread for mock i2c interface
        let join_handle_mock_i2c_interface = thread::Builder::new()
            .name("i2c_interface".to_string())
            .spawn(move || {
                mock_i2c_interface.execute(channels.i2c_interface);
            })
            .unwrap();

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                atlas_scientific.execute(
                    &mut channels.atlas_scientific,
                    mutex_atlas_scientific_temperature,
                    mutex_atlas_scientific_ph,
                    mutex_atlas_scientific_conductivity,
                );
                let driver_temperature = atlas_scientific
                    .drivers
                    .get(&AquariumSignal::WaterTemperature)
                    .unwrap();
                let driver_ph = atlas_scientific.drivers.get(&AquariumSignal::pH).unwrap();
                let driver_conductivity = atlas_scientific
                    .drivers
                    .get(&AquariumSignal::Conductivity)
                    .unwrap();
                assert_eq!(
                    driver_temperature
                        .get_mutex_access_duration_exceeded(&AquariumSignal::WaterTemperature),
                    true
                );
                assert_eq!(
                    driver_ph.get_mutex_access_duration_exceeded(&AquariumSignal::pH),
                    true
                );
                assert_eq!(
                    driver_conductivity
                        .get_mutex_access_duration_exceeded(&AquariumSignal::Conductivity),
                    true
                );
            })
            .unwrap();

        join_handle_test_object
            .join()
            .expect("Test object did not finish.");
        join_handle_mock_i2c_interface
            .join()
            .expect("Mock i2c interface did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment did not finish.");

        match mutex_atlas_scientific_temperature_clone_for_asserts.lock() {
            Ok(result) => {
                let result_data = result.clone();
                assert_eq!(result_data.value, 23.1);
            }
            Err(e) => {
                panic!("mutex_atlas_scientific_temperature lock poisoned: {:?}", e);
            }
        };

        match mutex_atlas_scientific_ph_clone_for_asserts.lock() {
            Ok(result) => {
                let result_data = result.clone();
                assert_eq!(result_data.value, 7.2);
            }
            Err(e) => {
                panic!("mutex_atlas_scientific_ph lock poisoned: {:?}", e);
            }
        };

        match mutex_atlas_scientific_conductivity_clone_for_asserts.lock() {
            Ok(result) => {
                let result_data = result.clone();
                assert_eq!(result_data.value, 48000.0);
            }
            Err(e) => {
                panic!("mutex_atlas_scientific_conductivity lock poisoned: {:?}", e);
            }
        };
    }

    #[test]
    // Test case checks if write_result_to_mutex correctly writes a successful
    // temperature result to the correct mutex.
    fn test_write_result_to_mutex_temperature_success() {
        let initial_temp = AtlasScientificResultData::new(0.0);
        let initial_ph = AtlasScientificResultData::new(0.0);
        let initial_cond = AtlasScientificResultData::new(0.0);

        let mutex_temp = Arc::new(Mutex::new(initial_temp.clone()));
        let mutex_ph = Arc::new(Mutex::new(initial_ph.clone()));
        let mutex_cond = Arc::new(Mutex::new(initial_cond.clone()));

        let new_temp_value = 25.5;
        let new_temp_result = AtlasScientificResultData::new(new_temp_value);

        let before = Instant::now();
        let result = AtlasScientific::write_result_to_mutex(
            &AquariumSignal::WaterTemperature,
            new_temp_result.clone(),
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );
        let after = Instant::now();

        assert!(result.is_ok());
        let result_instant = result.unwrap();
        assert!(result_instant >= before);
        assert!(result_instant <= after);

        // Check that the temperature mutex was updated
        let temp_lock = mutex_temp.lock().unwrap();
        assert_eq!(temp_lock.value, new_temp_value);

        // Check that other mutexes were not changed
        let ph_lock = mutex_ph.lock().unwrap();
        assert_eq!(ph_lock.value, initial_ph.value);
        let cond_lock = mutex_cond.lock().unwrap();
        assert_eq!(cond_lock.value, initial_cond.value);
    }

    #[test]
    // Test case checks if write_result_to_mutex correctly writes a successful
    // pH result to the correct mutex.
    fn test_write_result_to_mutex_ph_success() {
        let mutex_temp = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_cond = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));

        let new_ph_value = 8.2;
        let new_ph_result = AtlasScientificResultData::new(new_ph_value);

        let before = Instant::now();
        let result = AtlasScientific::write_result_to_mutex(
            &AquariumSignal::pH,
            new_ph_result.clone(),
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );
        let after = Instant::now();

        assert!(result.is_ok());
        let result_instant = result.unwrap();
        assert!(result_instant >= before);
        assert!(result_instant <= after);

        // Check that the pH mutex was updated
        let ph_lock = mutex_ph.lock().unwrap();
        assert_eq!(ph_lock.value, new_ph_value);
    }

    #[test]
    // Test case checks if write_result_to_mutex correctly writes a successful
    // conductivity result to the correct mutex.
    fn test_write_result_to_mutex_conductivity_success() {
        let mutex_temp = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_cond = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));

        let new_cond_value = 50000.0;
        let new_cond_result = AtlasScientificResultData::new(new_cond_value);

        let before = Instant::now();
        let result = AtlasScientific::write_result_to_mutex(
            &AquariumSignal::Conductivity,
            new_cond_result.clone(),
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );
        let after = Instant::now();

        assert!(result.is_ok());
        let result_instant = result.unwrap();
        assert!(result_instant >= before);
        assert!(result_instant <= after);

        // Check that the conductivity mutex was updated
        let cond_lock = mutex_cond.lock().unwrap();
        assert_eq!(cond_lock.value, new_cond_value);
    }

    #[test]
    // Test case checks that write_result_to_mutex handles an invalid signal
    // by doing nothing and not changing any mutex values.
    fn test_write_result_to_mutex_invalid_signal() {
        let initial_temp = AtlasScientificResultData::new(1.0);
        let initial_ph = AtlasScientificResultData::new(2.0);
        let initial_cond = AtlasScientificResultData::new(3.0);

        let mutex_temp = Arc::new(Mutex::new(initial_temp.clone()));
        let mutex_ph = Arc::new(Mutex::new(initial_ph.clone()));
        let mutex_cond = Arc::new(Mutex::new(initial_cond.clone()));

        let new_result = AtlasScientificResultData::new(99.0);

        let before = Instant::now();
        let result = AtlasScientific::write_result_to_mutex(
            &AquariumSignal::MockInvalidSignal,
            new_result,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );
        let after = Instant::now();

        assert!(result.is_ok());
        let result_instant = result.unwrap();
        assert!(result_instant >= before);
        assert!(result_instant <= after);

        // Check that no mutex values were changed
        let temp_lock = mutex_temp.lock().unwrap();
        assert_eq!(temp_lock.value, initial_temp.value);
        let ph_lock = mutex_ph.lock().unwrap();
        assert_eq!(ph_lock.value, initial_ph.value);
        let cond_lock = mutex_cond.lock().unwrap();
        assert_eq!(cond_lock.value, initial_cond.value);
    }

    #[test]
    // Test case checks that write_result_to_mutex correctly returns an error
    // when trying to write to a poisoned mutex.
    fn test_write_result_to_mutex_poisoned() {
        let mutex_temp = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_cond = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));

        // Poison the pH mutex by panicking while holding the lock in another thread
        let mutex_ph_clone = mutex_ph.clone();
        let handle = thread::spawn(move || {
            let _lock = mutex_ph_clone.lock().unwrap();
            panic!("Poisoning the mutex for the test");
        });
        assert!(handle.join().is_err()); // The join will return an Err because the thread panicked

        let new_result = AtlasScientificResultData::new(8.0);

        // Attempt to write to the now-poisoned mutex
        let result = AtlasScientific::write_result_to_mutex(
            &AquariumSignal::pH,
            new_result,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );

        // Assert that the correct error is returned
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(AtlasScientificError::CouldNotLockMutex { .. })
        ));
        if let Err(AtlasScientificError::CouldNotLockMutex {
            location: _,
            signal,
        }) = result
        {
            assert_eq!(signal, AquariumSignal::pH);
        }
    }

    #[test]
    // Test case checks if AtlasScientific::create_drivers returns an error if the configuration contains invalid data.
    fn test_new_with_invalid_configuration() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

        // Set an invalid sensor type for temperature
        config.atlas_scientific.sensor_type_temperature = "invalid_type".to_string();
        config.atlas_scientific.sensor_type_ph =
            atlas_scientific_constants::SENSOR_TYPE_OEM.to_string();
        config.atlas_scientific.sensor_type_conductivity =
            atlas_scientific_constants::SENSOR_TYPE_OEM.to_string();

        let result = AtlasScientific::create_drivers(&config.atlas_scientific);

        assert!(result.is_err());
        if let Err(AtlasScientificError::InvalidSensorType {
            location: _,
            signal,
            sensor_type,
        }) = result
        {
            assert_eq!(signal, AquariumSignal::WaterTemperature);
            assert_eq!(sensor_type, "invalid_type");
        } else {
            panic!("Expected InvalidSensorType error, got {:?}", result);
        }

        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

        // Set an invalid sensor type for pH
        config.atlas_scientific.sensor_type_temperature =
            atlas_scientific_constants::SENSOR_TYPE_OEM.to_string();
        config.atlas_scientific.sensor_type_ph = "invalid_type".to_string();
        config.atlas_scientific.sensor_type_conductivity =
            atlas_scientific_constants::SENSOR_TYPE_OEM.to_string();

        let result = AtlasScientific::create_drivers(&config.atlas_scientific);

        assert!(result.is_err());
        if let Err(AtlasScientificError::InvalidSensorType {
            location: _,
            signal,
            sensor_type,
        }) = result
        {
            assert_eq!(signal, AquariumSignal::pH);
            assert_eq!(sensor_type, "invalid_type");
        } else {
            panic!("Expected InvalidSensorType error, got {:?}", result);
        }

        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

        // Set an invalid sensor type for conductivity
        config.atlas_scientific.sensor_type_temperature =
            atlas_scientific_constants::SENSOR_TYPE_OEM.to_string();
        config.atlas_scientific.sensor_type_ph =
            atlas_scientific_constants::SENSOR_TYPE_OEM.to_string();
        config.atlas_scientific.sensor_type_conductivity = "invalid_type".to_string();

        let result = AtlasScientific::create_drivers(&config.atlas_scientific);

        assert!(result.is_err());
        if let Err(AtlasScientificError::InvalidSensorType {
            location: _,
            signal,
            sensor_type,
        }) = result
        {
            assert_eq!(signal, AquariumSignal::Conductivity);
            assert_eq!(sensor_type, "invalid_type");
        } else {
            panic!("Expected InvalidSensorType error, got {:?}", result);
        }
    }
}
