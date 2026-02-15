/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages all database interactions for the Balling dosing system.
//!
//! This module provides a dedicated interface, `SqlInterfaceBalling`, for handling
//! all SQL operations related to the Balling feature. It encapsulates logic for
//! reading pump configurations (`balling_set_vals`), logging dosing events
//! (`ballingdosinglog`), and retrieving dosing history.
//!
//! ## Key Components
//!
//! - **`SqlInterfaceBalling` Struct**: The primary struct that holds a database
//!   connection and provides methods for all Balling-related database operations.
//!
//! - **`BallingSetVal` Struct**: A data structure representing the configuration
//!   parameters for a single Balling pump, such as dosing speed and volume.
//!
//! - **`new()` Constructor**: A critical entry point that not only creates an
//!   `SqlInterfaceBalling` instance but also performs essential "fail-fast"
//!   validation. At initialization, it checks the database for:
//!   - The presence of `NULL` values in the configuration table.
//!   - Whether the number of rows in the `balling_set_vals` and `balling_log`
//!     tables exceeds the limits defined in the application configuration.
//!     This ensures the system doesn't start in an invalid or inconsistent state.
//!
//! ## Design and Purpose
//!
//! The main goal of this module is to provide a robust, encapsulated, and safe
//! interface for the Balling subsystem's data.
//!
//! - **Encapsulation**: All SQL queries and logic specific to Balling are contained
//!   within this module, separating concerns and making the overall database
//!   architecture cleaner.
//!
//! - **Data Integrity**: The constructor's validation logic enforces data integrity
//!   rules at startup, preventing runtime errors caused by misconfigured or
//!   overgrown database tables.

#[cfg(test)]
use log::error;

use chrono::NaiveDateTime;
use mysql::prelude::*;
use mysql::*;
use std::time::Duration;

use crate::database::sql_query_strings::{
    SQL_QUERY_CHECK_BALLING_LOG_COUNT, SQL_QUERY_CHECK_BALLING_SET_VALS_COUNT,
    SQL_QUERY_CHECK_BALLING_SET_VALS_NULL,
};
use crate::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
};

/// Contains the configuration and the implementation of the SQL interface for Balling.
#[derive(Debug)]
pub struct SqlInterfaceBalling {
    /// Connection to the database
    pub conn: PooledConn,
}

/// Contains set value data for each pump.
pub struct BallingSetVal {
    /// ID of the pump
    /// Attribute is public because access from within test cases.
    /// Data is not used in the implementation. Therefore, it is tagged as unused.
    #[allow(unused)]
    pub pump_id: i32,

    /// flow rate of the pump in ml/sec
    pub dosing_speed: f32,

    /// dosing volume in ml which the pump shall deliver each hour
    pub dosing_volume: f32,

    /// A label which describes the fluid that the pump is delivering
    /// Attribute is public because access from within test cases.
    /// Data is not used in the implementation. Therefore, it is tagged as unused.
    #[allow(unused)]
    pub label: String,
}

