use crate::database::sql_interface_heating_stats::{HeatingStatsEntry, SqlInterfaceHeating};
use crate::thermal::heating::HeatingStatsDataTransferTrait;
use log::error;

const LOG_TARGET_HEATING_STATS_DATA_TRANSFER: &str = "HeatingStatsDataTransfer";

/// Implements the `HeatingStatsDataTransferTrait` for transferring heating statistics to the SQL database.
///
/// This struct serves as the concrete component for persisting daily heating statistics
/// into the configured SQL database.
pub struct HeatingStatsDataTransfer;

impl HeatingStatsDataTransferTrait for HeatingStatsDataTransfer {
    /// The function transfers the provided daily heating statistics entry to the SQL database.
    ///
    /// This implementation attempts to insert or update the heating stats record in the database.
    /// Errors during database operations are logged,
    /// but the function proceeds without panicking to avoid disrupting the main application flow.
    ///
    /// # Arguments
    /// * `heating_stats_entry` - The **`HeatingStatsEntry`** containing the statistics for the day.
    ///   This is passed by value, meaning ownership is transferred to this function.
    /// * `sql_interface_heating` - A mutable reference to the **`SqlInterfaceHeating`** instance,
    ///   used for database interaction.
    fn transfer_heating_stats(
        &mut self,
        heating_stats_entry: HeatingStatsEntry,
        sql_interface_heating: &mut SqlInterfaceHeating,
    ) {
        match sql_interface_heating.insert_heating_stats_entry(&heating_stats_entry) {
            Ok(_) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_HEATING_STATS_DATA_TRANSFER,
                    "could not insert heating statistics into database: {e:?}"
                );
            }
        };
    }
}
