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
//! 1.  **Pre-flight checks**: Check validity of configuration and environment variables.
//! 1.  **Resident memory monitoring**: It periodically checks the process's resident memory,
//!     as informed by the allocator - conditionally compiled for feature 'jemalloc'.
//! 2.  **Allocated memory monitoring **: It also checks the allocated memory,
//!     as informed by the allocator - conditionally compiled for feature 'jemalloc'.
//!
//! The monitoring loop is controlled via channels and can be terminated cleanly on application shutdown.

use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::watchmen::memory_channels::MemoryChannels;
use crate::watchmen::memory_config::MemoryConfig;
cfg_if::cfg_if! {
    if #[cfg(feature = "jemalloc")] {
        use log::{error, info};
        use tikv_jemalloc_ctl::{epoch, stats};
        #[cfg(not(test))]
        use log::warn;
    }
}
use crate::watchmen::memory_config_check::{memory_config_check, MemoryConfigError};
#[cfg(target_os = "linux")]
use log::info;
use spin_sleep::SpinSleeper;
use std::time::{Duration, Instant};

#[cfg(all(target_os = "linux", not(test)))]
use nix::unistd::gettid;

/// The main struct for the memory monitoring thread.
///
/// An instance of this struct contains the configuration and the state required
/// to perform periodic memory checks.
pub struct Memory {
    /// configuration read from .toml file
    config: MemoryConfig,

    #[cfg(feature = "jemalloc")]
    /// Lock flag avoiding flooding of logfile with repeated messages about resident memory usage
    lock_warn_resident_memory_exceeded: bool,

