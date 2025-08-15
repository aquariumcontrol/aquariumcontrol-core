/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::utilities::channel_content::InternalCommand;
use log::error;
#[cfg(all(not(test), target_os = "linux"))]
use log::info;

#[cfg(not(test))]
use log::warn;

#[cfg(all(not(test), target_os = "linux"))]
use nix::unistd::gettid;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::sensors::dht_error::DhtError;
use crate::utilities::logger::log_error_chain;

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
        use rppal::gpio::{Gpio, Level, Trigger};
        use std::thread::{self, sleep};

        #[cfg(feature = "debug_dht")]
        use log::debug;
    }
    else {
        use crate::sensors::gpio_handler::Gpio;
    }
}

/// Type definition to avoid linter warnings about complex return types
pub type DhtResult = Result<(f32, f32), DhtError>; // Temperature and Humidity

/// Holds the configuration data for interacting with GPIO
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct DhtConfig {
    /// allows deactivation of the data acquisition
    pub active: bool,

    /// duration to keep pin down during start signal in microseconds
    #[allow(unused)]
    sensor_init_pin_down_duration_micros: u64,

    /// duration to keep pin up during start signal in microseconds
    #[allow(unused)]
    sensor_init_pin_up_duration_micros: u64,

    /// duration for sensor to respond after sending start signal in microseconds
    #[allow(unused)]
    sensor_init_input_duration_micros: u64,

    /// maximum permissible duration for the sensor to respond in microseconds
    #[allow(unused)]
    timeout_duration_micros: u64,

    /// duration in milliseconds the sensor is switched off before requesting transmission
    #[allow(unused)]
    sensor_reset_duration_millis: u64,

    /// duration in milliseconds the sensor is given to initialize
    #[allow(unused)]
    sensor_startup_duration_millis: u64,

    /// duration in milliseconds the sensor is given to pause between transmissions
    #[allow(unused)]
    sensor_pause_duration_millis: u64,

    /// duration in milliseconds between two measurements
    measurement_interval_millis: u64,
}

/// Contains constants for communication with the DHT22 sensor.
pub mod dht22_constants {
    #[allow(unused)]
    pub const DHT22_DATA_LEN: usize = 5;
    #[allow(unused)]
    pub const DHT22_BYTE0_START_IDX: usize = 1;
    #[allow(unused)]
    pub const DHT22_BYTE0_STOP_IDX: usize = 8;
    #[allow(unused)]
    pub const DHT22_BYTE1_START_IDX: usize = 9;
    #[allow(unused)]
    pub const DHT22_BYTE1_STOP_IDX: usize = 16;
    #[allow(unused)]
    pub const DHT22_BYTE2_START_IDX: usize = 17;
    #[allow(unused)]
    pub const DHT22_BYTE2_STOP_IDX: usize = 23;
    #[allow(unused)]
    pub const DHT22_BYTE3_START_IDX: usize = 24;
    #[allow(unused)]
    pub const DHT22_BYTE3_STOP_IDX: usize = 31;
    #[allow(unused)]
    pub const DHT22_BYTE4_START_IDX: usize = 32;
    #[allow(unused)]
    pub const DHT22_BYTE4_STOP_IDX: usize = 39;

    #[allow(unused)]
    pub const DHT22_TRANSMISSION_END_IDX: usize = 86;

    #[allow(unused)]
    pub const DHT22_HUMIDITY_START_IDX: usize = 1;
    #[allow(unused)]
    pub const DHT22_HUMIDITY_STOP_IDX: usize = 16;

    #[allow(unused)]
    pub const DHT22_TEMPERATURE_START_IDX: usize = 1;
    #[allow(unused)]
    pub const DHT22_TEMPERATURE_STOP_IDX: usize = 16;

    // minimum and maximum value for temperature10 range checks
    #[allow(unused)]
    pub const DHT22_MINVAL_TEMP10: u32 = 0;
    #[allow(unused)]
    pub const DHT22_MAXVAL_TEMP10: u32 = 500;

    // minimum and maximum value for temperature10 range checks
    #[allow(unused)]
    pub const DHT22_MINVAL_HUM10: u32 = 0;
    #[allow(unused)]
    pub const DHT22_MAXVAL_HUM10: u32 = 1000;

