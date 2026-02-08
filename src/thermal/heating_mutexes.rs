/* Copyright 2026 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::sensors::sensor_manager::SensorManagerSignals;
use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
use crate::thermal::thermal_control_status::ThermalControlStatus;
use std::sync::{Arc, Mutex};

/// Contains `Arc<Mutex>`-wrapped shared data points accessed by the heating control.
///
/// These mutexes provide thread-safe access to various sensor readings and
/// device states that are used by the heating control.
pub struct HeatingMutexes {
    /// Mutex for signals from sensor manager
    pub mutex_sensor_manager_signals: Arc<Mutex<SensorManagerSignals>>,

    /// Mutex for the tank level switch signals
    pub mutex_tank_level_switch_signals: Arc<Mutex<TankLevelSwitchSignals>>,

    /// Mutex for the heater status
    pub mutex_heating_status: Arc<Mutex<ThermalControlStatus>>,

    /// Mutex for the temperature gradient
    pub mutex_temperature_gradient: Arc<Mutex<f64>>,
}
