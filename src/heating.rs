use chrono::Local;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::fmt;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::sawtooth_profile::SawToothProfileConfig;
use crate::channel_content::AquariumDevice;
use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;

/// This struct holds the configuration data for the heating control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct HeatingConfig {
    switch_on_temperature: f32,
    switch_off_temperature: f32,
    stop_on_refill_error: bool,
    stop_on_refill_error_delay: u32,
    saw_tooth_profile_config: SawToothProfileConfig,
}