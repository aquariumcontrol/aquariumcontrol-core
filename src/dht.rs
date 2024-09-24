/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::error;

#[cfg(target_os = "linux")]
use log::warn;
#[cfg(target_os = "linux")]
use rppal::gpio::{Gpio, Level, Trigger};
use serde_derive::Deserialize;
#[cfg(target_os = "linux")]
use std::thread;
#[cfg(target_os = "linux")]
use std::thread::sleep;
#[cfg(target_os = "linux")]
use std::time::Duration;

const LOG_TARGET_DHT: &str = "dht";

/// Holds the configuration data for the GPIO handler.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct DhtConfig {
    /// duration to keep pin down during start signal in microseconds
    pub sensor_init_pin_down_duration_micros: u64,

    /// duration to keep pin up during start signal in microseconds
    pub sensor_init_pin_up_duration_micros: u64,

    /// duration for sensor to respond after sending start signal in microseconds
    pub sensor_init_input_duration_micros: u64,

    /// maximum permissible duration for sensor to respond in microseconds
    pub timeout_duration_micros: u64,

    // duration in milliseconds the sensor is switched off before requesting transmission
    pub sensor_reset_duration_millis: u64,

    // duration in milliseconds the sensor is given to initialize
    pub sensor_startup_duration_millis: u64,

    // duration in milliseconds the sensor is given to pause between transmissions
    pub sensor_pause_duration_millis: u64,
}

#[allow(unused)]
const DHT22_DATA_LEN: usize = 5;
#[allow(unused)]
const DHT22_BYTE0_START_IDX: usize = 1;
#[allow(unused)]
const DHT22_BYTE0_STOP_IDX: usize = 8;
#[allow(unused)]
const DHT22_BYTE1_START_IDX: usize = 9;
#[allow(unused)]
const DHT22_BYTE1_STOP_IDX: usize = 16;
#[allow(unused)]
const DHT22_BYTE2_START_IDX: usize = 17;
#[allow(unused)]
const DHT22_BYTE2_STOP_IDX: usize = 23;
#[allow(unused)]
const DHT22_BYTE3_START_IDX: usize = 24;
#[allow(unused)]
const DHT22_BYTE3_STOP_IDX: usize = 31;
#[allow(unused)]
const DHT22_BYTE4_START_IDX: usize = 32;
#[allow(unused)]
const DHT22_BYTE4_STOP_IDX: usize = 39;

#[allow(unused)]
const DHT22_TRANSMISSION_END_IDX: usize = 86;

#[allow(unused)]
const DHT22_HUMIDITY_START_IDX: usize = 1;
#[allow(unused)]
const DHT22_HUMIDITY_STOP_IDX: usize = 16;

#[allow(unused)]
const DHT22_TEMPERATURE_START_IDX: usize = 1;
#[allow(unused)]
const DHT22_TEMPERATURE_STOP_IDX: usize = 16;

// minimum and maximum value for temperature10 signal range check
#[allow(unused)]
const DHT22_MINVAL_TEMP10: i32 = -500;
#[allow(unused)]
const DHT22_MAXVAL_TEMP10: i32 = 500;

// minimum and maximum value for temperature10 signal range check
#[cfg(target_os = "linux")]
const DHT22_MINVAL_HUM10: i32 = -10;
#[cfg(target_os = "linux")]
const DHT22_MAXVAL_HUM10: i32 = 1000;

// timing of (neutral) low phase between data bit
#[allow(unused)]
const DHT22_NEUTRAL_LEN_MIN: u64 = 30;
#[allow(unused)]
const DHT22_NEUTRAL_LEN_MAX: u64 = 60;

// distinguish between true and false
#[allow(unused)]
const DHT22_BIT_TIMING_LIMIT: u64 = 50;

#[derive(Debug, PartialEq)]
pub enum DhtError {
    /// Vcc pin acquisition failed
    #[cfg(target_os = "linux")]
    VccPinAcquisitionFailed,

    /// Data pin acquisition for output failed
    #[cfg(target_os = "linux")]
    DataPinOutputAcquisitionFailed,

    /// Data pin acquisition for input failed
    #[cfg(target_os = "linux")]
    DataPinInputAcquisitionFailed,

    /// Could not set interrupt
    #[cfg(target_os = "linux")]
    CouldNotSetInterrupt,

    /// Could not clear interrupt
    #[cfg(target_os = "linux")]
    CouldNotClearInterrupt,

    // execution of interrupt failed
    #[cfg(target_os = "linux")]
    InterruptFailed,

    // error occurred during calculation of checksum
    ChecksumCalculationError,

    // calculated checksum does not match received value
    ChecksumError,

    // timeout during transmission of data
    Timeout,

    // temperature value is out of range
    TemperatureOutOfRange,

    // humidity value is out of range
    HumidityOutOfRange,
}

