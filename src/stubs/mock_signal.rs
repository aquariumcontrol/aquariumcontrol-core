/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use std::fmt;

    /// Enum is used for internal thread messaging between test environment and mock threads
    #[derive(Clone, Debug, PartialEq)]
    pub enum MockSignal {
        /// position of tank level switch
        TankLevelSwitchPosition(bool),

        #[allow(dead_code)]
        /// status bit of tank level switch signal
        TankLevelSwitchInvalid(bool),

        #[allow(unused)]
        /// position of tank level switch stabilized
        TankLevelSwitchPositionStabilized(bool),

        #[allow(unused)]
        /// water temperature in °C
        WaterTemperature(f32),

        #[allow(unused)]
        #[allow(non_camel_case_types)]
        /// pH Value
        pH(f32),

        #[allow(unused)]
        /// conductivity im uS/cm
        Conductivity(f32),

        #[allow(unused)]
        /// ambient (air) temperature in °C
        AmbientTemperature(f32),

        #[allow(unused)]
        /// ambient humidity in %
        AmbientHumidity(f32),

        /// heating control status
        HeatingControlStatus(bool),

        /// status of ventilation control
        VentilationControlStatus(bool),

        /// refill pump status
        RefillPumpStatus(bool),

        /// schedule allows operation
        #[allow(unused)]
        ScheduleAllows(bool),
    }

    impl fmt::Display for MockSignal {
        /// Function provides output of the enum values for debugging purposes.
        /// # Arguments
        /// * `f` - configuration for formatting    
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match self {
                MockSignal::TankLevelSwitchPosition(d) => write!(
                    f,
                    "TankLevelSwitchPosition({})",
                    match d {
                        true => "high",
                        false => "low",
                    }
                ),
                MockSignal::TankLevelSwitchInvalid(d) => write!(
                    f,
                    "TankLevelSwitchInvalid({})",
                    match d {
                        true => "invalid",
                        false => "valid",
                    }
                ),
                MockSignal::TankLevelSwitchPositionStabilized(d) => write!(
                    f,
                    "TankLevelSwitchPositionStabilized({})",
                    match d {
                        true => "high",
                        false => "low",
                    }
                ),
                MockSignal::WaterTemperature(d) => write!(f, "WaterTemperature({})", d),
                MockSignal::pH(d) => write!(f, "pH({})", d),
                MockSignal::Conductivity(d) => write!(f, "Conductivity({})", d),
                MockSignal::AmbientTemperature(d) => write!(f, "AmbientTemperature({})", d),
                MockSignal::AmbientHumidity(d) => write!(f, "AmbientHumidity({})", d),
                MockSignal::HeatingControlStatus(d) => write!(
                    f,
                    "VentilationControlStatus({})",
                    match d {
                        true => "on",
                        false => "off",
                    }
                ),
                MockSignal::VentilationControlStatus(d) => write!(
                    f,
                    "VentilationControlStatus({})",
                    match d {
                        true => "on",
                        false => "off",
                    }
                ),
                MockSignal::RefillPumpStatus(d) => write!(
                    f,
                    "RefillPumpStatus({})",
                    match d {
                        true => "on",
                        false => "off",
                    }
                ),
                MockSignal::ScheduleAllows(d) => write!(
                    f,
                    "ScheduleAllows({})",
                    match d {
                        true => "allowed",
                        false => "not allowed",
                    }
                ),
            }
        }
    }
}
