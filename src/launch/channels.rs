/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(any(test, target_os = "linux"))]
use crate::dispatch::messaging_channels::MessagingChannels;
use crate::food::feed_channels::FeedChannels;
use crate::mineral::balling_channels::BallingChannels;
use crate::permission::schedule_check_channels::ScheduleCheckChannels;
use crate::recorder::data_logger_channels::DataLoggerChannels;
use crate::relays::actuate_simulator_channels::ActuateSimulatorChannels;
use crate::relays::relay_manager_channels::RelayManagerChannels;
#[allow(unused)] // used in conditionally compiled code
use crate::sensors::atlas_scientific_channels::AtlasScientificChannels;
use crate::sensors::dht_channels::DhtChannels;
use crate::sensors::ds18b20_channels::Ds18b20Channels;
use crate::sensors::i2c_interface::{I2cRequest, I2cResult};
use crate::sensors::i2c_interface_channels::I2cInterfaceChannels;
use crate::sensors::sensor_manager_channels::SensorManagerChannels;
use crate::sensors::tank_level_switch_channels::TankLevelSwitchChannels;
use crate::simulator::tcp_communication::TcpCommunicationError;
use crate::simulator::tcp_communication_channels::TcpCommunicationChannels;
use crate::thermal::heating_channels::HeatingChannels;
use crate::thermal::ventilation_channels::VentilationChannels;
use crate::utilities::channel_content::InternalCommand;
use crate::utilities::signal_handler::SignalHandlerChannels;
use crate::watchmen::monitors_channels::MonitorsChannels;
use crate::watchmen::watchdog_channels::WatchDogChannels;
use crate::water::refill_channels::RefillChannels;
use crate::water::refill_monitor_view::RefillMonitorView;
use std::sync::mpsc;
#[cfg(feature = "debug_channels")]
use std::sync::mpsc::SyncSender;
use std::sync::mpsc::TryRecvError;
#[cfg(feature = "debug_channels")]
use std::sync::mpsc::TrySendError;
use thiserror::Error;

#[cfg(feature = "debug_channels")]
pub struct AquaSender<T: Send> {
    sender: SyncSender<T>,
    pub count: u64,
}

#[cfg(not(feature = "debug_channels"))]
#[derive(Debug, Clone)]
pub struct AquaSender<T: Send> {
    sender: mpsc::Sender<T>,
}

pub struct AquaReceiver<T> {
    receiver: mpsc::Receiver<T>,
    #[cfg(feature = "debug_channels")]
    pub count: u64,
}

pub fn channel<T: Send>(bound: usize) -> (AquaSender<T>, AquaReceiver<T>) {
    #[cfg(feature = "debug_channels")]
    {
        let (sender, receiver) = mpsc::sync_channel::<T>(bound);
        (
            AquaSender { sender, count: 0 },
            AquaReceiver { receiver, count: 0 },
        )
    }
    #[cfg(not(feature = "debug_channels"))]
    {
        let (sender, receiver) = {
            let _ = bound;
            mpsc::channel::<T>()
        };
        (AquaSender { sender }, AquaReceiver { receiver })
    }
}

#[derive(Debug, Error, Clone)]
pub enum AquaChannelError {
    #[cfg(feature = "debug_channels")]
    #[error("Channel is full.")]
    Full,

    #[error("Channel is disconnected.")]
    Disconnected,

    #[error("Channel is empty.")]
    Empty,
}

impl<T: Send> AquaSender<T> {
    /// Send a value into a channel, and on builds with "debug_channel" feature enabled,
    /// propagate outward an error to whatever thread is over-eagerly sending content.
    pub fn send(&mut self, data: T) -> Result<(), AquaChannelError> {
        #[cfg(feature = "debug_channels")]
        {
            match self.sender.try_send(data) {
                Ok(_) => {
                    self.count += 1;
                    Ok(())
                }
                Err(TrySendError::Full(_)) => Err(AquaChannelError::Full),
                Err(TrySendError::Disconnected(_)) => Err(AquaChannelError::Disconnected),
            }
        }
        #[cfg(not(feature = "debug_channels"))]
        {
            self.sender
                .send(data)
                .map_err(|_| AquaChannelError::Disconnected)?;
            Ok(())
        }
    }
}

