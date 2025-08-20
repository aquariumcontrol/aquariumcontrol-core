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
