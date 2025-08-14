/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#![allow(non_snake_case)]

#[cfg(target_os = "linux")]
use log::info;
use log::{error, warn};

#[cfg(target_os = "linux")]
use nix::unistd::gettid;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::sensors::atlas_scientific_error::AtlasScientificError;
use crate::sensors::i2c_interface::{I2cRequest, I2cResult};
use crate::simulator::get_resp_sim::GetResponseFromSimulatorTrait;
use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

// max loops for receiving when the quit command has been received
pub const RECEIVE_COUNTER_MAX: u32 = 10;

/// Holds the configuration data for the Atlas Scientific sensor communication.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct AtlasScientificConfig {
    /// allows deactivation of the data acquisition
    pub active: bool,

    /// I2C address of Atlas Scientific pH sensor unit
    #[allow(unused)]
    pub address_atlas_scientific_ph: u16,

    /// I2C address of Atlas Scientific conductivity sensor unit
    #[allow(unused)]
    pub address_atlas_scientific_conductivity: u16,

    /// I2C address of Atlas Scientific temperature sensor unit    
    #[allow(unused)]
    pub address_atlas_scientific_temperature: u16,

    /// sleep time in milliseconds before after writing before reading from the pH sensor unit
    #[allow(unused)]
    sleep_time_millis_ph: u64,

    /// sleep time in milliseconds before after writing before reading from the conductivity sensor unit
    #[allow(unused)]
    sleep_time_millis_conductivity: u64,

    /// sleep time in milliseconds before after writing before reading from the temperature sensor unit
    #[allow(unused)]
    sleep_time_millis_temperature: u64,

    /// maximum allowed value for pH used to assess if the temperature sensor reading is valid
    #[allow(unused)]
    max_val_atlas_scientific_ph: f32,

    /// minimum allowed value for pH used to assess if the temperature sensor reading is valid
    #[allow(unused)]
    min_val_atlas_scientific_ph: f32,

    /// maximum allowed value for temperature used to assess if the temperature sensor reading is valid
    #[allow(unused)]
    max_val_atlas_scientific_temperature: f32,

    /// minimum allowed value for temperature used to assess if the temperature sensor reading is valid
    #[allow(unused)]
    min_val_atlas_scientific_temperature: f32,

    /// maximum allowed value for conductivity used to assess if the conductivity sensor reading is valid
    #[allow(unused)]
    max_val_atlas_scientific_conductivity: f32,

    /// minimum allowed value for conductivity used to assess if the conductivity sensor reading is valid
    #[allow(unused)]
    min_val_atlas_scientific_conductivity: f32,

    /// measurement interval in milliseconds
    measurement_interval_millis: u64,
}

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

pub type AtlasScientificResult = Result<AtlasScientificResultData, AtlasScientificError>;

#[allow(dead_code)]
/// numeric values used in communication with Atlas Scientific sensor units
mod atlas_scientific_constants {
    /// command sent to sensor unit for initiating response
    pub const COMMAND_READ: u8 = b'R';

    /// length of the message received from the sensor (number of bytes)
    pub const MESSAGE_SIZE: usize = 8;

    /// location of the first character to be checked inside the message from the sensor
    pub const INDEX_FIRST_CHARACTER: usize = 0;

    /// start of data inside the message from the sensor
    pub const INDEX_CONTENT_START: usize = 1;

    /// finish of data inside the message from the sensor
    pub const INDEX_PENULTIMATE_CHARACTER: usize = 6;

    /// location of the last character to be checked inside the message from the sensor
    pub const INDEX_LAST_CHARACTER: usize = 7;

    /// ASCII value for representation of figure zero
    pub const ASCII_VAL_ZERO: u8 = 48;

    /// ASCII value for representation of figure nine
    pub const ASCII_VAL_NINE: u8 = 57;

    /// decimal point
    pub const DECIMAL_POINT: u8 = b'.';

    /// negative sign (note: positive sign does not occur in the message from the sensor)
    pub const NEGATIVE_SIGN: u8 = b'-';

    /// magic number at front position inside the message which identifies valid response from sensor unit
    pub const MESSAGE_START_MAGIC: u8 = 1;

    /// magic number at finish position inside the message which identifies valid response from sensor unit
    pub const MESSAGE_FINISH_MAGIC: u8 = 0;

    /// allow max. 10 milliseconds for mutex to be blocked by any other thread
    pub const MAX_MUTEX_ACCESS_DURATION_MILLIS: u64 = 10;
}

/// Struct collects the channels for the communication between AtlasScientific and other components.
pub struct AtlasScientificChannels {
    /// Sender part of the channel for communication to I2CInterface
    pub tx_atlas_scientific_to_i2c_interface: Sender<I2cRequest>,

    /// Receiver part of the channel for communication from I2CInterface   
    pub rx_atlas_scientific_from_i2c_interface: Receiver<I2cResult>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_atlas_scientific_to_signal_handler: Sender<bool>,

    /// Receiver part of the channel for communication from the signal handler                
    pub rx_atlas_scientific_from_signal_handler: Receiver<InternalCommand>,
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
pub struct AtlasScientific {
    /// configuration data for AtlasScientific
    config: AtlasScientificConfig,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    lock_error_channel_receive_termination: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via the channel from the signal handler thread
    lock_warn_inapplicable_command_signal_handler: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about to send request via the channel to I2cInterface thread
    lock_error_channel_send_i2c_interface: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for water temperature
    lock_warn_max_mutex_access_duration_temperature: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for pH
    lock_warn_max_mutex_access_duration_ph: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex for conductivity
    lock_warn_max_mutex_access_duration_conductivity: bool,

    // for testing purposes: record when mutex access time is exceeded without resetting it
    #[cfg(test)]
    mutex_temperature_access_duration_exceeded: bool,
    #[cfg(test)]
    mutex_ph_access_duration_exceeded: bool,
    #[cfg(test)]
    mutex_conductivity_access_duration_exceeded: bool,

    /// instant when the last measurement took place
    pub last_measurement_instant: Instant,

    /// duration of the measurement interval calculated from the configuration data
    pub measurement_interval: Duration,
}
impl ProcessExternalRequestTrait for AtlasScientific {}

impl GetResponseFromSimulatorTrait for AtlasScientific {}

impl AtlasScientific {
    /// Creates a new `AtlasScientific` control instance.
    ///
    /// This constructor initializes the Atlas Scientific sensor module. It takes the
    /// configuration, calculates the measurement interval, and sets up internal
    /// state, including lock flags used to prevent log flooding from recurring
    /// errors or warnings.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the Atlas Scientific module, specifying I2C
    ///   addresses, sleep times, and the measurement interval.
    ///
    /// # Returns
    /// A new `AtlasScientific` struct, ready to manage sensor data acquisition.
    pub fn new(config: AtlasScientificConfig) -> AtlasScientific {
        let measurement_interval = Duration::from_millis(config.measurement_interval_millis);

        AtlasScientific {
            config,
            lock_error_channel_receive_termination: false,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_send_i2c_interface: false,
            lock_warn_max_mutex_access_duration_temperature: false,
            lock_warn_max_mutex_access_duration_ph: false,
            lock_warn_max_mutex_access_duration_conductivity: false,

            #[cfg(test)]
            mutex_temperature_access_duration_exceeded: false,

            #[cfg(test)]
            mutex_ph_access_duration_exceeded: false,

            #[cfg(test)]
            mutex_conductivity_access_duration_exceeded: false,

            last_measurement_instant: Instant::now(),
            measurement_interval,
        }
    }

    /// Checks if a given character is a valid part of a numeric string, considering optional negative signs.
    ///
    /// This private helper function is used during the parsing of sensor responses to validate
    /// that each character in the data section is either a digit (`0`-`9`), a decimal point (`.`),
    /// or, if `negative_allowed` is `true`, a negative sign (`-`).
    ///
    /// # Arguments
    /// * `character` - The `u8` byte value of the character to be checked (typically an ASCII representation).
    /// * `negative_allowed` - A `bool` flag. If `true`, a negative sign character (`-`) will be considered
    ///   valid; otherwise, it will be considered invalid.
    ///
    /// # Returns
    /// `true` if the character is a valid numeric component based on the rules; `false` otherwise.
    fn is_numeric(character: u8, negative_allowed: bool) -> bool {
        match character {
            atlas_scientific_constants::ASCII_VAL_ZERO
                ..=atlas_scientific_constants::ASCII_VAL_NINE => true,
            atlas_scientific_constants::DECIMAL_POINT => true,
            atlas_scientific_constants::NEGATIVE_SIGN => negative_allowed,
            _ => false,
        }
    }

