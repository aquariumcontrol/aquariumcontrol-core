/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Provides the concrete implementation for transferring heating statistics to a SQL database.
//!
//! This module acts as a bridge, fulfilling the `HeatingStatsDataTransferTrait` contract
//! by using the `SqlInterfaceHeatingStats` component. Its primary purpose is to decouple
//! the core heating logic from the specific details of the database implementation, adhering
//! to the Dependency Inversion Principle.
//!
//! ## Key Components
//!
//! - **`HeatingStatsDataTransfer`**: A simple unit struct that serves as the concrete
//!   implementation of the `HeatingStatsDataTransferTrait`. It contains no data itself;
//!   its sole role is to provide the `transfer_heating_stats` method.
//!
//! ## Design and Purpose
//!
//! In a larger system, a high-level component (like a thermal controller) needs to persist
//! statistics but should not be tightly coupled to the database code. Instead, it depends
//! on an abstraction—the `HeatingStatsDataTransferTrait`.
//!
//! This module provides the "glue" that connects the abstraction to the concrete
//! `SqlInterfaceHeatingStats` implementation allowing usage of mock-implementation of the trait
//! to verify that statistics are being generated correctly without needing a real database
//! connection.

use crate::database::sql_interface_error::SqlInterfaceError;
use crate::database::sql_interface_heating_stats::{HeatingStatsEntry, SqlInterfaceHeatingStats};
use crate::thermal::heating::HeatingStatsDataTransferTrait;

/// Implements the `HeatingStatsDataTransferTrait` for transferring heating statistics to the SQL database.
///
/// This struct serves as the concrete component for persisting daily heating statistics
/// into the configured SQL database.
pub struct HeatingStatsDataTransfer;

impl HeatingStatsDataTransferTrait for HeatingStatsDataTransfer {
    /// Persists a daily heating statistics entry to the SQL database.
    ///
    /// This implementation calls the underlying `insert_heating_stats_entry` method
    /// to write the provided data. The operation is typically an "upsert" (insert or update).
    ///
    /// # Arguments
    /// * `heating_stats_entry` - The `HeatingStatsEntry` containing the complete statistics for a single day.
    /// * `sql_interface_heating` - A mutable reference to the `SqlInterfaceHeatingStats` instance,
    ///   used to perform the database write operation.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the data was successfully inserted or updated in the database.
    ///
    /// # Errors
    /// This function will return an `Err` variant of `SqlInterfaceError` if the underlying
    /// database operation fails. This is typically `SqlInterfaceError::InsertHeatingStatsEntryFailure`,
    /// which can be caused by:
    /// - A lost or broken database connection.
    /// - A violation of database constraints (e.g., a malformed date that violates the primary key).
    /// - Insufficient permissions for the database user to perform `INSERT` or `UPDATE` operations.
    /// - A malformed SQL query, which would indicate an internal logic error.
    fn transfer_heating_stats(
        &mut self,
        heating_stats_entry: HeatingStatsEntry,
        sql_interface_heating: &mut SqlInterfaceHeatingStats,
    ) -> Result<(), SqlInterfaceError> {
        sql_interface_heating.insert_heating_stats_entry(&heating_stats_entry)
    }
}