#[allow(unused)]
pub fn calc_data_bits(
    ticks: [u64; DHT22_TRANSMISSION_END_IDX],
    valid_ticks_received: usize,
) -> [bool; DHT22_TRANSMISSION_END_IDX] {
    let mut data_bits = [false; DHT22_TRANSMISSION_END_IDX];
    let mut count_data_bits = 0;
    for i in 0..(valid_ticks_received + 1) {
        if ticks[i] > DHT22_NEUTRAL_LEN_MIN && ticks[i] < DHT22_NEUTRAL_LEN_MAX {
            continue;
        }
        if ticks[i] > DHT22_BIT_TIMING_LIMIT {
            data_bits[i] = true;
        }
        println!(
            "data_bit_len[{}] = {} => {}",
            count_data_bits,
            ticks[i],
            match data_bits[i] {
                true => "TRUE",
                false => "FALSE",
            }
        );
        count_data_bits += 1;
    }
    data_bits
}

#[allow(unused)]
pub fn calc_byte_array(data_bits: [bool; DHT22_BYTE4_STOP_IDX]) -> [u8; DHT22_DATA_LEN] {
    let mut byte_vals = [0u8; DHT22_DATA_LEN];
    // calculate byte values as preparation for checksum calculation
    for idx_bit in DHT22_BYTE0_START_IDX..DHT22_BYTE0_STOP_IDX + 1 {
        byte_vals[0] |= match data_bits[idx_bit] {
            true => 1,
            false => 0,
        };
        byte_vals[0] = byte_vals[0] << 1;
    }
    for idx_bit in DHT22_BYTE1_START_IDX..DHT22_BYTE1_STOP_IDX + 1 {
        byte_vals[1] |= match data_bits[idx_bit] {
            true => 1,
            false => 0,
        };
        byte_vals[1] = byte_vals[1] << 1;
    }
    for idx_bit in DHT22_BYTE2_START_IDX..DHT22_BYTE2_STOP_IDX + 1 {
        byte_vals[2] |= match data_bits[idx_bit] {
            true => 1,
            false => 0,
        };
        byte_vals[2] = byte_vals[2] << 1;
    }
    for idx_bit in DHT22_BYTE3_START_IDX..DHT22_BYTE3_STOP_IDX + 1 {
        byte_vals[3] |= match data_bits[idx_bit] {
            true => 1,
            false => 0,
        };
        byte_vals[3] = byte_vals[3] << 1;
    }
    for idx_bit in DHT22_BYTE4_START_IDX..DHT22_BYTE4_STOP_IDX + 1 {
        byte_vals[4] |= match data_bits[idx_bit] {
            true => 1,
            false => 0,
        };
        byte_vals[4] = byte_vals[4] << 1;
    }
    byte_vals
}

#[allow(unused)]
pub fn calc_checksum(byte_vals: [u8; DHT22_DATA_LEN]) -> Result<u8, DhtError> {
    // calculate and compare checksum
    let mut checksum_intermediate: u16 =
        byte_vals[0] as u16 + byte_vals[1] as u16 + byte_vals[2] as u16 + byte_vals[3] as u16;
    checksum_intermediate = checksum_intermediate & 0xFF;
    let checksum_calculated: u8 = match ((checksum_intermediate << 8) >> 8).try_into() {
        Ok(c) => c,
        Err(_) => return Err(DhtError::ChecksumCalculationError),
    };

    let checksum_received: u8 = byte_vals[4].into();
    if checksum_calculated != checksum_received {
        error!(
            target: LOG_TARGET_DHT,
            "checksum error (calculated {}, received {})",
            checksum_calculated, checksum_received
        );
        return Err(DhtError::ChecksumError);
    }
    Ok(checksum_calculated)
}

