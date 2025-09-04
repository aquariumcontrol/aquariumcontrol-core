use crate::database::pingable::Pingable;
use crate::food::feed::Feed;
use crate::mineral::balling::Balling;
use crate::permission::schedule_check::ScheduleCheck;
use crate::recorder::data_logger::DataLogger;
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
    fn check_timing_and_ping_database(&mut self, database_interface: &mut (impl Pingable + ?Sized)) {
        if self.get_last_ping_instant().elapsed() > self.get_ping_interval() {
            database_interface.ping();
            self.update_last_ping_instant();
        }
    }
}

impl DatabasePingTrait for DataLogger {
    fn get_ping_interval(&self) -> Duration {
        self.database_ping_interval
    }

    fn get_last_ping_instant(&self) -> Instant {
        self.last_ping_instant
    }

    fn update_last_ping_instant(&mut self) {
        self.last_ping_instant = Instant::now();
    }
}

impl DatabasePingTrait for Feed {
    fn get_ping_interval(&self) -> Duration {
        self.database_ping_interval
    }

    fn get_last_ping_instant(&self) -> Instant {
        self.last_ping_instant
    }

    fn update_last_ping_instant(&mut self) {
        self.last_ping_instant = Instant::now();
    }
}

impl DatabasePingTrait for Refill {
    fn get_ping_interval(&self) -> Duration {
        self.database_ping_interval
    }

    fn get_last_ping_instant(&self) -> Instant {
        self.last_ping_instant
    }

    fn update_last_ping_instant(&mut self) {
        self.last_ping_instant = Instant::now();
    }
}

impl DatabasePingTrait for Heating {
    fn get_ping_interval(&self) -> Duration {
        self.database_ping_interval
    }

    fn get_last_ping_instant(&self) -> Instant {
        self.last_ping_instant
    }

    fn update_last_ping_instant(&mut self) {
        self.last_ping_instant = Instant::now();
    }
}

impl DatabasePingTrait for Balling {
    fn get_ping_interval(&self) -> Duration {
        self.database_ping_interval
    }

    fn get_last_ping_instant(&self) -> Instant {
        self.last_ping_instant
    }

    fn update_last_ping_instant(&mut self) {
        self.last_ping_instant = Instant::now();
    }
}

impl DatabasePingTrait for ScheduleCheck {
    fn get_ping_interval(&self) -> Duration {
        self.database_ping_interval
    }

    fn get_last_ping_instant(&self) -> Instant {
        self.last_ping_instant
    }

    fn update_last_ping_instant(&mut self) {
        self.last_ping_instant = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A mock struct to act as the object that can be pinged.
    // It implements the `Pingable` trait and tracks if its `ping` method was called.
    struct MockPingable {
        ping_was_called: bool,
    }

    impl MockPingable {
        fn new() -> Self {
            Self {
                ping_was_called: false,
            }
        }
    }

    impl Pingable for MockPingable {
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
        fn get_ping_interval(&self) -> Duration {
            self.database_ping_interval
        }

        fn get_last_ping_instant(&self) -> Instant {
            self.last_ping_instant
        }

        fn update_last_ping_instant(&mut self) {
            self.last_ping_instant = Instant::now();
        }
    }

    #[test]
    fn test_ping_not_needed_when_interval_not_elapsed() {
        // Arrange: Set up a pinger with a long interval and a recent last_ping time.
        let mut pinger = MockPinger {
            database_ping_interval: Duration::from_secs(10),
            last_ping_instant: Instant::now(),
        };
        let mut pingable = MockPingable::new();

        // Act: Check if a ping is needed.
        pinger.check_timing_and_ping_database(&mut pingable);

        // Assert: The pingable object's ping method should NOT have been called.
        assert!(
            !pingable.ping_was_called,
            "Ping should not have been called as the interval has not elapsed."
        );
    }

    #[test]
    fn test_ping_is_needed_when_interval_has_elapsed() {
        // Arrange: Set up a pinger with a short interval and a last_ping time in the past.
        let interval = Duration::from_millis(10);
        let mut pinger = MockPinger {
            database_ping_interval: interval,
            last_ping_instant: Instant::now() - (interval + Duration::from_millis(5)),
        };
        let mut pingable = MockPingable::new();

        // Act: Check if a ping is needed.
        pinger.check_timing_and_ping_database(&mut pingable);

        // Assert: The pingable object's ping method SHOULD have been called.
        assert!(
            pingable.ping_was_called,
            "Ping should have been called because the interval has elapsed."
        );
    }

    #[test]
    fn test_ping_timer_is_reset_after_pinging() {
        // Arrange: Set up a pinger where the interval has elapsed.
        let interval = Duration::from_millis(10);
        let mut pinger = MockPinger {
            database_ping_interval: interval,
            last_ping_instant: Instant::now() - (interval + Duration::from_millis(5)),
        };
        let mut pingable = MockPingable::new();

        // Act 1: The first check should trigger a ping.
        pinger.check_timing_and_ping_database(&mut pingable);

        // Assert 1: The ping was triggered.
        assert!(
            pingable.ping_was_called,
            "Ping should have been called the first time."
        );

        // Arrange 2: Reset the mock's flag for the next check.
        pingable.ping_was_called = false;

        // Act 2: Immediately check again. The timer should have been reset, so no ping is needed.
        pinger.check_timing_and_ping_database(&mut pingable);

        // Assert 2: The ping was NOT triggered a second time.
        assert!(
            !pingable.ping_was_called,
            "Ping should not be called again immediately, as the timer should have been reset."
        );
    }
}