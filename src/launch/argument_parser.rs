/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use std::path::Path;
use crate::utilities::config_file_definition_error::ConfigFileDefinitionError;

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
