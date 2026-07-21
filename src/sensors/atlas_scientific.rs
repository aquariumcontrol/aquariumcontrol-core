/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

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
//!   configuration, the collection of sensor drivers (implementing `AtlasScientificDriver`),
//!   and the current state of the measurement cycle.
//!
//! - **`AtlasScientificDriver` Trait**: An abstraction layer (defined in `atlas_scientific_driver.rs`)
//!   that allows the `AtlasScientific` thread to interact with different sensor types (e.g.,
//!   EZO vs. OEM) uniformly. Specific implementations handle the protocol details.
//!
//! - **`execute()` Method**: The main thread loop. It implements a sequential measurement
//!   state machine:
//!   1.  **Wait**: Waits for the configured `measurement_interval` to elapse.
//!   2.  **Send**: Delegates to the appropriate driver to send a request for the *next*
//!       sensor in the sequence (Temperature -> pH -> Conductivity) to the `I2cInterface` thread.
//!   3.  **Receive**: Polls the response channel from the `I2cInterface`.
//!   4.  **Process**: Delegates to the driver to parse and validate the response. If successful,
//!       the result is written to the appropriate shared `Mutex`.
//!   5.  **Cycle**: The state machine advances to the next sensor and returns to the **Wait** step.
//!
//! ## Design and Architecture
//!
//! This module is designed for robust, decoupled, and sequential sensor reading.
//!
//! - **Driver Abstraction**: The module uses the Strategy pattern via the `AtlasScientificDriver`
//!   trait. This allows the system to support different hardware revisions or types (like
//!   Atlas Scientific EZO circuits vs. OEM modules) without changing the core scheduling logic.
//!
//! - **Sequential Measurement**: The module does not attempt to read all sensors at once.
//!   This serialized approach simplifies the logic and avoids potential conflicts on the
//!   I2C bus.
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

#![allow(non_snake_case)]

#[cfg(all(not(feature = "test-utils"), not(test), target_os = "linux"))] // only required in real-life execution
use log::info;
use std::collections::HashMap;
use std::fmt;

use crate::sensors::atlas_scientific_channels::AtlasScientificChannels;
use crate::sensors::atlas_scientific_config::AtlasScientificConfig;
use crate::sensors::atlas_scientific_driver::AtlasScientificDriver;
use crate::sensors::atlas_scientific_error::AtlasScientificError;
use crate::sensors::atlas_scientific_ezo::AtlasScientificEzo;
use crate::sensors::atlas_scientific_monitor_view::AtlasScientificMonitorView;
use crate::sensors::atlas_scientific_oem::atlas_scientific_oem_constants;
use crate::sensors::atlas_scientific_oem::AtlasScientificOem;
use crate::simulator::get_resp_sim::GetResponseFromSimulatorTrait;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::channel_content::AquariumSignal;
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;
#[cfg(all(not(feature = "test-utils"), not(test), target_os = "linux"))] // only required in real-life execution
use nix::unistd::gettid;
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
    /// Creates a new instance.
    ///
    /// # Arguments
    /// * `value` - The value to set.
    pub fn new(value: f32) -> Self {
        AtlasScientificResultData {
            value,
            invalid: false,
            measurement_instant: Instant::now(),
        }
    }

    /// Creates a default instance of this type.
    pub fn new_for_test() -> Self {
        AtlasScientificResultData {
            value: 0.0,
            invalid: false,
            measurement_instant: Instant::now(),
        }
    }
}

impl fmt::Display for AtlasScientificResultData {
    /// Formats the value using the given formatter.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Write the floating-point sensor value directly to the formatter.
        write!(f, "{}", self.value)
    }
}

#[allow(unused)] // used in conditionally compiled code
pub type AtlasScientificResult = Result<AtlasScientificResultData, AtlasScientificError>;

/// Device-type-independent constants for AtlasScientific
pub mod atlas_scientific_constants {
    /// allow max. 10 milliseconds for mutex to be blocked by any other thread
    pub const MAX_MUTEX_ACCESS_DURATION_MILLIS: u64 = 10;

    pub const SENSOR_TYPE_OEM: &str = "oem";

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
    pub drivers: HashMap<AquariumSignal, Box<dyn AtlasScientificDriver>>,

    lock_error_driver_not_found: HashMap<AquariumSignal, bool>,

    /// Last sent monitor view for deduplication
    last_sent_monitor_view: Option<AtlasScientificMonitorView>,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send to monitors
    lock_error_channel_send_monitors: bool,
}

impl ProcessExternalRequestTrait for AtlasScientific {}

impl GetResponseFromSimulatorTrait for AtlasScientific {}

