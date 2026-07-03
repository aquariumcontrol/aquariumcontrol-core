/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the configuration structure for the automated freshwater refill system.
//!
//! This module provides the `RefillConfig` struct, which is used by `serde` to
//! deserialize the `[refill]` section of the main TOML configuration file. It holds
//! all the necessary parameters for the `Refill` thread, which intelligently
//! controls the water top-off process.
//!
//! ## Key Configuration Groups
//!
//! The parameters in `RefillConfig` can be grouped by their function:
//!
//! - **General Control**: Fields like `active`, `execute`, `initial_wait_interval`,
//!   and `check_interval` control the basic operation and timing of the refill thread.
//!
//! - **Safety Limits**: A crucial set of parameters (`max_refill_count_24h`,
//!   `max_refill_volume_hour`, `min_interval_last_refill`, etc.) that prevent
//!   over-filling the tank. If any of these historical limits are exceeded, refills
//!   are inhibited to prevent potential leaks or sensor failures from causing a flood.
//!
//! - **Error Detection**: The `max_duration_since_last_refill` field helps detect a
//!   "stuck high" sensor by flagging an error if an unusually long time passes
//!   without a refill request.
//!
//! - **Physical Properties**: Fields like `refill_pump_flow`, `max_refill_volume`, and
//!   `additional_refill_volume` describe the physical characteristics of the pump
//!   and the desired behavior of a single refill operation.

use serde_derive::Deserialize;

/// Holds the configuration data for the refill control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Clone)]
pub struct RefillConfig {
    /// control will only be active when the flag is set to true
    pub active: bool,

    /// indicates if the thread shall be started or not
    pub execute: bool,

    /// time interval in seconds which need to pass after initialization of the application before refill control is becoming active
    pub initial_wait_interval: u64,

    /// interval for the main control in seconds    
    pub check_interval: u64,

    /// the maximum number of permitted refill operations within the last 24h
    /// - If this number is exceeded, then the refill control will not further refill even if the water level sensor signal value is low.
    pub max_refill_count_24h: u64,

    /// maximum permitted refill volume within the last 24h
    /// - If this number is exceeded, then the refill control will not further refill even if the water level sensor signal value is low.
    pub max_refill_volume_24h: f64,

    /// the maximum number of permitted refill operations within the last hour
    /// - If this number is exceeded, then the refill control will not further refill even if the water level sensor signal value is low.
    pub max_refill_count_hour: u64,

    /// maximum permitted refill volume within the last hour
    /// - If this number is exceeded, then the refill control will not further refill even if the water level sensor signal value is low.
    pub max_refill_volume_hour: f64,

    /// minimum time which has to pass after a refill operation before a further refill operation is started
    pub min_interval_last_refill: u64,

    /// time interval in seconds which need to pass before the application recognizes a potential tank level switch fault
    pub max_duration_since_last_refill: u64,

    /// flow of the refill pump in liters / second
    pub refill_pump_flow: f64,

    /// maximum permitted refill volume in one refill operation
    pub max_refill_volume: f64,

    /// Refill volume which is added to the tank in a refill operation after the application recognizes tank level position high
    /// Background: This volume shall avoid too frequent refill operations. Too high values may lead to unstable water level.
    /// It is recommended to calibrate a value, which is a fraction of max_refill_volume.
    pub additional_refill_volume: f64,
}
