/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use serde_derive::Deserialize;

/// Holds the configuration data for interacting with GPIO
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct DhtConfig {
    /// allows deactivation of the data acquisition
    #[allow(unused)] // used in conditionally compiled code
    pub active: bool,

    /// indicates if the thread shall be started or not
    #[cfg(feature = "target_hw")]
    pub execute: bool,

    /// duration to keep pin down during start signal in microseconds
    #[allow(unused)]
    pub sensor_init_pin_down_duration_micros: u64,

    /// duration to keep pin up during start signal in microseconds
    #[allow(unused)]
    pub sensor_init_pin_up_duration_micros: u64,

    /// duration for sensor to respond after sending start signal in microseconds
    #[allow(unused)]
    pub sensor_init_input_duration_micros: u64,

    /// maximum permissible duration for the sensor to respond in microseconds
    #[allow(unused)]
    pub timeout_duration_micros: u64,

    /// duration in milliseconds the sensor is switched off before requesting transmission
    #[allow(unused)]
    pub sensor_reset_duration_millis: u64,

    /// duration in milliseconds the sensor is given to initialize
    #[allow(unused)]
    pub sensor_startup_duration_millis: u64,

    /// duration in milliseconds the sensor is given to pause between transmissions
    #[allow(unused)]
    pub sensor_pause_duration_millis: u64,

    /// duration in milliseconds between two measurements
    #[allow(unused)] // used in conditionally compiled code
    pub measurement_interval_millis: u64,
}
