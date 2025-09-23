/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Provides a startup validation function for memory-related environment variables.
//!
//! This module contains the logic to perform a one-time check at application startup
//! to ensure that a specific environment variable (e.g., `MALLOC_ARENA_MAX`) is set
//! and its value falls within a configured range. The primary goal is to prevent the
//! application from launching in a misconfigured or potentially unstable environment.
//!
//! ## Key Components
//!
//! - **`memory_config_check()`**: The main function that orchestrates the validation.
//!   It reads the name of an environment variable and its expected min/max values
//!   from the `MemoryConfig` struct. It then fetches the variable from the
//!   environment, parses it as an integer, and verifies it against the configured
//!   bounds.
//!
//! - **`MemoryConfigError`**: A custom, `thiserror`-based error enum that represents
//!   all possible validation failures. By providing specific error variants (e.g.,
//!   `EnvVarNotSet`, `EnvVarValueTooLow`), it allows the application to terminate
//!   on startup with a clear, actionable error message explaining what needs to be
//!   fixed in the environment or configuration.
//!
//! ## Usage
//!
//! This function is intended to be called once from the main `launch` module. If the
//! check is active, and it returns an error, the application will abort startup.

use crate::watchmen::memory_config::MemoryConfig;
use thiserror::Error;

/// Definition of errors which can be encountered during memory_config_check
#[derive(Error, Debug)]
pub enum MemoryConfigError {
    /// environment variable is not configured in .toml configuration file
    #[error("environment variable for memory configuration check is not configured in .toml configuration file")]
    ConfigEnvVarNameEmpty,

    /// result of environment variable value request could not be parsed to numeric value
    #[error(
        "result of environment variable value request ({0}) could not be parsed to numeric value"
    )]
    EnvVarParseError(String),

    /// environment variable value is lower than configured minimum value
    #[error("environment variable value {0} is lower than configured minimum value {1}")]
    EnvVarValueTooLow(u64, u64),

    /// environment variable value is higher than configured maximum value
    #[error("environment variable value {0} is higher than configured maximum value {1}")]
    EnvVarValueTooHigh(u64, u64),

    /// environment variable is not set
    #[error("environment variable {0} is not set")]
    EnvVarNotSet(String),
}

