/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::utilities::channel_content::InternalCommand;
use std::fmt;
use std::sync::mpsc::{Receiver, RecvError, SendError, Sender};

pub struct SignalHandlerChannels {
    // --- Channels for each module ---
    pub tx_signal_handler_to_messaging_opt: Option<Sender<InternalCommand>>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_messaging_opt: u64,
    pub rx_signal_handler_from_messaging_opt: Option<Receiver<bool>>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_messaging_opt: u64,

    pub tx_signal_handler_to_refill: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_refill: u64,
    pub rx_signal_handler_from_refill: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_refill: u64,

    pub tx_signal_handler_to_tank_level_switch: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_tank_level_switch: u64,
    pub rx_signal_handler_from_tank_level_switch: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_tank_level_switch: u64,

    pub tx_signal_handler_to_heating: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_heating: u64,
    pub rx_signal_handler_from_heating: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_heating: u64,

    pub tx_signal_handler_to_relay_manager: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_relay_manager: u64,
    pub rx_signal_handler_from_relay_manager: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_relay_manager: u64,

    pub tx_signal_handler_to_atlas_scientific: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_atlas_scientific: u64,
    pub rx_signal_handler_from_atlas_scientific: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_atlas_scientific: u64,

    pub tx_signal_handler_to_sensor_manager: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_sensor_manager: u64,
    pub rx_signal_handler_from_sensor_manager: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_sensor_manager: u64,

    pub tx_signal_handler_to_data_logger: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_data_logger: u64,
    pub rx_signal_handler_from_data_logger: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_data_logger: u64,

    pub tx_signal_handler_to_balling: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_balling: u64,
    pub rx_signal_handler_from_balling: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_balling: u64,

    pub tx_signal_handler_to_ventilation: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_ventilation: u64,
    pub rx_signal_handler_from_ventilation: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_ventilation: u64,

    pub tx_signal_handler_to_monitors: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_monitors: u64,
    pub rx_signal_handler_from_monitors: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_monitors: u64,

    pub tx_signal_handler_to_feed: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_feed: u64,
    pub rx_signal_handler_from_feed: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_feed: u64,

    pub tx_signal_handler_to_schedule_check: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_schedule_check: u64,
    pub rx_signal_handler_from_schedule_check: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_schedule_check: u64,

    pub tx_signal_handler_to_tcp_opt: Option<Sender<InternalCommand>>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_tcp_opt: u64,
    pub rx_signal_handler_from_tcp_opt: Option<Receiver<bool>>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_tcp_opt: u64,

    pub tx_signal_handler_to_watchdog: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_watchdog: u64,
    pub rx_signal_handler_from_watchdog: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_signal_handler_from_watchdog: u64,

    // Hardware-specific channels
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub tx_signal_handler_to_dht: Sender<InternalCommand>,
    #[cfg(all(target_os = "linux", feature = "target_hw", feature = "debug_channels"))]
    pub cnt_tx_signal_handler_to_dht: u64,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub tx_signal_handler_to_i2c_interface: Sender<InternalCommand>,
    #[cfg(all(target_os = "linux", feature = "target_hw", feature = "debug_channels"))]
    pub cnt_tx_signal_handler_to_i2c_interface: u64,

    pub tx_signal_handler_to_ds18b20: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_signal_handler_to_ds18b20: u64,
}

