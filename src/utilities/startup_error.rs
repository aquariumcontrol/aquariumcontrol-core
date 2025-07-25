use crate::database::sql_interface_error::SqlInterfaceError;
use crate::simulator::tcp_communication::TcpCommunicationError;
use crate::utilities::config_error::ConfigError;
use crate::utilities::config_file_definition_error::ConfigFileDefinitionError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StartupError {
    #[error("Invalid command-line arguments: {0}")]
    Config(#[from] ConfigFileDefinitionError),

    #[error(transparent)] // The error message will come directly from ConfigError
    LoadConfig(#[from] ConfigError),

    #[error("This application must be run as root.")]
    NotRoot,

    #[error(
        "Configuration of temperature ranges for heating and ventilation control is not valid."
    )]
    InvalidThermalConfig,

    #[error("A database error occurred.")]
    Database {
        #[from]
        source: SqlInterfaceError,
    },

    #[error("Could not open TCP connection to test server.")]
    TcpConnection {
        #[source]
        source: TcpCommunicationError,
    },

    #[allow(unused)]
    #[error("Configuration error: GPIO handler not initialized.")]
    GpioHandlerMissing,
}
