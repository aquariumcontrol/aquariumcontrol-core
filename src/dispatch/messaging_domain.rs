/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
//! Defines the domains (subsystems) that can be controlled via Inter-Process Communication (IPC).
//!
//! This module serves as a dictionary for the application's controllable parts, such as
//! `Refill`, `Heating`, and `Feed`. It provides both the raw integer constants used in the
//! low-level POSIX message queue protocol and a type-safe Rust `enum` for use in the
//! application's internal logic.
//!
//! This entire module is conditionally compiled and is only available on `target_os = "linux"`.
//!
//! ## Key Components
//!
//! - **`messaging_domains` Module**: A collection of `i32` constants that represent the
//!   unique identifier for each domain in the raw byte message. These are the "on-the-wire"
//!   values that form the external contract with command-line tools.
//!
//! - **`MessagingDomain` Enum**: A type-safe Rust representation of the domains. The main
//!   `Messaging` thread translates the raw integer from an incoming message into one of
//!   these variants before dispatching a command. This prevents logic errors related
//!   to "magic numbers."
//!
//! ## Design and Purpose
//!
//! The separation between the raw constants and the type-safe enum is a deliberate design
//! choice that enhances robustness.
//!
//! - **Protocol Stability**: The `messaging_domains` constants define a stable, language-agnostic
//!   protocol. External C or Python scripts can use these same integer values
//!   to communicate with the application.
//!
//! - **Type Safety**: By converting the raw integer to a `MessagingDomain` enum at the
//!   earliest opportunity, the rest of the application can use pattern matching and
//!   benefit from the compiler's checks, eliminating a whole class of potential bugs.
//!
//! - **Runtime Validation**: The `is_domain_thread_executed` method provides a way to
//!   check if a target domain's thread is actually running before attempting to send
//!   a message, preventing unnecessary channel errors for disabled features.
//!
//! ### Example Flow
//!
//! 1.  An external tool sends a message with the domain field set to `1`.
//! 2.  The `Messaging` thread receives the message and reads the integer `1`.
//! 3.  It matches `1` to `messaging_domains::REFILL`.
//! 4.  It converts this into the `MessagingDomain::Refill` enum variant.
//! 5.  It then uses this enum variant to route the command to the correct channel.

#[cfg(target_os = "linux")]
use crate::launch::execution_config::ExecutionConfig;
#[cfg(target_os = "linux")]
use std::fmt;

#[derive(PartialEq, Debug, Clone)]
/// Identifies the addressee of the message
#[cfg(target_os = "linux")]
#[repr(i32)]
pub enum MessagingDomain {
    /// refill control
    Refill = 1,

    /// ventilation control
    Ventilation = 3,

    /// feed control
    Feed = 8,

    /// heating control
    Heating = 9,

    /// Balling dosing control
    Balling = 11,

    /// monitors
    Monitors = 12,

    /// watchdog communication
    Watchdog = 13,

    /// in case the numeric value in the message could not be assigned to one of the above
    Unknown = -1,
}

#[cfg(target_os = "linux")]
impl TryFrom<i32> for MessagingDomain {
    fn from(value: i32) -> Self {
        match value {
            x if x == MessagingDomain::Refill as i32 => Ok(MessagingDomain::Refill),
            x if x == MessagingDomain::Ventilation as i32 => Ok(MessagingDomain::Ventilation),
            x if x == MessagingDomain::Feed as i32 => Ok(MessagingDomain::Feed),
            x if x == MessagingDomain::Heating as i32 => Ok(MessagingDomain::Heating),
            x if x == MessagingDomain::Balling as i32 => Ok(MessagingDomain::Balling),
            x if x == MessagingDomain::Monitors as i32 => Ok(MessagingDomain::Monitors),
            x if x == MessagingDomain::Watchdog as i32 => Ok(MessagingDomain::Watchdog),
            _ => MessagingDomain::UnknownDomain,
        }
    }
}

#[cfg(target_os = "linux")]
impl MessagingDomain {
    /// Checks if the thread corresponding to a specific messaging domain is active.
    ///
    /// This method consults the provided `ExecutionConfig` to determine if the thread
    /// responsible for handling a given `MessagingDomain` was started at application launch.
    /// This is useful for preventing attempts to send messages to threads that are not
    /// running, which would otherwise result in channel errors.
    ///
    /// # Arguments
    /// * `execution_config` - A reference to the application's runtime execution
    ///   configuration, which holds the active status of each thread.
    ///
    /// # Returns
    /// Returns `true` if the thread for the domain is configured as active, and `false`
    /// if the thread is inactive or if the domain is `MessagingDomain::Unknown`.
    pub fn is_domain_thread_executed(&self, execution_config: &ExecutionConfig) -> bool {
        match self {
            MessagingDomain::Refill => execution_config.refill,
            MessagingDomain::Ventilation => execution_config.ventilation,
            MessagingDomain::Feed => execution_config.feed,
            MessagingDomain::Heating => execution_config.heating,
            MessagingDomain::Balling => execution_config.balling,
            MessagingDomain::Monitors => execution_config.monitors,
            MessagingDomain::Watchdog => execution_config.watchdog,
            MessagingDomain::Unknown => false,
        }
    }
}

#[cfg(target_os = "linux")]
impl fmt::Display for MessagingDomain {
    /// Formats the `MessagingDomain` enum into its human-readable string representation.
    ///
    /// This implementation allows `MessagingDomain` variants to be displayed directly
    /// using macros like `println!` or `format!`, returning the name of the domain.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to the formatter, as required by the `fmt::Display` trait.
    ///
    /// # Returns
    /// A `fmt::Result` indicating whether the formatting operation was successful.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MessagingDomain::Refill => write!(f, "Refill"),
            MessagingDomain::Ventilation => write!(f, "Ventilation"),
            MessagingDomain::Feed => write!(f, "Feed"),
            MessagingDomain::Heating => write!(f, "Heating"),
            MessagingDomain::Balling => write!(f, "Balling"),
            MessagingDomain::Monitors => write!(f, "Monitors"),
            MessagingDomain::Watchdog => write!(f, "Watchdog"),
            MessagingDomain::Unknown => write!(f, "Unknown"),
        }
    }
}
