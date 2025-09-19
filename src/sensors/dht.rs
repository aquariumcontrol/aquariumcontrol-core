/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Implements the sensor management thread for a DHT22 temperature and humidity sensor.
//!
//! This module provides the `Dht` struct, which runs as a dedicated, long-running
//! thread. Its primary responsibility is to periodically read ambient temperature and
//! humidity from a DHT22 sensor connected via GPIO.
//!
//! The implementation handles the entire, highly timing-sensitive communication protocol,
//! from sending the start signal to parsing the response pulses, validating the checksum,
//! and calculating the final floating-point values. The latest valid measurement is made
//! available to the rest of the application through a shared `Arc<Mutex<...>>`.
//!
//! ## Key Components
//!
//! - **`Dht` Struct**: The core state machine for the thread. It holds the configuration,
//!   GPIO pins, and internal state like the last measurement time and log
//!   suppression flags.
//!
//! - **`execute()` Method**: The main thread loop. It periodically triggers a sensor
//!   reading based on the configured `measurement_interval`, writes the result to the
//!   shared mutex, and handles graceful shutdown commands.
//!
//! - **`get_data()` Method**: A robust, high-level read function that implements a retry
//!   mechanism. It calls the low-level `read()` function multiple times if recoverable
//!   errors (like a checksum mismatch or timeout) occur.
//!
//! - **`read()` Method**: The low-level, hardware-facing function that implements the
//!   DHT22 protocol. It is conditionally compiled to only run on a Linux target with
//!   the `target_hw` feature enabled. It uses `rppal::gpio` to manipulate the data pin
//!   and `set_interrupt` to capture the precise timing of the response pulses.
//!
//! - **Protocol Parsing Helpers**: A series of pure functions (`calc_data_bits`,
//!   `calc_byte_array`, `calc_checksum`, `calc_signals`) that are responsible for
//!   translating the raw pulse timings into a final, validated temperature and humidity
//!   reading.
//!
//! ## Design and Architecture
//!
//! This module is designed as a self-contained, resilient hardware driver.
//!
//! - **Dedicated Thread for Timing-Critical Work**: The DHT22 protocol requires microsecond-level
//!   timing precision. Running this logic in a dedicated thread isolates it from other
//!   application tasks, ensuring that its timing is not disrupted.
//!
//! - **Hardware Abstraction (via Conditional Compilation)**: The `read()` function has two
//!   implementations. On the target hardware, it uses the `rppal` GPIO library. On all
//!   other platforms (like a developer's machine), it uses a stub that immediately
//!   returns an `IncorrectPlatform` error. This allows the entire application to be
//!   compiled and tested without needing physical hardware.
//!
//! - **Shared Data via Mutex**: The module's "output" is a shared `Arc<Mutex<DhtResult>>`.
//!   This is a classic concurrent pattern that allows other threads (like the `DataLogger`)
//!   to safely access the latest sensor readings at any time without direct channel
//!   communication.
//!
//! - **Robustness through Retries**: The `get_data()` function makes the driver resilient
//!   to the transient communication glitches common with DHT sensors by automatically
//!   retrying failed read attempts.
//!
//! ### DHT22 Communication Protocol
//!
//! The `read()` function and its helpers implement the following protocol:
//! 1.  **Start Signal**: The host pulls the data line LOW for ~1ms, then HIGH for ~40µs.
//! 2.  **Sensor Response**: The sensor pulls the line LOW then HIGH to acknowledge.
//! 3.  **Data Transmission**: The sensor sends 40 bits of data. Each bit starts with a
//!     ~50µs LOW pulse. The length of the following HIGH pulse determines the bit's value
//!     (~26-28µs for a '0', ~70µs for a '1').
//! 4.  **Data Format**: The 40 bits represent 5 bytes: 2 bytes for humidity, 2 for
//!     temperature, and 1 for a checksum.
//! 5.  **Validation**: The checksum (sum of the first 4 bytes, modulo 256) is validated
//!     to ensure data integrity.

#[cfg(all(not(test), target_os = "linux"))]
use log::info;

use crate::sensors::dht_error::DhtError;

#[cfg(all(not(test), target_os = "linux", feature = "target_hw"))]
use crate::utilities::logger::log_error_chain;

#[cfg(all(not(test), target_os = "linux"))]
use nix::unistd::gettid;
use std::time::Duration;
cfg_if::cfg_if! {
    if #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))] {
        use crate::sensors::dht_channels::DhtChannels;
        use crate::sensors::dht_config::DhtConfig;
        use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
        use crate::utilities::check_mutex_access_duration::CheckMutexAccessDurationTrait;
        use std::time::Instant;
        use spin_sleep::SpinSleeper;
        use std::sync::{Arc, Mutex};
        use log::error;
    }
}

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
        use rppal::gpio::{Gpio, Level, Trigger};
        use std::thread::{self, sleep};
        use crate::sensors::gpio_handler::Gpio;

        #[cfg(feature = "debug_dht")]
        use log::debug;
    }
    else {
    }
}

#[cfg(not(all(target_os = "linux", feature = "target_hw")))]
use crate::sensors::gpio_handler::Gpio;

/// Type definition to avoid linter warnings about complex return types
pub type DhtResult = Result<(f32, f32), DhtError>; // Temperature and Humidity

#[allow(unused)]
/// allow max. 10 milliseconds for mutex to be blocked by any other thread
const MAX_MUTEX_ACCESS_DURATION_MILLIS: u64 = 10;

