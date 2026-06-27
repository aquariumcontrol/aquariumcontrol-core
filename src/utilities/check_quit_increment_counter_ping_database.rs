/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
/// A macro to handle common end-of-loop tasks for Balling and Feed.
///
/// This macro encapsulates the logic for:
/// 1. Breaking the loop if a quit command has been received.
/// 2. Sleeping for a short duration to prevent busy-waiting.
/// 3. Incrementing and wrapping a loop counter.
/// 4. Pinging the database to keep the connection alive.
#[macro_export]
macro_rules! check_quit_increment_counter_ping_database {
    (
        $quit_flag:expr,
        $sleep_duration:expr,
        $loop_counter:expr,
        $self_ref:expr,
        $sql_interface_mut_ref:expr
    ) => {
        if $quit_flag {
            break;
        }
        tokio::time::sleep($sleep_duration).await;

        $loop_counter += 1;

        // avoid overflow
        if $loop_counter == 1_000_000 {
            $loop_counter = 0;
        }

        $self_ref.check_timing_and_ping_database($sql_interface_mut_ref);
    };
}