    /// Evaluates a raw byte array response from an Atlas Scientific sensor unit, validating its format and converting it to `f32`.
    ///
    /// This private helper function performs several critical steps to parse sensor data:
    /// 1.  **Magic Number Check**: Verifies the protocol's start and end "magic" bytes.
    /// 2.  **Numeric Content Validation**: Ensures all characters within the data section are valid numeric components (digits, decimal point, and optionally negative sign).
    /// 3.  **UTF-8 Conversion**: Converts the validated numeric byte sequence into a `String`.
    /// 4.  **Float Parsing**: Attempts to parse the `String` into an `f32` floating-point value.
    ///
    /// The `is_ph` flag specifically controls whether a negative sign is allowed, as pH values are non-negative by definition.
    ///
    /// # Arguments
    /// * `buffer` - A fixed-size array of `u8` bytes representing the complete message received from the Atlas Scientific sensor.
    /// * `is_ph` - A `bool` flag indicating whether the response is expected from a pH sensor (`true`) or another type (`false`).
    ///
    /// # Returns
    /// A `Result<f32, AtlasScientificError>`:
    /// - `Ok(f32)`: The successfully parsed floating-point sensor reading.
    /// - `Err(AtlasScientificError::FirstCharacterNotMagic)`: If the message's first byte is incorrect.
    /// - `Err(AtlasScientificError::LastCharacterNotMagic)`: If the message's last byte is incorrect.
    /// - `Err(AtlasScientificError::ContainsInvalidCharacter)`: If the data section contains non-numeric characters (or an unexpected negative sign for pH).
    /// - `Err(AtlasScientificError::InvalidUtf8Sequence)`: If the numeric byte sequence is not valid UTF-8.
    /// - `Err(AtlasScientificError::ConversionFailure)`: If the valid numeric string cannot be parsed into an `f32`.
    fn check_response(
        buffer: [u8; atlas_scientific_constants::MESSAGE_SIZE],
        is_ph: bool,
    ) -> Result<f32, AtlasScientificError> {
        // first check: first character not magic
        let first_character = buffer[atlas_scientific_constants::INDEX_FIRST_CHARACTER];
        if first_character != atlas_scientific_constants::MESSAGE_START_MAGIC {
            return Err(AtlasScientificError::FirstCharacterNotMagic(
                atlas_scientific_constants::MESSAGE_START_MAGIC,
                first_character,
            ));
        }
        // second check: last character not magic
        let last_character = buffer[atlas_scientific_constants::INDEX_LAST_CHARACTER];
        if last_character != atlas_scientific_constants::MESSAGE_FINISH_MAGIC {
            return Err(AtlasScientificError::LastCharacterNotMagic(
                atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
                last_character,
            ));
        }

        // get a slice of the array
        let numeric_content = &buffer[atlas_scientific_constants::INDEX_CONTENT_START
            ..atlas_scientific_constants::INDEX_PENULTIMATE_CHARACTER];

        // third check: all characters in between belong to a number
        for character in numeric_content {
            if !Self::is_numeric(*character, !is_ph) {
                return Err(AtlasScientificError::ContainsInvalidCharacter(*character));
            }
        }

        // conversion to String
        let numeric_string = match str::from_utf8(numeric_content) {
            Ok(c) => c,
            Err(e) => return Err(AtlasScientificError::InvalidUtf8Sequence { source: e }),
        };

        // conversion to f32
        match &numeric_string.parse::<f32>() {
            Ok(c) => Ok(*c),
            Err(e) => Err(AtlasScientificError::ConversionFailure { source: e.clone() }),
        }
    }

    /// Constructs and sends a measurement request to the I2C interface thread.
    ///
    /// This function creates an `I2cRequest` based on the specified `signal`. It
    /// retrieves the correct I2C address and sleep time from the configuration
    /// for the given sensor (pH, conductivity, or temperature). The request is then
    /// sent over the provided channel to the I2C thread for execution.
    ///
    /// It also manages an internal lock flag (`lock_error_channel_send_i2c_interface`)
    /// to prevent flooding the logs with repeated errors if the channel send operation fails.
    ///
    /// # Arguments
    /// * `signal` - The `AquariumSignal` identifying which sensor to request a reading from.
    /// * `tx_atlas_scientific_to_i2c_interface` - The sender channel for sending `I2cRequest`
    ///   messages to the I2C thread.
    ///
    /// # Returns
    /// `true` if the measurement request was successfully sent; `false` if the channel
    /// operation failed or if an unsupported signal was provided.
    fn send_request_to_i2c_interface(
        &mut self,
        signal: &AquariumSignal,
        tx_atlas_scientific_to_i2c_interface: &Sender<I2cRequest>,
    ) -> bool {
        let (i2c_address, sleep_time_millis) = match signal {
            AquariumSignal::pH => (
                self.config.address_atlas_scientific_ph,
                self.config.sleep_time_millis_ph,
            ),
            AquariumSignal::Conductivity => (
                self.config.address_atlas_scientific_conductivity,
                self.config.sleep_time_millis_conductivity,
            ),
            AquariumSignal::WaterTemperature => (
                self.config.address_atlas_scientific_temperature,
                self.config.sleep_time_millis_temperature,
            ),
            _ => {
                #[cfg(not(test))]
                warn!(
                    target: module_path!(),
                    "ignoring invalid signal request ({signal})"
                );
                return false;
            }
        };

        let send_buf = vec![atlas_scientific_constants::COMMAND_READ];
        let request = I2cRequest::new(
            i2c_address,
            send_buf,
            sleep_time_millis,
            atlas_scientific_constants::MESSAGE_SIZE,
        );

        match tx_atlas_scientific_to_i2c_interface.send(request) {
            Ok(()) => {
                self.lock_error_channel_send_i2c_interface = false;
                true
            }
            Err(e) => {
                if !self.lock_error_channel_send_i2c_interface {
                    error!(
                        target: module_path!(),
                        "sending request to I2cInterface thread failed. ({e:?})"
                    );
                    self.lock_error_channel_send_i2c_interface = true;
                }
                false
            }
        }
    }

