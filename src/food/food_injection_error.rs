/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

// This could be in a new file or at the top of food_injection.rs
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
use thiserror::Error;

/// Contains error definition for FoodInjection
#[derive(Error, Debug, Clone)]
pub enum FoodInjectionError {
    /// Occurs when sending a command to the relay manager via an MPSC channel fails.
    /// This typically happens if the receiving end (the relay manager) has been dropped.
    #[error("[{location}] Failed to send command to relay manager for device: {device}")]
    RelayManagerSend {
        location: String,
        device: AquariumDevice,

        #[source]
        source: std::sync::mpsc::SendError<InternalCommand>,
    },

    /// Occurs when waiting for a confirmation message from the relay manager fails.
    /// This usually indicates that the relay manager's sending channel was dropped,
    /// preventing a response from ever being sent.
    #[error("[{location}] Failed to receive confirmation from relay manager for device: {device}")]
    RelayManagerReceive {
        location: String,
        device: AquariumDevice,

        #[source]
        source: std::sync::mpsc::RecvError,
    },

    /// An invalid or unexpected target state was requested for the skimmer.
    /// The skimmer should only be turned ON or OFF.
    #[error("[{0}] Received undefined state as target state for skimmer.")]
    UndefinedTargetStateSkimmer(String),

    /// An invalid or unexpected target state was requested for the feeder.
    /// The feeder should only be turned ON or OFF.
    #[error("[{0}] Received undefined target state for Feeder.")]
    UndefinedTargetStateFeeder(String),

    /// An invalid or unexpected target state was requested for the first main pump.
    /// The pump should only be turned ON or OFF.
    #[error("[{0}] Received undefined target state for Main Pump 1.")]
    UndefinedTargetStateMainPump1(String),

    /// An invalid or unexpected target state was requested for the second main pump.
    /// The pump should only be turned ON or OFF.
    #[error("[{0}] Received undefined target state for Main Pump 2.")]
    UndefinedTargetStateMainPump2(String),

    /// An invalid or unexpected target state was requested for the first auxiliary pump.
    /// The pump should only be turned ON or OFF.
    #[error("[{0}] Received undefined target state for aux. pump.")]
    UndefinedTargetStateAuxPump1(String),

    /// An invalid or unexpected target state was requested for the second auxiliary pump.
    /// The pump should only be turned ON or OFF.
    #[error("[{0}] Received undefined target state for aux. pump.")]
    UndefinedTargetStateAuxPump2(String),
}
