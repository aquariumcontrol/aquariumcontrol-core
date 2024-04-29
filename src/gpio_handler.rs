use serde_derive::Deserialize;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
#[cfg(target_os = "linux")]
use {
    rppal::gpio::Gpio,
    rppal::i2c::I2c, // required for communication with AtlasScientific sensors
};

use crate::channel_content::InternalCommand;
use crate::tcp_communication::TcpCommunication;

#[derive(Deserialize, Debug)]
pub struct GpioHandlerConfig {
    pub use_simulator: bool,
}

pub struct GpioHandler {
    config: GpioHandlerConfig,
}

impl GpioHandler {
    /// this function returns a new instance of GpioHandler
    /// if real hardware is used, then the function also opens interface to GPIO and I2C
    pub fn new(config: GpioHandlerConfig) -> GpioHandler {
        if !config.use_simulator {
            #[cfg(not(target_os = "linux"))]
            panic!("Usage of hardware not supported on this platform.");
        }
        GpioHandler { config: config }
    }

    /// request a response from the simulator via TCP communication
    /// this function is used for development purposes only
    fn get_response_from_simulator(
        tcp_communication_opt: Option<&mut TcpCommunication>,
        internal_command: InternalCommand,
    ) -> f32 {
        let tcp_communication = tcp_communication_opt.unwrap();
        match internal_command {
            InternalCommand::RequestSignal(c) => tcp_communication.get_signal(c),
            //InternalCommand::SwitchOn(c) => {0.0},
            //InternalCommand::SwitchOff(c) => {0.0},
            //InternalCommand::Quit => {0.0},
        }
    }

    #[cfg(target_os = "linux")]
    fn execute_linux_specific_code() {
        println!("Linux specific code.");
    }
    /// this function receives request from other threads and either actuates on real HW
    /// or communicates with simulator
    pub fn execute(
        &self,
        mut tcp_communication_opt: Option<TcpCommunication>,
        tx_to_data_logger: Sender<f32>,
        rx_from_data_logger: Receiver<InternalCommand>,
        tx_to_refill: Sender<bool>,
        rx_from_refill: Receiver<InternalCommand>,
    ) {
        let mut response_value: f32 = 0.0;

        //if cfg!(linux) {
        #[cfg(target_os = "linux")]
        execute_linux_specific_code();
        //}
        if self.config.use_simulator && tcp_communication_opt.is_none() {
            panic!("Gpiohandler configured to run with simulator, but no TCP connection provided.");
        }

        loop {
            // *** communication with data logger ***
            match rx_from_data_logger.try_recv() {
                Ok(c) => {
                    if self.config.use_simulator {
                        response_value =
                            Self::get_response_from_simulator(tcp_communication_opt.as_mut(), c);
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

            // *** communication with refill ***
            match rx_from_refill.try_recv() {
                Ok(c) => {
                    if self.config.use_simulator {
                        response_value =
                            Self::get_response_from_simulator(tcp_communication_opt.as_mut(), c);
                    }
                    match tx_to_refill.send(response_value > 0.0) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Error in trying to send response value back to refill control: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("GpioHandler: Error in trying to receive command from refill control: {e:?}");
                        }
                    }
                }
            }
            // **********************************
        }
    }
}
