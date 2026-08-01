/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// Struct contains the status of the refill control for communication to the data logger.
#[derive(Default)]
pub struct RefillStatus {
    /// Status bit of refill control: It follows the actuation of the refill pump.
    pub refill_in_progress_live: bool,

    /// Status bit of refill control for data logger:
    /// - It is set with the start when the refill pump is actuated.
    /// - It is unset by the data logger to make sure the data logger has captured the refill.
    pub refill_in_progress_for_database: bool,

    /// Status bit indicating that an error is present which impedes further refill operation
    pub critical_error_present: bool,

    /// Status bit indicating that controller was stopped by external command
    pub external_inhibition_request: bool,
}

impl RefillStatus {
    /// Translates the internal states into a humanly readable external state.
    pub fn description(&self) -> &'static str {
        if self.external_inhibition_request {
            "STOPPED"
        } else if self.critical_error_present {
            "ERROR"
        } else {
            "ACTIVE"
        }
    }
}