    // timing of (neutral) low phase between data bit
    #[allow(unused)]
    pub const DHT22_NEUTRAL_LEN_MIN: u64 = 30;
    #[allow(unused)]
    pub const DHT22_NEUTRAL_LEN_MAX: u64 = 60;

    // distinguish between true and false
    #[allow(unused)]
    pub const DHT22_BIT_TIMING_LIMIT: u64 = 50;

    // maximum number of allowed retries to get valid data
    #[allow(unused)]
    pub const DHT22_MAX_RETRIES: u64 = 5;

    /// allow max. 10 milliseconds for mutex to be blocked by any other thread
    #[allow(unused)]
    pub const MAX_MUTEX_ACCESS_DURATION_MILLIS: u64 = 10;
}

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
/// **Thread Communication: **
/// This module is designed to run in its own dedicated thread, periodically reading sensor data
/// and making the latest measurements available via a shared `Arc<Mutex<DhtResult>>`.
/// Other modules (like `Ambient` or `DataLogger`) can then read from this mutex to get the
/// current temperature and humidity. It also responds to `Quit` commands for graceful shutdown.
///
/// **Platform-Specific Behavior: **
/// The core sensor reading logic (`read` function) is conditionally compiled:
/// - On Linux with the `target_hw` feature enabled, it directly uses `rppal::gpio` for hardware interaction.
/// - On other platforms or when `target_hw` is disabled, it uses a stub implementation (`Err(DhtError::IncorrectPlatform)`)
///   to allow compilation and testing without physical hardware.
///
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     dht[Dht] -.-> Ambient[Ambient]
///     signal_handler[SignalHandler] --> dht
/// ```
pub struct Dht {
    #[allow(unused)]
    /// configuration for communication with DHT sensor
    config: DhtConfig,

    /// GPIO interface
    #[allow(unused)]
    gpio_opt: Option<Gpio>,

    #[allow(unused)]
    /// numeric GPIO pin for communication with DHT22 sensor
    gpio_dht22_io: u8,

    /// numeric GPIO pin for providing power to the DHT22 sensor
    #[allow(unused)]
    gpio_dht22_vcc: u8,

    /// time instance of the last recording
    instant_last_measurement: Instant,

    /// flag to avoid flooding the log file in case mutex cannot be locked
    lock_error_mutex: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex
    lock_warn_max_mutex_access_duration: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read from DHT sensor
    lock_error_failed_after_retries: bool,

    // for testing purposes: record when mutex access time is exceeded without resetting it
    #[cfg(test)]
    mutex_access_duration_exceeded: bool,
}