impl SignalHandlerChannels {
    // --- Optional Modules ---
    #[allow(unused)]
    pub fn send_to_messaging(
        &mut self,
        command: InternalCommand,
    ) -> Option<Result<(), SendError<InternalCommand>>> {
        self.tx_signal_handler_to_messaging_opt.as_ref().map(|tx| {
            let result = tx.send(command);
            #[cfg(feature = "debug_channels")]
            if result.is_ok() {
                self.cnt_tx_signal_handler_to_messaging_opt += 1;
            }
            result
        })
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
    ) -> Option<Result<(), SendError<InternalCommand>>> {
        self.tx_signal_handler_to_tcp_opt.as_ref().map(|tx| {
            let result = tx.send(command);
            #[cfg(feature = "debug_channels")]
            if result.is_ok() {
                self.cnt_tx_signal_handler_to_tcp_opt += 1;
            }
            result
        })
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

    // --- Standard Modules ---
    #[allow(unused)]
    pub fn send_to_refill(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_refill.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_refill += 1;
        }
        result
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
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_tank_level_switch.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_tank_level_switch += 1;
        }
        result
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
    pub fn send_to_heating(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_heating.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_heating += 1;
        }
        result
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
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_relay_manager.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_relay_manager += 1;
        }
        result
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
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_atlas_scientific.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_atlas_scientific += 1;
        }
        result
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
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_sensor_manager.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_sensor_manager += 1;
        }
        result
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
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_data_logger.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_data_logger += 1;
        }
        result
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
    pub fn send_to_balling(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_balling.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_balling += 1;
        }
        result
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
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_ventilation.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_ventilation += 1;
        }
        result
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
    pub fn send_to_monitors(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_monitors.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_monitors += 1;
        }
        result
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
    pub fn send_to_feed(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_feed.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_feed += 1;
        }
        result
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
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_schedule_check.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_schedule_check += 1;
        }
        result
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
    pub fn send_to_watchdog(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_watchdog.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_watchdog += 1;
        }
        result
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
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub fn send_to_dht(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_dht.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_dht += 1;
        }
        result
    }

    #[allow(unused)]
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    pub fn send_to_i2c_interface(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_i2c_interface.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_i2c_interface += 1;
        }
        result
    }

    #[allow(unused)]
    pub fn send_to_ds18b20(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), SendError<InternalCommand>> {
        let result = self.tx_signal_handler_to_ds18b20.send(command);
        #[cfg(feature = "debug_channels")]
        if result.is_ok() {
            self.cnt_tx_signal_handler_to_ds18b20 += 1;
        }
        result
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for SignalHandlerChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "--- Signal Handler Channel Status ---")?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_messaging_opt: {}",
            self.cnt_tx_signal_handler_to_messaging_opt
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_messaging_opt: {}",
            self.cnt_rx_signal_handler_from_messaging_opt
        )?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_refill: {}",
            self.cnt_tx_signal_handler_to_refill
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_refill: {}",
            self.cnt_rx_signal_handler_from_refill
        )?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_tank_level_switch: {}",
            self.cnt_tx_signal_handler_to_tank_level_switch
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_tank_level_switch: {}",
            self.cnt_rx_signal_handler_from_tank_level_switch
        )?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_heating: {}",
            self.cnt_tx_signal_handler_to_heating
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_heating: {}",
            self.cnt_rx_signal_handler_from_heating
        )?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_relay_manager: {}",
            self.cnt_tx_signal_handler_to_relay_manager
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_relay_manager: {}",
            self.cnt_rx_signal_handler_from_relay_manager
        )?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_atlas_scientific: {}",
            self.cnt_tx_signal_handler_to_atlas_scientific
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_atlas_scientific: {}",
            self.cnt_rx_signal_handler_from_atlas_scientific
        )?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_sensor_manager: {}",
            self.cnt_tx_signal_handler_to_sensor_manager
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_sensor_manager: {}",
            self.cnt_rx_signal_handler_from_sensor_manager
        )?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_data_logger: {}",
            self.cnt_tx_signal_handler_to_data_logger
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_data_logger: {}",
            self.cnt_rx_signal_handler_from_data_logger
        )?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_balling: {}",
            self.cnt_tx_signal_handler_to_balling
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_balling: {}",
            self.cnt_rx_signal_handler_from_balling
        )?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_ventilation: {}",
            self.cnt_tx_signal_handler_to_ventilation
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_ventilation: {}",
            self.cnt_rx_signal_handler_from_ventilation
        )?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_monitors: {}",
            self.cnt_tx_signal_handler_to_monitors
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_monitors: {}",
            self.cnt_rx_signal_handler_from_monitors
        )?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_feed: {}",
            self.cnt_tx_signal_handler_to_feed
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_feed: {}",
            self.cnt_rx_signal_handler_from_feed
        )?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_schedule_check: {}",
            self.cnt_tx_signal_handler_to_schedule_check
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_schedule_check: {}",
            self.cnt_rx_signal_handler_from_schedule_check
        )?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_tcp_opt: {}",
            self.cnt_tx_signal_handler_to_tcp_opt
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_tcp_opt: {}",
            self.cnt_rx_signal_handler_from_tcp_opt
        )?;
        writeln!(
            f,
            "cnt_tx_signal_handler_to_watchdog: {}",
            self.cnt_tx_signal_handler_to_watchdog
        )?;
        writeln!(
            f,
            "cnt_rx_signal_handler_from_watchdog: {}",
            self.cnt_rx_signal_handler_from_watchdog
        )?;

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        {
            writeln!(
                f,
                "cnt_tx_signal_handler_to_dht: {}",
                self.cnt_tx_signal_handler_to_dht
            )?;
            writeln!(
                f,
                "cnt_tx_signal_handler_to_i2c_interface: {}",
                self.cnt_tx_signal_handler_to_i2c_interface
            )?;
            writeln!(
                f,
                "cnt_tx_signal_handler_to_ds18b20: {}",
                self.cnt_tx_signal_handler_to_ds18b20
            )?;
        }

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