/// Contains constants for communication with the DHT22 sensor.
pub mod dht22_constants {
    pub const DHT22_DATA_LEN: usize = 5;
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub const DHT22_BYTE0_START_IDX: usize = 1;
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub const DHT22_BYTE0_STOP_IDX: usize = 8;
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub const DHT22_BYTE1_START_IDX: usize = 9;
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub const DHT22_BYTE1_STOP_IDX: usize = 16;
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub const DHT22_BYTE2_START_IDX: usize = 17;
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub const DHT22_BYTE2_STOP_IDX: usize = 23;
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub const DHT22_BYTE3_START_IDX: usize = 24;
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub const DHT22_BYTE3_STOP_IDX: usize = 31;
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub const DHT22_BYTE4_START_IDX: usize = 32;
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub const DHT22_BYTE4_STOP_IDX: usize = 39;

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub const DHT22_TRANSMISSION_END_IDX: usize = 86;

    // minimum and maximum value for temperature10 range checks
    pub const DHT22_MINVAL_TEMP10: u32 = 0;
    pub const DHT22_MAXVAL_TEMP10: u32 = 500;

    // minimum and maximum value for temperature10 range checks
    pub const DHT22_MINVAL_HUM10: u32 = 0;
    pub const DHT22_MAXVAL_HUM10: u32 = 1000;

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    // timing of (neutral) low phase between data bit
    pub const DHT22_NEUTRAL_LEN_MIN: u64 = 30;

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    pub const DHT22_NEUTRAL_LEN_MAX: u64 = 60;

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    // distinguish between true and false
    pub const DHT22_BIT_TIMING_LIMIT: u64 = 50;

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    // maximum number of allowed retries to get valid data
    pub const DHT22_MAX_RETRIES: u64 = 5;

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// allow max. 10 milliseconds for mutex to be blocked by any other thread
    pub const MAX_MUTEX_ACCESS_DURATION_MILLIS: u64 = 10;
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Represents a Digital Humidity and Temperature (DHT) sensor module, typically a DHT22,
/// responsible for reading ambient temperature and humidity.
///
/// This struct encapsulates the configuration, GPIO interface details, and internal state
/// required to communicate with a DHT sensor. It handles the low-level GPIO signaling
/// to read data, apply retries, and perform post-processing (checksum validation,
/// signal calculation) before providing the final temperature and humidity values.
///
/// The communication with the DHT sensor is highly timing-sensitive and involves
/// precise GPIO manipulation, including setting pin states, managing delays, and
/// capturing pulse durations via interrupts.
///
/// Thread Communication:
/// This module is designed to run in its own dedicated thread, periodically reading sensor data
/// and making the latest measurements available via a shared `Arc<Mutex<DhtResult>>`.
/// Other modules (like `SensorManager` or `DataLogger`) can then read from this mutex to get the
/// current temperature and humidity. It also responds to `Quit` commands for graceful shutdown.
///
/// Platform-Specific Behavior:
/// The core sensor reading logic (`read` function) is conditionally compiled:
/// - On Linux with the `target_hw` feature enabled, it directly uses `rppal::gpio` for hardware interaction.
/// - On other platforms or when `target_hw` is disabled, it uses a stub implementation (`Err(DhtError::IncorrectPlatform)`)
///   to allow compilation and testing without physical hardware.
///
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     dht[Dht] -.-> SensorManager[Sensor Managerr]
///     signal_handler[SignalHandler] --> dht
/// ```
pub struct Dht {
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// configuration for communication with DHT sensor
    config: DhtConfig,

    #[allow(unused)]
    /// GPIO interface
    gpio_opt: Option<Gpio>,

    #[allow(unused)]
    /// numeric GPIO pin for communication with DHT22 sensor
    gpio_dht22_io: u8,

    #[allow(unused)]
    /// numeric GPIO pin for providing power to the DHT22 sensor
    gpio_dht22_vcc: u8,

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// time instance of the last recording
    instant_last_measurement: Instant,

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// flag to avoid flooding the log file in case mutex cannot be locked
    lock_error_mutex: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex
    pub lock_warn_max_mutex_access_duration: bool,

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read from DHT sensor
    lock_error_failed_after_retries: bool,

    // for testing purposes: record when mutex access time is exceeded without resetting it
    #[cfg(test)]
    pub mutex_access_duration_exceeded: bool,

