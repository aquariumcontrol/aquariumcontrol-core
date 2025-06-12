/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#![allow(non_snake_case)]

use log::{error, warn};
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::sensors::i2c_interface::{I2cRequest, I2cResult};
use crate::simulator::get_resp_sim::GetResponseFromSimulatorTrait;
use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

const LOG_TARGET_ATLAS_SCIENTIFIC: &str = "atlas_scientific";

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
    address_atlas_scientific_ph: u16,

    /// I2C address of Atlas Scientific conductivity sensor unit
    #[allow(unused)]
    address_atlas_scientific_conductivity: u16,

    /// I2C address of Atlas Scientific temperature sensor unit    
    #[allow(unused)]
    address_atlas_scientific_temperature: u16,

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

/// error codes for evaluation of sensor unit response
#[allow(dead_code)]
#[derive(Debug, PartialEq)]
pub enum AtlasScientificError {
    /// The first character in the message sent from the sensor does not match the required magic number.
    FirstCharacterNotMagic,

    /// The last character in the message sent from the sensor does not match the required magic number.
    LastCharacterNotMagic,

    /// The message sent from the sensor unit contains non-numeric characters in the data section
    ContainsInvalidCharacter(u8),

    /// The error is masked by other checks. Still, the signature of the function to convert from u8 array slice to String throws an error.
    InvalidUtf8Sequence,

    /// Conversion from String to f32 failed.
    ConversionFailure,

    /// Incorrect buffer length
    IncorrectBufferLength,
}

pub struct AtlasScientificResultData {
    pub value: f32,

    #[allow(unused)]
    invalid: bool,

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
    // configuration data for AtlasScientific
    config: AtlasScientificConfig,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    lock_error_channel_receive_termination: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via the channel from the signal handler thread
    lock_warn_inapplicable_command_signal_handler: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about to send request via the channel to I2cInterface thread
    lock_error_channel_send_i2c_interface: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive via the channel from I2cInterface thread
    lock_error_channel_receive_i2c_interface: bool,

    pub last_measurement_instant: Instant,

    pub measurement_interval: Duration,
}
impl ProcessExternalRequestTrait for AtlasScientific {}

impl GetResponseFromSimulatorTrait for AtlasScientific {}

impl AtlasScientific {
    /// Creates a new `AtlasScientific` control instance.
    ///
    /// This constructor initializes the Atlas Scientific sensor module. It sets initial
    /// sensor values to a default `0.0` and marks them as invalid until actual readings
    /// are obtained. All internal "lock" flags are also initialized to `false` to
    /// prevent log flooding from recurring errors or warnings.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the Atlas Scientific module, specifying behavior
    ///   like the use of a simulator versus hardware, and replacement values for sensor readings.
    ///
    /// # Returns
    /// A new `AtlasScientific` struct, ready to collect and provide sensor data.
    pub fn new(config: AtlasScientificConfig) -> AtlasScientific {
        let measurement_interval = Duration::from_millis(config.measurement_interval_millis);

        AtlasScientific {
            config,
            lock_error_channel_receive_termination: false,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_send_i2c_interface: false,
            lock_error_channel_receive_i2c_interface: false,
            last_measurement_instant: Instant::now(),
            measurement_interval,
        }
    }

    #[allow(dead_code)]
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

