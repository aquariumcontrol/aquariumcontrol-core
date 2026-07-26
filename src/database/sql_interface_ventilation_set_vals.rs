/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages database interactions for the ventilation system's user-configurable set points.
//!
//! This module provides a dedicated interface, `SqlInterfaceVentilationSetVals`, for handling
//! all SQL operations related to the `ventilationsetvals` table. It encapsulates logic
//! for reading the ventilation on/off temperature thresholds and implements the
//! `ThermalSetValueUpdaterTrait` to periodically poll the database for changes.
//!
//! ## Key Components
//!
//! - **`SqlInterfaceVentilationSetVals` Struct**: The primary struct that holds a database
//!   connection and manages the periodic update logic.
//!
//! - **`VentilationSetVals` Struct**: A simple data structure representing the on/off
//!   temperature thresholds for the ventilation fans.
//!
//! - **`new()` Constructor**: A critical entry point that performs "fail-fast"
//!   validation at initialization. It checks the database to ensure:
//!   - The `ventilationsetvals` table contains no `NULL` values.
//!   - The table contains at most one row, as there should only be a single set of
//!     ventilation parameters for the system.
//!
//! - **`get_ventilation_set_values()`**: The core data-fetching method. It retrieves the
//!   ventilation thresholds and performs a critical consistency check to ensure the
//!   `ventilation_switch_on_temperature` is not lower than the `ventilation_switch_off_temperature`.
//!
//! - **`ThermalSetValueUpdaterTrait` Implementation**: The `update_set_value` method
//!   allows this struct to be used by a thermal controller to periodically and
//!   efficiently check for updated set points from the database without querying on
//!   every single loop iteration.
//!
//! ## Design and Purpose
//!
//! The main goal of this module is to provide a robust, encapsulated, and safe
//! interface for the ventilation subsystem's configuration data.
//!
//! - **Encapsulation**: All SQL queries and logic specific to ventilation set points are
//!   contained within this module, separating concerns.
//!
//! - **Data Integrity**: The constructor's validation logic and the consistency checks
//!   in `get_ventilation_set_values` enforce data integrity rules, preventing runtime
//!   errors caused by invalid database states.

use mysql::{params, PooledConn};
use std::time::{Duration, Instant};

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_error::SqlInterfaceError;
use crate::database::sql_query_strings;
use crate::database::sql_query_strings::{
    SQL_QUERY_CHECK_VENTILATION_SET_VALS_COUNT, SQL_QUERY_CHECK_VENTILATION_SET_VALS_NULL,
};
use crate::database::sql_test_query_strings::SQL_QUERY_WRITE_VENTILATION_SET_VALS;
use crate::database::thermal_set_value_updater_trait::ThermalSetValueUpdaterTrait;
use crate::thermal::ventilation_config::VentilationConfig;
use mysql::prelude::Queryable;

/// Contains the ventilation set values which can be set by the user.
#[derive(PartialEq, Debug)]
pub struct VentilationSetVals {
    /// the temperature at which the ventilation should be switched on 100%
    pub ventilation_switch_on_temperature: f32,

    /// the temperature at which the ventilation should be switched off 0%
    pub ventilation_switch_off_temperature: f32,
}

#[derive(Debug)]
pub struct SqlInterfaceVentilationSetVals {
    /// Connection to the database
    pub conn: PooledConn,

    /// recording of time when the database has been polled for the last time
    pub update_time: Option<Instant>,

    /// target update duration
    pub update_duration: Duration,
}

impl SqlInterfaceVentilationSetVals {
    /// Creates a new `SqlInterfaceVentilationSetVals` instance.
    ///
    /// This constructor initializes the SQL interface for ventilation set values. It performs
    /// several pre-flight checks to ensure data integrity, such as verifying that the
    /// `ventilation_set_vals` table contains no NULL values and has at most one row.
    ///
    /// # Arguments
    /// * `conn` - An active, pooled database connection.
    /// * `config` - A reference to the ventilation configuration to determine the update interval.
    ///
    /// # Returns
    /// A `Result` containing a new `SqlInterfaceVentilationSetVals` instance on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - Any of the initial database queries to get table counts fail (`DatabaseCheckVentilationSetValsFailure`).
    /// - Any of the retrieved counts are negative, indicating a database issue (`DatabaseVentilationSetValTableNegativeValue`).
    /// - The `ventilation_set_vals` table contains entries with `NULL` values (`DatabaseVentilationSetValTableContainsNull`).
    /// - The `ventilation_set_vals` table contains more than one row (`DatabaseVentilationSetValTableContainsTooManyRows`).
    pub fn new(
        mut conn: PooledConn,
        config: &VentilationConfig,
    ) -> Result<Self, SqlInterfaceError> {
        let count_null_values = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_VENTILATION_SET_VALS_NULL,
        )
        .map_err(
            |e| SqlInterfaceError::DatabaseCheckVentilationSetValsFailure {
                location: module_path!().to_string(),
                source: Box::new(e),
            },
        )?;

