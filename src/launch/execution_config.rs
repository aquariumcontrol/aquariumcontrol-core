/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines a lightweight snapshot of which application threads are configured to run.
//!
//! This module provides the `ExecutionConfig` struct, which is created once at application
//! startup. Its sole purpose is to hold a simple boolean flag for each major,
//! concurrently running module (like `feed`, `refill`, `heating`, etc.), indicating
//! whether it was intended to be executed.
//!
//! ## Design and Purpose
//!
//! The primary role of `ExecutionConfig` is to act as a decoupled, easily passable
//! source of truth about the runtime configuration.
//!
//! - **Decoupling**: Instead of passing the entire, heavy `ConfigData` object throughout
//!   the application, threads can be given this much simpler struct. This is
//!   particularly useful for cross-cutting concerns like the IPC `messaging` dispatcher,
//!   which needs to know if a target thread exists without needing to know its specific
//!   configuration details.
//!
//! - **Simplicity**: It provides a direct and unambiguous way to check if a thread was
//!   launched. The `new()` constructor handles the one-time translation from the
//!   more complex `ConfigData` struct.
//!
//! - **Testability**: The `Default` implementation provides a convenient way to create
//!   an `ExecutionConfig` where all modules are considered active, which is useful
//!   for setting up test environments.

use crate::utilities::config::ConfigData;

/// A snapshot of which application threads are configured to be active at runtime.
///
/// This struct is created at application startup from the main `ConfigData`. It serves
/// as a lightweight, easily passable representation of the execution state, allowing
/// different parts of the application (like the messaging dispatcher) to quickly
/// check if a specific thread or module was launched without needing access to the
/// full configuration object.
#[derive(Clone, Debug)]
pub struct ExecutionConfig {
    pub relay_manager: bool,
    pub data_logger: bool,
    pub refill: bool,
    pub heating: bool,
    pub tank_level_switch: bool,
    pub atlas_scientific: bool,
    pub sensor_manager: bool,
    pub feed: bool,
    pub balling: bool,
    pub monitors: bool,
    pub dht: bool,
    #[cfg(feature = "target_hw")]
    pub i2c_interface: bool,
    pub ventilation: bool,
    pub temperature_gradient: bool,
    pub schedule_check: bool,
    pub ds18b20: bool,
    pub watchdog: bool,
    pub tcp_communication: bool,
}

impl ExecutionConfig {
    /// Creates a new `ExecutionConfig` based on the application's configuration file.
    ///
    /// This constructor reads the `execute` flag from each module's configuration
    /// section within the provided `ConfigData` to build the execution state.
    ///
    /// # Arguments
    /// * `config` - A reference to the fully parsed application configuration data.
    pub fn new(config: &ConfigData) -> Self {
        Self {
            relay_manager: config.relay_manager.execute,
            data_logger: config.data_logger.execute,
            refill: config.refill.execute,
            heating: config.heating.execute,
            tank_level_switch: config.tank_level_switch.execute,
            atlas_scientific: config.atlas_scientific.execute,
            sensor_manager: config.sensor_manager.execute,
            feed: config.feed.execute,
            balling: config.balling.execute,
            monitors: config.monitors.execute,
            dht: config.dht.execute,
            #[cfg(feature = "target_hw")]
            i2c_interface: config.i2c_interface.execute,
            ventilation: config.ventilation.execute,
            temperature_gradient: config.temperature_gradient.execute,
            schedule_check: config.schedule_check.execute,
            ds18b20: config.ds18b20.execute,
            watchdog: config.watchdog.execute,
            tcp_communication: config.tcp_communication.execute,
        }
    }
}

/// Implements the `Default` trait for `ExecutionConfig`.
///
/// This provides a standard way to create a default instance where all modules are
/// considered active. This is particularly useful for testing environments or as a
/// fallback. Using the `Default` trait is more idiomatic than a custom `default()`
/// function and allows for convenient instantiation with `ExecutionConfig::default()`.
impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            relay_manager: true,
            data_logger: true,
            refill: true,
            heating: true,
            tank_level_switch: true,
            atlas_scientific: true,
            sensor_manager: true,
            feed: true,
            balling: true,
            monitors: true,
            dht: true,
            #[cfg(feature = "target_hw")]
            i2c_interface: true,
            ventilation: true,
            temperature_gradient: true,
            schedule_check: true,
            ds18b20: true,
            watchdog: true,
            tcp_communication: true,
        }
    }
}
