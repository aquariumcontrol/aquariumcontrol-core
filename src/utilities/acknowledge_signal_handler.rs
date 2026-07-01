/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines a trait for acknowledging shutdown commands from the `SignalHandler`.
//!
//! This module provides the `AcknowledgeSignalHandlerTrait`, an abstraction used to standardize
//! the shutdown acknowledgment process across all concurrent threads in the application.
//!
//! ## Purpose and Design
//!
//! During a graceful shutdown, the `SignalHandler` sends a `Quit` command to all
//! running threads in a specific sequence. It waits for some threads to send back a
//! confirmation message (`true`) to maintain the correct shutdown sequence.
//!
//! This trait encapsulates the boilerplate logic for sending this confirmation. Instead
//! of each thread implementing its own error-handling and logging for the send operation,
//! it can simply implement this trait on its `...Channels` struct and call the provided
//! `acknowledge_signal_handler()` method.
//!
//! This design achieves several goals:
//! - **Reduces Boilerplate**: Eliminates repetitive error-handling code in every thread.
//! - **Enforces a Protocol**: Provides a single, consistent way for threads to participate
//!   in the shutdown sequence.
//! - **Improves Readability**: Makes the shutdown logic within each thread's `execute`
//!   loop cleaner and more explicit.
//!
//! ## Key Components
//!
//! - **`AcknowledgeSignalHandlerTrait`**: The core trait of the module.
//!   - **`send_true_to_signal_handler()`**: The required method that each implementor
//!     must provide to perform the actual channel send operation.
//!   - **`location()`**: A required method that provides a string identifier for the
//!     implementing module, used for contextual logging.
//!   - **`acknowledge_signal_handler()`**: A provided "default" method that wraps the
//!     send-call with standardized error logging. This is the method that threads
//!     should call.

use crate::food::feed_channels::FeedChannels;
use crate::mineral::balling_channels::BallingChannels;
use crate::permission::schedule_check_channels::ScheduleCheckChannels;
use crate::recorder::data_logger_channels::DataLoggerChannels;
use crate::relays::relay_manager_channels::RelayManagerChannels;
use crate::sensors::atlas_scientific_channels::AtlasScientificChannels;
use crate::sensors::sensor_manager_channels::SensorManagerChannels;
use crate::sensors::tank_level_switch_channels::TankLevelSwitchChannels;
use crate::simulator::tcp_communication_channels::TcpCommunicationChannels;
use crate::thermal::heating_channels::HeatingChannels;
use crate::thermal::ventilation_channels::VentilationChannels;
use crate::watchmen::monitors_channels::MonitorsChannels;
use crate::watchmen::watchdog_channels::WatchDogChannels;
use crate::water::refill_channels::RefillChannels;
use log::error;

#[cfg(target_os = "linux")]
use crate::dispatch::messaging_channels::MessagingChannels;
use crate::launch::channels::AquaChannelError;
use crate::thermal::temperature_gradient_channels::TemperatureGradientChannels;

/// A trait for objects that can send a shutdown acknowledgment to the `SignalHandler`.
///
/// This trait abstracts the boilerplate logic required for a thread to confirm that it has
/// received a `Quit` command and is ready to terminate. By implementing this trait on a
/// module's channel struct, the module can simply call `acknowledge_signal_handler()`
/// to handle the confirmation and logging automatically.
pub trait AcknowledgeSignalHandlerTrait {
    /// Sends the raw acknowledgment message (`true`) to the `SignalHandler`.
    ///
    /// This method must be implemented by any struct that uses the trait. It is responsible
    /// for the actual send-operation to the channel.
    fn send_true_to_signal_handler(&mut self) -> Result<(), AquaChannelError>;

    /// Returns the source location (module path) for logging purposes.
    ///
    /// This method provides context for error logs, allowing developers to quickly
    /// identify which thread failed to send its acknowledgment.
    fn location(&self) -> String;

    /// Acknowledges the `SignalHandler` by sending a confirmation message.
    ///
    /// This is the primary method that threads should call during shutdown.
    /// It wraps the call to `send_true_to_signal_handler` with error handling,
    /// automatically logging a detailed error message if the sending to the channel fails.
    fn acknowledge_signal_handler(&mut self) {
        if let Err(e) = self.send_true_to_signal_handler() {
            error!(
                target: &self.location(),
                "sending quit confirmation back to signal handler failed ({e:?})"
            );
        }
    }
}

/// A macro to generate the `AcknowledgeSignalHandlerTrait` implementation for a given struct.
macro_rules! impl_acknowledge_signal_handler {
    // The macro takes the struct type, the location string and the channel as arguments.
    ($struct_name:ty, $location_str:expr, $channel:ident) => {
        impl AcknowledgeSignalHandlerTrait for $struct_name {
            /// Performs the `send_true_to_signal_handler` operation.
            fn send_true_to_signal_handler(&mut self) -> Result<(), AquaChannelError> {
                self.$channel.send(true)
            }

            /// Performs the `location` operation.
            fn location(&self) -> String {
                $location_str.to_string()
            }
        }
    };
}

// Generate the implementations using the macro
impl_acknowledge_signal_handler!(
    TemperatureGradientChannels,
    "temperature_gradient",
    tx_temperature_gradient_to_signal_handler
);
// Generate the implementations using the macro
impl_acknowledge_signal_handler!(
    AtlasScientificChannels,
    "atlas scientific",
    tx_atlas_scientific_to_signal_handler
);
impl_acknowledge_signal_handler!(FeedChannels, "feed", tx_feed_to_signal_handler);
impl_acknowledge_signal_handler!(BallingChannels, "balling", tx_balling_to_signal_handler);
impl_acknowledge_signal_handler!(
    ScheduleCheckChannels,
    "schedule_check",
    tx_schedule_check_to_signal_handler
);
impl_acknowledge_signal_handler!(
    DataLoggerChannels,
    "data_logger",
    tx_data_logger_to_signal_handler
);
impl_acknowledge_signal_handler!(
    RelayManagerChannels,
    "relay_manager",
    tx_relay_manager_to_signal_handler
);
impl_acknowledge_signal_handler!(
    SensorManagerChannels,
    "sensor_manager",
    tx_sensor_manager_to_signal_handler
);
impl_acknowledge_signal_handler!(
    TankLevelSwitchChannels,
    "tank_level_switch",
    tx_tank_level_switch_to_signal_handler
);
impl_acknowledge_signal_handler!(
    TcpCommunicationChannels,
    "tcp_communication",
    tx_tcp_communication_to_signal_handler
);
impl_acknowledge_signal_handler!(HeatingChannels, "heating", tx_heating_to_signal_handler);
impl_acknowledge_signal_handler!(
    VentilationChannels,
    "ventilation",
    tx_ventilation_to_signal_handler
);
impl_acknowledge_signal_handler!(MonitorsChannels, "monitors", tx_monitors_to_signal_handler);
impl_acknowledge_signal_handler!(WatchDogChannels, "watchdog", tx_watchdog_to_signal_handler);
impl_acknowledge_signal_handler!(RefillChannels, "refill", tx_refill_to_signal_handler);

#[cfg(target_os = "linux")]
impl_acknowledge_signal_handler!(
    MessagingChannels,
    "messaging",
    tx_messaging_to_signal_handler
);
