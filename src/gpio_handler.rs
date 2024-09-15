/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::channel_content::InternalCommand;
use crate::proc_ext_req::ProcessExternalRequestTrait;
use log::{error, info, warn};
use serde_derive::Deserialize;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

#[cfg(target_os = "linux")]
use crate::channel_content::AquariumSignal;

#[cfg(target_os = "linux")]
use crate::i2c_interface::I2cInterface;

#[cfg(target_os = "linux")]
use rppal::gpio::{Gpio, InputPin, Mode};

#[cfg(target_os = "linux")]
use crate::dht::read_dht_data;

use crate::dht::DhtConfig;

const LOG_TARGET_GPIO_HANDLER: &'static str = "gpio_handler";

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

    /// GPIO pin number for tank level sensor
    pub gpio_pin_number_tank_level: u8,

    /// GPIO pin number for communication with DHT22 sensor
    pub gpio_pin_number_dht22_io: u8,

    /// GPIO pin number for providing power to DHT22 sensor
    pub gpio_pin_number_dht22_vcc: u8,
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

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the GPIO handler.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     gpio_handler[GPIO handler] --> tcp_communication[TCP communication]
///     tcp_communication --> gpio_handler
///     gpio_handler --> data_logger[Data logger]
///     data_logger --> gpio_handler
///     gpio_handler --> refill[Refill control]
///     refill --> gpio_handler
///     gpio_handler --> heating[Heating control]
///     heating --> gpio_handler
///     gpio_handler --> ventilation[Ventilation control]
///     ventilation --> gpio_handler
///     gpio_handler --> monitors[Monitors]
///     monitors --> gpio_handler
///     gpio_handler --> signal_handler[Signal handler]
///     signal_handler --> gpio_handler
/// ```

pub struct GpioHandler {
    /// configuration data for GPIO handler
    config: GpioHandlerConfig,

    /// I2C interface for communication with Atlas Scientific sensor units
    #[cfg(target_os = "linux")]
    i2c_interface: Option<I2cInterface>,

    /// input pin for communication with tank level switch
    #[cfg(target_os = "linux")]
    input_pin_tank_level_switch: InputPin,
}

impl GpioHandler {
    /// checks if the GPIO pin configuration valid:
    /// - pin numbers shall be within permissible range
    /// - pin numbers shall be distinct
    /// # Arguments
    /// * `config` - configuration of GPIO handler
    pub fn check_valid_gpio_pin_config(config: &GpioHandlerConfig) -> bool {
        // check if pin ids are in valid range
        if config.gpio_pin_number_tank_level > 3
            && config.gpio_pin_number_tank_level < 28
            && config.gpio_pin_number_dht22_vcc > 3
            && config.gpio_pin_number_dht22_vcc < 28
            && config.gpio_pin_number_dht22_io > 3
            && config.gpio_pin_number_dht22_io < 28
        {
            // check if pin ids are distinct
            if config.gpio_pin_number_tank_level != config.gpio_pin_number_dht22_vcc
                && config.gpio_pin_number_dht22_vcc != config.gpio_pin_number_dht22_io
                && config.gpio_pin_number_dht22_io != config.gpio_pin_number_tank_level
            {
                true // valid GPIO pin number
            } else {
                error!(
                    target: LOG_TARGET_GPIO_HANDLER,
                    "GPIO pins are not distinct."
                );
                false // invalid GPIO pin numbers because not distinct
            }
        } else {
            error!(
                target: LOG_TARGET_GPIO_HANDLER,
                "GPIO pins are not inside permissible range."
            );
            false // invalid GPIO pin number because out of range
        }
    }

