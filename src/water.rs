/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

// Contains the main functionality for the fresh water refill
pub mod refill;

// Contains trait implementation for the execution of the fresh water refill
pub mod water_injection;

// Contains data definition between the main thread and trait implementation
pub mod refill_error_states;

// A data structure for reporting critical refill states, such as excessive triggering, to the monitoring module.
pub mod refill_monitor_view;

// Container for communication with SQL database
pub mod refill_stats_data;

// Container for channels
pub mod refill_channels;

// Configuration of Refill
pub mod refill_config;

/// Communication of refill state information to data logger
pub mod refill_status;