impl AtlasScientific {
    /// Performs the `create_drivers` operation.
    ///
    /// # Arguments
    /// * `config` - A parameter representing `config`.
    pub fn create_drivers(
        config: &AtlasScientificConfig,
    ) -> Result<HashMap<AquariumSignal, Box<dyn AtlasScientificDriver>>, AtlasScientificError> {
        let mut drivers: HashMap<AquariumSignal, Box<dyn AtlasScientificDriver>> = HashMap::new();

        let driver_temperature: Box<dyn AtlasScientificDriver> =
            match config.sensor_type_temperature.as_str() {
                atlas_scientific_constants::SENSOR_TYPE_OEM => {
                    println!("Using Atlas Scientific OEM driver for temperature");
                    Box::new(AtlasScientificOem::new(
                        config.address_atlas_scientific_temperature,
                        atlas_scientific_oem_constants::DEVICE_TYPE_TEMPERATURE,
                        config.sleep_time_millis_temperature,
                        atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
                        atlas_scientific_oem_constants::REG_DATA_MSB_TEMPERATURE,
                        atlas_scientific_oem_constants::SCALE_FACTOR_TEMPERATURE,
                        atlas_scientific_oem_constants::RESPONSE_LENGTH_TEMPERATURE,
                    ))
                }
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
            atlas_scientific_constants::SENSOR_TYPE_OEM => {
                println!("Using Atlas Scientific OEM driver for pH");
                Box::new(AtlasScientificOem::new(
                    config.address_atlas_scientific_ph,
                    atlas_scientific_oem_constants::DEVICE_TYPE_PH,
                    config.sleep_time_millis_ph,
                    atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
                    atlas_scientific_oem_constants::REG_DATA_MSB_PH,
                    atlas_scientific_oem_constants::SCALE_FACTOR_PH,
                    atlas_scientific_oem_constants::RESPONSE_LENGTH_PH,
                ))
            }
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
                atlas_scientific_constants::SENSOR_TYPE_OEM => {
                    println!("Using Atlas Scientific OEM driver for conductivity");
                    Box::new(AtlasScientificOem::new(
                        config.address_atlas_scientific_conductivity,
                        atlas_scientific_oem_constants::DEVICE_TYPE_CONDUCTIVITY,
                        config.sleep_time_millis_conductivity,
                        atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
                        atlas_scientific_oem_constants::REG_DATA_MSB_CONDUCTIVITY,
                        atlas_scientific_oem_constants::SCALE_FACTOR_CONDUCTIVITY,
                        atlas_scientific_oem_constants::RESPONSE_LENGTH_CONDUCTIVITY,
                    ))
                }
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
            last_sent_monitor_view: None,
            lock_error_channel_send_monitors: false,
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
            Ok(mut guard) => {
                *guard = result;
                #[cfg(test)]
                println!("write_result_to_mutex: Could lock mutex for {}", signal);
            }
            Err(_) => {
                #[cfg(test)]
                println!("write_result_to_mutex: Could not lock mutex for {}", signal);
                return Err(AtlasScientificError::CouldNotLockMutex {
                    location: module_path!().to_string(),
                    signal: signal.clone(),
                });
            }
        }
        Ok(Instant::now())
    }

    /// Helper function for retrieving the driver from the hash map
    ///
    /// # Arguments
    /// * `signal` - A parameter representing `signal`.
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

    /// Checks sensor values against configured thresholds and sends monitor view if needed.
    ///
    /// This method reads the current values from all three sensor mutexes, compares them
    /// against the configured min/max thresholds, and creates an `AtlasScientificMonitorView`
    /// indicating which sensors are out of range. The view is only sent if:
    /// - At least one sensor has valid data
    /// - The view has changed since the last transmission (deduplication)
    ///
    /// Sensors with invalid data are skipped and their flags are set to false (no issue).
    ///
    /// # Arguments
    /// * `atlas_scientific_channels` - Channels for communicating with other threads
    /// * `mutex_water_temperature` - Shared mutex containing temperature sensor data
    /// * `mutex_ph` - Shared mutex containing pH sensor data
    /// * `mutex_conductivity` - Shared mutex containing conductivity sensor data
    fn check_and_send_monitor_view(
        &mut self,
        atlas_scientific_channels: &mut AtlasScientificChannels,
        mutex_water_temperature: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_ph: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_conductivity: &Arc<Mutex<AtlasScientificResultData>>,
    ) {
        // Read current values from mutexes
        let temp_result = mutex_water_temperature.lock();
        let ph_result = mutex_ph.lock();
        let cond_result = mutex_conductivity.lock();

        // Check if all locks succeeded
        if temp_result.is_err() || ph_result.is_err() || cond_result.is_err() {
            return; // Skip monitoring if we can't read values
        }

        let temp = temp_result.unwrap();
        let ph = ph_result.unwrap();
        let cond = cond_result.unwrap();

        // Check each sensor individually - only check thresholds for valid sensors
        let temp_out_of_range = if temp.invalid {
            false // Skip invalid sensors - report no issue
        } else {
            temp.value < self.config.min_val_atlas_scientific_temperature
                || temp.value > self.config.max_val_atlas_scientific_temperature
        };

        let ph_out_of_range = if ph.invalid {
            false // Skip invalid sensors - report no issue
        } else {
            ph.value < self.config.min_val_atlas_scientific_ph
                || ph.value > self.config.max_val_atlas_scientific_ph
        };

        let cond_out_of_range = if cond.invalid {
            false // Skip invalid sensors - report no issue
        } else {
            cond.value < self.config.min_val_atlas_scientific_conductivity
                || cond.value > self.config.max_val_atlas_scientific_conductivity
        };

        // Skip sending if all sensors are invalid
        if temp.invalid && ph.invalid && cond.invalid {
            return;
        }

        let monitor_view = AtlasScientificMonitorView {
            temperature_out_of_range: temp_out_of_range,
            ph_out_of_range,
            conductivity_out_of_range: cond_out_of_range,
        };

        // Deduplicate: only send if different from last sent
        let mut should_send = true;
        if let Some(ref last_view) = self.last_sent_monitor_view {
            if &monitor_view == last_view {
                should_send = false;
            }
        }

        if should_send {
            match atlas_scientific_channels.send_to_monitor(monitor_view.clone()) {
                Ok(_) => {
                    self.lock_error_channel_send_monitors = false;
                    self.last_sent_monitor_view = Some(monitor_view);
                }
                Err(e) => {
                    if !self.lock_error_channel_send_monitors {
                        log_error_chain(module_path!(), "sending view to monitor failed", e);
                        self.lock_error_channel_send_monitors = true;
                    }
                }
            }
        }
    }

