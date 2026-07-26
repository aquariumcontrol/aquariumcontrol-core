/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! The central sensor data aggregation and management thread.
//!
//! This module implements the `SensorManager`, a dedicated, long-running thread that
//! acts as the primary hub for all sensor data within the application. Its core
//! responsibility is to periodically gather the latest readings from various individual
//! sensor threads (`Ds18b20`, `Dht`, `AtlasScientific`) and consolidate them into a
//! single, consistent data structure.
//!
//! The manager is highly configurable, allowing the user to select the physical sensor
//! source for logical measurements (e.g., using "ds18b20" or "atlas_scientific" for
//! water temperature) via the TOML configuration file. The final, consolidated data
//! is then made available to the rest of the application through a shared `Arc<Mutex<...>>`.
//!
//! ## Key Components
//!
//! - **`SensorManager` Struct**: The core state machine for the thread. It holds the
//!   configuration, internal state, and a vast number of log-suppression flags.
//!
//! - **`execute()` Method**: The main thread loop. It operates in two distinct modes:
//!   - **Hardware Mode**: Reads the latest data from shared `Arc<Mutex<...>>` objects
//!     that are populated by other sensor threads. It uses the configuration to
//!     select the correct source for each logical signal.
//!   - **Simulator Mode**: Communicates via TCP channels with a simulator to fetch
//!     mock sensor data, allowing for development and testing without physical hardware.
//!
//! - **`SensorManagerMutexes` Struct**: A container that holds `Arc<Mutex<...>>` handles
//!   to all the *input* data sources from other sensor threads. This struct is a key
//!   part of the dependency injection pattern for the `execute` method.
//!
//! - **`SensorManagerSignals` Struct**: The *output* data structure. After consolidating
//!   all readings, the `SensorManager` writes them into this struct, which is wrapped
//!   in a shared `Arc<Mutex<...>>` for other threads (like the `DataLogger` or UI) to consume.
//!
//! ## Design and Architecture
//!
//! - **Central Aggregation Hub**: The `SensorManager` decouples sensor data *producers*
//!   (the individual hardware threads) from data *consumers*. Consumers only need to
//!   access one well-known mutex to get all the latest, validated data, without
//!   needing to know which physical sensors are being used.
//!
//! - **Configurable Data Sources**: The design allows for flexible hardware setups. By
//!   changing a string in the `config.toml` file (e.g., `source_water_temperature = "ds18b20"`),
//!   the user can switch the source for a key measurement without recompiling the application.
//!   The `new()` constructor validates this configuration at startup to prevent runtime errors.
//!
//! - **Concurrent Data Sharing**: The module makes extensive use of `Arc<Mutex<...>>` for
//!   thread-safe data exchange. It reads from a set of input mutexes and writes to a
//!   single output mutex, acting as a fan-in data pipeline.
//!
//! ### Thread Communication
//!
//! The `SensorManager` primarily communicates with the `SignalHandler` for graceful shutdown
//! and with a `TcpCommunication` thread when in simulator mode.

#![allow(non_snake_case)]

#[cfg(feature = "debug_sensor_manager")]
use log::debug;

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", not(test), not(feature = "test-utils")))] { // only required in real-life execution
        use nix::unistd::gettid;
        use log::info;
    }
}

use crate::sensors::atlas_scientific::AtlasScientificResultData;
use crate::sensors::dht::DhtResult;
use crate::sensors::ds18b20::Ds18b20Result;
use crate::sensors::ds18b20::Ds18b20ResultData;

pub use crate::sensors::sensor_manager_channels::SensorManagerChannels;
use crate::sensors::sensor_manager_config::SensorManagerConfig;
use crate::simulator::get_resp_sim::GetResponseFromSimulatorTrait;
use crate::simulator::tcp_communication_error::TcpCommunicationError;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
use crate::utilities::check_mutex_access_duration::CheckMutexAccessDurationTrait;
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;
use log::error;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;

