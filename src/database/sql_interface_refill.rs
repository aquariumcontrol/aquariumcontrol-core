/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages all database interactions for the automated water refill system.
//!
//! This module provides a dedicated interface, `SqlInterfaceRefill`, for handling
//! all SQL operations related to the refill event log (`refill` table). It also
//! defines a `DatabaseInterfaceRefillTrait` to decouple the core refill logic
//! from the specific database implementation, adhering to the Dependency Inversion Principle.
//!
//! ## Key Components
//!
//! - **`DatabaseInterfaceRefillTrait`**: An abstraction that defines the contract for
//!   any component that interacts with refill data. This is crucial for testability,
//!   as it allows for mock implementations to be used in unit tests without needing a
//!   live database.
//!
//! - **`SqlInterfaceRefill` Struct**: The concrete implementation of the trait, holding
//!   a database connection and providing methods for all refill-related operations.
//!
//! - **`new()` Constructor**: A critical entry point that performs "fail-fast"
//!   validation at initialization. It checks whether the number of rows in the
//!   `refill` table exceeds the `max_rows_refill` limit defined in the application
//!   configuration, preventing uncontrolled table growth.
//!
//! - **Statistics Methods**: Functions like `get_refill_count_of_last_24h` and
//!   `get_refill_volume_of_last_24h` provide aggregated data about recent refill
//!   activity.
//!
//! - **`insert_refill_event()`**: The core logging function that persists the details
//!   of a completed refill operation to the database.
//!
//! ## Design and Purpose
//!
//! The main goal of this module is to provide a robust, encapsulated, and testable
//! interface for the refill subsystem's data.
//!
//! - **Encapsulation**: All SQL queries and logic specific to the `refill` table are
//!   contained within this module, separating concerns.
//!
//! - **Data Integrity**: The constructor's validation logic enforces the configured
//!   row limit at startup, ensuring the system doesn't start with an overgrown log table.
//!
//! - **Abstraction for Testability**: The `DatabaseInterfaceRefillTrait` allows higher-level
//!   components to be tested independently of the database.

use chrono::*;
use mysql::prelude::*;
use mysql::*;

use crate::database::pingable::Pingable;
use crate::database::sql_query_strings::SQL_QUERY_CHECK_REFILL_COUNT;
use crate::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
};

/// Trait for interfacing between refill and SQL database
/// This trait is prerequisite to use mock implementation for testing
pub trait DatabaseInterfaceRefillTrait: Pingable {
    fn get_duration_since_last_refill(&mut self) -> Result<u64, SqlInterfaceError>;

    fn get_positive_integer_value_from_database(
        &mut self,
        sql_query_string: &str,
    ) -> Result<u64, SqlInterfaceError>;

    fn check_empty(&mut self) -> Result<u64, SqlInterfaceError>;

    fn get_refill_count_of_last_hour(&mut self) -> Result<u64, SqlInterfaceError>;

    fn get_refill_count_of_last_24h(&mut self) -> Result<u64, SqlInterfaceError>;

    fn get_refill_volume_of_last_24h(&mut self) -> Result<f64, SqlInterfaceError>;

    fn get_refill_volume_of_last_hour(&mut self) -> Result<f64, SqlInterfaceError>;

    fn insert_refill_event(
        &mut self,
        timestamp: NaiveDateTime,
        duration: f64,
        volume: f64,
        error_code: i32,
    ) -> Result<(), SqlInterfaceError>;
}

/// Contains the configuration and the implementation of the SQL interface for Refill.
#[derive(Debug)]
pub struct SqlInterfaceRefill {
    /// Connection to the database
    pub conn: PooledConn,
}

