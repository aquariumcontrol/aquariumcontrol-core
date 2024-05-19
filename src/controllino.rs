/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use crate::channel_content::{AquariumDevice, InternalCommand};

/// This struct holds the configuration data for the Controllino interface.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct ControllinoConfig {
    /// Relay id of the protein skimmer
    /// Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub skimmer_id: u16,

    /// Relay id of main pump #1
    /// Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub main_pump1_id: u16,

    /// Relay id of main pump #2
    /// Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub main_pump2_id: u16,

    /// Relay id of auxiliary pump #1
    /// Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub aux_pump1_id: u16,

    /// Relay id of auxiliary pump #2
    /// Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub aux_pump2_id: u16,

    /// Relay id of refill pump
    pub refill_pump_id: u16,

    /// Relay id of heater
    pub heater_id: u16,

    /// Relay id of ventilation
    pub ventilation_id: u16,

    /// Relay id feeder
    pub feeder_id: u16,

    /// Relay id of peristaltic pump #1
    pub peristaltic_pump1_id: u16,

    /// Relay id of peristaltic pump #2
    pub peristaltic_pump2_id: u16,

    /// Relay id of peristaltic pump #3
    pub peristaltic_pump3_id: u16,

    /// Relay id of peristaltic pump #4
    pub peristaltic_pump4_id: u16,

    pub use_simulator: bool,
}

/// This struct contains the configuration and the implementation of the Controllino interface
pub struct Controllino {
    pub config: ControllinoConfig,
}

impl Controllino {
    pub fn new(config: ControllinoConfig) -> Controllino {
        Controllino { config }
    }

    fn get_relay_id(&self, device: AquariumDevice) -> u16 {
        match device {
            AquariumDevice::Skimmer => self.config.skimmer_id,
            AquariumDevice::MainPump1 => self.config.main_pump1_id,
            AquariumDevice::MainPump2 => self.config.main_pump2_id,
            AquariumDevice::AuxPump1 => self.config.aux_pump1_id,
            AquariumDevice::AuxPump2 => self.config.aux_pump2_id,
            AquariumDevice::Heater => self.config.heater_id,
            AquariumDevice::Ventilation => self.config.ventilation_id,
            AquariumDevice::RefillPump => self.config.refill_pump_id,
            AquariumDevice::Feeder => self.config.feeder_id,
            AquariumDevice::PeristalticPump1 => self.config.peristaltic_pump1_id,
            AquariumDevice::PeristalticPump2 => self.config.peristaltic_pump2_id,
            AquariumDevice::PeristalticPump3 => self.config.peristaltic_pump3_id,
            AquariumDevice::PeristalticPump4 => self.config.peristaltic_pump4_id,
        }
    }

    #[cfg(target_os = "linux")]
    fn execute_linux_specific_code() {
        println!("Linux specific code.");
    }

