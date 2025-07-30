use crate::database::sql_interface_error::SqlInterfaceError;
#[cfg(target_os = "linux")]
use crate::dispatch::messaging_error::MessagingError;
use crate::simulator::tcp_communication::TcpCommunicationError;
use crate::utilities::config_error::ConfigError;
use crate::utilities::config_file_definition_error::ConfigFileDefinitionError;
use crate::utilities::memory_config_check::MemoryConfigError;
use crate::utilities::version_information::VersionInformationError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StartupError {
    /// Invalid command-line arguments
    #[error("Invalid command-line arguments: {0}")]
    Config(#[from] ConfigFileDefinitionError),

    /// The error message will come directly from ConfigError.
    #[error(transparent)] // The error message will come directly from ConfigError
    LoadConfig(#[from] ConfigError),

    /// This application must be run as root.
    #[error("This application must be run as root.")]
    NotRoot,

    /// Configuration of temperature ranges for heating and ventilation control is not valid.
    #[error(
        "Configuration of temperature ranges for heating and ventilation control is not valid."
    )]
    InvalidThermalConfig,

    /// A database error occurred.
    #[error("A database error occurred.")]
    Database {
        #[source]
        source: SqlInterfaceError,
    },

    /// Could not open TCP connection to test server.
    #[error("Could not open TCP connection to test server.")]
    TcpConnection {
        #[source]
        source: TcpCommunicationError,
    },

    /// Could not open POSIX message queue.
    #[cfg(target_os = "linux")]
    #[error("Could not establish connection to messaging system.")]
    Messaging {
        #[source]
        source: MessagingError,
    },

    /// Configuration error: GPIO handler not initialized.
    #[allow(unused)]
    #[error("Configuration error: GPIO handler not initialized.")]
    GpioHandlerMissing,

    #[error("Configuration error: Memory configuration of operating system is invalid.")]
    InvalidMemoryConfig {
        #[source]
        source: MemoryConfigError,
    },

    #[error("Could not retrieve version information.")]
    VersionInformationRetrievalFailure {
        #[source]
        source: VersionInformationError,
    },
}