    #[cfg(target_os = "linux")]
    /// This function returns a struct of type GpioHandler.
    /// If real hardware is used, then the function also opens interface to GPIO and I2C.
    /// # Arguments
    /// * `config` - configuration data for GPIO handler
    /// * `dht_config` - configuration data for GPIO handler
    /// * `i2c_config` - configuration data for I2C handler
    pub fn new(
        config: GpioHandlerConfig,
        dht_config: DhtConfig,
        i2c_config: I2cInterfaceConfig,
    ) -> GpioHandler {
        let use_simulator = config.use_simulator; // create copy because struct is moved

        // check if GPIO pin configuration is valid
        if !Self::check_valid_gpio_pin_config(&config) {
            panic!(
                "{}: GPIO pin configuration is invalid",
                LOG_TARGET_GPIO_HANDLER,
            );
        }

        // open interface to GPIO library
        #[cfg(target_os = "linux")]
        let gpio = match Gpio::new() {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "{}: could not open interface to GPIO ({e:?})",
                    LOG_TARGET_GPIO_HANDLER,
                );
            }
        };

        // get pin for tank level monitoring
        #[cfg(target_os = "linux")]
        let tank_level_pin = match gpio.get(config.gpio_pin_number_tank_level) {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "{}: could not get pin {} from GPIO interface for tank level monitoring ({e:?})",
                    LOG_TARGET_GPIO_HANDLER,
                    config.gpio_pin_number_tank_level
                );
            }
        };

        // get pin for providing supply voltage to DHT sensor
        #[cfg(target_os = "linux")]
        let mut dht_vcc_pin = match gpio.get(config.gpio_pin_number_dht22_vcc) {
            Ok(c) => c.into_output(),
            Err(e) => {
                panic!(
                    "{}: could not get pin {} from GPIO interface for DHT power supply ({e:?})",
                    LOG_TARGET_GPIO_HANDLER, config.gpio_pin_number_dht22_vcc
                );
            }
        };

        // get pin for communication with DHT sensor
        #[cfg(target_os = "linux")]
        let mut dht_io_pin = match gpio.get(config.gpio_pin_number_dht22_io) {
            Ok(c) => c.into_io(Mode::Output),
            Err(e) => {
                panic!(
                    "{}: failed to aquire pin for communication with DHT sensor ({e:?})",
                    LOG_TARGET_GPIO_HANDLER
                );
            }
        };

        // read one sample from DHT sensor to verify SW and HW are working ok
        #[cfg(target_os = "linux")]
        match read_dht_data(&dht_config, &mut dht_vcc_pin, &mut dht_io_pin) {
            Ok((temperature, humidity)) => {
                info!(
                    target: LOG_TARGET_GPIO_HANDLER,
                    "Initial read from DHT sensor: temperature={:.1} deg C and humidity = {:.1}%",
                    temperature,
                    humidity,
                );
            }
            Err(e) => {
                panic!(
                    "{}: failed to read from DHT sensor ({e:?})",
                    LOG_TARGET_GPIO_HANDLER
                );
            }
        }

        GpioHandler {
            config: config,
            #[cfg(target_os = "linux")]
            i2c_interface: match use_simulator {
                true => None,
                false => Some(I2cInterface::new(i2c_config)),
            },
            #[cfg(target_os = "linux")]
            input_pin_tank_level_switch: tank_level_pin.into_input_pullup(),
        }
    }

    #[cfg(not(target_os = "linux"))]
    /// This function returns a struct of type GpioHandler for development purposes.
    /// # Arguments
    /// * `config` - configuration data for GPIO handler
    pub fn new(config: GpioHandlerConfig) -> GpioHandler {
        if !config.use_simulator {
            panic!(
                "{}: Usage of hardware not supported on this platform.",
                LOG_TARGET_GPIO_HANDLER
            );
        }

        if !Self::check_valid_gpio_pin_config(&config) {
            panic!(
                "{}: GPIO pin configuration is invalid",
                LOG_TARGET_GPIO_HANDLER,
            );
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
            InternalCommand::RequestSignal(_) => match tx_to_tcp.send(internal_command) {
                Ok(()) => match rx_from_tcp.recv() {
                    Ok(c) => c,
                    Err(e) => {
                        panic!(
                                "{}: Error occurred when trying to receive response from TCP thread ({e:?})",
                                LOG_TARGET_GPIO_HANDLER);
                    }
                },
                Err(e) => {
                    panic!(
                        "{}: Error occurred when trying to send request to TCP thread ({e:?})",
                        LOG_TARGET_GPIO_HANDLER
                    );
                }
            },
            _ => {
                warn!(
                    target: LOG_TARGET_GPIO_HANDLER,
                    "ignoring not applicable internal command ({})",
                    internal_command
                );
                0.0
            }
        }
    }

    /// Requests a response from the real hardware
    #[cfg(target_os = "linux")]
    fn get_response_from_hardware(&mut self, internal_command: InternalCommand) -> f32 {
        let i2c: &mut I2cInterface = self.i2c_interface.as_mut().unwrap();
        match internal_command {
            InternalCommand::RequestSignal(signal) => match signal {
                AquariumSignal::WaterTemperature => {
                    match i2c.get_response_from_i2c(AquariumSignal::WaterTemperature) {
                        Ok(c) => c,
                        Err(_) => self.config.replacement_value_water_temperature,
                    }
                }
                AquariumSignal::pH => match i2c.get_response_from_i2c(AquariumSignal::pH) {
                    Ok(c) => c,
                    Err(_) => self.config.replacement_value_ph,
                },
                AquariumSignal::Conductivity => {
                    match i2c.get_response_from_i2c(AquariumSignal::Conductivity) {
                        Ok(c) => c,
                        Err(_) => self.config.replacement_value_conductivity,
                    }
                }
                AquariumSignal::AmbientTemperature => 0.0,
                AquariumSignal::AmbientHumidity => 0.0,
                AquariumSignal::TankLevelSwitchPosition => {
                    match self.input_pin_tank_level_switch.is_high() {
                        true => 1.0,
                        false => 0.0,
                    }
                }
                AquariumSignal::TankLevelSwitchInvalid => 0.0, // not supported by library
            },
            _ => {
                warn!(
                    target: LOG_TARGET_GPIO_HANDLER,
                    "ignoring not applicable internal command ({})",
                    internal_command
                );
                0.0
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn get_response_from_hardware(&self, _internal_command: InternalCommand) -> f32 {
        panic!(
            "{}: Usage of hardware not supported on this platform.",
            LOG_TARGET_GPIO_HANDLER
        );
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
        let mut data_logger_channel_disconnected = false;
        let mut refill_channel_disconnected = false;
        let mut heating_channel_disconnected = false;
        let mut ventilation_channel_disconnected = false;
        let mut monitors_channel_disconnected = false;

        if (self.config.use_simulator) && (tx_to_tcp_opt.is_none() | rx_from_tcp_opt.is_none()) {
            panic!(
                "{}: configured to run with simulator, but no channel to TCP thread provided.",
                LOG_TARGET_GPIO_HANDLER
            );
        }

        loop {
            let (quit_command_received, _, _) =
                self.process_external_request(&rx_from_signal_handler, None);
            if quit_command_received {
                break;
            }

            // *** communication with data logger ***
            if !data_logger_channel_disconnected {
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
                                error!(
                                    target: LOG_TARGET_GPIO_HANDLER,
                                    "sending response value back to data logger failed ({e:?})"
                                );
                                data_logger_channel_disconnected = true;
                            }
                        }
                    }
                    Err(e) => {
                        match e {
                            TryRecvError::Empty => { /* empty buffer - no action required */ }
                            TryRecvError::Disconnected => {
                                error!(
                                    target: LOG_TARGET_GPIO_HANDLER,
                                    "receiving command from data logger failed ({e:?})",
                                );
                                data_logger_channel_disconnected = true;
                            }
                        }
                    }
                }
            }
            // **********************************

            // *** communication with refill control ***
            if !refill_channel_disconnected {
                match rx_from_refill.try_recv() {
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
                        }
                        match tx_to_refill.send(response_value) {
                            Ok(()) => {}
                            Err(e) => {
                                error!(
                                    target: LOG_TARGET_GPIO_HANDLER,
                                    "sending response value back to refill control failed ({e:?})"
                                );
                                refill_channel_disconnected = true;
                            }
                        }
                    }
                    Err(e) => {
                        match e {
                            TryRecvError::Empty => { /* empty buffer - no action required */ }
                            TryRecvError::Disconnected => {
                                error!(
                                    target: LOG_TARGET_GPIO_HANDLER,
                                    "receiving command from refill control failed ({e:?})"
                                );
                                refill_channel_disconnected = true;
                            }
                        }
                    }
                }
            }
            // **********************************

            // *** communication with heating control ***
            if !heating_channel_disconnected {
                match rx_from_heating.try_recv() {
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
                        }
                        match tx_to_heating.send(response_value) {
                            Ok(()) => {}
                            Err(e) => {
                                error!(
                                    target: LOG_TARGET_GPIO_HANDLER,
                                    "sending response value back to heating failed ({e:?})"
                                );
                                heating_channel_disconnected = true;
                            }
                        }
                    }
                    Err(e) => {
                        match e {
                            TryRecvError::Empty => { /* empty buffer - no action required */ }
                            TryRecvError::Disconnected => {
                                error!(
                                    target: LOG_TARGET_GPIO_HANDLER,
                                    "receiveing command from heating failed ({e:?})"
                                );
                                heating_channel_disconnected = true;
                            }
                        }
                    }
                }
            }
            // **********************************

            // *** communication with ventilation control ***
            if !ventilation_channel_disconnected {
                match rx_from_ventilation.try_recv() {
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
                        }
                        match tx_to_ventilation.send(response_value) {
                            Ok(()) => {}
                            Err(e) => {
                                error!(
                                    target: LOG_TARGET_GPIO_HANDLER,
                                    "sending response value back to ventilation failed ({e:?})"
                                );
                                ventilation_channel_disconnected = true;
                            }
                        }
                    }
                    Err(e) => {
                        match e {
                            TryRecvError::Empty => { /* empty buffer - no action required */ }
                            TryRecvError::Disconnected => {
                                error!(
                                    target: LOG_TARGET_GPIO_HANDLER,
                                    "receiving command from ventilation failed ({e:?})"
                                );
                                ventilation_channel_disconnected = true;
                            }
                        }
                    }
                }
            }
            // **********************************

            // *** communication with monitors ***
            if !monitors_channel_disconnected {
                match rx_from_monitors.try_recv() {
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
                        }
                        match tx_to_monitors.send(response_value) {
                            Ok(()) => {}
                            Err(e) => {
                                error!(
                                    target: LOG_TARGET_GPIO_HANDLER,
                                    "sending response value back to monitors failed ({e:?})"
                                );
                                monitors_channel_disconnected = true;
                            }
                        }
                    }
                    Err(e) => {
                        match e {
                            TryRecvError::Empty => { /* empty buffer - no action required */ }
                            TryRecvError::Disconnected => {
                                error!(
                                    target: LOG_TARGET_GPIO_HANDLER,
                                    "receiving command from monitors failed ({e:?})"
                                );
                                monitors_channel_disconnected = true;
                            }
                        }
                    }
                }
            }
            // **********************************
        }

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_GPIO_HANDLER,
                    "sending confirmation back to signal handler failed ({e:?})"
                );
            }
        };
    }
}

