/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use std::sync::{Arc, Mutex};

/// Function checks if a mutex is already blocked.
/// The result of this function is used to reduce probability of false triggering a thread timing warning.
/// # Arguments
/// * `mutex` - reference to the mutex for avoiding parallel device actuation.
pub fn check_if_mutex_is_blocked(mutex: &Arc<Mutex<i32>>) -> bool {
    match mutex.try_lock() {
        Ok(_) => {
            // The mutex is not blocked.
            false
        },
        Err(_) => {
            // The mutex is blocked.
            true
        }
    }
}

#[cfg(test)]
pub mod tests {
    // Function checks if a given value is exceeding previous maximum value or
    // if the given value is below previous minimum.
    // Depending on the result, either previous minimum/maximum values are returned
    // or one of the threholds is updated with the given value.
    pub fn update_min_max_actuation_duration(
        min_actuation_duration: u128,
        max_actuation_duration: u128,
        new_actuation_duration: u128,
    ) -> (u128, u128) {
        let mut result_min_actuation_duration: u128 = min_actuation_duration;
        let mut result_max_actuation_duration: u128 = max_actuation_duration;
        if new_actuation_duration < min_actuation_duration {
            result_min_actuation_duration = new_actuation_duration;
        }
        if new_actuation_duration > max_actuation_duration {
            result_max_actuation_duration = new_actuation_duration;
        }
        (result_min_actuation_duration, result_max_actuation_duration)
    }
}
