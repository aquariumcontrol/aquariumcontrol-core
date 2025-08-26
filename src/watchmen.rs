/// Contains the framework for running diagnostic checks.
pub mod monitors;

/// Contains the trait and default implementation watchdog communication.
pub mod petting;

/// Contains control of the watchdog.
pub mod watchdog;

/// Container for channels used by monitors module for communication with other threads.
pub mod monitors_channels;

/// Container for channels used by watchdog module for communication with other threads.
pub mod watchdog_channels;