/// Centralizes constants used throughout the `SensorManager` module.
///
/// This private module consolidates "magic strings" and other hardcoded values
/// to prevent them from being scattered throughout the `SensorManager`'s logic.
/// By keeping them in one place, it improves maintainability and reduces the
/// risk of typos when comparing against configuration values.
///
/// ### Constants
///
/// - **Sensor Source Identifiers**:
///   - `SOURCE_*`: These string constants define the valid values for the
///     `source_water_temperature` and `source_ambient_temperature` fields in the
///     `[sensor_manager]` section of the TOML configuration file. They map a
///     logical measurement to a physical sensor driver (e.g., "ds18b20", "dht").
///
/// - **Timing and Performance**:
///   - `*_MEASUREMENT_INTERVAL_MILLIS`: Define the polling rate for the sensor
///     manager, with different values for simulator and hardware modes.
///   - `MAX_MUTEX_ACCESS_DURATION_MILLIS`: Sets a threshold for how long a mutex
///     lock can be held before a warning is logged. This helps detect performance
///     bottlenecks or potential deadlocks.
mod sensor_manager_constants {
    pub const SOURCE_WATER_TEMPERATURE_ATLAS_SCIENTIFIC: &str = "atlas_scientific";

    pub const SOURCE_WATER_TEMPERATURE_DS18B20: &str = "ds18b20";

    pub const SOURCE_AMBIENT_TEMPERATURE_DHT: &str = "dht";

    pub const SOURCE_AMBIENT_TEMPERATURE_DS18B20: &str = "ds18b20";

    pub const SIMULATOR_MEASUREMENT_INTERVAL_MILLIS: u64 = 500;

    // time interval for updating measurement data
    pub const HW_MEASUREMENT_INTERVAL_MILLIS: u64 = 500;

    /// allow max. 10 milliseconds for mutex to be blocked by any other thread
    pub const MAX_MUTEX_ACCESS_DURATION_MILLIS: u64 = 10;
}

/// Contains `Arc<Mutex>`-wrapped shared data points accessed by the sensor manager.
///
/// These mutexes provide thread-safe access to various sensor readings and
/// allow the sensor manager to communicate the output to other threads.
#[derive(Clone)]
pub struct SensorManagerMutexes {
    /// Mutex for ambient temperature from Ds18b20 sensor
    pub mutex_ds18b20_ambient_temperature: Arc<Mutex<Ds18b20Result>>,

    /// Mutex for water temperature from Ds18b20 sensor
    pub mutex_ds18b20_water_temperature: Arc<Mutex<Ds18b20Result>>,

    /// Mutex for ambient temperature and humidity from DHT sensor
    pub mutex_dht: Arc<Mutex<DhtResult>>,

    /// Mutex for temperature from Atlas Scientific sensor unit
    pub mutex_atlas_scientific_temperature: Arc<Mutex<AtlasScientificResultData>>,

    /// Mutex for pH value from Atlas Scientific sensor unit
    pub mutex_atlas_scientific_ph: Arc<Mutex<AtlasScientificResultData>>,

    /// Mutex for conductivity from Atlas Scientific sensor unit
    pub mutex_atlas_scientific_conductivity: Arc<Mutex<AtlasScientificResultData>>,

    /// Mutex for output of sensor manager
    pub mutex_sensor_manager_signals: Arc<Mutex<SensorManagerSignals>>,
}

impl SensorManagerMutexes {
    /// Creates a default instance of this type.
    pub fn new_for_test() -> SensorManagerMutexes {
        SensorManagerMutexes {
            mutex_ds18b20_ambient_temperature: Arc::new(Mutex::new(Ok(
                Ds18b20ResultData::new_for_test(),
            ))),
            mutex_ds18b20_water_temperature: Arc::new(Mutex::new(Ok(
                Ds18b20ResultData::new_for_test(),
            ))),
            mutex_dht: Arc::new(Mutex::new(Ok((0.0, 0.0)))),
            mutex_atlas_scientific_temperature: Arc::new(Mutex::new(
                AtlasScientificResultData::new_for_test(),
            )),
            mutex_atlas_scientific_ph: Arc::new(Mutex::new(
                AtlasScientificResultData::new_for_test(),
            )),
            mutex_atlas_scientific_conductivity: Arc::new(Mutex::new(
                AtlasScientificResultData::new_for_test(),
            )),
            mutex_sensor_manager_signals: Arc::new(
                Mutex::new(SensorManagerSignals::new_for_test()),
            ),
        }
    }
}

/// Container for the signals managed by SensorManager.
/// Controls for heating and ventilation use these signals.
pub struct SensorManagerSignals {
    /// Water temperature in °C either coming from Atlas Scientific or Ds18b20
    pub water_temperature: f32,

