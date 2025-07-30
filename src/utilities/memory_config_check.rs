use serde_derive::Deserialize;
use thiserror::Error;

/// Holds the data for the memory configuration check.
/// The configuration is loaded from the .toml configuration file.
#[derive(Deserialize, Clone)]
pub struct MemoryConfig {
    /// flag for deactivating the memory configuration check, primarily used for testing
    active: bool,

    /// name of the environment variable to check
    env_variable_name: String,

    /// minimum value of the environment variable
    env_variable_min_val: u64,

    /// maximum value of the environment variable
    env_variable_max_val: u64,
}

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

pub fn memory_config_check(
    memory_config: &MemoryConfig,
) -> Result<Option<(String, u64)>, MemoryConfigError> {
    if memory_config.active {
        // check if environment variable is set in the configuration file
        if memory_config.env_variable_name.is_empty() {
            return Err(MemoryConfigError::ConfigEnvVarNameEmpty);
        }
        return match std::env::var(memory_config.env_variable_name.as_str()) {
            Ok(val_string) => {
                let val = match val_string.parse() {
                    Ok(val) => val,
                    Err(_) => {
                        return Err(MemoryConfigError::EnvVarParseError(val_string));
                    }
                };
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

                // return the pair of environment variable name and configured value, so that upper level can report it
                Ok(Some((memory_config.env_variable_name.clone(), val)))
            }
            Err(_) => Err(MemoryConfigError::EnvVarNotSet(
                memory_config.env_variable_name.clone(),
            )),
        };
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    // Helper function to create a default config for tests
    fn create_test_config() -> MemoryConfig {
        MemoryConfig {
            active: true,
            env_variable_name: "TEST_VAR".to_string(),
            env_variable_min_val: 1,
            env_variable_max_val: 10,
        }
    }

    // Test if memory_config_check correctly returns Ok(None) when the check is inactive.
    #[test]
    fn test_check_inactive_returns_ok_none() {
        let mut config = create_test_config();
        config.active = false;

        let result = memory_config_check(&config);
        assert!(matches!(result, Ok(None)));
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
