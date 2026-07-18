/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines a trait for monitoring and logging excessive `Mutex` lock contention.
//!
//! This module provides the `CheckMutexAccessDurationTrait`, a standardized mechanism
//! for detecting when acquiring a `Mutex` lock takes longer than a predefined threshold.
//! Long lock-wait times can be a symptom of high contention, performance bottlenecks,
//! or potential deadlocks, and this trait offers a reusable pattern to identify such
//! issues across the application.
//!
//! ## Purpose and Design
//!
//! In a multithreaded application, shared data is protected by mutexes. If a thread
//! has to wait an unexpectedly long time to acquire a lock, it can degrade system
//! performance. This trait provides a default implementation for `check_mutex_access_duration`
//! that any struct can use to time its lock acquisitions and log a warning if they
//! are too slow.
//!
//! This design achieves several goals:
//! - **Standardization**: Provides a single, consistent way to monitor mutex performance.
//! - **Code Reusability**: Eliminates the need to write timing and logging boilerplate
//!   in every thread that uses a mutex.
//! - **Log Flood Prevention**: Includes a `warn_lock` mechanism to ensure that a
//!   persistent contention issue only generates a single log message, preventing
//!   log spam.
//! - **Testability**: Includes hooks (`record_access_duration_violation`) to allow
//!   unit tests to verify that the violation detection logic is working correctly.
//!
//! ## Key Components
//!
//! - **`CheckMutexAccessDurationTrait`**: The core trait of the module.
//!   - **`check_mutex_access_duration()`**: The main-provided method. It takes timestamps
//!     from before and after a lock attempt, calculates the duration, and logs a
//!     warning if it exceeds the configured maximum.
//!   - **`get_warn_lock()` / `set_warn_lock()`**: Required methods for implementing
//!     the log flood prevention flag.
//!   - **`get_max_mutex_access_duration()`**: A required method that defines the
//!     maximum permissible wait time for the implementor.
//!   - **`get_location()`**: A required method that provides a string identifier for
//!     the implementing module, used for contextual logging.

use crate::sensors::atlas_scientific_ezo::AtlasScientificEzo;
#[cfg(any(all(feature = "target_hw", target_os = "linux"), test))]
use crate::sensors::atlas_scientific_oem::AtlasScientificOem;
use crate::sensors::dht::Dht;
use crate::sensors::ds18b20::Ds18b20;
use crate::sensors::sensor_manager::SensorManager;
use crate::sensors::tank_level_switch::TankLevelSwitch;
use crate::thermal::heating::Heating;
use crate::thermal::ventilation::Ventilation;
use crate::utilities::channel_content::AquariumSignal;
use crate::water::water_injection::WaterInjection;
#[cfg(not(test))] // only required in real-life execution
use log::warn;
use std::time::{Duration, Instant};

pub trait CheckMutexAccessDurationTrait {
    /// Reads the warn-lock.
    /// This flag is used to prevent log-flooding
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn get_warn_lock(&self, key_opt: &Option<&AquariumSignal>) -> bool;

    /// Sets the warn-lock.
    /// This flag is used to prevent log-flooding
    ///
    /// # Arguments
    /// * `key` - The configuration key.
    /// * `value` - The value to set.
    fn set_warn_lock(&mut self, key: &Option<&AquariumSignal>, value: bool);

    /// Records violation of mutex access duration for testing purposes
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    #[cfg(any(test, feature = "test-utils"))]
    fn record_access_duration_violation(&mut self, key_opt: &Option<&AquariumSignal>);

    /// Provides the maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration;

    /// Inform the originator for logging purposes
    #[cfg(not(test))] // only required in real-life execution
    fn get_location(&self) -> &str;

