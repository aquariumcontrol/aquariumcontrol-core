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
use crate::database::{sql_interface_error::SqlInterfaceError, sql_query_strings};
use crate::utilities::version_information::VersionInformation;

/// Struct is used to retrieve a single integer value from the SQL database.
struct SingleInteger {
    single_integer: i64,
}

/// Struct is used to retrieve a single string value from the SQL database.
struct SingleString {
    single_string: String,
}

/// Struct is used to retrieve a single float value from the SQL database.
struct SingleFloat {
    single_float: f64,
}

/// Holds the configuration and the implementation of the SQL interface.
pub struct SqlInterface {
    /// Configuration data read from the .toml file
    config: SqlInterfaceConfig,

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
    fn get_string_array_from_database(
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

    #[cfg(test)]
    /// Retrieves a timestamp from the database with a specified future or past offset.
    ///
    /// This function is exclusively for use in test environments. It queries the database
    /// for its current timestamp and then adds a given number of seconds to it, effectively
    /// simulating a future or past point in time for testing scenarios.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    /// * `offset_seconds` - The number of seconds to add to the current database timestamp.
    ///   A positive value shifts the timestamp into the future, while a negative value
    ///   shifts it into the past.
    ///
    /// # Returns
    /// A `Result` containing the calculated `NaiveDateTime` (offset by the given seconds) on success.
    ///
    /// # Errors
    /// This function will return an `Err` variant of `SqlInterfaceError` if:
    /// - The underlying database query to fetch the timestamp string fails. This can be due to
    ///   a connection issue, an invalid query, or if the query does not return exactly one row.
    ///   The original error is wrapped in `SqlInterfaceError::TimestampRequestFailure`.
    /// - The timestamp string returned by the database cannot be parsed into a `NaiveDateTime`.
    ///   This indicates a data format mismatch and returns `SqlInterfaceError::TimestampConversionFailure`.
    pub fn get_current_timestamp_offset_seconds(
        conn: &mut PooledConn,
        offset_seconds: i32,
    ) -> Result<NaiveDateTime, SqlInterfaceError> {
        let offset_seconds_string = offset_seconds.to_string();
        let sql_query_with_offset_resolved = str::replace(
            sql_query_strings::SQL_QUERY_CURRENT_TIMESTAMP_OFFSET_SECONDS,
            "#offset",
            &offset_seconds_string,
        );
        Self::get_timestamp(conn, &sql_query_with_offset_resolved)
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
    fn get_date(
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

    #[cfg(test)]
    // This helper function **empties the specified database table** using a TRUNCATE command.
    // The function is only used in test environments across different crates to ensure a clean state
    // before running tests.
    //
    // Arguments:
    // * `sql_interface`: A mutable reference to the main SQL interface.
    // * `sql_table_name`: The name of the table to be emptied.
    //
    /// # Returns
    /// An empty `Result` (`Ok(())`) on successful truncation of the table.
    ///
    /// # Errors
    /// This function will return an `Err(SqlInterfaceError::TruncateTableFailure)` if the
    /// `TRUNCATE TABLE` command fails. This can happen for several reasons, including
    /// - The specified table does not exist.
    /// - The database user lacks the necessary permissions to truncate the table.
    /// - The database connection has been lost.
    ///
    /// # Arguments
    /// * `sql_interface` - A parameter representing `sql_interface`.
    /// * `sql_table_name` - A parameter representing `sql_table_name`.
    pub fn truncate_table(
        sql_interface: &mut SqlInterface,
        sql_table_name: String,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_string = str::replace(
            sql_query_strings::SQL_QUERY_TRUNCATE_TABLE,
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
    // This test-only function calculates a `NaiveDateTime` that is a specific duration
    // in the past relative to the current local time. This function is used for setting up
    // historical data scenarios in tests across different crates.
    //
    // Arguments:
    // * `hours_in_past`: The number of hours back in time.
    // * `seconds_in_past`: The number of additional seconds back in time.
    //
    // Returns:
    // A `NaiveDateTime` representing the calculated past timestamp.
    /// Retrieves the naive timestamp from past.
    ///
    /// # Arguments
    /// * `hours_in_past` - A parameter representing `hours_in_past`.
    /// * `seconds_in_past` - A parameter representing `seconds_in_past`.
    pub fn get_naive_timestamp_from_past(
        hours_in_past: i64,
        seconds_in_past: i64,
    ) -> NaiveDateTime {
        let timestamp_from_past =
            Local::now() - Duration::hours(hours_in_past) - Duration::seconds(seconds_in_past);
        timestamp_from_past.naive_local()
    }

    #[cfg(test)]
    // This test-only function calculates the time difference in seconds between a
    // provided `NaiveDateTime` (expected to be in the past) and the current local time.
    //
    // Arguments:
    // * `timestamp_naive`: The past timestamp from which the duration will be measured.
    //
    // Returns:
    // `Some(Duration)` representing the duration in seconds if the conversion and calculation
    // are successful. Returns `None` if the `timestamp_naive` cannot be unambiguously
    // converted to a local `DateTime`.
    /// Retrieves the duration from naive timestamp to local now.
    ///
    /// # Arguments
    /// * `timestamp_naive` - A parameter representing `timestamp_naive`.
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

#[cfg(test)]
pub mod tests {
    use chrono::{DateTime, Local, NaiveDate, SubsecRound, TimeZone};
    use mysql::params;
    use mysql::prelude::Queryable;

    use crate::database::sql_interface::{SqlInterface, SqlInterfaceError};
    use crate::database::sql_query_strings::SQL_QUERY_WRITE_VERSION_INFORMATION;
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};
    use crate::utilities::version_information::VersionInformation;

    // Helper function to insert mock version information into database
    pub fn insert_mock_version_information(sql_interface: &mut SqlInterface) {
        let unknown_version_information = VersionInformation {
            major: -1,
            minor: -1,
            build: -1,
            hash: "void".to_string(),
        };
        let sql_query_string = SQL_QUERY_WRITE_VERSION_INFORMATION.to_string();
        match sql_interface.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "major" => unknown_version_information.major,
            "minor" => unknown_version_information.minor,
            "build" => unknown_version_information.build,
            "hash" => unknown_version_information.hash.clone(),
            },
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Error when inserting version information into database: {e:?}");
            }
        };
    }

    // Helper function to truncate all tables (used in testing of run.rs)
    pub fn truncate_all_tables(sql_interface: &mut SqlInterface) {
        // List of all tables to truncate
        let tables = [
            "balling_log",
            "balling_set_vals",
            "data",
            "feed_log",
            "feed_profiles",
            "feed_schedule",
            "heating_stats",
            "refill_log",
            "schedule",
            "users",
            "ventilation_set_vals",
            "heating_set_vals",
            "version",
        ];

        // Truncate all tables in the test database
        for table in &tables {
            let _ = SqlInterface::truncate_table(sql_interface, table.to_string());
        }
    }

    #[test]
    // This test case checks the functionality of get_string_array_from_database.
    // - response when querying an empty table (should be Ok with an empty Vec)
    // - response when querying a table with one entry (should be Ok with a Vec of 1)
    // - response when querying a table with multiple entries (should be Ok with a Vec of n)
    // Test case uses test database #56.
    fn test_get_string_array_from_database() {
        // Setup
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            56,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut conn = sql_interface.get_connection().unwrap();

        let table_name = "test_string_array";

        // Create a temporary table for the test
        conn.query_drop(format!(
            "CREATE TABLE IF NOT EXISTS {} (name VARCHAR(255));",
            table_name
        ))
        .unwrap();

        // --- Test Case 1: Empty table ---
        println!("* Testing string array with empty table...");
        conn.query_drop(format!("TRUNCATE TABLE {}", table_name))
            .unwrap();

        let result = SqlInterface::get_string_array_from_database(
            &mut conn,
            &format!("SELECT name FROM {}", table_name),
        );
        let result_vec = result.unwrap();
        assert!(
            result_vec.is_empty(),
            "Expected an empty vector for an empty table"
        );
        println!("* Succeeded: Empty table returns an empty Vec.");

        // --- Test Case 2: Table with one entry ---
        println!("* Testing string array with a single entry...");
        let test_string_1 = "Hello";
        conn.exec_drop(
            format!("INSERT INTO {} (name) VALUES (:name)", table_name),
            params! { "name" => test_string_1 },
        )
        .unwrap();

        let result = SqlInterface::get_string_array_from_database(
            &mut conn,
            &format!("SELECT name FROM {}", table_name),
        );
        let result_vec = result.unwrap();
        assert_eq!(result_vec.len(), 1);
        assert_eq!(result_vec[0].single_string, test_string_1);
        println!("* Succeeded: Table with one entry returns a Vec with one element.");

        // --- Test Case 3: Table with multiple entries ---
        println!("* Testing string array with multiple entries...");
        let test_string_2 = "World";
        conn.exec_drop(
            format!("INSERT INTO {} (name) VALUES (:name)", table_name),
            params! { "name" => test_string_2 },
        )
        .unwrap();

        // Use ORDER BY to ensure a predictable order for assertions
        let result = SqlInterface::get_string_array_from_database(
            &mut conn,
            &format!("SELECT name FROM {} ORDER BY name ASC", table_name),
        );
        let result_vec = result.unwrap();
        assert_eq!(result_vec.len(), 2);
        assert_eq!(result_vec[0].single_string, test_string_1); // "Hello"
        assert_eq!(result_vec[1].single_string, test_string_2); // "World"
        println!("* Succeeded: Table with multiple entries returns a Vec with all elements.");

        // Cleanup
        conn.query_drop(format!("DROP TABLE {}", table_name))
            .unwrap();
    }

    #[test]
    // This test case checks the functionality of get_optional_timestamp.
    // - response when querying an empty table (should be Ok(None))
    // - response when querying a table with one entry (should be Ok(Some(timestamp)))
    // - error response when querying a table with multiple entries
    // Test case uses test database #56.
    fn test_get_optional_timestamp() {
        // Setup
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            56,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut conn = sql_interface.get_connection().unwrap();

        let table_name = "test_optional_timestamp";

        // Create a temporary table for the test
        conn.query_drop(format!(
            "CREATE TABLE IF NOT EXISTS {} (ts TIMESTAMP);",
            table_name
        ))
        .unwrap();

        // --- Test Case 1: Empty table ---
        println!("* Testing optional timestamp with empty table...");
        conn.query_drop(format!("TRUNCATE TABLE {}", table_name))
            .unwrap();

        let result = SqlInterface::get_optional_timestamp(
            &mut conn,
            &format!("SELECT ts FROM {}", table_name),
        );
        assert_eq!(result.unwrap(), None);
        println!("* Succeeded: Empty table returns Ok(None).");

        // --- Test Case 2: Table with one entry ---
        println!("* Testing optional timestamp with a single entry...");
        let test_timestamp = Local::now().naive_local().trunc_subsecs(0);
        conn.exec_drop(
            format!("INSERT INTO {} (ts) VALUES (:ts)", table_name),
            params! { "ts" => test_timestamp },
        )
        .unwrap();

        let result = SqlInterface::get_optional_timestamp(
            &mut conn,
            &format!("SELECT ts FROM {}", table_name),
        );
        assert_eq!(result.unwrap(), Some(test_timestamp));
        println!("* Succeeded: Table with one entry returns Ok(Some(timestamp)).");

        // --- Test Case 3: Table with multiple entries ---
        println!("* Testing optional timestamp with multiple entries...");
        let another_timestamp = (Local::now() - chrono::Duration::minutes(5))
            .naive_local()
            .trunc_subsecs(0);
        conn.exec_drop(
            format!("INSERT INTO {} (ts) VALUES (:ts)", table_name),
            params! { "ts" => another_timestamp },
        )
        .unwrap();

        let result = SqlInterface::get_optional_timestamp(
            &mut conn,
            &format!("SELECT ts FROM {}", table_name),
        );
        assert_eq!(result.unwrap(), Some(test_timestamp));
        println!("* Succeeded: Table with multiple entries returns Ok(Some(timestamp)).");

        // Cleanup
        conn.query_drop(format!("DROP TABLE {}", table_name))
            .unwrap();
    }

    #[test]
    // This test case checks if current timestamp is local timestamp.
    // It does not modify any database.
    pub fn test_get_current_timestamp() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            28,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");

        // *** check if current timestamp is local timestamp ***************************************
        let sql_timestamp = match SqlInterface::get_current_timestamp(&mut sql_interface.conn) {
            Ok(c) => c,
            Err(_) => panic!("Could not get current timestamp from the database."),
        };
        let date_time: DateTime<Local> = Local.from_local_datetime(&sql_timestamp).unwrap();
        assert_eq!(date_time, Local::now().trunc_subsecs(0));
        println!("* checking current timestamp succeeded.");
        // *****************************************************************************************
    }

    #[test]
    // This test case checks the functionality of get_current_timestamp_offset_seconds.
    // It verifies that the function correctly returns timestamps in the future (positive offset),
    // in the past (negative offset), and at the present (zero offset).
    // Test case does not modify any database.
    fn test_get_current_timestamp_offset_seconds() {
        // Setup
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            56,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut conn = sql_interface.get_connection().unwrap();

        // --- Test Case 1: Positive offset (future) ---
        println!("* Testing timestamp with positive offset...");
        let offset_future: i64 = 3600; // 1 hour in the future
        let result_future =
            SqlInterface::get_current_timestamp_offset_seconds(&mut conn, offset_future as i32)
                .unwrap();
        let expected_future = Local::now().naive_local() + chrono::Duration::seconds(offset_future);
        let diff_future = (expected_future - result_future).num_seconds().abs();
        assert!(
            diff_future <= 2,
            "Future offset test failed. Expected ~{}, got {}. Difference: {}s",
            expected_future,
            result_future,
            diff_future
        );
        println!("* Succeeded: Positive offset returns a future timestamp.");

        // --- Test Case 2: Negative offset (past) ---
        println!("* Testing timestamp with negative offset...");
        let offset_past: i64 = -1800; // 30 minutes in the past
        let result_past =
            SqlInterface::get_current_timestamp_offset_seconds(&mut conn, offset_past as i32)
                .unwrap();
        let expected_past = Local::now().naive_local() + chrono::Duration::seconds(offset_past);
        let diff_past = (expected_past - result_past).num_seconds().abs();
        assert!(
            diff_past <= 2,
            "Past offset test failed. Expected ~{}, got {}. Difference: {}s",
            expected_past,
            result_past,
            diff_past
        );
        println!("* Succeeded: Negative offset returns a past timestamp.");

        // --- Test Case 3: Zero offset ---
        println!("* Testing timestamp with zero offset...");
        let offset_zero: i64 = 0;
        let result_zero =
            SqlInterface::get_current_timestamp_offset_seconds(&mut conn, offset_zero as i32)
                .unwrap();
        let expected_now = Local::now().naive_local();
        let diff_now = (expected_now - result_zero).num_seconds().abs();
        assert!(
            diff_now <= 2,
            "Zero offset test failed. Expected ~{}, got {}. Difference: {}s",
            expected_now,
            result_zero,
            diff_now
        );
        println!("* Succeeded: Zero offset returns the current timestamp.");
    }

    #[test]
    // This test case checks the functionality of the private `get_date` function
    // and its public wrappers.
    // - Verifies the error response when a query returns no rows.
    // - Verifies the correct response when a query returns exactly one row.
    // - Verifies that `get_current_date` and `get_tomorrow_date` work as expected.
    // Test case uses test database #56.
    fn test_get_date() {
        // Setup
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            56,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut conn = sql_interface.get_connection().unwrap();

        let table_name = "test_date_table";

        // Create a temporary table for the test
        conn.query_drop(format!(
            "CREATE TABLE IF NOT EXISTS {} (d DATE);",
            table_name
        ))
        .unwrap();

        // --- Test Case 1: Empty table (should error) ---
        println!("* Testing get_date with empty table...");
        conn.query_drop(format!("TRUNCATE TABLE {}", table_name))
            .unwrap();

        let result = SqlInterface::get_date(&mut conn, &format!("SELECT d FROM {}", table_name));

        // Assert that we get the specific error for an empty response.
        if let Err(SqlInterfaceError::DateRequestFailure { .. }) = result {
            // This is the expected error path.
        } else {
            panic!(
                "Expected DateRequestFailure for empty result, but got: {:?}",
                result
            );
        }
        println!("* Succeeded: Empty table returns a DateRequestFailure error.");

        // --- Test Case 2: Table with one entry ---
        println!("* Testing get_date with a single entry...");
        let test_date = NaiveDate::from_ymd_opt(2025, 7, 15).unwrap();
        conn.exec_drop(
            format!("INSERT INTO {} (d) VALUES (:d)", table_name),
            params! { "d" => test_date },
        )
        .unwrap();

        let result = SqlInterface::get_date(&mut conn, &format!("SELECT d FROM {}", table_name));
        assert_eq!(result.unwrap(), test_date);
        println!("* Succeeded: Table with one entry returns Ok(date).");

        // --- Test Case 3: Test public wrappers ---
        println!("* Testing get_current_date and get_tomorrow_date wrappers...");
        let current_date_from_db = SqlInterface::get_current_date(&mut conn).unwrap();
        let tomorrow_date_from_db = SqlInterface::get_tomorrow_date(&mut conn).unwrap();

        let local_today = Local::now().date_naive();
        let local_tomorrow = local_today.succ_opt().unwrap();

        assert_eq!(current_date_from_db, local_today);
        assert_eq!(tomorrow_date_from_db, local_tomorrow);
        println!("* Succeeded: Public wrappers return correct dates.");

        // Cleanup
        conn.query_drop(format!("DROP TABLE {}", table_name))
            .unwrap();
    }

    #[test]
    // This test case checks
    // - if required tables are existing
    // Test case uses test database #28.
    pub fn test_sql_interface_required_tables_existing() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            28,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let database_name_from_config = config.sql_interface.db_name.clone();
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");

        let database_name_from_db = match sql_interface.get_database() {
            Ok(c) => c,
            Err(e) => panic!("Could not get database name: {e:?}"),
        };
        assert_eq!(database_name_from_db, database_name_from_config);
        println!(
            "* checking database name succeeded. database_name_from_db={}",
            database_name_from_db
        );

        // *** check if required tables are existing - *********************************************
        let result_check_required_tables_existing = sql_interface.check_required_tables_existing();
        if let Err(e) = result_check_required_tables_existing {
            panic!("* checking required tables failed: {e:?}")
        } else {
            println!("* checking required tables succeeded.");
        }
        // *****************************************************************************************

        // *** check if required tables are existing - *********************************************
        let dummy_table = "dummy".to_string();
        sql_interface.config.db_tables.push(dummy_table.clone());
        assert!(matches!(
            sql_interface.check_required_tables_existing(),
            Err(SqlInterfaceError::RequiredTablesNotExisting(_, _, _))
        ));
        println!("* checking required tables succeeded.");
        // *****************************************************************************************

        // *** check if the current date is local date *********************************************
        let sql_date =
            match SqlInterface::get_current_date(&mut sql_interface.get_connection().unwrap()) {
                Ok(c) => c,
                Err(_) => panic!("Could not get current date from the database."),
            };
        let sql_date_string = sql_date.format("%Y-%m-%d").to_string();
        let local_date_string = Local::now().trunc_subsecs(0).format("%Y-%m-%d").to_string();
        assert_eq!(sql_date_string, local_date_string);
        println!("* checking current date succeeded.");
        // *****************************************************************************************
    }