    /// Iterates over all registered drivers and verifies their device type.
    /// This requires the I2cInterface thread to be running.
    ///
    /// # Arguments
    /// * `channels` - A parameter representing `channels`.
    pub fn init_devices(
        &self,
        channels: &mut AtlasScientificChannels,
    ) -> Result<(), AtlasScientificError> {
        for (signal, driver) in &self.drivers {
            // We verify the driver.
            // Note: check_device_type is defined in the Trait and takes &self
            driver.device_init(channels, signal)?;
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
    pub async fn execute(
        &mut self,
        atlas_scientific_channels: &mut AtlasScientificChannels,
        mutex_atlas_scientific_water_temperature: Arc<Mutex<AtlasScientificResultData>>,
        mutex_atlas_scientific_ph: Arc<Mutex<AtlasScientificResultData>>,
        mutex_atlas_scientific_conductivity: Arc<Mutex<AtlasScientificResultData>>,
    ) {
        #[cfg(all(target_os = "linux", not(test), not(feature = "test-utils")))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let sleep_duration_ten_millis = Duration::from_millis(10);
        let sleep_duration_one_millis = Duration::from_millis(1);

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
            if self.config.active && !channel_is_disconnected && !self.drivers.is_empty() {
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
                            let receive_result = tokio::task::block_in_place(|| {
                                driver.receive_response_from_i2c_interface(
                                    &signal,
                                    atlas_scientific_channels,
                                    &mutex_atlas_scientific_water_temperature,
                                    &mutex_atlas_scientific_ph,
                                    &mutex_atlas_scientific_conductivity,
                                )
                            });
                            match receive_result {
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

                                let (next_signal, measurement_set_complete) =
                                    signal.get_next_atlas_scientific_signal();
                                signal = next_signal;

                                if measurement_set_complete {
                                    self.last_measurement_instant = Instant::now();
                                    // Check thresholds and send monitoring view
                                    self.check_and_send_monitor_view(
                                        atlas_scientific_channels,
                                        &mutex_atlas_scientific_water_temperature,
                                        &mutex_atlas_scientific_ph,
                                        &mutex_atlas_scientific_conductivity,
                                    );
                                }
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

            tokio::time::sleep(sleep_duration_ten_millis).await;
        }

        atlas_scientific_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads.
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination_async(
            &mut atlas_scientific_channels.rx_atlas_scientific_from_signal_handler,
            sleep_duration_one_millis,
            module_path!(),
        )
        .await;
    }
}

#[cfg(test)]
pub mod tests {
    use crate::sensors::atlas_scientific::{
        AtlasScientific, AtlasScientificError, AtlasScientificResultData,
    };
    use crate::utilities::channel_content::AquariumSignal;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Instant;

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
    // Verifies that get next atlas scientific signal.
    fn test_get_next_atlas_scientific_signal() {
        // Test WaterTemperature branch
        let (next, complete) = AquariumSignal::WaterTemperature.get_next_atlas_scientific_signal();
        assert_eq!(next, AquariumSignal::pH);
        assert!(!complete);

        // Test pH branch
        let (next, complete) = AquariumSignal::pH.get_next_atlas_scientific_signal();
        assert_eq!(next, AquariumSignal::Conductivity);
        assert!(!complete);

        // Test Conductivity branch
        let (next, complete) = AquariumSignal::Conductivity.get_next_atlas_scientific_signal();
        assert_eq!(next, AquariumSignal::WaterTemperature);
        assert!(complete);

        // Test default branch (_) using AmbientTemperature
        let (next, complete) =
            AquariumSignal::AmbientTemperature.get_next_atlas_scientific_signal();
        assert_eq!(next, AquariumSignal::WaterTemperature);
        assert!(!complete);
    }
}
