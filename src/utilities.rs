/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// Defines enums and structs for inter-thread communication via channels.
pub mod channel_content;

/// Provides common, shared functions and constants used across multiple modules.
pub mod common;

/// Handles the loading and parsing of the main application configuration from a TOML file.
pub mod config;

/// Defines error types related to reading and parsing the configuration file.
pub mod config_error;

/// Defines error types for structural or semantic issues within the configuration file.
pub mod config_file_definition_error;

/// Implements a simple Infinite Impulse Response (IIR) filter for smoothing sensor data.
pub mod iir_filter;

/// Contains setup and configuration for the application-wide logging framework.
pub mod logger;

/// Provides functionality to check for required memory allocation environment variables.
pub mod memory_config_check;

/// Defines a trait for processing external requests, such as commands from the signal handler.
pub mod proc_ext_req;

/// Manages the creation of a PID file to make the process ID available to external scripts.
pub mod publish_pid;

/// Implements a sawtooth wave generator for creating low-frequency PWM-like signals.
pub mod sawtooth_profile;

/// Manages system signals (e.g., SIGINT, SIGTERM) to ensure graceful application shutdown.
pub mod signal_handler;

/// Provides access to the application's version information from the build environment.
pub mod version_information;

/// Container for channels used by signal handler
pub mod signal_handler_channels;

/// Trait for multiple threads which have to wait for termination signal
pub mod wait_for_termination;
