/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::food::feed_channels::FeedChannels;
use crate::mineral::balling_channels::BallingChannels;
use crate::permission::schedule_check_channels::ScheduleCheckChannels;
use crate::relays::actuate_simulator_channels::ActuateSimulatorChannels;
use crate::relays::relay_manager_channels::RelayManagerChannels;
#[allow(unused)] // used in conditionally compiled code
use crate::sensors::atlas_scientific_channels::AtlasScientificChannels;
use crate::sensors::dht_channels::DhtChannels;
use crate::sensors::ds18b20_channels::Ds18b20Channels;
use crate::sensors::i2c_interface::{I2cRequest, I2cResult};
use crate::sensors::sensor_manager_channels::SensorManagerChannels;
use crate::sensors::tank_level_switch_channels::TankLevelSwitchChannels;
use crate::simulator::tcp_communication::TcpCommunicationError;
use crate::simulator::tcp_communication_channels::{
    TcpCommunicationChannelPair, TcpCommunicationChannels,
};
use crate::thermal::heating_channels::HeatingChannels;
use crate::thermal::ventilation::VentilationChannels;
use crate::utilities::channel_content::InternalCommand;
use crate::utilities::signal_handler::SignalHandlerChannels;
use crate::watchmen::monitors_channels::MonitorsChannels;
use crate::watchmen::watchdog_channels::WatchDogChannels;
use crate::water::refill_channels::RefillChannels;
use crate::water::refill_monitor_view::RefillMonitorView;

#[cfg(target_os = "linux")]
use crate::dispatch::messaging_channels::MessagingChannels;

#[cfg(all(feature = "target_hw", target_os = "linux"))]
use crate::sensors::i2c_interface_channels::I2cInterfaceChannels;

use crate::recorder::data_logger_channels::DataLoggerChannels;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};

pub struct Channels {
    pub feed: FeedChannels,
    pub schedule_check: ScheduleCheckChannels,
    pub actuate_simulator: ActuateSimulatorChannels,
    pub relay_manager: RelayManagerChannels,
    #[cfg(all(feature = "target_hw", target_os = "linux"))]
    pub atlas_scientific: AtlasScientificChannels,
    #[allow(unused)] // used in conditionally compiled code
    pub dht: DhtChannels,
    pub ds18b20: Ds18b20Channels,
    pub balling: BallingChannels,
    pub data_logger: DataLoggerChannels,

    #[cfg(all(feature = "target_hw", target_os = "linux"))]
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

    #[cfg(target_os = "linux")]
    pub messaging: MessagingChannels,
}

