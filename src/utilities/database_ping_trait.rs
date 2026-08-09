/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines a trait for periodically pinging a database connection to keep it alive.
//!
//! This module provides the `DatabasePingTrait`, a reusable abstraction for handling
//! a common problem in long-running applications: database connection timeouts.
//! Many database systems will close idle connections after a certain period. This
//! trait encapsulates a standardized pattern for sending a lightweight "ping"
//! command to the database at a regular interval, ensuring the connection remains active.
//!
//! ## Purpose and Design
//!
//! Instead of each concurrent thread implementing its own timing and pinging logic,
//! this trait provides a single, consistent implementation. This achieves several goals:
//!
//! - **Reduces Boilerplate**: Eliminates repetitive timing and pinging code across
//!   multiple modules (`Heating`, `DataLogger`, `Feed`, etc.).
//! - **Enforces a Standard**: Ensures all modules use the same robust logic for
//!   connection keep-alive.
//! - **Decoupling**: The trait depends on a generic `Pingable` object, decoupling the
//!   timing logic from the specific database implementation.
//!
//! ## Key Components
//!
//! - **`DatabasePingTrait`**: The core trait.
//!   - **`get_ping_interval()`**, **`get_last_ping_instant()`**, **`update_last_ping_instant()`**:
//!     A set of required methods that implementor must provide to manage the state
//!     (the interval and the last ping time).
//!   - **`check_timing_and_ping_database()`**: A provided "default" method that contains
//!     the core logic. It checks if the interval has elapsed and, if so, calls the
//!     `ping()` method on the provided `Pingable` object and resets its internal timer.
//!
//! - **`Pingable` Trait**: An external trait (defined in `database::pingable`) that
//!   this module depends on. It represents any object that has a `.ping()` method,
//!   typically a database connection pool.

use crate::database::pingable::Pingable;
use crate::food::feed::Feed;
use crate::mineral::balling::Balling;
use crate::permission::schedule_check::ScheduleCheck;
use crate::thermal::heating::Heating;
use crate::water::refill::Refill;
use std::time::{Duration, Instant};

pub trait DatabasePingTrait {
    /// Gets the interval at which the resource should be pinged.
    fn get_ping_interval(&self) -> Duration;

    /// Gets the timestamp of the last successful ping.
    fn get_last_ping_instant(&self) -> Instant;

    /// Updates the timestamp of the last successful ping.
    fn update_last_ping_instant(&mut self);

    /// Pings the database.
    ///
    /// # Arguments
    /// * `database_interface` - The database interface connection.
    fn check_timing_and_ping_database(
        &mut self,
        database_interface: &mut (impl Pingable + ?Sized),
        is_multi_thread: bool,
    ) {
        if self.get_last_ping_instant().elapsed() > self.get_ping_interval() {
            if is_multi_thread {
                tokio::task::block_in_place(|| database_interface.ping());
            } else {
                database_interface.ping();
            }
            self.update_last_ping_instant();
        }
    }
}

/// A macro to generate the `DatabasePingTrait` implementation for a given struct.
/// This avoids boilerplate code for structs that share the same field names
/// for the ping interval and the last ping timestamp.
macro_rules! impl_database_ping_trait {
    ($struct_name:ty) => {
        impl DatabasePingTrait for $struct_name {
            fn get_ping_interval(&self) -> Duration {
                self.database_ping_interval
            }

            /// Retrieves the last ping instant.
            fn get_last_ping_instant(&self) -> Instant {
                self.last_ping_instant
            }

            /// Updates the last ping instant.
            fn update_last_ping_instant(&mut self) {
                self.last_ping_instant = Instant::now();
            }
        }
    };
}

