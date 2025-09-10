/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;

pub struct SignalHandlerChannels {
    // --- Channels for each module ---
    pub tx_signal_handler_to_messaging_opt: Option<AquaSender<InternalCommand>>,

    pub rx_signal_handler_from_messaging_opt: Option<AquaReceiver<bool>>,

    pub tx_signal_handler_to_refill: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_refill: AquaReceiver<bool>,

    pub tx_signal_handler_to_tank_level_switch: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_tank_level_switch: AquaReceiver<bool>,

    pub tx_signal_handler_to_heating: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_heating: AquaReceiver<bool>,

    pub tx_signal_handler_to_relay_manager: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_relay_manager: AquaReceiver<bool>,

    pub tx_signal_handler_to_atlas_scientific: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_atlas_scientific: AquaReceiver<bool>,

    pub tx_signal_handler_to_sensor_manager: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_sensor_manager: AquaReceiver<bool>,

    pub tx_signal_handler_to_data_logger: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_data_logger: AquaReceiver<bool>,

    pub tx_signal_handler_to_balling: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_balling: AquaReceiver<bool>,

    pub tx_signal_handler_to_ventilation: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_ventilation: AquaReceiver<bool>,

    pub tx_signal_handler_to_monitors: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_monitors: AquaReceiver<bool>,

    pub tx_signal_handler_to_feed: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_feed: AquaReceiver<bool>,

    pub tx_signal_handler_to_schedule_check: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_schedule_check: AquaReceiver<bool>,

    pub tx_signal_handler_to_tcp_opt: Option<AquaSender<InternalCommand>>,

    pub rx_signal_handler_from_tcp_opt: Option<AquaReceiver<bool>>,

    pub tx_signal_handler_to_watchdog: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_watchdog: AquaReceiver<bool>,

    // Hardware-specific channels
    pub tx_signal_handler_to_dht: AquaSender<InternalCommand>,

    pub tx_signal_handler_to_i2c_interface: AquaSender<InternalCommand>,

    pub tx_signal_handler_to_ds18b20: AquaSender<InternalCommand>,
}

impl SignalHandlerChannels {
    // --- Optional Modules ---
    #[allow(unused)]
    pub fn send_to_messaging(
        &mut self,
        command: InternalCommand,
    ) -> Option<Result<(), AquaChannelError>> {
        self.tx_signal_handler_to_messaging_opt
            .as_mut()
            .map(|mut tx| tx.send(command))
    }

    #[allow(unused)]
    pub fn receive_from_messaging(&mut self) -> Option<Result<bool, AquaChannelError>> {
        self.rx_signal_handler_from_messaging_opt
            .as_mut()
            .map(|rx| {
                rx.recv()
            })
    }

    #[allow(unused)]
    pub fn send_to_tcp(
        &mut self,
        command: InternalCommand,
    ) -> Option<Result<(), AquaChannelError>> {
        self.tx_signal_handler_to_tcp_opt
            .as_mut()
            .map(|mut tx| tx.send(command))
    }

    #[allow(unused)]
    pub fn receive_from_tcp(&mut self) -> Option<Result<bool, AquaChannelError>> {
        self.rx_signal_handler_from_tcp_opt.as_mut().map(|rx| {
            rx.recv()
        })
    }

    #[allow(unused)]
    pub fn send_to_refill(&mut self, command: InternalCommand) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_refill.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_refill(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_signal_handler_from_refill.recv()
    }

    #[allow(unused)]
    pub fn send_to_tank_level_switch(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_tank_level_switch.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_tank_level_switch(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_signal_handler_from_tank_level_switch.recv()
    }

    #[allow(unused)]
    pub fn send_to_heating(&mut self, command: InternalCommand) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_heating.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_heating(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_signal_handler_from_heating.recv()
    }

    #[allow(unused)]
    pub fn send_to_relay_manager(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_relay_manager.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_relay_manager(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_signal_handler_from_relay_manager.recv()
    }

    #[allow(unused)]
    pub fn send_to_atlas_scientific(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_atlas_scientific.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_atlas_scientific(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_signal_handler_from_atlas_scientific.recv()
    }

    #[allow(unused)]
    pub fn send_to_sensor_manager(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_sensor_manager.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_sensor_manager(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_signal_handler_from_sensor_manager.recv()
    }

    #[allow(unused)]
    pub fn send_to_data_logger(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_data_logger.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_data_logger(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_signal_handler_from_data_logger.recv()
    }

    #[allow(unused)]
    pub fn send_to_balling(&mut self, command: InternalCommand) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_balling.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_balling(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_signal_handler_from_balling.recv()
    }

    #[allow(unused)]
    pub fn send_to_ventilation(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_ventilation.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_ventilation(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_signal_handler_from_ventilation.recv()
    }

    #[allow(unused)]
    pub fn send_to_monitors(&mut self, command: InternalCommand) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_monitors.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_monitors(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_signal_handler_from_monitors.recv()
    }

    #[allow(unused)]
    pub fn send_to_feed(&mut self, command: InternalCommand) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_feed.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_feed(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_signal_handler_from_feed.recv()
    }

    #[allow(unused)]
    pub fn send_to_schedule_check(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_schedule_check.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_schedule_check(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_signal_handler_from_schedule_check.recv()
    }

    #[allow(unused)]
    pub fn send_to_watchdog(&mut self, command: InternalCommand) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_watchdog.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_watchdog(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_signal_handler_from_watchdog.recv()
    }

    // --- Hardware-Specific Modules (Send only) ---
    #[allow(unused)]
    pub fn send_to_dht(&mut self, command: InternalCommand) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_dht.send(command)
    }

    #[allow(unused)]
    pub fn send_to_i2c_interface(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_i2c_interface.send(command)
    }

    #[allow(unused)]
    pub fn send_to_ds18b20(&mut self, command: InternalCommand) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_ds18b20.send(command)
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for SignalHandlerChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== Signal Handler Channel Status ===")?;
        writeln!(
            f,
            "tx_signal_handler_to_messaging_opt: {}",
            self.tx_signal_handler_to_messaging_opt
                .as_ref()
                .unwrap()
                .count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_messaging_opt: {}",
            self.rx_signal_handler_from_messaging_opt.as_ref().unwrap().count
        )?;
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
        writeln!(
            f,
            "tx_signal_handler_to_tcp_opt: {}",
            self.tx_signal_handler_to_tcp_opt.as_ref().unwrap().count
        )?;
        writeln!(
            f,
            "rx_signal_handler_from_tcp_opt: {}",
            self.rx_signal_handler_from_tcp_opt.as_ref().unwrap().count
        )?;
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

        writeln!(
            f,
            "tx_signal_handler_to_dht: {}",
            self.tx_signal_handler_to_dht.count
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_i2c_interface: {}",
            self.tx_signal_handler_to_i2c_interface.count
        )?;
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}
