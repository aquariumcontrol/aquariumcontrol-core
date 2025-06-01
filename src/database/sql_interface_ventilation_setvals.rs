use log::warn;
use mysql::PooledConn;
use std::time::{Duration, Instant};

#[cfg(test)]
use mysql::params;

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_error::SqlInterfaceError;
use crate::database::sql_query_strings;
use crate::database::sql_query_strings::SQL_QUERY_CHECK_VENTILATIONSETVALS_NULL;
use crate::thermal::ventilation::VentilationConfig;
use mysql::prelude::Queryable;

const LOG_TARGET_SQL_INTERFACE_VENTILATION_SETVALS: &str = "sql_interface_ventilation_setvals";

/// Contains the ventilation set values which can be set by the user.
#[derive(PartialEq, Debug)]
pub struct VentilationSetVals {
    /// the temperature at which the ventilation should be switched on 100%
    pub ventilation_switch_on_temperature: f32,

    /// the temperature at which the ventilation should be switched off 0%
    pub ventilation_switch_off_temperature: f32,
}

/// Trait for updating the ventilation set values with values from the database.
/// This trait allows running the main control with a mock implementation for testing.
pub trait VentilationSetValueUpdaterTrait {
    fn update_set_value(
        &mut self,
        ventilation_switch_off_temperature: &mut f32,
        ventilation_switch_on_temperature: &mut f32,
    );
}

pub struct SqlInterfaceVentilationSetVals {
    /// Connection to the database
    pub conn: PooledConn,

    /// Flag to avoid flooding the log with warnings when the ventilation set values could not be read from the database
    lock_warning_ventilation_set_value_read_failure: bool,

    /// recording of time when the database has been polled for the last time
    update_time: Option<Instant>,

    /// target update duration
    update_duration: Duration,
}

impl SqlInterfaceVentilationSetVals {
    /// Creates a new `SqlInterfaceVentilationSetVals` instance.
    ///
    /// This constructor initializes the ventilation SQL interface for ventilation set values with a primary
    /// database connection. It also checks for NULL values in the set value table.
    ///
    /// # Arguments
    /// * `conn` - An active database connection pool object for general ventilation-related operations.
    ///
    /// # Returns
    /// A new `SqlInterfaceVentilationSetVals` struct.
    pub fn new(mut conn: PooledConn, config: &VentilationConfig) -> SqlInterfaceVentilationSetVals {
        if SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_VENTILATIONSETVALS_NULL,
        )
        .expect("Could not check ventilation set value table for NULL values.")
            > 0
        {
            panic!(
                "{}: Ventilation set value table may be inconsistent - it contains NULL values.",
                LOG_TARGET_SQL_INTERFACE_VENTILATION_SETVALS
            );
        }

