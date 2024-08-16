/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use crate::channel_content::InternalCommand;
use crate::proc_ext_req::ProcessExternalRequestTrait;

#[cfg(target_os = "linux")]
use crate::channel_content::AquariumSignal;

#[cfg(target_os = "linux")]
use crate::i2c_interface::I2cInterface;

/// Holds the configuration data for the GPIO handler.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct GpioHandlerConfig {
    /// Flag to deactivate functionality for testing purposes
    pub active: bool,

    /// Flag to determine if application shall communicate with hardware directly or via TCP with a simulator
    pub use_simulator: bool,

    /// replacement value for water temperature in case reading from I2C fails
    pub replacement_value_water_temperature: f32,

    /// replacement value for pH in case reading from I2C fails
    pub replacement_value_ph: f32,

    /// replacement value for conductivity in case reading from I2C fails
    pub replacement_value_conductivity: f32,
}

/// Holds the configuration data for the I2C interface.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
/// Struct is intentionally placed here instead of create i2c_interface.
#[derive(Deserialize, Debug)]
pub struct I2cInterfaceConfig {
    /// I2C address of Atlas Scientific pH sensor unit
    pub address_atlas_scientific_ph: u16,

    /// I2C address of Atlas Scientific conductivity sensor unit
    pub address_atlas_scientific_conductivity: u16,

    /// I2C address of Atlas Scientific temperature sensor unit    
    pub address_atlas_scientific_temperature: u16,

    /// maximum number of read attempts
    pub max_read_attempts: u16,

    /// timeout for transactions in milliseconds
    pub timeout_transaction: u32,

    /// sleep time in milliseconds before after writing before reading from pH sensor unit
    pub sleep_time_atlas_scientific_ph: u64,

    /// sleep time in milliseconds before after writing before reading from conductivity sensor unit
    pub sleep_time_atlas_scientific_conductivity: u64,

    /// sleep time in milliseconds before after writing before reading from temperature sensor unit
    pub sleep_time_atlas_scientific_temperature: u64,

    /// maximum allowed value for pH used to assess if the temperature sensor reading is valid
    pub max_val_atlas_scientific_ph: f32,

    /// minimum allowed value for pH used to assess if the temperature sensor reading is valid
    pub min_val_atlas_scientific_ph: f32,

    /// maximum allowed value for temperature used to assess if the temperature sensor reading is valid
    pub max_val_atlas_scientific_temperature: f32,

    /// minimum allowed value for temperature used to assess if the temperature sensor reading is valid
    pub min_val_atlas_scientific_temperature: f32,

    /// maximum allowed value for conducitivity used to assess if the conductivity sensor reading is valid
    pub max_val_atlas_scientific_conductivity: f32,

    /// minimum allowed value for conducitivity used to assess if the conductivity sensor reading is valid
    pub min_val_atlas_scientific_conductivity: f32,
}

/// Contains the configuration and the implementation for the GPIO handler.
pub struct GpioHandler {
    /// configuration data for GPIO handler
    config: GpioHandlerConfig,

    /// I2C interface for communication with Atlas Scientific sensor units
    #[cfg(target_os = "linux")]
    i2c_interface: I2cInterface,
}

impl GpioHandler {
    #[cfg(target_os = "linux")]
    /// This function returns a struct of type GpioHandler.
    /// If real hardware is used, then the function also opens interface to GPIO and I2C.
    /// # Arguments
    /// * `config` - configuration data for GPIO handler
    /// * `i2c_config` - configuration data for I2C handler
    pub fn new(config: GpioHandlerConfig, i2c_config: I2cInterfaceConfig) -> GpioHandler {
        GpioHandler {
            config: config,
            #[cfg(target_os = "linux")]
            i2c_interface: I2cInterface::new(i2c_config),
        }
    }

    #[cfg(not(target_os = "linux"))]
    /// This function returns a struct of type GpioHandler for development purposes.
    /// # Arguments
    /// * `config` - configuration data for GPIO handler
    pub fn new(config: GpioHandlerConfig) -> GpioHandler {
        if !config.use_simulator {
            panic!("Usage of hardware not supported on this platform.");
        }

        GpioHandler { config: config }
    }