    /// Outputs the ASCII (byte) values of a `[u8]` slice to the console.
    ///
    /// Each byte's integer value, representing its ASCII code, is printed,
    /// separated by spaces.
    ///
    /// # Arguments
    ///
    /// * `data` - A slice of `u8` (bytes) whose ASCII values are to be printed.
    pub fn print_ascii_values(data: &[u8]) {
        // Iterate over each byte in the input slice.
        for (i, &byte) in data.iter().enumerate() {
            // Print the byte's integer value (its ASCII value).
            // Use space as a separator for all but the last byte.
            if i > 0 {
                print!(" ");
            }
            print!("{}", byte);
        }
        // Print a newline character at the end to ensure the next output appears on a new line.
        println!();
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
        if buffer[atlas_scientific_constants::INDEX_FIRST_CHARACTER]
            != atlas_scientific_constants::MESSAGE_START_MAGIC
        {
            return Err(AtlasScientificError::FirstCharacterNotMagic);
        }
        // second check: last character not magic
        if buffer[atlas_scientific_constants::INDEX_LAST_CHARACTER]
            != atlas_scientific_constants::MESSAGE_FINISH_MAGIC
        {
            return Err(AtlasScientificError::LastCharacterNotMagic);
        }

        // get a slice of the array
        let numeric_content = &buffer[atlas_scientific_constants::INDEX_CONTENT_START
            ..atlas_scientific_constants::INDEX_PENULTIMATE_CHARACTER];

        // third check: all characters in between belong to a number
        for character in numeric_content {
            if !Self::is_numeric(*character, !is_ph) {
                println!("Encountered non-numeric character {} in sequence: ", character);
                Self::print_ascii_values(numeric_content);
                return Err(AtlasScientificError::ContainsInvalidCharacter(character.clone()));
            }
        }

        // conversion to String
        let numeric_string = match str::from_utf8(numeric_content) {
            Ok(c) => c,
            Err(_) => return Err(AtlasScientificError::InvalidUtf8Sequence),
        };

        // conversion to f32
        match &numeric_string.parse::<f32>() {
            Ok(c) => Ok(*c),
            Err(_) => Err(AtlasScientificError::ConversionFailure),
        }
    }

