use serde_derive::Deserialize;
use crate::channel_content::AquariumDevice;

pub enum DeviceState {
    On,
    Off,
}

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
}

/// This struct contains the configuration and the implementation of the Controllino interface
pub struct Controllino {
    pub config: ControllinoConfig,
}

impl Controllino {
    pub fn new(config: ControllinoConfig) -> Controllino {
        Controllino {
            config,
        }
    }
    
    pub fn switch(&self, device: AquariumDevice, state: DeviceState) {
        let relay_id = match device {
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
        };
    }
    
    pub fn switch_on(&self, device: AquariumDevice) {
        self.switch(device, DeviceState::On);
    }
    
    pub fn switch_off(&self, device: AquariumDevice) {
        self.switch(device, DeviceState::Off);
    }
}