    /// Maximum permissible access duration for Mutex
    pub max_mutex_access_duration: Duration,

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// Duration between measurements
    pub measurement_interval: Duration,
}

impl Dht {
    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// Creates a new **`Dht`** instance for interacting with a DHT22 sensor on the target platform.
    ///
    /// This constructor initializes the DHT sensor module, configuring it with the provided
    /// settings and gaining access to the necessary GPIO pins for communication and power supply
    /// to the DHT22 sensor. This specific implementation is enabled only for Linux targets
    /// when the `target_hw` feature is active.
    ///
    /// # Arguments
    /// * `config` - **Configuration data** for the DHT sensor, including timing parameters for communication.
    /// * `gpio_opt` - A **`Gpio` instance** from `rppal`, providing the interface to the system's GPIO pins, wrapped in Option.
    /// * `gpio_dht22_io` - The **GPIO pin** used for data input/output with the DHT22 sensor.
    /// * `gpio_dht22_vcc` - The **GPIO pin** used to control the power supply (VCC) to the DHT22 sensor.
    ///
    /// # Returns
    /// A new **`Dht` struct**, ready to read temperature and humidity.
    pub fn new(
        config: DhtConfig,
        gpio_opt: Option<Gpio>,
        gpio_dht22_io: u8,
        gpio_dht22_vcc: u8,
    ) -> Dht {
        let measurement_interval = Duration::from_millis(config.measurement_interval_millis);
        Dht {
            config,
            gpio_opt,
            gpio_dht22_io,
            gpio_dht22_vcc,
            instant_last_measurement: Instant::now(),
            lock_error_mutex: false,
            lock_warn_max_mutex_access_duration: false,
            lock_error_failed_after_retries: false,

            #[cfg(test)]
            mutex_access_duration_exceeded: false,
            max_mutex_access_duration: Duration::from_millis(MAX_MUTEX_ACCESS_DURATION_MILLIS),
            measurement_interval,
        }
    }

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// Calculates the raw data bits from the timing sequence of GPIO signal changes during DHT22 sensor communication.
    ///
    /// This private helper function interprets the pulse widths measured from the DHT22 sensor.
    /// It differentiates between "neutral" pauses (which are ignored) and actual data bits
    /// (distinguished by their duration relative to `DHT22_BIT_TIMING_LIMIT`).
    /// Longer pulses are interpreted as `true` bits, shorter as `false` bits.
    ///
    /// # Arguments
    /// * `ticks` - An array of `u64` values, where each value represents the duration (in microseconds)
    ///   of a specific pulse or pause observed on the data line.
    /// * `valid_ticks_received` - The actual number of valid (non-timeout) interrupt events captured,
    ///   indicating how many entries in `ticks` are relevant.
    ///
    /// # Returns
    /// A fixed-size array of `bool` representing the extracted data bits from the sensor.
    /// This array contains `true` for a '1' bit and `false` for a '0' bit.
    pub fn calc_data_bits(
        ticks: [u64; dht22_constants::DHT22_TRANSMISSION_END_IDX],
        valid_ticks_received: usize,
    ) -> [bool; dht22_constants::DHT22_BYTE4_STOP_IDX + 1] {
        let mut data_bits = [false; dht22_constants::DHT22_BYTE4_STOP_IDX + 1];
        let mut count_data_bits = 0;
        for tick in ticks.iter().take(valid_ticks_received + 1) {
            if tick > &dht22_constants::DHT22_NEUTRAL_LEN_MIN
                && tick < &dht22_constants::DHT22_NEUTRAL_LEN_MAX
            {
                continue;
            }
            if tick > &dht22_constants::DHT22_BIT_TIMING_LIMIT {
                data_bits[count_data_bits] = true;
            }
            count_data_bits += 1;
            if count_data_bits > dht22_constants::DHT22_BYTE4_STOP_IDX {
                break;
            }
        }
        data_bits
    }

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// Converts a specific range of raw boolean data bits into a single `u8` byte.
    ///
    /// This private helper function takes a segment of the `data_bits` array and reconstructs
    /// the corresponding byte by iterating through the bits and shifting them into place.
    ///
    /// # Arguments
    /// * `data_bits` - The array of `bool` representing the raw data bits received from the sensor.
    /// * `start_index` - The starting index (inclusive) within `data_bits` for the byte's data.
    /// * `stop_index` - The ending index (inclusive) within `data_bits` for the byte's data.
    ///
    /// # Returns
    /// A `u8` representing the reconstructed byte.
    pub fn calc_byte(
        data_bits: [bool; dht22_constants::DHT22_BYTE4_STOP_IDX + 1],
        start_index: usize,
        stop_index: usize,
    ) -> u8 {
        // `fold` iterates through the bits, accumulating the final byte value.
        data_bits[start_index..=stop_index]
            .iter()
            .fold(0u8, |accumulator, &bit| (accumulator << 1) | (bit as u8))
    }

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// Arranges the raw boolean bit information received from the DHT22 sensor into a 5-byte array.
    ///
    /// This private helper function takes the array of individual data bits (extracted from pulse timings)
    /// and reconstructs the full data payload from the sensor, byte by byte, according to the
    /// DHT22 communication protocol.
    ///
    /// # Arguments
    /// * `data_bits` - The fixed-size array of `bool` representing the raw data bits received from the sensor.
    ///
    /// # Returns
    /// A fixed-size array of `u8` (5 bytes) containing the reconstructed sensor data,
    /// ready for checksum validation and signal calculation.
    pub fn calc_byte_array(
        data_bits: [bool; dht22_constants::DHT22_BYTE4_STOP_IDX + 1],
    ) -> [u8; dht22_constants::DHT22_DATA_LEN] {
        let mut byte_vals = [0u8; dht22_constants::DHT22_DATA_LEN];

        byte_vals[0] = Self::calc_byte(
            data_bits,
            dht22_constants::DHT22_BYTE0_START_IDX,
            dht22_constants::DHT22_BYTE0_STOP_IDX,
        );

        byte_vals[1] = Self::calc_byte(
            data_bits,
            dht22_constants::DHT22_BYTE1_START_IDX,
            dht22_constants::DHT22_BYTE1_STOP_IDX,
        );

        byte_vals[2] = Self::calc_byte(
            data_bits,
            dht22_constants::DHT22_BYTE2_START_IDX,
            dht22_constants::DHT22_BYTE2_STOP_IDX,
        );

        byte_vals[3] = Self::calc_byte(
            data_bits,
            dht22_constants::DHT22_BYTE3_START_IDX,
            dht22_constants::DHT22_BYTE3_STOP_IDX,
        );

        byte_vals[4] = Self::calc_byte(
            data_bits,
            dht22_constants::DHT22_BYTE4_START_IDX,
            dht22_constants::DHT22_BYTE4_STOP_IDX,
        );

        byte_vals
    }

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// Calculates the checksum of the DHT22 sensor data and validates it against the received checksum.
    ///
    /// This private helper function computes an 8-bit sum (modulo 256) of the first four data bytes
    /// received from the DHT22 sensor. It then compares this calculated checksum with the checksum
    /// byte provided by the sensor itself.
    ///
    /// If the checksums do not match, a warning or error is logged (depending on build configuration),
    /// and a `DhtError` is returned, indicating data corruption during transmission.
    ///
    /// # Arguments
    /// * `byte_vals` - A fixed-size array of `u8` (5 bytes) containing the sensor's raw data,
    ///   where the last byte is the received checksum.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the calculated checksum matches the received one.
    ///
    /// # Errors
    /// Returns a `DhtError` if the checksum is invalid:
    /// - `DhtError::ChecksumError`: If the calculated checksum does not match the received checksum,
    ///   indicating that the data was corrupted during transmission. This is considered a
    ///   recoverable error, and the calling function should retry the read operation.
    pub fn calc_checksum(byte_vals: [u8; dht22_constants::DHT22_DATA_LEN]) -> Result<u8, DhtError> {
        // Use `wrapping_add` for a clear and correct 8-bit sum.
        let checksum_calculated = byte_vals[0]
            .wrapping_add(byte_vals[1])
            .wrapping_add(byte_vals[2])
            .wrapping_add(byte_vals[3]);

        let checksum_received: u8 = byte_vals[4];
        if checksum_calculated != checksum_received {
            return Err(DhtError::ChecksumError(
                module_path!().to_string(),
                checksum_calculated,
                checksum_received,
            ));
        }
        Ok(checksum_calculated)
    }

