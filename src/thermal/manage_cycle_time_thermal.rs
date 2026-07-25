/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// A macro to manage the timing of a fixed-duration loop applicable to both thermal control modules.
///
/// This macro encapsulates the logic for:
/// 1. Calculating the loop's execution duration.
/// 2. Sleeping for the remaining time to meet the target cycle duration.
/// 3. Logging a "log-once" warning if the execution time exceeds the target.
/// 4. Resetting the cycle timer and associated flags for the next iteration.
#[macro_export]
macro_rules! manage_cycle_time_thermal {
    (
        $start_time:expr,
        $cycle_time_duration:expr,
        $cycle_time_millis_const:expr,
        $spin_sleeper:expr,
        $lock_warn_cycle_time_exceeded:expr,
        $actuation_mutex_was_blocked:expr,
        $location:expr
    ) => {
        let stop_time = std::time::Instant::now();
        let execution_duration = stop_time.duration_since($start_time);

        if execution_duration > $cycle_time_duration {
            if !$lock_warn_cycle_time_exceeded && !$actuation_mutex_was_blocked {
                #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                #[cfg(not(tarpaulin_include))]
                warn!(target: $location,
                    "execution duration of {} milliseconds exceeds cycle time of {}",
                    execution_duration.as_millis(),
                    $cycle_time_millis_const);
                $lock_warn_cycle_time_exceeded = true;
            }
        } else {
            let remaining_sleep_time = $cycle_time_duration - execution_duration;
            tokio::time::sleep(remaining_sleep_time).await;
            $lock_warn_cycle_time_exceeded = false;
        }

        // Reset the actuation block flag for the next cycle.
        $actuation_mutex_was_blocked = false;

        // Reset the start time for the next cycle.
        $start_time = std::time::Instant::now();
    };
}