    #[cfg(feature = "jemalloc")]
    /// Lock flag avoiding flooding of logfile with repeated messages about allocated memory usage
    lock_warn_allocated_memory_exceeded: bool,
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
                    #[cfg(target_os = "linux")]
                    match _env_var_name_value_tuple_opt {
                        Some((env_var_name, value)) => {
                            if config.active {
                                info!("Memory configuration check: {env_var_name} is configured to {value}");
                            }
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
            #[cfg(feature = "jemalloc")]
            lock_warn_allocated_memory_exceeded: false,
            #[cfg(feature = "jemalloc")]
            lock_warn_resident_memory_exceeded: false,
        })
    }

    #[cfg(feature = "jemalloc")]
    /// Checks the provided memory usage values against configured limits and updates lock flags.
    ///
    /// This function contains the core logic for memory threshold checking, making it
    /// easy to test without depending on live system calls.
    fn check_memory_limits(&mut self, allocated: usize, resident: usize) {
        if allocated > self.config.max_allocated {
            if !self.lock_warn_allocated_memory_exceeded {
                #[cfg(not(test))]
                {
                    let warning_message = format!(
                        "Currently allocated memory ({}) exceeds threshold ({})",
                        allocated, self.config.max_allocated
                    );
                    warn!(target: module_path!(), "{}", warning_message);
                }
                self.lock_warn_allocated_memory_exceeded = true;
            }
        } else {
            // Reset the lock when usage is back to normal
            self.lock_warn_allocated_memory_exceeded = false;
        }

        if resident > self.config.max_resident {
            if !self.lock_warn_resident_memory_exceeded {
                #[cfg(not(test))]
                {
                    let warning_message = format!(
                        "Currently resident memory ({}) exceeds threshold ({})",
                        resident, self.config.max_resident
                    );
                    warn!(target: module_path!(), "{}", warning_message);
                }
                self.lock_warn_resident_memory_exceeded = true;
            }
        } else {
            // Reset the lock when usage is back to normal
            self.lock_warn_resident_memory_exceeded = false;
        }
    }

    /// Starts the memory monitoring loop.
    ///
    /// 1.  Checks for a `Quit` command from the signal handler to terminate gracefully.
    /// 2.  Checks the current resident and allocated memory usage against configured limits.
    /// 3.  Logs a warning if any limit is exceeded.
    /// 4.  Sleeps for the configured interval before the next check.
    ///
    /// # Arguments
    /// * `memory_channels` - A mutable reference to the channels used for receiving shutdown commands.
    pub fn execute(&mut self, memory_channels: &mut MemoryChannels) {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let sleep_time = Duration::from_millis(CYCLE_TIME_HEATING_MEMORY);
        let check_interval = Duration::from_secs(self.config.check_interval);
        let mut last_check_instant = Instant::now() - check_interval;
        let spin_sleeper = SpinSleeper::default();

        loop {
            // Check for a quit command before sleeping to ensure a responsive shutdown.
            let (quit_command_received, _, _) = self
                .process_external_request(&mut memory_channels.rx_memory_from_signal_handler, None);
            if quit_command_received {
                break;
            }
            spin_sleeper.sleep(sleep_time);

            if last_check_instant.elapsed() > check_interval {
                #[cfg(feature = "jemalloc")]
                {
                    match epoch::advance() {
                        Ok(_) => {
                            let allocated = match tikv_jemalloc_ctl::stats::allocated::read() {
                                Ok(allocated) => allocated,
                                Err(_) => {
                                    error!(target: module_path!(), "Could not read allocated memory");
                                    continue;
                                }
                            };
                            let resident = match stats::resident::read() {
                                Ok(resident) => resident,
                                Err(_) => {
                                    error!(target: module_path!(), "Could not read resident memory");
                                    continue;
                                }
                            };
                            info!(target: module_path!(), "Jemalloc: allocated = {allocated}, resident = {resident}");
                            self.check_memory_limits(allocated, resident);
                        }
                        Err(_) => {
                            error!(target: module_path!(), "Could not advance epoch.");
                        }
                    }
                }

                last_check_instant = Instant::now();
            }
        }
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
            #[cfg(feature = "jemalloc")]
            max_allocated: 100,
            #[cfg(feature = "jemalloc")]
            max_resident: 200,
            check_interval: 60,
            env_variable_max_val: 0,
        }
    }

    #[test]
    #[cfg(feature = "jemalloc")]
    fn test_new_initialization() {
        // Arrange
        let config = test_config();

        // Act
        let memory = Memory::new(config).unwrap();

        // Assert
        assert!(!memory.lock_warn_allocated_memory_exceeded);
        assert!(!memory.lock_warn_resident_memory_exceeded);
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
    #[cfg(feature = "jemalloc")]
    fn test_check_memory_limits_below_threshold() {
        // Arrange
        let mut memory = Memory::new(test_config()).unwrap();

        // Act
        memory.check_memory_limits(50, 150); // Values are below 100 and 200 limits

        // Assert
        assert!(
            !memory.lock_warn_allocated_memory_exceeded,
            "Lock should not be set when allocated memory usage is below threshold"
        );
        assert!(
            !memory.lock_warn_resident_memory_exceeded,
            "Lock should not be set when resident memory usage is below threshold"
        );
    }

    #[test]
    #[cfg(feature = "jemalloc")]
    fn test_check_memory_limits_above_allocated_threshold() {
        // Arrange
        let mut memory = Memory::new(test_config()).unwrap();

        // Act
        memory.check_memory_limits(150, 150); // allocated is above the 100-Byte limit

        // Assert
        assert!(
            memory.lock_warn_allocated_memory_exceeded,
            "Allocated memory usage warn lock flag should be set when usage exceeds limit"
        );
        assert!(
            !memory.lock_warn_resident_memory_exceeded,
            "Resident memory usage warn lock flag should remain unset"
        );
    }

    #[test]
    #[cfg(feature = "jemalloc")]
    fn test_check_memory_limits_above_resident_threshold() {
        // Arrange
        let mut memory = Memory::new(test_config()).unwrap();

        // Act
        memory.check_memory_limits(50, 250); // Resident memory is above the 200-Byte limit

        // Assert
        assert!(
            !memory.lock_warn_allocated_memory_exceeded,
            "Allocated memory usage warn lock flag should remain unset"
        );
        assert!(
            memory.lock_warn_resident_memory_exceeded,
            "Resident memory memory lock flag should be set when usage exceeds limit"
        );
    }

    #[test]
    #[cfg(feature = "jemalloc")]
    fn test_check_memory_limits_resets_lock_when_normal() {
        // Arrange
        let mut memory = Memory::new(test_config()).unwrap();

        // Set the lock initially by exceeding the limit
        memory.check_memory_limits(150, 250);
        assert!(memory.lock_warn_resident_memory_exceeded);
        assert!(memory.lock_warn_allocated_memory_exceeded);

        // Act
        // Now, run with values below the limit
        memory.check_memory_limits(50, 150);

        // Assert
        assert!(
            !memory.lock_warn_allocated_memory_exceeded,
            "Allocated memory usage warn flag should be reset when usage returns to normal"
        );
        assert!(
            !memory.lock_warn_resident_memory_exceeded,
            "Resident memory usage warn lock flag should be reset when usage returns to normal"
        );
    }
}
