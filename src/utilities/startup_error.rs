use thiserror::Error;
use crate::database::sql_interface_error::SqlInterfaceError;
use crate::simulator::tcp_communication::TcpCommunicationError;
use crate::utilities::config_file_definition_error::ConfigFileDefinitionError;

#[derive(Error, Debug)]
pub enum StartupError {
    #[error("Invalid command-line arguments: {0}")]
    Config(#[from] ConfigFileDefinitionError),

    #[error("This application must be run as root.")]
    NotRoot,

    #[error("Configuration of temperature ranges for heating and ventilation control is not valid.")]
    InvalidThermalConfig,

    #[error("Could not connect to the database.")]
    DatabaseConnection { #[source] source: SqlInterfaceError },

    #[error("Failed to query the database.")]
    DatabaseQuery { #[source] source: SqlInterfaceError },

    #[error("Could not get database name.")]
    DatabaseName { #[source] source: SqlInterfaceError },

    #[error("Database version mismatch.")]
    DatabaseVersion { #[source] source: SqlInterfaceError },

    #[error("A required table is missing from the database.")]
    MissingTable { #[source] source: SqlInterfaceError },

    #[error("Could not open TCP connection to test server.")]
    TcpConnection { #[source] source: TcpCommunicationError },

    #[allow(unused)]
    #[error("Configuration error: GPIO handler not initialized.")]
    GpioHandlerMissing,
}