    #[test]
    // This test case verifies that `check_wait_timeout` successfully validates
    // that the database's `wait_timeout` setting is greater than or equal to
    // the minimum value specified in the configuration file.
    // Test case does not modify any database.
    fn test_check_wait_timeout_success() {
        // Setup
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            56,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let min_wait_timeout_from_config = config.sql_interface.db_min_wait_timeout;

        // We need a connection to run the check. The `new` function itself
        // already runs this check, but we call it explicitly for a focused test.
        let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut conn = sql_interface.get_connection().unwrap();

        // --- Test Execution ---
        println!(
            "* Testing check_wait_timeout with value from config: {}s",
            min_wait_timeout_from_config
        );
        let result = SqlInterface::check_wait_timeout(&mut conn, min_wait_timeout_from_config);

        // --- Assertion ---
        assert!(
            result.is_ok(),
            "check_wait_timeout failed, but was expected to succeed. Result: {:?}",
            result
        );
        println!("* Succeeded: Database wait_timeout is sufficient.");
    }

    #[test]
    // This test case checks if a zero value for the minimum wait timeout allows instantiation.
    pub fn test_sql_interface_min_wait_timeout_check_deactivated() {
        let mut config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            28,
        );
        config.sql_interface.db_min_wait_timeout = 0;
        println!("Testing with database {}", config.sql_interface.db_name);
        _ = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed with min wait timeout=0.");
    }

    #[test]
    // This test case checks if an excessively high value for the minimum wait timeout triggers an error.
    pub fn test_sql_interface_min_wait_timeout_check() {
        let mut config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            28,
        );
        config.sql_interface.db_min_wait_timeout = 1000000;
        println!("Testing with database {}", config.sql_interface.db_name);
        let result = SqlInterface::new(config.sql_interface);

        assert!(matches!(
            result,
            Err(SqlInterfaceError::WaitTimeoutTooLow(_, _, _))
        ));
    }

    #[test]
    // This test case checks
    // - error response when querying empty table
    // - error response when version information does not match entry in database
    // - check error response when the hash is incorrect
    // Test case uses test database #29.
    pub fn test_sql_interface_version() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            29,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");

        // *** check error response when querying empty table **************************************
        let version_information = VersionInformation::new().unwrap();
        match SqlInterface::truncate_table(&mut sql_interface, "version".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        match sql_interface.get_hash_from_database(&version_information) {
            Ok(_) => {
                panic!("Found version information in truncated table.");
            }
            Err(e) => {
                assert!(matches!(
                    e,
                    SqlInterfaceError::SingleVersionRequestNotListed(_, _, _, _, _)
                ));
            }
        }
        // *****************************************************************************************

        insert_mock_version_information(&mut sql_interface);

        // *** check error response when version information does not match entry in database ******
        match sql_interface.get_hash_from_database(&version_information) {
            Ok(_) => {
                panic!("Found correct version information although having inserted incorrect version information table.");
            }
            Err(e) => {
                assert!(matches!(
                    e,
                    SqlInterfaceError::SingleVersionRequestNotListed(_, _, _, _, _)
                ))
            }
        }

        // *** check response when the hash is correct ********************************************
        let version_information_with_wrong_hash = VersionInformation {
            major: version_information.major,
            minor: version_information.minor,
            build: version_information.build,
            hash: "wrong".to_string(),
        };
        let sql_query_string = SQL_QUERY_WRITE_VERSION_INFORMATION.to_string();
        match sql_interface.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "major" => version_information_with_wrong_hash.major,
            "minor" => version_information_with_wrong_hash.minor,
            "build" => version_information_with_wrong_hash.build,
            "hash" => version_information_with_wrong_hash.hash.clone(),
            },
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Error when inserting version information into database: {e:?}");
            }
        };
        match sql_interface.get_hash_from_database(&version_information) {
            Ok(c) => {
                let reference_hash = &version_information_with_wrong_hash.hash;
                assert_eq!(&c, reference_hash);
            }
            Err(e) => {
                panic!("Error retrieving version information from the database: {e:?}");
            }
        }
        // ***************************************************************************************
    }

    #[test]
    // This test case checks if the ping query can be executed
    pub fn test_sql_interface_ping() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            28,
        );
        let sql_interface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed");

        SqlInterface::ping_database(&mut sql_interface.get_connection().unwrap()).unwrap();
    }
}