impl Dht {
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
        }
    }

    #[allow(unused)]
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

    #[allow(unused)]
    /// Converts a specific range of raw boolean data bits into a single `u8` byte.
    ///
    /// This private helper function takes a segment of the `data_bits` array,
    /// shifts bits to the left, and sets them according to their boolean value
    /// to reconstruct the corresponding byte.
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
        let mut byte_val: u8 = 0;
        for bit in data_bits.iter().take(stop_index).skip(start_index) {
            byte_val |= match bit {
                true => 1,
                false => 0,
            };
            byte_val <<= 1;
        }
        byte_val |= match data_bits[stop_index] {
            true => 1,
            false => 0,
        };
        byte_val
    }

    #[allow(unused)]
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

    #[allow(unused)]
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
    /// * `try_again` - A mutable reference to a boolean flag. Set to `true` when the top-level calling function may try again.    
    ///
    /// # Returns
    /// A `Result<u8, DhtError>`:
    /// - `Ok(u8)`: The calculated checksum if it matches the received one.
    /// - `Err(DhtError::ChecksumCalculationError)`: If an unexpected error occurs during the checksum calculation process.
    /// - `Err(DhtError::ChecksumError)`: If the calculated checksum does not match the received checksum,
    ///   indicating invalid sensor data.
    pub fn calc_checksum(
        byte_vals: [u8; dht22_constants::DHT22_DATA_LEN],
        try_again: &mut bool,
    ) -> Result<u8, DhtError> {
        // calculate and compare checksum
        let mut checksum_intermediate: u16 =
            byte_vals[0] as u16 + byte_vals[1] as u16 + byte_vals[2] as u16 + byte_vals[3] as u16;
        checksum_intermediate &= 0xFF;
        let checksum_calculated: u8 = match ((checksum_intermediate << 8) >> 8).try_into() {
            Ok(c) => c,
            Err(_) => return Err(DhtError::ChecksumCalculationError),
        };

        let checksum_received: u8 = byte_vals[4];
        if checksum_calculated != checksum_received {
            *try_again = true;
            return Err(DhtError::ChecksumError(
                checksum_calculated,
                checksum_received,
            ));
        }
        Ok(checksum_calculated)
    }

    #[allow(unused)]
    /// Calculates temperature and humidity values (as `f32`) from the raw byte array received from a DHT22 sensor.
    ///
    /// This private helper function reconstructs the 16-bit integer values for humidity and temperature
    /// from their respective byte pairs in the `byte_vals` array. It then performs range checks
    /// to ensure the values are within the sensor's specified operating limits. Finally, it converts
    /// these scaled integer values into standard floating-point `f32` representations.
    ///
    /// # Arguments
    /// * `byte_vals` - A fixed-size array of `u8` (5 bytes) containing the sensor's raw data,
    ///   where the first four bytes represent humidity and temperature, and the last is the checksum.
    ///
    /// # Returns
    /// A `Result<(f32, f32), DhtError>`:
    /// - `Ok((f32, f32))`: A tuple containing the calculated temperature in °C and humidity in %.
    /// - `Err(DhtError::TemperatureOutOfRange)`: If the calculated temperature is outside the permissible limits.
    /// - `Err(DhtError::HumidityOutOfRange)`: If the calculated humidity is outside the permissible limits.
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
                (temp10 / 10) as f32,
                (dht22_constants::DHT22_MINVAL_TEMP10 / 10) as f32,
                (dht22_constants::DHT22_MAXVAL_TEMP10 / 10) as f32,
            ));
        }

        // range check for humidity
        if hum10 > dht22_constants::DHT22_MAXVAL_HUM10 {
            return Err(DhtError::HumidityOutOfRange(
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
    /// Performs a low-level read operation to get temperature and humidity data from a DHT22 sensor.
    ///
    /// This function directly interacts with the GPIO pins to implement the DHT22 communication protocol.
    /// It handles sending the start signal, setting up interrupts to capture response timings,
    /// and then processing these timings into raw data bits and bytes. It includes checks
    /// for timeouts and validates checksums.
    ///
    /// # Arguments
    /// * `reset_sensor` - A boolean flag. If `true`, the sensor's power supply (VCC) will be briefly
    ///   interrupted and restored before measurement, which can act as a hardware reset.
    ///   If `false`, a pause duration is observed instead.
    /// * `try_again` - A mutable reference to a boolean flag. Set to `true` when the top-level calling function may try again.    
    ///
    /// # Returns
    /// A `Result<(f32, f32), DhtError>`:
    /// - `Ok((f32, f32))`: A tuple containing the measured temperature in °C and humidity in %, if the reading is successful and valid.
    /// - `Err(DhtError::VccPinAcquisitionFailed)`: If the VCC GPIO pin cannot be acquired.
    /// - `Err(DhtError::DataPinOutputAcquisitionFailed)`: If the data GPIO pin cannot be acquired for output.
    /// - `Err(DhtError::DataPinInputAcquisitionFailed)`: If the data GPIO pin cannot be acquired for input.
    /// - `Err(DhtError::CouldNotSetInterrupt)`: If setting the GPIO interrupt fails.
    /// - `Err(DhtError::InterruptFailed)`: If an error occurs during interrupt polling.
    /// - `Err(DhtError::Timeout)`: If the sensor does not respond within the expected time frame.
    /// - `Err(DhtError::WrongNumberOfValidTicks)`: If the number of signal transitions received is incorrect.
    /// - `Err(DhtError::ChecksumCalculationError)`: If an error occurs during the checksum calculation.
    /// - `Err(DhtError::ChecksumError)`: If the calculated checksum does not match the received checksum.
    /// - `Err(DhtError::TemperatureOutOfRange)`: If the measured temperature is outside the sensor's specified range.
    /// - `Err(DhtError::HumidityOutOfRange)`: If the measured humidity is outside the sensor's specified range.
    /// - `Err(DhtError::IncorrectPlatform)`: If called with no gpio lib handle available.
    pub fn read(&mut self, reset_sensor: bool, try_again: &mut bool) -> DhtResult {
        let mut ticks = [0u64; dht22_constants::DHT22_TRANSMISSION_END_IDX];

        let gpio: &mut Gpio = if self.gpio_opt.is_some() {
            self.gpio_opt.as_mut().unwrap()
        } else {
            return Err(DhtError::IncorrectPlatform);
        };

        // get pin for providing supply voltage to DHT sensor
        let mut dht_vcc_pin = match gpio.get(self.gpio_dht22_vcc) {
            Ok(c) => c.into_output(),
            Err(e) => {
                return Err(DhtError::VccPinAcquisitionFailed {
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
                return Err(DhtError::CouldNotSetInterrupt);
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
                    return Err(DhtError::InterruptFailed);
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
                return Err(DhtError::CouldNotClearInterrupt);
            }
        }

        if timeout_occurred {
            *try_again = true;
            return Err(DhtError::Timeout);
        }

        if valid_ticks_received != dht22_constants::DHT22_TRANSMISSION_END_IDX - 1 {
            return Err(DhtError::WrongNumberOfValidTicks(
                dht22_constants::DHT22_TRANSMISSION_END_IDX - 1,
                valid_ticks_received,
            ));
        }

        // identify the data bits and store values in data_bits
        let data_bits = Self::calc_data_bits(ticks, valid_ticks_received);

        let byte_vals = Self::calc_byte_array(data_bits);

        // checksum calculation
        match Self::calc_checksum(byte_vals, try_again) {
            Ok(_) => { /* do nothing */ }
            Err(e) => return Err(e),
        }

        // calculate signals (or error code) and directly return it
        Self::calc_signals(byte_vals)
    }

    #[cfg(not(all(target_os = "linux", feature = "target_hw")))]
    /// Stub function to allow build on a non-target platform
    pub fn read(&self, _reset_sensor: bool, _try_again: &mut bool) -> DhtResult {
        Err(DhtError::IncorrectPlatform)
    }

    /// Initiates the reading process from the DHT22 sensor, including retry logic and post-processing of data.
    ///
    /// This function attempts to read temperature and humidity data from the DHT22 sensor.
    /// It includes an internal retry mechanism (`DHT22_MAX_RETRIES`) for transient
    /// errors like timeouts or checksum mismatches, making the sensor reading more robust.
    /// If an unrecoverable error occurs or all retries fail, an appropriate `DhtError` is returned.
    ///
    /// # Returns
    /// A `Result<(f32, f32), DhtError>`:
    /// - `Ok((f32, f32))`: A tuple containing the measured temperature in °C and humidity in %, if the reading is successful and valid.
    /// - `Err(DhtError)`: An error indicating various failure modes, including sensor communication issues,
    ///   checksum errors, timeouts after all retries, or out-of-range sensor values.
    pub fn get_data(&mut self) -> DhtResult {
        // read one sample from DHT sensor to verify SW and HW are working ok
        let mut retry_counter = 0;
        let mut try_again = false;

        loop {
            let read_result = self.read(retry_counter == 0, &mut try_again); // reset power supply for the first read
            match read_result {
                Ok((temperature, humidity)) => {
                    #[cfg(feature = "debug_dht")]
                    debug!(
                        target: module_path!(),
                        "measured successfully: temperature={}, humidity={}",
                        temperature,
                        humidity
                    );

                    return Ok((temperature, humidity));
                }
                Err(e) => {
                    if try_again {
                        // for errors DhtError::Timeout and DhtError::ChecksumError
                        // These two errors shall not be reported directly.
                        // Instead, the application will try to read again for multiple times.
                        retry_counter += 1;
                        if retry_counter > dht22_constants::DHT22_MAX_RETRIES {
                            return Err(DhtError::FailedAfterRetries {
                                retry_counter,
                                source: Box::new(e),
                            });
                        }
                    } else {
                        // any other failure will be reported directly
                        return Err(e);
                    }
                }
            }
        }
    }

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
    /// * `tx_to_ambient` - The sender channel for sending `Result<(f32, f32), DhtError>`
    ///   measurement data back to the `Ambient` module.
    /// * `rx_from_ambient_or_signal_handler` - The receiver channel for getting `InternalCommand`s
    ///   (e.g., `RequestSignal`, `Quit`) from the `Ambient` module or the `SignalHandler`.
    pub fn execute(
        &mut self,
        mutex_dht: Arc<Mutex<DhtResult>>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let max_mutex_access_duration =
            Duration::from_millis(dht22_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS);
        let mut lock_error_invalid_command: bool = false;
        let mut lock_error_receive: bool = false;
        let spin_sleeper = SpinSleeper::default();
        let sleep_duration_100_millis = Duration::from_millis(100);

        loop {
            match rx_from_signal_handler.try_recv() {
                Ok(c) => {
                    lock_error_receive = false;
                    match c {
                        InternalCommand::Quit => {
                            #[cfg(feature = "debug_dht")]
                            debug!(
                                target: module_path!(),
                                "Received QUIT command"
                            );
                            break;
                        }
                        _ => {
                            if !lock_error_invalid_command {
                                error!(
                                    target: module_path!(),
                                    "Received invalid command from signal handler."
                                );
                                lock_error_invalid_command = true;
                            }
                        }
                    };
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* do nothing */ }
                        TryRecvError::Disconnected => {
                            if !lock_error_receive {
                                error!(
                                    target: module_path!(),
                                    "Error in trying to receive command from signal handler: {e:?}"
                                );
                                lock_error_receive = true;
                            }
                        }
                    }
                }
            };
            spin_sleeper.sleep(sleep_duration_100_millis);

            // Check if data acquisition is enabled.
            if self.config.active {
                // Check if a sufficient amount of time has passed since the last measurement.
                let current_instant = Instant::now();
                if current_instant
                    .duration_since(self.instant_last_measurement)
                    .as_millis()
                    > self.config.measurement_interval_millis.into()
                {
                    let result = self.get_data();
                    if let Err(ref dht_error) = result {
                        if matches!(
                            dht_error,
                            DhtError::FailedAfterRetries {
                                retry_counter: _,
                                source: _
                            }
                        ) {
                            if !self.lock_error_failed_after_retries {
                                // avoid log flooding
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
                    let measured_mutex_access_duration =
                        instant_after_locking_mutex.duration_since(instant_before_locking_mutex);
                    if measured_mutex_access_duration > max_mutex_access_duration {
                        if !self.lock_warn_max_mutex_access_duration {
                            self.lock_warn_max_mutex_access_duration = true;
                            #[cfg(not(test))]
                            warn!(target: module_path!(),
                                "Access to mutex took too long. Allowed={}ms, Measured={}ms",
                                max_mutex_access_duration.as_millis(),
                                measured_mutex_access_duration.as_millis(),
                            );

                            // record for testing purposes only
                            cfg_if::cfg_if! {
                                if #[cfg(test)] {
                                    self.mutex_access_duration_exceeded = true;
                                }
                            }
                        }
                    } else {
                        self.lock_warn_max_mutex_access_duration = false;
                    }

                    self.instant_last_measurement = current_instant;
                }
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use assert_float_eq::*;

    use crate::sensors::dht::{dht22_constants, Dht, DhtError};
    use crate::utilities::logger::{setup_logger, LoggerConfig};

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
        let mut try_again: bool = false;

        setup_logger(LoggerConfig::default()).unwrap();

        let stimuli: [u8; dht22_constants::DHT22_DATA_LEN] = [2, 205, 0, 245, 196];

        let result = Dht::calc_checksum(stimuli, &mut try_again);

        let ok_result = result.unwrap();

        assert_eq!(ok_result, 196);

        assert!(!try_again);
    }

    #[test]
    // Test case checks if the implementation correctly calculates an invalid checksum.
    // Test case does not require any communication with the database.
    pub fn test_dht_calc_checksum_error() {
        let mut try_again: bool = false;

        setup_logger(LoggerConfig::default()).unwrap();

        let stimuli: [u8; dht22_constants::DHT22_DATA_LEN] = [255, 255, 255, 255, 10];

        let result = Dht::calc_checksum(stimuli, &mut try_again);

        assert!(matches!(result, Err(DhtError::ChecksumError(_, _))));
        assert!(try_again);
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
            Err(DhtError::HumidityOutOfRange(_, _, _))
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
            Err(DhtError::TemperatureOutOfRange(_, _, _))
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
}
