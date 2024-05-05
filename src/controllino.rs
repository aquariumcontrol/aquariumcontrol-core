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
            InternalCommand::RequestSignal(_)
            | InternalCommand::SetRelay(_)
            | InternalCommand::UnsetRelay(_)
            | InternalCommand::ResetAllErrors
            | InternalCommand::Quit => {
                println!(
                    "Controllino: ignoring irrelevant internal command {}.",
                    internal_command.to_string()
                );
            }
        }
    }

    /// this function receives request from other threads and either actuates on real HW
    /// or communicates with simulator
    pub fn execute(
        &self,
        tx_to_tcp_opt: Option<Sender<InternalCommand>>,
        tx_to_refill: Sender<bool>,
        rx_from_refill: Receiver<InternalCommand>,
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
            // *** communication with refill control ***
            match rx_from_refill.try_recv() {
                Ok(c) => {
                    println!("Controllino: received request from Refill thread.");
                    if self.config.use_simulator {
                        self.actuate_in_simulator(&tx_to_tcp, c);
                    }
                    // todo: else branch using real

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
        }
    }
}
