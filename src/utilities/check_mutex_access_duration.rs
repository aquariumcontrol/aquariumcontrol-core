/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::sensors::tank_level_switch::TankLevelSwitch;
#[cfg(not(test))]
use log::warn;
use std::time::{Duration, Instant};
use crate::sensors::atlas_scientific::AtlasScientific;
use crate::sensors::dht::Dht;
use crate::sensors::ds18b20::Ds18b20;
use crate::sensors::sensor_manager::SensorManager;
use crate::thermal::heating::Heating;
use crate::thermal::ventilation::Ventilation;
use crate::utilities::channel_content::AquariumSignal;
use crate::water::water_injection::WaterInjection;

pub trait CheckMutexAccessDurationTrait {
    /// Reads the warn-lock.
    /// This flag is used to prevent log-flooding
    fn get_warn_lock(&self, key_opt: &Option<&AquariumSignal>) -> bool;

    /// Sets the warn-lock.
    /// This flag is used to prevent log-flooding
    fn set_warn_lock(&mut self, key: &Option<&AquariumSignal>, value: bool);

    /// Records violation of mutex access duration for testing purposes
    #[cfg(test)]
    fn record_access_duration_violation(&mut self, key_opt: &Option<&AquariumSignal>);

    /// Provides the maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration;

    /// Inform the originator for logging purposes
    #[cfg(not(test))]
    fn get_location(&self) -> &str;

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
                #[cfg(not(test))]
                warn!(target: self.get_location(),
                        "Access to mutex took too long. Allowed={}ms, Measured={}ms",
                        self.get_max_mutex_access_duration().as_millis(),
                        measured_mutex_access_duration.as_millis(),
                );
                // record for testing purposes only
                cfg_if::cfg_if! {
                    if #[cfg(test)] {
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
    fn get_warn_lock(&self, _key_opt: &Option<&AquariumSignal>) -> bool {
        self.lock_warn_max_mutex_access_duration
    }

    /// Method connects the default trait implementation with the specific implementation
    /// setting the warn-lock.
    fn set_warn_lock(&mut self, _key_opt: &Option<&AquariumSignal>, value: bool) {
        self.lock_warn_max_mutex_access_duration = value;
    }

    #[cfg(test)]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    fn record_access_duration_violation(&mut self, _key_opt: &Option<&AquariumSignal>) {
        self.mutex_access_duration_exceeded = true;
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))]
    fn get_location(&self) -> &str {
        "tank level switch"
    }
}

impl CheckMutexAccessDurationTrait for Heating {
    /// Method connects the default trait implementation with the specific implementation
    /// reading the warn-lock.
    fn get_warn_lock(&self, _key_opt: &Option<&AquariumSignal>) -> bool {
        self.lock_warn_max_mutex_access_duration
    }

    /// Method connects the default trait implementation with the specific implementation
    /// setting the warn-lock.
    fn set_warn_lock(&mut self, _key_opt: &Option<&AquariumSignal>, value: bool) {
        self.lock_warn_max_mutex_access_duration = value;
    }

    #[cfg(test)]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    fn record_access_duration_violation(&mut self, _key_opt: &Option<&AquariumSignal>) {
        self.mutex_access_duration_exceeded = true;
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))]
    fn get_location(&self) -> &str {
        "heating"
    }
}

impl CheckMutexAccessDurationTrait for Ventilation {
    /// Method connects the default trait implementation with the specific implementation
    /// reading the warn-lock.
    fn get_warn_lock(&self, _key_opt: &Option<&AquariumSignal>) -> bool {
        self.lock_warn_max_mutex_access_duration
    }

    /// Method connects the default trait implementation with the specific implementation
    /// setting the warn-lock.
    fn set_warn_lock(&mut self, _key_opt: &Option<&AquariumSignal>, value: bool) {
        self.lock_warn_max_mutex_access_duration = value;
    }

    #[cfg(test)]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    fn record_access_duration_violation(&mut self, _key_opt: &Option<&AquariumSignal>) {
        self.mutex_access_duration_exceeded = true;
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))]
    fn get_location(&self) -> &str {
        "ventilation"
    }
}

impl CheckMutexAccessDurationTrait for Dht {
    /// Method connects the default trait implementation with the specific implementation
    /// reading the warn-lock.
    fn get_warn_lock(&self, _key_opt: &Option<&AquariumSignal>) -> bool {
        self.lock_warn_max_mutex_access_duration
    }

    /// Method connects the default trait implementation with the specific implementation
    /// setting the warn-lock.
    fn set_warn_lock(&mut self, _key_opt: &Option<&AquariumSignal>, value: bool) {
        self.lock_warn_max_mutex_access_duration = value;
    }

    #[cfg(test)]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    fn record_access_duration_violation(&mut self, _key_opt: &Option<&AquariumSignal>) {
        self.mutex_access_duration_exceeded = true;
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))]
    fn get_location(&self) -> &str {
        "dht"
    }
}