impl SqlInterfaceRefill {
    /// Creates a new `SqlInterfaceRefill` instance.
    ///
    /// This constructor initializes the refill management SQL interface with an established
    /// database connection. It performs a pre-flight check to ensure the number of rows
    /// in the `refill` table does not exceed the configured limit.
    ///
    /// # Arguments
    /// * `conn` - An active, pooled database connection.
    /// * `max_rows_refill` - The maximum allowed number of rows in the `refill` table.
    ///   If `0`, the check is skipped.
    ///
    /// # Returns
    /// A `Result` containing a new `SqlInterfaceRefill` instance on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The initial database query to get the table row count fails (`DatabaseCheckRefillFailure`).
    /// - The retrieved row count is a negative number, indicating a database issue (`DatabaseRefillTableNegativeValue`).
    /// - The number of rows in the `refill` table exceeds the configured `max_rows_refill` limit (`DatabaseRefillTableContainsTooManyRows`).
    pub fn new(
        mut conn: PooledConn,
        max_rows_refill: u64,
    ) -> Result<SqlInterfaceRefill, SqlInterfaceError> {
        let count_rows =
            SqlInterface::get_single_integer_from_database(&mut conn, SQL_QUERY_CHECK_REFILL_COUNT)
                .map_err(|e| SqlInterfaceError::DatabaseCheckRefillFailure {
                    location: module_path!().to_string(),
                    source: Box::new(e),
                })?;

        // check the query results
        if count_rows < 0 {
            return Err(SqlInterfaceError::DatabaseRefillTableNegativeValue(
                module_path!().to_string(),
                count_rows,
            ));
        }

        if max_rows_refill > 0 {
            // execute the check only when the limit is greater than zero
            if count_rows > max_rows_refill.cast_signed() {
                return Err(SqlInterfaceError::DatabaseRefillTableContainsTooManyRows(
                    module_path!().to_string(),
                    count_rows.cast_unsigned(),
                    max_rows_refill,
                ));
            }
        }

        Ok(SqlInterfaceRefill { conn })
    }

    /// Fetches the timestamp of the most recent water refill event from the database.
    ///
    /// This private helper function queries the database for the latest recorded
    /// refill event.
    ///
    /// # Returns
    /// A `Result` containing the `NaiveDateTime` of the last refill event on success.
    ///
    /// # Errors
    /// This function will return an `Err` variant of `SqlInterfaceError`, most likely
    /// `TimestampRequestFailure`, if:
    /// - The underlying database query fails due to a connection issue or invalid SQL.
    /// - The `refill` table is empty, and thus no timestamp can be returned.
    /// - The query unexpectedly returns more than one row.
    fn get_last_refill_timestamp(&mut self) -> Result<NaiveDateTime, SqlInterfaceError> {
        SqlInterface::get_timestamp(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_LAST_REFILL_TIMESTAMP,
        )
    }
}

impl DatabaseInterfaceRefillTrait for SqlInterfaceRefill {
    /// Calculates the time elapsed in seconds since the most recent water refill event.
    ///
    /// This function determines the duration by comparing the current database timestamp
    /// with the timestamp of the last recorded refill event.
    ///
    /// # Returns
    /// A `Result` containing the number of seconds (`u64`) that have passed since the last refill event.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - It fails to retrieve either the current timestamp or the last refill timestamp from the database.
    ///   The original error from the database call will be propagated.
    /// - The calculated duration is negative (`NegativeTimeDeltaSeconds`), which suggests a data anomaly
    ///   where the "last refill" timestamp is in the future.
    fn get_duration_since_last_refill(&mut self) -> Result<u64, SqlInterfaceError> {
        let current_datetime = SqlInterface::get_current_timestamp(&mut self.conn)?;

        let last_refill_datetime = self.get_last_refill_timestamp()?;

        let delta_time = current_datetime.signed_duration_since(last_refill_datetime);

        if delta_time.num_seconds() >= 0 {
            Ok(delta_time.num_seconds() as u64)
        } else {
            Err(SqlInterfaceError::NegativeTimeDeltaSeconds(
                module_path!().to_string(),
                delta_time.num_seconds(),
            ))
        }
    }

