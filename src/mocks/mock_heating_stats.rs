/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

pub mod tests {
    use crate::database::sql_interface_error::SqlInterfaceError;
    use crate::database::sql_interface_heating_stats::{
        HeatingStatsEntry, SqlInterfaceHeatingStats,
    };
    use crate::database::sql_interface_midnight::SqlInterfaceMidnightCalculatorTrait;
    use crate::thermal::heating::HeatingStatsDataTransferTrait;
    use mysql::PooledConn;

    pub struct HeatingStatsMockDataTransfer {
        pub heating_stats_entry_previous: HeatingStatsEntry,
        pub heating_stats_entry: HeatingStatsEntry,
        pub transfer_counter: u32,
        pub last_transferred_heating_stats_entry: Option<HeatingStatsEntry>,
    }

    impl HeatingStatsMockDataTransfer {
        // Creates a new `HeatingStatsMockDataTransfer` instance.
        //
        // This constructor initializes the mock data transfer object for testing heating statistics.
        // It sets up two `HeatingStatsEntry` instances, both initialized with default (zero) values
        // for the current date, to simulate the previous and current day's statistics.
        //
        // # Returns
        // A new `HeatingStatsMockDataTransfer` struct.
        /// Creates a new instance.
        ///
        /// # Arguments
        /// * `conn` - An active pooled database connection.
        pub fn new(conn: &mut PooledConn) -> HeatingStatsMockDataTransfer {
            Self {
                heating_stats_entry_previous: HeatingStatsEntry::new(conn),
                heating_stats_entry: HeatingStatsEntry::new(conn),
                transfer_counter: 0,
                last_transferred_heating_stats_entry: None,
            }
        }
    }

    impl HeatingStatsDataTransferTrait for HeatingStatsMockDataTransfer {
        // Transfer function emulates updating SQL database with daily heating statistics.
        //
        // This implementation replaces the current value with a new one and stores the old
        // one in a "previous" variable.
        //
        // # Arguments
        // * `heating_stats_entry` - The `HeatingStatsEntry` containing the statistics for the day.
        //   This is passed by value, meaning ownership is transferred to this function.
        // * `_sql_interface_heating` - A mutable reference to the `SqlInterfaceHeating` instance,
        //   not used for this mock function
        /// Performs the `transfer_heating_stats` operation.
        ///
        /// # Arguments
        /// * `heating_stats_entry` - A parameter representing `heating_stats_entry`.
        /// * `sql_interface_heating` - Database interface connection for heating stats.
        fn transfer_heating_stats(
            &mut self,
            heating_stats_entry: HeatingStatsEntry,
            _sql_interface_heating: &mut SqlInterfaceHeatingStats,
        ) -> Result<(), SqlInterfaceError> {
            self.heating_stats_entry_previous = self.heating_stats_entry.clone();
            self.heating_stats_entry = heating_stats_entry.clone();
            self.last_transferred_heating_stats_entry = Some(heating_stats_entry);
            println!("Heating: Received command to transfer mock data to SQL database:");
            println!("=== Previous ===\n {}", self.heating_stats_entry_previous);
            println!("=== Current ===\n {}", self.heating_stats_entry);
            self.transfer_counter += 1;
            Ok(())
        }
    }

    pub struct SqlInterfaceHeatingMockMidnightCalculator {
        seconds_to_midnight: i64,
    }

    impl SqlInterfaceHeatingMockMidnightCalculator {
        // Creates a new `SqlInterfaceHeatingMockMidnightCalculator` instance.
        //
        // This constructor initializes the mock calculator, setting the specific
        // number of seconds until midnight that it will consistently return during tests.
        // This allows for predictable simulation of time progression in testing scenarios.
        //
        // # Arguments
        // * `seconds_to_midnight` - The fixed `i64` value representing the number of
        //   seconds until midnight that this mock will provide.
        //
        // # Returns
        // A new `SqlInterfaceHeatingMockMidnightCalculator` struct.
        /// Creates a new instance.
        ///
        /// # Arguments
        /// * `seconds_to_midnight` - A parameter representing `seconds_to_midnight`.
        pub fn new(seconds_to_midnight: i64) -> SqlInterfaceHeatingMockMidnightCalculator {
            Self {
                seconds_to_midnight,
            }
        }
    }

    impl SqlInterfaceMidnightCalculatorTrait for SqlInterfaceHeatingMockMidnightCalculator {
        // Provides a predefined duration until midnight for testing heating statistics transfer.
        //
        // This mock implementation of `get_duration_until_midnight` always returns a
        // fixed `i64` value, `self.seconds_to_midnight`. This allows tests to simulate
        // various durations until midnight without actually waiting for real-world time to pass,
        // enabling deterministic and fast testing of the heating statistics transfer logic.
        //
        // # Returns
        // An `Ok(i64)` containing the predefined number of seconds until midnight.
        // This function will always succeed (`Ok`) as it does not interact with a real database.
        /// Retrieves the duration until midnight.
        fn get_duration_until_midnight(&mut self) -> Result<i64, SqlInterfaceError> {
            Ok(self.seconds_to_midnight) // seconds to midnight after which heating stats are inserted into the database
                                         // for testing, we do not want to wait until midnight
        }
    }
}
