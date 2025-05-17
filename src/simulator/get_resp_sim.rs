/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use log::warn;
use std::sync::mpsc::{Receiver, Sender};

use crate::utilities::channel_content::InternalCommand;

const LOG_TARGET_GET_RESP_SIM: &str = "get_resp_sim";

/// The trait implements communication with the simulator instead of real HW.
pub trait GetResponseFromSimulatorTrait {
    /// Requests a response from the simulator via TCP communication by sending
    /// an internal message via the channel to the TCP thread.
    /// This function is used for development only.
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
                                LOG_TARGET_GET_RESP_SIM);
                    }
                },
                Err(e) => {
                    panic!(
                        "{}: Error occurred when trying to send request to TCP thread ({e:?})",
                        LOG_TARGET_GET_RESP_SIM
                    );
                }
            },
            _ => {
                warn!(
                    target: LOG_TARGET_GET_RESP_SIM,
                    "ignoring not applicable internal command ({})",
                    internal_command
                );
                0.0
            }
        }
    }
}