    /// Executes an SQL query to retrieve a single integer, validating it as non-negative.
    ///
    /// This helper function is used for database queries that are expected to return
    /// a single, non-negative integer.
    ///
    /// # Arguments
    /// * `sql_query_string` - The SQL query to be executed.
    ///
    /// # Returns
    /// A `Result` containing the retrieved integer value as a `u64` on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The underlying database call fails (e.g., query execution error, no result, or multiple results).
    ///   The original error is propagated.
    /// - The database query returns a negative integer (`NegativeInteger`), which is not expected.
    fn get_positive_integer_value_from_database(
        &mut self,
        sql_query_string: &str,
    ) -> Result<u64, SqlInterfaceError> {
        match SqlInterface::get_single_integer_from_database(&mut self.conn, sql_query_string) {
            Ok(c) => {
                if c >= 0 {
                    Ok(c as u64)
                } else {
                    Err(SqlInterfaceError::NegativeInteger(
                        module_path!().to_string(),
                        c,
                    ))
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Checks whether the refill events table in the database is empty.
    ///
    /// This function executes an SQL query to determine if any entries exist
    /// in the refill log.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(0)` if the refill table contains no entries.
    /// - `Ok(1)` if the refill table contains at least one entry.
    ///
    /// # Errors
    /// This function will return an `Err` if the database query fails or if the query
    /// returns an unexpected value (e.g., a negative number).
    fn check_empty(&mut self) -> Result<u64, SqlInterfaceError> {
        self.get_positive_integer_value_from_database(
            sql_query_strings::SQL_QUERY_CHECK_REFILL_EMPTY,
        )
    }

    /// Retrieves the total count of water refill events that occurred within the last hour.
    ///
    /// This function queries the database to count all refill events whose timestamps
    /// fall within the most recent 60-minute period.
    ///
    /// # Returns
    /// A `Result` containing the number of refill events (`u64`) recorded in the last hour.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::RefillPastDataRetrievalFailed` if the underlying
    /// database call fails. This can be caused by a connection issue, an invalid query,
    /// or if the query returns a negative number where a positive count is expected.
    fn get_refill_count_of_last_hour(&mut self) -> Result<u64, SqlInterfaceError> {
        self.get_positive_integer_value_from_database(
            sql_query_strings::SQL_QUERY_READ_REFILL_COUNT_LAST_HOUR,
        )
        .map_err(|e| SqlInterfaceError::RefillPastDataRetrievalFailed {
            location: module_path!().to_string(),
            category: "count".to_string(),
            interval: "last hour".to_string(),
            source: Box::new(e),
        })
    }

    /// Retrieves the total count of water refill events that occurred within the last 24 hours.
    ///
    /// This function queries the database to count all refill events whose timestamps
    /// fall within the most recent 24-hour period.
    ///
    /// # Returns
    /// A `Result` containing the number of refill events (`u64`) recorded in the last 24 hours.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::RefillPastDataRetrievalFailed` if the underlying
    /// database call fails. This can be caused by a connection issue, an invalid query,
    /// or if the query returns a negative number where a positive count is expected.
    fn get_refill_count_of_last_24h(&mut self) -> Result<u64, SqlInterfaceError> {
        self.get_positive_integer_value_from_database(
            sql_query_strings::SQL_QUERY_READ_REFILL_COUNT_LAST_24H,
        )
        .map_err(|e| SqlInterfaceError::RefillPastDataRetrievalFailed {
            location: module_path!().to_string(),
            category: "count".to_string(),
            interval: "last 24h".to_string(),
            source: Box::new(e),
        })
    }

    /// Retrieves the total volume of water (in liters) refilled within the last 24 hours.
    ///
    /// This function queries the database to sum the volume of all refill events
    /// that occurred within the most recent 24-hour period.
    ///
    /// # Returns
    /// A `Result` containing the total volume (`f64`) of water refilled in the last 24 hours.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::RefillPastDataRetrievalFailed` if the underlying
    /// database call fails. This can be caused by a connection issue, an invalid query,
    /// or if the query does not return a single float value.
    fn get_refill_volume_of_last_24h(&mut self) -> Result<f64, SqlInterfaceError> {
        SqlInterface::get_single_float_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_REFILL_VOLUME_LAST_24H,
        )
        .map_err(|e| SqlInterfaceError::RefillPastDataRetrievalFailed {
            location: module_path!().to_string(),
            category: "volume".to_string(),
            interval: "last 24h".to_string(),
            source: Box::new(e),
        })
    }

    /// Retrieves the total volume of water (in liters) refilled within the last hour.
    ///
    /// This function queries the database to sum the volume of all refill events
    /// that occurred within the most recent 60-minute period.
    ///
    /// # Returns
    /// A `Result` containing the total volume (`f64`) of water refilled in the last hour.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::RefillPastDataRetrievalFailed` if the underlying
    /// database call fails. This can be caused by a connection issue, an invalid query,
    /// or if the query does not return a single float value.
    fn get_refill_volume_of_last_hour(&mut self) -> Result<f64, SqlInterfaceError> {
        SqlInterface::get_single_float_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_REFILL_VOLUME_LAST_HOUR,
        )
        .map_err(|e| SqlInterfaceError::RefillPastDataRetrievalFailed {
            location: module_path!().to_string(),
            category: "volume".to_string(),
            interval: "last hour".to_string(),
            source: Box::new(e),
        })
    }

    /// Inserts a new water refill event record into the database.
    ///
    /// This function logs the details of a completed water refill operation,
    /// including its timestamp, duration, volume, and any associated error code.
    ///
    /// # Arguments
    /// * `timestamp` - The `NaiveDateTime` when the refill event occurred.
    /// * `duration` - The duration (in seconds) of the refill event.
    /// * `volume` - The volume (in liters) of water dispensed during the refill.
    /// * `error_code` - A numerical code associated with the event (0 for success).
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the refill event record was successfully inserted.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::InsertRefillEventFailure` if the `INSERT` query fails.
    /// This can be caused by a lost connection, constraint violations, or incorrect
    /// data types. The original `mysql::Error` is included as the source.
    fn insert_refill_event(
        &mut self,
        timestamp: NaiveDateTime,
        duration: f64,
        volume: f64,
        error_code: i32,
    ) -> Result<(), SqlInterfaceError> {
        self.conn
            .exec_drop::<_, _>(
                sql_query_strings::SQL_QUERY_WRITE_REFILL_EVENT,
                params! {
                "timestamp" => timestamp,
                "duration" => duration,
                "volume" => volume,
                "error_code" => error_code,
                },
            )
            .map_err(|e| SqlInterfaceError::InsertRefillEventFailure {
                location: module_path!().to_string(),
                query: sql_query_strings::SQL_QUERY_WRITE_REFILL_EVENT.to_string(),
                source: e,
            })
    }
}

#[cfg(test)]
pub mod tests {
    use chrono::{Local, NaiveDateTime, SubsecRound};
    use mysql::prelude::Queryable;

    use crate::database::sql_interface_refill::{DatabaseInterfaceRefillTrait, SqlInterfaceRefill};
    use crate::database::sql_query_strings::SQL_TABLE_REFILL;
    use crate::database::{
        sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
    };
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};

    pub struct SqlRefillEvent {
        timestamp: String,
        duration: f32,
        volume: f32,
        error_code: i32,
    }

    pub struct RefillEvent {
        timestamp: NaiveDateTime,
        duration: f32,
        volume: f32,
        error_code: i32,
    }
    #[test]
    // This test case verifies the validation logic within the `SqlInterfaceRefill::new()` constructor.
    // It covers the following scenarios:
    // 1. Happy Path: Initialization succeeds when the row count is within the limit.
    // 2. Failure on Row Limit: Fails when the `refill` table exceeds its max row count.
    // 3. Deactivated Check: Succeeds even with excess rows if the limit is set to 0.
    // 4. Edge Case: Succeeds when the row count is exactly the limit.
    // Test case uses a dedicated test database to avoid conflicts.
    fn test_sql_interface_refill_new() {
        // --- Common Setup ---
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            62,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");

        // --- Test Case 1: Happy Path (the row count is within limit) ---
        println!("* Testing new() with valid row count (Happy Path)...");
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string())
            .expect("Test setup failed: Could not truncate table.");

        // Insert one row
        let mut refill_db = SqlInterfaceRefill {
            conn: sql_interface.get_connection().unwrap(),
        };
        refill_db
            .insert_refill_event(Local::now().naive_local(), 1.0, 1.0, 0)
            .expect("Test setup failed: Could not insert row.");

        // Check: new() should succeed with a generous limit of 10
        let result = SqlInterfaceRefill::new(sql_interface.get_connection().unwrap(), 10);
        assert!(
            result.is_ok(),
            "Expected new() to succeed on happy path, but it failed: {:?}",
            result.err()
        );
        println!("* Succeeded: Happy path initialization is successful.");

        // --- Test Case 2: Failure on row limit exceeded ---
        println!("* Testing new() with too many rows in the refill table...");
        // Insert a second row
        let mut refill_db = SqlInterfaceRefill {
            conn: sql_interface.get_connection().unwrap(),
        };
        refill_db
            .insert_refill_event(Local::now().naive_local(), 2.0, 2.0, 0)
            .expect("Test setup failed: Could not insert second row.");

        // Check: new() should fail with a strict limit of 1
        let result = SqlInterfaceRefill::new(sql_interface.get_connection().unwrap(), 1);
        assert!(
            matches!(
                result,
                Err(SqlInterfaceError::DatabaseRefillTableContainsTooManyRows(
                    _,
                    _,
                    _
                ))
            ),
            "Expected row limit error, but got {:?}",
            result
        );
        println!("* Succeeded: Initialization fails if refill table exceeds row limit.");

        // --- Test Case 3: Deactivated check (limit is 0) ---
        println!("* Testing new() with deactivated row limit check (limit = 0)...");
        // We reuse the state from the previous test (2 rows in the table)
        // Set the limit to 0
        let result = SqlInterfaceRefill::new(sql_interface.get_connection().unwrap(), 0);
        assert!(
            result.is_ok(),
            "Expected new() to succeed with deactivated check, but it failed: {:?}",
            result.err()
        );
        println!("* Succeeded: Initialization passes when row limit check is deactivated.");

        // --- Test Case 4: Edge case (row count equals limit) ---
        println!("* Testing new() with row count equal to the limit...");
        // We reuse the state from the previous test (2 rows in the table)
        // Set the limit to 2
        let result = SqlInterfaceRefill::new(sql_interface.get_connection().unwrap(), 2);
        assert!(
            result.is_ok(),
            "Expected new() to succeed when row count equals limit, but it failed: {:?}",
            result.err()
        );
        println!("* Succeeded: Initialization passes when row count is exactly the limit.");
    }

    #[test]
    // Test case includes all checks for this table combined in one function to limit the number of additional databases.
    // Test case uses test database #37.
    pub fn test_sql_interface_refill() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            37,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_refill = config.sql_interface.max_rows_refill;
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_refill =
            SqlInterfaceRefill::new(sql_interface.get_connection().unwrap(), max_rows_refill)
                .unwrap();

        // *** check timestamp of last refill *************************************************
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_REFILL.to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }
        assert!(matches!(sql_interface_refill.check_empty(), Ok(_)));
        assert!(matches!(
            sql_interface_refill.get_last_refill_timestamp(),
            Err(SqlInterfaceError::TimestampRequestFailure {
                location: _,
                query: _
            })
        ));
        println!("* checking timestamp of last refill (FAIL) succeeded.");
        // **************************************************************************************

        // *** check timestamp of last refill - ****************************************************
        let refill_timestamp =
            NaiveDateTime::parse_from_str("2025-04-12 07:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        match sql_interface_refill.insert_refill_event(refill_timestamp, 3.6, 0.4, 0) {
            Ok(_) => {}
            Err(e) => panic!("Could not insert refill event into database: {e:?}"),
        };
        assert!(matches!(sql_interface_refill.check_empty(), Ok(1)));
        assert!(matches!(
            sql_interface_refill.get_last_refill_timestamp(),
            Ok(_)
        ));
        println!("* checking timestamp of last refill succeeded.");
        //**************************************************************************************

        //*** check duration since last refill - **************************************************
        let duration_since_last_refill = match sql_interface_refill.get_duration_since_last_refill()
        {
            Ok(c) => c,
            Err(e) => panic!("Could not calculate duration since last refill: {e:?}"),
        };
        let reference_duration_since_last_refill: u64 =
            SqlInterface::get_duration_from_naive_timestamp_to_local_now(refill_timestamp)
                .unwrap()
                .num_seconds() as u64;
        assert_eq!(
            duration_since_last_refill,
            reference_duration_since_last_refill
        );
        println!("* checking duration since last refill succeeded.");
        //**************************************************************************************

        // *** check refill count of last 24h (zero) - *********************************************
        let refill_count_of_last_24h_zero =
            match sql_interface_refill.get_refill_count_of_last_24h() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill count from last 24h (target=0): {e:?}"),
            };
        assert_eq!(refill_count_of_last_24h_zero, 0);
        println!("* checking refill count of last 24h (target zero) succeeded.");
        // **************************************************************************************

        // *** check refill count of last hour (zero) - ********************************************
        let refill_count_of_last_hour_zero =
            match sql_interface_refill.get_refill_count_of_last_hour() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill count from last hour (target=0): {e:?}"),
            };
        assert_eq!(refill_count_of_last_hour_zero, 0);
        println!("* checking refill count of last hour (target zero) succeeded.");
        // **************************************************************************************

        //*** check refill volume of last 24h (zero) - *********************************************
        let refill_volume_of_last_24h_zero =
            match sql_interface_refill.get_refill_volume_of_last_24h() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill volume from last 24h (target=0): {e:?}"),
            };
        assert_eq!(refill_volume_of_last_24h_zero, 0.0);
        println!("* checking refill volume of last 24h (target zero) succeeded.");
        // **************************************************************************************

        //*** check refill volume of last hour (zero) - *********************************************
        let refill_volume_of_last_hour_zero =
            match sql_interface_refill.get_refill_volume_of_last_hour() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill volume from last hour (target=0): {e:?}"),
            };
        assert_eq!(refill_volume_of_last_hour_zero, 0.0);
        println!("* checking refill volume of last hour (target zero) succeeded.");
        // **************************************************************************************

        // *** check refill count of last 24h (one) - *********************************************
        let naive_datetime_12h_ago = SqlInterface::get_naive_timestamp_from_past(12, 0);
        match sql_interface_refill.insert_refill_event(naive_datetime_12h_ago, 5.0, 0.25, -2) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not insert refill event in data base for test preparation: {e:?}")
            }
        }
        let refill_count_of_last_24h_one = match sql_interface_refill.get_refill_count_of_last_24h()
        {
            Ok(c) => c,
            Err(e) => panic!("Could not get refill count from last 24h (target=1): {e:?}"),
        };
        assert_eq!(refill_count_of_last_24h_one, 1);
        println!("* checking refill count of last 24h (target non-zero) succeeded.");
        // **************************************************************************************

        // *** check refill volume of last 24h (one) - *********************************************
        let refill_volume_of_last_24h_one =
            match sql_interface_refill.get_refill_volume_of_last_24h() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill volume from last 24h (target=0.25): {e:?}"),
            };
        assert_eq!(refill_volume_of_last_24h_one, 0.25);
        println!("* checking refill volume (target non-zero) succeeded.");
        // **************************************************************************************

        // *** check refill count of last hour (one) - *********************************************
        let naive_datetime_30min_ago = SqlInterface::get_naive_timestamp_from_past(0, 30);
        match sql_interface_refill.insert_refill_event(naive_datetime_30min_ago, 5.0, 0.25, -2) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not insert refill event in data base for test preparation: {e:?}")
            }
        }
        let refill_count_of_last_hour_one =
            match sql_interface_refill.get_refill_count_of_last_hour() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill count from last hour (target=1): {e:?}"),
            };
        assert_eq!(refill_count_of_last_hour_one, 1);
        println!("* checking refill count of last hour (target non-zero) succeeded.");
        // **************************************************************************************

        // *** check refill volume of last hour (one) - *********************************************
        let refill_volume_of_last_hour_one =
            match sql_interface_refill.get_refill_volume_of_last_hour() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill volume from last hour (target=0.25): {e:?}"),
            };
        assert_eq!(refill_volume_of_last_hour_one, 0.25);
        println!("* checking refill volume (target non-zero) succeeded.");
        // **************************************************************************************

        // *** check the content of last refill event - ********************************************
        // Read back refill event entry from the database
        let single_refill_event_entry_array = match sql_interface_refill.conn.query_map(
            sql_query_strings::SQL_QUERY_READ_REFILL_EVENT,
            |(timestamp, duration, volume, error_code)| SqlRefillEvent {
                timestamp,
                duration,
                volume,
                error_code,
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not read refill event from data base: {e:?}");
            }
        };
        // check if SQL response has the right structure
        assert_eq!(single_refill_event_entry_array.len(), 1);
        // check if SQL response contains the correct data
        let refill_event = RefillEvent {
            timestamp: NaiveDateTime::parse_from_str(
                single_refill_event_entry_array[0].timestamp.as_str(),
                "%Y-%m-%d %H:%M:%S",
            )
            .expect("conversion of timestamp to NaiveDate failed."),
            duration: single_refill_event_entry_array[0].duration,
            volume: single_refill_event_entry_array[0].volume,
            error_code: single_refill_event_entry_array[0].error_code,
        };
        assert_eq!(
            refill_event.timestamp,
            naive_datetime_30min_ago.trunc_subsecs(0)
        );
        assert_eq!(refill_event.duration, 5.0);
        assert_eq!(refill_event.volume, 0.25);
        assert_eq!(refill_event.error_code, -2);

        // *** Check max. number of rows detection ***
        // refill table already has three rows at this point.
        let test_result = SqlInterfaceRefill::new(sql_interface.get_connection().unwrap(), 1);
        assert!(matches!(
            test_result,
            Err(SqlInterfaceError::DatabaseRefillTableContainsTooManyRows(
                _,
                _,
                _
            ))
        ));
        // **************************************************************************************
    }
}