/// Checks a specified environment variable against a configured minimum and maximum values.
///
/// This function is used at application startup to verify that a system memory
/// setting (`MALLOC_ARENA_MAX`) is within a safe and expected range.
///
/// # Arguments
/// * `memory_config` - A reference to the `MemoryConfig` struct containing the check
///   parameters (active flag, variable name, min/max values).
///
/// # Returns
/// A `Result` containing an `Option` on success:
/// - `Ok(Some((String, u64)))`: If the check is active and the environment variable's
///   value is within the configured bounds. The tuple contains the variable's name
///   and its parsed value.
/// - `Ok(None)`: If the check is configured to be inactive (`active: false`).
///
/// # Errors
/// Returns a `MemoryConfigError` if the check is active and any validation step fails:
/// - `MemoryConfigError::ConfigEnvVarNameEmpty`: If `active` is true, but the
///   `env_variable_name` in the configuration is empty.
/// - `MemoryConfigError::EnvVarNotSet`: If the specified environment variable is not
///   set in the current environment.
/// - `MemoryConfigError::EnvVarParseError`: If the value of the environment variable
///   cannot be parsed into a `u64` integer.
/// - `MemoryConfigError::EnvVarValueTooLow`: If the parsed value is less than the
///   configured `env_variable_min_val`.
/// - `MemoryConfigError::EnvVarValueTooHigh`: If the parsed value is greater than the
///   configured `env_variable_max_val`.
pub fn memory_config_check(
    memory_config: &MemoryConfig,
) -> Result<Option<(String, u64)>, MemoryConfigError> {
    if memory_config.env_variable_name.is_empty() {
        return Err(MemoryConfigError::ConfigEnvVarNameEmpty);
    }

    let var_name = &memory_config.env_variable_name;

    // Get the environment variable's value as a string.
    let val_string =
        std::env::var(var_name).map_err(|_| MemoryConfigError::EnvVarNotSet(var_name.clone()))?;

    // Parse the string into u64.
    let val = val_string
        .parse::<u64>()
        .map_err(|_| MemoryConfigError::EnvVarParseError(val_string))?;

    // Check if the value is within the configured bounds.
    if val < memory_config.env_variable_min_val {
        return Err(MemoryConfigError::EnvVarValueTooLow(
            val,
            memory_config.env_variable_min_val,
        ));
    }
    if val > memory_config.env_variable_max_val {
        return Err(MemoryConfigError::EnvVarValueTooHigh(
            val,
            memory_config.env_variable_max_val,
        ));
    }

    // If all checks pass, return the name and value.
    Ok(Some((var_name.clone(), val)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    // Helper function to create a default config for tests
    fn create_test_config() -> MemoryConfig {
        MemoryConfig {
            active: true,
            execute: false,
            env_variable_name: "TEST_VAR".to_string(),
            env_variable_min_val: 1,
            env_variable_max_val: 10,
            #[cfg(feature = "jemalloc")]
            max_resident: 0,
            #[cfg(feature = "jemalloc")]
            max_allocated: 0,
            check_interval: 0,
        }
    }

    // Test if memory_config_check returns an error if the check is active, but the variable name is empty.
    #[test]
    fn test_empty_env_variable_name_returns_error() {
        let mut config = create_test_config();
        config.env_variable_name = String::new();

        let result = memory_config_check(&config);
        assert!(matches!(
            result,
            Err(MemoryConfigError::ConfigEnvVarNameEmpty)
        ));
    }

    // Test if memory_config_check returns an error if the environment variable is not set.
    #[test]
    fn test_unset_env_variable_returns_error() {
        let mut config = create_test_config();
        let exotic_name = "EXOTIC_VAR_NAME_THAT_DOES_NOT_EXIST_123";
        config.env_variable_name = exotic_name.to_string();

        // Ensure the variable is not set
        env::remove_var(exotic_name);

        let result = memory_config_check(&config);
        match result {
            Err(MemoryConfigError::EnvVarNotSet(name)) => {
                assert_eq!(name, exotic_name);
            }
            _ => panic!("{}: Expected EnvVarNotSet error", module_path!()),
        }
    }

    // Test if memory_config_check returns a parse error for a non-numeric environment variable.
    #[test]
    fn test_non_numeric_env_variable_returns_parse_error() {
        let mut config = create_test_config();
        let var_name = "TEST_VAR_NON_NUMERIC";
        let var_value = "not-a-number";
        config.env_variable_name = var_name.to_string();

        env::set_var(var_name, var_value);
        let result = memory_config_check(&config);
        env::remove_var(var_name); // Cleanup

        match result {
            Err(MemoryConfigError::EnvVarParseError(val)) => {
                assert_eq!(val, var_value);
            }
            _ => panic!("{}: Expected EnvVarParseError", module_path!()),
        }
    }

    // Test if memory_config_check returns an error if the value is below the configured minimum.
    #[test]
    fn test_value_below_minimum_returns_error() {
        let mut config = create_test_config();
        let var_name = "TEST_VAR_TOO_LOW";
        config.env_variable_name = var_name.to_string();
        config.env_variable_min_val = 5;
        config.env_variable_max_val = 10;

        env::set_var(var_name, "2");
        let result = memory_config_check(&config);
        env::remove_var(var_name); // Cleanup

        match result {
            Err(MemoryConfigError::EnvVarValueTooLow(val, min)) => {
                assert_eq!(val, 2);
                assert_eq!(min, 5);
            }
            _ => panic!("{}: Expected EnvVarValueTooLow error", module_path!()),
        }
    }

    // Test if memory_config_check returns an error if the value is above the configured maximum.
    #[test]
    fn test_value_above_maximum_returns_error() {
        let mut config = create_test_config();
        let var_name = "TEST_VAR_TOO_HIGH";
        config.env_variable_name = var_name.to_string();
        config.env_variable_min_val = 5;
        config.env_variable_max_val = 10;

        env::set_var(var_name, "15");
        let result = memory_config_check(&config);
        env::remove_var(var_name); // Cleanup

        match result {
            Err(MemoryConfigError::EnvVarValueTooHigh(val, max)) => {
                assert_eq!(val, 15);
                assert_eq!(max, 10);
            }
            _ => panic!("{}: Expected EnvVarValueTooHigh error", module_path!()),
        }
    }

    // Test if memory_config_check returns Ok(value) when the value is within the configured bounds.
    #[test]
    fn test_value_within_bounds_is_ok() {
        let mut config = create_test_config();
        let var_name = "TEST_VAR_IN_BOUNDS";
        config.env_variable_name = var_name.to_string();
        config.env_variable_min_val = 5;
        config.env_variable_max_val = 10;

        env::set_var(var_name, "8");
        let result = memory_config_check(&config);
        env::remove_var(var_name); // Cleanup

        match result {
            Ok(Some((name, val))) => {
                assert_eq!(name, var_name);
                assert_eq!(val, 8);
            }
            _ => panic!("{}, Expected Ok(Some(...))", module_path!()),
        }
    }
}
