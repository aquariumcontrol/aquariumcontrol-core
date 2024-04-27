use serde_derive::Deserialize;
use std::sync::mpsc::{Receiver, Sender};

use crate::channel_content::InternalCommand;
use crate::tcp_communication::TcpCommunication;

#[derive(Deserialize, Debug)]
pub struct GpioHandlerConfig {
    use_simulator: bool,
}

pub struct GpioHandler {
    config: GpioHandlerConfig,
}

impl GpioHandler {
    pub fn new(config: GpioHandlerConfig) -> GpioHandler {
        GpioHandler { config: config }
    }

    fn get_response_from_simulator(
        tcp_communication: &mut TcpCommunication,
        internal_command: InternalCommand,
    ) -> f32 {
        match internal_command {
            InternalCommand::RequestSignal(c) => tcp_communication.get_signal(c),
            //InternalCommand::SwitchOn(c) => {0.0},
            //InternalCommand::SwitchOff(c) => {0.0},
            //InternalCommand::Quit => {0.0},
        }
    }

    pub fn execute(
        &self,
        mut tcp_communication: TcpCommunication,
        tx_to_data_logger: Sender<f32>,
        rx_from_data_logger: Receiver<InternalCommand>,
    ) {
        let mut response_value: f32 = 0.0;

        loop {
            match rx_from_data_logger.recv() {
                Ok(c) => {
                    if self.config.use_simulator {
                        response_value = Self::get_response_from_simulator(&mut tcp_communication, c);
                    }
                    match tx_to_data_logger.send(response_value) {
                        Ok(()) => {},
                        Err(e) => {
                            println!("Error in trying to send response value back to data logger: {e:?}");   
                        },
                    }
                }
                Err(e) => {
                    println!("Error in trying to receive command from data logger: {e:?}");
                }
            }
        }
    }
}
