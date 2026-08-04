/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// Centralizes constants used throughout the `SensorManager` module.
///
/// This private module consolidates "magic strings" and other hardcoded values
/// to prevent them from being scattered throughout the `SensorManager`'s logic.
/// By keeping them in one place, it improves maintainability and reduces the
/// risk of typos when comparing against configuration values.
///
/// ### Constants
///
/// - **Sensor Source Identifiers**:
///   - `SOURCE_*`: These string constants define the valid values for the
///     `source_water_temperature` and `source_ambient_temperature` fields in the
///     `[sensor_manager]` section of the TOML configuration file. They map a
///     logical measurement to a physical sensor driver (e.g., "ds18b20", "dht").
///
/// - **Timing and Performance**:
///   - `*_MEASUREMENT_INTERVAL_MILLIS`: Define the polling rate for the sensor
///     manager, with different values for simulator and hardware modes.
///   - `MAX_MUTEX_ACCESS_DURATION_MILLIS`: Sets a threshold for how long a mutex
///     lock can be held before a warning is logged. This helps detect performance
///     bottlenecks or potential deadlocks.
pub mod sensor_manager_source_constants {
    pub const SOURCE_WATER_TEMPERATURE_ATLAS_SCIENTIFIC: &str = "atlas_scientific";

    pub const SOURCE_WATER_TEMPERATURE_DS18B20: &str = "ds18b20";

    pub const SOURCE_AMBIENT_TEMPERATURE_DHT: &str = "dht";

    pub const SOURCE_AMBIENT_TEMPERATURE_DS18B20: &str = "ds18b20";

    pub const SIMULATOR_MEASUREMENT_INTERVAL_MILLIS: u64 = 500;

    /// time interval for updating measurement data
    pub const HW_MEASUREMENT_INTERVAL_MILLIS: u64 = 500;

    /// allow max. 10 milliseconds for mutex to be blocked by any other thread
    pub const MAX_MUTEX_ACCESS_DURATION_MILLIS: u64 = 10;
}
