/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
        use log::error;
        use rppal::gpio::{Gpio, Level, Trigger};
        use std::thread::{self, sleep};
        use std::time::Duration;
        use crate::InternalCommand;
        use std::sync::mpsc::{Sender, Receiver};
        use crate::channel_content::AquariumSignal;

        const LOG_TARGET_DHT: &str = "dht";

        #[cfg(feature = "debug_dht")]
        use log::debug;
    }
}

cfg_if::cfg_if! {
   if #[cfg(all(not(test), not(all(target_os = "linux", feature = "target_hw"))))] {
        use log::error;
        const LOG_TARGET_DHT: &str = "dht";
   }
}

/// Holds the configuration data for the GPIO handler.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct DhtConfig {
    /// duration to keep pin down during start signal in microseconds
    #[allow(unused)]
    sensor_init_pin_down_duration_micros: u64,

    /// duration to keep pin up during start signal in microseconds
    #[allow(unused)]
    sensor_init_pin_up_duration_micros: u64,

    /// duration for sensor to respond after sending start signal in microseconds
    #[allow(unused)]
    sensor_init_input_duration_micros: u64,

    /// maximum permissible duration for sensor to respond in microseconds
    #[allow(unused)]
    timeout_duration_micros: u64,

    // duration in milliseconds the sensor is switched off before requesting transmission
    #[allow(unused)]
    sensor_reset_duration_millis: u64,

    // duration in milliseconds the sensor is given to initialize
    #[allow(unused)]
    sensor_startup_duration_millis: u64,

    // duration in milliseconds the sensor is given to pause between transmissions
    #[allow(unused)]
    sensor_pause_duration_millis: u64,
}

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

    // minimum and maximum value for temperature10 signal range check
    #[allow(unused)]
    pub const DHT22_MINVAL_TEMP10: u32 = 0;
    #[allow(unused)]
    pub const DHT22_MAXVAL_TEMP10: u32 = 500;

    // minimum and maximum value for temperature10 signal range check
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

    // maximum number of allowed retries to get a valid data set
    #[allow(unused)]
    pub const DHT22_MAX_RETRIES: u64 = 5;
}

#[derive(Debug, PartialEq)]
pub enum DhtError {
    /// Vcc pin acquisition failed
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    VccPinAcquisitionFailed,

    /// Data pin acquisition for output failed
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    DataPinOutputAcquisitionFailed,

    /// Data pin acquisition for input failed
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    DataPinInputAcquisitionFailed,

    /// Could not set interrupt
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    CouldNotSetInterrupt,

    /// Could not clear interrupt
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    CouldNotClearInterrupt,

    // execution of interrupt failed
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    InterruptFailed,

    // transmission yields wrong number of valid bits
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    WrongNumberOfValidTicks,

    // error occurred during calculation of checksum
    ChecksumCalculationError,

    // calculated checksum does not match received value
    ChecksumError,

    // timeout during transmission of data
    #[allow(unused)]
    Timeout,

    // temperature value is out of range
    TemperatureOutOfRange,

    // humidity value is out of range
    HumidityOutOfRange,
}

pub struct Dht {
    // configuration for communication with DHT sensor
    #[allow(unused)]
    config: DhtConfig,

    // reference to GPIO interface
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    gpio: Gpio,

    /// GPIO pin number for communication with DHT22 sensor
    #[allow(unused)]
    gpio_pin_number_dht22_io: u8,

    /// GPIO pin number for providing power to DHT22 sensor
    #[allow(unused)]
    gpio_pin_number_dht22_vcc: u8,
}

impl Dht {
    /// Provides a struct of type Dht - Linux platform specific version
    /// # Arguments    
    /// * `config` - configuration for communication with DHT sensor
    /// * `gpio` - reference to GPIO interface
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub fn new(
        config: DhtConfig,
        gpio: Gpio,
        gpio_pin_number_dht22_io: u8,
        gpio_pin_number_dht22_vcc: u8,
    ) -> Dht {
        Dht {
            config: config,
            gpio: gpio,
            gpio_pin_number_dht22_io: gpio_pin_number_dht22_io,
            gpio_pin_number_dht22_vcc: gpio_pin_number_dht22_vcc,
        }
    }