impl CheckMutexAccessDurationTrait for SensorManager {
    /// Method connects the default trait implementation with the specific implementation
    /// reading the warn-lock.
    fn get_warn_lock(&self, _key_opt: &Option<&AquariumSignal>) -> bool {
        self.lock_warn_max_mutex_access_duration
    }

    /// Method connects the default trait implementation with the specific implementation
    /// setting the warn-lock.
    fn set_warn_lock(&mut self, _key_opt: &Option<&AquariumSignal>, value: bool) {
        self.lock_warn_max_mutex_access_duration = value;
    }

    #[cfg(test)]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    fn record_access_duration_violation(&mut self, _key_opt: &Option<&AquariumSignal>) {
        self.mutex_access_duration_exceeded = true;
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))]
    fn get_location(&self) -> &str {
        "dht"
    }
}

impl CheckMutexAccessDurationTrait for WaterInjection {
    /// Method connects the default trait implementation with the specific implementation
    /// reading the warn-lock.
    fn get_warn_lock(&self, _key_opt: &Option<&AquariumSignal>) -> bool {
        self.lock_warn_max_mutex_access_duration
    }

    /// Method connects the default trait implementation with the specific implementation
    /// setting the warn-lock.
    fn set_warn_lock(&mut self, _key_opt: &Option<&AquariumSignal>, value: bool) {
        self.lock_warn_max_mutex_access_duration = value;
    }

    #[cfg(test)]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    fn record_access_duration_violation(&mut self, _key_opt: &Option<&AquariumSignal>) {
        self.mutex_access_duration_exceeded = true;
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))]
    fn get_location(&self) -> &str {
        "water_injection"
    }
}

impl CheckMutexAccessDurationTrait for Ds18b20 {
    /// Method connects the default trait implementation with the specific implementation
    /// reading the warn-lock.
    fn get_warn_lock(&self, _key_opt: &Option<&AquariumSignal>) -> bool {
        self.lock_warn_max_mutex_access_duration
    }

    /// Method connects the default trait implementation with the specific implementation
    /// setting the warn-lock.
    fn set_warn_lock(&mut self, _key: &Option<&AquariumSignal>, value: bool) {
        self.lock_warn_max_mutex_access_duration = value;
    }

    #[cfg(test)]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    fn record_access_duration_violation(&mut self, _key_opt: &Option<&AquariumSignal>) {
        self.mutex_access_duration_exceeded = true;
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration {
        self.max_mutex_access_duration
    }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))]
    fn get_location(&self) -> &str {
        "ventilation"
    }
}
impl CheckMutexAccessDurationTrait for AtlasScientific {
    fn get_warn_lock(&self, key_opt: &Option<&AquariumSignal>) -> bool {
        if key_opt.is_some() {
            match key_opt.as_ref().unwrap() {
                AquariumSignal::WaterTemperature => self.lock_warn_max_mutex_access_duration_temperature,
                AquariumSignal::pH => self.lock_warn_max_mutex_access_duration_ph,
                AquariumSignal::Conductivity => self.lock_warn_max_mutex_access_duration_conductivity,
                _ => false, // Default for unhandled signals
            }
        }
        else {
            // defensive programming: this case should not occur
            true
        }
    }

    fn set_warn_lock(&mut self, key_opt: &Option<&AquariumSignal>, value: bool) {
        if key_opt.is_some() {
            match key_opt.as_ref().unwrap() {
                AquariumSignal::WaterTemperature => self.lock_warn_max_mutex_access_duration_temperature = value,
                AquariumSignal::pH => self.lock_warn_max_mutex_access_duration_ph = value,
                AquariumSignal::Conductivity => self.lock_warn_max_mutex_access_duration_conductivity = value,
                _ => {} // Do nothing for unhandled signals
            }
        }
    }

    #[cfg(test)]
    /// Method connects the default trait implementation with the specific implementation
    /// for recording access duration violation
    #[cfg(test)]
    fn record_access_duration_violation(&mut self, key_opt: &Option<&AquariumSignal>) {
        if key_opt.is_some() {
            match key_opt.as_ref().unwrap() {
                AquariumSignal::WaterTemperature => self.mutex_temperature_access_duration_exceeded = true,
                AquariumSignal::pH => self.mutex_ph_access_duration_exceeded = true,
                AquariumSignal::Conductivity => self.mutex_conductivity_access_duration_exceeded = true,
                _ => { /* do nothing for unrecognized signal */ }
            }
        }
    }

    /// Method connects the default trait implementation with the specific implementation
    /// for getting maximum permissible access duration
    fn get_max_mutex_access_duration(&self) -> Duration { self.max_mutex_access_duration_millis }

    /// inform the location of the warning for logging purposes
    #[cfg(not(test))]
    fn get_location(&self) -> &str { "atlas_scientific" }
}