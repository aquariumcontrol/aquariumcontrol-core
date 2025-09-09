/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::launch::channels::{AquaChannelError, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use std::fmt;
use std::sync::mpsc::{Receiver, RecvError};

pub struct SignalHandlerChannels {
    // --- Channels for each module ---
    pub tx_signal_handler_to_messaging_opt: Option<AquaSender<InternalCommand>>,

    pub rx_signal_handler_from_messaging_opt: Option<Receiver<bool>>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_messaging_opt: u64,

    pub tx_signal_handler_to_refill: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_refill: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_refill: u64,

    pub tx_signal_handler_to_tank_level_switch: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_tank_level_switch: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_tank_level_switch: u64,

    pub tx_signal_handler_to_heating: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_heating: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_heating: u64,

    pub tx_signal_handler_to_relay_manager: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_relay_manager: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_relay_manager: u64,

    pub tx_signal_handler_to_atlas_scientific: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_atlas_scientific: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_atlas_scientific: u64,

    pub tx_signal_handler_to_sensor_manager: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_sensor_manager: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_sensor_manager: u64,

    pub tx_signal_handler_to_data_logger: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_data_logger: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_data_logger: u64,

    pub tx_signal_handler_to_balling: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_balling: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_balling: u64,

    pub tx_signal_handler_to_ventilation: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_ventilation: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_ventilation: u64,

    pub tx_signal_handler_to_monitors: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_monitors: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_monitors: u64,

    pub tx_signal_handler_to_feed: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_feed: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_feed: u64,

    pub tx_signal_handler_to_schedule_check: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_schedule_check: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_schedule_check: u64,

    pub tx_signal_handler_to_tcp_opt: Option<AquaSender<InternalCommand>>,

    pub rx_signal_handler_from_tcp_opt: Option<Receiver<bool>>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_tcp_opt: u64,

    pub tx_signal_handler_to_watchdog: AquaSender<InternalCommand>,

    pub rx_signal_handler_from_watchdog: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_watchdog: u64,

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
    pub fn receive_from_messaging(&mut self) -> Option<Result<bool, RecvError>> {
        self.rx_signal_handler_from_messaging_opt
            .as_ref()
            .map(|rx| {
                let result = rx.recv();
                #[cfg(feature = "debug_channels")]
                if result.is_ok() {
                    self.cnt_rx_signal_handler_from_messaging_opt += 1;
                }
                result
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
    pub fn receive_from_tcp(&mut self) -> Option<Result<bool, RecvError>> {
        self.rx_signal_handler_from_tcp_opt.as_ref().map(|rx| {
            let result = rx.recv();
            #[cfg(feature = "debug_channels")]
            if result.is_ok() {
                self.cnt_rx_signal_handler_from_tcp_opt += 1;
            }
            result
        })
    }

    #[allow(unused)]
    pub fn send_to_refill(&mut self, command: InternalCommand) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_refill.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_refill(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_signal_handler_from_refill.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_signal_handler_from_refill += 1;
        }
        result
    }

    #[allow(unused)]
    pub fn send_to_tank_level_switch(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_tank_level_switch.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_tank_level_switch(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_signal_handler_from_tank_level_switch.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_signal_handler_from_tank_level_switch += 1;
        }
        result
    }

    #[allow(unused)]
    pub fn send_to_heating(&mut self, command: InternalCommand) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_heating.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_heating(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_signal_handler_from_heating.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_signal_handler_from_heating += 1;
        }
        result
    }

    #[allow(unused)]
    pub fn send_to_relay_manager(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_relay_manager.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_relay_manager(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_signal_handler_from_relay_manager.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_signal_handler_from_relay_manager += 1;
        }
        result
    }

    #[allow(unused)]
    pub fn send_to_atlas_scientific(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_atlas_scientific.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_atlas_scientific(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_signal_handler_from_atlas_scientific.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_signal_handler_from_atlas_scientific += 1;
        }
        result
    }

    #[allow(unused)]
    pub fn send_to_sensor_manager(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_sensor_manager.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_sensor_manager(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_signal_handler_from_sensor_manager.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_signal_handler_from_sensor_manager += 1;
        }
        result
    }

    #[allow(unused)]
    pub fn send_to_data_logger(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_data_logger.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_data_logger(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_signal_handler_from_data_logger.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_signal_handler_from_data_logger += 1;
        }
        result
    }

    #[allow(unused)]
    pub fn send_to_balling(&mut self, command: InternalCommand) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_balling.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_balling(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_signal_handler_from_balling.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_signal_handler_from_balling += 1;
        }
        result
    }

    #[allow(unused)]
    pub fn send_to_ventilation(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_ventilation.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_ventilation(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_signal_handler_from_ventilation.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_signal_handler_from_ventilation += 1;
        }
        result
    }

    #[allow(unused)]
    pub fn send_to_monitors(&mut self, command: InternalCommand) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_monitors.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_monitors(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_signal_handler_from_monitors.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_signal_handler_from_monitors += 1;
        }
        result
    }

    #[allow(unused)]
    pub fn send_to_feed(&mut self, command: InternalCommand) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_feed.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_feed(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_signal_handler_from_feed.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_signal_handler_from_feed += 1;
        }
        result
    }

    #[allow(unused)]
    pub fn send_to_schedule_check(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_schedule_check.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_schedule_check(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_signal_handler_from_schedule_check.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_signal_handler_from_schedule_check += 1;
        }
        result
    }

    #[allow(unused)]
    pub fn send_to_watchdog(&mut self, command: InternalCommand) -> Result<(), AquaChannelError> {
        self.tx_signal_handler_to_watchdog.send(command)
    }

    #[allow(unused)]
    pub fn receive_from_watchdog(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_signal_handler_from_watchdog.recv();
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_rx_signal_handler_from_watchdog += 1;
        }
        result
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
            "cnt_rx_signal_handler_from_messaging_opt: {}",
            self.cnt_rx_signal_handler_from_messaging_opt
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_refill: {}",
            self.tx_signal_handler_to_refill.count
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_refill: {}",
            self.cnt_rx_signal_handler_from_refill
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_tank_level_switch: {}",
            self.tx_signal_handler_to_tank_level_switch.count
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_tank_level_switch: {}",
            self.cnt_rx_signal_handler_from_tank_level_switch
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_heating: {}",
            self.tx_signal_handler_to_heating.count
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_heating: {}",
            self.cnt_rx_signal_handler_from_heating
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_relay_manager: {}",
            self.tx_signal_handler_to_relay_manager.count
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_relay_manager: {}",
            self.cnt_rx_signal_handler_from_relay_manager
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_atlas_scientific: {}",
            self.tx_signal_handler_to_atlas_scientific.count
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_atlas_scientific: {}",
            self.cnt_rx_signal_handler_from_atlas_scientific
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_sensor_manager: {}",
            self.tx_signal_handler_to_sensor_manager.count
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_sensor_manager: {}",
            self.cnt_rx_signal_handler_from_sensor_manager
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_data_logger: {}",
            self.tx_signal_handler_to_data_logger.count
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_data_logger: {}",
            self.cnt_rx_signal_handler_from_data_logger
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_balling: {}",
            self.tx_signal_handler_to_balling.count
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_balling: {}",
            self.cnt_rx_signal_handler_from_balling
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_ventilation: {}",
            self.tx_signal_handler_to_ventilation.count
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_ventilation: {}",
            self.cnt_rx_signal_handler_from_ventilation
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_monitors: {}",
            self.tx_signal_handler_to_monitors.count
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_monitors: {}",
            self.cnt_rx_signal_handler_from_monitors
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_feed: {}",
            self.tx_signal_handler_to_feed.count
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_feed: {}",
            self.cnt_rx_signal_handler_from_feed
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_schedule_check: {}",
            self.tx_signal_handler_to_schedule_check.count
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_schedule_check: {}",
            self.cnt_rx_signal_handler_from_schedule_check
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_tcp_opt: {}",
            self.tx_signal_handler_to_tcp_opt.as_ref().unwrap().count
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_tcp_opt: {}",
            self.cnt_rx_signal_handler_from_tcp_opt
        )?;
        writeln!(
            f,
            "tx_signal_handler_to_watchdog: {}",
            self.tx_signal_handler_to_watchdog.count
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_watchdog: {}",
            self.cnt_rx_signal_handler_from_watchdog
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
