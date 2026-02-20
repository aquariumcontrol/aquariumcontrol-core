/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
// Contains the framework for running diagnostic checks.
pub mod monitors;

// Contains the trait and default implementation watchdog communication.
pub mod petting;

// Contains control of the watchdog.
pub mod watchdog;

// Container for channels used by monitors module for communication with other threads.
pub mod monitors_channels;

// Container for channels used by watchdog module for communication with other threads.
pub mod watchdog_channels;

// Configuration of Monitors
pub mod monitors_config;

// Configuration of memory configuration check
pub mod memory_config;
// Provides functionality to check for required memory allocation environment variables.
pub mod memory_config_check;
// Configuration of Watchdog
pub mod watchdog_config;

// Memory utilization check (virtual memory reservation and RAM usage)
pub mod memory;

// Container for channels used by memory module for communication with other threads.
pub mod memory_channels;