#[cfg(test)]
pub mod tests {
    #[cfg(target_os = "linux")]
    use crate::config::read_config_file;

    #[cfg(target_os = "linux")]
    use crate::config::ConfigData;

    #[cfg(target_os = "linux")]
    use crate::gpio_handler::GpioHandler;

    #[test]
    #[cfg(target_os = "linux")]
    pub fn test_valid_gpio_config() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_gpio_handler_valid.toml".to_string());
        let _gpio_handler = GpioHandler::new(config.gpio_handler, config.i2c_interface);
    }

    #[test]
    #[cfg(target_os = "linux")]
    #[should_panic]
    pub fn test_invalid_gpio_config_low() {
        let config: ConfigData = read_config_file(
            "/config/aquarium_control_test_gpio_handler_invalid_low.toml".to_string(),
        );
        let _gpio_handler = GpioHandler::new(config.gpio_handler, config.i2c_interface);
    }

    #[test]
    #[cfg(target_os = "linux")]
    #[should_panic]
    pub fn test_invalid_gpio_config_high() {
        let config: ConfigData = read_config_file(
            "/config/aquarium_control_test_gpio_handler_invalid_high.toml".to_string(),
        );
        let _gpio_handler = GpioHandler::new(config.gpio_handler, config.i2c_interface);
    }
}
