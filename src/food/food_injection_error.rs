/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the custom error type for the food injection process.
//!
//! This module contains the `FoodInjectionError` enum, which consolidates all potential
//! failures that can occur when the `FoodInjection` controller executes a feed pattern.
//! By using a dedicated, descriptive error type, the system can provide clear diagnostics
//! when a feeding action fails.
//!
//! ## Design and Purpose
//!
//! The `FoodInjectionError` enum is designed using the `thiserror` crate to provide
//! structured and informative error messages.
//!
//! - **Source Chaining**: For errors originating from channel communication (e.g.,
//!   `RelayManagerSend`), the `#[source]` attribute preserves the underlying
//!   `AquaChannelError`. This creates a full error chain, which is invaluable for
//!   debugging the root cause of a communication failure.
//!
//! - **Contextual Information**: Each error variant includes important context, such as
//!   the code `location` and the specific `AquariumDevice` that was being controlled
//!   when the error occurred.
//!
//! - **Error Collection**: The `FoodInjection` process is designed to collect multiple
//!   errors into a `Vec<FoodInjectionError>` rather than failing on the first one.
//!   This allows it to report a complete set of failures from a single injection
//!   attempt (e.g., if communication with multiple devices fails).
//!
//! ### Error Categories
//!
//! The errors fall into two main categories:
//!
//! 1.  **Channel Communication Failures**: These occur when sending a command to, or
//!     receiving a confirmation from, the `RelayManager` thread fails.
//!
//! 2.  **Invalid State Errors**: These occur if the logic attempts to set a device to an
//!     undefined state (i.e., something other than `On` or `Off`).

use crate::launch::channels::AquaChannelError;
use crate::utilities::channel_content::AquariumDevice;
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
        source: AquaChannelError,
    },

    /// Occurs when waiting for a confirmation message from the relay manager fails.
    /// This usually indicates that the relay manager's sending channel was dropped,
    /// preventing a response from ever being sent.
    #[error("[{location}] Failed to receive confirmation from relay manager for device: {device}")]
    RelayManagerReceive {
        location: String,
        device: AquariumDevice,

        #[source]
        source: AquaChannelError,
    },

    /// An invalid or unexpected target state was requested for a device.
    /// The device should only be turned ON or OFF.
    #[error("[{0}] Received undefined state as target state for {1}.")]
    UndefinedTargetState(String, AquariumDevice),
}
