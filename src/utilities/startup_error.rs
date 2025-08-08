use crate::database::sql_interface_error::SqlInterfaceError;
#[cfg(target_os = "linux")]
use crate::dispatch::messaging_error::MessagingError;
use crate::mineral::balling_error::BallingError;
use crate::recorder::data_logger::DataLoggerError;
use crate::relays::actuate_controllino::ActuateControllinoError;
use crate::simulator::tcp_communication::TcpCommunicationError;
use crate::utilities::config_error::ConfigError;
use crate::utilities::config_file_definition_error::ConfigFileDefinitionError;
use crate::utilities::logger::LoggerSetupError;
use crate::utilities::memory_config_check::MemoryConfigError;
use crate::utilities::version_information::VersionInformationError;
use thiserror::Error;
#[cfg(all(feature = "target_hw", target_os = "linux"))]
use crate::sensors::gpio_handler::{GpioHandler, GpioHandlerError};

#[derive(Error, Debug)]
pub enum StartupError {
    /// Invalid command-line arguments
    #[error("Invalid command-line arguments: {0}")]
    Config(#[from] ConfigFileDefinitionError),

    /// The error message will come directly from ConfigError.
    #[error(transparent)]
    LoadConfig(#[from] ConfigError),

    /// This application must be run as root.
    #[error("This application must be run as root.")]
    NotRoot,

    /// Configuration of temperature ranges for heating and ventilation control is not valid.
    #[error(
        "Configuration error: temperature ranges for heating and ventilation control are conflicting each other."
    )]
    InvalidThermalConfig,

    /// A database error occurred.
    #[error("A database error occurred.")]
    Database {
        #[source]
        source: SqlInterfaceError,
    },

    /// The Balling dosing configuration is invalid.
    #[error("Configuration error: Balling dosing configuration is invalid.")]
    InvalidBallingConfiguration { source: BallingError },

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

    /// Configuration error: Memory configuration of the operating system is invalid.
    #[error("Configuration error: Memory configuration of operating system is invalid.")]
    InvalidMemoryConfig {
        #[source]
        source: MemoryConfigError,
    },

    /// Could not retrieve version information.
    #[error("Could not retrieve version information.")]
    VersionInformationRetrievalFailure {
        #[source]
        source: VersionInformationError,
    },

    /// Could not initialize the logging system.
    #[error("Could not setup logger.")]
    LoggingSetupFailure {
        #[source]
        source: LoggerSetupError,
    },

    /// Could not initialize the data recording system.
    #[error("Could not setup data logger.")]
    DataLoggerSetupFailure {
        #[source]
        source: DataLoggerError,
    },

    /// Could not initialize communication with Controllino.
    #[error("Could not setup communication with Controllino.")]
    ControllinoSetupFailure {
        #[source]
        source: ActuateControllinoError,
    },

    /// Could not initialize the interface to the GPIO.
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    #[error("Could not setup the interface to the GPIO.")]
    GpioHandlerSetupFailure {
        #[source]
        source: GpioHandlerError,
    }
}
