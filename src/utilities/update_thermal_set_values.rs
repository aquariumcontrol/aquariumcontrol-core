/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// A macro to update set values from a data source and handle logging on failure.
///
/// This macro encapsulates the logic for:
/// 1. Calling an `update_set_value` method on a given updater.
/// 2. On failure, logging the error using a "log-once" pattern.
/// 3. On success, resetting the "log-once" flag.
#[macro_export]
macro_rules! update_thermal_set_values {
    (
        $updater:expr,
        $off_temp:expr,
        $on_temp:expr,
        $lock_flag:expr,
        $location:expr,
        $is_multi_thread:expr
    ) => {
        let update_res = if $is_multi_thread {
            tokio::task::block_in_place(|| $updater.update_set_value(&mut $off_temp, &mut $on_temp))
        } else {
            $updater.update_set_value(&mut $off_temp, &mut $on_temp)
        };

        if let Err(e) = update_res {
            if !$lock_flag {
                log::error!(
                    target: $location,
                    concat!(
                        "Error occurred when trying to retrieve thermal set values from database, ",
                        "maintaining the current set values (off={:?}, on={:?}): {:?}"
                    ),
                    $off_temp,
                    $on_temp,
                    e
                );
                $lock_flag = true;
            }
        } else {
            $lock_flag = false;
        }
    };
}
