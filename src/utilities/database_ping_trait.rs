use std::time::{Duration, Instant};
use crate::database::pingable::Pingable;
use crate::food::feed::Feed;
use crate::mineral::balling::Balling;
use crate::permission::schedule_check::ScheduleCheck;
use crate::recorder::data_logger::DataLogger;
use crate::thermal::heating::Heating;
use crate::water::refill::Refill;

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