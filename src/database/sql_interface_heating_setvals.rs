use mysql::PooledConn;
use std::time::{Duration, Instant};

#[cfg(test)]
use mysql::params;

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_error::SqlInterfaceError;
use crate::database::sql_query_strings;
use crate::database::sql_query_strings::{
    SQL_QUERY_CHECK_HEATING_SETVALS_COUNT, SQL_QUERY_CHECK_HEATING_SETVALS_NULL,
};
use crate::thermal::heating::HeatingConfig;
use mysql::prelude::Queryable;

/// Contains the heating set values which can be set by the user.
#[derive(PartialEq, Debug)]
pub struct HeatingSetVals {
    /// the temperature at which the heating should be switched on 100%
    pub heating_switch_on_temperature: f32,

    /// the temperature at which the heating should be switched off 0%
    pub heating_switch_off_temperature: f32,
}

/// Trait for updating the heating set values with values from the database.
/// This trait allows running the main control with a mock implementation for testing.
pub trait HeatingSetValueUpdaterTrait {
    fn update_set_value(
        &mut self,
        heating_switch_off_temperature: &mut f32,
        heating_switch_on_temperature: &mut f32,
    ) -> Result<(), SqlInterfaceError>;
}

pub struct SqlInterfaceHeatingSetVals {
    /// Connection to the database
    pub conn: PooledConn,

    /// recording of time when the database has been polled for the last time
    update_time: Option<Instant>,

    /// target update duration
    update_duration: Duration,
}

impl SqlInterfaceHeatingSetVals {
    /// Creates a new `SqlInterfaceHeatingSetVals` instance.
    ///
    /// This constructor initializes the heating SQL interface for heating set values with a primary
    /// database connection. It also checks for NULL values in the set value table.
    ///
    /// # Arguments
    /// * `conn` - An active database connection pool object for general heating-related operations.
    ///
    /// # Returns
    /// * A new `SqlInterfaceHeatingSetVals` struct in case of success.
    /// * An error `SqlInterfaceError` in case of failure.
    pub fn new(
        mut conn: PooledConn,
        config: &HeatingConfig,
    ) -> Result<SqlInterfaceHeatingSetVals, SqlInterfaceError> {
        let count_null_values = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_HEATING_SETVALS_NULL,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckHeatingSetValsFailure {
            location: module_path!().to_string(),
            source: Box::new(e),
        })?;
        let count_rows = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_HEATING_SETVALS_COUNT,
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
    /// - `Ok(Some(HeatingSetVals))`: If a single, consistent set of heating set values
    ///   is successfully retrieved.
    /// - `Ok(None)`: If the database query returns no entries, indicating that values
    ///   have not yet been configured.
    /// - `Err(SqlInterfaceError::HeatingSetValsReadError)`: If there's an error during
    ///   database interaction, if multiple entries are found, or if the retrieved values
    ///   fail the consistency check (e.g., `switch_on_temperature` is higher than `switch_off_temperature`).
    ///
    /// # Errors
    /// This function returns an `SqlInterfaceError::HeatingSetValsReadError` in the following cases:
    /// - The underlying database query fails.
    /// - More than one `HeatingSetVals` entry is found in the database.
    /// - The `heating_switch_on_temperature` is found to be greater than the
    ///   `heating_switch_off_temperature`, which is a critical configuration inconsistency.
    pub fn get_heating_set_values(
        &mut self,
    ) -> mysql::Result<Option<HeatingSetVals>, SqlInterfaceError> {
        let sql_heating_setvals_array = match self.conn.query_map(
            sql_query_strings::SQL_QUERY_READ_HEATING_SETVALS,
            |(heatingSwitchOffTemp, heatingSwitchOnTemp)| HeatingSetVals {
                heating_switch_off_temperature: heatingSwitchOffTemp,
                heating_switch_on_temperature: heatingSwitchOnTemp,
            },
        ) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::HeatingSetValsRequestFailure(
                    module_path!().to_string(),
                    sql_query_strings::SQL_QUERY_READ_HEATING_SETVALS.to_string(),
                ));
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
                sql_query_strings::SQL_QUERY_READ_HEATING_SETVALS.to_string(),
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