    /// Sends a request to the I2C interface thread to perform a new measurement from Atlas Scientific sensors.
    ///
    /// It sends an `InternalCommand::RequestSignal`
    /// for `AquariumSignal::WaterTemperature` (which implicitly triggers the reading of
    /// temperature, pH, and conductivity from Atlas Scientific sensors) to the I2CInterface thread.
    ///
    /// It updates an internal lock flag (`lock_error_channel_send_i2c_interface`) to prevent
    /// repeated error logging if the channel operation fails.
    ///
    /// # Arguments
    /// * `tx_atlas_scientific_to_i2c_interface` - A reference to the sender part of the channel, used to
    ///   send commands to the I2CInterface thread.
    ///
    /// # Returns
    /// `true` if the measurement request was successfully sent; `false` if the channel operation failed.
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
                warn!(
                    target: LOG_TARGET_ATLAS_SCIENTIFIC,
                    "ignoring invalid signal request ({})",
                    signal
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
                        target: LOG_TARGET_ATLAS_SCIENTIFIC,
                        "sending request to I2cInterface thread failed. ({e:?})"
                    );
                    self.lock_error_channel_send_i2c_interface = true;
                }
                false
            }
        }
    }

    /// Receives and processes measurement responses from the I2C interface thread for Atlas Scientific sensors.
    ///
    /// It attempts to receive a `Result` without blocking containing
    /// water temperature, pH, and conductivity data from the `I2CInterface` thread.
    ///
    /// If successful, it updates the module's internal sensor fields (`water_temperature`, `pH`, `conductivity`)
    /// and resets their `_invalid` flags to `false`. It manages various lock flags (`lock_error_i2c_interface`,
    /// `lock_error_channel_receive_i2c_interface`) to prevent log flooding from sensor reading errors
    /// or channel disconnections.
    ///
    /// # Arguments
    /// * `rx_atlas_scientific_from_i2c_interface` - A reference to the receiver part of the channel,
    ///   used to get measurement results from the `I2CInterface` thread.
    ///
    /// # Returns
    /// `true` if a response was expected but not yet received (meaning `receive_response_from_i2c_interface`
    /// should be called again soon, e.g., on the next loop iteration); `false` if a response
    /// was successfully received and processed, or if a non-empty error occurred.
    fn receive_response_from_i2c_interface(
        &mut self,
        signal: &AquariumSignal,
        rx_atlas_scientific_from_i2c_interface: &Receiver<I2cResult>,
        mutex_water_temperature: &Arc<Mutex<AtlasScientificResult>>,
        mutex_ph: &Arc<Mutex<AtlasScientificResult>>,
        mutex_conductivity: &Arc<Mutex<AtlasScientificResult>>,
    ) -> bool {
        let is_ph = matches!(signal, AquariumSignal::pH);

        match rx_atlas_scientific_from_i2c_interface.try_recv() {
            Ok(i2c_result) => match i2c_result {
                // Result from channel is available
                Ok(i2c_response) => {
                    let result: AtlasScientificResult = match i2c_response.read_buf.try_into() {
                        Ok(response_buffer) =>
                        // type for result is defined by parameter definition of check_response
                        {
                            match Self::check_response(response_buffer, is_ph) {
                                Ok(value) => Ok(AtlasScientificResultData::new(value)),
                                Err(e) => {
                                    error!(
                                        target: LOG_TARGET_ATLAS_SCIENTIFIC,
                                        "error in sensor unit response for {}: {:?}",
                                        signal,
                                        e
                                    );
                                    Err(e)
                                }
                            }
                        }
                        Err(_) => {
                            error!(target: LOG_TARGET_ATLAS_SCIENTIFIC, "Could not convert response buffer.");
                            Err(AtlasScientificError::IncorrectBufferLength)
                        }
                    };

                    match signal {
                        AquariumSignal::WaterTemperature => match mutex_water_temperature.lock() {
                            Ok(mut water_temperature) => *water_temperature = result,
                            Err(_) => {
                                error!("Could not lock mutex_conductivity.");
                                return false;
                            }
                        },
                        AquariumSignal::pH => match mutex_ph.lock() {
                            Ok(mut ph) => *ph = result,
                            Err(_) => {
                                error!("Could not lock mutex_ph.");
                                return false;
                            }
                        },
                        AquariumSignal::Conductivity => match mutex_conductivity.lock() {
                            Ok(mut conductivity) => *conductivity = result,
                            Err(_) => {
                                error!("Could not lock mutex_conductivity.");
                                return false;
                            }
                        },
                        _ => { /* do nothing */ }
                    }

                    false // allow for the next request to be sent
                }
                Err(_) => false,
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => {
                        /* empty buffer - no action required */
                        self.lock_error_channel_receive_i2c_interface = false;
                    }
                    TryRecvError::Disconnected => {
                        if !self.lock_error_channel_receive_i2c_interface {
                            error!(
                                target: LOG_TARGET_ATLAS_SCIENTIFIC,
                                "error occurred when trying to receive response from I2cInterface ({e:?})"
                            );
                            self.lock_error_channel_receive_i2c_interface = true;
                        }
                    }
                }
                true // repeat receiving
            }
        }
    }

    /// Executes the main control loop for the Atlas Scientific sensor module.
    ///
    /// This function runs continuously, managing the periodic measurement of
    /// water temperature, pH, and conductivity data from the AtlasScientific sensor unit.
    /// It interacts directly with an I2C interface thread, sending requests and receiving actual sensor readings.
    ///
    /// The collected data is then written to shared mutexes, making it available to other
    /// application threads. The loop maintains a measurement interval and remains
    /// responsive to `Quit` and `Terminate` commands from the signal handler for graceful shutdown.
    ///
    /// # Arguments
    /// * `atlas_scientific_channels` - A struct containing all `mpsc` channels necessary for inter-thread
    ///   communication with the `I2CInterface` and the `SignalHandler`.
    /// * `mutex_atlas_scientific_water_temperature` - An `Arc<Mutex<f32>>` into which the latest water temperature
    ///   reading will be written, ensuring thread-safe access for other modules.
    /// * `mutex_atlas_scientific_ph` - An `Arc<Mutex<f32>>` into which the latest pH reading will be written,
    ///   ensuring thread-safe access for other modules.
    /// * `mutex_atlas_scientific_conductivity` - An `Arc<Mutex<f32>>` into which the latest conductivity reading
    ///   will be written, ensuring thread-safe access for other modules.
    ///
    /// # Panics
    /// This function will panic if:
    /// - It's configured to use the simulator, but essential TCP communication channels are missing.
    /// - During hardware mode, a fatal error occurs while trying to receive a response from the I2C thread (e.g., a channel disconnected permanently).
    pub fn execute(
        &mut self,
        atlas_scientific_channels: AtlasScientificChannels,
        mutex_atlas_scientific_water_temperature: Arc<Mutex<AtlasScientificResult>>,
        mutex_atlas_scientific_ph: Arc<Mutex<AtlasScientificResult>>,
        mutex_atlas_scientific_conductivity: Arc<Mutex<AtlasScientificResult>>,
    ) {
        let sleep_duration_ten_millis = Duration::from_millis(10);
        let sleep_duration_one_millis = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        let mut state_send_receive: bool = false;
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
            if self.config.active {
                if !state_send_receive
                    && Instant::now().duration_since(self.last_measurement_instant)
                        > self.measurement_interval
                {
                    state_send_receive = self.send_request_to_i2c_interface(
                        &signal,
                        &atlas_scientific_channels.tx_atlas_scientific_to_i2c_interface,
                    );
                } else if state_send_receive {
                    // read from the channel if the measurement result is available from I2cInterface thread
                    state_send_receive = self.receive_response_from_i2c_interface(
                        &signal,
                        &atlas_scientific_channels.rx_atlas_scientific_from_i2c_interface,
                        &mutex_atlas_scientific_water_temperature,
                        &mutex_atlas_scientific_ph,
                        &mutex_atlas_scientific_conductivity,
                    );

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
                    target: LOG_TARGET_ATLAS_SCIENTIFIC,
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
                                target: LOG_TARGET_ATLAS_SCIENTIFIC,
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
                            target: LOG_TARGET_ATLAS_SCIENTIFIC,
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
    use crate::sensors::atlas_scientific::{
        atlas_scientific_constants, AtlasScientific, AtlasScientificError,
    };
    use crate::sensors::i2c_interface::{I2cError, I2cRequest, I2cResponse};
    use crate::utilities::channel_content::AquariumSignal;
    use crate::utilities::config::{read_config_file, ConfigData};
    use assert_float_eq::*;
    use std::sync::mpsc;
    use std::sync::mpsc::{Receiver, Sender};

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
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '-' as u8,
            '8' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        assert_eq!(
            AtlasScientific::check_response(stimuli, true),
            Err(AtlasScientificError::ContainsInvalidCharacter('-' as u8))
        );
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
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC - 1,
            '2' as u8,
            '5' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC,
        ];
        assert_eq!(
            AtlasScientific::check_response(stimuli, false),
            Err(AtlasScientificError::FirstCharacterNotMagic)
        );
    }

    #[test]
    // checking if an incorrect magic number in the rear part of the message is identified correctly
    pub fn test_atlas_scientific_check_response_invalid_magic_rear() {
        let stimuli: [u8; atlas_scientific_constants::MESSAGE_SIZE] = [
            atlas_scientific_constants::MESSAGE_START_MAGIC,
            '2' as u8,
            '5' as u8,
            atlas_scientific_constants::DECIMAL_POINT,
            '0' as u8,
            '0' as u8,
            '0' as u8,
            atlas_scientific_constants::MESSAGE_FINISH_MAGIC + 1,
        ];
        assert_eq!(
            AtlasScientific::check_response(stimuli, false),
            Err(AtlasScientificError::LastCharacterNotMagic)
        );
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
        assert_eq!(
            AtlasScientific::check_response(stimuli, false),
            Err(AtlasScientificError::ConversionFailure)
        );
    }

    #[test]
    // Test the error response of get_response_from_i2c when requesting an invalid signal.
    pub fn test_atlas_scientific_error_response_to_invalid_signal_request() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
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