    /// calculates bit array from timing sequence of signal change
    /// # Arguments
    /// * `ticks` - timing information stored in array
    /// * `valid_ticks_received` - number valid (without timeout) interrupt events received
    #[allow(unused)]
    pub fn calc_data_bits(
        ticks: [u64; dht22_constants::DHT22_TRANSMISSION_END_IDX],
        valid_ticks_received: usize,
    ) -> [bool; dht22_constants::DHT22_BYTE4_STOP_IDX + 1] {
        let mut data_bits = [false; dht22_constants::DHT22_BYTE4_STOP_IDX + 1];
        let mut count_data_bits = 0;
        for i in 0..(valid_ticks_received + 1) {
            if ticks[i] > dht22_constants::DHT22_NEUTRAL_LEN_MIN
                && ticks[i] < dht22_constants::DHT22_NEUTRAL_LEN_MAX
            {
                continue;
            }
            if ticks[i] > dht22_constants::DHT22_BIT_TIMING_LIMIT {
                data_bits[count_data_bits] = true;
            }
            /*println!(
                "data_bit_len[{}] = {} => {}",
                count_data_bits,
                ticks[i],
                match data_bits[count_data_bits] {
                    true => "TRUE",
                    false => "FALSE",
                }
            );*/
            count_data_bits += 1;
            if count_data_bits > dht22_constants::DHT22_BYTE4_STOP_IDX {
                break;
            }
        }
        data_bits
    }

    #[allow(unused)]
    /// arranges bit information of one byte
    /// # Arguments
    /// * `data_bits` - array with bit data received from sensor
    pub fn calc_byte(
        data_bits: [bool; dht22_constants::DHT22_BYTE4_STOP_IDX + 1],
        start_index: usize,
        stop_index: usize,
    ) -> u8 {
        let mut byte_val: u8 = 0;
        for idx_bit in start_index..stop_index {
            byte_val |= match data_bits[idx_bit] {
                true => 1,
                false => 0,
            };
            byte_val = byte_val << 1;
        }
        byte_val |= match data_bits[stop_index] {
            true => 1,
            false => 0,
        };
        byte_val
    }