impl<T> AquaReceiver<T> {
    /// Receive a value from a channel (blocking), and on builds with "debug_channel" feature enabled,
    /// count each call for each receiver
    pub fn recv(&mut self) -> Result<T, AquaChannelError> {
        #[cfg(feature = "debug_channels")]
        {
            self.count += 1;
        }
        self.receiver
            .recv()
            .map_err(|_| AquaChannelError::Disconnected)
    }

    /// Receive a value from a channel (non-blocking), and on builds with "debug_channel" feature enabled,
    /// count each call for each receiver
    pub fn try_recv(&mut self) -> Result<T, AquaChannelError> {
        #[cfg(feature = "debug_channels")]
        {
            self.count += 1;
        }
        match self.receiver.try_recv() {
            Ok(c) => Ok(c),
            Err(TryRecvError::Empty) => Err(AquaChannelError::Empty),
            Err(TryRecvError::Disconnected) => Err(AquaChannelError::Disconnected),
        }
    }
}

/// Creates a pair of bidirectional channels for request/response communication.
macro_rules! create_bidirectional_channel {
    ($req:ty, $resp:ty) => {{
        let (tx_req, rx_req): (AquaSender<$req>, AquaReceiver<$req>) = channel(1);
        let (tx_resp, rx_resp): (AquaSender<$resp>, AquaReceiver<$resp>) = channel(1);
        ((tx_req, rx_req), (tx_resp, rx_resp))
    }};
}

/// Creates a channel for one-way communication.
macro_rules! create_unidirectional_channel {
    ($req:ty) => {{
        let (tx, rx): (AquaSender<$req>, AquaReceiver<$req>) = channel(1);
        (tx, rx)
    }};
}
pub struct Channels {
    pub feed: FeedChannels,
    pub schedule_check: ScheduleCheckChannels,
    pub actuate_simulator: ActuateSimulatorChannels,
    pub relay_manager: RelayManagerChannels,

    #[allow(unused)] // used in conditionally compiled code
    pub atlas_scientific: AtlasScientificChannels,

    #[allow(unused)] // used in conditionally compiled code
    pub dht: DhtChannels,
    pub ds18b20: Ds18b20Channels,
    pub balling: BallingChannels,
    pub data_logger: DataLoggerChannels,

    #[allow(unused)] // used in conditionally compiled code
    pub i2c_interface: I2cInterfaceChannels,
    pub tank_level_switch: TankLevelSwitchChannels,

    pub tcp_communication: TcpCommunicationChannels,
    pub heating: HeatingChannels,
    pub ventilation: VentilationChannels,

    pub signal_handler: SignalHandlerChannels,
    pub monitors: MonitorsChannels,
    pub watchdog: WatchDogChannels,
    pub refill: RefillChannels,
    pub sensor_manager: SensorManagerChannels,

    // used in conditionally compiled implementation and platform-independent testing
    #[cfg(any(test, target_os = "linux"))]
    pub messaging: MessagingChannels,
}

impl Channels {
    /// Creates a new set of channels for the application based on the provided simulator configuration.
    pub fn new(
        relay_manager_use_simulator: bool,
        sensor_manager_use_simulator: bool,
        tank_level_switch_use_simulator: bool,
    ) -> Channels {
        // The public `new` function is now just a clean wrapper.
        Self::build(
            relay_manager_use_simulator,
            sensor_manager_use_simulator,
            tank_level_switch_use_simulator,
        )
    }

    /// Creates a new set of channels with all simulators enabled for comprehensive unit testing.
    #[cfg(test)]
    pub fn new_for_test() -> Channels {
        Self::build(true, true, true)
    }

