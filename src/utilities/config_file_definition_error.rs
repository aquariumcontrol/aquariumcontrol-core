use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigFileDefinitionError {
    /// The configuration file is not an existing file system entry.
    #[error("The configuration file ({0}) is not an existing file system entry.")]
    PathDoesNotExist(String),

    /// The provided argument for the configuration file is not pointing to a file.
    #[error("The provided argument for the configuration file ({0}) is not pointing to a file.")]
    IsNotAFile(String),

    /// The provided argument for the configuration file does not have any extension.
    #[error("The provided argument for the configuration file ({0}) does not have any extension.")]
    FileNameHasNoSuffix(String),

    /// The provided argument for the configuration file does not have extension '.toml'.
    #[error(
        "The provided argument for the configuration file ({0}) does not have extension '.toml'."
    )]
    SuffixIsNotToml(String),
}