    /// Ambient temperature in °C either coming from Dht or Ds18b20
    pub ambient_temperature: f32,

    /// Ambient humidity in % coming from Dht (if enabled)
    pub ambient_humidity: f32,

    /// pH
    pub ph: f32,

    /// conductivity in mS
    pub conductivity: f32,

    /// Recording of the instant where the mutex is being written
    pub instant_last_recording: Instant,
}

impl SensorManagerSignals {
    /// Creates a new instance.
    ///
    /// # Arguments
    /// * `config` - A parameter representing `config`.
    /// [impl->swreq~sensors_read_data~1]
    pub fn new(config: &SensorManagerConfig) -> SensorManagerSignals {
        SensorManagerSignals {
            water_temperature: config.replacement_value_water_temperature,
            ambient_temperature: config.replacement_value_ambient_temperature,
            ambient_humidity: config.replacement_value_ambient_humidity,
            ph: config.replacement_value_ph,
            conductivity: config.replacement_value_conductivity,
            instant_last_recording: Instant::now(),
        }
    }

    /// Creates a default instance of this type.
    pub fn new_for_test() -> SensorManagerSignals {
        SensorManagerSignals {
            water_temperature: 0.0,
            ambient_temperature: 0.0,
            ambient_humidity: 0.0,
            ph: 0.0,
            conductivity: 0.0,
            instant_last_recording: Instant::now(),
        }
    }
}

#[derive(Error, Debug)]
pub enum SensorManagerError {
    /// Invalid configuration of ambient temperature measurement.
    #[error("[{0}]: invalid configuration of ambient temperature. source_ambient_temperature may only have values ({1}, {2}) configured value is {3}")]
    InvalidAmbientTemperatureConfiguration(String, String, String, String),

    /// Invalid configuration of water temperature measurement.
    #[error("[{0}]: invalid configuration of water temperature. source_ambient_temperature may only have values ({1}, {2}) configured value is {3}")]
    InvalidWaterTemperatureConfiguration(String, String, String, String),

    /// Communication with the simulator failed.
    #[error("[{location}] Communication with simulator for {signal} failed")]
    SimulatorCommunicationError {
        location: String,
        signal: AquariumSignal,

        #[source]
        source: TcpCommunicationError,
    },
}

/// Contains the communication with DHT ambient temperature and humidity sensor.
/// Alternatively, TCP communication is used when configured to run with the simulator.
/// Sensor data is read periodically and stored.
/// Requests from data logger and heating control are answered with last measured data.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     signal_handler[SignalHandler] --> sensor_manager[SensorManager]
///     sensor_manager[SensorManager] --> signal_handler[SignalHandler]
///     signal_handler[SignalHandler] --> tcp_communication[TcpCommunication]
///     tcp_communication[TcpCommunication] --> signal_handler[SignalHandler]
/// ```
pub struct SensorManager {
    // configuration data for Ambient
    config: SensorManagerConfig,

    /// ambient temperature in °C
    ambient_temperature: f32,

    /// humidity in %
    ambient_humidity: f32,

    /// water temperature in °C
    water_temperature: f32,

    /// conductivity in mS
    conductivity: f32,

    /// pH value
    pH: f32,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    pub lock_error_channel_receive_termination: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via the channel from the signal handler thread
    pub lock_warn_inapplicable_command_signal_handler: bool,

    /// Recording when the last measurement took place
    pub instant_last_measurement: Instant,

    /// Duration in between two requests to either the simulator or the hardware
    pub measurement_interval: Duration,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to acquire lock on mutex for Dht readings
    lock_error_dht_mutex: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to acquire lock on mutex for temperature from Atlas Scientific sensor unit
    lock_error_atlas_scientific_temperature_mutex: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to acquire lock on mutex pH value from Atlas Scientific sensor unit
    lock_error_atlas_scientific_ph_mutex: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to acquire lock on mutex for conductivity from Atlas Scientific sensor unit
    lock_error_atlas_scientific_conductivity_mutex: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to acquire lock on mutex for water temperature from Ds18b20 sensor
    lock_error_ds18b20_water_temperature_mutex: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to acquire lock on mutex for ambient temperature from Ds18b20 sensor
    lock_error_ds18b20_ambient_temperature_mutex: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex
    pub lock_warn_max_mutex_access_duration: bool,