    #[cfg(test)]
    pub fn insert_heating_set_values(
        &mut self,
        heating_set_vals: &HeatingSetVals,
    ) -> mysql::Result<(), SqlInterfaceError> {
        match self.conn.exec_drop::<_, _>(
            sql_query_strings::SQL_QUERY_WRITE_HEATING_SETVALS,
            params! {
               "heating_switch_off_temp" => heating_set_vals.heating_switch_off_temperature,
               "heating_switch_on_temp" => heating_set_vals.heating_switch_on_temperature,
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("{e:?}");
                Err(SqlInterfaceError::InsertHeatingSetValuesFailure(
                    module_path!().to_string(),
                    sql_query_strings::SQL_QUERY_WRITE_HEATING_SETVALS.to_string(),
                ))
            }
        }
    }
}
impl HeatingSetValueUpdaterTrait for SqlInterfaceHeatingSetVals {
    fn update_set_value(
        &mut self,
        heating_switch_off_temperature: &mut f32,
        heating_switch_on_temperature: &mut f32,
    ) -> Result<(), SqlInterfaceError> {
        if self.update_time.is_some()
            && Instant::now().duration_since(self.update_time.unwrap()) < self.update_duration
        {
            // do not update the values if the last update was less than the configured update duration ago
            return Ok(());
        } else {
            // update the time before reading the values from the database
            self.update_time = Some(Instant::now());
        }
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

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface_heating_setvals::{
        HeatingSetVals, SqlInterfaceHeatingSetVals,
    };
    use crate::database::sql_query_strings::SQL_TABLE_HEATING_SETVALS;
    use crate::database::{sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError};
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};

    #[test]
    // Test case includes all checks for this table combined in one function to limit the number of additional databases.
    // Test case uses the test database #36 - focussing on heatingsetvals table only.
    pub fn test_sql_interface_heating_setvals() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            36,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        // *** test reading heating set values **************************************************
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface.clone())
            .expect("Initialization of SQL interface for test failed.");
        match SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_HEATING_SETVALS.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }

        // instantiation of the test object
        let mut sql_interface_heating_setvals =
            SqlInterfaceHeatingSetVals::new(sql_interface.get_connection().unwrap(), &config.heating)
                .unwrap();

        // check if reading an empty table returns None
        match sql_interface_heating_setvals.get_heating_set_values() {
            Ok(c) => {
                assert_eq!(c.is_some(), false);
            }
            Err(_) => {
                panic!("Call to get_heating_set_values failed.");
            }
        }
        println!("* reading empty heating set values succeeded.");

        // define valid value combination
        let valid_heating_set_vals = HeatingSetVals {
            heating_switch_on_temperature: 20.0,
            heating_switch_off_temperature: 25.0,
        };

        // insert valid values
        assert!(matches!(
            sql_interface_heating_setvals.insert_heating_set_values(&valid_heating_set_vals),
            Ok(())
        ));
        println!("* inserting valid heating set values as precondition succeeded.");

        match sql_interface_heating_setvals.get_heating_set_values() {
            Ok(c) => {
                assert_eq!(c.is_some(), true);
                let heating_set_vals = c.unwrap();
                assert_eq!(
                    heating_set_vals.heating_switch_off_temperature,
                    valid_heating_set_vals.heating_switch_off_temperature
                );
                assert_eq!(
                    heating_set_vals.heating_switch_on_temperature,
                    valid_heating_set_vals.heating_switch_on_temperature
                );
            }
            Err(_) => {
                panic!("Call to get_heating_set_values failed.");
            }
        }
        println!("* reading valid heating set values succeeded.");

        match SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_HEATING_SETVALS.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }

        // define invalid value combination
        let invalid_heating_set_vals = HeatingSetVals {
            heating_switch_on_temperature: 26.0,
            heating_switch_off_temperature: 21.0,
        };

        // insert invalid values
        assert!(matches!(
            sql_interface_heating_setvals.insert_heating_set_values(&invalid_heating_set_vals),
            Ok(())
        ));
        println!("* inserting invalid heating set values as precondition succeeded.");

        // check if reading an invalid value combination returns an error
        assert!(matches!(
            sql_interface_heating_setvals.get_heating_set_values(),
            Err(SqlInterfaceError::HeatingSetValsInvalid(_, _, _))
        ));
        println!("* reading invalid heating set values succeeded.");

        // test if too many rows are detected
        match SqlInterface::truncate_table(
            &mut sql_interface,
            SQL_TABLE_HEATING_SETVALS.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        assert!(matches!(
            sql_interface_heating_setvals.insert_heating_set_values(&valid_heating_set_vals),
            Ok(())
        ));
        assert!(matches!(
            sql_interface_heating_setvals.insert_heating_set_values(&valid_heating_set_vals),
            Ok(())
        ));
        let test_result =
            SqlInterfaceHeatingSetVals::new(sql_interface.get_connection().unwrap(), &config.heating);
        assert!(matches!(
            test_result,
            Err(SqlInterfaceError::DatabaseHeatingSetValTableContainsTooManyRows(_, _))
        ));
        println!("* detecting too many rows succeeded.");

        //***************************************************************************************
    }
}
