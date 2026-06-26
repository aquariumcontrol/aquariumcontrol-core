/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the single, top-level error type for all application startup failures.
//!
//! This module contains the `StartupError` enum, which aggregates every possible error
//! that can occur during the application's initialization phase. By consolidating all
//! potential failures into a single type, it provides a unified and robust error
//! handling mechanism for the `main` function.
//!
//! ## Design and Purpose
//!
//! The `StartupError` enum is the root of the error handling tree for the entire
//! application launch sequence. It is designed using the `thiserror` crate to provide
//! several key benefits:
//!
//! - **Unified Error Type**: Functions involved in the startup process can return a
//!   `Result<_, StartupError>`, simplifying the function signatures and error
//!   propagation logic.
//!
//! - **Automatic Conversion**: Through `#[from]` and `#[error(transparent)]`, many
//!   lower-level errors (like `ConfigError`) are automatically converted into a
//!   `StartupError` variant, reducing boilerplate code.
//!
//! - **Rich Context and Source Chaining**: Most variants use `#[source]` to wrap the
//!   original error. This preserves the full error chain, allowing logs to display not
//!   just *that* a subsystem failed to initialize, but exactly *why* it failed,
//!   including the original error from the underlying library.
//!
//! ## Error Categories
//!
//! The startup errors can be grouped into several logical categories:
//!
//! 1.  **Configuration and Environment Errors**: Failures related to loading or parsing
//!     the configuration file, invalid command-line arguments, or environment checks
//!     (e.g., `NotRoot`, `PidCheckError`).
//!
//! 2.  **Subsystem Initialization Failures**: The largest category, covering failures
//!     when setting up core components like the logger, database connection, or IPC
//!     messaging (`LoggingSetupFailure`, `Database`, `Messaging`).
//!
//! 3.  **Hardware Interface Failures**: Errors that occur when initializing communication
//!     with physical hardware, such as Controllino, GPIO pins, I2C devices, or specific
//!     sensors (`ControllinoSetupFailure`, `I2cSetupFailure`, `Ds18b20SetupFailure`).
//!     These are often conditionally compiled based on the target hardware.

#[cfg(all(feature = "target_hw", target_os = "linux"))]
use crate::beacon::ws2812b::Ws2812BError;
use crate::database::sql_interface_error::SqlInterfaceError;
#[cfg(target_os = "linux")]
use crate::dispatch::messaging_error::MessagingError;
use crate::mineral::balling_error::BallingError;
use crate::recorder::data_logger_error::DataLoggerError;
use crate::relays::actuate_controllino_error::ActuateControllinoError;
#[cfg(all(feature = "target_hw", target_os = "linux"))]
use crate::relays::relay_error::RelayError;
#[cfg(all(feature = "target_hw", target_os = "linux"))]
use crate::sensors::atlas_scientific_error::AtlasScientificError;
use crate::sensors::ds18b20_error::Ds18b20Error;
#[cfg(all(feature = "target_hw", target_os = "linux"))]
use crate::sensors::gpio_handler::GpioHandlerError;
#[cfg(all(feature = "target_hw", target_os = "linux"))]
use crate::sensors::i2c_error::I2cError;
use crate::sensors::sensor_manager::SensorManagerError;
use crate::sensors::tank_level_switch::TankLevelSwitchError;
use crate::simulator::tcp_communication_error::TcpCommunicationError;
use crate::thermal::temperature_gradient_error::TemperatureGradientError;
use crate::utilities::config_error::ConfigError;
use crate::utilities::config_file_definition_error::ConfigFileDefinitionError;
use crate::utilities::logger::LoggerSetupError;
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
        source: RelayError,
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

    /// Could not set up the watchdog.
    #[error("Could not setup watchdog.")]
    WatchdogSetupFailure {
        #[source]
        source: WatchdogError,
    },

    /// Configuration of AtlasScientific is invalid
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    #[error("Configuration of AtlasScientific is invalid.")]
    AtlasScientificSetupFailure {
        #[source]
        source: AtlasScientificError,
    },

    /// AtlasScientific sensor type mismatch
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    #[error("AtlasScientific sensors not recognized.")]
    AtlasScientificSensorTypeMismatch {
        #[source]
        source: AtlasScientificError,
    },

    /// Configuration of the temperature gradient calculation is invalid.
    #[error("Configuration of the temperature gradient calculation is invalid.")]
    TemperatureGradientConfigFailure {
        #[source]
        source: TemperatureGradientError,
    },
}
