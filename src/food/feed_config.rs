/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;

/// Holds the configuration data for the feed control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug, PartialEq, Clone)]
pub struct FeedConfig {
    /// control will only be active when the flag is set to true
    pub active: bool,

    /// indicates if the thread shall be started or not
    pub execute: bool,

    /// interval for the main control in seconds    
    pub schedule_check_interval: u32,

    /// waiting time in milliseconds before switching on or off a device during feed
    pub device_switch_delay_millis: u32,

    /// Describes the strategy to use when there are multiple feed schedule entries in the past.
    /// Using strategy_multiple_schedule_entries_in_past:
    /// negative value: execute none of them
    /// positive value: execute the xth entry according to how they are stacked in time
    /// Example 0: Execute the first entry which has a timestamp closest to the current time.
    /// Example 1: Execute the second last entry which lies closest to the current time.
    /// Example 99: Numbers higher than the number of entries found in the past will trigger execution of the
    /// last entry which has a timestamp furthest from the current time.
    pub strategy_multiple_schedule_entries_in_past: i32,
}
