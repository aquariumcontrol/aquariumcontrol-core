/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Provides the core SQL database interface and connection management for the application.
//!
//! This module is the foundation of the application's database layer. It defines the
//! main `SqlInterface` struct, which is responsible for establishing and managing a
//! connection pool to the MySQL database. It also provides a suite of low-level
//! helper functions for executing common query patterns, which are then used by
//! more specialized `SqlInterface...` modules throughout the application.
//!
//! ## Key Components
//!
//! - **`SqlInterface` Struct**: The central struct that holds the `mysql::Pool`. It is
//!   instantiated once at application startup and provides a `get_connection()` method
//!   to lend out `PooledConn` objects to other threads and modules.
//!
//! - **`new()` Constructor**: Handles the critical task of initializing the database
//!   connection. It builds the connection URL, creates the pool, and performs
//!   essential pre-flight checks, such as verifying the database's `wait_timeout`
//!   setting to prevent unexpected connection drops.
//!
//! - **Generic Helper Functions**: A collection of private and public static methods like
//!   `get_single_string_from_database`, `get_single_integer_from_database`, and
//!   `get_timestamp`. These functions encapsulate common SQL query patterns (e.g.,
//!   fetching a single value, fetching an optional value) and provide consistent
//!   error handling.
//!
//! - **`ping_database()`**: A static method that executes a lightweight query (`SELECT 1`)
//!   to verify that a connection is still alive. This is the core implementation
//!   used by the `Pingable` trait to prevent connection timeouts.
//!
//! ## Design and Purpose
//!
//! The primary goals of this module are **efficiency, robustness, and centralization**.
//!
//! - **Connection Pooling**: By using `mysql::Pool`, the application avoids the high
//!   cost of opening and closing a new database connection for every operation. The pool
//!   manages a set of ready-to-use connections, which is essential for performance in a
//!   multithreaded environment.
//!
//! - **Startup Validation**: The `new()` constructor performs critical checks at startup,
//!   such as `check_wait_timeout` and `check_required_tables_existing`. This "fail-fast"
//!   approach ensures that the application does not start in a misconfigured or
//!   unstable state.
//!
//! - **Centralized Logic**: It centralizes the logic for creating connections and
//!   executing basic queries, preventing this boilerplate from being scattered across
//!   the application.

use chrono::*;
use mysql::prelude::*;
use mysql::*;

use crate::database::sql_interface_config::SqlInterfaceConfig;
use crate::database::sql_query_strings::{SQL_QUERY_PING, SQL_QUERY_READ_TIMEOUT};
pub use crate::database::{sql_interface_error::SqlInterfaceError, sql_query_strings};
use crate::database::sql_test_query_strings;
use crate::utilities::version_information::VersionInformation;

/// Struct is used to retrieve a single integer value from the SQL database.
struct SingleInteger {
    single_integer: i64,
}

/// Struct is used to retrieve a single string value from the SQL database.
pub struct SingleString {
    pub single_string: String,
}

/// Struct is used to retrieve a single float value from the SQL database.
struct SingleFloat {
    single_float: f64,
}

/// Holds the configuration and the implementation of the SQL interface.
pub struct SqlInterface {
    /// Configuration data read from the .toml file
    pub config: SqlInterfaceConfig,

    /// Connection pool
    pool: Pool,

    /// Connection to the database (public because accessed from within test cases)
    pub conn: PooledConn,
}