    /// Logs a warning if acquiring a sensor's mutex takes longer than the configured maximum.
    ///
    /// This helper function is invoked when the time spent waiting to lock a mutex for a
    /// specific sensor signal (temperature, pH, or conductivity) exceeds a predefined
    /// threshold. To avoid flooding the log files with repetitive warnings for a
    /// persistent issue, it uses an internal lock flag for each signal.
    ///
    /// The warning is logged only the first time the condition is detected. The lock
    /// is reset when later mutex access for the same signal completes within
    /// the allowed time outside of this function.
    ///
    /// # Arguments
    /// * `signal` - The `AquariumSignal` identifying which sensor's mutex was slow to acquire.
    /// * `measured_mutex_access_duration` - The `Duration` that was actually measured for the lock acquisition.
    /// * `max_mutex_access_duration` - The maximum `Duration` allowed before a warning is triggered.
    #[allow(unused)]
    fn warn_about_long_mutex_wait(
        &mut self,
        signal: &AquariumSignal,
        measured_mutex_access_duration: Duration,
        max_mutex_access_duration: Duration,
    ) {
        match signal {
            AquariumSignal::WaterTemperature => {
                if !self.lock_warn_max_mutex_access_duration_temperature {
                    self.lock_warn_max_mutex_access_duration_temperature = true;
                    #[cfg(not(test))]
                    warn!(
                        target: module_path!(),
                        "Access to temperature mutex took too long. Allowed={}ms, Measured={}ms",
                        max_mutex_access_duration.as_millis(),
                        measured_mutex_access_duration.as_millis(),
                    );
                    // record for testing purposes only
                    cfg_if::cfg_if! {
                        if #[cfg(test)] {
                            self.mutex_temperature_access_duration_exceeded = true;
                        }
                    }
                }
            }
            AquariumSignal::pH => {
                if !self.lock_warn_max_mutex_access_duration_ph {
                    self.lock_warn_max_mutex_access_duration_ph = true;
                    #[cfg(not(test))]
                    warn!(
                        target: module_path!(),
                        "Access to pH mutex took too long. Allowed={}ms, Measured={}ms",
                        max_mutex_access_duration.as_millis(),
                        measured_mutex_access_duration.as_millis(),
                    );
                    // record for testing purposes only
                    cfg_if::cfg_if! {
                        if #[cfg(test)] {
                            self.mutex_ph_access_duration_exceeded = true;
                        }
                    }
                }
            }
            AquariumSignal::Conductivity => {
                if !self.lock_warn_max_mutex_access_duration_conductivity {
                    self.lock_warn_max_mutex_access_duration_conductivity = true;
                    #[cfg(not(test))]
                    warn!(
                        target: module_path!(),
                        "Access to conductivity mutex took too long. Allowed={}ms, Measured={}ms",
                        max_mutex_access_duration.as_millis(),
                        measured_mutex_access_duration.as_millis(),
                    );
                    // record for testing purposes only
                    cfg_if::cfg_if! {
                        if #[cfg(test)] {
                            self.mutex_conductivity_access_duration_exceeded = true;
                        }
                    }
                }
            }
            _ => { /* do nothing */ }
        }
    }

    /// Resets the warning lock flag for a given sensor's mutex.
    ///
    /// This function is called after a mutex has been successfully and quickly acquired,
    /// indicating that a potential previous condition of a slow mutex lock has resolved.
    ///
    /// By setting the corresponding internal lock flag back to `false`, it re-enables
    /// the `warn_about_long_mutex_wait` function to log a new warning if the mutex
    /// becomes slow to acquire again in the future.
    ///
    /// # Arguments
    /// * `signal` - The `AquariumSignal` identifying which sensor's warning lock to reset.
    fn reset_warning_flag(&mut self, signal: &AquariumSignal) {
        match signal {
            AquariumSignal::WaterTemperature => {
                self.lock_warn_max_mutex_access_duration_temperature = false;
            }
            AquariumSignal::Conductivity => {
                self.lock_warn_max_mutex_access_duration_conductivity = false;
            }
            AquariumSignal::pH => {
                self.lock_warn_max_mutex_access_duration_ph = false;
            }
            _ => { /* do nothing */ }
        }
    }

    /// Atomically writes a sensor measurement result to the appropriate shared mutex.
    ///
    /// This helper function directs an `AtlasScientificResult` to its corresponding
    /// shared `Mutex` based on the provided `signal`. It handles the logic for
    /// selecting the correct mutex (temperature, pH, or conductivity), locking it,
    /// and updating its value.
    ///
    /// This encapsulation simplifies the calling code by centralizing the
    /// mutex selection and locking logic.
    ///
    /// # Arguments
    /// * `signal` - The `AquariumSignal` that determines which mutex to update.
    /// * `result` - The `AtlasScientificResult` (containing either sensor data or an error) to be written.
    /// * `mutex_water_temperature` - The shared mutex for the water temperature sensor result.
    /// * `mutex_ph` - The shared mutex for the pH sensor result.
    /// * `mutex_conductivity` - The shared mutex for the conductivity sensor result.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(Instant)`: On successful write, returns the `Instant` at which the lock was released.
    ///   The caller can use this to measure total lock duration.
    /// - `Err(AtlasScientificError::CouldNotLockMutex)`: If the target mutex is poisoned and
    ///   cannot be locked.
    fn write_result_to_mutex(
        &self,
        signal: &AquariumSignal,
        result: AtlasScientificResultData,
        mutex_water_temperature: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_ph: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_conductivity: &Arc<Mutex<AtlasScientificResultData>>,
    ) -> Result<Instant, AtlasScientificError> {
        match signal {
            AquariumSignal::WaterTemperature => match mutex_water_temperature.lock() {
                Ok(mut water_temperature) => *water_temperature = result,
                Err(_) => {
                    return Err(AtlasScientificError::CouldNotLockMutex {
                        signal: signal.clone(),
                    });
                }
            },
            AquariumSignal::pH => match mutex_ph.lock() {
                Ok(mut ph) => *ph = result,
                Err(_) => {
                    return Err(AtlasScientificError::CouldNotLockMutex {
                        signal: signal.clone(),
                    });
                }
            },
            AquariumSignal::Conductivity => match mutex_conductivity.lock() {
                Ok(mut conductivity) => *conductivity = result,
                Err(_) => {
                    return Err(AtlasScientificError::CouldNotLockMutex {
                        signal: signal.clone(),
                    });
                }
            },
            _ => { /* do nothing */ }
        }
        Ok(Instant::now())
    }

    /// Receives and processes a measurement response from the I2C interface thread.
    ///
    /// This function orchestrates the full cycle of handling a sensor reading. It performs a
    /// non-blocking receive on the I2C channel. If a message is available, it processes
    /// the result, which can be either a successful data payload or an I2C-level error.
    ///
    /// For a successful payload, the function:
    /// 1. Parses the raw byte buffer using `check_response`.
    /// 2. Converts the valid data into an `AtlasScientificResultData` struct.
    /// 3. Atomically writes this result to the correct shared `Mutex` (identified by `signal`)
    ///    using the `write_result_to_mutex` helper.
    /// 4. Monitors the duration of the mutex lock, issuing a warning via
    ///    `warn_about_long_mutex_wait` if it exceeds the configured threshold.
    ///
    /// The function also handles all error conditions, wrapping them in the appropriate
    /// `AtlasScientificError` variant for the caller to handle.
    ///
    /// # Arguments
    /// * `signal` - The `AquariumSignal` indicating which sensor's data is expected (e.g., Temperature, pH).
    /// * `rx_atlas_scientific_from_i2c_interface` - The receiver channel for `I2cResult` messages from the I2C thread.
    /// * `mutex_water_temperature` - The shared `Mutex` for storing the water temperature result.
    /// * `mutex_ph` - The shared `Mutex` for storing the pH result.
    /// * `mutex_conductivity` - The shared `Mutex` for storing the conductivity result.
    ///
    /// # Returns
    /// An `AtlasScientificResult` which is:
    /// - `Ok(AtlasScientificResultData)`: If a valid sensor reading was successfully received, parsed, and stored in its mutex.
    /// - `Err(AtlasScientificError)`: If any error occurred. This includes:
    ///   - `ChannelIsEmpty`: A recoverable error indicating no message was available. The caller should try again.
    ///   - `ChannelIsDisconnected`: A critical error indicating the I2C thread has terminated.
    ///   - `I2cCommunicationFailure`: The I2C thread reported a hardware-level communication error.
    ///   - `ResponseCheckFailed` / `IncorrectBufferLength`: The received data payload was malformed.
    ///   - `CouldNotLockMutex`: The target mutex was poisoned and could not be updated.
    fn receive_response_from_i2c_interface(
        &mut self,
        signal: &AquariumSignal,
        rx_atlas_scientific_from_i2c_interface: &Receiver<I2cResult>,
        mutex_water_temperature: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_ph: &Arc<Mutex<AtlasScientificResultData>>,
        mutex_conductivity: &Arc<Mutex<AtlasScientificResultData>>,
    ) -> AtlasScientificResult {
        let max_mutex_access_duration =
            Duration::from_millis(atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS);
        let is_ph = matches!(signal, AquariumSignal::pH);
        let mut result: AtlasScientificResult = Err(AtlasScientificError::NotInitialized);

        match rx_atlas_scientific_from_i2c_interface.try_recv() {
            Ok(i2c_result) => match i2c_result {
                // Result from channel is available
                Ok(i2c_response) => {
                    let result_unevaluated = match i2c_response.read_buf.try_into() {
                        Ok(response_buffer) =>
                        // type for result is defined by parameter definition of check_response
                        {
                            match Self::check_response(response_buffer, is_ph) {
                                Ok(value) => Ok(AtlasScientificResultData::new(value)),
                                Err(e) => Err(AtlasScientificError::ResponseCheckFailed {
                                    signal: signal.clone(),
                                    source: Box::new(e),
                                }),
                            }
                        }
                        Err(e) => Err(AtlasScientificError::IncorrectBufferLength(
                            signal.clone(),
                            e.len(),
                        )),
                    };

                    // Write to mutex only if reading from sensor was successful
                    if let Ok(result_data) = result_unevaluated {
                        let instant_before_locking_mutex = Instant::now();
                        let result_data_clone = result_data.clone();
                        let instant_after_locking_mutex = self.write_result_to_mutex(
                            signal,
                            result_data,
                            mutex_water_temperature,
                            mutex_ph,
                            mutex_conductivity,
                        )?;

                        // check if access to mutex took too long
                        let measured_mutex_access_duration = instant_after_locking_mutex
                            .duration_since(instant_before_locking_mutex);
                        if measured_mutex_access_duration > max_mutex_access_duration {
                            self.warn_about_long_mutex_wait(
                                signal,
                                measured_mutex_access_duration,
                                max_mutex_access_duration,
                            );
                        } else {
                            self.reset_warning_flag(signal);
                        }

                        result = Ok(result_data_clone);
                    }
                }
                Err(e) => {
                    result = Err(AtlasScientificError::I2cCommunicationFailure {
                        signal: signal.clone(),
                        source: e,
                    })
                }
            },
            Err(e) => match e {
                TryRecvError::Empty => {
                    result = Err(AtlasScientificError::ChannelIsEmpty);
                }
                TryRecvError::Disconnected => {
                    result = Err(AtlasScientificError::ChannelIsDisconnected { source: e });
                }
            },
        }
        result
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
    /// * `atlas_scientific_channels` - A struct containing all `mpsc` channels necessary for
    ///   inter-thread communication with the `I2CInterface` and the `SignalHandler`.
    /// * `mutex_atlas_scientific_water_temperature` - The shared `Mutex` for storing the water
    ///   temperature result.
    /// * `mutex_atlas_scientific_ph` - The shared `Mutex` for storing the pH result.
    /// * `mutex_atlas_scientific_conductivity` - The shared `Mutex` for storing the conductivity result.
    pub fn execute(
        &mut self,
        atlas_scientific_channels: AtlasScientificChannels,
        mutex_atlas_scientific_water_temperature: Arc<Mutex<AtlasScientificResultData>>,
        mutex_atlas_scientific_ph: Arc<Mutex<AtlasScientificResultData>>,
        mutex_atlas_scientific_conductivity: Arc<Mutex<AtlasScientificResultData>>,
    ) {
        #[cfg(target_os = "linux")]
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
                &atlas_scientific_channels.rx_atlas_scientific_from_signal_handler,
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
                //#[cfg(test)]
                //println!(
                //    "{}: state_send_receive={}, duration since last measurement={}, measurement_interval={}",
                //    module_path!(),
                //    state_send_receive,
                //    duration_since_last_measurement.as_millis(),
                //    self.measurement_interval.as_millis(),
                //);
                if !state_send_receive
                    && duration_since_last_measurement > self.measurement_interval
                {
                    #[cfg(test)]
                    println!(
                        "{}: Sending to i2c interface request for {}",
                        module_path!(),
                        signal
                    );
                    state_send_receive = self.send_request_to_i2c_interface(
                        &signal,
                        &atlas_scientific_channels.tx_atlas_scientific_to_i2c_interface,
                    );
                    #[cfg(test)]
                    println!(
                        "{}: Sent to i2c interface request for {}",
                        module_path!(),
                        signal
                    );
                } else if state_send_receive {
                    // read from the channel if the measurement result is available from I2cInterface thread
                    match self.receive_response_from_i2c_interface(
                        &signal,
                        &atlas_scientific_channels.rx_atlas_scientific_from_i2c_interface,
                        &mutex_atlas_scientific_water_temperature,
                        &mutex_atlas_scientific_ph,
                        &mutex_atlas_scientific_conductivity,
                    ) {
                        Ok(_) => {
                            state_send_receive = false; // allow sending again
                        }
                        Err(e) => {
                            if matches!(e, AtlasScientificError::ChannelIsEmpty) {
                                state_send_receive = true; // receive again
                            } else if matches!(
                                e,
                                AtlasScientificError::ChannelIsDisconnected { source: _ }
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
            }

            spin_sleeper.sleep(sleep_duration_ten_millis);
        }

        match atlas_scientific_channels
            .tx_atlas_scientific_to_signal_handler
            .send(true)
        {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: module_path!(),
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // This thread has channel connections to underlying threads (GpioHandler).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match atlas_scientific_channels
                .rx_atlas_scientific_from_signal_handler
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
                                target: module_path!(),
                                "ignoring inapplicable command from signal handler ({c})"
                            );
                            self.lock_warn_inapplicable_command_signal_handler = true;
                        }
                        false
                    }
                },
                Err(e) => {
                    if !self.lock_error_channel_receive_termination {
                        error!(
                            target: module_path!(),
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
    use crate::mocks::mock_i2c_interface::tests::MockI2cInterface;
    use crate::sensors::atlas_scientific::{
        atlas_scientific_constants, AtlasScientific, AtlasScientificChannels, AtlasScientificError,
        AtlasScientificResultData,
    };
    use crate::sensors::i2c_error::I2cError;
    use crate::sensors::i2c_interface::{I2cRequest, I2cResponse, I2cResult};
    use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
    use crate::utilities::config::{read_config_file, ConfigData};
    use assert_float_eq::*;
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    // happy case for evaluating response from pH sensor unit
    pub fn test_atlas_scientific_check_response_valid_ph() {
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '8' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let result = AtlasScientific::check_response(stimuli, true);
        match result {
            Ok(c) => {
                assert_float_absolute_eq!(c, 8.0, 0.001);
            }
            Err(e) => {
                panic!("test_atlas_scientific_check_response_valid_ph returned error: {e:?}");
            }
        }
    }
    #[test]
    // evaluating response from pH sensor unit
    // checking if negative values are correctly identified
    pub fn test_atlas_scientific_check_response_invalid_ph_negative() {
        let sign = '-' as u8;

        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            sign,
            '8' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];

        let result = AtlasScientific::check_response(stimuli, true);

        let error_result = result.unwrap_err();

        assert!(matches!(
            error_result,
            AtlasScientificError::ContainsInvalidCharacter(_sign)
        ));
    }

    #[test]
    // happy case for evaluating response from conductivity sensor
    pub fn test_i2c_interface_check_response_valid_conductivity() {
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '5' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let result = AtlasScientific::check_response(stimuli, false);
        match result {
            Ok(c) => {
                assert_float_absolute_eq!(c, 50000.0, 0.001);
            }
            Err(e) => {
                panic!("test_i2c_interface_check_response_valid_conductivity return error: {e:?}");
            }
        }
    }

    // happy case for evaluating the response from the temperature sensor unit
    #[test]
    pub fn test_atlas_scientific_check_response_valid_temperature() {
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '2' as u8,
            '5' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let result = AtlasScientific::check_response(stimuli, false);
        match result {
            Ok(c) => {
                assert_float_absolute_eq!(c, 25.0, 0.001);
            }
            Err(e) => {
                panic!(
                    "test_atlas_scientific_check_response_valid_temperature return error: {e:?}"
                );
            }
        }
    }

    #[test]
    // checking if an incorrect magic number in the front of the message is identified correctly
    pub fn test_atlas_scientific_check_response_invalid_magic_front() {
        let incorrect_magic_front = atlas_scientific_constants::MESSAGE_START_MAGIC - 1;

        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            incorrect_magic_front,
            '2' as u8,
            '5' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];

        let result = AtlasScientific::check_response(stimuli, false);

        let error_result = result.unwrap_err();

        assert!(matches!(
            error_result,
            AtlasScientificError::FirstCharacterNotMagic(
                atlas_scientific_constants::MESSAGE_START_MAGIC,
                _incorrect_magic_front
            )
        ));
    }

    #[test]
    // checking if an incorrect magic number in the rear part of the message is identified correctly
    pub fn test_atlas_scientific_check_response_invalid_magic_rear() {
        let incorrect_magic_rear = atlas_scientific_constants::MESSAGE_FINISH_MAGIC + 1;

        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '2' as u8,
            '5' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            incorrect_magic_rear,
        ];

        let result = AtlasScientific::check_response(stimuli, false);

        let error_result = result.unwrap_err();

        assert!(matches!(
            error_result,
            AtlasScientificError::LastCharacterNotMagic(
                atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
                _incorrect_magic_rear
            )
        ));
    }

    #[test]
    // checking if an incorrect sequence of numeric characters is identified correctly
    pub fn test_atlas_scientific_check_response_invalid_conversion() {
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '2' as u8,
            '5' as u8,
            atlas_scientific_constants::NEGATIVE_SIGN,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        assert!(matches!(
            AtlasScientific::check_response(stimuli, false),
            Err(AtlasScientificError::ConversionFailure { source: _ })
        ));
    }

    #[test]
    // Test case checks if the check_response function correctly identifies an
    // invalid non-numeric character (like an alphabet letter) in the data payload.
    fn test_atlas_scientific_check_response_invalid_character_in_payload() {
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            b'2',
            b'5',
            atlas_scientific_constants::DECIMAL_POINT,
            b'A', // Invalid character
            b'0',
            b'0',
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let result = AtlasScientific::check_response(stimuli, false);

        let error_result = result.unwrap_err();

        assert!(matches!(
            error_result,
            AtlasScientificError::ContainsInvalidCharacter(b'A')
        ));
    }

    #[test]
    // Test the error response of get_response_from_i2c when requesting an invalid signal.
    pub fn test_atlas_scientific_error_response_to_invalid_signal_request() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let mut atlas_scientific = AtlasScientific::new(config.atlas_scientific);

        // Atlas Scientific <=> i2c_interface
        let (tx_atlas_scientific_to_i2c_interface, _rx_i2c_interface_from_atlas_scientific): (
            Sender<I2cRequest>,
            Receiver<I2cRequest>,
        ) = mpsc::channel();
        let (_tx_i2c_interface_to_atlas_scientific, _rx_atlas_scientific_from_i2c_interface): (
            Sender<Result<I2cResponse, I2cError>>,
            Receiver<Result<I2cResponse, I2cError>>,
        ) = mpsc::channel();
        let result = atlas_scientific.send_request_to_i2c_interface(
            &AquariumSignal::MockInvalidSignal,
            &tx_atlas_scientific_to_i2c_interface,
        );

        assert_eq!(result, false);
    }

    #[test]
    // Test function receive_response_from_i2c_interface
    pub fn test_receive_response_from_i2c_interface() {
        let stimuli_array_temperature: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '2' as u8,
            '3' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '1' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let stimuli_i2c_response_temperature = I2cResponse {
            read_buf: stimuli_array_temperature.into(),
            length: atlas_scientific_constants::MESSAGE_SIZE,
        };

        let stimuli_array_conductivity: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '4' as u8,
            '8' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let stimuli_i2c_response_conductivity = I2cResponse {
            read_buf: stimuli_array_conductivity.into(),
            length: atlas_scientific_constants::MESSAGE_SIZE,
        };

        let stimuli_array_ph: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '7' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '2' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let stimuli_i2c_response_ph = I2cResponse {
            read_buf: stimuli_array_ph.into(),
            length: atlas_scientific_constants::MESSAGE_SIZE,
        };

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

        let mut atlas_scientific = AtlasScientific::new(config.atlas_scientific);

        let (_tx_atlas_scientific_to_i2c_interface, _rx_i2c_interface_from_atlas_scientific): (
            Sender<I2cRequest>,
            Receiver<I2cRequest>,
        ) = mpsc::channel();

        let (tx_i2c_interface_to_atlas_scientific, rx_atlas_scientific_from_i2c_interface): (
            Sender<I2cResult>,
            Receiver<I2cResult>,
        ) = mpsc::channel();

        // Mutexes for Atlas Scientific signals
        let mutex_atlas_scientific_temperature =
            Arc::new(Mutex::new(AtlasScientificResultData::new(
                config.sensor_manager.replacement_value_water_temperature,
            )));

        let mutex_atlas_scientific_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(
            config.sensor_manager.replacement_value_ph,
        )));

        let mutex_atlas_scientific_conductivity = Arc::new(Mutex::new(
            AtlasScientificResultData::new(config.sensor_manager.replacement_value_conductivity),
        ));

        // Sending stimuli for water temperature before triggering try_recv from the test object
        _ = tx_i2c_interface_to_atlas_scientific.send(Ok(stimuli_i2c_response_temperature));

        _ = atlas_scientific.receive_response_from_i2c_interface(
            &AquariumSignal::WaterTemperature,
            &rx_atlas_scientific_from_i2c_interface,
            &mutex_atlas_scientific_temperature,
            &mutex_atlas_scientific_ph,
            &mutex_atlas_scientific_conductivity,
        );

        match mutex_atlas_scientific_temperature.lock() {
            Ok(result) => {
                let result_data = result.clone();
                assert_eq!(result_data.value, 23.1);
            }
            Err(e) => {
                panic!("mutex_atlas_scientific_temperature lock poisoned: {:?}", e);
            }
        };

        // Sending stimuli for pH before triggering try_recv from the test object
        _ = tx_i2c_interface_to_atlas_scientific.send(Ok(stimuli_i2c_response_ph));

        _ = atlas_scientific.receive_response_from_i2c_interface(
            &AquariumSignal::pH,
            &rx_atlas_scientific_from_i2c_interface,
            &mutex_atlas_scientific_temperature,
            &mutex_atlas_scientific_ph,
            &mutex_atlas_scientific_conductivity,
        );

        match mutex_atlas_scientific_ph.lock() {
            Ok(result) => {
                let result_data = result.clone();
                assert_eq!(result_data.value, 7.2);
            }
            Err(e) => {
                panic!("mutex_atlas_scientific_ph lock poisoned: {:?}", e);
            }
        };

        // Sending stimuli for conductivity before triggering try_recv from the test object
        _ = tx_i2c_interface_to_atlas_scientific.send(Ok(stimuli_i2c_response_conductivity));

        _ = atlas_scientific.receive_response_from_i2c_interface(
            &AquariumSignal::Conductivity,
            &rx_atlas_scientific_from_i2c_interface,
            &mutex_atlas_scientific_temperature,
            &mutex_atlas_scientific_ph,
            &mutex_atlas_scientific_conductivity,
        );

        match mutex_atlas_scientific_conductivity.lock() {
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
    // Test function receive_response_from_i2c_interface
    pub fn test_atlas_scientific_interfaces_without_blocking_mutexes() {
        let stimuli_array_temperature: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '2' as u8,
            '3' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '1' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let stimuli_i2c_response_temperature = I2cResponse {
            read_buf: stimuli_array_temperature.into(),
            length: atlas_scientific_constants::MESSAGE_SIZE,
        };

        let stimuli_array_conductivity: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '4' as u8,
            '8' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let stimuli_i2c_response_conductivity = I2cResponse {
            read_buf: stimuli_array_conductivity.into(),
            length: atlas_scientific_constants::MESSAGE_SIZE,
        };

        let stimuli_array_ph: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '7' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '2' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let stimuli_i2c_response_ph = I2cResponse {
            read_buf: stimuli_array_ph.into(),
            length: atlas_scientific_constants::MESSAGE_SIZE,
        };

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

        let measurement_interval_millis_for_test_environment =
            config.atlas_scientific.measurement_interval_millis;

        let mut mock_i2c_interface = MockI2cInterface::new(
            &config.atlas_scientific,
            stimuli_i2c_response_temperature.clone(),
            stimuli_i2c_response_ph.clone(),
            stimuli_i2c_response_conductivity.clone(),
        );

        let mut atlas_scientific = AtlasScientific::new(config.atlas_scientific);

        let (tx_atlas_scientific_to_i2c_interface, rx_i2c_interface_from_atlas_scientific): (
            Sender<I2cRequest>,
            Receiver<I2cRequest>,
        ) = mpsc::channel();

        let (tx_i2c_interface_to_atlas_scientific, rx_atlas_scientific_from_i2c_interface): (
            Sender<I2cResult>,
            Receiver<I2cResult>,
        ) = mpsc::channel();

        let (tx_i2c_interface_to_signal_handler, _rx_signal_handler_from_i2c_interface): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();

        let (tx_signal_handler_to_i2c_interface, rx_i2c_interface_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();

        let (tx_atlas_scientific_to_signal_handler, _rx_signal_handler_from_atlas_scientific): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();

        let (tx_signal_handler_to_atlas_scientific, rx_atlas_scientific_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();

        let atlas_scientific_channels = AtlasScientificChannels {
            tx_atlas_scientific_to_i2c_interface,
            rx_atlas_scientific_from_i2c_interface,
            tx_atlas_scientific_to_signal_handler,
            rx_atlas_scientific_from_signal_handler,
        };

        // Mutexes for Atlas Scientific signals
        let mutex_atlas_scientific_temperature =
            Arc::new(Mutex::new(AtlasScientificResultData::new(
                config.sensor_manager.replacement_value_water_temperature,
            )));
        let mutex_atlas_scientific_temperature_clone_for_asserts =
            mutex_atlas_scientific_temperature.clone();

        let mutex_atlas_scientific_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(
            config.sensor_manager.replacement_value_ph,
        )));
        let mutex_atlas_scientific_ph_clone_for_asserts = mutex_atlas_scientific_ph.clone();

        let mutex_atlas_scientific_conductivity = Arc::new(Mutex::new(
            AtlasScientificResultData::new(config.sensor_manager.replacement_value_conductivity),
        ));
        let mutex_atlas_scientific_conductivity_clone_for_asserts =
            mutex_atlas_scientific_conductivity.clone();

        // thread for test environment (incl. signal handler)
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                // let test run a bit longer than the measurement interval
                let sleep_time = Duration::from_millis(
                    measurement_interval_millis_for_test_environment * 3 + 1000,
                );
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_time);
                tx_signal_handler_to_atlas_scientific
                    .send(InternalCommand::Quit)
                    .unwrap();
                tx_signal_handler_to_i2c_interface
                    .send(InternalCommand::Quit)
                    .unwrap();
                tx_signal_handler_to_atlas_scientific
                    .send(InternalCommand::Terminate)
                    .unwrap();
            })
            .unwrap();

        // thread for mock i2c interface
        let join_handle_mock_i2c_interface = thread::Builder::new()
            .name("i2c_interface".to_string())
            .spawn(move || {
                mock_i2c_interface.execute(
                    tx_i2c_interface_to_atlas_scientific,
                    rx_i2c_interface_from_atlas_scientific,
                    tx_i2c_interface_to_signal_handler,
                    rx_i2c_interface_from_signal_handler,
                );
            })
            .unwrap();

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                atlas_scientific.execute(
                    atlas_scientific_channels,
                    mutex_atlas_scientific_temperature,
                    mutex_atlas_scientific_ph,
                    mutex_atlas_scientific_conductivity,
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
    // Test function receive_response_from_i2c_interface
    pub fn test_atlas_scientific_interfaces_with_blocking_mutexes() {
        let stimuli_array_temperature: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '2' as u8,
            '3' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '1' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let stimuli_i2c_response_temperature = I2cResponse {
            read_buf: stimuli_array_temperature.into(),
            length: atlas_scientific_constants::MESSAGE_SIZE,
        };

        let stimuli_array_conductivity: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '4' as u8,
            '8' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let stimuli_i2c_response_conductivity = I2cResponse {
            read_buf: stimuli_array_conductivity.into(),
            length: atlas_scientific_constants::MESSAGE_SIZE,
        };

        let stimuli_array_ph: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '7' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '2' as u8,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        let stimuli_i2c_response_ph = I2cResponse {
            read_buf: stimuli_array_ph.into(),
            length: atlas_scientific_constants::MESSAGE_SIZE,
        };

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();

        let measurement_interval_millis_for_test_environment =
            config.atlas_scientific.measurement_interval_millis;

        let mut mock_i2c_interface = MockI2cInterface::new(
            &config.atlas_scientific,
            stimuli_i2c_response_temperature.clone(),
            stimuli_i2c_response_ph.clone(),
            stimuli_i2c_response_conductivity.clone(),
        );

        let mut atlas_scientific = AtlasScientific::new(config.atlas_scientific);

        let (tx_atlas_scientific_to_i2c_interface, rx_i2c_interface_from_atlas_scientific): (
            Sender<I2cRequest>,
            Receiver<I2cRequest>,
        ) = mpsc::channel();

        let (tx_i2c_interface_to_atlas_scientific, rx_atlas_scientific_from_i2c_interface): (
            Sender<I2cResult>,
            Receiver<I2cResult>,
        ) = mpsc::channel();

        let (tx_i2c_interface_to_signal_handler, _rx_signal_handler_from_i2c_interface): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();

        let (tx_signal_handler_to_i2c_interface, rx_i2c_interface_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();

        let (tx_atlas_scientific_to_signal_handler, _rx_signal_handler_from_atlas_scientific): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();

        let (tx_signal_handler_to_atlas_scientific, rx_atlas_scientific_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();

        let atlas_scientific_channels = AtlasScientificChannels {
            tx_atlas_scientific_to_i2c_interface,
            rx_atlas_scientific_from_i2c_interface,
            tx_atlas_scientific_to_signal_handler,
            rx_atlas_scientific_from_signal_handler,
        };

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
                    Ok(_) => {
                        spin_sleeper.sleep(sleep_time);
                    }
                    Err(e) => {
                        panic!("mutex_atlas_scientific_temperature lock poisoned: {:?}", e);
                    }
                };
                match mutex_atlas_scientific_ph_clone_for_test_environment.lock() {
                    Ok(_) => {
                        spin_sleeper.sleep(sleep_time);
                    }
                    Err(e) => {
                        panic!("mutex_atlas_scientific_ph lock poisoned: {:?}", e);
                    }
                };
                match mutex_atlas_scientific_conductivity_clone_for_test_environment.lock() {
                    Ok(_) => {
                        spin_sleeper.sleep(sleep_time);
                    }
                    Err(e) => {
                        panic!("mutex_atlas_scientific_conductivity lock poisoned: {:?}", e);
                    }
                };

                tx_signal_handler_to_atlas_scientific
                    .send(InternalCommand::Quit)
                    .unwrap();
                tx_signal_handler_to_i2c_interface
                    .send(InternalCommand::Quit)
                    .unwrap();
                tx_signal_handler_to_atlas_scientific
                    .send(InternalCommand::Terminate)
                    .unwrap();
            })
            .unwrap();

        // thread for mock i2c interface
        let join_handle_mock_i2c_interface = thread::Builder::new()
            .name("i2c_interface".to_string())
            .spawn(move || {
                mock_i2c_interface.execute(
                    tx_i2c_interface_to_atlas_scientific,
                    rx_i2c_interface_from_atlas_scientific,
                    tx_i2c_interface_to_signal_handler,
                    rx_i2c_interface_from_signal_handler,
                );
            })
            .unwrap();

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                atlas_scientific.execute(
                    atlas_scientific_channels,
                    mutex_atlas_scientific_temperature,
                    mutex_atlas_scientific_ph,
                    mutex_atlas_scientific_conductivity,
                );
                assert_eq!(
                    atlas_scientific.mutex_temperature_access_duration_exceeded,
                    true
                );
                assert_eq!(atlas_scientific.mutex_ph_access_duration_exceeded, true);
                assert_eq!(
                    atlas_scientific.mutex_conductivity_access_duration_exceeded,
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
    // Test case checks if the is_numeric function correctly identifies valid digits.
    fn test_is_numeric_with_valid_digits() {
        // Test with '0'
        assert!(AtlasScientific::is_numeric(b'0', false));
        assert!(AtlasScientific::is_numeric(b'0', true));

        // Test with '5'
        assert!(AtlasScientific::is_numeric(b'5', false));
        assert!(AtlasScientific::is_numeric(b'5', true));

        // Test with '9'
        assert!(AtlasScientific::is_numeric(b'9', false));
        assert!(AtlasScientific::is_numeric(b'9', true));
    }

    #[test]
    // Test case checks if the is_numeric function correctly identifies the decimal point.
    fn test_is_numeric_with_decimal_point() {
        assert!(AtlasScientific::is_numeric(b'.', false));
        assert!(AtlasScientific::is_numeric(b'.', true));
    }

    #[test]
    // Test case checks if the is_numeric function correctly handles the negative sign
    // when negative values are allowed.
    fn test_is_numeric_with_negative_sign_allowed() {
        assert!(AtlasScientific::is_numeric(b'-', true));
    }

    #[test]
    // Test case checks if the is_numeric function correctly handles the negative sign
    // when negative values are not allowed (e.g., for pH values).
    fn test_is_numeric_with_negative_sign_not_allowed() {
        assert!(!AtlasScientific::is_numeric(b'-', false));
    }

    #[test]
    // Test case checks if the is_numeric function correctly identifies various
    // invalid characters that are not part of a number.
    fn test_is_numeric_with_invalid_characters() {
        // Test with an alphabet character
        assert!(!AtlasScientific::is_numeric(b'a', false));
        assert!(!AtlasScientific::is_numeric(b'X', true));

        // Test with a special symbol
        assert!(!AtlasScientific::is_numeric(b'?', false));
        assert!(!AtlasScientific::is_numeric(b'#', true));

        // Test with a space
        assert!(!AtlasScientific::is_numeric(b' ', false));
        assert!(!AtlasScientific::is_numeric(b' ', true));

        // Test with a newline character
        assert!(!AtlasScientific::is_numeric(b'\n', false));
        assert!(!AtlasScientific::is_numeric(b'\n', true));
    }

    #[test]
    // Test case checks if send_request_to_i2c_interface correctly sends a request for pH.
    fn test_send_request_to_i2c_interface_ph_success() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let sleep_time_millis_ph = config.atlas_scientific.sleep_time_millis_ph;
        let address_atlas_scientific_ph = config.atlas_scientific.address_atlas_scientific_ph;
        let mut atlas_scientific = AtlasScientific::new(config.atlas_scientific);

        let (tx, rx): (Sender<I2cRequest>, Receiver<I2cRequest>) = mpsc::channel();

        let result = atlas_scientific.send_request_to_i2c_interface(&AquariumSignal::pH, &tx);

        // Assert that the sending was successful
        assert!(result);

        // Assert that a message was received and it's the correct one
        let received_request = rx.try_recv().unwrap();
        assert_eq!(received_request.i2c_address, address_atlas_scientific_ph);
        assert_eq!(received_request.sleep_time_millis, sleep_time_millis_ph);
        assert_eq!(
            received_request.send_buf,
            vec![atlas_scientific_constants::COMMAND_READ]
        );
        assert_eq!(
            received_request.expected_response_length,
            atlas_scientific_constants::MESSAGE_SIZE
        );
    }

    #[test]
    // Test case checks if send_request_to_i2c_interface correctly sends a request for conductivity.
    fn test_send_request_to_i2c_interface_conductivity_success() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let sleep_time_millis_conductivity = config.atlas_scientific.sleep_time_millis_conductivity;
        let address_atlas_scientific_conductivity = config
            .atlas_scientific
            .address_atlas_scientific_conductivity;
        let mut atlas_scientific = AtlasScientific::new(config.atlas_scientific);

        let (tx, rx): (Sender<I2cRequest>, Receiver<I2cRequest>) = mpsc::channel();

        let result =
            atlas_scientific.send_request_to_i2c_interface(&AquariumSignal::Conductivity, &tx);

        // Assert that the sending was successful
        assert!(result);

        // Assert that a message was received and it's the correct one
        let received_request = rx.try_recv().unwrap();
        assert_eq!(
            received_request.i2c_address,
            address_atlas_scientific_conductivity
        );
        assert_eq!(
            received_request.sleep_time_millis,
            sleep_time_millis_conductivity
        );
        assert_eq!(
            received_request.send_buf,
            vec![atlas_scientific_constants::COMMAND_READ]
        );
        assert_eq!(
            received_request.expected_response_length,
            atlas_scientific_constants::MESSAGE_SIZE
        );
    }

    #[test]
    // Test case checks if send_request_to_i2c_interface correctly sends a request for water temperature.
    fn test_send_request_to_i2c_interface_temperature_success() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let sleep_time_millis_temperature = config.atlas_scientific.sleep_time_millis_temperature;
        let address_atlas_scientific_temperature =
            config.atlas_scientific.address_atlas_scientific_temperature;
        let mut atlas_scientific = AtlasScientific::new(config.atlas_scientific);

        let (tx, rx): (Sender<I2cRequest>, Receiver<I2cRequest>) = mpsc::channel();

        let result =
            atlas_scientific.send_request_to_i2c_interface(&AquariumSignal::WaterTemperature, &tx);

        // Assert that the sending was successful
        assert!(result);

        // Assert that a message was received and it's the correct one
        let received_request = rx.try_recv().unwrap();
        assert_eq!(
            received_request.i2c_address,
            address_atlas_scientific_temperature
        );
        assert_eq!(
            received_request.sleep_time_millis,
            sleep_time_millis_temperature
        );
        assert_eq!(
            received_request.send_buf,
            vec![atlas_scientific_constants::COMMAND_READ]
        );
        assert_eq!(
            received_request.expected_response_length,
            atlas_scientific_constants::MESSAGE_SIZE
        );
    }

    #[test]
    // Test case checks that send_request_to_i2c_interface handles an invalid signal gracefully.
    fn test_send_request_to_i2c_interface_invalid_signal() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let mut atlas_scientific = AtlasScientific::new(config.atlas_scientific);

        let (tx, rx): (Sender<I2cRequest>, Receiver<I2cRequest>) = mpsc::channel();

        // Use a signal not handled by the function's match arms
        let result =
            atlas_scientific.send_request_to_i2c_interface(&AquariumSignal::MockInvalidSignal, &tx);

        // Assert that the function returns false, indicating failure
        assert!(!result);

        // Assert that no message was sent over the channel
        assert!(rx.try_recv().is_err());
    }

    #[test]
    // Test case checks that send_request_to_i2c_interface handles a disconnected channel.
    fn test_send_request_to_i2c_interface_channel_disconnected() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let mut atlas_scientific = AtlasScientific::new(config.atlas_scientific);

        let (tx, rx): (Sender<I2cRequest>, Receiver<I2cRequest>) = mpsc::channel();

        // Drop the receiver to simulate a disconnected channel
        drop(rx);

        // Attempt to send a request
        let result = atlas_scientific.send_request_to_i2c_interface(&AquariumSignal::pH, &tx);

        // Assert that the function returns false, indicating failure
        assert!(!result);

        // Assert that the internal lock flag is set to prevent log spam
        assert!(atlas_scientific.lock_error_channel_send_i2c_interface);
    }

    #[test]
    // Test case checks if write_result_to_mutex correctly writes a successful
    // temperature result to the correct mutex.
    fn test_write_result_to_mutex_temperature_success() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let atlas_scientific = AtlasScientific::new(config.atlas_scientific);

        let initial_temp = AtlasScientificResultData::new(0.0);
        let initial_ph = AtlasScientificResultData::new(0.0);
        let initial_cond = AtlasScientificResultData::new(0.0);

        let mutex_temp = Arc::new(Mutex::new(initial_temp.clone()));
        let mutex_ph = Arc::new(Mutex::new(initial_ph.clone()));
        let mutex_cond = Arc::new(Mutex::new(initial_cond.clone()));

        let new_temp_value = 25.5;
        let new_temp_result = AtlasScientificResultData::new(new_temp_value);

        let before = Instant::now();
        let result = atlas_scientific.write_result_to_mutex(
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
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let atlas_scientific = AtlasScientific::new(config.atlas_scientific);

        let mutex_temp = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_cond = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));

        let new_ph_value = 8.2;
        let new_ph_result = AtlasScientificResultData::new(new_ph_value);

        let before = Instant::now();
        let result = atlas_scientific.write_result_to_mutex(
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
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let atlas_scientific = AtlasScientific::new(config.atlas_scientific);

        let mutex_temp = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_cond = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));

        let new_cond_value = 50000.0;
        let new_cond_result = AtlasScientificResultData::new(new_cond_value);

        let before = Instant::now();
        let result = atlas_scientific.write_result_to_mutex(
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
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let atlas_scientific = AtlasScientific::new(config.atlas_scientific);

        let initial_temp = AtlasScientificResultData::new(1.0);
        let initial_ph = AtlasScientificResultData::new(2.0);
        let initial_cond = AtlasScientificResultData::new(3.0);

        let mutex_temp = Arc::new(Mutex::new(initial_temp.clone()));
        let mutex_ph = Arc::new(Mutex::new(initial_ph.clone()));
        let mutex_cond = Arc::new(Mutex::new(initial_cond.clone()));

        let new_result = AtlasScientificResultData::new(99.0);

        let before = Instant::now();
        let result = atlas_scientific.write_result_to_mutex(
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
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let atlas_scientific = AtlasScientific::new(config.atlas_scientific);

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
        let result = atlas_scientific.write_result_to_mutex(
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
        if let Err(AtlasScientificError::CouldNotLockMutex { signal }) = result {
            assert_eq!(signal, AquariumSignal::pH);
        }
    }

    /// Helper function to set up a standard test environment for receive tests.
    fn setup_receive_test() -> (
        AtlasScientific,
        Sender<I2cResult>,
        Receiver<I2cResult>,
        Arc<Mutex<AtlasScientificResultData>>,
        Arc<Mutex<AtlasScientificResultData>>,
        Arc<Mutex<AtlasScientificResultData>>,
    ) {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let atlas_scientific = AtlasScientific::new(config.atlas_scientific);

        let (tx, rx) = mpsc::channel();

        // Initialize mutexes with a known default value (0.0)
        let mutex_temp = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
        let mutex_cond = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));

        (atlas_scientific, tx, rx, mutex_temp, mutex_ph, mutex_cond)
    }

    /// Helper function to run a successful receive-and-parse test for a given signal.
    fn run_receive_success_test(signal: AquariumSignal, value: f32) {
        let (mut atlas_scientific, tx, rx, mutex_temp, mutex_ph, mutex_cond) = setup_receive_test();

        // Create a valid I2C response buffer without spaces, as per the requirement.
        // The current `check_response` implementation expects a 5-character numeric string
        // from its slice of the buffer. We create a 6-byte payload that satisfies this.
        let mut value_str = value.to_string();
        if value_str.len() > 5 {
            // Truncate if the string representation is too long for the 5-char slice.
            value_str.truncate(5);
        }
        // Right-pad with '0' to a total of 6 characters. The parsing logic will slice the
        // first 5 characters, which will form a valid, non-space-padded float string.
        while value_str.len() < 6 {
            value_str.push('0');
        }
        let response_string = value_str;

        let mut buffer = vec![atlas_scientific_constants::MESSAGE_START_MAGIC];
        buffer.extend_from_slice(response_string.as_bytes());
        buffer.push(atlas_scientific_constants::MESSAGE_FINISH_MAGIC);
        assert_eq!(buffer.len(), 8);

        let i2c_response = I2cResponse {
            read_buf: buffer,
            length: 8,
        };

        // Send the valid response
        tx.send(Ok(i2c_response)).unwrap();

        // Execute the function under test
        let result = atlas_scientific.receive_response_from_i2c_interface(
            &signal,
            &rx,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );

        // Assert the outcome
        assert!(
            result.is_ok(),
            "Test failed for signal {:?} with value {}. Error: {:?}",
            signal,
            value,
            result.err()
        );
        assert_float_absolute_eq!(result.unwrap().value, value, 0.001);

        // Check that the correct mutex was updated and others were not
        let (target_mutex, other1, other2) = match signal {
            AquariumSignal::WaterTemperature => (&mutex_temp, &mutex_ph, &mutex_cond),
            AquariumSignal::pH => (&mutex_ph, &mutex_temp, &mutex_cond),
            AquariumSignal::Conductivity => (&mutex_cond, &mutex_temp, &mutex_ph),
            _ => panic!("Invalid signal for this test"),
        };

        let lock = target_mutex.lock().unwrap();
        assert_float_absolute_eq!(lock.value, value, 0.001);

        let lock1 = other1.lock().unwrap();
        assert_float_absolute_eq!(lock1.value, 0.0, 0.001);
        let lock2 = other2.lock().unwrap();
        assert_float_absolute_eq!(lock2.value, 0.0, 0.001);
    }
    
    #[test]
    /// Tests the happy path for all three Atlas Scientific signals.
    fn test_receive_response_success_all_signals() {
        run_receive_success_test(AquariumSignal::WaterTemperature, 25.5);
        run_receive_success_test(AquariumSignal::pH, 8.2);
        run_receive_success_test(AquariumSignal::Conductivity, 51000.0);
    }
    
    #[test]
    /// Tests that the function correctly handles an I2C communication failure from the channel.
    fn test_receive_response_i2c_failure() {
        let (mut atlas_scientific, tx, rx, mutex_temp, mutex_ph, mutex_cond) = setup_receive_test();

        // Send an I2C error over the channel.
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        let simulated_io_error = io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Simulated hardware permission error",
        );

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        let mock_rppal_error = rppal::i2c::Error::Io(simulated_io_error);

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        let mock_error = I2cError::WriteFailure {
            source: mock_rppal_error,
        };

        #[cfg(not(any(target_os = "linux", feature = "target_hw")))]
        let mock_error = I2cError::WriteFailure;

        // for testing on Linux server (without target hardware)
        #[cfg(all(target_os = "linux", not(feature = "target_hw")))]
        let mock_error = I2cError::WriteFailure;
        
        let _ = tx.send(Err(mock_error)).unwrap();

        let result = atlas_scientific.receive_response_from_i2c_interface(
            &AquariumSignal::pH,
            &rx,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );

        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(AtlasScientificError::I2cCommunicationFailure { .. })
        ));
    }
    
    #[test]
    /// Tests that the function returns the correct error when the channel is empty.
    fn test_receive_response_channel_empty() {
        let (mut atlas_scientific, _tx, rx, mutex_temp, mutex_ph, mutex_cond) =
            setup_receive_test();

        let result = atlas_scientific.receive_response_from_i2c_interface(
            &AquariumSignal::pH,
            &rx,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );

        assert!(matches!(result, Err(AtlasScientificError::ChannelIsEmpty)));
    }

    #[test]
    /// Tests that the function returns the correct error when the channel is disconnected.
    fn test_receive_response_channel_disconnected() {
        let (mut atlas_scientific, tx, rx, mutex_temp, mutex_ph, mutex_cond) = setup_receive_test();

        drop(tx); // Disconnect the channel

        let result = atlas_scientific.receive_response_from_i2c_interface(
            &AquariumSignal::pH,
            &rx,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );

        assert!(matches!(
            result,
            Err(AtlasScientificError::ChannelIsDisconnected { .. })
        ));
    }

    #[test]
    /// Tests that a long wait for a mutex lock triggers the internal warning flag.
    fn test_receive_response_long_mutex_wait_triggers_warning() {
        let (mut atlas_scientific, tx, rx, mutex_temp, mutex_ph, mutex_cond) = setup_receive_test();

        // Spawn a thread that will hold the lock for a while, simulating a slow consumer.
        let mutex_temp_clone = mutex_temp.clone();
        let handle = thread::spawn(move || {
            let _lock = mutex_temp_clone.lock().unwrap();
            let spin_sleeper = SpinSleeper::default();
            spin_sleeper.sleep(Duration::from_millis(
                atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS + 5,
            ));
            // Lock is released when the thread finishes.
        });

        // Give the spawned thread a moment to acquire the lock.
        let spin_sleeper = SpinSleeper::default();
        spin_sleeper.sleep(Duration::from_millis(5));

        // Send a valid response that will be processed.
        let i2c_response = I2cResponse {
            read_buf: vec![1, b'2', b'5', b'.', b'0', b'0', b'0', 0],
            length: 8,
        };
        tx.send(Ok(i2c_response)).unwrap();

        // This call will now block until the spawned thread releases the lock.
        // The duration of this block will be longer than the threshold.
        let result = atlas_scientific.receive_response_from_i2c_interface(
            &AquariumSignal::WaterTemperature,
            &rx,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );

        // Wait for the spawned thread to complete.
        handle.join().unwrap();

        // Assert that the operation was ultimately successful.
        assert!(result.is_ok());

        // Assert that the warning flag was set because of the long wait.
        assert!(atlas_scientific.mutex_temperature_access_duration_exceeded);
    }

    #[test]
    /// Tests that the function handles an unsupported signal without altering any state.
    fn test_receive_response_with_unsupported_signal() {
        let (mut atlas_scientific, tx, rx, mutex_temp, mutex_ph, mutex_cond) = setup_receive_test();

        // Store initial values of the mutexes.
        let initial_temp_val = mutex_temp.lock().unwrap().value;
        let initial_ph_val = mutex_ph.lock().unwrap().value;
        let initial_cond_val = mutex_cond.lock().unwrap().value;

        // Send a valid response.
        let i2c_response = I2cResponse {
            read_buf: vec![1, b'9', b'9', b'.', b'0', b'0', b'0', 0],
            length: 8,
        };
        tx.send(Ok(i2c_response)).unwrap();

        // Execute the function with a signal that is not handled by `write_result_to_mutex`.
        let result = atlas_scientific.receive_response_from_i2c_interface(
            &AquariumSignal::MockInvalidSignal,
            &rx,
            &mutex_temp,
            &mutex_ph,
            &mutex_cond,
        );

        // The function should still "succeed" in processing the message.
        assert!(result.is_ok());
        assert_float_absolute_eq!(result.unwrap().value, 99.0, 0.001);

        // Assert that none of the mutexes were updated.
        let final_temp_val = mutex_temp.lock().unwrap().value;
        let final_ph_val = mutex_ph.lock().unwrap().value;
        let final_cond_val = mutex_cond.lock().unwrap().value;

        assert_eq!(initial_temp_val, final_temp_val);
        assert_eq!(initial_ph_val, final_ph_val);
        assert_eq!(initial_cond_val, final_cond_val);
    }

    #[test]
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[ignore]
    // read from real Atlas Scientific hardware combined in one test case to avoid race conditions:
    // - pH sensor
    // - conductivity sensor
    // - temperature
    pub fn test_atlas_scientific_interface_read_sensors() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        let mut i2c_interface = I2cInterface::new(config.i2c_interface);

        let send_buf = vec![atlas_scientific_constants::COMMAND_READ];
        let request_ph = I2cRequest::new(
            config.atlas_scientific.address_atlas_scientific_ph,
            send_buf.clone(),
            config.atlas_scientific.sleep_time_millis_ph,
            atlas_scientific_constants::MESSAGE_SIZE,
        );
        let request_temperature = I2cRequest::new(
            config.atlas_scientific.address_atlas_scientific_temperature,
            send_buf.clone(),
            config.atlas_scientific.sleep_time_millis_temperature,
            atlas_scientific_constants::MESSAGE_SIZE,
        );
        let request_conductivity = I2cRequest::new(
            config
                .atlas_scientific
                .address_atlas_scientific_conductivity,
            send_buf,
            config.atlas_scientific.sleep_time_millis_conductivity,
            atlas_scientific_constants::MESSAGE_SIZE,
        );

        let response_ph = i2c_interface.get_response_from_i2c(request_ph);
        println!("Atlas Scientific response for pH: {:?}", response_ph);

        let response_temperature = i2c_interface.get_response_from_i2c(request_temperature);
        println!(
            "Atlas Scientific response for temperature: {:?}",
            response_temperature
        );

        let response_conductivity = i2c_interface.get_response_from_i2c(request_conductivity);
        println!(
            "Atlas Scientific response for conductivity: {:?}",
            response_conductivity
        );

        let ph = AtlasScientific::check_response(
            response_ph.unwrap().read_buf.try_into().unwrap(),
            true,
        )
        .unwrap();
        println!("pH={}", ph);
        let temperature = AtlasScientific::check_response(
            response_temperature.unwrap().read_buf.try_into().unwrap(),
            true,
        )
        .unwrap();
        println!("temperature={}", temperature);
        let conductivity = AtlasScientific::check_response(
            response_conductivity.unwrap().read_buf.try_into().unwrap(),
            true,
        )
        .unwrap();
        println!("conductivity={}", conductivity);
    }
}