impl Channels {
    pub fn new(
        relay_manager_use_simulator: bool,
        sensor_manager_use_simulator: bool,
        tank_level_switch_use_simulator: bool,
    ) -> Channels {
        let use_simulator = relay_manager_use_simulator
            | sensor_manager_use_simulator
            | tank_level_switch_use_simulator;

        // create channels for communication between threads
        let tx_signal_handler_to_messaging_opt: Option<Sender<InternalCommand>>;
        let rx_signal_handler_from_messaging_opt: Option<Receiver<bool>>;
        let rx_refill_from_messaging_opt: Option<Receiver<InternalCommand>>;
        let rx_ventilation_from_messaging_opt: Option<Receiver<InternalCommand>>;
        let rx_heating_from_messaging_opt: Option<Receiver<InternalCommand>>;
        let rx_feed_from_messaging_opt: Option<Receiver<InternalCommand>>;
        let rx_balling_from_messaging_opt: Option<Receiver<InternalCommand>>;
        let rx_monitors_from_messaging_opt: Option<Receiver<InternalCommand>>;

        #[allow(clippy::needless_late_init)]
        let rx_watchdog_from_messaging_opt: Option<Receiver<InternalCommand>>;

        cfg_if::cfg_if! {
            if #[cfg(target_os = "linux")] {
                // channels for linux-specific code (messaging)
                // *** signal_handler => messaging ***
                let (tx_signal_handler_to_messaging, rx_messaging_from_signal_handler): (
                    Sender<InternalCommand>,
                    Receiver<InternalCommand>,
                ) = mpsc::channel();
                tx_signal_handler_to_messaging_opt = Some(tx_signal_handler_to_messaging);

                let (tx_messaging_to_signal_handler, rx_signal_handler_from_messaging): (
                    Sender<bool>,
                    Receiver<bool>,
                ) = mpsc::channel();
                rx_signal_handler_from_messaging_opt = Some(rx_signal_handler_from_messaging);

                // *** messaging => refill ***
                let (tx_messaging_to_refill, rx_refill_from_messaging): (
                    Sender<InternalCommand>,
                    Receiver<InternalCommand>,
                ) = mpsc::channel();
                rx_refill_from_messaging_opt = Some(rx_refill_from_messaging);

                // *** messaging => ventilation ***
                let (tx_messaging_to_ventilation, rx_ventilation_from_messaging): (
                    Sender<InternalCommand>,
                    Receiver<InternalCommand>,
                ) = mpsc::channel();
                rx_ventilation_from_messaging_opt = Some(rx_ventilation_from_messaging);

                // *** messaging => heating ***
                let (tx_messaging_to_heating, rx_heating_from_messaging): (
                    Sender<InternalCommand>,
                    Receiver<InternalCommand>,
                ) = mpsc::channel();
                rx_heating_from_messaging_opt = Some(rx_heating_from_messaging);

                // *** messaging => feed ***
                let (tx_messaging_to_feed, rx_feed_from_messaging): (
                    Sender<InternalCommand>,
                    Receiver<InternalCommand>,
                ) = mpsc::channel();
                rx_feed_from_messaging_opt = Some(rx_feed_from_messaging);

                // *** messaging => balling ***
                let (tx_messaging_to_balling, rx_balling_from_messaging): (
                    Sender<InternalCommand>,
                    Receiver<InternalCommand>,
                ) = mpsc::channel();
                rx_balling_from_messaging_opt = Some(rx_balling_from_messaging);

                // *** messaging => monitors ***
                let (tx_messaging_to_monitors, rx_monitors_from_messaging): (
                    Sender<InternalCommand>,
                    Receiver<InternalCommand>,
                ) = mpsc::channel();
                rx_monitors_from_messaging_opt = Some(rx_monitors_from_messaging);

                // *** messaging => monitors ***
                let (tx_messaging_to_watchdog, rx_watchdog_from_messaging): (
                    Sender<InternalCommand>,
                    Receiver<InternalCommand>,
                ) = mpsc::channel();
                rx_watchdog_from_messaging_opt = Some(rx_watchdog_from_messaging);
            }
            else {
                // replacement values for channels for non-linux specific code
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
        let (tx_refill_to_relay_manager, rx_relay_manager_from_refill): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_relay_manager_to_refill, rx_refill_from_relay_manager): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // *******************************

        // *** refill <=> schedule_check ***
        let (tx_refill_to_schedule_check, rx_schedule_check_from_refill): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_schedule_check_to_refill, rx_refill_from_schedule_check): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // *******************************

