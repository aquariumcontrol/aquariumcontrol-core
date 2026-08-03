/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use thiserror::Error;
use crate::simulator::tcp_communication::TcpCommunicationError;
use crate::utilities::channel_content::AquariumSignal;

/// Contains error definitions for TankLevelSwitch
#[derive(Error, Debug)]
pub enum TankLevelSwitchError {
    /// Application shall use real HW, but GPIO lib interface was not provided.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{0}] Application shall use real HW, but GPIO lib interface was not provided.")]
    GPIOInterfaceNotProvided(String),

    /// Could not get pin from GPIO interface for tank level monitoring.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{location}] Could not get pin {pin_number} from GPIO interface for tank level monitoring.")]
    GpioPinRetrievalError {
        location: String,
        pin_number: u8,

        #[source]
        source: rppal::gpio::Error,
    },

    /// Application shall use simulator, but the channel to TCP thread was not provided.
    #[error("[{0}] Configured to run with simulator, but no channel to TCP thread provided.")]
    SimulatorChannelNotProvided(String),

    /// Input pin has not been provided. This is a configuration error.
    #[allow(unused)] // variant is constructed in platform-specific code and evaluated in generic code.
    #[error("[{0}] Input pin has not been provided. This is a configuration error.")]
    InputPinNotProvided(String),

    /// Communication with the simulator failed.
    #[error("[{location}] Communication with simulator for {signal} failed")]
    SimulatorCommunicationError {
        location: String,
        signal: AquariumSignal,

        #[source]
        source: TcpCommunicationError,
    },
}