        SqlInterfaceVentilationSetVals {
            conn,
            lock_warning_ventilation_set_value_read_failure: false,
            update_time: None,
            update_duration: Duration::from_secs(config.set_value_check_interval),
        }
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
    ) -> mysql::Result<Option<VentilationSetVals>, SqlInterfaceError> {
        let sql_ventilation_setvals_array = match self.conn.query_map(
            sql_query_strings::SQL_QUERY_READ_VENTILATION_SETVALS,
            |(ventilationSwitchOffTemp, ventilationSwitchOnTemp)| VentilationSetVals {
                ventilation_switch_off_temperature: ventilationSwitchOffTemp,
                ventilation_switch_on_temperature: ventilationSwitchOnTemp,
            },
        ) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::VentilationSetValsReadError(
                    "Could not get ventilation set values from the database with query:"
                        .to_string()
                        + sql_query_strings::SQL_QUERY_READ_VENTILATION_SETVALS,
                ));
            }
        };

        // return none if the query result is empty (user may have not set any values yet)
        if sql_ventilation_setvals_array.is_empty() {
            return Ok(None);
        }

        // return error if the query result contains more than one entry
        if sql_ventilation_setvals_array.len() > 1 {
            return Err(SqlInterfaceError::VentilationSetValsReadError(
                "The database returned more than one ventilation set value entry.".to_string(),
            ));
        }

        // get the first element of the array which has size one
        let ventilation_set_vals = &sql_ventilation_setvals_array[0];

        // consistency check
        if ventilation_set_vals.ventilation_switch_on_temperature
            < ventilation_set_vals.ventilation_switch_off_temperature
        {
            return Err(SqlInterfaceError::VentilationSetValsReadError(
                "The ventilation switch on temperature is lower than the ventilation switch off temperature.".to_string(),
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
    pub fn insert_ventilation_set_values(
        &mut self,
        ventilation_set_vals: &VentilationSetVals,
    ) -> mysql::Result<(), SqlInterfaceError> {
        match self.conn.exec_drop::<_, _>(
            sql_query_strings::SQL_QUERY_WRITE_VENTILATION_SETVALS, params!{
                "ventilation_switch_off_temp" => ventilation_set_vals.ventilation_switch_off_temperature,
                "ventilation_switch_on_temp" => ventilation_set_vals.ventilation_switch_on_temperature,
             },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("{e:?}");
                Err(SqlInterfaceError::InsertVentilationSetValuesFailure(
                    "Error when inserting ventilation set values into database using query: ".to_string() +
                        sql_query_strings::SQL_QUERY_WRITE_VENTILATION_SETVALS,
                ))
            }
        }
    }
}
impl VentilationSetValueUpdaterTrait for SqlInterfaceVentilationSetVals {
    fn update_set_value(
        &mut self,
        ventilation_switch_off_temperature: &mut f32,
        ventilation_switch_on_temperature: &mut f32,
    ) {
        if self.update_time.is_some()
            && Instant::now().duration_since(self.update_time.unwrap()) < self.update_duration
        {
            // do not update the values if the last update was less than the configured update duration ago
            return;
        } else {
            // update the time before reading the values from the database
            self.update_time = Some(Instant::now());
        }
        match self.get_ventilation_set_values() {
            Ok(Some(ventilation_set_vals)) => {
                *ventilation_switch_off_temperature =
                    ventilation_set_vals.ventilation_switch_off_temperature;
                *ventilation_switch_on_temperature =
                    ventilation_set_vals.ventilation_switch_on_temperature;
            }
            Ok(None) => {
                println!("No ventilation set values found in database.");
            }
            Err(e) => {
                if !self.lock_warning_ventilation_set_value_read_failure {
                    warn!(
                        target: LOG_TARGET_SQL_INTERFACE_VENTILATION_SETVALS,
                        "Error when trying to retrieve ventilation set values from database: {e:?} \
                         maintaining the current ventilation set values (off={:?}, on={:?}).",
                        ventilation_switch_off_temperature, ventilation_switch_on_temperature
                    );
                    self.lock_warning_ventilation_set_value_read_failure = true;
                }
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface_ventilation_setvals::{
        SqlInterfaceVentilationSetVals, VentilationSetVals,
    };
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
        match SqlInterface::truncate_table(&mut sql_interface, "ventilationsetvals".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }

        // instantiation of the test object
        let mut sql_interface_ventilation_setvals = SqlInterfaceVentilationSetVals::new(
            sql_interface.get_connection(),
            &config.ventilation,
        );

        // check if reading an empty table returns None
        match sql_interface_ventilation_setvals.get_ventilation_set_values() {
            Ok(c) => {
                assert_eq!(c.is_some(), false);
            }
            Err(_) => {
                panic!("Call to get_ventilation_set_values failed.");
            }
        }
        println!("* reading empty ventilation set values (PASS) succeeded.");

        // define valid value combination
        let valid_ventilation_set_vals = VentilationSetVals {
            ventilation_switch_on_temperature: 25.0,
            ventilation_switch_off_temperature: 20.0,
        };

        // insert valid values
        assert_eq!(
            sql_interface_ventilation_setvals
                .insert_ventilation_set_values(&valid_ventilation_set_vals),
            Ok(())
        );
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
        println!("* reading valid ventilation set values (PASS) succeeded.");

        match SqlInterface::truncate_table(&mut sql_interface, "ventilationsetvals".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }

        // define invalid value combination
        let invalid_ventilation_set_vals = VentilationSetVals {
            ventilation_switch_on_temperature: 21.0,
            ventilation_switch_off_temperature: 26.0,
        };

        // insert invalid values
        assert_eq!(
            sql_interface_ventilation_setvals
                .insert_ventilation_set_values(&invalid_ventilation_set_vals),
            Ok(())
        );
        println!("* inserting invalid ventilation set values as precondition succeeded.");

        // check if reading an invalid value combination returns an error
        assert_eq!(
            sql_interface_ventilation_setvals.get_ventilation_set_values(),
            Err(SqlInterfaceError::VentilationSetValsReadError(
                "The ventilation switch on temperature is lower \
                       than the ventilation switch off temperature."
                    .to_string()
            ))
        );
        println!("* reading  invalid ventilation set values (FAIL) succeeded.");

        //***************************************************************************************
    }
}