        let count_rows = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_VENTILATION_SET_VALS_COUNT,
        )
        .map_err(
            |e| SqlInterfaceError::DatabaseCheckVentilationSetValsFailure {
                location: module_path!().to_string(),
                source: Box::new(e),
            },
        )?;

        // check the query results
        if count_null_values < 0 || count_rows < 0 {
            return Err(
                SqlInterfaceError::DatabaseVentilationSetValTableNegativeValue(
                    module_path!().to_string(),
                    count_null_values,
                    count_rows,
                ),
            );
        }

        if count_null_values > 0 {
            return Err(
                SqlInterfaceError::DatabaseVentilationSetValTableContainsNull(
                    module_path!().to_string(),
                    count_null_values,
                ),
            );
        }

        if count_rows > 1 {
            return Err(
                SqlInterfaceError::DatabaseVentilationSetValTableContainsTooManyRows(
                    module_path!().to_string(),
                    count_rows.cast_unsigned(),
                ),
            );
        }

        Ok(SqlInterfaceVentilationSetVals {
            conn,
            update_time: None,
            update_duration: Duration::from_secs(config.set_value_check_interval),
        })
    }

    #[allow(non_snake_case)]
    /// Retrieves the ventilation set point values (switch-on and switch-off temperatures) from the database.
    ///
    /// This function queries the database for the configured ventilation thresholds. It expects
    /// either no entries (if values haven't been set yet) or exactly one entry.
    ///
    /// It performs the following checks:
    /// 1. **Query Execution: ** Attempts to fetch ventilation set values from the database using
    ///    `SQL_QUERY_READ_VENTILATION_STATS`.
    /// 2. **Empty Result: ** If the query returns no entries, it indicates that the set points
    ///    have not yet been configured in the database, and `Ok(None)` is returned.
    /// 3. **Multiple Results: ** If the query returns more than one entry, it signifies a data
    ///    inconsistency, and an `Err` is returned, as only a single set of ventilation values
    ///    is expected.
    /// 4. **Consistency Check: ** Validates that the `ventilation_switch_on_temperature` is not
    ///    greater than the `ventilation_switch_off_temperature`. If this crucial control
    ///    logic consistency is violated, an `Err` is returned.
    ///
    /// # Returns
    /// - `Ok(Some(VentilationSetVals))`: If a single, consistent set of ventilation set values
    ///   is successfully retrieved.
    /// - `Ok(None)`: If the database query returns no entries, indicating that values
    ///   have not yet been configured.
    /// - `Err(SqlInterfaceError::VentilationSetValsReadError)`: If there's an error during
    ///   database interaction, if multiple entries are found, or if the retrieved values
    ///   fail the consistency check (e.g., `switch_on_temperature` is higher than `switch_off_temperature`).
    ///
    /// # Errors
    /// This function returns an `SqlInterfaceError::VentilationSetValsReadError` in the following cases:
    /// - The underlying database query fails.
    /// - More than one `VentilationSetVals` entry is found in the database.
    /// - The `ventilation_switch_on_temperature` is found to be greater than the
    ///   `ventilation_switch_off_temperature`, which is a critical configuration inconsistency.
    /// [impl->swreq~thermal_read_ventilation_limits~1]
    pub fn get_ventilation_set_values(
        &mut self,
    ) -> Result<Option<VentilationSetVals>, SqlInterfaceError> {
        let sql_ventilation_setvals_array = self
            .conn
            .query_map(
                sql_query_strings::SQL_QUERY_READ_VENTILATION_SET_VALS,
                |(ventilationSwitchOffTemp, ventilationSwitchOnTemp)| VentilationSetVals {
                    ventilation_switch_off_temperature: ventilationSwitchOffTemp,
                    ventilation_switch_on_temperature: ventilationSwitchOnTemp,
                },
            )
            .map_err(|e| SqlInterfaceError::VentilationSetValsRequestFailure {
                location: module_path!().to_string(),
                query: sql_query_strings::SQL_QUERY_READ_VENTILATION_SET_VALS.to_string(),
                source: e,
            })?;

        // return none if the query result is empty (user may have not set any values yet)
        if sql_ventilation_setvals_array.is_empty() {
            return Ok(None);
        }

        // return error if the query result contains more than one entry
        if sql_ventilation_setvals_array.len() > 1 {
            return Err(SqlInterfaceError::VentilationSetValsNoSingleResponse(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_READ_VENTILATION_SET_VALS.to_string(),
            ));
        }

        // get the first element of the array which has size one
        let ventilation_set_vals = &sql_ventilation_setvals_array[0];

        // consistency check
        if ventilation_set_vals.ventilation_switch_on_temperature
            < ventilation_set_vals.ventilation_switch_off_temperature
        {
            return Err(SqlInterfaceError::VentilationSetValsInvalid(
                module_path!().to_string(),
                ventilation_set_vals.ventilation_switch_on_temperature,
                ventilation_set_vals.ventilation_switch_off_temperature,
            ));
        }

        Ok(Some(VentilationSetVals {
            ventilation_switch_off_temperature: ventilation_set_vals
                .ventilation_switch_off_temperature,
            ventilation_switch_on_temperature: ventilation_set_vals
                .ventilation_switch_on_temperature,
        }))
    }

    /// Inserts a set of ventilation values into the database for testing.
    ///
    /// This helper function is used exclusively in test environments to
    /// pre-populate the `ventilationsetvals` table with specific data.
    ///
    /// # Arguments
    /// * `ventilation_set_vals` - A reference to the `VentilationSetVals` to be inserted.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the values were successfully inserted.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::InsertVentilationSetValuesFailure` if the `INSERT`
    /// query fails. This error should be updated to include the source `mysql::Error`
    /// for better diagnostics in test failures.
    pub fn insert_ventilation_set_values(
        &mut self,
        ventilation_set_vals: &VentilationSetVals,
    ) -> Result<(), SqlInterfaceError> {
        match self.conn.exec_drop::<_, _>(
            SQL_QUERY_WRITE_VENTILATION_SET_VALS, params!{
                "ventilation_switch_off_temp" => ventilation_set_vals.ventilation_switch_off_temperature,
                "ventilation_switch_on_temp" => ventilation_set_vals.ventilation_switch_on_temperature,
             },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("{e:?}");
                Err(SqlInterfaceError::InsertVentilationSetValuesFailure {
                    location: module_path!().to_string(),
                    query: SQL_QUERY_WRITE_VENTILATION_SET_VALS.to_string(),
                    source: e,
                })
            }
        }
    }
}