    /// Checks the mutex access duration.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    /// * `instant_after_locking_mutex` - A parameter representing `instant_after_locking_mutex`.
    /// * `instant_before_locking_mutex` - A parameter representing `instant_before_locking_mutex`.
    fn check_mutex_access_duration(
        &mut self,
        key_opt: Option<&AquariumSignal>,
        instant_after_locking_mutex: Instant,
        instant_before_locking_mutex: Instant,
    ) {
        // check if access to mutex took too long
        let measured_mutex_access_duration =
            instant_after_locking_mutex.duration_since(instant_before_locking_mutex);
        let warn_lock = self.get_warn_lock(&key_opt);
        if measured_mutex_access_duration > self.get_max_mutex_access_duration() {
            if !warn_lock {
                self.set_warn_lock(&key_opt, true);
                #[cfg(not(test))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                warn!(target: self.get_location(),
                        "Access to mutex took too long. Allowed={}ms, Measured={}ms",
                        self.get_max_mutex_access_duration().as_millis(),
                        measured_mutex_access_duration.as_millis(),
                );

                // record for testing purposes only
                cfg_if::cfg_if! {
                    if #[cfg(any(test, feature = "test-utils"))] {
                        println!(
                            "*** Access to mutex took too long. Allowed={}ms, Measured={}ms ***",
                            self.get_max_mutex_access_duration().as_millis(),
                            measured_mutex_access_duration.as_millis(),
                        );
                        self.record_access_duration_violation(&key_opt);
                    }
                }
            }
        } else {
            self.set_warn_lock(&key_opt, false);
        }
    }
}

impl CheckMutexAccessDurationTrait for TankLevelSwitch {
    /// Method connects the default trait implementation with the specific implementation
    /// reading the warn-lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn get_warn_lock(&self, _key_opt: &Option<&AquariumSignal>) -> bool {
        self.lock_warn_max_mutex_access_duration
    }

    /// Method connects the default trait implementation with the specific implementation
    /// setting the warn-lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    /// * `value` - The value to set.
    fn set_warn_lock(&mut self, _key_opt: &Option<&AquariumSignal>, value: bool) {
        self.lock_warn_max_mutex_access_duration = value;
    }

    #[cfg(any(test, feature = "test-utils"))]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn record_access_duration_violation(&mut self, _key_opt: &Option<&AquariumSignal>) {
        self.mutex_access_duration_exceeded = true;
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))] // only required in real-life execution
    fn get_location(&self) -> &str {
        "tank level switch"
    }
}

impl CheckMutexAccessDurationTrait for Heating {
    /// Method connects the default trait implementation with the specific implementation
    /// reading the warn-lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn get_warn_lock(&self, _key_opt: &Option<&AquariumSignal>) -> bool {
        self.lock_warn_max_mutex_access_duration
    }

    /// Method connects the default trait implementation with the specific implementation
    /// setting the warn-lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    /// * `value` - The value to set.
    fn set_warn_lock(&mut self, _key_opt: &Option<&AquariumSignal>, value: bool) {
        self.lock_warn_max_mutex_access_duration = value;
    }

    #[cfg(any(test, feature = "test-utils"))]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn record_access_duration_violation(&mut self, _key_opt: &Option<&AquariumSignal>) {
        self.mutex_access_duration_exceeded = true;
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))] // only required in real-life execution
    fn get_location(&self) -> &str {
        "heating"
    }
}

impl CheckMutexAccessDurationTrait for Ventilation {
    /// Method connects the default trait implementation with the specific implementation
    /// reading the warn-lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn get_warn_lock(&self, _key_opt: &Option<&AquariumSignal>) -> bool {
        self.lock_warn_max_mutex_access_duration
    }

    /// Method connects the default trait implementation with the specific implementation
    /// setting the warn-lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    /// * `value` - The value to set.
    fn set_warn_lock(&mut self, _key_opt: &Option<&AquariumSignal>, value: bool) {
        self.lock_warn_max_mutex_access_duration = value;
    }

    #[cfg(any(test, feature = "test-utils"))]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn record_access_duration_violation(&mut self, _key_opt: &Option<&AquariumSignal>) {
        self.mutex_access_duration_exceeded = true;
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))] // only required in real-life execution
    fn get_location(&self) -> &str {
        "ventilation"
    }
}

