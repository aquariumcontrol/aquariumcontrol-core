/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages database interactions for the heating system's user-configurable set points.
//!
//! This module provides a dedicated interface, `SqlInterfaceHeatingSetVals`, for handling
//! all SQL operations related to the `heatingsetvals` table. It encapsulates logic for
//! reading the heating on/off temperature thresholds and implements the
//! `ThermalSetValueUpdaterTrait` to periodically poll the database for changes.
//!
//! ## Key Components
//!
//! - **`SqlInterfaceHeatingSetVals` Struct**: The primary struct that holds a database
//!   connection and manages the periodic update logic.
//!
//! - **`HeatingSetVals` Struct**: A simple data structure representing the on/off
//!   temperature thresholds for the heater.
//!
//! - **`new()` Constructor**: A critical entry point that performs "fail-fast"
//!   validation at initialization. It checks the database to ensure:
//!   - The `heatingsetvals` table contains no `NULL` values.
//!   - The table contains at most one row, as there should only be a single set of
//!     heating parameters for the system.
//!
//! - **`get_heating_set_values()`**: The core data-fetching method. It retrieves the
//!   heating thresholds and performs a critical consistency check to ensure the
//!   `heating_switch_on_temperature` is not greater than the `heating_switch_off_temperature`.
//!
//! - **`ThermalSetValueUpdaterTrait` Implementation**: The `update_set_value` method
//!   allows this struct to be used by a thermal controller to periodically and
//!   efficiently check for updated set points from the database without querying on
//!   every single loop iteration.
//!
//! ## Design and Purpose
//!
//! The main goal of this module is to provide a robust, encapsulated, and safe
//! interface for the heating subsystem's configuration data.
//!
//! - **Encapsulation**: All SQL queries and logic specific to heating set points are
//!   contained within this module, separating concerns.
//!
//! - **Data Integrity**: The constructor's validation logic and the consistency checks
//!   in `get_heating_set_values` enforce data integrity rules, preventing runtime
//!   errors caused by invalid database states.

use mysql::params;
use mysql::prelude::Queryable;
use mysql::PooledConn;
use std::time::{Duration, Instant};

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_error::SqlInterfaceError;
use crate::database::sql_query_strings;
use crate::database::sql_query_strings::{
    SQL_QUERY_CHECK_HEATING_SET_VALS_COUNT, SQL_QUERY_CHECK_HEATING_SET_VALS_NULL,
};
use crate::database::sql_test_query_strings::SQL_QUERY_WRITE_HEATING_SET_VALS;
use crate::database::thermal_set_value_updater_trait::ThermalSetValueUpdaterTrait;
use crate::thermal::heating_config::HeatingConfig;

/// Contains the heating set values which can be set by the user.
#[derive(PartialEq, Debug)]
pub struct HeatingSetVals {
    /// the temperature at which the heating should be switched on 100%
    pub heating_switch_on_temperature: f32,

    /// the temperature at which the heating should be switched off 0%
    pub heating_switch_off_temperature: f32,
}

#[derive(Debug)]
pub struct SqlInterfaceHeatingSetVals {
    /// Connection to the database
    pub conn: PooledConn,

    /// recording of time when the database has been polled for the last time
    pub update_time: Option<Instant>,

    /// target update duration
    pub update_duration: Duration,
}