impl SqlInterfaceBalling {
    /// Creates a new `SqlInterfaceBalling` instance.
    ///
    /// This constructor initializes the Balling SQL interface with an established
    /// database connection. It performs several pre-flight checks to ensure data
    /// integrity and adherence to configured limits, such as verifying that the
    /// Balling set value table contains no NULL values and that table row counts
    /// are within their specified maximums.
    ///
    /// # Arguments
    /// * `conn` - An active, pooled database connection.
    /// * `max_rows_balling_set_values` - Maximum allowed number of rows in the Balling set value table.
    /// * `max_rows_balling_dosing_log` - Maximum allowed number of rows in the Balling dosing log table.
    ///
    /// # Returns
    /// A `Result` containing a new `SqlInterfaceBalling` instance on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - Any of the initial database queries to get table counts fail (`DatabaseCheckBallingSetValsFailure`).
    /// - Any of the retrieved counts are negative, indicating a database issue (`DatabaseBallingSetValTableNegativeValue`).
    /// - The `ballingsetvals` table contains entries with `NULL` values (`DatabaseBallingSetValTableContainsNull`).
    /// - The number of rows in the `ballingsetvals` table exceeds `max_rows_balling_set_values` (`DatabaseBallingSetValTableContainsTooManyRows`).
    /// - The number of rows in the `ballingdosinglog` table exceeds `max_rows_balling_dosing_log` (`DatabaseBallingDosingLogTableContainsTooManyRows`).
    pub fn new(
        mut conn: PooledConn,
        max_rows_balling_set_values: u64,
        max_rows_balling_dosing_log: u64,
    ) -> Result<Self, SqlInterfaceError> {
        // query the database
        let count_null_values = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_BALLING_SET_VALS_NULL,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckBallingSetValsFailure {
            location: module_path!().to_string(),
            source: Box::new(e),
        })?;
        let count_rows_setvals = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_BALLING_SET_VALS_COUNT,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckBallingSetValsFailure {
            location: module_path!().to_string(),
            source: Box::new(e),
        })?;
        let count_rows_dosing_log = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_BALLING_LOG_COUNT,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckBallingSetValsFailure {
            location: module_path!().to_string(),
            source: Box::new(e),
        })?;

        // check the query results
        if count_null_values < 0 || count_rows_setvals < 0 || count_rows_dosing_log < 0 {
            return Err(SqlInterfaceError::DatabaseBallingSetValTableNegativeValue(
                module_path!().to_string(),
                count_null_values,
                count_rows_setvals,
                count_rows_dosing_log,
            ));
        }

        if count_null_values > 0 {
            return Err(SqlInterfaceError::DatabaseBallingSetValTableContainsNull(
                module_path!().to_string(),
                count_null_values,
            ));
        }

        if max_rows_balling_set_values > 0 {
            // execute the check only when the limit is greater than zero
            if count_rows_setvals as u64 > max_rows_balling_set_values {
                return Err(
                    SqlInterfaceError::DatabaseBallingSetValTableContainsTooManyRows(
                        module_path!().to_string(),
                        count_rows_setvals.cast_unsigned(),
                        max_rows_balling_set_values,
                    ),
                );
            }
        }

        if max_rows_balling_dosing_log > 0 {
            // execute the check only when the limit is greater than zero
            if count_rows_dosing_log as u64 > max_rows_balling_dosing_log {
                return Err(
                    SqlInterfaceError::DatabaseBallingDosingLogTableContainsTooManyRows(
                        module_path!().to_string(),
                        count_rows_dosing_log.cast_unsigned(),
                        max_rows_balling_dosing_log,
                    ),
                );
            }
        }

        Ok(SqlInterfaceBalling { conn })
    }

    /// Retrieves the timestamp of the last Balling dosing event for a specific pump.
    ///
    /// This function queries the database for the most recent dosing timestamp associated
    /// with the given `pump_id`.
    ///
    /// # Arguments
    /// * `pump_id` - The unique identifier of the pump.
    ///
    /// # Returns
    /// A `Result` containing an `Option<NaiveDateTime>`:
    /// - `Ok(Some(NaiveDateTime))` if a last dosing timestamp is found for the pump.
    /// - `Ok(None)` if no previous dosing events are recorded for the specified pump.
    ///
    /// # Errors
    /// This function will return an error if the underlying database query fails. This
    /// could be due to a connection issue, a syntax error, or if the query unexpectedly
    /// returns more than one row. The specific error is propagated from the underlying
    /// `get_optional_timestamp` call.
    fn get_last_balling_dosing_timestamp(
        &mut self,
        pump_id: i64,
    ) -> Result<Option<NaiveDateTime>, SqlInterfaceError> {
        let sql_query = str::replace(
            sql_query_strings::SQL_QUERY_READ_LAST_BALLING_DOSING_TIMESTAMP,
            "#pump_id",
            &pump_id.to_string(),
        );
        SqlInterface::get_optional_timestamp(&mut self.conn, &sql_query)
    }

    /// Calculates the duration in seconds since the last Balling dosing event for a given pump.
    ///
    /// This function retrieves both the current database timestamp and the last dosing
    /// timestamp for the specified `pump_id`. It then computes the time difference.
    ///
    /// # Arguments
    /// * `pump_id` - The unique identifier of the pump.
    ///
    /// # Returns
    /// A `Result` containing an `Option<i64>`:
    /// - `Ok(Some(Duration::new))`: The duration that has passed since the last dosing event.
    /// - `Ok(None)`: If no previous dosing event is recorded for this pump.
    ///
    /// # Errors
    /// This function will return an error if it fails to retrieve either the current
    /// timestamp or the last dosing timestamp from the database. This could be due to
    /// a connection loss or other database-side issues. It also will return an error if the
    /// database contains any entries that are in the future.
    pub fn get_duration_since_last_balling_dosing(
        &mut self,
        pump_id: i64,
    ) -> Result<Option<Duration>, SqlInterfaceError> {
        let current_datetime = SqlInterface::get_current_timestamp(&mut self.conn)?;
        let last_dosing_datetime_opt = self.get_last_balling_dosing_timestamp(pump_id)?;

        match last_dosing_datetime_opt {
            Some(last_dosing_datetime) => {
                if last_dosing_datetime > current_datetime {
                    Err(SqlInterfaceError::BallingDosingLogEntryInFuture(
                        module_path!().to_string(),
                        pump_id,
                        last_dosing_datetime,
                    ))
                } else {
                    let duration =
                        (current_datetime - last_dosing_datetime)
                            .to_std()
                            .map_err(|e| {
                                SqlInterfaceError::BallingDosingTimestampConversionFailure {
                                    location: module_path!().to_string(),
                                    pump_id,
                                    current_datetime,
                                    last_dosing_datetime,
                                    source: e,
                                }
                            })?;

                    Ok(Some(duration))
                }
            }
            None => Ok(None),
        }
    }

    // Inserts a new Balling dosing event record into the SQL database using a given timestamp.
    // This function is used for testing purposes only.
    //
    // This function logs details of a Balling dosing operation, including the
    // exact time, the pump used, and the volume of fluid dispensed.
    //
    // # Arguments
    // * `timestamp` - The `NaiveDateTime` when the dosing event occurred.
    // * `pump_id` - The identifier of the pump that performed the dosing.
    // * `dosing_volume` - The volume (in milliliters) of fluid dispensed.
    //
    // # Returns
    // An empty `Result` (`Ok(())`) if the event record was successfully inserted.
    //
    // # Errors
    // Returns `SqlInterfaceError::InsertBallingEventFailure` if the `INSERT`
    // query fails. This can happen due to a lost connection, constraint violation
    // (e.g., foreign key), or incorrect data types. The original `mysql::Error`
    // is included as the source.
    #[cfg(test)]
    pub fn insert_balling_event_for_test(
        &mut self,
        timestamp: NaiveDateTime,
        pump_id: i64,
        dosing_volume: f64,
    ) -> Result<(), SqlInterfaceError> {
        self.conn
            .exec_drop(
                sql_query_strings::SQL_QUERY_WRITE_BALLING_EVENT_WITH_TIMESTAMP.to_owned(),
                params! {
                    "timestamp" => timestamp,
                    "pump_id" => pump_id,
                    "dosing_volume" => dosing_volume,
                },
            )
            .map_err(|e| SqlInterfaceError::InsertBallingEventFailure {
                location: module_path!().to_string(),
                source: e,
            })
    }

    /// Inserts a new Balling dosing event record into the SQL database using current timestamp of database.
    ///
    /// This function logs details of a Balling dosing operation, including the
    /// exact time, the pump used, and the volume of fluid dispensed.
    ///
    /// # Arguments
    /// * `timestamp` - The `NaiveDateTime` when the dosing event occurred.
    /// * `pump_id` - The identifier of the pump that performed the dosing.
    /// * `dosing_volume` - The volume (in milliliters) of fluid dispensed.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the event record was successfully inserted.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::InsertBallingEventFailure` if the `INSERT`
    /// query fails. This can happen due to a lost connection, constraint violation
    /// (e.g., foreign key), or incorrect data types. The original `mysql::Error`
    /// is included as the source.
    pub fn insert_balling_event(
        &mut self,
        pump_id: i64,
        dosing_volume: f64,
    ) -> Result<(), SqlInterfaceError> {
        self.conn
            .exec_drop(
                sql_query_strings::SQL_QUERY_WRITE_BALLING_EVENT.to_owned(),
                params! {
                    "pump_id" => pump_id,
                    "dosing_volume" => dosing_volume,
                },
            )
            .map_err(|e| SqlInterfaceError::InsertBallingEventFailure {
                location: module_path!().to_string(),
                source: e,
            })
    }

    /// Retrieves a single set of Balling dosing parameters for a specified pump from the database.
    ///
    /// This function queries the database for the Balling set values (e.g., dosing speed, volume, label)
    /// associated with a given `pump_id`. It strictly expects to find exactly one matching dataset.
    ///
    /// # Arguments
    /// * `pump_id` - The unique identifier of the pump for which to retrieve the set values.
    ///
    /// # Returns
    /// A `Result` containing the `BallingSetVal` for the specified pump on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The database query fails for any reason (`SingleBallingSetValRequestFailure`).
    /// - The query returns zero rows (`SingleBallingSetValEmptyResponse`).
    /// - The query returns more than one row (`SingleBallingSetValNoSingleResponse`), which
    ///   indicates a data consistency issue.
    pub fn get_single_balling_setval_from_database(
        &mut self,
        pump_id: i64,
    ) -> Result<BallingSetVal, SqlInterfaceError> {
        let params = params! { "pump_id" => pump_id };

        // Get a single data set from the database
        let mut single_balling_setval_array = match self.conn.exec_map(
            sql_query_strings::SQL_QUERY_READ_BALLING_SET_VALS,
            params,
            |(pump_id, dosing_speed, dosing_volume, label)| BallingSetVal {
                pump_id,
                dosing_speed,
                dosing_volume,
                label,
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                return Err(SqlInterfaceError::SingleBallingSetValRequestFailure {
                    location: module_path!().to_string(),
                    query: sql_query_strings::SQL_QUERY_READ_BALLING_SET_VALS.to_string(),
                    pump_id,
                    source: e,
                });
            }
        };

        // check the error case if there is no response
        if single_balling_setval_array.is_empty() {
            return Err(SqlInterfaceError::SingleBallingSetValEmptyResponse(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_READ_BALLING_SET_VALS.to_string(),
                pump_id,
            ));
        }

        // check the error case if there is more than one response
        if single_balling_setval_array.len() > 1 {
            return Err(SqlInterfaceError::SingleBallingSetValNoSingleResponse(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_READ_BALLING_SET_VALS.to_string(),
                pump_id,
            ));
        }
        Ok(single_balling_setval_array.remove(0))
    }

    #[cfg(test)]
    // Inserts a pump configuration dataset into the database.
    //
    // This helper function is used exclusively in test environments to
    // pre-populate the database with specific pump configuration data,
    // allowing for consistent and isolated testing of related functionalities.
    //
    // # Arguments
    // * `pump_id` - The unique identifier for the pump.
    // * `dosing_speed` - The flow rate of the pump in milliliters per second.
    // * `dosing_volume` - The volume (in milliliters) intended to be dosed in a single event.
    // * `label` - A descriptive label for the fluid being dispensed by the pump.
    //
    // # Returns
    // An empty `Result` (`Ok(())`) if the configuration was successfully inserted.
    //
    // # Errors
    // Returns `SqlInterfaceError::InsertBallingPumpConfigurationFailure` if the
    // `INSERT` query fails. This error should be updated to include the source
    // `mysql::Error` for better diagnostics in test failures.
    pub fn insert_pump_configuration(
        &mut self,
        pump_id: i64,
        dosing_speed: f32,
        dosing_volume: f32,
        label: String,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_PUMP_CONFIGURATION.to_owned();
        let sql_query_string_for_error = sql_query_string.clone();
        match self.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "pump_id" => pump_id,
            "dosing_speed" => dosing_speed,
            "dosing_volume" => dosing_volume,
            "label" => label,
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                error!( // logging for testing purposes only
                    target: module_path!(),
                    "could not insert Balling pump configuration into database ({e:?})"
                );
                Err(SqlInterfaceError::InsertBallingPumpConfigurationFailure(
                    module_path!().to_string(),
                    sql_query_string_for_error,
                ))
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface_balling::SqlInterfaceBalling;
    use crate::database::{
        sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
    };
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};
    use all_asserts::assert_le;
    use chrono::Local;
    use chrono::NaiveDateTime;
    use mysql::{params, prelude::Queryable};
    use spin_sleep::SpinSleeper;
    use std::time::Duration;
    use assert_float_eq::assert_float_absolute_eq;

    pub struct SqlBallingEvent {
        timestamp: String,
        pump_id: i32,
        dosing_volume: f32,
    }

    pub struct BallingEvent {
        timestamp: NaiveDateTime,
        pump_id: i32,
        dosing_volume: f32,
    }

    #[test]
    // This test case verifies the validation logic within the `SqlInterfaceBalling::new()` constructor.
    // It covers the following scenarios:
    // 1. Happy Path: Initialization succeeds with valid data and row counts.
    // 2. Failure on Row Limit (SetVals): Fails when `ballingsetvals` exceeds its max row count.
    // 3. Failure on Row Limit (DosingLog): Fails when `ballingdosinglog` exceeds its max row count.
    // 4. Deactivated Checks: Succeeds even with excess rows if the limits are set to 0.
    // Test case uses test database #57.
    fn test_sql_interface_balling_new() {
        // --- Common Setup ---
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            57,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");

        // --- Test Case 1: Happy Path ---
        println!("* Testing new() with valid data (Happy Path)...");
        SqlInterface::truncate_table(
            &mut sql_interface,
            sql_query_strings::SQL_TABLE_BALLING_SET_VALS.to_string(),
        )
        .unwrap();
        SqlInterface::truncate_table(
            &mut sql_interface,
            sql_query_strings::SQL_TABLE_BALLING_DOSING_LOG.to_string(),
        )
        .unwrap();

        // Insert one valid row into each table
        let mut conn = sql_interface.get_connection().unwrap();
        conn.exec_drop(
            sql_query_strings::SQL_QUERY_WRITE_BALLING_SET_VALS,
            params! { "pump_id" => 1, "dosing_speed" => 1.0, "dosing_volume" => 1.0, "label" => "valid" },
        ).unwrap();
        conn.exec_drop(
            sql_query_strings::SQL_QUERY_WRITE_BALLING_EVENT_WITH_TIMESTAMP,
            params! { "timestamp" => Local::now().naive_local(), "pump_id" => 1, "dosing_volume" => 1.0 },
        ).unwrap();

        let result = SqlInterfaceBalling::new(conn, 10, 10);
        assert!(
            result.is_ok(),
            "Expected new() to succeed on happy path, but it failed: {:?}",
            result.err()
        );
        println!("* Succeeded: Happy path initialization is successful.");

        // --- Test Case 2: Failure on ballingsetvals row limit exceeded ---
        println!("* Testing new() with too many rows in ballingsetvals...");
        SqlInterface::truncate_table(
            &mut sql_interface,
            sql_query_strings::SQL_TABLE_BALLING_SET_VALS.to_string(),
        )
        .unwrap();
        let mut conn = sql_interface.get_connection().unwrap();
        conn.exec_drop(sql_query_strings::SQL_QUERY_WRITE_BALLING_SET_VALS, params! { "pump_id" => 1, "dosing_speed" => 1.0, "dosing_volume" => 1.0, "label" => "row1" }).unwrap();
        conn.exec_drop(sql_query_strings::SQL_QUERY_WRITE_BALLING_SET_VALS, params! { "pump_id" => 2, "dosing_speed" => 1.0, "dosing_volume" => 1.0, "label" => "row2" }).unwrap();

        let result = SqlInterfaceBalling::new(conn, 1, 10); // Set the limit to 1
        assert!(
            matches!(
                result,
                Err(SqlInterfaceError::DatabaseBallingSetValTableContainsTooManyRows(_, _, _))
            ),
            "Expected row limit error for setvals, but got {:?}",
            result
        );
        println!("* Succeeded: Initialization fails if ballingsetvals exceeds row limit.");

        // --- Test Case 3: Failure on ballingdosinglog row limit exceeded ---
        println!("* Testing new() with too many rows in ballingdosinglog...");
        let spin_sleeper = SpinSleeper::default();
        SqlInterface::truncate_table(
            &mut sql_interface,
            sql_query_strings::SQL_TABLE_BALLING_DOSING_LOG.to_string(),
        )
        .unwrap();
        let mut conn = sql_interface.get_connection().unwrap();
        conn.exec_drop(sql_query_strings::SQL_QUERY_WRITE_BALLING_EVENT_WITH_TIMESTAMP, params! { "timestamp" => Local::now().naive_local(), "pump_id" => 1, "dosing_volume" => 1.0 }).unwrap();
        spin_sleeper.sleep(Duration::from_millis(500));
        conn.exec_drop(sql_query_strings::SQL_QUERY_WRITE_BALLING_EVENT_WITH_TIMESTAMP, params! { "timestamp" => Local::now().naive_local(), "pump_id" => 2, "dosing_volume" => 1.0 }).unwrap();

        let result = SqlInterfaceBalling::new(conn, 10, 1); // Set the limit to 1
        assert!(
            matches!(
                result,
                Err(SqlInterfaceError::DatabaseBallingDosingLogTableContainsTooManyRows(_, _, _))
            ),
            "Expected row limit error for the dosing log, but got {:?}",
            result
        );
        println!("* Succeeded: Initialization fails if ballingdosinglog exceeds row limit.");

        // --- Test Case 4: Deactivated checks (limits are 0) ---
        println!("* Testing new() with deactivated row limit checks (limits = 0)...");
        // We reuse the state from the previous test (2 rows in the dosing log)
        let conn = sql_interface.get_connection().unwrap();
        let result = SqlInterfaceBalling::new(conn, 0, 0); // Set limits to 0
        assert!(
            result.is_ok(),
            "Expected new() to succeed with deactivated checks, but it failed: {:?}",
            result.err()
        );
        println!("* Succeeded: Initialization passes when row limit checks are deactivated.");
    }

    #[test]
    // Test case verifies the behavior of all functions accessing the Balling event log table.
    // They are all combined in one function to limit the number of additional databases.
    // Test case uses test database #30.
    pub fn test_sql_interface_balling_events() {
        let pump_id = 1;
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            30,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
        let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_balling = SqlInterfaceBalling::new(
            sql_interface.get_connection().unwrap(),
            max_rows_balling_set_values,
            max_rows_balling_dosing_log,
        )
        .unwrap();

        // *** check the timestamp of last Balling dosing ******************************************
        match SqlInterface::truncate_table(
            &mut sql_interface,
            sql_query_strings::SQL_TABLE_BALLING_DOSING_LOG.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        assert!(matches!(
            sql_interface_balling.get_last_balling_dosing_timestamp(pump_id),
            Ok(None)
        ));
        println!("* checking timestamp of last Balling dosing log succeeded.");
        // *****************************************************************************************

        // *** check timestamp of last Balling dosing **********************************************
        let balling_timestamp =
            NaiveDateTime::parse_from_str("2025-04-12 11:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        match sql_interface_balling.insert_balling_event_for_test(balling_timestamp, pump_id, 0.5) {
            Ok(_) => {}
            Err(e) => panic!("Could not insert Balling dosing event into database: {e:?}"),
        };
        assert!(matches!(
            sql_interface_balling.get_last_balling_dosing_timestamp(pump_id),
            Ok(_)
        ));
        println!("* checking timestamp of last Balling dosing succeeded.");
        // *****************************************************************************************

        // *** check the content of last Balling dosing ********************************************
        // Read back Balling dosing event entry from the database
        let single_balling_event_entry_array = match sql_interface_balling.conn.query_map(
            sql_query_strings::SQL_QUERY_READ_BALLING_EVENT,
            |(timestamp, pump_id, dosing_volume)| SqlBallingEvent {
                timestamp,
                pump_id,
                dosing_volume,
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not read Balling dosing event from data base: {e:?}");
            }
        };
        // check if SQL response has the right structure
        assert_eq!(single_balling_event_entry_array.len(), 1);
        // check if SQL response contains the correct data
        let balling_event = BallingEvent {
            timestamp: NaiveDateTime::parse_from_str(
                single_balling_event_entry_array[0].timestamp.as_str(),
                "%Y-%m-%d %H:%M:%S",
            )
            .expect("conversion of timestamp to NaiveDate failed."),
            pump_id: single_balling_event_entry_array[0].pump_id,
            dosing_volume: single_balling_event_entry_array[0].dosing_volume,
        };
        assert_eq!(balling_event.timestamp, balling_timestamp);
        assert_eq!(balling_event.pump_id, 1);
        assert_eq!(balling_event.dosing_volume, 0.5);
        // *****************************************************************************************

        // *** check duration since last Balling dosing ********************************************
        let duration_since_last_balling_dosing =
            match sql_interface_balling.get_duration_since_last_balling_dosing(1) {
                Ok(c) => c,
                Err(e) => panic!("Could not calculate duration since last Balling dosing: {e:?}"),
            }
            .unwrap();

        // Reference calculation uses the database clock to avoid timezone and DST issues
        let db_now = SqlInterface::get_current_timestamp(&mut sql_interface_balling.conn)
            .expect("Failed to get current timestamp from DB for reference calculation.");
        let reference_duration_since_last_balling_dosing =
            (db_now - balling_timestamp).num_seconds();

        let duration_delta_seconds = duration_since_last_balling_dosing
            .as_secs()
            .abs_diff(reference_duration_since_last_balling_dosing as u64);
        assert_le!(duration_delta_seconds, 1);
        println!("* checking duration since last Balling dosing succeeded.");
        // *****************************************************************************************
    }

    #[test]
    // Test case verifies the behavior of all functions accessing the Balling set value table.
    // They are all combined in one function to limit the number of additional databases.
    // Test case uses test database #31.
    pub fn test_sql_interface_balling_set_vals() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            31,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
        let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_balling = SqlInterfaceBalling::new(
            sql_interface.get_connection().unwrap(),
            max_rows_balling_set_values,
            max_rows_balling_dosing_log,
        )
        .unwrap();

        // *** read balling set vals from empty table **********************************************
        match SqlInterface::truncate_table(
            &mut sql_interface,
            sql_query_strings::SQL_TABLE_BALLING_SET_VALS.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        match sql_interface_balling.get_single_balling_setval_from_database(1) {
            Ok(_) => {
                panic!("Reading from empty set value table returned value.");
            }
            Err(e) => {
                assert!(matches!(
                    e,
                    SqlInterfaceError::SingleBallingSetValEmptyResponse(_, _, _)
                ));
            }
        }
        println!("* checking reading from empty Balling dosing set value table succeeded.");
        // *****************************************************************************************

        // *** read balling set vals from pre-filled table *****************************************
        // prepare database: insert Balling set vals
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_BALLING_SET_VALS.to_string();
        match sql_interface_balling.conn.exec_drop::<_, _>(
            sql_query_string.clone(),
            params! {
            "pump_id" => 1,
            "dosing_speed" => 2.0,
            "dosing_volume" => 0.5,
            "label" => "test_pump".to_string(),
            },
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Error when inserting Balling set values into database: {e:?}");
            }
        };
        match sql_interface_balling.get_single_balling_setval_from_database(1) {
            Ok(c) => {
                assert_eq!(c.pump_id, 1);
                assert_eq!(c.dosing_speed, 2.0);
                assert_eq!(c.dosing_volume, 0.5);
                assert_eq!(c.label, "test_pump");
            }
            Err(e) => {
                panic!("Error when reading from pre-filled Balling set value table: {e:?}");
            }
        }
        println!("* checking reading existing Balling dosing set values succeeded.");

        // *** check if instantiation of SqlInterfaceBalling checks maximum number of rows ******
        // add one more Balling set value row
        match sql_interface_balling.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "pump_id" => 2,
            "dosing_speed" => 3.0,
            "dosing_volume" => 0.5,
            "label" => "test_pump2".to_string(),
            },
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Error when inserting Balling set values into database: {e:?}");
            }
        };
        let test_result = SqlInterfaceBalling::new(sql_interface.get_connection().unwrap(), 1, 0);
        assert!(matches!(
            test_result,
            Err(SqlInterfaceError::DatabaseBallingSetValTableContainsTooManyRows(_, _, _))
        ));

        // **************************************************************************************
    }

    #[test]
    // This test case verifies the `insert_balling_event` function.
    // It ensures that a Balling dosing event is correctly inserted into the database
    // with the current database timestamp.
    // Test case uses test database #66.
    fn test_insert_balling_event() {
        let pump_id = 1;
        let dosing_volume = 1.5;
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            66,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_balling_set_values = config.sql_interface.max_rows_balling_set_values;
        let max_rows_balling_dosing_log = config.sql_interface.max_rows_balling_dosing_log;
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_balling = SqlInterfaceBalling::new(
            sql_interface.get_connection().unwrap(),
            max_rows_balling_set_values,
            max_rows_balling_dosing_log,
        )
        .unwrap();

        // Clear the dosing log table
        match SqlInterface::truncate_table(
            &mut sql_interface,
            sql_query_strings::SQL_TABLE_BALLING_DOSING_LOG.to_string(),
        ) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }

        // Insert a Balling event
        match sql_interface_balling.insert_balling_event(pump_id, dosing_volume) {
            Ok(_) => {}
            Err(e) => panic!("Could not insert Balling dosing event into database: {e:?}"),
        };

        // Read back the event to verify
        let single_balling_event_entry_array = match sql_interface_balling.conn.query_map(
            sql_query_strings::SQL_QUERY_READ_BALLING_EVENT,
            |(timestamp, pump_id, dosing_volume)| SqlBallingEvent {
                timestamp,
                pump_id,
                dosing_volume,
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not read Balling dosing event from data base: {e:?}");
            }
        };

        // Check if SQL response has the right structure
        assert_eq!(single_balling_event_entry_array.len(), 1);
        // Check if SQL response contains the correct data
        let event = &single_balling_event_entry_array[0];
        assert_eq!(event.pump_id, pump_id as i32);
        assert_float_absolute_eq!(event.dosing_volume, dosing_volume as f32, 0.001);

        // Verify timestamp is recent (within last few seconds)
        let event_timestamp = NaiveDateTime::parse_from_str(
            event.timestamp.as_str(),
            "%Y-%m-%d %H:%M:%S",
        )
        .expect("conversion of timestamp to NaiveDate failed.");
        let db_now = SqlInterface::get_current_timestamp(&mut sql_interface_balling.conn)
            .expect("Failed to get current timestamp from DB.");
        let duration_diff = (db_now - event_timestamp).num_seconds().abs();
        assert_le!(duration_diff, 5, "Timestamp should be recent");

        println!("* checking insert_balling_event succeeded.");
    }
}