#[cfg(target_os = "linux")]
/// read temperature and humity from DHT sensor
/// # Arguments
/// * `config` - configuration for DHT communicationn
/// * `pin` - struct of type InputPin
pub fn read_dht_data(
    config: &DhtConfig,
    gpio: &Gpio,
    gpio_pin_number_dht22_vcc: u8,
    gpio_pin_number_dht22_io: u8,
    reset_sensor: bool,
) -> Result<(f32, f32), DhtError> {
    let mut ticks = [0u64; DHT22_TRANSMISSION_END_IDX];

    // get pin for providing supply voltage to DHT sensor
    #[cfg(target_os = "linux")]
    let mut dht_vcc_pin = match gpio.get(gpio_pin_number_dht22_vcc) {
        Ok(c) => c.into_output(),
        Err(e) => {
            error!(
                target: LOG_TARGET_DHT,
                "could not get pin {} from GPIO interface for DHT power supply ({e:?})",
                gpio_pin_number_dht22_vcc
            );
            return Err(DhtError::VccPinAcquisitionFailed);
        }
    };

    // scope to force drop of output pin
    {
        // get pin for triggering DHT sensor
        #[cfg(target_os = "linux")]
        let mut data_pin = match gpio.get(gpio_pin_number_dht22_io) {
            Ok(c) => c.into_output(),
            Err(e) => {
                error!(
                    target: LOG_TARGET_DHT,
                    "failed to aquire pin {} for triggering DHT sensor ({e:?})",
                    gpio_pin_number_dht22_io,
                );
                return Err(DhtError::DataPinOutputAcquisitionFailed);
            }
        };

        // reset of DHT sensor
        if reset_sensor {
            dht_vcc_pin.set_low();
            thread::sleep(Duration::from_millis(config.sensor_reset_duration_millis));
            dht_vcc_pin.set_high();
            thread::sleep(Duration::from_millis(config.sensor_startup_duration_millis));
        // give sensor some time to wake up
        } else {
            thread::sleep(Duration::from_millis(sensor_pause_duration_millis)); // pause between transmissions
        }

        // send start signal
        data_pin.write(Level::Low);
        sleep(Duration::from_micros(
            config.sensor_init_pin_down_duration_micros,
        ));
        data_pin.write(Level::High);
        sleep(Duration::from_micros(
            config.sensor_init_pin_up_duration_micros,
        ));
    }

    // get pin for reading from DHT sensor
    #[cfg(target_os = "linux")]
    let mut data_pin = match gpio.get(gpio_pin_number_dht22_io) {
        Ok(c) => c.into_input_pullup(),
        Err(e) => {
            error!(
                "failed to aquire pin {} for reading from DHT sensor ({e:?})",
                gpio_pin_number_dht22_io,
            );
            return Err(DhtError::DataPinInputAcquisitionFailed);
        }
    };

    // interrupts shall be triggered for both falling and rising edge
    // debouncing does not work, hence no (None) debouncing time provided
    match data_pin.set_interrupt(Trigger::Both, None) {
        Ok(()) => {}
        Err(_) => {
            return Err(DhtError::CouldNotSetInterrupt);
        }
    }

    let timeout_duration = Duration::from_micros(config.timeout_duration_micros);

    let mut timeout_occurred = false;
    let mut valid_ticks_received: usize = 0;

    let mut duration_before: Duration = Duration::from_micros(0); // initialization to avoid compiler error
    let mut duration_after: Duration;

    // observe level changes and record the timing
    for i in 0..DHT22_TRANSMISSION_END_IDX {
        // poll the interrupt
        let event_opt = match data_pin.poll_interrupt(false, Some(timeout_duration)) {
            Ok(c) => c,
            Err(_) => {
                return Err(DhtError::InterruptFailed);
            }
        };

        if i == 0 {
            // for the first bit timing, no start data is available
            ticks[0] = 0;
        } else {
            // evaluate the recorded timestamp
            match event_opt {
                Some(event) => {
                    duration_after = event.timestamp;
                }
                None => {
                    warn!(
                        target: LOG_TARGET_DHT,
                        "empty interrupt event for bit #{}",
                        i
                    );
                    duration_after = duration_before;
                }
            }
            let bit_duration = duration_after - duration_before;
            ticks[i] = bit_duration.as_micros() as u64;
        }
        duration_before = duration_after;

        // abort measuring, if timeout occurred
        if ticks[i] > config.timeout_duration_micros {
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
        return Err(DhtError::Timeout);
    }

    // identify the data bits and store values in data_bits
    let data_bits = calc_data_bits(ticks, valid_ticks_received);

    let byte_vals = calc_byte_array(data_bits);

    for i in 0..DHT22_DATA_LEN {
        println!("byte_vals[{}] = {}", i, byte_vals[i]);
    }

    // checksum calculation
    let checksum = match calc_checksum(byte_vals) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // calculate humidity
    let mut hum10: u32 = byte_vals[0].into();
    hum10 = hum10 << 8;
    hum10 += byte_vals[1] as u32;

    // calculate temperature
    let mut temp10: u32 = byte_vals[2].into();
    temp10 = temp10 << 8;
    temp10 += byte_vals[3] as u32;

    println!("hum10={}, temp10={}", hum10, temp10);

    // range check for temperature
    if (temp10 < DHT22_MINVAL_TEMP10) || (temp10 > DHT22_MAXVAL_TEMP10) {
        error!(
            "temperature is out of range (measured {}, limits are {}, {})",
            (temp10 / 10),
            DHT22_MINVAL_TEMP10 / 10,
            DHT22_MAXVAL_TEMP10 / 10
        );
        return Err(DhtError::TemperatureOutOfRange);
    }

    // range check for humidity
    if (hum10 < DHT22_MINVAL_HUM10) || (hum10 > DHT22_MAXVAL_HUM10) {
        error!(
            "humidity is out of range (measured {}, limits are {}, {})",
            (hum10 / 10),
            DHT22_MINVAL_HUM10 / 10,
            DHT22_MAXVAL_HUM10 / 10
        );
        return Err(DhtError::HumidityOutOfRange);
    }

    // calculate final values
    let temperature: f32 = (temp10 / 10) as f32;
    let humidity: f32 = (hum10 / 10) as f32;

    Ok((temperature, humidity))
}