impl SqlInterface {
    /// Creates a new `SqlInterface` instance and establishes a connection pool
    /// to the SQL database based on the provided configuration.
    ///
    /// This function attempts to build a database connection URL from the
    /// `SqlInterfaceConfig` and then uses it to create a connection pool.
    /// It also retrieves an initial connection from the pool for the main thread.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the SQL interface, including
    ///   database user, password, host, port, and name.
    ///
    /// # Returns
    /// A `Result` containing a new `SqlInterface` instance on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The connection pool cannot be established (`SqlInterfaceError::ConnectionPoolFailure`), for example,
    ///   due to an invalid URL, incorrect credentials, or network issues.
    /// - An initial connection cannot be retrieved from the pool (`SqlInterfaceError::ConnectionFromPool`).
    /// - The database's `wait_timeout` setting is too low (`SqlInterfaceError::WaitTimeoutTooLow`).
    pub fn new(config: SqlInterfaceConfig) -> Result<SqlInterface, SqlInterfaceError> {
        // Create the connection pool
        let url = "mysql://".to_owned()
            + &config.db_user
            + ":"
            + &config.db_password
            + "@"
            + &config.db_host
            + ":"
            + &config.db_port.to_string()
            + "/"
            + config.db_name.as_str();
        let pool = match Pool::new(url.as_str()) {
            Ok(c) => c,
            Err(e) => {
                return Err(SqlInterfaceError::ConnectionPoolFailure {
                    location: module_path!().to_string(),
                    url,
                    source: e,
                });
            }
        };

        // Get connection for requests of the main thread, retrying in case of transient too many connections errors
        let mut conn_opt = None;
        let mut last_err = None;
        for i in 0..10 {
            match pool.get_conn() {
                Ok(c) => {
                    conn_opt = Some(c);
                    break;
                }
                Err(e) => {
                    last_err = Some(e);
                    if i < 9 {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                }
            }
        }
        let mut conn = conn_opt.ok_or_else(|| SqlInterfaceError::ConnectionFromPool {
            location: module_path!().to_string(),
            source: last_err.unwrap(),
        })?;

        // Call the refactored check_wait_timeout
        Self::check_wait_timeout(&mut conn, config.db_min_wait_timeout)?;

        Ok(SqlInterface { config, pool, conn })
    }

    /// Retrieves a new connection from the internal database connection pool.
    ///
    /// # Returns
    /// A `Result` containing a `PooledConn` object on success.
    ///
    /// # Errors
    /// This function will return an `Err(SqlInterfaceError::ConnectionFromPool)` if it
    /// fails to retrieve a database connection from the pool. This typically indicates
    /// a severe issue with database availability, pool exhaustion, or configuration.
    pub fn get_connection(&self) -> Result<PooledConn, SqlInterfaceError> {
        self.pool
            .get_conn()
            .map_err(|e| SqlInterfaceError::ConnectionFromPool {
                location: module_path!().to_string(),
                source: e,
            })
    }

    /// Executes a given SQL query and maps the results into a vector of `SingleString` structs.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    /// * `sql_query_string` - The SQL query string to be executed.
    ///
    /// # Returns
    /// A `Result` containing a vector of `SingleString` structs. The vector
    /// may be empty if the query returns no rows.
    ///
    /// # Errors
    /// This function will return an `(SqlInterfaceError::SingleStringRequestFailure)`
    /// if the database query execution fails for any reason, such as a syntax error
    /// in the SQL or a connection issue.
    pub fn get_string_array_from_database(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<Vec<SingleString>, SqlInterfaceError> {
        conn.query_map(sql_query_string, |single_string| SingleString {
            single_string,
        })
        .map_err(|e| SqlInterfaceError::SingleStringRequestFailure {
            location: module_path!().to_string(),
            query: sql_query_string.to_string(),
            source: e,
        })
    }

    /// Executes an SQL query and strictly expects a single string result.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    /// * `sql_query_string` - The SQL query designed to return exactly one string value.
    ///
    /// # Returns
    /// A `Result` containing the single `String` value on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The underlying database query fails.
    /// - The query returns zero rows (`SqlInterfaceError::SingleStringRequestEmptyResponse`).
    /// - The query returns more than one row (`SqlInterfaceError::SingleStringRequestNoSingleResponse`).
    fn get_single_string_from_database(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<String, SqlInterfaceError> {
        // Get a single string value from the database
        let single_string_array = Self::get_string_array_from_database(conn, sql_query_string)?;

        // check the error case if there is no response
        if single_string_array.is_empty() {
            return Err(SqlInterfaceError::SingleStringRequestEmptyResponse(
                module_path!().to_string(),
                sql_query_string.to_string(),
            ));
        }

        // check the error case if there is more than one response
        if single_string_array.len() > 1 {
            return Err(SqlInterfaceError::SingleStringRequestNoSingleResponse(
                module_path!().to_string(),
                sql_query_string.to_string(),
            ));
        }
        Ok(single_string_array[0].single_string.clone())
    }

    /// Retrieves the stored hash for a specific software version from the database.
    ///
    /// # Arguments
    /// * `version_information` - A reference to a `VersionInformation` struct.
    ///
    /// # Returns
    /// A `Result` containing the hash `String` associated with the specified version.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The database query fails (`SqlInterfaceError::SingleVersionRequestFailure`).
    /// - No hash is found for the given version (`SqlInterfaceError::SingleVersionRequestNotListed`).
    /// - More than one hash is found, indicating a data inconsistency (`SqlInterfaceError::SingleVersionRequestMultipleResults`).
    pub fn get_hash_from_database(
        &mut self,
        version_information: &VersionInformation,
    ) -> Result<String, SqlInterfaceError> {
        // Get a single string value from the database
        let params = params!(
            "major" => version_information.major,
            "minor" => version_information.minor,
            "build" => version_information.build,
        );

        let single_string_array = self
            .conn
            .exec_map(
                sql_query_strings::SQL_QUERY_VERSION_INFORMATION,
                params,
                |single_string| SingleString { single_string },
            )
            .map_err(|_| {
                SqlInterfaceError::SingleVersionRequestFailure(
                    module_path!().to_string(),
                    sql_query_strings::SQL_QUERY_VERSION_INFORMATION.to_string(),
                    version_information.major,
                    version_information.minor,
                    version_information.build,
                )
            })?;

        // check the error case if there is no response
        if single_string_array.is_empty() {
            return Err(SqlInterfaceError::SingleVersionRequestNotListed(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_VERSION_INFORMATION.to_string(),
                version_information.major,
                version_information.minor,
                version_information.build,
            ));
        }

        // check the error case if there is more than one response
        if single_string_array.len() > 1 {
            Err(SqlInterfaceError::SingleVersionRequestMultipleResults(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_VERSION_INFORMATION.to_string(),
                version_information.major,
                version_information.minor,
                version_information.build,
            ))
        } else {
            Ok(single_string_array[0].single_string.clone())
        }
    }

    /// Executes an SQL query and collects multiple string results into a vector.
    ///
    /// This function is specifically designed for queries expected to return **more than one**
    /// string value.
    ///
    /// # Arguments
    /// * `sql_query_string` - The SQL query intended to retrieve multiple string values.
    ///
    /// # Returns
    /// A `Result` containing a `Vec<String>` with all the string values retrieved from the database on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The underlying database query execution fails (`SqlInterfaceError::SingleStringRequestFailure`).
    /// - The query returns zero rows (`SqlInterfaceError::MultipleStringRequestEmptyResponse`).
    /// - The query returns only one row (`SqlInterfaceError::MultipleStringRequestSingleResponse`), as this function
    ///   strictly expects multiple results.
    fn get_multiple_strings_from_database(
        &mut self,
        sql_query_string: &str,
    ) -> Result<Vec<String>, SqlInterfaceError> {
        // Get multiple string values from the database
        let single_string_array =
            match self
                .conn
                .query_map(sql_query_string, |single_string| SingleString {
                    single_string,
                }) {
                Ok(c) => c,
                Err(e) => {
                    return Err(SqlInterfaceError::SingleStringRequestFailure {
                        location: module_path!().to_string(),
                        query: sql_query_string.to_string(),
                        source: e,
                    });
                }
            };

        // check if there is no response or only one row
        if single_string_array.is_empty() {
            return Err(SqlInterfaceError::MultipleStringRequestEmptyResponse(
                module_path!().to_string(),
                sql_query_string.to_string(),
            ));
        } else if single_string_array.len() == 1 {
            return Err(SqlInterfaceError::MultipleStringRequestSingleResponse(
                module_path!().to_string(),
                sql_query_string.to_string(),
            ));
        }

        let mut response_array: Vec<String> = Vec::new();

        for single_string_struct in single_string_array {
            response_array.push(single_string_struct.single_string);
        }
        Ok(response_array)
    }

    /// Executes an SQL query and strictly expects a single integer result.
    ///
    /// This function is designed for queries that should return **exactly one**
    /// integer value. It will return an error if the query yields no results
    /// or more than one result.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    /// * `sql_query_string` - The SQL query designed to return precisely one integer.
    ///
    /// # Returns
    /// A `Result` containing `i64`: The unique integer value retrieved from the database.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The database query returns zero rows or more than one row (`SqlInterfaceError::SingleIntegerRequestNoSingleResponse)`.
    /// - The database query execution fails.`(SqlInterfaceError::SingleIntegerRequestFailure)`.
    pub fn get_single_integer_from_database(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<i64, SqlInterfaceError> {
        // Get a single integer value from the database
        let single_integer_array = match conn.query_map(sql_query_string, |single_integer| {
            SingleInteger { single_integer }
        }) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::SingleIntegerRequestFailure(
                    module_path!().to_string(),
                    sql_query_string.to_string(),
                ));
            }
        };

        // check the error case if there is no response
        if single_integer_array.is_empty() {
            return Err(SqlInterfaceError::SingleIntegerRequestNoSingleResponse(
                module_path!().to_string(),
                sql_query_string.to_string(),
            ));
        }

        // check the error case if there is more than one response
        if single_integer_array.len() > 1 {
            return Err(SqlInterfaceError::SingleIntegerRequestNoSingleResponse(
                module_path!().to_string(),
                sql_query_string.to_string(),
            ));
        }
        Ok(single_integer_array[0].single_integer)
    }

    /// Executes an SQL query and strictly expects a single floating-point value.
    ///
    /// This function is tailored for queries that should return **exactly one**
    /// floating-point number (`f64`). It will return an error if the query
    /// results in zero rows or more than one row.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    /// * `sql_query_string` - The SQL query designed to return a single `f64` value.
    ///
    /// # Returns
    /// A `Result` which contains `f64`: The unique floating-point value retrieved from the database.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The database query returns no rows or more than one row (`(SqlInterfaceError::SingleFloatRequestNoSingleResponse)`.
    /// - The database query execution encounters an error `(SqlInterfaceError::SingleFloatRequestFailure`).
    pub fn get_single_float_from_database(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<f64, SqlInterfaceError> {
        // Get a single float value from the database
        let single_float_array = match conn.query_map(sql_query_string, |single_float| {
            SingleFloat { single_float }
        }) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::SingleFloatRequestFailure(
                    module_path!().to_string(),
                    sql_query_string.to_string(),
                ));
            }
        };

