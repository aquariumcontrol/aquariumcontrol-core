/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

// This could be in a new file or at the top of food_injection.rs
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum FoodInjectionError {
    #[error("Failed to send command to relay manager for device: {0}")]
    RelayManagerSend(
        AquariumDevice,
        #[source] std::sync::mpsc::SendError<InternalCommand>,
    ),

    #[error("Failed to receive confirmation from relay manager for device: {0}")]
    RelayManagerReceive(AquariumDevice, #[source] std::sync::mpsc::RecvError),

    #[error("Received undefined state as target state for skimmer.")]
    UndefinedTargetStateSkimmer,

    #[error("Received undefined target state for Feeder.")]
    UndefinedTargetStateFeeder,

    #[error("Received undefined target state for Main Pump 1.")]
    UndefinedTargetStateMainPump1,

    #[error("Received undefined target state for Main Pump 2.")]
    UndefinedTargetStateMainPump2,

    #[error("Received undefined target state for aux. pump.")]
    UndefinedTargetStateAuxPump1,

    #[error("Received undefined target state for aux. pump.")]
    UndefinedTargetStateAuxPump2,
}
