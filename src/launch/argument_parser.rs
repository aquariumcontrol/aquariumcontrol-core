/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::utilities::config_file_definition_error::ConfigFileDefinitionError;
use std::path::Path;

/// Checks if the provided string is a valid path to a `.toml` configuration file.
///
/// This function validates a user-selected configuration file path by ensuring that:
/// - The path exists on the file system.
/// - It points to a regular file (not a directory).
/// - The file has an extension.
/// - The file's extension is `.toml`.
///
/// # Arguments
/// * `user_selected_config_file_name` - A string slice representing the potential path to the configuration file.
///
/// # Returns
/// A `Result` containing the validated and owned file path as a `String` on success.
///
/// # Errors
/// Returns a `ConfigFileDefinitionError` if any validation check fails:
/// - `ConfigFileDefinitionError::PathDoesNotExist`: If no file or directory exists at the given path.
/// - `ConfigFileDefinitionError::IsNotAFile`: If the path points to a directory, not a regular file.
/// - `ConfigFileDefinitionError::FileNameHasNoSuffix`: If the path does not have a file extension.
/// - `ConfigFileDefinitionError::SuffixIsNotToml`: If the file's extension is not `.toml`.
pub fn check_argument_for_valid_config_file_name(
    user_selected_config_file_name: &str,
) -> Result<String, ConfigFileDefinitionError> {
    let path = Path::new(&user_selected_config_file_name);

    let desired_config_file_name = user_selected_config_file_name.to_string();

    if !path.exists() {
        return Err(ConfigFileDefinitionError::PathDoesNotExist(
            desired_config_file_name,
        ));
    }

    if !path.is_file() {
        return Err(ConfigFileDefinitionError::IsNotAFile(
            desired_config_file_name,
        ));
    }

    match path.extension() {
        Some(extension) => {
            if extension == "toml" {
                Ok(String::from(user_selected_config_file_name))
            } else {
                Err(ConfigFileDefinitionError::SuffixIsNotToml(
                    desired_config_file_name,
                ))
            }
        }
        None => Err(ConfigFileDefinitionError::FileNameHasNoSuffix(
            desired_config_file_name,
        )),
    }
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
mod tests {
    use super::*;

    #[test]
    // Tests that a valid existing .toml configuration file path returns Ok with the path string.
    fn test_check_argument_valid_toml_file() {
        let path_str = "config/aquarium_control_test_generic.toml";
        let res = check_argument_for_valid_config_file_name(path_str);
        assert_eq!(res, Ok(path_str.to_string()));
    }

    #[test]
    // Tests that a non-existent file path returns Err(PathDoesNotExist).
    fn test_check_argument_path_does_not_exist() {
        let path_str = "non_existent_file.toml";
        let res = check_argument_for_valid_config_file_name(path_str);
        assert_eq!(
            res,
            Err(ConfigFileDefinitionError::PathDoesNotExist(
                path_str.to_string()
            ))
        );
    }

    #[test]
    // Tests that a path pointing to a directory instead of a regular file returns Err(IsNotAFile).
    fn test_check_argument_is_not_a_file() {
        let path_str = "config";
        let res = check_argument_for_valid_config_file_name(path_str);
        assert_eq!(
            res,
            Err(ConfigFileDefinitionError::IsNotAFile(path_str.to_string()))
        );
    }

    #[test]
    // Tests that an existing file without a file extension returns Err(FileNameHasNoSuffix).
    fn test_check_argument_file_has_no_suffix() {
        let path_str = "target/temp_file_no_suffix";
        let _ = std::fs::File::create(path_str);
        let res = check_argument_for_valid_config_file_name(path_str);
        let _ = std::fs::remove_file(path_str);
        assert_eq!(
            res,
            Err(ConfigFileDefinitionError::FileNameHasNoSuffix(
                path_str.to_string()
            ))
        );
    }

    #[test]
    // Tests that an existing file with a non-toml extension returns Err(SuffixIsNotToml).
    fn test_check_argument_suffix_is_not_toml() {
        let path_str = "Cargo.lock";
        let res = check_argument_for_valid_config_file_name(path_str);
        assert_eq!(
            res,
            Err(ConfigFileDefinitionError::SuffixIsNotToml(
                path_str.to_string()
            ))
        );
    }
}
