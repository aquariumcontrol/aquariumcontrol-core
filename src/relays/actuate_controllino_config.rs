/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use serde_derive::Deserialize;

#[derive(Clone, Deserialize, Debug)]
/// Configuration settings for direct hardware actuation via the Controllino device.
///
/// This struct holds all parameters required to establish and manage communication
/// with the Controllino (e.g., serial port settings) and maps application-level
/// devices to their specific relay IDs on the Controllino board.
pub struct ActuateControllinoConfig {
    /// Disables the functionality - required for certain testing scenarios
    pub active: bool,

    /// Baud rate for communication via serial port
    pub baud_rate: u32,

    /// Port name for communication via serial port
    pub port_name: String,

    /// timeout in milliseconds for serial port communication
    pub timeout_millis: u64,

    /// time in milliseconds to wait for response
    pub controllino_processing_millis: u64,

    /// Relay id of the protein skimmer
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub skimmer_id: u8,

    /// Relay id of the main pump #1
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub main_pump1_id: u8,

    /// Relay id of the main pump #2
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub main_pump2_id: u8,

    /// Relay id of the auxiliary pump #1
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub aux_pump1_id: u8,

    /// Relay id of the auxiliary pump #2
    /// - Caution: relay login (true/false) and device logic (off/on) is reverse.
    pub aux_pump2_id: u8,

    /// Relay id of the refill pump
    pub refill_pump_id: u8,

    /// Relay id of the heater
    pub heater_id: u8,

    /// Relay id of ventilation
    pub ventilation_id: u8,

    /// Relay id feeder
    pub feeder_id: u8,

    /// Relay id of peristaltic pump #1
    pub peristaltic_pump1_id: u8,

    /// Relay id of peristaltic pump #2
    pub peristaltic_pump2_id: u8,

    /// Relay id of peristaltic pump #3
    pub peristaltic_pump3_id: u8,

    /// Relay id of peristaltic pump #4
    pub peristaltic_pump4_id: u8,

    /// Heartbeat interval
    pub heartbeat_interval_seconds: u64,
}
