/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

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
use crate::database::sql_test_query_strings::SQL_QUERY_CHECK_REFILL_COUNT;
use crate::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum DatabaseRefillErrorCode {
    Success = 0,
    InvalidSwitch = -1,
    StuckLowTimeout = -2,
}

/// Trait for interfacing between refill and SQL database
/// This trait is prerequisite to use mock implementation for testing
pub trait DatabaseInterfaceRefillTrait: Pingable {
    fn get_duration_since_last_refill(&mut self) -> Result<u64, SqlInterfaceError>;

    /// Retrieves the positive integer value from database.
    ///
    /// # Arguments
    /// * `sql_query_string` - A parameter representing `sql_query_string`.
    fn get_positive_integer_value_from_database(
        &mut self,
        sql_query_string: &str,
    ) -> Result<u64, SqlInterfaceError>;

    /// Checks the empty.
    fn check_empty(&mut self) -> Result<u64, SqlInterfaceError>;

    /// Retrieves the refill count of last hour.
    fn get_refill_count_of_last_hour(&mut self) -> Result<u64, SqlInterfaceError>;

    /// Retrieves the refill count of last 24h.
    fn get_refill_count_of_last_24h(&mut self) -> Result<u64, SqlInterfaceError>;

    /// Retrieves the refill volume of last 24h.
    fn get_refill_volume_of_last_24h(&mut self) -> Result<f64, SqlInterfaceError>;

    /// Retrieves the refill volume of last hour.
    fn get_refill_volume_of_last_hour(&mut self) -> Result<f64, SqlInterfaceError>;

    /// Performs the `insert_refill_event` operation.
    ///
    /// # Arguments
    /// * `timestamp` - A parameter representing `timestamp`.
    /// * `duration` - A parameter representing `duration`.
    /// * `volume` - A parameter representing `volume`.
    /// * `error_code` - A parameter representing `error_code`.
    fn insert_refill_event(
        &mut self,
        timestamp: NaiveDateTime,
        duration: f64,
        volume: f64,
        error_code: DatabaseRefillErrorCode,
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
    ///
    /// [impl->swreq~water_startup_data_recovery~1]
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
    pub fn get_last_refill_timestamp(&mut self) -> Result<NaiveDateTime, SqlInterfaceError> {
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
        error_code: DatabaseRefillErrorCode,
    ) -> Result<(), SqlInterfaceError> {
        self.conn
            .exec_drop::<_, _>(
                sql_query_strings::SQL_QUERY_WRITE_REFILL_EVENT,
                params! {
                "timestamp" => timestamp,
                "duration" => duration,
                "volume" => volume,
                "error_code" => error_code as i32,
                },
            )
            .map_err(|e| SqlInterfaceError::InsertRefillEventFailure {
                location: module_path!().to_string(),
                query: sql_query_strings::SQL_QUERY_WRITE_REFILL_EVENT.to_string(),
                source: e,
            })
    }
}
