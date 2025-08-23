#[cfg(all(feature = "target_hw", target_os = "linux"))]
use crate::beacon::ws2812b::Ws2812BError;
use crate::database::sql_interface_error::SqlInterfaceError;
#[cfg(target_os = "linux")]
use crate::dispatch::messaging_error::MessagingError;
use crate::mineral::balling_error::BallingError;
use crate::recorder::data_logger::DataLoggerError;
use crate::relays::actuate_controllino::ActuateControllinoError;
use crate::sensors::ds18b20_error::Ds18b20Error;
#[cfg(all(feature = "target_hw", target_os = "linux"))]
use crate::sensors::gpio_handler::GpioHandlerError;
#[cfg(all(feature = "target_hw", target_os = "linux"))]
use crate::sensors::i2c_error::I2cError;
use crate::sensors::sensor_manager::SensorManagerError;
use crate::sensors::tank_level_switch::TankLevelSwitchError;
use crate::simulator::tcp_communication_error::TcpCommunicationError;
use crate::utilities::config_error::ConfigError;
use crate::utilities::config_file_definition_error::ConfigFileDefinitionError;
use crate::utilities::logger::LoggerSetupError;
use crate::utilities::memory_config_check::MemoryConfigError;
use crate::utilities::publish_pid::PublishPidError;
use crate::utilities::version_information::VersionInformationError;
use crate::watchmen::watchdog::WatchdogError;
use thiserror::Error;

/// Contains top-level error definitions for errors which may happen at startup.
/// No additional parameter to identify the module is required here.
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
        source: Box<SqlInterfaceError>,
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

    /// Could not initialize the interface to Gpio.
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    #[error("Could not setup interface to Gpio.")]
    ActuateGpioSetupFailure {
        #[source]
        source: ActuateGpioError,
    },

    /// Could not initialize the interface to the GPIO.
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    #[error("Could not setup the interface to the GPIO.")]
    GpioHandlerSetupFailure {
        #[source]
        source: GpioHandlerError,
    },

    /// Could not initialize the interface to the I2c.
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    #[error("Could not setup the interface to the I2C.")]
    I2cSetupFailure {
        #[source]
        source: I2cError,
    },

    /// Could not initialize the Ws2812B LED driver
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    #[error("Could not setup the Ws2812B LED driver.")]
    Ws2812BSetupFailure {
        #[source]
        source: Ws2812BError,
    },

    /// Could not initialize the tank level switch measurement and calculation.
    #[error("Could not initialize the tank level switch measurement and calculation.")]
    TankLevelSwitchSetupFailure {
        #[source]
        source: TankLevelSwitchError,
    },

    /// Could not initialize the sensor manager.
    #[error("Could not initialize the sensor manager.")]
    SensorManagerSetupFailure {
        #[source]
        source: SensorManagerError,
    },

    /// Could not initialize the Ds18b20 sensor communication.
    #[error("Could not initialize Ds18b20 sensor communication.")]
    Ds18b20SetupFailure {
        #[source]
        source: Ds18b20Error,
    },

    /// Could not check for an already running version of the application.
    #[error("Could not check for already running version of the application.")]
    PidCheckError {
        #[source]
        source: PublishPidError,
    },

    /// Could not set up watchdog.
    #[error("Could not setup watchdog.")]
    WatchdogSetupFailure {
        #[source]
        source: WatchdogError,
    },
}
