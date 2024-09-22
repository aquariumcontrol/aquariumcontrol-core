/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::error;
use rppal::gpio::{Gpio, Level, Trigger};
use serde_derive::Deserialize;
use std::thread;
use std::thread::sleep;
use std::time::Duration;
use std::time::SystemTime;

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
}

// Excerpt from AM2302 Product Manual, AOSONG
// www.aosong.com
// Response time from sensor from Table 6: Single bus signal characteristics

// Symbol       Parameter                               min     typ     max     Unit
//-----------------------------------------------------------------------------------

// TH0          Signal "0" high time	                22      26      30      micS
const DHT22_T_H0_MIN: u64 = 22;
const DHT22_T_H0_MAX: u64 = 30;

// TH1          Signal "1" high time                    68      70      75      micS
const DHT22_T_H1_MIN: u64 = 42;
const DHT22_T_H1_MAX: u64 = 179;

const DHT22_T_LATENCY: u64 = 5;

const DHT22_TRANSMISSION_END_IDX: usize = 86;
const DHT22_DATA_LEN: usize = 5;

// minimum and maximum value for temperature10 signal range check
const DHT22_MINVAL_TEMP10: i32 = -500;
const DHT22_MAXVAL_TEMP10: i32 = 500;

// minimum and maximum value for temperature10 signal range check
const DHT22_MINVAL_HUM10: i32 = -10;
const DHT22_MAXVAL_HUM10: i32 = 1000;

#[derive(Debug, PartialEq)]
pub enum DhtError {
    /// Vcc pin acquisition failed
    VccPinAcquisitionFailed,

    /// Data pin acquisition for output failed
    DataPinOutputAcquisitionFailed,

    /// Data pin acquisition for input failed
    DataPinInputAcquisitionFailed,

    /// Could not set interrupt
    CouldNotSetInterrupt,

    /// Could not clear interrupt
    CouldNotClearInterrupt,

    // Could not measure interrupt duration
    CouldNotMeasureInterruptDuration,

    // execution of interrupt failed
    InterruptFailed,

    // bit timing is not within global interval
    BitTimingNotWithinGlobalInterval,

    // timing not within specific interval for high value
    BitTimingNotWithinSpecificInterval,

    // error occurred during calculation of checksum
    ChecksumCalculationError,

    // calculated checksum does not match received value
    ChecksumError,

    // timeout during transmission of data
    Timeout,

    // interrupt handling returned empty event
    EmptyEvent,

    // temperature value is out of range
    TemperatureOutOfRange,

    // humidity value is out of range
    HumidityOutOfRange,
}

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
    let mut byte_vals = [0u8; DHT22_DATA_LEN];

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
            thread::sleep(Duration::from_millis(500));
            dht_vcc_pin.set_high();
            thread::sleep(Duration::from_millis(1000)); // give sensor some time to wake up
        } else {
            thread::sleep(Duration::from_millis(500)); // pause between transmissions
        }

        // send start signal
        data_pin.write(Level::Low);
        sleep(Duration::from_micros(
            config.sensor_init_pin_down_duration_micros,
        ));
        data_pin.write(Level::High);
        //        sleep(Duration::from_micros(
        //            config.sensor_init_pin_up_duration_micros,
        //        ));
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

    // duration for debouncing of interrupts
//    let debounce_duration_opt = Some(Duration::from_micros(1));

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
            Ok(c) => {c}
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

        // abort measuring, if timeout occurred
        if ticks[i] > config.timeout_duration_micros {
            timeout_occurred = true;
            break;
        }
        valid_ticks_received = i;
    }

    // output of tick timing to terminal for development purposes
    for k in 0..(valid_ticks_received + 1) {
        println!("ticks[{}]={}", k, ticks[k]);
    }
    println!("**************************");

    match data_pin.clear_interrupt() {
        Ok(()) => {}
        Err(_) => {
            return Err(DhtError::CouldNotClearInterrupt);
        }
    }

    if timeout_occurred {
        return Err(DhtError::Timeout);
    }

    // evaluate bit lengths and compute transmission values
    let mut idx_bit = 0;
    let mut idx_byte = 0;
    let mut i: usize = 0;

    while i < DHT22_TRANSMISSION_END_IDX {
        // part 1: check level and if bit timing is within global interval
        if (ticks[i] < DHT22_T_H0_MIN - 4 * DHT22_T_LATENCY)
            || (ticks[i] > DHT22_T_H1_MAX + 4 * DHT22_T_LATENCY)
        {
            error!(
                target: LOG_TARGET_DHT,
                "bit timing is not within global interval (measured {}, limits are {}, {})",
                ticks[i], DHT22_T_H0_MIN - 4 * DHT22_T_LATENCY, DHT22_T_H1_MAX + 4 * DHT22_T_LATENCY
            );
            return Err(DhtError::BitTimingNotWithinGlobalInterval);
        }

        // part 2: check separate intervals for low and high bit
        if (ticks[i] >= DHT22_T_H0_MIN - 2 * DHT22_T_LATENCY)
            && (ticks[i] <= DHT22_T_H0_MAX + 2 * DHT22_T_LATENCY)
        {
            byte_vals[idx_byte % DHT22_DATA_LEN] = byte_vals[idx_byte % DHT22_DATA_LEN] << 1;
        } else {
            if (ticks[i] >= DHT22_T_H1_MIN - 2 * DHT22_T_LATENCY)
                && (ticks[i] <= DHT22_T_H1_MAX + 2 * DHT22_T_LATENCY)
            {
                byte_vals[idx_byte % DHT22_DATA_LEN] = byte_vals[idx_byte % DHT22_DATA_LEN] << 1;
                byte_vals[idx_byte % DHT22_DATA_LEN] |= 1;
            } else {
                error!(
                    target: LOG_TARGET_DHT,
                    "timing not within specific interval for high value (measured {}, limits are {}, {})",
                    ticks[i], DHT22_T_H1_MIN - 2 * DHT22_T_LATENCY, DHT22_T_H1_MAX + 2 * DHT22_T_LATENCY
                );
                return Err(DhtError::BitTimingNotWithinSpecificInterval);
            }
        }

        if idx_bit >= 7 {
            idx_bit = 0;
            idx_byte += 1;
        } else {
            idx_bit += 1;
        }

        i += 1;
    }

    for i in 0..DHT22_DATA_LEN {
        println!("byte_vals[{}] = {}", i, byte_vals[i]);
    }

    // transfer data into signal variables
    let mut hum10: i32 = byte_vals[0].into();
    hum10 = hum10 << 8;
    hum10 += byte_vals[1] as i32;

    let mut temp10: i32 = byte_vals[2].into();
    temp10 = temp10 << 8;
    temp10 += byte_vals[3] as i32;

    println!("hum10={}, temp10={}", hum10, temp10);

    // calculate and compare checksum
    let mut checksum_intermediate: u16 =
        byte_vals[0] as u16 + byte_vals[1] as u16 + byte_vals[2] as u16 + byte_vals[3] as u16;
    checksum_intermediate = checksum_intermediate & 0xFF;
    let checksum_calculated: u8 = match ((checksum_intermediate << (16 - 8)) >> (16 - 8)).try_into()
    {
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
