/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the central channel aggregation struct for the application's signal handler.
//!
//! This module provides the `SignalHandlerChannels` struct, which acts as a "switchboard"
//! by consolidating all the `mpsc` channel endpoints that the main `signal_handler`
//! thread needs to communicate with every other concurrent module in the application.
//!
//! ## Purpose and Design
//!
//! The primary purpose of this struct is to provide a single, organized container
//! for the communication infrastructure required for orchestrating a graceful,
//! application-wide shutdown.
//!
//! - **Centralization**: By gathering all channel senders (`AquaSender`) and receivers
//!   (`AquaReceiver`) into one place, it simplifies the process of passing the
//!   necessary communication lines to the `signal_handler` thread at startup.
//!
//! - **Explicit Naming**: The fields follow a strict `tx_source_to_destination` and
//!   `rx_source_from_destination` naming convention. This makes the direction of
//!   data flow for commands (`InternalCommand`) and acknowledgements (`bool`)
//!   unambiguous.
//!
//! - **Optionality**: Fields for optional modules (like `messaging` or the `tcp`
//!   simulator) are wrapped in `Option`, allowing the application to be compiled
//!   with different feature sets without breaking the communication structure.
//!
//! - **Unidirectional Communication**: Some fields are senders only, representing
//!   communication with modules that are not expected to send acknowledgements
//!   back to the signal handler (e.g., simple hardware interface threads).