impl SqlInterfaceHeatingSetVals {
    /// Creates a new `SqlInterfaceHeatingSetVals` instance.
    ///
    /// This constructor initializes the SQL interface for heating set values. It performs
    /// several pre-flight checks to ensure data integrity, such as verifying that the
    /// `heatingsetvals` table contains no NULL values and has at most one row.
    ///
    /// # Arguments
    /// * `conn` - An active, pooled database connection.
    /// * `config` - A reference to the heating configuration to determine the update interval.
    ///
    /// # Returns
    /// A `Result` containing a new `SqlInterfaceHeatingSetVals` instance on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - Any of the initial database queries to get table counts fail (`DatabaseCheckHeatingSetValsFailure`).
    /// - Any of the retrieved counts are negative, indicating a database issue (`DatabaseHeatingSetValTableNegativeValue`).
    /// - The `heatingsetvals` table contains entries with `NULL` values (`DatabaseHeatingSetValTableContainsNull`).
    /// - The `heatingsetvals` table contains more than one row (`DatabaseHeatingSetValTableContainsTooManyRows`).
    pub fn new(
        mut conn: PooledConn,
        config: &HeatingConfig,
    ) -> Result<SqlInterfaceHeatingSetVals, SqlInterfaceError> {
        let count_null_values = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_HEATING_SET_VALS_NULL,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckHeatingSetValsFailure {
            location: module_path!().to_string(),
            source: Box::new(e),
        })?;
        let count_rows = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_HEATING_SET_VALS_COUNT,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckHeatingSetValsFailure {
            location: module_path!().to_string(),
            source: Box::new(e),
        })?;

        // check the query results
        if count_null_values < 0 || count_rows < 0 {
            return Err(SqlInterfaceError::DatabaseHeatingSetValTableNegativeValue(
                module_path!().to_string(),
                count_null_values,
                count_rows,
            ));
        }

        if count_null_values > 0 {
            return Err(SqlInterfaceError::DatabaseHeatingSetValTableContainsNull(
                module_path!().to_string(),
                count_null_values,
            ));
        }

        if count_rows > 1 {
            return Err(
                SqlInterfaceError::DatabaseHeatingSetValTableContainsTooManyRows(
                    module_path!().to_string(),
                    count_rows.cast_unsigned(),
                ),
            );
        }

        Ok(SqlInterfaceHeatingSetVals {
            conn,
            update_time: None,
            update_duration: Duration::from_secs(config.set_value_check_interval),
        })
    }

    #[allow(non_snake_case)]
    /// Retrieves the heating set point values (switch-on and switch-off temperatures) from the database.
    ///
    /// This function queries the database for the configured heating thresholds. It expects
    /// either no entries (if values haven't been set yet) or exactly one entry.
    ///
    /// It performs the following checks:
    /// 1. **Query Execution: ** Attempts to fetch heating set values from the database using
    ///    `SQL_QUERY_READ_HEATING_STATS`.
    /// 2. **Empty Result: ** If the query returns no entries, it indicates that the set points
    ///    have not yet been configured in the database, and `Ok(None)` is returned.
    /// 3. **Multiple Results: ** If the query returns more than one entry, it signifies a data
    ///    inconsistency, and an `Err` is returned, as only a single set of heating values
    ///    is expected.
    /// 4. **Consistency Check: ** Validates that the `heating_switch_on_temperature` is not
    ///    greater than the `heating_switch_off_temperature`. If this crucial control
    ///    logic consistency is violated, an `Err` is returned.
    ///
    /// # Returns
    /// A `Result` containing an `Option<HeatingSetVals>`:
    /// - `Ok(Some(HeatingSetVals))`: If a single, consistent set of heating values is found.
    /// - `Ok(None)`: If the query returns no rows, indicating values have not been set.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The underlying database query fails. This will be a `HeatingSetValsRequestFailure`
    ///   that should be updated to include the source `mysql::Error`.
    /// - The query returns more than one row, indicating a data inconsistency (`HeatingSetValsNoSingleResponse`).
    /// - The retrieved `heating_switch_on_temperature` is bigger than the `heating_switch_off_temperature`,
    ///   which is a critical configuration error (`HeatingSetValsInvalid`).
    pub fn get_heating_set_values(&mut self) -> Result<Option<HeatingSetVals>, SqlInterfaceError> {
        let sql_heating_setvals_array = match self.conn.query_map(
            sql_query_strings::SQL_QUERY_READ_HEATING_SET_VALS,
            |(heatingSwitchOffTemp, heatingSwitchOnTemp)| HeatingSetVals {
                heating_switch_off_temperature: heatingSwitchOffTemp,
                heating_switch_on_temperature: heatingSwitchOnTemp,
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                return Err(SqlInterfaceError::HeatingSetValsRequestFailure {
                    location: module_path!().to_string(),
                    query: sql_query_strings::SQL_QUERY_READ_HEATING_SET_VALS.to_string(),
                    source: e,
                });
            }
        };

        // return none if the query result is empty (user may have not set any values yet)
        if sql_heating_setvals_array.is_empty() {
            return Ok(None);
        }

        // return error if the query result contains more than one entry
        if sql_heating_setvals_array.len() > 1 {
            return Err(SqlInterfaceError::HeatingSetValsNoSingleResponse(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_READ_HEATING_SET_VALS.to_string(),
            ));
        }

        // get the first element of the array which has size one
        let heating_set_vals = &sql_heating_setvals_array[0];

        // consistency check
        if heating_set_vals.heating_switch_on_temperature
            > heating_set_vals.heating_switch_off_temperature
        {
            return Err(SqlInterfaceError::HeatingSetValsInvalid(
                module_path!().to_string(),
                heating_set_vals.heating_switch_on_temperature,
                heating_set_vals.heating_switch_off_temperature,
            ));
        }

        Ok(Some(HeatingSetVals {
            heating_switch_off_temperature: heating_set_vals.heating_switch_off_temperature,
            heating_switch_on_temperature: heating_set_vals.heating_switch_on_temperature,
        }))
    }

    /// Inserts a set of heating values into the database for testing.
    ///
    /// This helper function is used exclusively in test environments to
    /// pre-populate the `heatingsetvals` table with specific data.
    ///
    /// # Arguments
    /// * `heating_set_vals` - A reference to the `HeatingSetVals` to be inserted.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the values were successfully inserted.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::InsertHeatingSetValuesFailure` if the `INSERT`
    /// query fails. This error should be updated to include the source `mysql::Error`
    /// for better diagnostics in test failures.
    pub fn insert_heating_set_values(
        &mut self,
        heating_set_vals: &HeatingSetVals,
    ) -> Result<(), SqlInterfaceError> {
        self.conn
            .exec_drop::<_, _>(
                SQL_QUERY_WRITE_HEATING_SET_VALS,
                params! {
                   "heating_switch_off_temp" => heating_set_vals.heating_switch_off_temperature,
                   "heating_switch_on_temp" => heating_set_vals.heating_switch_on_temperature,
                },
            )
            .map_err(|e| SqlInterfaceError::InsertHeatingSetValuesFailure {
                location: module_path!().to_string(),
                query: SQL_QUERY_WRITE_HEATING_SET_VALS.to_string(),
                source: e,
            })?;
        Ok(())
    }
}
impl ThermalSetValueUpdaterTrait for SqlInterfaceHeatingSetVals {
    /// Periodically updates heating set point values from the database.
    ///
    /// This method checks if a configured interval (`update_duration`) has passed since
    /// the last database check. If it has, it queries the database for the latest
    /// `HeatingSetVals` and updates the provided mutable references. If no values are
    /// found in the database, the existing values are kept unchanged.
    ///
    /// # Arguments
    /// * `heating_switch_off_temperature` - A mutable reference to the current switch-off temperature.
    /// * `heating_switch_on_temperature` - A mutable reference to the current switch-on temperature.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) on success. Success includes cases where the update
    /// was skipped due to the time interval or when no new values were found in the database.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::HeatingSetValsUpdateFailure` if the underlying call to
    /// `get_heating_set_values` fails. This can happen due to a database connection issue,
    /// data inconsistency (e.g., multiple rows), or invalid set values (e.g., on-temp > off-temp).
    fn update_set_value(
        &mut self,
        heating_switch_off_temperature: &mut f32,
        heating_switch_on_temperature: &mut f32,
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

        match self.get_heating_set_values() {
            Ok(Some(heating_set_vals)) => {
                *heating_switch_off_temperature = heating_set_vals.heating_switch_off_temperature;
                *heating_switch_on_temperature = heating_set_vals.heating_switch_on_temperature;
                Ok(())
            }
            Ok(None) => {
                Ok(()) // no heating set values found in database - maintain the configured ones
            }
            Err(e) => Err(SqlInterfaceError::HeatingSetValsUpdateFailure {
                location: module_path!().to_string(),
                source: Box::new(e),
            }),
        }
    }
}