impl ThermalSetValueUpdaterTrait for SqlInterfaceVentilationSetVals {
    /// Periodically updates ventilation set point values from the database.
    ///
    /// This method checks if a configured interval (`update_duration`) has passed since
    /// the last database check. If it has, it queries the database for the latest
    /// `VentilationSetVals` and updates the provided mutable references. If no values are
    /// found in the database, the existing values are kept unchanged.
    ///
    /// # Arguments
    /// * `ventilation_switch_off_temperature` - A mutable reference to the current switch-off temperature.
    /// * `ventilation_switch_on_temperature` - A mutable reference to the current switch-on temperature.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) on success. Success includes cases where the update
    /// was skipped due to the time interval or when no new values were found in the database.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::VentilationSetValsUpdateFailure` if the underlying call to
    /// `get_ventilation_set_values` fails. This can happen due to a database connection issue,
    /// data inconsistency (e.g., multiple rows), or invalid set values.
    fn update_set_value(
        &mut self,
        ventilation_switch_off_temperature: &mut f32,
        ventilation_switch_on_temperature: &mut f32,
    ) -> Result<(), SqlInterfaceError> {
        if let Some(last_update) = self.update_time {
            if last_update.elapsed() < self.update_duration {
                // Not enough time has passed, so skip the update.
                return Ok(());
            }
        }

        // Either enough time has passed or this is the first update.
        // Update the time before reading the values from the database.
        self.update_time = Some(Instant::now());

        match self.get_ventilation_set_values() {
            Ok(Some(ventilation_set_vals)) => {
                *ventilation_switch_off_temperature =
                    ventilation_set_vals.ventilation_switch_off_temperature;
                *ventilation_switch_on_temperature =
                    ventilation_set_vals.ventilation_switch_on_temperature;
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(e) => Err(SqlInterfaceError::VentilationSetValsUpdateFailure {
                location: module_path!().to_string(),
                source: Box::new(e),
            }),
        }
    }
}
