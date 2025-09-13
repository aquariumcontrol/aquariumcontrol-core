/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
//! Manages memory monitoring and allocator configuration checks for the application.
//!
//! This module provides a `Memory` struct designed to run as a dedicated thread.
//! It monitors the application's memory footprint over time and issues a warning if thresholds are exceeded.
//!
//! Its primary responsibilities are:
//!
//! 1.  **RAM Usage Monitoring**: It periodically checks the process's Resident Set Size (RSS),
//!     which is the actual physical memory (RAM) it is currently consuming.
//! 2.  **Virtual Memory Monitoring**: It also checks the total virtual memory (VmSize)
//!     reserved by the process.
//!
//! The monitoring loop is controlled via channels and can be terminated cleanly on application shutdown.

use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::watchmen::memory_channels::MemoryChannels;
use crate::watchmen::memory_config::MemoryConfig;
use log::{error};
#[cfg(all(target_os = "linux", feature = "debug_channels"))]
use log::info;
#[cfg(not(test))]
use log::warn;
use spin_sleep::SpinSleeper;
use std::time::{Duration, Instant};
use sysinfo::{Pid, System};
use crate::watchmen::memory_config_check::{memory_config_check, MemoryConfigError};
#[cfg(all(target_os = "linux", feature = "debug_channels"))]
use tikv_jemalloc_ctl::{epoch, stats};

/// The main struct for the memory monitoring thread.
///
/// An instance of this struct contains the configuration and the state required
/// to perform periodic memory checks.
pub struct Memory {
    /// configuration read from .toml file
    config: MemoryConfig,

    /// Lock flag avoiding flooding of logfile with repeated messages about virtual memory reservation
    lock_warn_virtual_memory_exceeded: bool,

    /// Lock flag avoiding flooding of logfile with repeated messages about RAM usage
    lock_warn_ram_usage_exceeded: bool,

    /// Lock flag avoiding flooding of logfile with repeated messages about process information not available
    lock_error_process_information_not_available: bool,
}

const CYCLE_TIME_HEATING_MEMORY: u64 = 250;

