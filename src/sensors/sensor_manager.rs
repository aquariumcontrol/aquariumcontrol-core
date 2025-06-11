/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#![allow(non_snake_case)]

use log::{error, warn};
use std::sync::{Arc, Mutex};

#[cfg(feature = "debug_sensor_manager")]
use log::debug;

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};

use crate::sensors::atlas_scientific::AtlasScientificResult;
use crate::sensors::dht::DhtResult;
use crate::simulator::get_resp_sim::GetResponseFromSimulatorTrait;
use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use std::time::{Duration, Instant};
use crate::sensors::ds18b20::Ds18b20Result;

const LOG_TARGET_SENSOR_MANAGER: &str = "ambient";

#[derive(Deserialize, Debug)]
/// Holds the configuration data for the measurement of ambient signals from DHT sensor
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
pub struct SensorManagerConfig {
    /// flag to determine if the application shall communicate with hardware directly or via TCP with a simulator
    pub use_simulator: bool,

    /// replacement value for ambient temperature in case reading from DHT22 sensor fails
    pub replacement_value_ambient_temperature: f32,

    /// replacement value for ambient humidity in case reading from DHT22 sensor fails
    pub replacement_value_ambient_humidity: f32,

    /// replacement value for water temperature in case reading from I2C fails
    pub replacement_value_water_temperature: f32,

    /// replacement value for pH in case reading from I2C fails
    pub replacement_value_ph: f32,

    /// replacement value for conductivity in case reading from I2C fails
    pub replacement_value_conductivity: f32,

    /// definition of the source for the water temperature - valid values are "ds18b20" or "atlas_scientific".
    pub source_water_temperature: String,

    /// definition of the source for the ambient temperature - valid values are "ds18b20" or "dht".
    pub source_ambient_temperature: String,
}

mod sensor_manager_constants {
    pub const SOURCE_WATER_TEMPERATURE_ATLAS_SCIENTIFIC: &str = "atlas_scientific";

    pub const SOURCE_WATER_TEMPERATURE_DS18B20: &str = "ds18b20";

    pub const SOURCE_AMBIENT_TEMPERATURE_DHT: &str = "dht";

    pub const SOURCE_AMBIENT_TEMPERATURE_DS18B20: &str = "ds18b20";

    pub const SIMULATOR_MEASUREMENT_INTERVAL_MILLIS: u64 = 500;

    // time interval for updating measurement data
    pub const HW_MEASUREMENT_INTERVAL_MILLIS: u64 = 30000;
}

pub struct SensorManagerChannels<'a> {
    /// Sender part of the channel for communication via TCP wrapped as optional
    pub tx_sensor_manager_to_tcp_opt: &'a Option<Sender<InternalCommand>>,

    /// Receiver part of the channel for communication via TCP wrapped as optional
    pub rx_sensor_manager_from_tcp_opt: &'a Option<Receiver<f32>>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_sensor_manager_to_signal_handler: Sender<bool>,

    /// Receiver part of the channel for communication from the signal handler                
    pub rx_sensor_manager_from_signal_handler: Receiver<InternalCommand>,
}

/// Contains `Arc<Mutex>`-wrapped shared data points accessed by the sensor manager.
///
/// These mutexes provide thread-safe access to various sensor readings and
/// allow the sensor manager to communicate the output to other threads.
pub struct SensorManagerMutexes {
    /// Mutex for ambient temperature from Ds18b20 sensor
    pub mutex_ds18b20_ambient_temperature: Arc<Mutex<Ds18b20Result>>,
    
    /// Mutex for water temperature from Ds18b20 sensor
    pub mutex_ds18b20_water_temperature: Arc<Mutex<Ds18b20Result>>,
    
    /// Mutex for ambient temperature and humidity fron DHT sensor
    pub mutex_dht: Arc<Mutex<DhtResult>>,
    
    /// Mutex for temperature from Atlas Scientific sensor unit
    pub mutex_atlas_scientific_temperature: Arc<Mutex<AtlasScientificResult>>,

