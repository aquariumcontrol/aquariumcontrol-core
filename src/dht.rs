/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::error;
use rppal::gpio::{Bias, IoPin, Level, Mode, OutputPin};
use serde_derive::Deserialize;
use std::thread;
use std::thread::sleep;
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
    pub timeout_duration_micros: u32,
}

// Excerpt from AM2302 Product Manual, AOSONG
// www.aosong.com
// Response time from sensor from Table 6: Single bus signal characteristics

// Symbol       Parameter                               min     typ     max     Unit
//-----------------------------------------------------------------------------------
// Tbe          Host the start signal down time         0.8     1       20      mS
const DHT22_T_BE_IDX: usize = 1;
const DHT22_T_BE_MIN: u32 = 800;
const DHT22_T_BE_MAX: u32 = 20000;
const DHT22_T_BE_LVL: bool = true;

// Tgo          Bus master has released time            20      30      200     micS
const DHT22_T_GO_IDX: usize = 2;
const DHT22_T_GO_MIN: u32 = 20;
const DHT22_T_GO_MAX: u32 = 200;
const DHT22_T_GO_LVL: bool = false;

// Trel         Response to low time                    75      80      85      micS
const DHT22_T_REL_IDX: usize = 3;
const DHT22_T_REL_MIN: u32 = 75;
const DHT22_T_REL_MAX: u32 = 85;
const DHT22_T_REL_LVL: bool = true;

// Treh         In response to high time                75      80      85      micS
const DHT22_T_REH_IDX: usize = 4;
const DHT22_T_REH_MIN: u32 = 75;
const DHT22_T_REH_MAX: u32 = 85;
const DHT22_T_REH_LVL: bool = false;

// TLOW         Signal "0", "1" low time                48      50      55      micS
const DHT22_T_LOW_MIN: u32 = 48;
const DHT22_T_LOW_MAX: u32 = 55;
const DHT22_T_LOW_LVL: bool = true;

// TH0          Signal "0" high time                    22      26      30      micS
const DHT22_T_H0_MIN: u32 = 22;
const DHT22_T_H0_MAX: u32 = 30;
const DHT22_T_H0_LVL: bool = false;

// TH1          Signal "1" high time                    68      70      75      micS
const DHT22_T_H1_MIN: u32 = 68;
const DHT22_T_H1_MAX: u32 = 70;

// Ten          Sensor to release the bus time          45      50      55      micS
const DHT22_T_EN_IDX: usize = 85;
const DHT22_T_EN_MIN: u32 = 45;
const DHT22_T_EN_MAX: u32 = 55;
const DHT22_T_EN_LVL: bool = true;

const DHT22_T_LATENCY: u32 = 5;

// minimum and maximum value for temperature10 signal range check
const DHT22_MINVAL_TEMP10: i32 = -500;
const DHT22_MAXVAL_TEMP10: i32 = 500;

// minimum and maximum value for temperature10 signal range check
const DHT22_MINVAL_HUM10: i32 = -10;
const DHT22_MAXVAL_HUM10: i32 = 1000;

#[derive(Debug)]
pub enum DhtError {
    /// timeout happened during data transmission
    Timeout,

    /// data transmission is not starting with low level
    DataNotStartingWithFalse,

    /// start signal pin down down time is out of range
    StartSignalDownTimeOutOfRange,

    // bus master release time is out of range
    BusMasterReleaseTimeOutOfRange,

    // response to low time is out of range
    ResponseToLowTimeOutOfRange,

    // response to high time is out of range
    ResponseToHighTimeOutOfRange,

    // sensor bus release time is out of range
    SensorBusReleaseTimeOutOfRange,

    // Signal 0->1 time is out of range
    Signal0To1TimeOutOfRange,

    // bit timing is not within global interval
    BitTimingNotWithinGlobalInterval,

    // timing not within specific interval for high value
    BitTimingNotWithinSpecificInterval,