impl_database_ping_trait!(Feed);
impl_database_ping_trait!(Refill);
impl_database_ping_trait!(Heating);
impl_database_ping_trait!(Balling);
impl_database_ping_trait!(ScheduleCheck);

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    // A mock struct to act as the object that can be pinged.
    // It implements the `Pingable` trait and tracks if its `ping` method was called.
    struct MockPingable {
        ping_was_called: bool,
    }

    impl MockPingable {
        /// Creates a new instance.
        fn new() -> Self {
            Self {
                ping_was_called: false,
            }
        }
    }

    impl Pingable for MockPingable {
        /// Performs the `ping` operation.
        fn ping(&mut self) {
            self.ping_was_called = true;
        }
    }

    // A mock struct to implement the `DatabasePingTrait`.
    // This allows us to test the trait's default method in isolation.
    struct MockPinger {
        database_ping_interval: Duration,
        last_ping_instant: Instant,
    }

    impl DatabasePingTrait for MockPinger {
        /// Retrieves the ping interval.
        fn get_ping_interval(&self) -> Duration {
            self.database_ping_interval
        }

        /// Retrieves the last ping instant.
        fn get_last_ping_instant(&self) -> Instant {
            self.last_ping_instant
        }

        /// Updates the last ping instant.
        fn update_last_ping_instant(&mut self) {
            self.last_ping_instant = Instant::now();
        }
    }

    #[test]
    // [utest->swreq~utilities_check_version_compatibility~1]
    // Verifies that ping not needed when interval not elapsed.
    fn test_ping_not_needed_when_interval_not_elapsed() {
        // Arrange: Set up a pinger with a long interval and a recent last_ping time.
        let mut pinger = MockPinger {
            database_ping_interval: Duration::from_secs(10),
            last_ping_instant: Instant::now(),
        };
        let mut pingable = MockPingable::new();

        // Act: Check if a ping is needed.
        pinger.check_timing_and_ping_database(&mut pingable, false);

        // Assert: The pingable object's ping method should NOT have been called.
        assert!(
            !pingable.ping_was_called,
            "Ping should not have been called as the interval has not elapsed."
        );
    }

    #[test]
    // [utest->swreq~utilities_check_version_compatibility~1]
    // Verifies that ping is needed when interval has elapsed.
    fn test_ping_is_needed_when_interval_has_elapsed() {
        // Arrange: Set up a pinger with a short interval and a last_ping time in the past.
        let interval = Duration::from_millis(10);
        let mut pinger = MockPinger {
            database_ping_interval: interval,
            last_ping_instant: Instant::now() - (interval + Duration::from_millis(5)),
        };
        let mut pingable = MockPingable::new();

        // Act: Check if a ping is needed.
        pinger.check_timing_and_ping_database(&mut pingable, false);

        // Assert: The pingable object's ping method SHOULD have been called.
        assert!(
            pingable.ping_was_called,
            "Ping should have been called because the interval has elapsed."
        );
    }

    #[test]
    // [utest->swreq~utilities_check_version_compatibility~1]
    // Verifies that ping timer is reset after pinging.
    fn test_ping_timer_is_reset_after_pinging() {
        // Arrange: Set up a pinger where the interval has elapsed.
        let interval = Duration::from_millis(10);
        let mut pinger = MockPinger {
            database_ping_interval: interval,
            last_ping_instant: Instant::now() - (interval + Duration::from_millis(5)),
        };
        let mut pingable = MockPingable::new();

        // Act 1: The first check should trigger a ping.
        pinger.check_timing_and_ping_database(&mut pingable, false);

        // Assert 1: The ping was triggered.
        assert!(
            pingable.ping_was_called,
            "Ping should have been called the first time."
        );

        // Arrange 2: Reset the mock's flag for the next check.
        pingable.ping_was_called = false;

        // Act 2: Immediately check again. The timer should have been reset, so no ping is needed.
        pinger.check_timing_and_ping_database(&mut pingable, false);

        // Assert 2: The ping was NOT triggered a second time.
        assert!(
            !pingable.ping_was_called,
            "Ping should not be called again immediately, as the timer should have been reset."
        );
    }
}