    /// Mutex for pH value from Atlas Scientific sensor unit
    pub mutex_atlas_scientific_ph: Arc<Mutex<AtlasScientificResult>>,
    
    /// Mutex for conductivity from Atlas Scientific sensor unit
    pub mutex_atlas_scientific_conductivity: Arc<Mutex<AtlasScientificResult>>,
    
    /// Mutex for output of sensor manager
    pub mutex_sensor_manager_signals: Arc<Mutex<SensorManagerSignals>>,
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
}

/// Contains the communication with DHT ambient temperature and humidity sensor.
/// Alternatively, TCP communication is used when configured to run with the simulator.
/// Sensor data is read periodically and stored.
/// Requests from data logger and heating control are answered with last measured data.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     ambient[Ambient] -.-> data_logger[DataLogger]
///     ambient --> tcp_communication[TcpCommunication]
///     tcp_communication --> ambient
///     ambient -.-> heating[Heating]
///     signal_handler[SignalHandler] --> ambient
///     ambient --> signal_handler
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
    lock_error_channel_receive_termination: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via the channel from the signal handler thread
    lock_warn_inapplicable_command_signal_handler: bool,
    
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
    /// # Arguments
    /// * `config` - Configuration data for the SensorManager module, specifying the data source for
    ///   certain sensor signals, and replacement values for sensor readings.
    ///
    /// # Returns
    /// A new `Ambient` struct, ready to collect and provide ambient sensor data.
    pub fn new(config: SensorManagerConfig) -> SensorManager {
        // Check if the configuration of the ambient temperature source is valid.
        if config.source_ambient_temperature
            != sensor_manager_constants::SOURCE_AMBIENT_TEMPERATURE_DHT
            && config.source_ambient_temperature
                != sensor_manager_constants::SOURCE_AMBIENT_TEMPERATURE_DS18B20
        {
            panic!(
                "{}: invalid configuration of sensor manager for ambient temperature.\n \
                source_ambient_temperature may only have values ({}, {})\n \
                configured value is {}",
                LOG_TARGET_SENSOR_MANAGER,
                sensor_manager_constants::SOURCE_AMBIENT_TEMPERATURE_DHT,
                sensor_manager_constants::SOURCE_AMBIENT_TEMPERATURE_DS18B20,
                config.source_ambient_temperature
            );
        }

        // Check if the configuration of the water temperature source is valid.
        if config.source_water_temperature
            != sensor_manager_constants::SOURCE_WATER_TEMPERATURE_ATLAS_SCIENTIFIC
            && config.source_water_temperature
                != sensor_manager_constants::SOURCE_WATER_TEMPERATURE_DS18B20
        {
            panic!(
                "{}: invalid configuration of sensor manager for water temperature.\n \
                source_water_temperature may only have values ({}, {})\n \
                configured value is {}",
                LOG_TARGET_SENSOR_MANAGER,
                sensor_manager_constants::SOURCE_WATER_TEMPERATURE_ATLAS_SCIENTIFIC,
                sensor_manager_constants::SOURCE_WATER_TEMPERATURE_DS18B20,
                config.source_water_temperature
            );
        }

        let replacement_value_ambient_temperature = config.replacement_value_ambient_temperature;
        let replacement_value_ambient_humidity = config.replacement_value_ambient_humidity;

        // define the measurement interval depending on if simulator or hardware shall be used
        let measurement_interval = if config.use_simulator {
            Duration::from_millis(sensor_manager_constants::SIMULATOR_MEASUREMENT_INTERVAL_MILLIS)
        } else {
            Duration::from_millis(sensor_manager_constants::HW_MEASUREMENT_INTERVAL_MILLIS)
        };

        SensorManager {
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
        }
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
    /// * `rx_sensor_manager_from_tcp_opt` - A reference to an `Option<Receiver<f32>>` used for
    ///   receiving simulated sensor data from the `TcpCommunication` thread.
    ///
    /// # Panics
    /// This function will **panic** if either `tx_sensor_manager_to_tcp_opt` or `rx_sensor_manager_from_tcp_opt`
    /// are `None`. This is because the `SensorManager` module performs checks beforehand to ensure
    /// these channels are `Some` when running in simulator mode.
    fn communicate_with_simulator(
        &mut self,
        tx_sensor_manager_to_tcp_opt: &Option<Sender<InternalCommand>>,
        rx_sensor_manager_from_tcp_opt: &Option<Receiver<f32>>,
    ) {
        // unwrap is safe because tested beforehand
        let tx_sensor_manager_to_simulator = tx_sensor_manager_to_tcp_opt.as_ref().unwrap();
        let rx_sensor_manager_from_simulator = rx_sensor_manager_from_tcp_opt.as_ref().unwrap();

        self.ambient_temperature = Self::get_response_from_simulator(
            tx_sensor_manager_to_simulator,
            rx_sensor_manager_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::AmbientTemperature),
        );
        self.ambient_humidity = Self::get_response_from_simulator(
            tx_sensor_manager_to_simulator,
            rx_sensor_manager_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::AmbientHumidity),
        );
        self.water_temperature = Self::get_response_from_simulator(
            tx_sensor_manager_to_simulator,
            rx_sensor_manager_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::WaterTemperature),
        );
        self.conductivity = Self::get_response_from_simulator(
            tx_sensor_manager_to_simulator,
            rx_sensor_manager_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::Conductivity),
        );
        self.pH = Self::get_response_from_simulator(
            tx_sensor_manager_to_simulator,
            rx_sensor_manager_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::pH),
        );
    }

    /// Executes the main control loop for the sensor manager.
    ///
    /// This function runs continuously, managing the periodic measurement and routing of measurement data.
    /// It operates in two main modes:
    /// - **Simulator Mode**: Fetches simulated sensor data via a TCP communication channel.
    /// - **Hardware Mode (Linux with `target_hw` feature)**: Interacts directly with a DHT sensor
    ///   thread, sending requests and receiving actual sensor readings.
    ///
    /// The collected data is then written to a shared mutex, making it available to other
    /// application threads. The loop maintains a measurement interval and remains
    /// responsive to `Quit` and `Terminate` commands from the signal handler for graceful shutdown.
    ///
    /// # Arguments
    /// * `sensor_manager_channels` - A struct containing all `mpsc` channels necessary for inter-thread
    ///   communication with the `TcpCommunication` (if in simulator mode) and the `SignalHandler`.
    /// * `mutex_dht` - An `Arc<Mutex<DhtResult>>` into which the latest recording
    ///   from Dht sensor is written. Ambient only reads it.
    /// * `mutex_sensor_manager_signals` - An `Arc<Mutex<SensorManagerSignals>>` into which the
    ///   measurement data is written, ensuring thread-safe access for other modules.
    ///
    /// # Panics
    /// This function will panic if:
    /// - It's configured to use the simulator but essential TCP communication channels are missing.
    pub fn execute(
        &mut self,
        sensor_manager_channels: SensorManagerChannels,
        mutexes: SensorManagerMutexes,
    ) {
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
                LOG_TARGET_SENSOR_MANAGER
            );
        }

        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let sleep_duration_one_millis = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        // initialize signals with replacement values
        self.ambient_temperature = self.config.replacement_value_ambient_temperature;
        self.ambient_humidity = self.config.replacement_value_ambient_humidity;

        loop {
            let (
                quit_command_received, // the request to end the application has been received
                _,
                _,
            ) = self.process_external_request(
                &sensor_manager_channels.rx_sensor_manager_from_signal_handler,
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
                if self.config.use_simulator {
                    self.communicate_with_simulator(
                        sensor_manager_channels.tx_sensor_manager_to_tcp_opt,
                        sensor_manager_channels.rx_sensor_manager_from_tcp_opt,
                    );
                } else {
                    //*** Ds18b20 ******************************************************************
                    if self.config.source_water_temperature == sensor_manager_constants::SOURCE_WATER_TEMPERATURE_DS18B20 {
                        match mutexes.mutex_ds18b20_water_temperature.lock() {
                            Ok(ds18b20_water_temperature_result) => {
                                match &*ds18b20_water_temperature_result {
                                    Ok(ds18b20_water_temperature_result_data) => {
                                        self.ambient_temperature = ds18b20_water_temperature_result_data.value;
                                    }
                                    Err(_) => {
                                        // keep the values from previous measurement
                                    }
                                }
                            }
                            Err(e) => {
                                if !self.lock_error_ds18b20_water_temperature_mutex {
                                    self.lock_error_ds18b20_water_temperature_mutex = true;
                                    error!(target: LOG_TARGET_SENSOR_MANAGER, "error when trying to lock Ds18b20 water temperature mutex: {}", e);
                                }
                            }
                        }
                        
                    }
                    if self.config.source_ambient_temperature == sensor_manager_constants::SOURCE_WATER_TEMPERATURE_DS18B20 {
                        match mutexes.mutex_ds18b20_ambient_temperature.lock() {
                            Ok(ds18b20_ambient_temperature_result) => {
                                match &*ds18b20_ambient_temperature_result {
                                    Ok(ds18b20_ambient_temperature_result_data) => {
                                        self.ambient_temperature = ds18b20_ambient_temperature_result_data.value;
                                    }
                                    Err(_) => {
                                        // keep the values from previous measurement
                                    }
                                }
                            }
                            Err(e) => {
                                if !self.lock_error_ds18b20_ambient_temperature_mutex {
                                    self.lock_error_ds18b20_ambient_temperature_mutex = true;
                                    error!(target: LOG_TARGET_SENSOR_MANAGER, "error when trying to lock Ds18b20 ambient temperature mutex: {}", e);
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
                                    error!(target: LOG_TARGET_SENSOR_MANAGER, "error when trying to lock Dht mutex: {}", e);
                                }
                            }
                        }
                    }

                    //*** Atlas Scientific **********************************************************
                    {
                        match mutexes.mutex_atlas_scientific_temperature.lock() {
                            Ok(atlas_scientific_temperature_result) => {
                                match &*atlas_scientific_temperature_result {
                                    Ok(atlas_scientific_temperature_data) => {
                                        if self.config.source_water_temperature == sensor_manager_constants::SOURCE_WATER_TEMPERATURE_ATLAS_SCIENTIFIC {
                                            self.water_temperature = atlas_scientific_temperature_data.value;
                                        }
                                    }
                                    Err(_) => {
                                        // keep the values from previous measurement
                                    }
                                }
                            }
                            Err(e) => {
                                if !self.lock_error_atlas_scientific_temperature_mutex {
                                    self.lock_error_atlas_scientific_temperature_mutex = true;
                                    error!(target: LOG_TARGET_SENSOR_MANAGER, "error when trying to lock mutex: {}", e);
                                }
                            }
                        }
                    }

                    {
                        match mutexes.mutex_atlas_scientific_ph.lock() {
                            Ok(atlas_scientific_ph_result) => {
                                match &*atlas_scientific_ph_result {
                                    Ok(atlas_scientific_ph_data) => {
                                        self.pH = atlas_scientific_ph_data.value;
                                    }
                                    Err(_) => {
                                        // keep the values from previous measurement
                                    }
                                }
                            }
                            Err(e) => {
                                if !self.lock_error_atlas_scientific_ph_mutex {
                                    self.lock_error_atlas_scientific_ph_mutex = true;
                                    error!(target: LOG_TARGET_SENSOR_MANAGER, "error when trying to lock mutex: {}", e);
                                }
                            }
                        }
                    }

                    {
                        match mutexes.mutex_atlas_scientific_conductivity.lock() {
                            Ok(atlas_scientific_conductivity_result) => {
                                match &*atlas_scientific_conductivity_result {
                                    Ok(atlas_scientific_conductivity_data) => {
                                        self.conductivity =
                                            atlas_scientific_conductivity_data.value;
                                    }
                                    Err(_) => {
                                        // keep the values from previous measurement
                                    }
                                }
                            }
                            Err(e) => {
                                if !self.lock_error_atlas_scientific_conductivity_mutex {
                                    self.lock_error_atlas_scientific_conductivity_mutex = true;
                                    error!(target: LOG_TARGET_SENSOR_MANAGER, "error when trying to lock mutex: {}", e);
                                }
                            }
                        }
                    }

                    // write the measurement result to Mutex
                    {
                        match mutexes.mutex_sensor_manager_signals.lock() {
                            Ok(mut c) => {
                                c.ambient_humidity = self.ambient_humidity;
                                c.ambient_temperature = self.ambient_temperature;
                                c.water_temperature = self.water_temperature;
                                c.instant_last_recording = Instant::now();
                            }
                            Err(_) => {
                                // do nothing
                            }
                        }
                    }
                }
            }

            spin_sleeper.sleep(sleep_duration_hundred_millis);
        }

        match sensor_manager_channels
            .tx_sensor_manager_to_signal_handler
            .send(true)
        {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_SENSOR_MANAGER,
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // This thread has channel connections to underlying threads (GpioHandler).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match sensor_manager_channels
                .rx_sensor_manager_from_signal_handler
                .recv()
            {
                Ok(c) => match c {
                    InternalCommand::Terminate => {
                        self.lock_warn_inapplicable_command_signal_handler = false;
                        true
                    }
                    _ => {
                        if !self.lock_warn_inapplicable_command_signal_handler {
                            warn!(
                                target: LOG_TARGET_SENSOR_MANAGER,
                                "ignoring inapplicable command from signal handler ({})",
                                c
                            );
                            self.lock_warn_inapplicable_command_signal_handler = true;
                        }
                        false
                    }
                },
                Err(e) => {
                    if !self.lock_error_channel_receive_termination {
                        error!(
                            target: LOG_TARGET_SENSOR_MANAGER,
                            "receiving termination signal from signal handler failed ({e:?})"
                        );
                        self.lock_error_channel_receive_termination = true;
                    }
                    false
                }
            };
            spin_sleeper.sleep(sleep_duration_one_millis);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::sensors::atlas_scientific::AtlasScientificResultData;
    use crate::sensors::sensor_manager::{SensorManager, SensorManagerChannels, SensorManagerMutexes, SensorManagerSignals};
    use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
    use crate::utilities::config::{read_config_file, ConfigData};
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc;
    use std::sync::mpsc::TryRecvError;
    use std::sync::{Arc, Mutex};
    use std::thread::scope;
    use std::time::Duration;
    use crate::sensors::ds18b20::Ds18b20ResultData;

    #[test]
    #[should_panic]
    pub fn test_sensor_manager_panics_when_water_temperature_source_is_invalid() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.sensor_manager.source_water_temperature = "invalid".to_string();
        let _ = SensorManager::new(config.sensor_manager);
    }

    #[test]
    #[should_panic]
    pub fn test_sensor_manager_panics_when_ambient_temperature_source_is_invalid() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.sensor_manager.source_ambient_temperature = "invalid".to_string();
        let _ = SensorManager::new(config.sensor_manager);
    }

    #[test]
    // Test case verifies if the test object is processing the interfaces correctly.
    // @todo: toggle the switches and check if output is changing accordingly
    pub fn test_sensor_manager_interfaces() {
        // channels for communication with the test environment
        let (tx_tcp_to_sensor_manager, rx_sensor_manager_from_tcp) = mpsc::channel();
        let (tx_sensor_manager_to_tcp, rx_tcp_from_sensor_manager) = mpsc::channel();
        let tx_sensor_manager_to_tcp_opt = Some(tx_sensor_manager_to_tcp.clone());
        let rx_sensor_manager_from_tcp_opt = Some(rx_sensor_manager_from_tcp);
        let (tx_signal_handler_to_sensor_manager, rx_sensor_manager_from_signal_handler) =
            mpsc::channel();
        let (tx_sensor_manager_to_signal_handler, rx_signal_handler_from_sensor_manager) =
            mpsc::channel();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());

        let mutex_ds18b20_water_temperature = Arc::new(Mutex::new(Ok(Ds18b20ResultData::new(config.sensor_manager.replacement_value_water_temperature))));
        let mutex_ds18b20_ambient_temperature = Arc::new(Mutex::new(Ok(Ds18b20ResultData::new(config.sensor_manager.replacement_value_ambient_temperature))));
        let mutex_atlas_scientific_temperature =
            Arc::new(Mutex::new(Ok(AtlasScientificResultData::new(
                config.sensor_manager.replacement_value_water_temperature,
            ))));
        let mutex_atlas_scientific_conductivity = Arc::new(Mutex::new(Ok(
            AtlasScientificResultData::new(config.sensor_manager.replacement_value_conductivity),
        )));
        let mutex_atlas_scientific_ph = Arc::new(Mutex::new(Ok(AtlasScientificResultData::new(
            config.sensor_manager.replacement_value_ph,
        ))));
        let mutex_sensor_manager_signals = Arc::new(Mutex::new(SensorManagerSignals::new(
            &config.sensor_manager,
        )));
        let mut sensor_manager = SensorManager::new(config.sensor_manager);

        let dht_result = Ok((1.0, 2.0));
        let mutex_dht = Arc::new(Mutex::new(dht_result));

        scope(|scope| {
            scope.spawn(move || {
                // mock signal handler
                let sleep_duration_8_seconds = Duration::from_millis(8000);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_8_seconds);
                let _ = tx_signal_handler_to_sensor_manager.send(InternalCommand::Quit);
                rx_signal_handler_from_sensor_manager.recv().unwrap();
                let _ = tx_signal_handler_to_sensor_manager.send(InternalCommand::Terminate);
            });

            scope.spawn(move || {
                // mock TCP communication thread
                let sleep_duration_one_millisecond = Duration::from_millis(1);
                let spin_sleeper = SpinSleeper::default();
                loop {
                    match rx_tcp_from_sensor_manager.try_recv() {
                        Ok(c) => {
                            let response_value = match c {
                                InternalCommand::RequestSignal(d) => {
                                    match d {
                                        AquariumSignal::AmbientTemperature => 20.0,
                                        AquariumSignal::AmbientHumidity => 60.0,
                                        AquariumSignal::WaterTemperature => 25.0,
                                        AquariumSignal::Conductivity => 50000.0,
                                        AquariumSignal::pH => 7.0,
                                        _ => {
                                            panic!("Mock TCP communication: Received request for inapplicable signal  {}.", d);
                                        }
                                    }
                                },
                                InternalCommand::Quit => {
                                    break;
                                }
                                _ => {
                                    panic!("Mock TCP communication: Received invalid command.");
                                },
                            };
                            match tx_tcp_to_sensor_manager.send(response_value) {
                                Ok(()) => {}
                                Err(e) => {
                                    panic!(
                                        "Error in trying to send mock TCP communication response back to test object: {e:?}"
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!("Mock TCP communication: Error in trying to receive command from data logger: {e:?}");
                                }
                            }
                        }
                    }
                    spin_sleeper.sleep(sleep_duration_one_millisecond);
                }
            });

            scope.spawn(move || {
                let sensor_manager_channels = SensorManagerChannels {
                    tx_sensor_manager_to_tcp_opt: &tx_sensor_manager_to_tcp_opt,
                    rx_sensor_manager_from_tcp_opt: &rx_sensor_manager_from_tcp_opt,
                    tx_sensor_manager_to_signal_handler,
                    rx_sensor_manager_from_signal_handler,
                };

                let mutexes = SensorManagerMutexes {
                    mutex_ds18b20_ambient_temperature,
                    mutex_ds18b20_water_temperature,
                    mutex_dht,
                    mutex_atlas_scientific_temperature,
                    mutex_atlas_scientific_ph,
                    mutex_atlas_scientific_conductivity,
                    mutex_sensor_manager_signals,
                };
                
                // test object
                sensor_manager.execute(
                    sensor_manager_channels,
                    mutexes,
                );

                // signal end of test case execution to mock TCP communication thread
                let _ = tx_sensor_manager_to_tcp.send(InternalCommand::Quit);
            });
        });
    }
}