    /// Requests a response from the simulator via TCP communication by sending
    /// an internal message via channel to the TCP thread.
    /// This function is used for development purposes only.
    fn get_response_from_simulator(
        tx_to_tcp: &Sender<InternalCommand>,
        rx_from_tcp: &Receiver<f32>,
        internal_command: InternalCommand,
    ) -> f32 {
        match internal_command {
            InternalCommand::RequestSignal(_) => {
                //println!("GpioHandler::get_response_from_simulator: requesting {}", c);
                match tx_to_tcp.send(internal_command) {
                    Ok(()) => match rx_from_tcp.recv() {
                        Ok(c) => c,
                        Err(e) => {
                            panic!("GpioHandler: Error occurred when trying to receive response from TCP thread: {e:?}");
                        }
                    },
                    Err(e) => {
                        panic!("GpioHandler: Error in trying to send request to TCP thread: {e:?}");
                    }
                }
            }
            _ => {
                println!("GpioHandler: ignoring irrelevant internal commands.");
                0.0
            }
        }
    }

    /// Requests a response from the real hardware
    #[cfg(target_os = "linux")]
    fn get_response_from_hardware(&mut self, internal_command: InternalCommand) -> f32 {
        match internal_command {
            InternalCommand::RequestSignal(signal) => match signal {
                AquariumSignal::WaterTemperature => {
                    match self
                        .i2c_interface
                        .get_response_from_i2c(AquariumSignal::WaterTemperature)
                    {
                        Ok(c) => c,
                        Err(_) => self.config.replacement_value_water_temperature,
                    }
                }
                AquariumSignal::pH => {
                    match self.i2c_interface.get_response_from_i2c(AquariumSignal::pH) {
                        Ok(c) => c,
                        Err(_) => self.config.replacement_value_ph,
                    }
                }
                AquariumSignal::Conductivity => {
                    match self
                        .i2c_interface
                        .get_response_from_i2c(AquariumSignal::Conductivity)
                    {
                        Ok(c) => c,
                        Err(_) => self.config.replacement_value_conductivity,
                    }
                }
                AquariumSignal::AmbientTemperature => 0.0,
                AquariumSignal::AmbientHumidity => 0.0,
                AquariumSignal::TankLevelSwitchPosition => 0.0,
                AquariumSignal::TankLevelSwitchInvalid => 0.0,
            },
            _ => {
                println!("GpioHandler: ignoring irrelevant internal commands.");
                0.0
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn get_response_from_hardware(&self, _internal_command: InternalCommand) -> f32 {
        panic!("Usage of hardware not supported on this platform.");
    }

    /// Executes the main control in a loop until signal handler commands quitting and termination.
    /// This function receives request from other threads and either actuates on real HW
    /// or communicates with simulator.
    /// # Arguments
    /// * `tx_to_tcp_opt` - sender part of channel for communication with signal handler
    /// * `rx_from_tcp_opt` -  receiver part of channel for communication with signal handler
    /// * `tx_to_data_logger` - sender part of channel for communication to data logger
    /// * `rx_to_data_logger` - receiver part of channel for communication from data logger
    /// * `tx_from_refill` - sender part of channel for communication to refill thread
    /// * `rx_from_refill` - receiver part of channel for communication from refill thread
    /// * `tx_to_heating` - sender part of channel for communication to heating thread
    /// * `rx_from_heating` - receiver part of channel for communication from heating thread    
    /// * `tx_to_ventilation` - sender part of channel for communication to ventilation thread
    /// * `rx_from_ventilation` - receiver part of channel for communication from ventilation thread
    /// * `tx_to_monitors` - sender part of channel for communication to monitors thread
    /// * `rx_from_monitors` - receiver part of channel for communication from monitors thread
    /// * `tx_to_signal_handler` - sender part of channel for communication to signal handler
    /// * `rx_from_signal_handler` - receiver part of channel for communication from signal handler    
    pub fn execute(
        &mut self,
        tx_to_tcp_opt: Option<Sender<InternalCommand>>,
        rx_from_tcp_opt: Option<Receiver<f32>>,
        tx_to_data_logger: Sender<f32>,
        rx_from_data_logger: Receiver<InternalCommand>,
        tx_to_refill: Sender<f32>,
        rx_from_refill: Receiver<InternalCommand>,
        tx_to_heating: Sender<f32>,
        rx_from_heating: Receiver<InternalCommand>,
        tx_to_ventilation: Sender<f32>,
        rx_from_ventilation: Receiver<InternalCommand>,
        tx_to_monitors: Sender<f32>,
        rx_from_monitors: Receiver<InternalCommand>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        let mut response_value: f32 = 0.0;

        if (self.config.use_simulator) && (tx_to_tcp_opt.is_none() | rx_from_tcp_opt.is_none()) {
            panic!("Gpiohandler configured to run with simulator, but no channel to TCP thread provided.");
        }

        loop {
            let (quit_command_received, _, _) =
                self.process_external_request(&rx_from_signal_handler, &rx_from_signal_handler);
            if quit_command_received {
                break;
            }

            // *** communication with data logger ***
            match rx_from_data_logger.try_recv() {
                Ok(c) => {
                    if self.config.active {
                        if self.config.use_simulator {
                            response_value = Self::get_response_from_simulator(
                                tx_to_tcp_opt.as_ref().unwrap(),
                                rx_from_tcp_opt.as_ref().unwrap(),
                                c,
                            );
                        } else {
                            response_value = self.get_response_from_hardware(c);
                        }
                    } else {
                        response_value = 0.0;
                    }
                    match tx_to_data_logger.send(response_value) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Error in trying to send response value back to data logger: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("GpioHandler: Error in trying to receive command from data logger: {e:?}");
                        }
                    }
                }
            }
            // **********************************

            // *** communication with refill control ***
            match rx_from_refill.try_recv() {
                Ok(c) => {
                    if self.config.active {
                        if self.config.use_simulator {
                            response_value = Self::get_response_from_simulator(
                                tx_to_tcp_opt.as_ref().unwrap(),
                                rx_from_tcp_opt.as_ref().unwrap(),
                                c,
                            );
                        }
                        // @todo: implement communication with real HW
                    }
                    println!("GpioHandler: Sending back to refill.");
                    match tx_to_refill.send(response_value) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Error in trying to send response value back to data logger: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("GpioHandler: Error in trying to receive command from data refill: {e:?}");
                        }
                    }
                }
            }
            // **********************************

            // *** communication with heating control ***
            match rx_from_heating.try_recv() {
                Ok(c) => {
                    if self.config.active {
                        if self.config.use_simulator {
                            response_value = Self::get_response_from_simulator(
                                tx_to_tcp_opt.as_ref().unwrap(),
                                rx_from_tcp_opt.as_ref().unwrap(),
                                c,
                            );
                        }
                        // @todo: implement communication with real HW
                    }
                    println!("GpioHandler: Sending back to Heating");
                    match tx_to_heating.send(response_value) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Error in trying to send response value back to data logger: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("GpioHandler: Error in trying to receive command from heating: {e:?}");
                        }
                    }
                }
            }
            // **********************************

            // *** communication with ventilation control ***
            match rx_from_ventilation.try_recv() {
                Ok(c) => {
                    if self.config.active {
                        if self.config.use_simulator {
                            response_value = Self::get_response_from_simulator(
                                tx_to_tcp_opt.as_ref().unwrap(),
                                rx_from_tcp_opt.as_ref().unwrap(),
                                c,
                            );
                        }
                        // @todo: implement communication with real HW
                    }
                    println!("GpioHandler: Sending back to Ventilation");
                    match tx_to_ventilation.send(response_value) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Error in trying to send response value back to data logger: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("GpioHandler: Error in trying to receive command from ventilation: {e:?}");
                        }
                    }
                }
            }
            // **********************************

            // *** communication with monitors ***
            match rx_from_monitors.try_recv() {
                Ok(c) => {
                    if self.config.active {
                        if self.config.use_simulator {
                            response_value = Self::get_response_from_simulator(
                                tx_to_tcp_opt.as_ref().unwrap(),
                                rx_from_tcp_opt.as_ref().unwrap(),
                                c,
                            );
                        }
                        // @todo: implement communication with real HW
                    }
                    println!("GpioHandler: Sending back to Monitors");
                    match tx_to_monitors.send(response_value) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Error in trying to send response value back to data logger: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("GpioHandler: Error in trying to receive command from monitors: {e:?}");
                        }
                    }
                }
            }
            // **********************************
        }

        println!("GpioHandler: Exited loop - sending back to signal handler");

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                println!("GpioHandler: Error in trying to send confirmation back to signal handler: {e:?}");
            }
        };

        println!("GpioHandler finished.");
    }
}
