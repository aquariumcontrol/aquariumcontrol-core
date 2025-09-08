/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use mysql::PooledConn;
use std::time::{Duration, Instant};

#[cfg(test)]
use mysql::params;

use crate::database::thermal_set_value_updater_trait::ThermalSetValueUpdaterTrait;
use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_error::SqlInterfaceError;
use crate::database::sql_query_strings;
use crate::database::sql_query_strings::{
    SQL_QUERY_CHECK_VENTILATION_SETVALS_COUNT, SQL_QUERY_CHECK_VENTILATION_SETVALS_NULL,
};
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

pub struct SqlInterfaceVentilationSetVals {
    /// Connection to the database
    pub conn: PooledConn,

    /// recording of time when the database has been polled for the last time
    update_time: Option<Instant>,

    /// target update duration
    update_duration: Duration,
}

impl SqlInterfaceVentilationSetVals {
    /// Creates a new `SqlInterfaceVentilationSetVals` instance.
    ///
    /// This constructor initializes the SQL interface for ventilation set values. It performs
    /// several pre-flight checks to ensure data integrity, such as verifying that the
    /// `ventilationsetvals` table contains no NULL values and has at most one row.
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
    /// - The `ventilationsetvals` table contains entries with `NULL` values (`DatabaseVentilationSetValTableContainsNull`).
    /// - The `ventilationsetvals` table contains more than one row (`DatabaseVentilationSetValTableContainsTooManyRows`).
    pub fn new(
        mut conn: PooledConn,
        config: &VentilationConfig,
    ) -> Result<Self, SqlInterfaceError> {
        let count_null_values = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_VENTILATION_SETVALS_NULL,
        )
        .map_err(
            |e| SqlInterfaceError::DatabaseCheckVentilationSetValsFailure {
                location: module_path!().to_string(),
                source: Box::new(e),
            },
        )?;

