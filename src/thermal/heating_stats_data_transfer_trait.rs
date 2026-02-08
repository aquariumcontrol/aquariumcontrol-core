/* Copyright 2026 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! This module contains `HeatingStatsDataTransferTrait`: A trait used for dependency injection, allowing
//!   the database-writing logic to be mocked during testing.

use crate::database::sql_interface_error::SqlInterfaceError;
use crate::database::sql_interface_heating_stats::{HeatingStatsEntry, SqlInterfaceHeatingStats};

/// Trait for the execution of data transfer to SQL database after midnight has arrived.
/// This trait allows running the main control with a mock implementation for testing.
pub trait HeatingStatsDataTransferTrait {
    /// Transfers a completed daily heating statistics entry to the database.
    ///
    /// This function is called once per day, typically just after midnight, to persist
    /// the aggregated heating statistics (energy usage, average temperatures) for the
    /// previous day.
    ///
    /// # Arguments
    /// * `data` - The `HeatingStatsEntry` struct containing the statistics for the day.
    /// * `sql_interface_heating` - A mutable reference to the `SqlInterfaceHeatingStats` instance,
    ///   used for database interaction.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) on a successful database transaction.
    ///
    /// # Errors
    /// Returns an `SqlInterfaceError` if the database operation fails (e.g., connection
    /// issue, SQL error during insertion/update).
    fn transfer_heating_stats(
        &mut self,
        data: HeatingStatsEntry,
        sql_interface_heating: &mut SqlInterfaceHeatingStats,
    ) -> Result<(), SqlInterfaceError>;
}