        // *** heating <=> relay_manager ***
        let (tx_heating_to_relay_manager, rx_relay_manager_from_heating): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_relay_manager_to_heating, rx_heating_from_relay_manager): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // *******************************

        // *** heating <=> schedule_check ***
        let (tx_heating_to_schedule_check, rx_schedule_check_from_heating): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_schedule_check_to_heating, rx_heating_from_schedule_check): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // *******************************

        // *** ventilation <=> relay_manager ***
        let (tx_ventilation_to_relay_manager, rx_relay_manager_from_ventilation): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_relay_manager_to_ventilation, rx_ventilation_from_relay_manager): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // *******************************

        // *** ventilation <=> schedule_check ***
        let (tx_ventilation_to_schedule_check, rx_schedule_check_from_ventilation): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_schedule_check_to_ventilation, rx_ventilation_from_schedule_check): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // *******************************

        // *** feed <=> relay_manager ***
        let (tx_feed_to_relay_manager, rx_relay_manager_from_feed): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_relay_manager_to_feed, rx_feed_from_relay_manager): (Sender<bool>, Receiver<bool>) =
            mpsc::channel();
        // *******************************

        // *** balling <=> relay_manager ***
        let (tx_balling_to_relay_manager, rx_relay_manager_from_balling): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_relay_manager_to_balling, rx_balling_from_relay_manager): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // *******************************

        // *** balling <=> schedule_check ***
        let (tx_balling_to_schedule_check, rx_schedule_check_from_balling): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_schedule_check_to_balling, rx_balling_from_schedule_check): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // *******************************

        // *** refill => monitors ***
        let (tx_refill_to_monitors, rx_monitors_from_refill): (
            Sender<RefillMonitorView>,
            Receiver<RefillMonitorView>,
        ) = mpsc::channel();
        //********************************

        // *** signal handler <=> refill ***
        let (tx_signal_handler_to_refill, rx_refill_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_refill_to_signal_handler, rx_signal_handler_from_refill): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***********************************

        // *** signal handler <=> heating ***
        let (tx_signal_handler_to_heating, rx_heating_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_heating_to_signal_handler, rx_signal_handler_from_heating): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***********************************

        // *** signal handler <=> feed ***
        let (tx_signal_handler_to_feed, rx_feed_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_feed_to_signal_handler, rx_signal_handler_from_feed): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***********************************

        // *** signal handler <=> balling ***
        let (tx_signal_handler_to_balling, rx_balling_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_balling_to_signal_handler, rx_signal_handler_from_balling): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***********************************

        // *** signal handler <=> monitors ***
        let (tx_signal_handler_to_monitors, rx_monitors_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_monitors_to_signal_handler, rx_signal_handler_from_monitors): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***********************************

        // *** signal handler <=> ventilation ***
        let (tx_signal_handler_to_ventilation, rx_ventilation_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_ventilation_to_signal_handler, rx_signal_handler_from_ventilation): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***********************************

        // *** signal handler <=> schedule_check ***
        let (tx_signal_handler_to_schedule_check, rx_schedule_check_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_schedule_check_to_signal_handler, rx_signal_handler_from_schedule_check): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***********************************

        // *** signal handler <=> gpio ***
        let (tx_signal_handler_to_tank_level_switch, rx_tank_level_switch_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_tank_level_switch_to_signal_handler, rx_signal_handler_from_tank_level_switch): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // *********************************

        // *** signal handler <=> relay_manager ***
        let (tx_signal_handler_to_relay_manager, rx_relay_manager_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_relay_manager_to_signal_handler, rx_signal_handler_from_relay_manager): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // ***************************************

        // *** signal handler <=> data_logger ***
        let (tx_signal_handler_to_data_logger, rx_data_logger_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_data_logger_to_signal_handler, rx_signal_handler_from_data_logger): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // **************************************

        // *** signal handler <=> atlas_scientific ***
        #[allow(unused)] // used in conditionally compiled code
        let (tx_signal_handler_to_atlas_scientific, rx_atlas_scientific_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        #[allow(unused)] // used in conditionally compiled code
        let (tx_atlas_scientific_to_signal_handler, rx_signal_handler_from_atlas_scientific): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // **************************************

        // *** signal handler <=> watchdog ***
        let (tx_signal_handler_to_watchdog, rx_watchdog_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_watchdog_to_signal_handler, rx_signal_handler_from_watchdog): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // **************************************

        // *** signal handler <=> sensor_manager ***
        let (tx_signal_handler_to_sensor_manager, rx_sensor_manager_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_sensor_manager_to_signal_handler, rx_signal_handler_from_sensor_manager): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // **************************************

        // *** signal handler <=> tcp ***
        let (tx_signal_handler_to_tcp, rx_tcp_communication_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_tcp_communication_to_signal_handler, rx_signal_handler_from_tcp): (
            Sender<bool>,
            Receiver<bool>,
        ) = mpsc::channel();
        // *******************************

        // *** signal handler => ds18b20 ***
        let (tx_signal_handler_to_ds18b20, rx_ds18b20_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        // *******************************

        // *** relay_manager => tcp ***
        let (tx_actuate_simulator_to_tcp, rx_actuate_simulator_from_relay_manager): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        // ****************************

        // *** sensor_manager <=> tcp ***
        let (tx_sensor_manager_to_tcp, rx_tcp_communication_from_sensor_manager): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_tcp_communication_to_sensor_manager, rx_sensor_manager_from_tcp): TcpCommunicationChannelPair = mpsc::channel();
        // **********************

        // *** tank_level_switch <=> tcp ***
        let (tx_tank_level_switch_to_tcp, rx_tcp_communication_from_tank_level_switch): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();
        let (tx_tcp_communication_to_tank_level_switch, rx_tank_level_switch_from_tcp): TcpCommunicationChannelPair = mpsc::channel();
        // **********************

        // wrapping sender/receiver for communication with TCP thread
        let tx_signal_handler_to_tcp_opt: Option<Sender<InternalCommand>> = match use_simulator {
            true => Some(tx_signal_handler_to_tcp),
            false => None,
        };

        let rx_signal_handler_from_tcp_opt: Option<Receiver<bool>> = match use_simulator {
            true => Some(rx_signal_handler_from_tcp),
            false => None,
        };

        let tx_actuate_simulator_to_tcp_opt: Option<Sender<InternalCommand>> =
            match relay_manager_use_simulator {
                true => Some(tx_actuate_simulator_to_tcp),
                false => None,
            };

        let rx_tcp_communication_from_relay_manager_opt: Option<Receiver<InternalCommand>> =
            match relay_manager_use_simulator {
                true => Some(rx_actuate_simulator_from_relay_manager),
                false => None,
            };

        let tx_tank_level_switch_to_tcp_opt: Option<Sender<InternalCommand>> =
            match tank_level_switch_use_simulator {
                true => Some(tx_tank_level_switch_to_tcp),
                false => None,
            };

        let rx_tcp_communication_from_tank_level_switch_opt: Option<Receiver<InternalCommand>> =
            match tank_level_switch_use_simulator {
                true => Some(rx_tcp_communication_from_tank_level_switch),
                false => None,
            };

        let tx_tcp_communication_to_tank_level_switch_opt: Option<
            Sender<Result<f32, TcpCommunicationError>>,
        > = match tank_level_switch_use_simulator {
            true => Some(tx_tcp_communication_to_tank_level_switch),
            false => None,
        };

        let rx_tank_level_switch_from_tcp_opt: Option<
            Receiver<Result<f32, TcpCommunicationError>>,
        > = match tank_level_switch_use_simulator {
            true => Some(rx_tank_level_switch_from_tcp),
            false => None,
        };

        let tx_sensor_manager_to_tcp_opt: Option<Sender<InternalCommand>> =
            match sensor_manager_use_simulator {
                true => Some(tx_sensor_manager_to_tcp),
                false => None,
            };

        let rx_tcp_communication_from_sensor_manager_opt: Option<Receiver<InternalCommand>> =
            match sensor_manager_use_simulator {
                true => Some(rx_tcp_communication_from_sensor_manager),
                false => None,
            };

        let tx_tcp_communication_to_sensor_manager_opt: Option<
            Sender<Result<f32, TcpCommunicationError>>,
        > = match sensor_manager_use_simulator {
            true => Some(tx_tcp_communication_to_sensor_manager),
            false => None,
        };

        let rx_sensor_manager_from_tcp_opt: Option<Receiver<Result<f32, TcpCommunicationError>>> =
            match sensor_manager_use_simulator {
                true => Some(rx_sensor_manager_from_tcp),
                false => None,
            };

        #[allow(unused)] // channel used in conditionally compiled code]
        let (tx_signal_handler_to_dht, rx_dht_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();

        #[allow(unused)] // channel used in conditionally compiled code
        let (tx_signal_handler_to_i2c_interface, rx_i2c_interface_from_signal_handler): (
            Sender<InternalCommand>,
            Receiver<InternalCommand>,
        ) = mpsc::channel();

        // Atlas Scientific <=> i2c_interface
        #[allow(unused)] // channel used in conditionally compiled code
        let (tx_atlas_scientific_to_i2c_interface, rx_i2c_interface_from_atlas_scientific): (
            Sender<I2cRequest>,
            Receiver<I2cRequest>,
        ) = mpsc::channel();
        #[allow(unused)] // channel used in conditionally compiled code
        let (tx_i2c_interface_to_atlas_scientific, rx_atlas_scientific_from_i2c_interface): (
            Sender<I2cResult>,
            Receiver<I2cResult>,
        ) = mpsc::channel();

        let relay_manager = RelayManagerChannels {
            tx_relay_manager_to_refill,
            #[cfg(feature = "debug_channels")]
            cnt_tx_relay_manager_to_refill: 0,

            rx_relay_manager_from_refill,
            #[cfg(feature = "debug_channels")]
            cnt_rx_relay_manager_from_refill: 0,

            tx_relay_manager_to_heating,
            #[cfg(feature = "debug_channels")]
            cnt_tx_relay_manager_to_heating: 0,

            rx_relay_manager_from_heating,
            #[cfg(feature = "debug_channels")]
            cnt_rx_relay_manager_from_heating: 0,

            tx_relay_manager_to_ventilation,
            #[cfg(feature = "debug_channels")]
            cnt_tx_relay_manager_to_ventilation: 0,

            rx_relay_manager_from_ventilation,
            #[cfg(feature = "debug_channels")]
            cnt_rx_relay_manager_from_ventilation: 0,

            tx_relay_manager_to_balling,
            #[cfg(feature = "debug_channels")]
            cnt_tx_relay_manager_to_balling: 0,

            rx_relay_manager_from_balling,
            #[cfg(feature = "debug_channels")]
            cnt_rx_relay_manager_from_balling: 0,

            tx_relay_manager_to_feed,
            #[cfg(feature = "debug_channels")]
            cnt_tx_relay_manager_to_feed: 0,

            rx_relay_manager_from_feed,
            #[cfg(feature = "debug_channels")]
            cnt_rx_relay_manager_from_feed: 0,

            tx_relay_manager_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_relay_manager_to_signal_handler: 0,

            rx_relay_manager_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_relay_manager_from_signal_handler: 0,
        };

        #[cfg(all(feature = "target_hw", target_os = "linux"))]
        let atlas_scientific = AtlasScientificChannels {
            tx_atlas_scientific_to_i2c_interface,
            #[cfg(feature = "debug_channels")]
            cnt_tx_atlas_scientific_to_i2c_interface: 0,

            rx_atlas_scientific_from_i2c_interface,
            #[cfg(feature = "debug_channels")]
            cnt_rx_atlas_scientific_from_i2c_interface: 0,

            tx_atlas_scientific_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_atlas_scientific_to_signal_handler: 0,

            rx_atlas_scientific_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_atlas_scientific_from_signal_handler: 0,
        };

        let refill = RefillChannels {
            tx_refill_to_relay_manager,
            #[cfg(feature = "debug_channels")]
            cnt_tx_refill_to_relay_manager: 0,

            rx_refill_from_relay_manager,
            #[cfg(feature = "debug_channels")]
            cnt_rx_refill_from_relay_manager: 0,

            tx_refill_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_refill_to_signal_handler: 0,

            rx_refill_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_refill_from_signal_handler: 0,

            tx_refill_to_schedule_check,
            #[cfg(feature = "debug_channels")]
            cnt_tx_refill_to_schedule_check: 0,

            rx_refill_from_schedule_check,
            #[cfg(feature = "debug_channels")]
            cnt_rx_refill_from_schedule_check: 0,

            tx_refill_to_monitors,
            #[cfg(feature = "debug_channels")]
            cnt_tx_refill_to_monitors: 0,

            rx_refill_from_messaging_opt,
            #[cfg(feature = "debug_channels")]
            cnt_rx_refill_from_messaging_opt: 0,
        };

        let balling = BallingChannels {
            tx_balling_to_relay_manager,
            #[cfg(feature = "debug_channels")]
            cnt_tx_balling_to_relay_manager: 0,

            rx_balling_from_relay_manager,
            #[cfg(feature = "debug_channels")]
            cnt_rx_balling_from_relay_manager: 0,

            tx_balling_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_balling_to_signal_handler: 0,

            rx_balling_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_balling_from_signal_handler: 0,

            tx_balling_to_schedule_check,
            #[cfg(feature = "debug_channels")]
            cnt_tx_balling_to_schedule_check: 0,

            rx_balling_from_schedule_check,
            #[cfg(feature = "debug_channels")]
            cnt_rx_balling_from_schedule_check: 0,

            rx_balling_from_messaging_opt,
            #[cfg(feature = "debug_channels")]
            cnt_rx_balling_from_messaging_opt: 0,
        };

        let ventilation = VentilationChannels {
            tx_ventilation_to_relay_manager,
            #[cfg(feature = "debug_channels")]
            cnt_tx_ventilation_to_relay_manager: 0,

            rx_ventilation_from_relay_manager,
            #[cfg(feature = "debug_channels")]
            cnt_rx_ventilation_from_relay_manager: 0,

            tx_ventilation_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_ventilation_to_signal_handler: 0,

            rx_ventilation_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_ventilation_from_signal_handler: 0,

            tx_ventilation_to_schedule_check,
            #[cfg(feature = "debug_channels")]
            cnt_tx_ventilation_to_schedule_check: 0,

            rx_ventilation_from_schedule_check,
            #[cfg(feature = "debug_channels")]
            cnt_rx_ventilation_from_schedule_check: 0,

            rx_ventilation_from_messaging_opt,
            #[cfg(feature = "debug_channels")]
            cnt_rx_ventilation_from_messaging_opt: 0,
        };

        let feed = FeedChannels {
            tx_feed_to_relay_manager,
            #[cfg(feature = "debug_channels")]
            cnt_tx_feed_to_relay_manager: 0,

            rx_feed_from_relay_manager,
            #[cfg(feature = "debug_channels")]
            cnt_rx_feed_from_relay_manager: 0,

            tx_feed_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_feed_to_signal_handler: 0,

            rx_feed_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_feed_from_signal_handler: 0,

            rx_feed_from_messaging_opt,
            #[cfg(feature = "debug_channels")]
            cnt_rx_feed_from_messaging_opt: 0,
        };

        let heating = HeatingChannels {
            tx_heating_to_relay_manager,
            #[cfg(feature = "debug_channels")]
            cnt_tx_heating_to_relay_manager: 0,

            rx_heating_from_relay_manager,
            #[cfg(feature = "debug_channels")]
            cnt_rx_heating_from_relay_manager: 0,

            tx_heating_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_heating_to_signal_handler: 0,

            rx_heating_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_heating_from_signal_handler: 0,

            tx_heating_to_schedule_check,
            #[cfg(feature = "debug_channels")]
            cnt_tx_heating_to_schedule_check: 0,

            rx_heating_from_schedule_check,
            #[cfg(feature = "debug_channels")]
            cnt_rx_heating_from_schedule_check: 0,

            rx_heating_from_messaging_opt,
            #[cfg(feature = "debug_channels")]
            cnt_rx_heating_from_messaging_opt: 0,
        };

        let tcp_communication = TcpCommunicationChannels {
            rx_tcp_communication_from_relay_manager_opt,
            #[cfg(feature = "debug_channels")]
            cnt_rx_tcp_communication_from_relay_manager_opt: 0,

            rx_tcp_communication_from_sensor_manager_opt,
            #[cfg(feature = "debug_channels")]
            cnt_rx_tcp_communication_from_sensor_manager_opt: 0,

            tx_tcp_communication_to_sensor_manager_opt,
            #[cfg(feature = "debug_channels")]
            cnt_tx_tcp_communication_to_sensor_manager_opt: 0,

            rx_tcp_communication_from_tank_level_switch_opt,
            #[cfg(feature = "debug_channels")]
            cnt_rx_tcp_communication_from_tank_level_switch_opt: 0,

            tx_tcp_communication_to_tank_level_switch_opt,
            #[cfg(feature = "debug_channels")]
            cnt_tx_tcp_communication_to_tank_level_switch_opt: 0,

            tx_tcp_communication_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_tcp_communication_to_signal_handler: 0,

            rx_tcp_communication_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_tcp_communication_from_signal_handler: 0,
        };

        let schedule_check = ScheduleCheckChannels {
            tx_schedule_check_to_ventilation,
            #[cfg(feature = "debug_channels")]
            cnt_tx_schedule_check_to_ventilation: 0,

            rx_schedule_check_from_ventilation,
            #[cfg(feature = "debug_channels")]
            cnt_rx_schedule_check_from_ventilation: 0,

            tx_schedule_check_to_heating,
            #[cfg(feature = "debug_channels")]
            cnt_tx_schedule_check_to_heating: 0,

            rx_schedule_check_from_heating,
            #[cfg(feature = "debug_channels")]
            cnt_rx_schedule_check_from_heating: 0,

            tx_schedule_check_to_refill,
            #[cfg(feature = "debug_channels")]
            cnt_tx_schedule_check_to_refill: 0,

            rx_schedule_check_from_refill,
            #[cfg(feature = "debug_channels")]
            cnt_rx_schedule_check_from_refill: 0,

            tx_schedule_check_to_balling,
            #[cfg(feature = "debug_channels")]
            cnt_tx_schedule_check_to_balling: 0,

            rx_schedule_check_from_balling,
            #[cfg(feature = "debug_channels")]
            cnt_rx_schedule_check_from_balling: 0,

            tx_schedule_check_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_schedule_check_to_signal_handler: 0,

            rx_schedule_check_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_schedule_check_from_signal_handler: 0,
        };

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        let i2c_interface = I2cInterfaceChannels {
            tx_i2c_interface_to_atlas_scientific,
            #[cfg(feature = "debug_channels")]
            cnt_tx_i2c_interface_to_atlas_scientific: 0,

            rx_i2c_interface_from_atlas_scientific,
            #[cfg(feature = "debug_channels")]
            cnt_rx_i2c_interface_from_atlas_scientific: 0,

            rx_i2c_interface_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_i2c_interface_from_signal_handler: 0,
        };

        let signal_handler = SignalHandlerChannels {
            tx_signal_handler_to_messaging_opt,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_messaging_opt: 0,

            rx_signal_handler_from_messaging_opt,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_messaging_opt: 0,

            tx_signal_handler_to_refill,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_refill: 0,

            rx_signal_handler_from_refill,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_refill: 0,

            tx_signal_handler_to_tank_level_switch,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_tank_level_switch: 0,

            rx_signal_handler_from_tank_level_switch,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_tank_level_switch: 0,

            tx_signal_handler_to_heating,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_heating: 0,

            rx_signal_handler_from_heating,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_heating: 0,

            tx_signal_handler_to_relay_manager,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_relay_manager: 0,

            rx_signal_handler_from_relay_manager,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_relay_manager: 0,

            tx_signal_handler_to_atlas_scientific,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_atlas_scientific: 0,

            rx_signal_handler_from_atlas_scientific,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_atlas_scientific: 0,

            tx_signal_handler_to_sensor_manager,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_sensor_manager: 0,

            rx_signal_handler_from_sensor_manager,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_sensor_manager: 0,

            tx_signal_handler_to_data_logger,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_data_logger: 0,

            rx_signal_handler_from_data_logger,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_data_logger: 0,

            tx_signal_handler_to_balling,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_balling: 0,

            rx_signal_handler_from_balling,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_balling: 0,

            tx_signal_handler_to_ventilation,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_ventilation: 0,

            rx_signal_handler_from_ventilation,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_ventilation: 0,

            tx_signal_handler_to_monitors,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_monitors: 0,

            rx_signal_handler_from_monitors,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_monitors: 0,

            tx_signal_handler_to_feed,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_feed: 0,

            rx_signal_handler_from_feed,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_feed: 0,

            tx_signal_handler_to_schedule_check,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_schedule_check: 0,

            rx_signal_handler_from_schedule_check,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_schedule_check: 0,

            tx_signal_handler_to_tcp_opt,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_tcp_opt: 0,

            rx_signal_handler_from_tcp_opt,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_tcp_opt: 0,

            #[cfg(all(feature = "target_hw", target_os = "linux"))]
            tx_signal_handler_to_dht,
            #[cfg(all(feature = "target_hw", target_os = "linux", feature = "debug_channels"))]
            cnt_tx_signal_handler_to_dht: 0,

            #[cfg(all(feature = "target_hw", target_os = "linux"))]
            tx_signal_handler_to_i2c_interface,
            #[cfg(all(feature = "target_hw", target_os = "linux", feature = "debug_channels"))]
            cnt_tx_signal_handler_to_i2c_interface: 0,

            tx_signal_handler_to_watchdog,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_watchdog: 0,

            rx_signal_handler_from_watchdog,
            #[cfg(feature = "debug_channels")]
            cnt_rx_signal_handler_from_watchdog: 0,

            tx_signal_handler_to_ds18b20,
            #[cfg(feature = "debug_channels")]
            cnt_tx_signal_handler_to_ds18b20: 0,
        };

        let monitors = MonitorsChannels {
            rx_monitors_from_refill,
            #[cfg(feature = "debug_channels")]
            cnt_rx_monitors_from_refill: 0,

            tx_monitors_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_monitors_to_signal_handler: 0,

            rx_monitors_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_monitors_from_signal_handler: 0,

            rx_monitors_from_messaging_opt,
            #[cfg(feature = "debug_channels")]
            cnt_rx_monitors_from_messaging_opt: 0,
        };

        #[cfg(target_os = "linux")]
        let messaging = MessagingChannels {
            tx_messaging_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_messaging_to_signal_handler: 0,

            rx_messaging_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_messaging_from_signal_handler: 0,

            tx_messaging_to_refill,
            #[cfg(feature = "debug_channels")]
            cnt_tx_messaging_to_refill: 0,

            tx_messaging_to_ventilation,
            #[cfg(feature = "debug_channels")]
            cnt_tx_messaging_to_ventilation: 0,

            tx_messaging_to_heating,
            #[cfg(feature = "debug_channels")]
            cnt_tx_messaging_to_heating: 0,

            tx_messaging_to_feed,
            #[cfg(feature = "debug_channels")]
            cnt_tx_messaging_to_feed: 0,

            tx_messaging_to_balling,
            #[cfg(feature = "debug_channels")]
            cnt_tx_messaging_to_balling: 0,

            tx_messaging_to_monitors,
            #[cfg(feature = "debug_channels")]
            cnt_tx_messaging_to_monitors: 0,

            tx_messaging_to_watchdog,
            #[cfg(feature = "debug_channels")]
            cnt_tx_messaging_to_watchdog: 0,
        };

        let tank_level_switch = TankLevelSwitchChannels {
            tx_tank_level_switch_to_tcp_opt,
            #[cfg(feature = "debug_channels")]
            cnt_tx_tank_level_switch_to_tcp_opt: 0,

            rx_tank_level_switch_from_tcp_opt,
            #[cfg(feature = "debug_channels")]
            cnt_rx_tank_level_switch_from_tcp_opt: 0,

            tx_tank_level_switch_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_tank_level_switch_to_signal_handler: 0,

            rx_tank_level_switch_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_tank_level_switch_from_signal_handler: 0,
        };

        let watchdog = WatchDogChannels {
            tx_watchdog_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_watchdog_to_signal_handler: 0,

            rx_watchdog_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_watchdog_from_signal_handler: 0,

            rx_watchdog_from_messaging_opt,
            #[cfg(feature = "debug_channels")]
            cnt_rx_watchdog_from_messaging_opt: 0,
        };

        let ds18b20 = Ds18b20Channels {
            rx_ds18b20_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_ds18b20_from_signal_handler: 0,
        };

        let dht = DhtChannels {
            rx_dht_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_dht_from_signal_handler: 0,
        };

        let actuate_simulator = ActuateSimulatorChannels {
            tx_actuate_simulator_to_tcp_opt,
            #[cfg(feature = "debug_channels")]
            cnt_tx_actuate_simulator_to_tcp_opt: 0,
        };

        let sensor_manager = SensorManagerChannels {
            tx_sensor_manager_to_tcp_opt,
            #[cfg(feature = "debug_channels")]
            cnt_tx_sensor_manager_to_tcp_opt: 0,

            rx_sensor_manager_from_tcp_opt,
            #[cfg(feature = "debug_channels")]
            cnt_rx_sensor_manager_from_tcp_opt: 0,

            tx_sensor_manager_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_sensor_manager_to_signal_handler: 0,

            rx_sensor_manager_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_sensor_manager_from_signal_handler: 0,
        };

        let data_logger = DataLoggerChannels {
            tx_data_logger_to_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_tx_data_logger_to_signal_handler: 0,

            rx_data_logger_from_signal_handler,
            #[cfg(feature = "debug_channels")]
            cnt_rx_data_logger_from_signal_handler: 0,
        };

        Channels {
            feed,
            schedule_check,
            actuate_simulator,
            relay_manager,

            #[cfg(all(feature = "target_hw", target_os = "linux"))]
            atlas_scientific,

            dht,
            ds18b20,
            balling,
            data_logger,
            tank_level_switch,
            tcp_communication,
            heating,

            #[cfg(all(feature = "target_hw", target_os = "linux"))]
            i2c_interface,

            ventilation,
            signal_handler,
            monitors,
            watchdog,
            refill,
            sensor_manager,

            #[cfg(target_os = "linux")]
            messaging,
        }
    }
}
