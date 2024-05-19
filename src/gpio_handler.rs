/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
#[cfg(target_os = "linux")]
use {
    rppal::gpio::Gpio,
    rppal::i2c::I2c, // required for communication with AtlasScientific sensors
};

use crate::channel_content::InternalCommand;

#[derive(Deserialize, Debug)]
pub struct GpioHandlerConfig {
    pub use_simulator: bool,
}

pub struct GpioHandler {
    config: GpioHandlerConfig,
}

impl GpioHandler {
    /// This function returns a new instance of GpioHandler
    /// If real hardware is used, then the function also opens interface to GPIO and I2C
    pub fn new(config: GpioHandlerConfig) -> GpioHandler {
        if !config.use_simulator {
            #[cfg(not(target_os = "linux"))]
            panic!("Usage of hardware not supported on this platform.");
        }
        GpioHandler { config: config }
    }

    /// request a response from the simulator via TCP communication by sending
    /// an internal message via channel to the TCP thread.
    /// this function is used for development purposes only
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

    #[cfg(target_os = "linux")]
    fn execute_linux_specific_code() {
        println!("Linux specific code.");
    }

    /// function checks if there are new messages in the channel.
    /// if the message contains the quit command, then the function returns true, otherwise false.
    fn check_pending_external_request(
        &mut self,
        rx_from_signal_handler: &Receiver<InternalCommand>,
    ) -> bool {
        match rx_from_signal_handler.try_recv() {
            Ok(c) => match c {
                InternalCommand::Quit => true,
                _ => {
                    println!("GpioHandler: ignoring not applicable command from signal handler.");
                    false
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        println!(
                            "GpioHandler: Error in trying to receive command from signal handler: {e:?}"
                        );
                    }
                }
                false
            }
        }
    }

    /// this function receives request from other threads and either actuates on real HW
    /// or communicates with simulator
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

        #[cfg(target_os = "linux")]
        execute_linux_specific_code();

        let tx_to_tcp: Sender<InternalCommand>;
        let rx_from_tcp: Receiver<f32>;
        if self.config.use_simulator && (tx_to_tcp_opt.is_none() | rx_from_tcp_opt.is_none()) {
            panic!("Gpiohandler configured to run with simulator, but no channel to TCP thread provided.");
        } else {
            tx_to_tcp = tx_to_tcp_opt.unwrap(); // this unwrap is safe because checked above.
            rx_from_tcp = rx_from_tcp_opt.unwrap(); // this unwrap is safe because checked above.
        }

        loop {
            if self.check_pending_external_request(&rx_from_signal_handler) {
                break;
            }

            // *** communication with data logger ***
            match rx_from_data_logger.try_recv() {
                Ok(c) => {
                    if self.config.use_simulator {
                        response_value =
                            Self::get_response_from_simulator(&tx_to_tcp, &rx_from_tcp, c);
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
                    if self.config.use_simulator {
                        response_value =
                            Self::get_response_from_simulator(&tx_to_tcp, &rx_from_tcp, c);
                    }
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
                    if self.config.use_simulator {
                        response_value =
                            Self::get_response_from_simulator(&tx_to_tcp, &rx_from_tcp, c);
                    }
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
                    if self.config.use_simulator {
                        response_value =
                            Self::get_response_from_simulator(&tx_to_tcp, &rx_from_tcp, c);
                    }
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
                    if self.config.use_simulator {
                        response_value =
                            Self::get_response_from_simulator(&tx_to_tcp, &rx_from_tcp, c);
                    }
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

        // application received request to terminate, that is why the loop was left.
        // answer to thread which sent the request for termination, so that shutdown can proceed further.
        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                println!("GpioHandler: Error in trying to send confirmation back to signal handler: {e:?}");
            }
        };
    }
}