    /// trigger actuation on simulator via TCP communication
    /// this function is used for development purposes only.
    /// It translates the commands SwitchOn and SwitchOff to SetRelay and UnsetRelay
    /// considering relay id and relay logic.
    fn actuate_in_simulator(
        &self,
        tx_to_tcp: &Sender<InternalCommand>,
        internal_command: InternalCommand,
    ) {
        let relay_id: u16;
        match internal_command {
            InternalCommand::SwitchOn(c) => {
                relay_id = self.get_relay_id(c);
                match tx_to_tcp.send(InternalCommand::SetRelay(relay_id)) {
                    Ok(()) => {}
                    Err(e) => {
                        panic!("Controllino: error when sending request for switching on relay #{} to TCP thread: {e:?}", relay_id);
                    }
                }
            }
            InternalCommand::SwitchOff(c) => {
                relay_id = self.get_relay_id(c);
                match tx_to_tcp.send(InternalCommand::UnsetRelay(relay_id)) {
                    Ok(()) => {}
                    Err(e) => {
                        panic!("Controllino: error when sending request for switching on relay #{} to TCP thread: {e:?}", relay_id);
                    }
                }
            }
            _ => {
                println!(
                    "Controllino: ignoring irrelevant internal command {}.",
                    internal_command.to_string()
                );
            }
        }
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
                    println!("Controllino: ignoring not applicable command from signal handler.");
                    false
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => { /* empty buffer - no action required */ }
                    TryRecvError::Disconnected => {
                        println!(
                            "Controllino: Error in trying to receive command from signal handler: {e:?}"
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
        tx_to_refill: Sender<bool>,
        rx_from_refill: Receiver<InternalCommand>,
        tx_to_heating: Sender<bool>,
        rx_from_heating: Receiver<InternalCommand>,
        tx_to_ventilation: Sender<bool>,
        rx_from_ventilation: Receiver<InternalCommand>,
        tx_to_balling: Sender<bool>,
        rx_from_balling: Receiver<InternalCommand>,
        tx_to_feed: Sender<bool>,
        rx_from_feed: Receiver<InternalCommand>,
        tx_to_signal_handler: Sender<bool>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        #[cfg(target_os = "linux")]
        execute_linux_specific_code();

        let tx_to_tcp: Sender<InternalCommand>;
        if self.config.use_simulator && tx_to_tcp_opt.is_none() {
            panic!("Controllino configured to run with simulator, but no channel to TCP thread provided.");
        } else {
            tx_to_tcp = tx_to_tcp_opt.unwrap(); // this unwrap is safe because checked above.
        }

        loop {
            if self.check_pending_external_request(&rx_from_signal_handler) {
                break;
            }

            // *** communication with refill control ***
            match rx_from_refill.try_recv() {
                Ok(c) => {
                    println!("Controllino: received request from Refill thread.");
                    if self.config.use_simulator {
                        self.actuate_in_simulator(&tx_to_tcp, c);
                    }
                    // @todo: else branch using real HW

                    match tx_to_refill.send(true) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in trying to send response value back to refill control: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("Controllino: Error in trying to receive command from refill control {e:?}");
                        }
                    }
                }
            }
            // **********************************

            // *** communication with heating control ***
            match rx_from_heating.try_recv() {
                Ok(c) => {
                    println!("Controllino: received request from Heating thread.");
                    if self.config.use_simulator {
                        self.actuate_in_simulator(&tx_to_tcp, c);
                    }
                    // @todo: else branch using real HW

                    match tx_to_heating.send(true) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in trying to send response value back to heating control: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("Controllino: Error in trying to receive command from heating control {e:?}");
                        }
                    }
                }
            }
            // **********************************
            
            // *** communication with ventilation control ***
            match rx_from_ventilation.try_recv() {
                Ok(c) => {
                    println!("Controllino: received request from Ventilation thread.");
                    if self.config.use_simulator {
                        self.actuate_in_simulator(&tx_to_tcp, c);
                    }
                    // @todo: else branch using real HW

                    match tx_to_ventilation.send(true) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in trying to send response value back to ventilation control: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("Controllino: Error in trying to receive command from ventilation control {e:?}");
                        }
                    }
                }
            }
            // **********************************
            
            // *** communication with Balling dosing control ***
            match rx_from_balling.try_recv() {
                Ok(c) => {
                    println!("Controllino: received request from Balling thread.");
                    if self.config.use_simulator {
                        self.actuate_in_simulator(&tx_to_tcp, c);
                    }
                    // @todo: else branch using real HW

                    match tx_to_balling.send(true) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in trying to send response value back to Balling dosing control: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("Controllino: Error in trying to receive command from Balling dosing control {e:?}");
                        }
                    }
                }
            }
            // **********************************

            // *** communication with feed control ***
            match rx_from_feed.try_recv() {
                Ok(c) => {
                    println!("Controllino: received request from Feed thread.");
                    if self.config.use_simulator {
                        self.actuate_in_simulator(&tx_to_tcp, c);
                    }
                    // @todo: else branch using real HW

                    match tx_to_feed.send(true) {
                        Ok(()) => {}
                        Err(e) => {
                            println!(
                                "Controllino: Error in trying to send response value back to feed control: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("Controllino: Error in trying to receive command from feed control {e:?}");
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
                println!("Controllino: Error in trying to send confirmation back to signal handler: {e:?}");
            }
        };
    }
}