    // calculated checksum does not match received value
    ChecksumError,

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
    vcc_pin: &mut OutputPin,
    data_pin: &mut IoPin,
) -> Result<(f32, f32), DhtError> {
    let mut ticks = [0u32; 86];
    let mut byte_vals = [0u8; 5];
    let mut lvls = [false; 86];
    let mut tick_counter: u32 = 0;
    let mut last_state: bool;
    let sleep_time = Duration::from_micros(1);	// sleep time between monitoring pin state

    data_pin.set_mode(Mode::Output);
    data_pin.write(Level::High);
    // switch on power supply of DHT sensor
    #[cfg(target_os = "linux")]
    vcc_pin.set_low();
    thread::sleep(Duration::from_secs(1));
    vcc_pin.set_high();
    // give sensor some time to wake up
    thread::sleep(Duration::from_secs(1));

    // send start signal
    data_pin.write(Level::Low);
    sleep(Duration::from_micros(
        config.sensor_init_pin_down_duration_micros,
    ));
    data_pin.write(Level::High);
    sleep(Duration::from_micros(
        config.sensor_init_pin_up_duration_micros,
    ));

    // configure pin for subsequent reading
    data_pin.set_bias(Bias::PullUp);
    data_pin.set_mode(Mode::Input);

    // give pin some time to initialize
    sleep(Duration::from_micros(
        config.sensor_init_input_duration_micros,
    ));

    last_state = data_pin.is_high();

    // observe level changes and record the timing
    for i in 0..85 {
        while data_pin.is_high() == last_state {
            tick_counter += 1;
            sleep(sleep_time);
            if tick_counter > config.timeout_duration_micros {
                for k in 0..i {
                    println!("lvs[{}]={} ticks[{}]={}",
                        k,
                        match lvls[k] {
                            false => "LOW",
                            true => "HIGH",
                        },
                        k,
                        ticks[k]
                    );
                }
                error!(
                    target: LOG_TARGET_DHT,
                    "timeout during data transmission of bit {} (limit={})",
                    i,
                    config.timeout_duration_micros
                );
                return Err(DhtError::Timeout);
            }
        }
        lvls[i] = data_pin.is_high();
        last_state = lvls[i];
        ticks[i] = tick_counter;
        tick_counter = 0;
    }

    // transmission has to start with level false
    if lvls[0] {
        error!(
            target: LOG_TARGET_DHT,
            "data transmission is not starting with low level"
        );
        return Err(DhtError::DataNotStartingWithFalse);
    }

    // host the start signal down time
    if (lvls[DHT22_T_BE_IDX] != DHT22_T_BE_LVL)
        || (ticks[DHT22_T_BE_IDX] < DHT22_T_BE_MIN - DHT22_T_LATENCY)
        || (ticks[DHT22_T_BE_IDX] > DHT22_T_BE_MAX + DHT22_T_LATENCY)
    {
        error!(
            target: LOG_TARGET_DHT,
            "start signal pin down down time is out of range (measured {}, limits are {}, {})",
            ticks[DHT22_T_BE_IDX],
            DHT22_T_BE_MIN - DHT22_T_LATENCY,
            DHT22_T_BE_MAX + DHT22_T_LATENCY
        );
        return Err(DhtError::StartSignalDownTimeOutOfRange);
    }

    // bus master release time
    if (lvls[DHT22_T_GO_IDX] != DHT22_T_GO_LVL)
        || (ticks[DHT22_T_GO_IDX] < DHT22_T_GO_MIN - DHT22_T_LATENCY)
        || (ticks[DHT22_T_GO_IDX] > DHT22_T_GO_MAX + DHT22_T_LATENCY)
    {
        error!(
            target: LOG_TARGET_DHT,
            "bus master release time is out of range (measured {}, limits are {}, {})",
            ticks[DHT22_T_GO_IDX],
            DHT22_T_GO_MIN - DHT22_T_LATENCY,
            DHT22_T_GO_MAX + DHT22_T_LATENCY
        );
        return Err(DhtError::BusMasterReleaseTimeOutOfRange);
    }

    // response to low time
    if (lvls[DHT22_T_REL_IDX] != DHT22_T_REL_LVL)
        || (ticks[DHT22_T_REL_IDX] < DHT22_T_REL_MIN - DHT22_T_LATENCY)
        || (ticks[DHT22_T_REL_IDX] > DHT22_T_REL_MAX + DHT22_T_LATENCY)
    {
        error!(
            target: LOG_TARGET_DHT,
            "response to down time is out of range (measured {}, limits are {}, {})",
            ticks[DHT22_T_REL_IDX],
            DHT22_T_REL_MIN - DHT22_T_LATENCY,
            DHT22_T_REL_MAX + DHT22_T_LATENCY
        );
        return Err(DhtError::ResponseToLowTimeOutOfRange);
    }

    // response to high time
    if (lvls[DHT22_T_REH_IDX] != DHT22_T_REH_LVL)
        || (ticks[DHT22_T_REH_IDX] < DHT22_T_REH_MIN - DHT22_T_LATENCY)
        || (ticks[DHT22_T_REH_IDX] > DHT22_T_REH_MAX + DHT22_T_LATENCY)
    {
        error!(
            target: LOG_TARGET_DHT,
            "response to high time is out of range (measured {}, limits are {}, {})",
            ticks[DHT22_T_REH_IDX],
            DHT22_T_REH_MIN - DHT22_T_LATENCY,
            DHT22_T_REH_MAX + DHT22_T_LATENCY
        );
        return Err(DhtError::ResponseToHighTimeOutOfRange);
    }

    // sensor bus release time
    if (lvls[DHT22_T_EN_IDX] != DHT22_T_EN_LVL)
        || (ticks[DHT22_T_EN_IDX] < DHT22_T_EN_MIN - DHT22_T_LATENCY)
        || (ticks[DHT22_T_EN_IDX] > DHT22_T_EN_MAX + DHT22_T_LATENCY)
    {
        error!(
            target: LOG_TARGET_DHT,
            "sensor bus release time is out of range (measured {}, limits are {}, {})",
            ticks[DHT22_T_GO_IDX],
            DHT22_T_EN_MIN - DHT22_T_LATENCY,
            DHT22_T_EN_MAX + DHT22_T_LATENCY
        );
        return Err(DhtError::SensorBusReleaseTimeOutOfRange);
    }

    // signal "0" -> "1" low time
    for i in (DHT22_T_REH_IDX + 1..DHT22_T_EN_IDX + 1).step_by(2) {
        if (lvls[i] != DHT22_T_LOW_LVL)
            || (ticks[i] < DHT22_T_LOW_MIN - 4 * DHT22_T_LATENCY)
            || (ticks[i] > DHT22_T_LOW_MAX + 4 * DHT22_T_LATENCY)
        {
            error!(
                target: LOG_TARGET_DHT,
                "signal 0->1 time is out of range (measured {}, limits are {}, {})",
                ticks[i],
                DHT22_T_LOW_MIN - 4 * DHT22_T_LATENCY,
                DHT22_T_LOW_MAX + 4 * DHT22_T_LATENCY
            );
            return Err(DhtError::Signal0To1TimeOutOfRange);
        }
    }

    // evaluate bit lengths and compute transmission values
    let mut idx_bit = 0;
    let mut idx_byte = 0;

    let mut i: usize = DHT22_T_REH_IDX + 2;
    while i < DHT22_T_EN_IDX {
        // part 1: check level and if bit timing is within global interval
        if (lvls[i] != DHT22_T_H0_LVL)
            || (ticks[i] < DHT22_T_H0_MIN - 4 * DHT22_T_LATENCY)
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
            byte_vals[idx_byte] = byte_vals[idx_byte] << 1;
        } else {
            if (ticks[i] >= DHT22_T_H1_MIN - 2 * DHT22_T_LATENCY)
                && (ticks[i] <= DHT22_T_H1_MAX + 2 * DHT22_T_LATENCY)
            {
                byte_vals[idx_byte] = byte_vals[idx_byte] << 1;
                byte_vals[idx_byte] |= 1;
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

        i += 2;
    }

    // transfer data into signal variables
    let mut hum10: i32 = byte_vals[0].into();
    hum10 = hum10 << 8;
    hum10 += byte_vals[1] as i32;

    let mut temp10: i32 = byte_vals[2].into();
    temp10 = temp10 << 8;
    temp10 += byte_vals[3] as i32;

    // calculate and compare checksum
    let checksum_calculated: i32 =
        ((byte_vals[0] + byte_vals[1] + byte_vals[2] + byte_vals[3]) & 0xFF).into();
    let checksum_received: i32 = byte_vals[4].into();
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
