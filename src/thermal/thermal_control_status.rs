/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// This struct is used by Heating and Ventilation to communicate the status to the data logging.
#[derive(Default, Clone)]
pub struct ThermalControlStatus {
    /// Status bit of thermal control: It follows the actuation of heater or the ventilation.
    pub actuator_switched_on: bool,

    /// Status bit indicating that an ineffectiveness of the actuator has been detected
    pub actuator_ineffective: bool,

    /// Status bit indicating that an actuator has been switched off due to detected ineffectiveness
    pub actuator_disabled: bool,

    /// Status bit indicating that controller was stopped by external command
    pub external_inhibition_request: bool,
}

impl ThermalControlStatus {
    /// Translates the internal states into a humanly readable external state.
    pub fn description(&self) -> &'static str {
        if self.external_inhibition_request {
            "STOPPED"
        } else if self.actuator_disabled {
            "ERROR"
        } else {
            "ACTIVE"
        }
    }
}
