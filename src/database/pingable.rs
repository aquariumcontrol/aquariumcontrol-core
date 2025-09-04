/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_balling::SqlInterfaceBalling;
use crate::database::sql_interface_data::SqlInterfaceData;
use crate::database::sql_interface_feed::SqlInterfaceFeed;
use crate::database::sql_interface_heating_stats::SqlInterfaceHeatingStats;
use crate::database::sql_interface_refill::SqlInterfaceRefill;
use crate::database::sql_interface_schedule::SqlInterfaceSchedule;
use crate::utilities::logger::log_error_chain;

/// Represents a component that can be "pinged" to keep a connection alive.
pub trait Pingable {
    /// Pings the database to check connection health and prevent timeouts.
    ///
    /// This function executes a lightweight query to ensure the connection is still
    /// active. If the ping fails, it logs an error using `log::error!` but does
    /// not propagate the error to the caller. This is a "fire-and-forget"
    /// keep-alive mechanism.
    fn ping(&mut self);
}

impl Pingable for SqlInterfaceData {
    fn ping(&mut self) {
        if let Err(e) = SqlInterface::ping_database(&mut self.conn) {
            log_error_chain(module_path!(), "Encountered error while pinging database.", e);
        }
    }
}

impl Pingable for SqlInterfaceBalling {
    fn ping(&mut self) {
        if let Err(e) = SqlInterface::ping_database(&mut self.conn) {
            log_error_chain(module_path!(), "Encountered error while pinging database.", e);
        }
    }
}

impl Pingable for SqlInterfaceFeed {
    fn ping(&mut self) {
        if let Err(e) = SqlInterface::ping_database(&mut self.conn) {
            log_error_chain(module_path!(), "Encountered error while pinging database.", e);
        }
    }
}

impl Pingable for SqlInterfaceHeatingStats {
    fn ping(&mut self) {
        if let Err(e) = SqlInterface::ping_database(&mut self.conn) {
            log_error_chain(module_path!(), "Encountered error while pinging database.", e);
        }
    }
}

impl Pingable for SqlInterfaceRefill {
    fn ping(&mut self) {
        if let Err(e) = SqlInterface::ping_database(&mut self.conn) {
            log_error_chain(module_path!(), "Encountered error while pinging database.", e);
        }
    }
}

impl Pingable for SqlInterfaceSchedule {
    fn ping(&mut self) {
        if let Err(e) = SqlInterface::ping_database(&mut self.conn) {
            log_error_chain(module_path!(), "Encountered error while pinging database.", e);
        }
    }
}