impl CheckMutexAccessDurationTrait for Dht {
    /// Method connects the default trait implementation with the specific implementation
    /// reading the warn-lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn get_warn_lock(&self, _key_opt: &Option<&AquariumSignal>) -> bool {
        self.lock_warn_max_mutex_access_duration
    }

    /// Method connects the default trait implementation with the specific implementation
    /// setting the warn-lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    /// * `value` - The value to set.
    fn set_warn_lock(&mut self, _key_opt: &Option<&AquariumSignal>, value: bool) {
        self.lock_warn_max_mutex_access_duration = value;
    }

    #[cfg(any(test, feature = "test-utils"))]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn record_access_duration_violation(&mut self, _key_opt: &Option<&AquariumSignal>) {
        self.mutex_access_duration_exceeded = true;
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))] // only required in real-life execution
    fn get_location(&self) -> &str {
        "dht"
    }
}

impl CheckMutexAccessDurationTrait for SensorManager {
    /// Method connects the default trait implementation with the specific implementation
    /// reading the warn-lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn get_warn_lock(&self, _key_opt: &Option<&AquariumSignal>) -> bool {
        self.lock_warn_max_mutex_access_duration
    }

    /// Method connects the default trait implementation with the specific implementation
    /// setting the warn-lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    /// * `value` - The value to set.
    fn set_warn_lock(&mut self, _key_opt: &Option<&AquariumSignal>, value: bool) {
        self.lock_warn_max_mutex_access_duration = value;
    }

    #[cfg(any(test, feature = "test-utils"))]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn record_access_duration_violation(&mut self, _key_opt: &Option<&AquariumSignal>) {
        self.mutex_access_duration_exceeded = true;
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))] // only required in real-life execution
    fn get_location(&self) -> &str {
        "sensor_manager"
    }
}

impl CheckMutexAccessDurationTrait for WaterInjection {
    /// Method connects the default trait implementation with the specific implementation
    /// reading the warn-lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn get_warn_lock(&self, _key_opt: &Option<&AquariumSignal>) -> bool {
        self.lock_warn_max_mutex_access_duration
    }

    /// Method connects the default trait implementation with the specific implementation
    /// setting the warn-lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    /// * `value` - The value to set.
    fn set_warn_lock(&mut self, _key_opt: &Option<&AquariumSignal>, value: bool) {
        self.lock_warn_max_mutex_access_duration = value;
    }

    #[cfg(any(test, feature = "test-utils"))]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn record_access_duration_violation(&mut self, _key_opt: &Option<&AquariumSignal>) {
        self.mutex_access_duration_exceeded = true;
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))] // only required in real-life execution
    fn get_location(&self) -> &str {
        "water_injection"
    }
}