    // for testing purposes: record when mutex access time is exceeded without resetting it
    #[cfg(any(test, feature = "test-utils"))]
    pub mutex_access_duration_exceeded: bool,

    /// Maximum permissible duration for accessing the mutex
    pub max_mutex_access_duration: Duration,
}

impl ProcessExternalRequestTrait for SensorManager {}

impl GetResponseFromSimulatorTrait for SensorManager {}

impl SensorManager {
    /// Creates a new `SensorManager` control instance.
    ///
    /// This constructor initializes the ambient temperature and humidity measurement module.
    /// It sets the initial temperature and humidity values to configured replacement values
    /// and initializes all internal "lock" flags to `false`. These flags help prevent
    /// log flooding from recurring errors or warnings during operation.
    ///
    /// # Returns
    /// A `Result` containing a new `SensorManager` instance on success, or a
    /// `SensorManagerError` if the configuration is invalid.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - `source_ambient_temperature` in the config is not a valid, known source.
    /// - `source_water_temperature` in the config is not a valid, known source.
    pub fn new(config: SensorManagerConfig) -> Result<SensorManager, SensorManagerError> {
        // Check if the configuration of the ambient temperature source is valid.
        if config.source_ambient_temperature
            != sensor_manager_constants::SOURCE_AMBIENT_TEMPERATURE_DHT
            && config.source_ambient_temperature
                != sensor_manager_constants::SOURCE_AMBIENT_TEMPERATURE_DS18B20
        {
            return Err(SensorManagerError::InvalidAmbientTemperatureConfiguration(
                module_path!().to_string(),
                sensor_manager_constants::SOURCE_AMBIENT_TEMPERATURE_DHT.to_string(),
                sensor_manager_constants::SOURCE_AMBIENT_TEMPERATURE_DS18B20.to_string(),
                config.source_ambient_temperature,
            ));
        }

        // Check if the configuration of the water temperature source is valid.
        if config.source_water_temperature
            != sensor_manager_constants::SOURCE_WATER_TEMPERATURE_ATLAS_SCIENTIFIC
            && config.source_water_temperature
                != sensor_manager_constants::SOURCE_WATER_TEMPERATURE_DS18B20
        {
            return Err(SensorManagerError::InvalidWaterTemperatureConfiguration(
                module_path!().to_string(),
                sensor_manager_constants::SOURCE_WATER_TEMPERATURE_ATLAS_SCIENTIFIC.to_string(),
                sensor_manager_constants::SOURCE_WATER_TEMPERATURE_DS18B20.to_string(),
                config.source_water_temperature,
            ));
        }

        let replacement_value_ambient_temperature = config.replacement_value_ambient_temperature;
        let replacement_value_ambient_humidity = config.replacement_value_ambient_humidity;

        // define the measurement interval depending on if simulator or hardware shall be used
        let measurement_interval = if config.use_simulator {
            Duration::from_millis(sensor_manager_constants::SIMULATOR_MEASUREMENT_INTERVAL_MILLIS)
        } else {
            Duration::from_millis(sensor_manager_constants::HW_MEASUREMENT_INTERVAL_MILLIS)
        };

        Ok(SensorManager {
            config,
            ambient_temperature: replacement_value_ambient_temperature,
            ambient_humidity: replacement_value_ambient_humidity,
            water_temperature: 0.0,
            lock_error_channel_receive_termination: false,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_dht_mutex: false,
            conductivity: 0.0,
            pH: 0.0,
            instant_last_measurement: Instant::now(),
            measurement_interval,
            lock_error_atlas_scientific_temperature_mutex: false,
            lock_error_atlas_scientific_ph_mutex: false,
            lock_error_atlas_scientific_conductivity_mutex: false,
            lock_error_ds18b20_water_temperature_mutex: false,
            lock_error_ds18b20_ambient_temperature_mutex: false,
            lock_warn_max_mutex_access_duration: false,
            #[cfg(any(test, feature = "test-utils"))]
            mutex_access_duration_exceeded: false,
            max_mutex_access_duration: Duration::from_millis(
                sensor_manager_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
            ),
        })
    }

