/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use std::sync::mpsc::SendError;
use log::error;
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
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>>;

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

impl AcknowledgeSignalHandlerTrait for AtlasScientificChannels {
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>> {
        self.send_to_signal_handler(true)
    }

    fn location(&self) -> String {
        "atlas scientific".to_string()
    }
}

impl AcknowledgeSignalHandlerTrait for FeedChannels {
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>> {
        self.send_to_signal_handler(true)
    }

    fn location(&self) -> String {
        "feed".to_string()
    }
}

impl AcknowledgeSignalHandlerTrait for BallingChannels {
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>> {
        self.send_to_signal_handler(true)
    }

    fn location(&self) -> String {
        "balling".to_string()
    }
}

impl AcknowledgeSignalHandlerTrait for ScheduleCheckChannels {
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>> {
        self.send_to_signal_handler(true)
    }

    fn location(&self) -> String {
        "schedule_check".to_string()
    }
}

impl AcknowledgeSignalHandlerTrait for DataLoggerChannels {
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>> {
        self.send_to_signal_handler(true)
    }

    fn location(&self) -> String {
        "data_logger".to_string()
    }
}

impl AcknowledgeSignalHandlerTrait for RelayManagerChannels {
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>> {
        self.send_to_signal_handler(true)
    }

    fn location(&self) -> String {
        "relay_manager".to_string()
    }
}

impl AcknowledgeSignalHandlerTrait for SensorManagerChannels {
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>> {
        self.send_to_signal_handler(true)
    }

    fn location(&self) -> String {
        "sensor_manager".to_string()
    }
}

impl AcknowledgeSignalHandlerTrait for TankLevelSwitchChannels {
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>> {
        self.send_to_signal_handler(true)
    }

    fn location(&self) -> String {
        "tank_level_switch".to_string()
    }
}

impl AcknowledgeSignalHandlerTrait for TcpCommunicationChannels {
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>> {
        self.send_to_signal_handler(true)
    }

    fn location(&self) -> String {
        "tcp_communication".to_string()
    }
}

impl AcknowledgeSignalHandlerTrait for HeatingChannels {
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>> {
        self.send_to_signal_handler(true)
    }

    fn location(&self) -> String {
        "heating".to_string()
    }
}

impl AcknowledgeSignalHandlerTrait for VentilationChannels {
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>> {
        self.send_to_signal_handler(true)
    }

    fn location(&self) -> String {
        "ventilation".to_string()
    }
}

impl AcknowledgeSignalHandlerTrait for MonitorsChannels {
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>> {
        self.send_to_signal_handler(true)
    }

    fn location(&self) -> String {
        "monitors".to_string()
    }
}

impl AcknowledgeSignalHandlerTrait for WatchDogChannels {
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>> {
        self.send_to_signal_handler(true)
    }

    fn location(&self) -> String {
        "watchdog".to_string()
    }
}

impl AcknowledgeSignalHandlerTrait for RefillChannels {
    fn send_true_to_signal_handler(&mut self) -> Result<(), SendError<bool>> {
        self.send_to_signal_handler(true)
    }

    fn location(&self) -> String {
        "refill".to_string()
    }
}
