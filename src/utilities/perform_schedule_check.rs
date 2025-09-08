/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
/// A macro to perform a request-response check with the schedule_check module.
///
/// This macro encapsulates the logic for:
/// 1. Sending a `ScheduleCheck` command.
/// 2. Receiving a boolean response.
/// 3. Handling send/receive errors with a "log-once" pattern.
/// 4. Assigning the result, defaulting to `false` on any communication failure.
#[macro_export]
macro_rules! perform_schedule_check {
    (
        $channels:expr,
        $result_var:expr,
        $send_lock:expr,
        $receive_lock:expr,
        $location:expr
    ) => {
        match $channels.send_to_schedule_check(InternalCommand::ScheduleCheck) {
            Ok(_) => {
                $send_lock = false;
                // Now listen for the response
                $result_var = match $channels.receive_from_schedule_check() {
                    Ok(c) => {
                        $receive_lock = false;
                        c
                    }
                    Err(e) => {
                        if !$receive_lock {
                            error!(
                                target: $location,
                                "Receiving from schedule check failed ({e:?})"
                            );
                            $receive_lock = true;
                        }
                        // Assume allowed to actuate if communication fails
                        true
                    }
                };
            }
            Err(e) => {
                if !$send_lock {
                    error!(
                        target: $location,
                        "Channel communication to schedule check failed ({e:?})"
                    );
                    $send_lock = true;
                }
                // On send failure, also assume allowed to actuate
                $result_var = true;
            }
        }
    };
}
