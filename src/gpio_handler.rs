use serde_derive::Deserialize;
use std::sync::mpsc::{Receiver, Sender};

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
    pub fn new(config: GpioHandlerConfig) -> GpioHandler {
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

    /// this function receives request from other threads and either actuates on real HW
    /// or communicates with simulator
    pub fn execute(
        &self,
        mut tcp_communication_opt: Option<TcpCommunication>,
        tx_to_data_logger: Sender<f32>,
        rx_from_data_logger: Receiver<InternalCommand>,
    ) {
        let mut response_value: f32 = 0.0;

        if self.config.use_simulator && tcp_communication_opt.is_none() {
            panic!("Gpiohandler configured to run with simulator, but no TCP connection provided.");
        }

        loop {
            match rx_from_data_logger.recv() {
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
                    println!("Error in trying to receive command from data logger: {e:?}");
                }
            }
        }
    }
}