    #[allow(unused)]
    /// arranges bit information received from sensor into a byte array
    /// # Arguments
    /// * `data_bits` - array with bit data received from sensor
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
    /// calculates checksum from byte array
    /// # Arguments
    /// * `byte_vals` - array with data received from sensor
    pub fn calc_checksum(byte_vals: [u8; dht22_constants::DHT22_DATA_LEN]) -> Result<u8, DhtError> {
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
            #[cfg(not(test))]
            error!(
                target: LOG_TARGET_DHT,
                "checksum error (calculated {}, received {})",
                checksum_calculated, checksum_received
            );
            return Err(DhtError::ChecksumError);
        }
        Ok(checksum_calculated)
    }

    #[allow(unused)]
    /// calculates temperature and humidity values from byte array
    /// # Arguments
    /// * `byte_vals` - array with data received from sensor
    pub fn calc_signals(
        byte_vals: [u8; dht22_constants::DHT22_DATA_LEN],
    ) -> Result<(f32, f32), DhtError> {
        // calculate integer value for humidity * 10
        let mut hum10: u32 = byte_vals[0].into();
        hum10 = hum10 << 8;
        hum10 += byte_vals[1] as u32;

        // calculate integer value for temperature * 10
        let mut temp10: u32 = byte_vals[2].into();
        temp10 = temp10 << 8;
        temp10 += byte_vals[3] as u32;

        //println!("hum10={}, temp10={}", hum10, temp10);

        // range check for temperature
        if (temp10 < dht22_constants::DHT22_MINVAL_TEMP10)
            || (temp10 > dht22_constants::DHT22_MAXVAL_TEMP10)
        {
            #[cfg(not(test))]
            error!(
                "temperature is out of range (measured {}, limits are {}, {})",
                (temp10 / 10),
                dht22_constants::DHT22_MINVAL_TEMP10 / 10,
                dht22_constants::DHT22_MAXVAL_TEMP10 / 10
            );
            return Err(DhtError::TemperatureOutOfRange);
        }

        // range check for humidity
        if (hum10 < dht22_constants::DHT22_MINVAL_HUM10)
            || (hum10 > dht22_constants::DHT22_MAXVAL_HUM10)
        {
            #[cfg(not(test))]
            error!(
                "humidity is out of range (measured {}, limits are {}, {})",
                (hum10 / 10),
                dht22_constants::DHT22_MINVAL_HUM10 / 10,
                dht22_constants::DHT22_MAXVAL_HUM10 / 10
            );
            return Err(DhtError::HumidityOutOfRange);
        }

        // calculate final values
        let temperature: f32 = (temp10 as f32) / 10.0;
        let humidity: f32 = (hum10 as f32) / 10.0;

        Ok((temperature, humidity))
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// reads temperature and humity from DHT sensor
    /// # Arguments
    /// * `reset_sensor` - flag to indicate if power supply to sensor shall be interrupted before measuring
    pub fn read(&self, reset_sensor: bool) -> Result<(f32, f32), DhtError> {
        let mut ticks = [0u64; dht22_constants::DHT22_TRANSMISSION_END_IDX];

        // get pin for providing supply voltage to DHT sensor
        let mut dht_vcc_pin = match self.gpio.get(self.gpio_pin_number_dht22_vcc) {
            Ok(c) => c.into_output(),
            #[cfg(not(test))]
            Err(e) => {
                error!(
                    target: LOG_TARGET_DHT,
                    "could not get pin {} from GPIO interface for DHT power supply ({e:?})",
                    self.gpio_pin_number_dht22_vcc
                );
                return Err(DhtError::VccPinAcquisitionFailed);
            }
            #[cfg(test)]
            Err(_) => {
                return Err(DhtError::VccPinAcquisitionFailed);
            }
        };

        // scope to force drop of output pin
        {
            // get pin for triggering DHT sensor
            let mut data_pin = match self.gpio.get(self.gpio_pin_number_dht22_io) {
                Ok(c) => c.into_output(),
                #[cfg(not(test))]
                Err(e) => {
                    error!(
                        target: LOG_TARGET_DHT,
                        "failed to aquire pin {} for triggering DHT sensor ({e:?})",
                        self.gpio_pin_number_dht22_io,
                    );
                    return Err(DhtError::DataPinOutputAcquisitionFailed);
                }
                #[cfg(test)]
                Err(_) => {
                    return Err(DhtError::DataPinOutputAcquisitionFailed);
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
            // give sensor some time to wake up
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
        let mut data_pin = match self.gpio.get(self.gpio_pin_number_dht22_io) {
            Ok(c) => c.into_input_pullup(),
            Err(e) => {
                error!(
                    "failed to aquire pin {} for reading from DHT sensor ({e:?})",
                    self.gpio_pin_number_dht22_io,
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

        let timeout_duration = Duration::from_micros(self.config.timeout_duration_micros);

        let mut timeout_occurred = false;
        let mut valid_ticks_received: usize = 0;

        // initialization to avoid compiler error
        let mut duration_before: Duration = Duration::from_micros(0);
        let mut duration_after: Duration;

        // observe level changes and record the timing
        for i in 0..dht22_constants::DHT22_TRANSMISSION_END_IDX {
            // poll the interrupt
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
                    /*warn!(
                        target: LOG_TARGET_DHT,
                        "empty interrupt event for bit #{}",
                        i
                    );*/
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
            /*for i in 0..dht22_constants::DHT22_TRANSMISSION_END_IDX {
                println!("ticks[{}] = {}", i, ticks[i]);
            }*/
            return Err(DhtError::Timeout);
        }

        if valid_ticks_received != dht22_constants::DHT22_TRANSMISSION_END_IDX - 1 {
            /*for i in 0..dht22_constants::DHT22_TRANSMISSION_END_IDX {
                println!("ticks[{}] = {}", i, ticks[i]);
            }*/
            error!(
                target: LOG_TARGET_DHT,
                "incorrect number of balid ticks received: target={}, measured={}",
                dht22_constants::DHT22_TRANSMISSION_END_IDX - 1,
                valid_ticks_received
            );
            return Err(DhtError::WrongNumberOfValidTicks);
        }

        // identify the data bits and store values in data_bits
        let data_bits = Self::calc_data_bits(ticks, valid_ticks_received);

        let byte_vals = Self::calc_byte_array(data_bits);

        /*for i in 0..dht22_constants::DHT22_DATA_LEN {
            println!("byte_vals[{}] = {}", i, byte_vals[i]);
        }*/

        // checksum calculation
        match Self::calc_checksum(byte_vals) {
            Ok(_) => { /* do nothing */ }
            Err(e) => return Err(e),
        }

        // calculate signals (or error code) and directly return it
        Self::calc_signals(byte_vals)
    }

    /// initiates reading and postprocesses data read from sensor
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub fn get_data(&self) -> Result<(f32, f32), DhtError> {
        // read one sample from DHT sensor to verify SW and HW are working ok
        let mut measurement_successful = false;
        let mut retry_counter = 0;

        let mut result_tuple: (f32, f32) = (0.0, 0.0);
        let mut dht_error = DhtError::Timeout; // to avoid compiler error. Value will be overwritten in error case.

        while !measurement_successful {
            match self.read(
                retry_counter == 0, /* reset power supply for first read attempt */
            ) {
                Ok((temperature, humidity)) => {
                    measurement_successful = true;

                    #[cfg(feature = "debug_dht")]
                    debug!(
                        target: LOG_TARGET_DHT,
                        "measured successfully: temperature={}, humidity={}",
                        temperature,
                        humidity
                    );

                    result_tuple = (temperature, humidity);
                }
                Err(e) => {
                    if (e == DhtError::Timeout) || (e == DhtError::ChecksumError) {
                        retry_counter += 1;
                        if retry_counter > dht22_constants::DHT22_MAX_RETRIES {
                            #[cfg(not(test))]
                            error!(
                                target: LOG_TARGET_DHT,
                                "failed to read from DHT sensor (after {} retries, {e:?}",
                                retry_counter,
                            );
                            dht_error = e;
                            break;
                        }
                    } else {
                        #[cfg(not(test))]
                        error!(
                            target: LOG_TARGET_DHT,
                            "failed to read from DHT sensor ({e:?})",
                        );
                        return Err(e);
                    }
                }
            }
        }
        if measurement_successful {
            Ok(result_tuple)
        } else {
            Err(dht_error)
        }
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// receives and response to requests via channel        
    pub fn execute(
        &mut self,
        tx_to_ambient: Sender<Result<(f32, f32), DhtError>>,
        rx_from_ambient_or_signal_handler: Receiver<InternalCommand>,
    ) {
        let mut lock_error_invalid_command: bool = false;
        let mut lock_error_invalid_signal: bool = false;
        let mut lock_error_receive: bool = false;
        let mut lock_error_send: bool = false;

        loop {
            match rx_from_ambient_or_signal_handler.recv() {
                Ok(c) => {
                    lock_error_receive = false;
                    match c {
                        InternalCommand::RequestSignal(signal_request) => {
                            lock_error_invalid_command = false;
                            // Ambient requests either temperature or humidity
                            match signal_request {
                                AquariumSignal::AmbientHumidity
                                | AquariumSignal::AmbientTemperature => {
                                    let measurement = self.get_data();
                                    match tx_to_ambient.send(measurement) {
                                        Ok(()) => {
                                            #[cfg(feature = "debug_dht")]
                                            debug!(
                                                target: LOG_TARGET_DHT,
                                                "sent data to Ambient via channel"
                                            );
                                        }
                                        Err(e) => {
                                            if !lock_error_send {
                                                error!(
                                                    target: LOG_TARGET_DHT,
                                                    "Error occurred when sending data to Ambient ({e:?})"
                                                );
                                                lock_error_send = true;
                                            }
                                        }
                                    }
                                }
                                _ => {
                                    if !lock_error_invalid_signal {
                                        error!(
                                            target: LOG_TARGET_DHT,
                                            "Received request for invalid signal."
                                        );
                                        lock_error_invalid_signal = true;
                                    }
                                }
                            }
                        }
                        InternalCommand::Quit => {
                            #[cfg(feature = "debug_dht")]
                            debug!(
                                target: LOG_TARGET_DHT,
                                "Received QUIT command"
                            );
                            break;
                        }
                        _ => {
                            if !lock_error_invalid_command {
                                error!(
                                    target: LOG_TARGET_DHT,
                                    "Received invalid command."
                                );
                                lock_error_invalid_command = true;
                            }
                        }
                    };
                }
                Err(e) => {
                    if !lock_error_receive {
                        error!(
                            target: LOG_TARGET_DHT,
                            "Error in trying to receive command from Ambient or Signal handler: {e:?}"
                        );
                        lock_error_receive = true;
                    }
                }
            };
        }
    }
}

#[cfg(test)]
pub mod tests {
    use assert_float_eq::*;

    use crate::dht::{dht22_constants, Dht, DhtError};
    use crate::logger::setup_logger;

    // feature-specific imports
    cfg_if::cfg_if! {
        if #[cfg(all(feature = "target_hw", target_os = "linux"))] {
            use crate::config::{read_config_file, ConfigData};
            use rppal::gpio::Gpio;
        }
    }

    #[test]
    // Test case checks if the implementation correctly processes the bit information into a byte array
    // Test case does not require any communication with SQL database.
    pub fn test_dht_calc_byte_array() {
        let stimuli: [bool; 40] = [
            false, // initial interrupt - to be ignored
            false, false, false, false, false, false, true, false, // Byte 0
            true, true, false, false, true, true, false, true, // Byte 1
            false, false, false, false, false, false, false, // Byte 2 - 7 bits only
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
    // Test case does not require any communication with SQL database.
    pub fn test_dht_calc_checksum_valid() {
        setup_logger();

        let stimuli: [u8; dht22_constants::DHT22_DATA_LEN] = [2, 205, 0, 245, 196];

        let result = Dht::calc_checksum(stimuli);

        assert_eq!(result, Ok(196));
    }

    #[test]
    // Test case checks if the implementation correctly calculates an invalid checksum.
    // Test case does not require any communication with SQL database.
    pub fn test_dht_calc_checksum_error() {
        setup_logger();

        let stimuli: [u8; dht22_constants::DHT22_DATA_LEN] = [255, 255, 255, 255, 10];

        let result = Dht::calc_checksum(stimuli);

        assert_eq!(result, Err(DhtError::ChecksumError));
    }

    #[test]
    // Test case checks if the implementation correctly calculates the signals from the byte array
    // given valid input data.
    // Test case does not require any communication with SQL database.
    pub fn test_dht_calc_signals_valid() {
        setup_logger();

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
    // Test case does not require any communication with SQL database.
    pub fn test_dht_calc_signals_humidity_too_high() {
        setup_logger();

        let stimuli: [u8; dht22_constants::DHT22_DATA_LEN] = [4, 205, 0, 245, 198];

        assert_eq!(
            Dht::calc_signals(stimuli),
            Err(DhtError::HumidityOutOfRange)
        );
    }

    #[test]
    // Test case checks if the implementation correctly calculates the signals from the byte array
    // given invalid input data for the temperature.
    // Test case does not require any communication with SQL database.
    pub fn test_dht_calc_signals_temperature_too_high() {
        setup_logger();

        let stimuli: [u8; dht22_constants::DHT22_DATA_LEN] = [2, 205, 1, 245, 197];

        assert_eq!(
            Dht::calc_signals(stimuli),
            Err(DhtError::TemperatureOutOfRange)
        );
    }

    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    #[test]
    // Test case executes a read operation from real hardware.
    // Test case does not require any communication with SQL database.
    pub fn test_dht_read() {
        setup_logger();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());

        // open interface to GPIO library
        let gpio = match Gpio::new() {
            Ok(c) => c,
            Err(e) => {
                panic!("test_dht_read: could not open interface to GPIO ({e:?})",);
            }
        };

        let gpio_pin_number_dht22_io = config.gpio_handler.gpio_pin_number_dht22_io;
        let gpio_pin_number_dht22_vcc = config.gpio_handler.gpio_pin_number_dht22_vcc;

        let dht = Dht::new(
            config.dht,
            gpio,
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
    // Test case does not require any communication with SQL database.
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