    pub fn build(
        relay_manager_use_simulator: bool,
        sensor_manager_use_simulator: bool,
        tank_level_switch_use_simulator: bool,
    ) -> Channels {
        let use_simulator = relay_manager_use_simulator
            | sensor_manager_use_simulator
            | tank_level_switch_use_simulator;

        // create channels for communication between threads
        let tx_signal_handler_to_messaging_opt: Option<AquaSender<InternalCommand>>;
        let rx_signal_handler_from_messaging_opt: Option<AquaReceiver<bool>>;
        let rx_refill_from_messaging_opt: Option<AquaReceiver<InternalCommand>>;
        let rx_ventilation_from_messaging_opt: Option<AquaReceiver<InternalCommand>>;
        let rx_heating_from_messaging_opt: Option<AquaReceiver<InternalCommand>>;
        let rx_feed_from_messaging_opt: Option<AquaReceiver<InternalCommand>>;
        let rx_balling_from_messaging_opt: Option<AquaReceiver<InternalCommand>>;
        let rx_monitors_from_messaging_opt: Option<AquaReceiver<InternalCommand>>;

        #[allow(clippy::needless_late_init)]
        let rx_watchdog_from_messaging_opt: Option<AquaReceiver<InternalCommand>>;

        // channels for messaging used in Linux implementation and on unit testing on any platform
        cfg_if::cfg_if! {
            if #[cfg(any(test, target_os = "linux"))] {
                // *** signal_handler <=> messaging ***
                let ((tx_signal_handler_to_messaging, rx_messaging_from_signal_handler),
                    (tx_messaging_to_signal_handler, rx_signal_handler_from_messaging))
                    = create_bidirectional_channel!(InternalCommand, bool);

                tx_signal_handler_to_messaging_opt = Some(tx_signal_handler_to_messaging);
                rx_signal_handler_from_messaging_opt = Some(rx_signal_handler_from_messaging);

                // *** messaging => refill ***
                let (tx_messaging_to_refill, rx_refill_from_messaging) = create_unidirectional_channel!(InternalCommand);
                rx_refill_from_messaging_opt = Some(rx_refill_from_messaging);

                // *** messaging => ventilation ***
                let (tx_messaging_to_ventilation, rx_ventilation_from_messaging) = create_unidirectional_channel!(InternalCommand);
                rx_ventilation_from_messaging_opt = Some(rx_ventilation_from_messaging);

                // *** messaging => heating ***
                let (tx_messaging_to_heating, rx_heating_from_messaging) = create_unidirectional_channel!(InternalCommand);
                rx_heating_from_messaging_opt = Some(rx_heating_from_messaging);

                // *** messaging => feed ***
                let (tx_messaging_to_feed, rx_feed_from_messaging) = create_unidirectional_channel!(InternalCommand);
                rx_feed_from_messaging_opt = Some(rx_feed_from_messaging);

                // *** messaging => balling ***
                let (tx_messaging_to_balling, rx_balling_from_messaging) = create_unidirectional_channel!(InternalCommand);
                rx_balling_from_messaging_opt = Some(rx_balling_from_messaging);

                // *** messaging => monitors ***
                let (tx_messaging_to_monitors, rx_monitors_from_messaging) = create_unidirectional_channel!(InternalCommand);
                rx_monitors_from_messaging_opt = Some(rx_monitors_from_messaging);

                // *** messaging => monitors ***
                let (tx_messaging_to_watchdog, rx_watchdog_from_messaging) = create_unidirectional_channel!(InternalCommand);
                rx_watchdog_from_messaging_opt = Some(rx_watchdog_from_messaging);
            }
            else {
                tx_signal_handler_to_messaging_opt = None;
                rx_signal_handler_from_messaging_opt = None;
                rx_refill_from_messaging_opt = None;
                rx_ventilation_from_messaging_opt = None;
                rx_heating_from_messaging_opt = None;
                rx_feed_from_messaging_opt = None;
                rx_balling_from_messaging_opt = None;
                rx_monitors_from_messaging_opt = None;
                rx_watchdog_from_messaging_opt = None;
            }
        }

