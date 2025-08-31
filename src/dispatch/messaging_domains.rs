/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(target_os = "linux")]
/// Contains constants of all domains addressed in IPC messages
pub mod messaging_domains {
    /// value represents the refill control as a domain
    pub const REFILL: i32 = 1;

    /// value represents the ventilation control as a domain
    pub const VENTILATION: i32 = 3;

    /// value represents the feed control as a domain
    pub const FEED: i32 = 8;

    /// value represents the heating control as a domain
    pub const HEATING: i32 = 9;

    /// value represents the Balling dosing control as a domain
    pub const BALLING: i32 = 11;

    /// value represents the monitoring as a domain
    pub const MONITORS: i32 = 12;

    /// value represents the watchdog communication as a domain
    pub const WATCHDOG: i32 = 13;
}

#[derive(PartialEq, Debug, Clone)]
/// Identifies the addressee of the message
#[cfg(target_os = "linux")]
pub enum MessagingDomain {
    /// refill control
    Refill,

    /// ventilation control
    Ventilation,

    /// feed control
    Feed,

    /// heating control
    Heating,

    /// Balling dosing control
    Balling,

    /// monitors
    Monitors,

    /// watchdog communication
    Watchdog,

    /// in case the numeric value in the message could not be assigned to one of the above
    Unknown,
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