        let count_rows = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_VENTILATION_SETVALS_COUNT,
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
    pub fn get_ventilation_set_values(
        &mut self,
    ) -> Result<Option<VentilationSetVals>, SqlInterfaceError> {
        let sql_ventilation_setvals_array = self
            .conn
            .query_map(
                sql_query_strings::SQL_QUERY_READ_VENTILATION_SETVALS,
                |(ventilationSwitchOffTemp, ventilationSwitchOnTemp)| VentilationSetVals {
                    ventilation_switch_off_temperature: ventilationSwitchOffTemp,
                    ventilation_switch_on_temperature: ventilationSwitchOnTemp,
                },
            )
            .map_err(|e| SqlInterfaceError::VentilationSetValsRequestFailure {
                location: module_path!().to_string(),
                query: sql_query_strings::SQL_QUERY_READ_VENTILATION_SETVALS.to_string(),
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
                sql_query_strings::SQL_QUERY_READ_VENTILATION_SETVALS.to_string(),
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

    #[cfg(test)]
    // Inserts a set of ventilation values into the database for testing.
    //
    // This helper function is used exclusively in test environments to
    // pre-populate the `ventilationsetvals` table with specific data.
    //
    // # Arguments
    // * `ventilation_set_vals` - A reference to the `VentilationSetVals` to be inserted.
    //
    // # Returns
    // An empty `Result` (`Ok(())`) if the values were successfully inserted.
    //
    // # Errors
    // Returns `SqlInterfaceError::InsertVentilationSetValuesFailure` if the `INSERT`
    // query fails. This error should be updated to include the source `mysql::Error`
    // for better diagnostics in test failures.
    pub fn insert_ventilation_set_values(
        &mut self,
        ventilation_set_vals: &VentilationSetVals,
    ) -> Result<(), SqlInterfaceError> {
        match self.conn.exec_drop::<_, _>(
            sql_query_strings::SQL_QUERY_WRITE_VENTILATION_SETVALS, params!{
                "ventilation_switch_off_temp" => ventilation_set_vals.ventilation_switch_off_temperature,
                "ventilation_switch_on_temp" => ventilation_set_vals.ventilation_switch_on_temperature,
             },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("{e:?}");
                Err(SqlInterfaceError::InsertVentilationSetValuesFailure {
                    location: module_path!().to_string(),
                    query: sql_query_strings::SQL_QUERY_WRITE_VENTILATION_SETVALS.to_string(),
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

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface_ventilation_setvals::{
        SqlInterfaceVentilationSetVals, VentilationSetVals,
    };
    use crate::database::sql_query_strings::SQL_TABLE_VENTILATION_SETVALS;
    use crate::database::{sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError};
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};

    #[test]
    // Test case includes all checks for this table combined in one function to limit the number of additional databases.
    // Test case uses the test database #51.
    pub fn test_sql_interface_ventilation_setvals() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            51,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        // *** test reading ventilation set values **************************************************
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        match SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_VENTILATION_SETVALS.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }

        // instantiation of the test object
        let mut sql_interface_ventilation_setvals = SqlInterfaceVentilationSetVals::new(
            sql_interface.get_connection().unwrap(),
            &config.ventilation,
        )
        .unwrap();

        // check if reading an empty table returns None
        match sql_interface_ventilation_setvals.get_ventilation_set_values() {
            Ok(c) => {
                assert_eq!(c.is_some(), false);
            }
            Err(_) => {
                panic!("Call to get_ventilation_set_values failed.");
            }
        }
        println!("* reading empty ventilation set values succeeded.");

        // define valid value combination
        let valid_ventilation_set_vals = VentilationSetVals {
            ventilation_switch_on_temperature: 25.0,
            ventilation_switch_off_temperature: 20.0,
        };

        // insert valid values
        assert!(matches!(
            sql_interface_ventilation_setvals
                .insert_ventilation_set_values(&valid_ventilation_set_vals),
            Ok(())
        ));
        println!("* inserting valid ventilation set values as precondition succeeded.");

        match sql_interface_ventilation_setvals.get_ventilation_set_values() {
            Ok(c) => {
                assert_eq!(c.is_some(), true);
                let ventilation_set_vals = c.unwrap();
                assert_eq!(
                    ventilation_set_vals.ventilation_switch_off_temperature,
                    valid_ventilation_set_vals.ventilation_switch_off_temperature
                );
                assert_eq!(
                    ventilation_set_vals.ventilation_switch_on_temperature,
                    valid_ventilation_set_vals.ventilation_switch_on_temperature
                );
            }
            Err(_) => {
                panic!("Call to get_ventilation_set_values failed.");
            }
        }
        println!("* reading valid ventilation set values succeeded.");

        match SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_VENTILATION_SETVALS.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }

        // define invalid value combination
        let invalid_ventilation_set_vals = VentilationSetVals {
            ventilation_switch_on_temperature: 21.0,
            ventilation_switch_off_temperature: 26.0,
        };

        // insert invalid values
        assert!(matches!(
            sql_interface_ventilation_setvals
                .insert_ventilation_set_values(&invalid_ventilation_set_vals),
            Ok(())
        ));
        println!("* inserting invalid ventilation set values as precondition succeeded.");

        // check if reading an invalid value combination returns an error
        assert!(matches!(
            sql_interface_ventilation_setvals.get_ventilation_set_values(),
            Err(SqlInterfaceError::VentilationSetValsInvalid(_, _, _))
        ));
        println!("* reading  invalid ventilation set values succeeded.");

        // test if too many rows are detected
        match SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_VENTILATION_SETVALS.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        assert!(matches!(
            sql_interface_ventilation_setvals
                .insert_ventilation_set_values(&valid_ventilation_set_vals),
            Ok(())
        ));
        assert!(matches!(
            sql_interface_ventilation_setvals
                .insert_ventilation_set_values(&valid_ventilation_set_vals),
            Ok(())
        ));
        let test_result = SqlInterfaceVentilationSetVals::new(
            sql_interface.get_connection().unwrap(),
            &config.ventilation,
        );
        assert!(matches!(
            test_result,
            Err(SqlInterfaceError::DatabaseVentilationSetValTableContainsTooManyRows(_, _))
        ));
        println!("* detecting too many rows succeeded.");

        //***************************************************************************************
    }
}