        // *** refill <=> relay_manager ***
        let (
            (tx_refill_to_relay_manager, rx_relay_manager_from_refill),
            (tx_relay_manager_to_refill, rx_refill_from_relay_manager),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // *******************************

        // *** refill <=> schedule_check ***
        let (
            (tx_refill_to_schedule_check, rx_schedule_check_from_refill),
            (tx_schedule_check_to_refill, rx_refill_from_schedule_check),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // *******************************

        // *** heating <=> relay_manager ***
        let (
            (tx_heating_to_relay_manager, rx_relay_manager_from_heating),
            (tx_relay_manager_to_heating, rx_heating_from_relay_manager),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // *******************************

        // *** heating <=> schedule_check ***
        let (
            (tx_heating_to_schedule_check, rx_schedule_check_from_heating),
            (tx_schedule_check_to_heating, rx_heating_from_schedule_check),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // *******************************

        // *** ventilation <=> relay_manager ***
        let (
            (tx_ventilation_to_relay_manager, rx_relay_manager_from_ventilation),
            (tx_relay_manager_to_ventilation, rx_ventilation_from_relay_manager),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // *******************************

        // *** ventilation <=> schedule_check ***
        let (
            (tx_ventilation_to_schedule_check, rx_schedule_check_from_ventilation),
            (tx_schedule_check_to_ventilation, rx_ventilation_from_schedule_check),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // *******************************

        // *** feed <=> relay_manager ***
        let (
            (tx_feed_to_relay_manager, rx_relay_manager_from_feed),
            (tx_relay_manager_to_feed, rx_feed_from_relay_manager),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // *******************************

        // *** balling <=> relay_manager ***
        let (
            (tx_balling_to_relay_manager, rx_relay_manager_from_balling),
            (tx_relay_manager_to_balling, rx_balling_from_relay_manager),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // *******************************

        // *** balling <=> schedule_check ***
        let (
            (tx_balling_to_schedule_check, rx_schedule_check_from_balling),
            (tx_schedule_check_to_balling, rx_balling_from_schedule_check),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // *******************************

        // *** refill => monitors ***
        let (tx_refill_to_monitors, rx_monitors_from_refill) =
            create_unidirectional_channel!(RefillMonitorView);
        //********************************

        // *** signal handler <=> refill ***
        let (
            (tx_signal_handler_to_refill, rx_refill_from_signal_handler),
            (tx_refill_to_signal_handler, rx_signal_handler_from_refill),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // ***********************************

        // *** signal handler <=> heating ***
        let (
            (tx_signal_handler_to_heating, rx_heating_from_signal_handler),
            (tx_heating_to_signal_handler, rx_signal_handler_from_heating),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // ***********************************

        // *** signal handler <=> feed ***
        let (
            (tx_signal_handler_to_feed, rx_feed_from_signal_handler),
            (tx_feed_to_signal_handler, rx_signal_handler_from_feed),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // ***********************************

        // *** signal handler <=> balling ***
        let (
            (tx_signal_handler_to_balling, rx_balling_from_signal_handler),
            (tx_balling_to_signal_handler, rx_signal_handler_from_balling),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // ***********************************

        // *** signal handler <=> monitors ***
        let (
            (tx_signal_handler_to_monitors, rx_monitors_from_signal_handler),
            (tx_monitors_to_signal_handler, rx_signal_handler_from_monitors),
        ) = create_bidirectional_channel!(InternalCommand, bool);

        // ***********************************

        // *** signal handler <=> ventilation ***
        let (
            (tx_signal_handler_to_ventilation, rx_ventilation_from_signal_handler),
            (tx_ventilation_to_signal_handler, rx_signal_handler_from_ventilation),
        ) = create_bidirectional_channel!(InternalCommand, bool);

        // ***********************************

        // *** signal handler <=> schedule_check ***
        let (
            (tx_signal_handler_to_schedule_check, rx_schedule_check_from_signal_handler),
            (tx_schedule_check_to_signal_handler, rx_signal_handler_from_schedule_check),
        ) = create_bidirectional_channel!(InternalCommand, bool);

        // ***********************************

        // *** signal handler <=> gpio ***
        let (
            (tx_signal_handler_to_tank_level_switch, rx_tank_level_switch_from_signal_handler),
            (tx_tank_level_switch_to_signal_handler, rx_signal_handler_from_tank_level_switch),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // *********************************

        // *** signal handler <=> relay_manager ***
        let (
            (tx_signal_handler_to_relay_manager, rx_relay_manager_from_signal_handler),
            (tx_relay_manager_to_signal_handler, rx_signal_handler_from_relay_manager),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // ***************************************

        // *** signal handler <=> data_logger ***
        let (
            (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler),
            (tx_data_logger_to_signal_handler, rx_signal_handler_from_data_logger),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // **************************************

        // *** signal handler <=> atlas_scientific ***
        #[allow(unused)] // used in conditionally compiled code
        let (
            (tx_signal_handler_to_atlas_scientific, rx_atlas_scientific_from_signal_handler),
            (tx_atlas_scientific_to_signal_handler, rx_signal_handler_from_atlas_scientific),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // **************************************

        // *** signal handler <=> watchdog ***
        let (
            (tx_signal_handler_to_watchdog, rx_watchdog_from_signal_handler),
            (tx_watchdog_to_signal_handler, rx_signal_handler_from_watchdog),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // **************************************

        // *** signal handler <=> sensor_manager ***
        let (
            (tx_signal_handler_to_sensor_manager, rx_sensor_manager_from_signal_handler),
            (tx_sensor_manager_to_signal_handler, rx_signal_handler_from_sensor_manager),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // **************************************

        // *** signal handler <=> tcp ***
        let (
            (tx_signal_handler_to_tcp, rx_tcp_communication_from_signal_handler),
            (tx_tcp_communication_to_signal_handler, rx_signal_handler_from_tcp),
        ) = create_bidirectional_channel!(InternalCommand, bool);
        // *******************************

        // *** signal handler => ds18b20 ***
        let (tx_signal_handler_to_ds18b20, rx_ds18b20_from_signal_handler) =
            create_unidirectional_channel!(InternalCommand);
        // *******************************

        // *** relay_manager => tcp ***
        let (tx_actuate_simulator_to_tcp, rx_actuate_simulator_from_relay_manager) =
            create_unidirectional_channel!(InternalCommand);
        // ****************************

        // *** sensor_manager <=> tcp ***
        let (
            (tx_sensor_manager_to_tcp, rx_tcp_communication_from_sensor_manager),
            (tx_tcp_communication_to_sensor_manager, rx_sensor_manager_from_tcp),
        ) = create_bidirectional_channel!(InternalCommand, Result<f32, TcpCommunicationError>);
        // **********************

        // *** tank_level_switch <=> tcp ***
        let (
            (tx_tank_level_switch_to_tcp, rx_tcp_communication_from_tank_level_switch),
            (tx_tcp_communication_to_tank_level_switch, rx_tank_level_switch_from_tcp),
        ) = create_bidirectional_channel!(InternalCommand, Result<f32, TcpCommunicationError>);
        // **********************

        // wrapping sender/receiver for communication with TCP thread
        let tx_signal_handler_to_tcp_opt = use_simulator.then_some(tx_signal_handler_to_tcp);
        let rx_signal_handler_from_tcp_opt = use_simulator.then_some(rx_signal_handler_from_tcp);
        let tx_actuate_simulator_to_tcp_opt = use_simulator.then_some(tx_actuate_simulator_to_tcp);
        let rx_tcp_communication_from_relay_manager_opt =
            relay_manager_use_simulator.then_some(rx_actuate_simulator_from_relay_manager);
        let tx_tank_level_switch_to_tcp_opt =
            tank_level_switch_use_simulator.then_some(tx_tank_level_switch_to_tcp);
        let rx_tcp_communication_from_tank_level_switch_opt =
            tank_level_switch_use_simulator.then_some(rx_tcp_communication_from_tank_level_switch);
        let tx_tcp_communication_to_tank_level_switch_opt =
            tank_level_switch_use_simulator.then_some(tx_tcp_communication_to_tank_level_switch);
        let rx_tank_level_switch_from_tcp_opt =
            tank_level_switch_use_simulator.then_some(rx_tank_level_switch_from_tcp);
        let tx_sensor_manager_to_tcp_opt =
            sensor_manager_use_simulator.then_some(tx_sensor_manager_to_tcp);
        let rx_tcp_communication_from_sensor_manager_opt =
            sensor_manager_use_simulator.then_some(rx_tcp_communication_from_sensor_manager);
        let tx_tcp_communication_to_sensor_manager_opt =
            sensor_manager_use_simulator.then_some(tx_tcp_communication_to_sensor_manager);
        let rx_sensor_manager_from_tcp_opt =
            sensor_manager_use_simulator.then_some(rx_sensor_manager_from_tcp);

        #[allow(unused)] // channel used in conditionally compiled code
        let (tx_signal_handler_to_dht, rx_dht_from_signal_handler) =
            create_unidirectional_channel!(InternalCommand);

        #[allow(unused)] // channel used in conditionally compiled code
        let (tx_signal_handler_to_i2c_interface, rx_i2c_interface_from_signal_handler) =
            create_unidirectional_channel!(InternalCommand);

        // Atlas Scientific <=> i2c_interface
        #[allow(unused)] // channel used in conditionally compiled code
        let (
            (tx_atlas_scientific_to_i2c_interface, rx_i2c_interface_from_atlas_scientific),
            (tx_i2c_interface_to_atlas_scientific, rx_atlas_scientific_from_i2c_interface),
        ) = create_bidirectional_channel!(I2cRequest, I2cResult);

        let relay_manager = RelayManagerChannels {
            tx_relay_manager_to_refill,
            rx_relay_manager_from_refill,
            tx_relay_manager_to_heating,
            rx_relay_manager_from_heating,
            tx_relay_manager_to_ventilation,
            rx_relay_manager_from_ventilation,
            tx_relay_manager_to_balling,
            rx_relay_manager_from_balling,
            tx_relay_manager_to_feed,
            rx_relay_manager_from_feed,
            tx_relay_manager_to_signal_handler,
            rx_relay_manager_from_signal_handler,
        };

        let atlas_scientific = AtlasScientificChannels {
            tx_atlas_scientific_to_i2c_interface,
            rx_atlas_scientific_from_i2c_interface,
            tx_atlas_scientific_to_signal_handler,
            rx_atlas_scientific_from_signal_handler,
        };

        let refill = RefillChannels {
            tx_refill_to_relay_manager,
            rx_refill_from_relay_manager,
            tx_refill_to_signal_handler,
            rx_refill_from_signal_handler,
            tx_refill_to_schedule_check,
            rx_refill_from_schedule_check,
            tx_refill_to_monitors,
            rx_refill_from_messaging_opt,
        };

        let balling = BallingChannels {
            tx_balling_to_relay_manager,
            rx_balling_from_relay_manager,
            tx_balling_to_signal_handler,
            rx_balling_from_signal_handler,
            tx_balling_to_schedule_check,
            rx_balling_from_schedule_check,
            rx_balling_from_messaging_opt,
        };

        let ventilation = VentilationChannels {
            tx_ventilation_to_relay_manager,
            rx_ventilation_from_relay_manager,
            tx_ventilation_to_signal_handler,
            rx_ventilation_from_signal_handler,
            tx_ventilation_to_schedule_check,
            rx_ventilation_from_schedule_check,
            rx_ventilation_from_messaging_opt,
        };

        let feed = FeedChannels {
            tx_feed_to_relay_manager,
            rx_feed_from_relay_manager,
            tx_feed_to_signal_handler,
            rx_feed_from_signal_handler,
            rx_feed_from_messaging_opt,
        };

        let heating = HeatingChannels {
            tx_heating_to_relay_manager,
            rx_heating_from_relay_manager,
            tx_heating_to_signal_handler,
            rx_heating_from_signal_handler,
            tx_heating_to_schedule_check,
            rx_heating_from_schedule_check,
            rx_heating_from_messaging_opt,
        };

        let tcp_communication = TcpCommunicationChannels {
            rx_tcp_communication_from_relay_manager_opt,
            rx_tcp_communication_from_sensor_manager_opt,
            tx_tcp_communication_to_sensor_manager_opt,
            rx_tcp_communication_from_tank_level_switch_opt,
            tx_tcp_communication_to_tank_level_switch_opt,
            tx_tcp_communication_to_signal_handler,
            rx_tcp_communication_from_signal_handler,
        };

        let schedule_check = ScheduleCheckChannels {
            tx_schedule_check_to_ventilation,
            rx_schedule_check_from_ventilation,
            tx_schedule_check_to_heating,
            rx_schedule_check_from_heating,
            tx_schedule_check_to_refill,
            rx_schedule_check_from_refill,
            tx_schedule_check_to_balling,
            rx_schedule_check_from_balling,
            tx_schedule_check_to_signal_handler,
            rx_schedule_check_from_signal_handler,
        };

        let i2c_interface = I2cInterfaceChannels {
            tx_i2c_interface_to_atlas_scientific,
            rx_i2c_interface_from_atlas_scientific,
            rx_i2c_interface_from_signal_handler,
        };

        let signal_handler = SignalHandlerChannels {
            tx_signal_handler_to_messaging_opt,
            rx_signal_handler_from_messaging_opt,
            tx_signal_handler_to_refill,
            rx_signal_handler_from_refill,
            tx_signal_handler_to_tank_level_switch,
            rx_signal_handler_from_tank_level_switch,
            tx_signal_handler_to_heating,
            rx_signal_handler_from_heating,
            tx_signal_handler_to_relay_manager,
            rx_signal_handler_from_relay_manager,
            tx_signal_handler_to_atlas_scientific,
            rx_signal_handler_from_atlas_scientific,
            tx_signal_handler_to_sensor_manager,
            rx_signal_handler_from_sensor_manager,
            tx_signal_handler_to_data_logger,
            rx_signal_handler_from_data_logger,
            tx_signal_handler_to_balling,
            rx_signal_handler_from_balling,
            tx_signal_handler_to_ventilation,
            rx_signal_handler_from_ventilation,
            tx_signal_handler_to_monitors,
            rx_signal_handler_from_monitors,
            tx_signal_handler_to_feed,
            rx_signal_handler_from_feed,
            tx_signal_handler_to_schedule_check,
            rx_signal_handler_from_schedule_check,
            tx_signal_handler_to_tcp_opt,
            rx_signal_handler_from_tcp_opt,
            tx_signal_handler_to_dht,
            tx_signal_handler_to_i2c_interface,
            tx_signal_handler_to_watchdog,
            rx_signal_handler_from_watchdog,
            tx_signal_handler_to_ds18b20,
        };

        let monitors = MonitorsChannels {
            rx_monitors_from_refill,
            tx_monitors_to_signal_handler,
            rx_monitors_from_signal_handler,
            rx_monitors_from_messaging_opt,
        };

        #[cfg(any(test, target_os = "linux"))]
        let messaging = MessagingChannels {
            tx_messaging_to_signal_handler,
            rx_messaging_from_signal_handler,
            tx_messaging_to_refill,
            tx_messaging_to_ventilation,
            tx_messaging_to_heating,
            tx_messaging_to_feed,
            tx_messaging_to_balling,
            tx_messaging_to_monitors,
            tx_messaging_to_watchdog,
        };

        let tank_level_switch = TankLevelSwitchChannels {
            tx_tank_level_switch_to_tcp_opt,
            rx_tank_level_switch_from_tcp_opt,
            tx_tank_level_switch_to_signal_handler,
            rx_tank_level_switch_from_signal_handler,
        };

        let watchdog = WatchDogChannels {
            tx_watchdog_to_signal_handler,
            rx_watchdog_from_signal_handler,
            rx_watchdog_from_messaging_opt,
        };

        let ds18b20 = Ds18b20Channels {
            rx_ds18b20_from_signal_handler,
        };

        let dht = DhtChannels {
            rx_dht_from_signal_handler,
        };

        let actuate_simulator = ActuateSimulatorChannels {
            tx_actuate_simulator_to_tcp_opt,
        };

        let sensor_manager = SensorManagerChannels {
            tx_sensor_manager_to_tcp_opt,
            rx_sensor_manager_from_tcp_opt,
            tx_sensor_manager_to_signal_handler,
            rx_sensor_manager_from_signal_handler,
        };

        let data_logger = DataLoggerChannels {
            tx_data_logger_to_signal_handler,
            rx_data_logger_from_signal_handler,
        };

        Channels {
            feed,
            schedule_check,
            actuate_simulator,
            relay_manager,
            atlas_scientific,
            dht,
            ds18b20,
            balling,
            data_logger,
            tank_level_switch,
            tcp_communication,
            heating,
            i2c_interface,
            ventilation,
            signal_handler,
            monitors,
            watchdog,
            refill,
            sensor_manager,

            #[cfg(any(test, target_os = "linux"))]
            messaging,
        }
    }
}