        // check the error case if there is no response
        if single_float_array.is_empty() {
            return Err(SqlInterfaceError::SingleFloatRequestEmptyResponse(
                module_path!().to_string(),
                sql_query_string.to_string(),
            ));
        }

        // check the error case if there is more than one response
        if single_float_array.len() > 1 {
            return Err(SqlInterfaceError::SingleFloatRequestNoSingleResponse(
                module_path!().to_string(),
                sql_query_string.to_string(),
            ));
        }
        Ok(single_float_array[0].single_float)
    }

    /// Retrieves the name of the currently connected SQL database.
    ///
    /// This function executes an internal SQL query to fetch the database name.
    ///
    /// # Returns
    /// A `Result` which `String`: The name of the database as a string.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - there is any issue retrieving the database name, such as a communication
    ///   error or an unexpected response from the database `(SqlInterfaceError::DatabaseNameRequestFailure)`.
    pub fn get_database(&mut self) -> Result<String, SqlInterfaceError> {
        // trigger SQL call
        Self::get_single_string_from_database(&mut self.conn, sql_query_strings::SQL_QUERY_DATABASE)
            .map_err(|e| SqlInterfaceError::DatabaseNameRequestFailure {
                location: module_path!().to_string(),
                source: Box::new(e),
            })
    }

    /// Executes an SQL query to retrieve a single timestamp string and converts it to `NaiveDateTime`.
    ///
    /// This function fetches a single timestamp from the database of type `NaiveDateTime`.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    /// * `sql_query_string` - The SQL query to retrieve the timestamp as a string.
    ///
    /// # Returns
    /// A `Result` containing `NaiveDateTime` on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The underlying database query to fetch the string fails (`SqlInterfaceError::TimestampRequestFailure`).
    ///   This can happen if the query is invalid, the connection is lost, or if the query
    ///   does not return at least one row.
    /// - The retrieved string cannot be parsed into a valid `NaiveDateTime`
    ///   (`SqlInterfaceError::TimestampConversionFailure`). This indicates the database
    ///   returned a string in an unexpected format.
    pub fn get_timestamp(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<NaiveDateTime, SqlInterfaceError> {
        // request timestamp from database
        let timestamp_opt: Option<NaiveDateTime> =
            conn.query_first(sql_query_string).map_err(|_| {
                SqlInterfaceError::TimestampRequestFailure {
                    location: module_path!().to_string(),
                    query: sql_query_string.to_string(),
                }
            })?;

        // Check if we got a result and extract the value.
        match timestamp_opt {
            Some(timestamp) => Ok(timestamp),
            None => Err(SqlInterfaceError::TimestampRequestFailure {
                location: module_path!().to_string(),
                query: sql_query_string.to_string(),
            }),
        }
    }

    /// Executes an SQL query to retrieve an optional timestamp and converts it to `NaiveDateTime`.
    ///
    /// This function is designed for queries that might return a single timestamp, but where an
    /// empty result is also a valid outcome (e.g., checking for the last event in a log that might be empty).
    /// It fetches a string (if present, expected in "YYYY-MM-DD HH:MM:SS" format)
    /// and attempts to parse it.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    /// * `sql_query_string` - The SQL query to retrieve the optional timestamp as a string.
    ///
    /// # Returns
    /// A `Result` containing an `Option<NaiveDateTime>` on success:
    /// - `Ok(Some(NaiveDateTime))`: If the query returns exactly one row with a parsable timestamp string.
    /// - `Ok(None)`: If the query returns no rows.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The underlying database query fails for reasons like a syntax error or connection issue.
    ///   The original error is wrapped in `SqlInterfaceError::OptionalTimestampRequestFailure`.
    /// - The query returns more than one row, which is considered a data inconsistency. This is also
    ///   wrapped in `SqlInterfaceError::OptionalTimestampRequestFailure`.
    /// - The retrieved string cannot be parsed into a valid `NaiveDateTime`, which returns
    ///   `SqlInterfaceError::TimestampConversionFailure`. This indicates the database
    ///   returned a string in an unexpected format.
    pub fn get_optional_timestamp(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<Option<NaiveDateTime>, SqlInterfaceError> {
        // Request (optional) timestamp from the database.
        conn.query_first(sql_query_string).map_err(|_| {
            SqlInterfaceError::OptionalTimestampRequestFailure {
                location: module_path!().to_string(),
                query: sql_query_string.to_string(),
            }
        })
    }

    /// Fetches the current timestamp directly from the database.
    ///
    /// This function executes a database query to get the database server's current timestamp
    /// and returns it as a `NaiveDateTime`.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    ///
    /// # Returns
    /// A `Result` containing the current `NaiveDateTime` from the database on success.
    ///
    /// # Errors
    /// This function will return an `Err` variant of `SqlInterfaceError` if:
    /// - The underlying database query to fetch the timestamp string fails. This can be due to
    ///   a connection issue, an invalid query, or if the query does not return exactly one row.
    ///   The original error is wrapped in `SqlInterfaceError::TimestampRequestFailure`.
    /// - The timestamp string returned by the database cannot be parsed into a `NaiveDateTime`.
    ///   This indicates a data format mismatch and returns `SqlInterfaceError::TimestampConversionFailure`.
    pub fn get_current_timestamp(
        conn: &mut PooledConn,
    ) -> Result<NaiveDateTime, SqlInterfaceError> {
        Self::get_timestamp(conn, sql_query_strings::SQL_QUERY_READ_CURRENT_TIMESTAMP)
    }

    /// Executes an SQL query to retrieve a single date of type `NaiveDate`.
    ///
    /// This function fetches a date from the database.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    /// * `sql_query_string` - The SQL query designed to retrieve the date as a string.
    ///
    /// # Returns
    /// A `Result` containing the successfully parsed `NaiveDate` on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The underlying database query to fetch the string fails (`SqlInterfaceError::DateRequestFailure`).
    ///   This can happen if the query is invalid, the connection is lost, or if the query
    ///   does not return exactly one row.
    pub fn get_date(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<NaiveDate, SqlInterfaceError> {
        let date_opt: Option<(NaiveDate,)> = conn.query_first(sql_query_string).map_err(|_| {
            SqlInterfaceError::DateRequestFailure {
                location: module_path!().to_string(),
                query: sql_query_string.to_string(),
            }
        })?;

        // Handle the case where the query returned no rows.
        date_opt
            .map(|(date,)| date) // Extracts the NaiveDate from the tuple `(NaiveDate)`
            .ok_or_else(|| {
                // Create the specific error for an empty response, just like the old code did.
                SqlInterfaceError::DateRequestFailure {
                    location: module_path!().to_string(),
                    query: sql_query_string.to_string(),
                }
            })
    }

    /// Fetches the current calendar date directly from the database.
    ///
    /// This function executes a standard SQL query to get the database server's current date
    /// and returns it as a `NaiveDate`.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    ///
    /// # Returns
    /// A `Result` containing the current `NaiveDate` from the database on success.
    ///
    /// # Errors
    /// This function will return an `Err` variant of `SqlInterfaceError` if:
    /// - The underlying database query to fetch the date string fails. This can be due to
    ///   a connection issue, an invalid query, or if the query does not return exactly one row.
    ///   The original error is wrapped in `SqlInterfaceError::DateRequestFailure`.
    /// - The date string returned by the database cannot be parsed into a `NaiveDate`.
    ///   This indicates a data format mismatch and returns `SqlInterfaceError::TimestampConversionFailure`.
    pub fn get_current_date(conn: &mut PooledConn) -> Result<NaiveDate, SqlInterfaceError> {
        Self::get_date(conn, sql_query_strings::SQL_QUERY_READ_CURRENT_DATE)
    }

    /// Fetches the calendar date for the following day directly from the database.
    ///
    /// This function executes a standard SQL query to get the database server's date for "tomorrow"
    /// and returns it as a `NaiveDate`.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    ///
    /// # Returns
    /// A `Result` containing tomorrow's `NaiveDate` from the database on success.
    ///
    /// # Errors
    /// This function will return an `Err` variant of `SqlInterfaceError` if:
    /// - The underlying database query to fetch the date string fails. This can be due to
    ///   a connection issue, an invalid query, or if the query does not return exactly one row.
    ///   The original error is wrapped in `SqlInterfaceError::DateRequestFailure`.
    /// - The date string returned by the database cannot be parsed into a `NaiveDate`.
    ///   This indicates a data format mismatch and returns `SqlInterfaceError::TimestampConversionFailure`.
    pub fn get_tomorrow_date(conn: &mut PooledConn) -> Result<NaiveDate, SqlInterfaceError> {
        Self::get_date(conn, sql_query_strings::SQL_QUERY_READ_TOMORROW_DATE)
    }

    /// Fetches the names of all tables present in the connected SQL database.
    ///
    /// This function executes a standard SQL query to list all tables.
    ///
    /// # Returns
    /// A `Result` containing a `Vec<String>` with the names of all tables found in the database.
    /// The vector may be empty if the database contains no tables.
    ///
    /// # Errors
    /// This function will return an `Err` if the underlying database query to list tables
    /// fails. The specific error will likely be `SqlInterfaceError::SingleStringRequestFailure`,
    /// which can be caused by a connection issue, lack of permissions, or other
    /// database-side errors.
    pub fn get_tables(&mut self) -> Result<Vec<String>, SqlInterfaceError> {
        match self.get_multiple_strings_from_database(sql_query_strings::SQL_QUERY_READ_TABLES) {
            Ok(c) => Ok(c),
            Err(_) => Err(SqlInterfaceError::ShowTablesWithoutProperResult(
                module_path!().to_string(),
            )),
        }
    }

    /// Determines if all required table names are present within a list of existing tables.
    ///
    /// This private helper function checks if every string in `required_tables`
    /// can be found within `existing_tables`. The comparison is case-sensitive.
    ///
    /// # Arguments
    /// * `required_tables` - A `Vec<String>` containing the names of tables that
    ///   are expected to exist.
    /// * `existing_tables` - A `Vec<String>` containing the names of tables
    ///   actually found in the database.
    ///
    /// # Returns
    /// `true` if every table name in `required_tables` is found in `existing_tables`;
    /// `false` otherwise.  
    fn compare_required_tables_existing_tables(
        required_tables: Vec<String>,
        existing_tables: Vec<String>,
    ) -> bool {
        for required_table in required_tables {
            if !existing_tables.contains(&required_table) {
                return false;
            }
        }
        true
    }

    /// Verifies that all tables specified in the configuration exist in the database.
    ///
    /// This function first retrieves a list of all tables currently in the database.
    /// It then compares this list against the `db_tables` defined in the
    /// `SqlInterfaceConfig` to ensure all required tables are present.
    ///
    /// # Returns
    /// A `Result` which is `Ok(())` on success, indicating that all tables listed in the
    /// configuration were found in the database.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - One or more required tables are missing from the database. This will result in
    ///   an `SqlInterfaceError::RequiredTablesNotExisting` error, which includes the
    ///   list of required tables and the list of tables that were actually found.
    /// - The underlying call to `get_tables` fails. This will propagate the error,
    ///   which could be due to a database connection issue or insufficient permissions.
    pub fn check_required_tables_existing(&mut self) -> Result<(), SqlInterfaceError> {
        let existing_tables = self.get_tables()?;

        if !Self::compare_required_tables_existing_tables(
            self.config.db_tables.clone(),
            existing_tables.clone(),
        ) {
            let required_tables_string = self.config.db_tables.join(",");
            let existing_tables_string = existing_tables.join(",");
            Err(SqlInterfaceError::RequiredTablesNotExisting(
                module_path!().to_string(),
                required_tables_string,
                existing_tables_string,
            ))
        } else {
            Ok(())
        }
    }

    /// Checks if the database's `wait_timeout` setting meets a minimum required value.
    ///
    /// This function retrieves the current `wait_timeout` from the connected database
    /// and compares it against a `min_wait_timeout` provided by the application.
    /// It is primarily used to ensure that database connections do not time out
    /// prematurely from the server's perspective, which could lead to "Broken pipe" errors.
    /// The function is called in the initialization phase of the control application.
    ///
    /// # Arguments
    ///
    /// * `conn` - A mutable reference to a `PooledConn` (a database connection from a pool).
    ///   This connection is used to execute the SQL query to fetch the `wait_timeout`.
    /// * `min_wait_timeout` - The minimum acceptable `wait_timeout` value (in seconds)
    ///   that the application requires for stable operation.
    ///
    /// # Errors
    ///
    /// This function will return an error in the following scenarios:
    /// - If it fails to retrieve the `wait_timeout` value from the database (e.g., due to
    ///   connection issues, query execution errors).
    /// - If the retrieved `wait_timeout` value from the database is not a valid unsigned 64-bit integer
    ///   (e.g., if the database returns a value that cannot be converted to `u64`). This indicates
    ///   an unexpected data type or format from the database.
    /// - If the `current_wait_timeout` read from the database is less than `min_wait_timeout`.
    ///   In this case, the application will return an error with a message indicating the discrepancy,
    ///   as it considers an insufficient `wait_timeout` a critical configuration error.
    pub fn check_wait_timeout(
        conn: &mut PooledConn,
        min_wait_timeout: u64,
    ) -> Result<(), SqlInterfaceError> {
        let current_wait_timeout_i64 =
            Self::get_single_integer_from_database(conn, SQL_QUERY_READ_TIMEOUT)?;

        let current_wait_timeout: u64 = current_wait_timeout_i64.try_into().map_err(|_| {
            SqlInterfaceError::WaitTimeoutInvalid(
                module_path!().to_string(),
                current_wait_timeout_i64,
            )
        })?;

        if current_wait_timeout < min_wait_timeout && min_wait_timeout != 0 {
            return Err(SqlInterfaceError::WaitTimeoutTooLow(
                module_path!().to_string(),
                current_wait_timeout,
                min_wait_timeout,
            ));
        }
        Ok(())
    }

    /// Pings the database to check connection health and prevent timeouts.
    ///
    /// This function executes a lightweight SQL query (`SQL_QUERY_PING`, typically `SELECT 1;`)
    /// against the provided database connection. Its primary purpose is to:
    /// 1. **Keep the connection alive:** By sending a query, it resets the database server's
    ///    idle timeout counters (`wait_timeout`), preventing the
    ///    server from unilaterally closing the connection due to inactivity.
    /// 2. **Verify connection status: ** A successful ping indicates that the connection
    ///    is still active and able to communicate with the database.
    ///
    /// If the ping query fails (e.g., due to a broken connection, network issue, or
    /// database server unavailability), an error is logged, and an `Err` is returned.
    ///
    /// # Arguments
    ///
    /// * `conn` - A mutable reference to a `PooledConn`, representing an active
    ///   database connection obtained from a connection pool.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) on success, indicating the connection is healthy.
    ///
    /// # Errors
    /// This function will return an `Err(SqlInterfaceError::DatabasePingFailure)` if the
    /// ping query fails. This typically indicates that the connection is no longer valid,
    /// which could be caused by:
    /// - A network interruption between the application and the database server.
    /// - The database server being shut down or restarted.
    /// - The connection being explicitly closed by the server due to exceeding the
    ///   `wait_timeout` or other resource limits.
    pub fn ping_database(conn: &mut PooledConn) -> Result<(), SqlInterfaceError> {
        SqlInterface::get_single_integer_from_database(conn, SQL_QUERY_PING)
            .map_err(|e| SqlInterfaceError::DatabasePingFailure {
                location: module_path!().to_string(),
                source: Box::new(e),
            })
            .map(|_| ()) // On success, transform the i64 into a unit type
    }

    //#[cfg(test)]
    /// This helper function **empties the specified database table** using a TRUNCATE command.
    /// The function is only used in test environments across different crates to ensure a clean state
    /// before running tests.
    ///
    /// Arguments:
    /// * `sql_interface`: A mutable reference to the main SQL interface.
    /// * `sql_table_name`: The name of the table to be emptied.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) on successful truncation of the table.
    ///
    /// # Errors
    /// This function will return an `Err(SqlInterfaceError::TruncateTableFailure)` if the
    /// `TRUNCATE TABLE` command fails. This can happen for several reasons, including
    /// - The specified table does not exist.
    /// - The database user lacks the necessary permissions to truncate the table.
    /// - The database connection has been lost.
    pub fn truncate_table(
        sql_interface: &mut SqlInterface,
        sql_table_name: String,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_string = str::replace(
            sql_test_query_strings::SQL_QUERY_TRUNCATE_TABLE,
            "#table",
            &sql_table_name,
        );
        match sql_interface.conn.query_drop(sql_query_string) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("{e:?}");
                Err(SqlInterfaceError::TruncateTableFailure(
                    module_path!().to_string(),
                    sql_table_name,
                ))
            }
        }
    }

    #[cfg(test)]
    /// This test-only function calculates a `NaiveDateTime` that is a specific duration
    /// in the past relative to the current local time. This function is used for setting up
    /// historical data scenarios in tests across different crates.
    ///
    /// Arguments:
    /// * `hours_in_past`: The number of hours back in time.
    /// * `seconds_in_past`: The number of additional seconds back in time.
    ///
    /// Returns:
    /// A `NaiveDateTime` representing the calculated past timestamp.
    pub fn get_naive_timestamp_from_past(
        hours_in_past: i64,
        seconds_in_past: i64,
    ) -> NaiveDateTime {
        let timestamp_from_past =
            Local::now() - Duration::hours(hours_in_past) - Duration::seconds(seconds_in_past);
        timestamp_from_past.naive_local()
    }

    #[cfg(test)]
    /// This test-only function calculates the time difference in seconds between a
    /// provided `NaiveDateTime` (expected to be in the past) and the current local time.
    ///
    /// Arguments:
    /// * `timestamp_naive`: The past timestamp from which the duration will be measured.
    ///
    /// Returns:
    /// `Some(Duration)` representing the duration in seconds if the conversion and calculation
    /// are successful. Returns `None` if the `timestamp_naive` cannot be unambiguously
    /// converted to a local `DateTime`.
    pub fn get_duration_from_naive_timestamp_to_local_now(
        timestamp_naive: NaiveDateTime,
    ) -> Option<Duration> {
        use chrono::LocalResult::{Ambiguous, None, Single};

        let reference_current_timestamp = Local::now();
        let timestamp_dt: DateTime<Local> = match Local.from_local_datetime(&timestamp_naive) {
            Single(t) => t,
            Ambiguous(_t1, _t2) => {
                return Option::None;
            }
            None => {
                return Option::None;
            }
        };
        Some(reference_current_timestamp.signed_duration_since(timestamp_dt))
    }
}