impl CheckMutexAccessDurationTrait for Ds18b20 {
    /// Method connects the default trait implementation with the specific implementation
    /// reading the warn-lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn get_warn_lock(&self, _key_opt: &Option<&AquariumSignal>) -> bool {
        self.lock_warn_max_mutex_access_duration
    }

    /// Method connects the default trait implementation with the specific implementation
    /// setting the warn-lock.
    ///
    /// # Arguments
    /// * `key` - The configuration key.
    /// * `value` - The value to set.
    fn set_warn_lock(&mut self, _key: &Option<&AquariumSignal>, value: bool) {
        self.lock_warn_max_mutex_access_duration = value;
    }

    #[cfg(any(test, feature = "test-utils"))]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn record_access_duration_violation(&mut self, _key_opt: &Option<&AquariumSignal>) {
        self.mutex_access_duration_exceeded = true;
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))] // only required in real-life execution
    fn get_location(&self) -> &str {
        "ds18b20"
    }
}
impl CheckMutexAccessDurationTrait for AtlasScientificEzo {
    /// Retrieves the warn lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn get_warn_lock(&self, key_opt: &Option<&AquariumSignal>) -> bool {
        if let Some(key) = key_opt {
            match key {
                AquariumSignal::WaterTemperature => {
                    self.lock_warn_max_mutex_access_duration_temperature
                }
                AquariumSignal::pH => self.lock_warn_max_mutex_access_duration_ph,
                AquariumSignal::Conductivity => {
                    self.lock_warn_max_mutex_access_duration_conductivity
                }
                _ => false, // Default for unhandled signals
            }
        } else {
            // defensive programming: this case should not occur
            true
        }
    }

    /// Sets the warn lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    /// * `value` - The value to set.
    fn set_warn_lock(&mut self, key_opt: &Option<&AquariumSignal>, value: bool) {
        if let Some(key) = key_opt {
            match key {
                AquariumSignal::WaterTemperature => {
                    self.lock_warn_max_mutex_access_duration_temperature = value
                }
                AquariumSignal::pH => self.lock_warn_max_mutex_access_duration_ph = value,
                AquariumSignal::Conductivity => {
                    self.lock_warn_max_mutex_access_duration_conductivity = value
                }
                _ => {} // Do nothing for unhandled signals
            }
        }
    }

    #[cfg(any(test, feature = "test-utils"))]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn record_access_duration_violation(&mut self, key_opt: &Option<&AquariumSignal>) {
        if let Some(key) = key_opt {
            match key {
                AquariumSignal::WaterTemperature => {
                    self.mutex_temperature_access_duration_exceeded = true
                }
                AquariumSignal::pH => self.mutex_ph_access_duration_exceeded = true,
                AquariumSignal::Conductivity => {
                    self.mutex_conductivity_access_duration_exceeded = true
                }
                _ => { /* do nothing for unrecognized signal */ }
            }
        }
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))] // only required in real-life execution
    fn get_location(&self) -> &str {
        "atlas_scientific_ezo"
    }
}

#[cfg(any(all(feature = "target_hw", target_os = "linux"), test))]
impl CheckMutexAccessDurationTrait for AtlasScientificOem {
    /// Retrieves the warn lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    fn get_warn_lock(&self, key_opt: &Option<&AquariumSignal>) -> bool {
        if let Some(key) = key_opt {
            match key {
                AquariumSignal::WaterTemperature => {
                    self.lock_warn_max_mutex_access_duration_temperature
                }
                AquariumSignal::pH => self.lock_warn_max_mutex_access_duration_ph,
                AquariumSignal::Conductivity => {
                    self.lock_warn_max_mutex_access_duration_conductivity
                }
                _ => false, // Default for unhandled signals
            }
        } else {
            // defensive programming: this case should not occur
            true
        }
    }

    /// Sets the warn lock.
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    /// * `value` - The value to set.
    fn set_warn_lock(&mut self, key_opt: &Option<&AquariumSignal>, value: bool) {
        if let Some(key) = key_opt {
            match key {
                AquariumSignal::WaterTemperature => {
                    self.lock_warn_max_mutex_access_duration_temperature = value
                }
                AquariumSignal::pH => self.lock_warn_max_mutex_access_duration_ph = value,
                AquariumSignal::Conductivity => {
                    self.lock_warn_max_mutex_access_duration_conductivity = value
                }
                _ => {} // Do nothing for unhandled signals
            }
        }
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    ///
    /// # Arguments
    /// * `key_opt` - An optional configuration key.
    #[cfg(any(test, feature = "test-utils"))]
    fn record_access_duration_violation(&mut self, key_opt: &Option<&AquariumSignal>) {
        if let Some(key) = key_opt {
            match key {
                AquariumSignal::WaterTemperature => {
                    self.mutex_temperature_access_duration_exceeded = true
                }
                AquariumSignal::pH => self.mutex_ph_access_duration_exceeded = true,
                AquariumSignal::Conductivity => {
                    self.mutex_conductivity_access_duration_exceeded = true
                }
                _ => { /* do nothing for unrecognized signal */ }
            }
        }
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))] // only required in real-life execution
    fn get_location(&self) -> &str {
        "atlas_scientific_oem"
    }
}