impl Memory {
    /// Creates a new `Memory` instance.
    ///
    /// Stores the configuration and initializes the lock flags.
    ///
    /// # Arguments
    /// * `config` - A reference to the global `ConfigData` containing the memory configuration.
    ///
    /// # Returns
    /// A new `Memory` instance wrapped in a Result type ready to be executed in its own thread.
    pub fn new(config: MemoryConfig) -> Result<Memory, MemoryConfigError> {
        // execute the configuration check
        if config.active {
            // check if the memory configuration is valid
            match memory_config_check(&config) {
                Ok(_env_var_name_value_tuple_opt) => {
                    #[cfg(all(target_os = "linux", feature = "debug_channels"))]
                    match _env_var_name_value_tuple_opt {
                        Some((env_var_name, value)) => {
                            info!("Memory configuration check: {env_var_name} is configured to {value}");
                        }
                        None => { /* do nothing, configuration check may be disabled */ }
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Ok(Self {
            config,
            lock_warn_virtual_memory_exceeded: false,
            lock_warn_ram_usage_exceeded: false,
            lock_error_process_information_not_available: false,
        })
    }

    /// Checks the provided memory usage values against configured limits and updates lock flags.
    ///
    /// This function contains the core logic for memory threshold checking, making it
    /// easy to test without depending on live system calls.
    fn check_memory_limits(&mut self, ram_usage_kb: u64, virtual_memory_kb: u64) {
        if ram_usage_kb > self.config.max_ram_usage {
            if !self.lock_warn_ram_usage_exceeded {
                #[cfg(not(test))]
                {
                    let warning_message = format!(
                        "Current RAM usage ({} kByte) exceeds threshold ({} kByte)",
                        ram_usage_kb, self.config.max_ram_usage
                    );
                    warn!(target: module_path!(), "{}", warning_message);
                }
                self.lock_warn_ram_usage_exceeded = true;
            }
        } else {
            // Reset the lock when usage is back to normal
            self.lock_warn_ram_usage_exceeded = false;
        }

        if virtual_memory_kb > self.config.max_virtual_memory {
            if !self.lock_warn_virtual_memory_exceeded {
                #[cfg(not(test))]
                {
                    let warning_message = format!(
                        "Current virtual memory reservation ({} kByte) exceeds threshold ({} kByte)",
                        virtual_memory_kb, self.config.max_virtual_memory
                    );
                    warn!(target: module_path!(), "{}", warning_message);
                }
                self.lock_warn_virtual_memory_exceeded = true;
            }
        } else {
            // Reset the lock when usage is back to normal
            self.lock_warn_virtual_memory_exceeded = false;
        }
    }

    /// Starts the memory monitoring loop.
    ///
    /// This is the entry point for the thread. It performs an initial check of the
    /// allocator's environment variable configuration. Then, it enters a loop that:
    /// 1.  Checks for a `Quit` command from the signal handler to terminate gracefully.
    /// 2.  Checks the current RAM and virtual memory usage against configured limits.
    /// 3.  Logs a warning if any limit is exceeded.
    /// 4.  Sleeps for the configured interval before the next check.
    ///
    /// # Arguments
    /// * `memory_channels` - A mutable reference to the channels used for receiving shutdown commands.
    pub fn execute(&mut self, memory_channels: &mut MemoryChannels) {
        let sleep_time = Duration::from_millis(CYCLE_TIME_HEATING_MEMORY);
        let check_interval = Duration::from_secs(self.config.check_interval);
        let mut last_check_instant = Instant::now() - check_interval;
        let spin_sleeper = SpinSleeper::default();

        // Create a new System object.
        let mut sys = System::new();

        // Get the current process's PID.
        let pid = Pid::from_u32(std::process::id());

        loop {
            // Check for a quit command before sleeping to ensure a responsive shutdown.
            let (quit_command_received, _, _) = self
                .process_external_request(&mut memory_channels.rx_memory_from_signal_handler, None);
            if quit_command_received {
                break;
            }

            if last_check_instant.elapsed() > check_interval {
                sys.refresh_memory();

                if let Some(process) = sys.process(pid) {
                    // Convert from bytes to kilobytes for comparison
                    let ram_usage_kb = process.memory() / 1024;
                    let virtual_memory_reservation_kb = process.virtual_memory() / 1024;

                    self.check_memory_limits(ram_usage_kb, virtual_memory_reservation_kb);

                    // Reset the "not available" error lock since we succeeded
                    self.lock_error_process_information_not_available = false;

                    #[cfg(all(target_os = "linux", feature = "debug_channels"))]
                    {
                        epoch::advance().unwrap();
                        let allocated = tikv_jemalloc_ctl::stats::allocated::read().unwrap();
                        let resident = stats::resident::read().unwrap();
                        info!(target: module_path!(), "Jemalloc: allocated = {allocated}, resident = {resident}");
                    }
                } else if !self.lock_error_process_information_not_available {
                    error!(
                        target: module_path!(),
                        "Could not find information for current process (PID: {})", pid
                    );
                    self.lock_error_process_information_not_available = true;
                }
                last_check_instant = Instant::now();
            }
        }

        spin_sleeper.sleep(sleep_time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::channels::Channels;
    use crate::utilities::channel_content::InternalCommand;
    use std::thread;

    /// Helper to create a default config for testing.
    fn test_config() -> MemoryConfig {
        MemoryConfig {
            active: false,
            execute: false,
            env_variable_name: "dummy".to_string(),
            env_variable_min_val: 0,
            max_ram_usage: 100,      // 100 kB
            max_virtual_memory: 200, // 200 kB
            check_interval: 60,
            env_variable_max_val: 0,
        }
    }

    #[test]
    fn test_new_initialization() {
        // Arrange
        let config = test_config();

        // Act
        let memory = Memory::new(config).unwrap();

        // Assert
        assert!(!memory.lock_warn_ram_usage_exceeded);
        assert!(!memory.lock_warn_virtual_memory_exceeded);
        assert!(!memory.lock_error_process_information_not_available);
        assert_eq!(memory.config.max_ram_usage, 100);
    }

    #[test]
    fn test_execute_graceful_shutdown() {
        // Arrange
        let config = test_config();
        let mut memory = Memory::new(config).unwrap();
        let mut channels = Channels::new_for_test();

        // Act
        let handle = thread::spawn(move || {
            // The `process_external_request` trait is not implemented in this file,
            // so we simulate its behavior for the test.
            memory.execute(&mut channels.memory);
        });

        let spin_sleeper = SpinSleeper::default();
        spin_sleeper.sleep(Duration::from_secs(1));

        channels
            .signal_handler
            .send_to_memory(InternalCommand::Quit)
            .unwrap();

        // Assert
        let result = handle.join();
        assert!(
            result.is_ok(),
            "Thread should shut down gracefully on Quit command"
        );
    }

    #[test]
    fn test_check_memory_limits_below_threshold() {
        // Arrange
        let mut memory = Memory::new(test_config()).unwrap();

        // Act
        memory.check_memory_limits(50, 150); // Values are below 100 and 200 limits

        // Assert
        assert!(
            !memory.lock_warn_ram_usage_exceeded,
            "Lock should not be set when RAM is below limit"
        );
        assert!(
            !memory.lock_warn_virtual_memory_exceeded,
            "Lock should not be set when virtual memory is below limit"
        );
    }

    #[test]
    fn test_check_memory_limits_above_ram_threshold() {
        // Arrange
        let mut memory = Memory::new(test_config()).unwrap();

        // Act
        memory.check_memory_limits(150, 150); // RAM is above 100 kByte limit

        // Assert
        assert!(
            memory.lock_warn_ram_usage_exceeded,
            "RAM lock should be set when usage exceeds limit"
        );
        assert!(
            !memory.lock_warn_virtual_memory_exceeded,
            "Virtual memory lock should remain unset"
        );
    }

    #[test]
    fn test_check_memory_limits_above_virtual_threshold() {
        // Arrange
        let mut memory = Memory::new(test_config()).unwrap();

        // Act
        memory.check_memory_limits(50, 250); // Virtual memory is above 200 kByte limit

        // Assert
        assert!(
            !memory.lock_warn_ram_usage_exceeded,
            "RAM lock should remain unset"
        );
        assert!(
            memory.lock_warn_virtual_memory_exceeded,
            "Virtual memory lock should be set when usage exceeds limit"
        );
    }

    #[test]
    fn test_check_memory_limits_resets_lock_when_normal() {
        // Arrange
        let mut memory = Memory::new(test_config()).unwrap();

        // Set the lock initially by exceeding the limit
        memory.check_memory_limits(150, 250);
        assert!(memory.lock_warn_ram_usage_exceeded);
        assert!(memory.lock_warn_virtual_memory_exceeded);

        // Act
        // Now, run with values below the limit
        memory.check_memory_limits(50, 150);

        // Assert
        assert!(
            !memory.lock_warn_ram_usage_exceeded,
            "RAM lock should be reset when usage returns to normal"
        );
        assert!(
            !memory.lock_warn_virtual_memory_exceeded,
            "Virtual memory lock should be reset when usage returns to normal"
        );
    }
}