    #[allow(unused)]
    /// Calculates final temperature and humidity values from the raw byte array.
    ///
    /// This function reconstructs the 16-bit integer values for humidity and temperature
    /// from their respective byte pairs. It then performs range checks to ensure the values
    /// are within the sensor's specified operating limits before converting them into
    /// standard `f32` floating-point representations.
    ///
    /// # Arguments
    /// * `byte_vals` - A fixed-size array of `u8` (5 bytes) containing the validated sensor data.
    ///
    /// # Returns
    /// A `Result` containing a tuple `(f32, f32)` with the temperature in °C and humidity in %RH.
    ///
    /// # Errors
    /// Returns a `DhtError` if a value is outside the sensor's valid operating range:
    /// - `DhtError::TemperatureOutOfRange`: If the calculated temperature is invalid.
    /// - `DhtError::HumidityOutOfRange`: If the calculated humidity is invalid.
    pub fn calc_signals(
        byte_vals: [u8; dht22_constants::DHT22_DATA_LEN],
    ) -> Result<(f32, f32), DhtError> {
        // calculate integer value for humidity * 10
        let mut hum10: u32 = byte_vals[0].into();
        hum10 <<= 8;
        hum10 += byte_vals[1] as u32;

        // calculate integer value for temperature * 10
        let mut temp10: u32 = byte_vals[2].into();
        temp10 <<= 8;
        temp10 += byte_vals[3] as u32;

        // range check for temperature
        if temp10 > dht22_constants::DHT22_MAXVAL_TEMP10 {
            return Err(DhtError::TemperatureOutOfRange(
                module_path!().to_string(),
                (temp10 / 10) as f32,
                (dht22_constants::DHT22_MINVAL_TEMP10 / 10) as f32,
                (dht22_constants::DHT22_MAXVAL_TEMP10 / 10) as f32,
            ));
        }

        // range check for humidity
        if hum10 > dht22_constants::DHT22_MAXVAL_HUM10 {
            return Err(DhtError::HumidityOutOfRange(
                module_path!().to_string(),
                (hum10 / 10) as f32,
                (dht22_constants::DHT22_MINVAL_HUM10 / 10) as f32,
                (dht22_constants::DHT22_MAXVAL_HUM10 / 10) as f32,
            ));
        }

        // calculate final values
        let temperature: f32 = (temp10 as f32) / 10.0;
        let humidity: f32 = (hum10 as f32) / 10.0;

        Ok((temperature, humidity))
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Performs a low-level read from a DHT22 sensor using the `rppal` library.
    ///
    /// This function directly interacts with the GPIO pins to implement the DHT22 communication protocol.
    /// It handles sending the start signal, setting up interrupts to capture response timings,
    /// and then processing these timings into a final result.
    ///
    /// # Arguments
    /// * `reset_sensor` - If `true`, the sensor's power (VCC) will be toggled to perform a
    ///   hardware reset before the reading.
    ///
    /// # Returns
    /// A `Result` containing a tuple `(f32, f32)` with the temperature and humidity on success.
    ///
    /// # Errors
    /// Returns a `DhtError` for a wide range of hardware and protocol failures:
    /// - `DhtError::VccPinAcquisitionFailed`: If the VCC GPIO pin cannot be acquired.
    /// - `DhtError::DataPinOutputAcquisitionFailed`: If the data GPIO pin cannot be acquired for output.
    /// - `DhtError::DataPinInputAcquisitionFailed`: If the data GPIO pin cannot be acquired for input.
    /// - `DhtError::CouldNotSetInterrupt`: If setting the GPIO interrupt fails.
    /// - `DhtError::InterruptFailed`: If an error occurs during interrupt polling.
    /// - `DhtError::Timeout`: If the sensor does not respond within the expected time frame.
    /// - `DhtError::WrongNumberOfValidTicks`: If the number of signal transitions received is incorrect.
    /// - `DhtError::ChecksumCalculationError`: If an error occurs during the checksum calculation.
    /// - `DhtError::ChecksumError`: If the calculated checksum does not match the received checksum.
    /// - `DhtError::TemperatureOutOfRange`: If the measured temperature is outside the sensor's specified range.
    /// - `DhtError::HumidityOutOfRange`: If the measured humidity is outside the sensor's specified range.
    /// - `DhtError::IncorrectPlatform`: If called with no gpio lib handle available.
    pub fn read(&mut self, reset_sensor: bool) -> DhtResult {
        let mut ticks = [0u64; dht22_constants::DHT22_TRANSMISSION_END_IDX];

        let gpio: &mut Gpio = if self.gpio_opt.is_some() {
            self.gpio_opt.as_mut().unwrap()
        } else {
            return Err(DhtError::GpioHandleNotProvided(module_path!().to_string()));
        };

        // get pin for providing supply voltage to DHT sensor
        let mut dht_vcc_pin = match gpio.get(self.gpio_dht22_vcc) {
            Ok(c) => c.into_output(),
            Err(e) => {
                return Err(DhtError::VccPinAcquisitionFailed {
                    location: module_path!().to_string(),
                    vcc_pin_nr: self.gpio_dht22_vcc,
                    source: e,
                });
            }
        };

        // scope to force the drop of the output pin
        {
            // get pin for triggering DHT sensor
            let mut data_pin = match gpio.get(self.gpio_dht22_io) {
                Ok(c) => c.into_output(),
                Err(e) => {
                    return Err(DhtError::DataPinOutputAcquisitionFailed {
                        location: module_path!().to_string(),
                        data_pin_nr: self.gpio_dht22_io,
                        source: e,
                    });
                }
            };

            // reset of DHT sensor
            if reset_sensor {
                dht_vcc_pin.set_low();
                thread::sleep(Duration::from_millis(
                    self.config.sensor_reset_duration_millis,
                ));
                dht_vcc_pin.set_high();
                thread::sleep(Duration::from_millis(
                    self.config.sensor_startup_duration_millis,
                ));
            // give the sensor some time to wake up
            } else {
                thread::sleep(Duration::from_millis(
                    self.config.sensor_pause_duration_millis,
                ));
                // pause between transmissions
            }

            // send start signal
            data_pin.write(Level::Low);
            sleep(Duration::from_micros(
                self.config.sensor_init_pin_down_duration_micros,
            ));
            data_pin.write(Level::High);
            sleep(Duration::from_micros(
                self.config.sensor_init_pin_up_duration_micros,
            ));
        }

        // get pin for reading from DHT sensor
        let mut data_pin = match gpio.get(self.gpio_dht22_io) {
            Ok(c) => c.into_input_pullup(),
            Err(e) => {
                return Err(DhtError::DataPinInputAcquisitionFailed {
                    location: module_path!().to_string(),
                    input_pin_nr: self.gpio_dht22_io,
                    source: e,
                });
            }
        };

        // The interrupt shall be triggered for both, falling and rising edge.
        // Debouncing does not work, hence no (None) debouncing time provided
        match data_pin.set_interrupt(Trigger::Both, None) {
            Ok(()) => {}
            Err(_) => {
                return Err(DhtError::CouldNotSetInterrupt(module_path!().to_string()));
            }
        }

        let timeout_duration = Duration::from_micros(self.config.timeout_duration_micros);

        let mut timeout_occurred = false;
        let mut valid_ticks_received: usize = 0;

        // initialization to avoid compiler error
        let mut duration_before: Duration = Duration::from_micros(0);
        let mut duration_after: Duration;

        // observe level changes and record the timing
        for i in 0..dht22_constants::DHT22_TRANSMISSION_END_IDX {
            // poll interrupt
            let event_opt = match data_pin.poll_interrupt(false, Some(timeout_duration)) {
                Ok(c) => c,
                Err(_) => {
                    return Err(DhtError::InterruptFailed(module_path!().to_string()));
                }
            };

            // evaluate the recorded timestamp
            match event_opt {
                Some(event) => {
                    duration_after = event.timestamp;
                }
                None => {
                    duration_after = duration_before;
                }
            }

            if i == 0 {
                // for the first bit timing, no start data is available
                ticks[0] = 0;
            } else {
                let bit_duration = duration_after - duration_before;
                ticks[i] = bit_duration.as_micros() as u64;
            }
            duration_before = duration_after;

            // abort measurement if timeout occurred
            if ticks[i] > self.config.timeout_duration_micros {
                timeout_occurred = true;
                break;
            }
            valid_ticks_received = i;
        }
        match data_pin.clear_interrupt() {
            Ok(()) => {}
            Err(_) => {
                return Err(DhtError::CouldNotClearInterrupt(module_path!().to_string()));
            }
        }

        if timeout_occurred {
            return Err(DhtError::Timeout(module_path!().to_string()));
        }

        if valid_ticks_received != dht22_constants::DHT22_TRANSMISSION_END_IDX - 1 {
            return Err(DhtError::WrongNumberOfValidTicks(
                module_path!().to_string(),
                dht22_constants::DHT22_TRANSMISSION_END_IDX - 1,
                valid_ticks_received,
            ));
        }

        // identify the data bits and store values in data_bits
        let data_bits = Self::calc_data_bits(ticks, valid_ticks_received);

        let byte_vals = Self::calc_byte_array(data_bits);

        // checksum calculation
        Self::calc_checksum(byte_vals).map_err(|e| {
            try_again = true;
            e
        });

        // calculate signals (or error code) and directly return it
        Self::calc_signals(byte_vals)
    }

    #[allow(unused)] // used in conditionally compiled code
    #[cfg(not(all(target_os = "linux", feature = "target_hw")))]
    /// Stub function for non-target platforms to allow compilation.
    ///
    /// This version is used when the `target_hw` feature is disabled or on a non-Linux OS.
    /// It does not interact with hardware and always returns an error.
    ///
    /// # Returns
    /// This function does not return a value.
    ///
    /// # Errors
    /// Always returns `Err(DhtError::IncorrectPlatform)`.
    pub fn read(&self, _reset_sensor: bool, _try_again: &mut bool) -> DhtResult {
        Err(DhtError::IncorrectPlatform(module_path!().to_string()))
    }

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// Initiates a robust reading process from the DHT22 sensor, with retries.
    ///
    /// This function attempts to read temperature and humidity data. It includes an
    /// internal retry mechanism for transient, recoverable errors like timeouts or
    /// checksum mismatches, making the sensor reading more reliable.
    ///
    /// # Returns
    /// A `Result` containing a tuple `(f32, f32)` with the temperature and humidity on success.
    ///
    /// # Errors
    /// Returns a `DhtError` if the reading fails permanently or after all retries:
    /// - `DhtError::FailedAfterRetries`: If all attempts fail due to recoverable errors.
    /// - Any other `DhtError` variant for non-recoverable hardware or configuration failures.
    pub fn get_data(&mut self) -> DhtResult {
        for i in 0..=dht22_constants::DHT22_MAX_RETRIES {
            let mut try_again = false;
            // Reset power supply only on the first attempt (i == 0).
            let read_result = self.read(i == 0, &mut try_again);

            match read_result {
                // On success, return immediately.
                Ok(data) => return Ok(data),
                Err(e) => {
                    // Check if the error is recoverable
                    let is_recoverable = matches!(e, DhtError::ChecksumError(..))
                        || matches!(e, DhtError::Timeout(_));

                    if !is_recoverable {
                        return Err(e); // Hard failure, return immediately.
                    }

                    // If this was the last retry, and it failed, return FailedAfterRetries.
                    if i == dht22_constants::DHT22_MAX_RETRIES {
                        return Err(DhtError::FailedAfterRetries {
                            location: module_path!().to_string(),
                            retry_counter: i,
                            source: Box::new(e),
                        });
                    }
                    // Otherwise, the loop will continue for another retry.
                }
            }
        }
        unreachable!();
    }

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// Executes the main loop for the DHT sensor thread, receiving requests and sending measurement responses.
    ///
    /// This function runs indefinitely, acting as a dedicated service for reading ambient
    /// temperature and humidity from the DHT22 sensor. It waits for incoming `InternalCommand`
    /// from other modules (like `Ambient` or `SignalHandler`).
    ///
    /// When a `RequestSignal` for ambient temperature or humidity is received, it performs
    /// a sensor reading (using `self.get_data()`) and sends the `Result` back. It handles
    /// `Quit` commands for graceful shutdown and logs errors/warnings for invalid commands or channel issues.
    ///
    /// # Arguments
    /// * `mutex_dht` - Mutex to store the result of the measurement
    /// * `dht_channels` - Mutable reference to the struct containing the channel
    pub fn execute(&mut self, mutex_dht: Arc<Mutex<DhtResult>>, dht_channels: &mut DhtChannels) {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        #[allow(unused)]
        let max_mutex_access_duration =
            Duration::from_millis(dht22_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS);
        #[allow(unused)]
        let mut lock_error_invalid_command: bool = false;
        #[allow(unused)]
        let mut lock_error_receive: bool = false;
        let spin_sleeper = SpinSleeper::default();
        let sleep_duration_100_millis = Duration::from_millis(100);

        loop {
            let (
                quit_command_received, // the request to end the application has been received
                _,
                _,
            ) = self.process_external_request(&mut dht_channels.rx_dht_from_signal_handler, None);

            if quit_command_received {
                break;
            }

            spin_sleeper.sleep(sleep_duration_100_millis);

            // Check if data acquisition is enabled.
            if self.config.active {
                // Check if a sufficient amount of time has passed since the last measurement.
                if self.instant_last_measurement.elapsed() > self.measurement_interval {
                    let result = self.get_data();
                    if let Err(ref dht_error) = result {
                        if matches!(
                            dht_error,
                            DhtError::FailedAfterRetries {
                                location: _,
                                retry_counter: _,
                                source: _
                            }
                        ) {
                            if !self.lock_error_failed_after_retries {
                                // avoid log flooding
                                #[cfg(not(test))]
                                log_error_chain(
                                    module_path!(),
                                    "Failure to read from DHT sensor",
                                    dht_error,
                                );
                                self.lock_error_failed_after_retries = true;
                            }
                        } else {
                            self.lock_error_failed_after_retries = false;
                            // report directly
                            #[cfg(not(test))]
                            log_error_chain(
                                module_path!(),
                                "Failure to read from DHT sensor",
                                dht_error,
                            );
                        }
                    }

                    let instant_before_locking_mutex = Instant::now();
                    let mut instant_after_locking_mutex = Instant::now(); // initialization is overwritten

                    {
                        // internal scope to limit lifetime of mutex lock
                        match mutex_dht.lock() {
                            Ok(mut c) => {
                                instant_after_locking_mutex = Instant::now();
                                *c = result;
                                self.lock_error_mutex = false;
                            }
                            Err(e) => {
                                if !self.lock_error_mutex {
                                    self.lock_error_mutex = true;
                                    error!(target: module_path!(), "error locking mutex for dht result ({e:?})");
                                }
                            }
                        };
                    }

                    // check if access to mutex took too long
                    self.check_mutex_access_duration(
                        None,
                        instant_after_locking_mutex,
                        instant_before_locking_mutex,
                    );

                    self.instant_last_measurement = Instant::now();
                }
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::launch::channels::{channel, AquaSender};
    use crate::sensors::dht::DhtResult;
    use crate::sensors::dht::{dht22_constants, Dht, DhtError};
    use crate::sensors::dht_channels::DhtChannels;
    use crate::sensors::dht_config::DhtConfig;
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::logger::setup_logger;
    use crate::utilities::logger_config::LoggerConfig;
    use assert_float_eq::*;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    // feature-specific imports
    cfg_if::cfg_if! {
        if #[cfg(all(feature = "target_hw", target_os = "linux"))] {
            use rppal::gpio::Gpio;
            use crate::utilities::config::{read_config_file, ConfigData};
        }
    }

    #[test]
    // Test case checks if the implementation correctly processes the bit information into an array.
    // The test case does not require any communication with SQL database.
    pub fn test_dht_calc_byte_array() {
        let stimuli: [bool; 40] = [
            false, // initial interrupt - to be ignored
            false, false, false, false, false, false, true, false, // Byte 0
            true, true, false, false, true, true, false, true, // Byte 1
            false, false, false, false, false, false, false, // Byte 2-7 bits only
            true, true, true, true, false, true, false, true, // Byte 3
            true, true, false, false, false, true, false, false, // Byte 4
        ];
        for i in 0..40 {
            println!("stimuli[{}]={}", i, stimuli[i]);
        }

        let reference: [u8; dht22_constants::DHT22_DATA_LEN] = [2, 205, 0, 245, 196];

        let result = Dht::calc_byte_array(stimuli);

        for i in 0..dht22_constants::DHT22_DATA_LEN {
            println!(
                "reference[{}]={} result[{}]={}",
                i, reference[i], i, result[i]
            );
        }

        assert!(reference.iter().eq(result.iter()));
    }

    #[test]
    // Test case checks if the implementation correctly calculates a valid checksum.
    // Test case does not require any communication with the database.
    pub fn test_dht_calc_checksum_valid() {
        setup_logger(LoggerConfig::default()).unwrap();

        let stimuli: [u8; dht22_constants::DHT22_DATA_LEN] = [2, 205, 0, 245, 196];

        let result = Dht::calc_checksum(stimuli);

        let ok_result = result.unwrap();

        assert_eq!(ok_result, 196);
    }

    #[test]
    // Test case checks if the implementation correctly calculates an invalid checksum.
    // Test case does not require any communication with the database.
    pub fn test_dht_calc_checksum_error() {
        setup_logger(LoggerConfig::default()).unwrap();

        let stimuli: [u8; dht22_constants::DHT22_DATA_LEN] = [255, 255, 255, 255, 10];

        let result = Dht::calc_checksum(stimuli);

        assert!(matches!(result, Err(DhtError::ChecksumError(_, _, _))));
    }

    #[test]
    // Test case checks if the implementation correctly calculates the signals from the byte array
    // given valid input data.
    // Test case does not require any communication with the database.
    pub fn test_dht_calc_signals_valid() {
        setup_logger(LoggerConfig::default()).unwrap();

        let stimuli: [u8; dht22_constants::DHT22_DATA_LEN] = [2, 205, 0, 245, 196];

        let (temperature, humidity) = match Dht::calc_signals(stimuli) {
            Ok((c, d)) => (c, d),
            Err(e) => {
                panic!("{e:?}");
            }
        };

        println!("temperature={}, humidity={}", temperature, humidity);
        assert_float_absolute_eq!(temperature, 24.5, 0.001);
        assert_float_absolute_eq!(humidity, 71.7, 0.001);
    }

    #[test]
    // Test case checks if the implementation correctly calculates the signals from the byte array
    // given invalid input data for the humidity.
    // Test case does not require any communication with the database.
    pub fn test_dht_calc_signals_humidity_too_high() {
        setup_logger(LoggerConfig::default()).unwrap();

        let stimuli: [u8; dht22_constants::DHT22_DATA_LEN] = [4, 205, 0, 245, 198];

        assert!(matches!(
            Dht::calc_signals(stimuli),
            Err(DhtError::HumidityOutOfRange(_, _, _, _))
        ));
    }

    #[test]
    // Test case checks if the implementation correctly calculates the signals from the byte array
    // given invalid input data for the temperature.
    // Test case does not require any communication with the database.
    pub fn test_dht_calc_signals_temperature_too_high() {
        setup_logger(LoggerConfig::default()).unwrap();

        let stimuli: [u8; dht22_constants::DHT22_DATA_LEN] = [2, 205, 1, 245, 197];

        assert!(matches!(
            Dht::calc_signals(stimuli),
            Err(DhtError::TemperatureOutOfRange(_, _, _, _))
        ));
    }

    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    #[test]
    // Test case executes a read operation from real hardware.
    // Test case does not require any communication with the database.
    pub fn test_dht_read() {
        setup_logger(LoggerConfig::default()).unwrap();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());

        // open interface to the GPIO library
        let gpio = match Gpio::new() {
            Ok(c) => c,
            Err(e) => {
                panic!("test_dht_read: could not open interface to GPIO ({e:?})",);
            }
        };

        let gpio_pin_number_dht22_io = config.gpio_handler.dht22_io;
        let gpio_pin_number_dht22_vcc = config.gpio_handler.dht22_vcc;

        let mut dht = Dht::new(
            config.dht,
            Some(gpio),
            gpio_pin_number_dht22_io,
            gpio_pin_number_dht22_vcc,
        );

        let (temperature, humidity) = match dht.get_data() {
            Ok((c, d)) => (c, d),
            Err(e) => {
                panic!("{e:?}");
            }
        };

        println!("temperature={}, humidity={}", temperature, humidity);
    }

    #[test]
    // Test case checks if the implementation correctly calculates the bit array from
    // a sample set of GPIO pin timings.
    // Test case does not require any communication with the database.
    pub fn test_calc_data_bits() {
        let stimuli: [u64; dht22_constants::DHT22_TRANSMISSION_END_IDX] = [
            0, 23, 51, 25, 51, 25, 51, 25, 51, 25, 51, 25, 51, 71, 51, 26, 51, 71, 51, 71, 51, 71,
            51, 25, 51, 71, 51, 25, 51, 71, 51, 27, 51, 25, 51, 25, 51, 25, 51, 25, 51, 25, 51, 25,
            51, 25, 51, 33, 51, 71, 51, 71, 51, 71, 51, 71, 51, 25, 51, 71, 51, 71, 51, 73, 51, 71,
            51, 71, 51, 71, 51, 25, 51, 25, 51, 25, 51, 71, 51, 71, 53, 0, 0, 0, 0, 0,
        ];

        let result = Dht::calc_data_bits(stimuli, 86);

        let reference: [bool; dht22_constants::DHT22_BYTE4_STOP_IDX + 1] = [
            false, false, false, false, false, false, false, true, false, true, true, true, false,
            true, false, true, false, false, false, false, false, false, false, false, true, true,
            true, true, false, true, true, true, true, true, true, false, false, false, true, true,
        ];

        for i in 0..dht22_constants::DHT22_BYTE4_STOP_IDX {
            println!(
                "reference[{}]={} result[{}]={}",
                i, reference[i], i, result[i]
            );
        }

        assert!(reference.iter().eq(result.iter()));
    }

    /// Helper function to create a standard setup for execute function tests.
    /// This reduces boilerplate code across multiple tests.
    fn setup_execute_test() -> (
        Dht,
        DhtChannels,
        AquaSender<InternalCommand>,
        Arc<Mutex<DhtResult>>,
    ) {
        let (tx, rx) = channel(1);
        let channels = DhtChannels {
            rx_dht_from_signal_handler: rx,
            #[cfg(feature = "debug_channels")]
            cnt_rx_dht_from_signal_handler: 0,
        };
        // Create a default config for testing purposes.
        let config = DhtConfig {
            active: false,
            measurement_interval_millis: 2000,
        };
        let dht = Dht::new(config, None, 0, 0);
        let mutex = Arc::new(Mutex::new(Ok((0.0, 0.0))));

        (dht, channels, tx, mutex)
    }

    #[test]
    /// Verifies that the `execute` loop terminates gracefully when a `Quit` command is received.
    fn test_execute_handles_quit_command() {
        // Arrange
        let (mut dht, mut channels, mut tx, mutex) = setup_execute_test();

        // Action
        let handle = thread::spawn(move || {
            dht.execute(mutex, &mut channels);
        });
        tx.send(InternalCommand::Quit).unwrap();

        // Assert
        // The thread should join successfully, which proves it terminated as expected.
        let join_result = handle.join();
        assert!(
            join_result.is_ok(),
            "The DHT thread should terminate gracefully on Quit."
        );
    }

    #[test]
    /// Verifies that the `execute` loop continues running after receiving an unrecognized command.
    fn test_execute_handles_invalid_command() {
        // Arrange
        let (mut dht, mut channels, mut tx, mutex) = setup_execute_test();

        // Action
        let handle = thread::spawn(move || {
            dht.execute(mutex, &mut channels);
        });
        // Send a command that the `execute` function doesn't explicitly handle.
        tx.send(InternalCommand::Stop).unwrap();
        // Give the thread a moment to process the invalid command.
        thread::sleep(Duration::from_millis(150));
        // Now send the Quit command to terminate the loop.
        tx.send(InternalCommand::Quit).unwrap();

        // Assert
        // The thread should not have panicked and should join successfully.
        let join_result = handle.join();
        assert!(
            join_result.is_ok(),
            "The DHT thread should handle invalid commands and continue until Quit."
        );
    }

    #[test]
    #[cfg(not(all(target_os = "linux", feature = "target_hw")))]
    /// Verifies that a measurement is performed and the result is written to the shared mutex
    /// when the measurement interval elapses. This test is for non-hardware platforms.
    fn test_execute_updates_mutex_on_measurement_interval() {
        // Arrange
        let (mut tx, rx) = channel(1);
        let mut channels = DhtChannels {
            rx_dht_from_signal_handler: rx,
            #[cfg(feature = "debug_channels")]
            cnt_rx_dht_from_signal_handler: 0,
        };
        let config = DhtConfig {
            active: true,                    // Enable measurement
            measurement_interval_millis: 50, // Use a short interval for testing
        };
        let mut dht = Dht::new(config, None, 0, 0);
        let mutex = Arc::new(Mutex::new(Ok((0.0, 0.0))));
        let mutex_clone = mutex.clone();

        // Action
        let handle = thread::spawn(move || {
            dht.execute(mutex_clone, &mut channels);
        });

        // Wait for longer than the measurement interval to ensure a measurement is attempted.
        thread::sleep(Duration::from_millis(200));

        println!("Sending quit command to DHT thread");
        tx.send(InternalCommand::Quit).unwrap();
        handle.join().unwrap();

        // Assert
        // On non-hardware platforms, `get_data()` returns `Err(DhtError::IncorrectPlatform)`.
        // We check that this specific error was written to the mutex.
        let final_result = mutex.lock().unwrap();
        assert!(
            matches!(*final_result, Err(DhtError::IncorrectPlatform(_))),
            "Mutex should contain IncorrectPlatform error on non-hardware targets after measurement"
        );
    }
}