    /// Communicates with a simulator thread via channels to retrieve
    /// water temperature, ambient temperature, ambient humidity, pH and conductivity.
    ///
    /// This private helper function is used when the `SensorManager` module is configured
    /// to run in simulation mode. It sends `RequestSignal` commands to the simulator and
    /// updates the module's internal fields with the responses.
    ///
    /// # Arguments
    /// * `tx_sensor_manager_to_tcp_opt` - A reference to an `Option<Sender<InternalCommand>>` used
    ///   for sending commands to the `TcpCommunication` thread (which acts as the simulator interface).
    /// * `rx_sensor_manager_from_tcp_opt` - A reference to an `Option<Receiver<Result<f32, TcpCommunicationError>>>`
    ///   used for receiving simulated sensor data from the `TcpCommunication` thread.
    ///
    /// # Returns
    /// An empty `Result` on success, or a `SensorManagerError` if communication fails.
    ///
    /// # Errors
    /// This function will return a `SensorManagerError::SimulatorCommunicationError` if sending a
    /// request or receiving a response over the channels fails.
    ///
    /// # Panics
    /// This function will **panic** if either `tx_sensor_manager_to_tcp_opt` or `rx_sensor_manager_from_tcp_opt`
    /// are `None`. This case is not possible because the `SensorManager` module performs checks beforehand to ensure
    /// these channels are `Some` when running in simulator mode.
    /// * `sensor_manager_channels` - A mutable reference to the `SensorManagerChannels` communication interface.
    fn communicate_with_simulator(
        &mut self,
        sensor_manager_channels: &mut SensorManagerChannels,
    ) -> Result<(), SensorManagerError> {
        // unwrap is safe because tested beforehand
        let tx_sensor_manager_to_simulator = sensor_manager_channels
            .tx_sensor_manager_to_tcp_opt
            .as_mut()
            .unwrap();
        let rx_sensor_manager_from_simulator = sensor_manager_channels
            .rx_sensor_manager_from_tcp_opt
            .as_mut()
            .unwrap();

        self.ambient_temperature = Self::get_response_from_simulator_blocking(
            module_path!().to_string(),
            tx_sensor_manager_to_simulator,
            rx_sensor_manager_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::AmbientTemperature),
        )
        .map_err(|e| SensorManagerError::SimulatorCommunicationError {
            location: module_path!().to_string(),
            signal: AquariumSignal::AmbientTemperature,
            source: e,
        })?;
        self.ambient_humidity = Self::get_response_from_simulator_blocking(
            module_path!().to_string(),
            tx_sensor_manager_to_simulator,
            rx_sensor_manager_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::AmbientHumidity),
        )
        .map_err(|e| SensorManagerError::SimulatorCommunicationError {
            location: module_path!().to_string(),
            signal: AquariumSignal::AmbientHumidity,
            source: e,
        })?;
        self.water_temperature = Self::get_response_from_simulator_blocking(
            module_path!().to_string(),
            tx_sensor_manager_to_simulator,
            rx_sensor_manager_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::WaterTemperature),
        )
        .map_err(|e| SensorManagerError::SimulatorCommunicationError {
            location: module_path!().to_string(),
            signal: AquariumSignal::WaterTemperature,
            source: e,
        })?;
        self.conductivity = Self::get_response_from_simulator_blocking(
            module_path!().to_string(),
            tx_sensor_manager_to_simulator,
            rx_sensor_manager_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::Conductivity),
        )
        .map_err(|e| SensorManagerError::SimulatorCommunicationError {
            location: module_path!().to_string(),
            signal: AquariumSignal::Conductivity,
            source: e,
        })?;
        self.pH = Self::get_response_from_simulator_blocking(
            module_path!().to_string(),
            tx_sensor_manager_to_simulator,
            rx_sensor_manager_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::pH),
        )
        .map_err(|e| SensorManagerError::SimulatorCommunicationError {
            location: module_path!().to_string(),
            signal: AquariumSignal::pH,
            source: e,
        })?;

        Ok(())
    }

    /// Executes the main control loop for the sensor manager.
    ///
    /// This function runs continuously, managing the periodic consolidation of sensor data.
    /// It operates in two main modes:
    /// - **Simulator Mode**: Fetches simulated sensor data for all signals via a TCP communication channel.
    /// - **Hardware Mode**: Reads the latest sensor data from various shared mutexes that are
    ///   populated by other dedicated sensor threads (like `Ds18b20`, `Dht`, `AtlasScientific`).
    ///
    /// Based on the configuration, it selects the appropriate data source (e.g., Ds18b20 vs.
    /// Atlas Scientific for water temperature). The consolidated data is then written to the
    /// `mutex_sensor_manager_signals` mutex, making it available to other application threads.
    /// The loop maintains a measurement interval and remains responsive to `Quit` and `Terminate`
    /// commands from the signal handler for graceful shutdown.
    ///
    /// # Arguments
    /// * `sensor_manager_channels` - A struct containing all `mpsc` channels necessary for inter-thread
    ///   communication with the `TcpCommunication` (if in simulator mode) and the `SignalHandler`.
    /// * `mutexes` - A struct containing `Arc<Mutex<...>>` handles to all required sensor data
    ///   sources and the final output signals struct.
    ///
    /// # Panics
    /// This function will panic if:
    /// - It's configured to use the simulator but essential TCP communication channels are missing.
    pub async fn execute(
        &mut self,
        sensor_manager_channels: &mut SensorManagerChannels,
        mutexes: SensorManagerMutexes,
    ) {
        #[cfg(all(target_os = "linux", not(test), not(feature = "test-utils")))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        if (self.config.use_simulator)
            && (sensor_manager_channels
                .tx_sensor_manager_to_tcp_opt
                .is_none()
                | sensor_manager_channels
                    .rx_sensor_manager_from_tcp_opt
                    .is_none())
        {
            panic!(
                "{}: configured to run with simulator, but no channel to TCP thread provided.",
                module_path!()
            );
        }

        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let sleep_duration_one_millis = Duration::from_millis(1);
        let mut mutex_poisoned = false;

        // initialize signals with replacement values
        self.ambient_temperature = self.config.replacement_value_ambient_temperature;
        self.ambient_humidity = self.config.replacement_value_ambient_humidity;

        loop {
            let (
                quit_command_received, // the request to end the application has been received
                _,
                _,
            ) = self.process_external_request(
                &mut sensor_manager_channels.rx_sensor_manager_from_signal_handler,
                None,
            );

            if quit_command_received {
                // quit command has been received
                break;
            }

            let current_instant = Instant::now();
            if current_instant.duration_since(self.instant_last_measurement)
                > self.measurement_interval
            {
                self.instant_last_measurement = current_instant;
                if self.config.use_simulator {
                    let result = tokio::task::block_in_place(|| {
                        self.communicate_with_simulator(sensor_manager_channels)
                    });
                    if result.is_err() {
                        log_error_chain(
                            module_path!(),
                            "error when communicating with simulator",
                            result.err().unwrap(),
                        );
                    }
                } else {
                    //*** Ds18b20 ******************************************************************
                    if self.config.source_water_temperature
                        == sensor_manager_constants::SOURCE_WATER_TEMPERATURE_DS18B20
                    {
                        match mutexes.mutex_ds18b20_water_temperature.lock() {
                            Ok(ds18b20_water_temperature_result) => {
                                match &*ds18b20_water_temperature_result {
                                    Ok(ds18b20_water_temperature_result_data) => {
                                        self.water_temperature =
                                            ds18b20_water_temperature_result_data.value;
                                    }
                                    Err(_) => {
                                        // keep the values from previous measurement
                                    }
                                }
                            }
                            Err(e) => {
                                if !self.lock_error_ds18b20_water_temperature_mutex {
                                    self.lock_error_ds18b20_water_temperature_mutex = true;
                                    error!(target: module_path!(), "error when trying to lock Ds18b20 water temperature mutex: {e}");
                                }
                            }
                        }
                    }
                    if self.config.source_ambient_temperature
                        == sensor_manager_constants::SOURCE_WATER_TEMPERATURE_DS18B20
                    {
                        match mutexes.mutex_ds18b20_ambient_temperature.lock() {
                            Ok(ds18b20_ambient_temperature_result) => {
                                match &*ds18b20_ambient_temperature_result {
                                    Ok(ds18b20_ambient_temperature_result_data) => {
                                        self.ambient_temperature =
                                            ds18b20_ambient_temperature_result_data.value;
                                    }
                                    Err(_) => {
                                        // keep the values from previous measurement
                                    }
                                }
                            }
                            Err(e) => {
                                if !self.lock_error_ds18b20_ambient_temperature_mutex {
                                    self.lock_error_ds18b20_ambient_temperature_mutex = true;
                                    error!(target: module_path!(), "error when trying to lock Ds18b20 ambient temperature mutex: {e}");
                                }
                            }
                        }
                    }
                    //*** Dht **********************************************************************
                    {
                        match mutexes.mutex_dht.lock() {
                            Ok(dht_result) => {
                                match *dht_result {
                                    Ok(tuple) => {
                                        if self.config.source_ambient_temperature ==
                                            sensor_manager_constants::SOURCE_AMBIENT_TEMPERATURE_DHT {
                                            self.ambient_temperature = tuple.0;
                                        }
                                        self.ambient_humidity = tuple.1;
                                    }
                                    Err(_) => {
                                        // keep the values from previous measurement
                                    }
                                }
                            }
                            Err(e) => {
                                if !self.lock_error_dht_mutex {
                                    self.lock_error_dht_mutex = true;
                                    error!(target: module_path!(), "error when trying to lock Dht mutex: {e}");
                                }
                            }
                        }
                    }

                    //*** Atlas Scientific **********************************************************
                    {
                        match mutexes.mutex_atlas_scientific_temperature.lock() {
                            Ok(atlas_scientific_temperature) => {
                                if self.config.source_water_temperature == sensor_manager_constants::SOURCE_WATER_TEMPERATURE_ATLAS_SCIENTIFIC {
                                    self.water_temperature = atlas_scientific_temperature.value;
                                }
                            }
                            Err(e) => {
                                if !self.lock_error_atlas_scientific_temperature_mutex {
                                    self.lock_error_atlas_scientific_temperature_mutex = true;
                                    error!(target: module_path!(), "error when trying to lock mutex: {e}");
                                }
                            }
                        }
                    }

                    {
                        match mutexes.mutex_atlas_scientific_ph.lock() {
                            Ok(atlas_scientific_ph) => {
                                self.pH = atlas_scientific_ph.value;
                            }
                            Err(e) => {
                                if !self.lock_error_atlas_scientific_ph_mutex {
                                    self.lock_error_atlas_scientific_ph_mutex = true;
                                    error!(target: module_path!(), "error when trying to lock mutex: {e}");
                                }
                            }
                        }
                    }

                    {
                        match mutexes.mutex_atlas_scientific_conductivity.lock() {
                            Ok(atlas_scientific_conductivity) => {
                                self.conductivity = atlas_scientific_conductivity.value;
                            }
                            Err(e) => {
                                if !self.lock_error_atlas_scientific_conductivity_mutex {
                                    self.lock_error_atlas_scientific_conductivity_mutex = true;
                                    error!(target: module_path!(), "error when trying to lock mutex: {e}");
                                }
                            }
                        }
                    }
                }

                if !mutex_poisoned {
                    let instant_before_locking_mutex = Instant::now();
                    let mut instant_after_locking_mutex = Instant::now(); // initialization is overwritten

                    // write the measurement result to Mutex
                    {
                        match mutexes.mutex_sensor_manager_signals.lock() {
                            Ok(mut sensor_manager_signals) => {
                                instant_after_locking_mutex = Instant::now();
                                sensor_manager_signals.ambient_humidity = self.ambient_humidity;
                                sensor_manager_signals.ambient_temperature =
                                    self.ambient_temperature;
                                sensor_manager_signals.water_temperature = self.water_temperature;
                                sensor_manager_signals.conductivity = self.conductivity;
                                sensor_manager_signals.ph = self.pH;
                                sensor_manager_signals.instant_last_recording = Instant::now();
                            }
                            Err(e) => {
                                log_error_chain(module_path!(), "error when trying to lock mutex for writing sensor manager data", e);
                                mutex_poisoned = true;
                            }
                        }
                    }

                    // check if access to mutex took too long
                    self.check_mutex_access_duration(
                        None,
                        instant_after_locking_mutex,
                        instant_before_locking_mutex,
                    );
                }
            }
            tokio::time::sleep(sleep_duration_hundred_millis).await;
        }

        sensor_manager_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads.
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        self.wait_for_termination_async(
            &mut sensor_manager_channels.rx_sensor_manager_from_signal_handler,
            sleep_duration_one_millis,
            module_path!(),
        )
        .await;
    }
}
