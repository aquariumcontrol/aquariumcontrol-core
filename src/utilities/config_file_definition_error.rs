use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigFileDefinitionError {
    #[error("The configuration file ({0}) is not an existing file system entry.")]
    PathDoesNotExist(String),

    #[error("The provided argument for the configuration file ({0}) is not pointing to a file.")]
    IsNotAFile(String),

    #[error("The provided argument for the configuration file ({0}) does not have any extension.")]
    FileNameHasNoSuffix(String),

    #[error(
        "The provided argument for the configuration file ({0}) does not have extension '.toml'."
    )]
    SuffixIsNotToml(String),
}