use crate::launch::channels::{AquaReceiver, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

/// A container for all `mpsc` channel endpoints required by the `signal_handler`.
///
/// This struct acts as a central "switchboard" by aggregating the sender and receiver
/// handles for communication between the main signal handler thread and all other
/// concurrent modules. Its primary purpose is to simplify passing the entire communication
/// infrastructure to the signal handler upon its creation.
///
/// The fields follow a strict naming convention:
/// - `tx_source_to_destination`: A sender (`AquaSender`) from the source to the destination.
/// - `rx_source_from_destination`: A receiver (`AquaReceiver`) at the source, listening for messages from the destination.
///
/// Fields for optional modules (e.g., `messaging`, `tcp`) are wrapped in `Option`,
/// allowing for flexible compilation with different feature sets. They have a postfix `_opt`.
pub struct SignalHandlerChannels {
    /// (Optional) Sender to the `Messaging` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_messaging_opt: Option<AquaSender<InternalCommand>>,

    /// (Optional) Receiver from the `Messaging` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_messaging_opt: Option<AquaReceiver<bool>>,

    /// Sender to the `Refill` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_refill: AquaSender<InternalCommand>,

    /// Receiver from the `Refill` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_refill: AquaReceiver<bool>,

    /// Sender to the `TankLevelSwitch` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_tank_level_switch: AquaSender<InternalCommand>,

    /// Receiver from the `TankLevelSwitch` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_tank_level_switch: AquaReceiver<bool>,

    /// Sender to the `Heating` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_heating: AquaSender<InternalCommand>,

    /// Receiver from the `Heating` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_heating: AquaReceiver<bool>,

    /// Sender to the `RelayManager` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_relay_manager: AquaSender<InternalCommand>,

    /// Receiver from the `RelayManager` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_relay_manager: AquaReceiver<bool>,

    /// Sender to the `AtlasScientific` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_atlas_scientific: AquaSender<InternalCommand>,

    /// Receiver from the `AtlasScientific` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_atlas_scientific: AquaReceiver<bool>,

    /// Sender to the `SensorManager` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_sensor_manager: AquaSender<InternalCommand>,

    /// Receiver from the `SensorManager` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_sensor_manager: AquaReceiver<bool>,

    /// Sender to the `TemperatureGradient` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_temperature_gradient: AquaSender<InternalCommand>,

    /// Receiver from the `TemperatureGradient` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_temperature_gradient: AquaReceiver<bool>,

    /// Sender to the `DataLogger` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_data_logger: AquaSender<InternalCommand>,

    /// Receiver from the `DataLogger` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_data_logger: AquaReceiver<bool>,

    /// Sender to the `Balling` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_balling: AquaSender<InternalCommand>,

    /// Receiver from the `Balling` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_balling: AquaReceiver<bool>,

    /// Sender to the `Ventilation` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_ventilation: AquaSender<InternalCommand>,

    /// Receiver from the `Ventilation` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_ventilation: AquaReceiver<bool>,

    /// Sender to the `Monitors` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_monitors: AquaSender<InternalCommand>,

    /// Receiver from the `Monitors` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_monitors: AquaReceiver<bool>,

    /// Sender to the `Feed` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_feed: AquaSender<InternalCommand>,

    /// Receiver from the `Feed` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_feed: AquaReceiver<bool>,

    /// Sender to the `ScheduleCheck` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_schedule_check: AquaSender<InternalCommand>,

    /// Receiver from the `ScheduleCheck` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_schedule_check: AquaReceiver<bool>,

    /// (Optional) Sender to the `TcpCommunication` (simulator) thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_tcp_opt: Option<AquaSender<InternalCommand>>,

    /// (Optional) Receiver from the `TcpCommunication` (simulator) thread for shutdown acknowledgements.
    pub rx_signal_handler_from_tcp_opt: Option<AquaReceiver<bool>>,

    /// Sender to the `Watchdog` thread for `Quit`/`Terminate` commands.
    pub tx_signal_handler_to_watchdog: AquaSender<InternalCommand>,

    /// Receiver from the `Watchdog` thread for shutdown acknowledgements.
    pub rx_signal_handler_from_watchdog: AquaReceiver<bool>,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Sender to the `Dht` sensor thread for `Quit` command.
    pub tx_signal_handler_to_dht: AquaSender<InternalCommand>,

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    /// Sender to the `I2cInterface` thread for `Quit` command.
    pub tx_signal_handler_to_i2c_interface: AquaSender<InternalCommand>,

    /// Sender to the `Ds18b20` sensor thread for `Quit` command.
    pub tx_signal_handler_to_ds18b20: AquaSender<InternalCommand>,
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for SignalHandlerChannels {
    /// Formats the value using the given formatter.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== Signal Handler Channel Status ===")?;
        if let Some(tx_signal_handler_to_messaging) = &self.tx_signal_handler_to_messaging_opt {
            writeln!(
                f,
                "tx_signal_handler_to_messaging_opt: {}",
                tx_signal_handler_to_messaging.count
            )?;
        }
        if let Some(rx_signal_handler_from_messaging) = &self.rx_signal_handler_from_messaging_opt {
            writeln!(
                f,
                "rx_signal_handler_from_messaging_opt: {}",
                rx_signal_handler_from_messaging.count
            )?;
        }
        writeln!(
            f,
            "tx_signal_handler_to_refill: {}",
            self.tx_signal_handler_to_refill.count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_refill: {}",
            self.rx_signal_handler_from_refill.count
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_tank_level_switch: {}",
            self.tx_signal_handler_to_tank_level_switch.count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_tank_level_switch: {}",
            self.rx_signal_handler_from_tank_level_switch.count
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_heating: {}",
            self.tx_signal_handler_to_heating.count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_heating: {}",
            self.rx_signal_handler_from_heating.count
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_relay_manager: {}",
            self.tx_signal_handler_to_relay_manager.count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_relay_manager: {}",
            self.rx_signal_handler_from_relay_manager.count
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_atlas_scientific: {}",
            self.tx_signal_handler_to_atlas_scientific.count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_atlas_scientific: {}",
            self.rx_signal_handler_from_atlas_scientific.count
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_sensor_manager: {}",
            self.tx_signal_handler_to_sensor_manager.count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_sensor_manager: {}",
            self.rx_signal_handler_from_sensor_manager.count
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_temperature_gradient: {}",
            self.tx_signal_handler_to_temperature_gradient.count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_temperature_gradient: {}",
            self.rx_signal_handler_from_temperature_gradient.count
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_data_logger: {}",
            self.tx_signal_handler_to_data_logger.count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_data_logger: {}",
            self.rx_signal_handler_from_data_logger.count
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_balling: {}",
            self.tx_signal_handler_to_balling.count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_balling: {}",
            self.rx_signal_handler_from_balling.count
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_ventilation: {}",
            self.tx_signal_handler_to_ventilation.count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_ventilation: {}",
            self.rx_signal_handler_from_ventilation.count
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_monitors: {}",
            self.tx_signal_handler_to_monitors.count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_monitors: {}",
            self.rx_signal_handler_from_monitors.count
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_feed: {}",
            self.tx_signal_handler_to_feed.count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_feed: {}",
            self.rx_signal_handler_from_feed.count
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_schedule_check: {}",
            self.tx_signal_handler_to_schedule_check.count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_schedule_check: {}",
            self.rx_signal_handler_from_schedule_check.count
        )?;
        if let Some(tx_signal_handler_to_tcp) = self.tx_signal_handler_to_tcp_opt.as_ref() {
            writeln!(
                f,
                "tx_signal_handler_to_tcp_opt: {}",
                tx_signal_handler_to_tcp.count
            )?;
        }
        if let Some(rx_signal_handler_from_tcp) = self.rx_signal_handler_from_tcp_opt.as_ref() {
            writeln!(
                f,
                "rx_signal_handler_from_tcp_opt: {}",
                rx_signal_handler_from_tcp.count
            )?;
        }
        writeln!(
            f,
            "tx_signal_handler_to_watchdog: {}",
            self.tx_signal_handler_to_watchdog.count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_watchdog: {}",
            self.rx_signal_handler_from_watchdog.count
        )?;

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        writeln!(
            f,
            "tx_signal_handler_to_dht: {}",
            self.tx_signal_handler_to_dht.count
        )?;

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        if let Some(tx_signal_handler_to_i2c) = self.tx_signal_handler_to_i2c_interface_opt {
            writeln!(
                f,
                "tx_signal_handler_to_i2c_interface: {}",
                tx_signal_handler_to_i2c_interface.count
            )?;
        }
        writeln!(
            f,
            "tx_signal_handler_to_ds18b20: {}",
            self.tx_signal_handler_to_ds18b20.count
        )?;

        Ok(())
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for SignalHandlerChannels {
    /// Formats the value using the given formatter.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
