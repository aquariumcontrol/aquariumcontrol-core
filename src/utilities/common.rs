/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use std::sync::{Arc, Mutex};

/// Checks if a given mutex is currently locked without blocking.
///
/// This function attempts to acquire the mutex immediately. If successful, it means
/// the mutex was not blocked. If unsuccessful, it indicates the mutex is currently held by
/// another thread. This function is used by time-critical control algorithms
/// (e.g., heating or ventilation) to identify root causes for excessive cycle times.
///
/// # Arguments
/// * `mutex` - A reference to the `Arc<Mutex<i32>>` to be checked for its locked status.
///
/// # Returns
/// `true` if the mutex is currently blocked (locked by another thread);
/// `false` if the mutex is not blocked and could be acquired immediately.
pub fn check_if_mutex_is_blocked(mutex: &Arc<Mutex<i32>>) -> bool {
    match mutex.try_lock() {
        Ok(_) => {
            // The mutex is not blocked.
            false
        }
        Err(_) => {
            // The mutex is blocked.
            true
        }
    }
}

#[cfg(test)]
pub mod tests {
    // Updates the minimum and maximum observed actuation durations based on a new sample.
    //
    // This function takes current minimum and maximum actuation durations and a new
    // observed duration. If the new duration is lower than the current minimum, it
    // becomes the new minimum. Similarly, if it's higher than the current maximum,
    // it becomes the new maximum.
    //
    // Arguments:
    // * `min_actuation_duration`: The current minimum recorded actuation duration.
    // * `max_actuation_duration`: The current maximum recorded actuation duration.
    // * `new_actuation_duration`: The latest observed actuation duration.
    //
    // Returns:
    // A tuple `(u128, u128)` containing the updated minimum and maximum actuation durations.
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
