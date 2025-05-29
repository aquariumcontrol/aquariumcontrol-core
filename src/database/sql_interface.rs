/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
use chrono::LocalResult::{Ambiguous, None, Single};
use chrono::*;
use log::error;
use mysql::prelude::*;
use mysql::*;
use serde_derive::Deserialize;

use crate::database::{sql_interface_error::SqlInterfaceError, sql_query_strings};
use crate::database::sql_query_strings::SQL_QUERY_READ_TIMEOUT;
use crate::utilities::version_information::VersionInformation;

/// This struct holds the configuration data for the SQL interface.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Clone)]
pub struct SqlInterfaceConfig {
    /// database user
    pub db_user: String,

    /// database password
    pub db_password: String,

    /// hostname of the database
    pub db_host: String,

    /// port name of the database
    pub db_port: u16,

    /// database name
    pub db_name: String,

    /// array of required tables
    pub db_tables: Vec<String>,
    
    /// minimum wait timout - application checks server variable before execution
    pub db_min_wait_timeout: u64,
}

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
    const LOG_TARGET_SQL_INTERFACE: &'static str = "sql_interface";

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
    /// A `Result` which is:
    /// - `Ok(SqlInterface)` containing the newly created `SqlInterface`
    ///   with an established connection pool and an initial database connection.
    /// - `Err(SqlInterfaceError::ConnectionPoolFailure)` if the connection pool
    ///   cannot be established (e.g., invalid URL, network issues).
    ///
    /// # Panics
    /// This function will panic if it successfully creates a connection pool
    /// but then fails to retrieve an initial connection from that pool.
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
                return Err(SqlInterfaceError::ConnectionPoolFailure(
                    "Could not establish connection pool with ".to_owned()
                        + &url
                        + ": "
                        + &e.to_string(),
                ));
            }
        };

        // Get connection for requests of the main thread
        let mut conn = match pool.get_conn() {
            Ok(c) => c,
            Err(_) => {
                panic!("Could not open SQL data base connection.");
            }
        };

        Self::check_wait_timeout(&mut conn, config.db_min_wait_timeout);
        
        Ok(SqlInterface { config, pool, conn })
    }

    /// Retrieves a new connection from the internal database connection pool.
    ///
    /// This function is used to get a fresh connection for executing SQL queries.
    /// If the connection pool fails to provide a connection, the function will panic.
    ///
    /// # Returns
    /// A `PooledConn` object representing an active connection to the SQL database.
    ///
    /// # Panics
    /// This function will panic if it fails to retrieve a database connection
    /// from the established connection pool. This typically indicates a severe
    /// issue with database availability or configuration that prevents further operation.
    pub fn get_connection(&self) -> PooledConn {
        match self.pool.get_conn() {
            Ok(c) => c,
            Err(_) => {
                panic!("Could not open SQL data base connection.");
            }
        }
    }

    /// Executes a given SQL query and maps the results into a vector of `SingleString` structs.
    ///
    /// This private helper function is designed to fetch one or more string values
    /// from the database based on the provided SQL query. Each row returned
    /// by the query is expected to contain a single string value, which is then
    /// encapsulated in a `SingleString` struct.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    /// * `sql_query_string` - The SQL query string to be executed.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(Vec<SingleString>)` containing a vector of `SingleString` structs,
    ///   each holding a string value retrieved from the database. This vector
    ///   may be empty if the query returns no rows.
    /// - `Err(SqlInterfaceError::SingleStringRequestFailure)` if the database
    ///   query execution fails for any reason.
    fn get_string_array_from_database(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<Vec<SingleString>, SqlInterfaceError> {
        match conn.query_map(sql_query_string, |single_string| SingleString {
            single_string,
        }) {
            Ok(c) => Ok(c),
            Err(e) => {
                error!(target: Self::LOG_TARGET_SQL_INTERFACE, "SQL database query failed: {}", e);
                Err(SqlInterfaceError::SingleStringRequestFailure(
                    "Could not get string array from the database using query:".to_string()
                        + sql_query_string,
                    )
                )
            }
        }
    }

    /// Executes an SQL query and strictly expects a single string result.
    ///
    /// This function is a wrapper around `get_string_array_from_database` that
    /// enforces a single-row, single-column string response. It will return an
    /// error if the query yields no results or more than one result.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    /// * `sql_query_string` - The SQL query designed to return exactly one string value.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(String)` containing the single string value retrieved from the database.
    /// - `Err(SqlInterfaceError::SingleStringRequestNoSingleResponse)` if the query
    ///   returns zero rows or more than one row.
    /// - `Err(SqlInterfaceError::SingleStringRequestFailure)` if the underlying
    ///   database query execution fails.
    fn get_single_string_from_database(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<String, SqlInterfaceError> {
        // Get a single string value from the database
        let single_string_array = Self::get_string_array_from_database(conn, sql_query_string)?;

        // check the error case if there is no response
        if single_string_array.is_empty() {
            return Err(SqlInterfaceError::SingleStringRequestNoSingleResponse(
                "SQL database responded with empty response to query: ".to_string()
                    + sql_query_string,
            ));
        }

        // check the error case if there is more than one response
        if single_string_array.len() > 1 {
            return Err(SqlInterfaceError::SingleStringRequestNoSingleResponse(
                "SQL database responded with multiple responses to request to query:".to_string()
                    + sql_query_string,
            ));
        }
        Ok(single_string_array[0].single_string.clone())
    }

    /// Executes an SQL query and returns an optional single string value.
    ///
    /// This function is similar to `get_single_string_from_database`, but it
    /// explicitly permits the query to return no results, in which case it
    /// provides `Option::None`. It will, however, return an error if the
    /// query yields more than one result.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    /// * `sql_query_string` - The SQL query designed to return at most one string value.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(Some(String))` if the query returns exactly one string value.
    /// - `Ok(Option::None)` if the query returns no rows.
    /// - `Err(SqlInterfaceError::SingleStringRequestNoSingleResponse)` if the query
    ///   returns more than one row.
    /// - `Err(SqlInterfaceError::SingleStringRequestFailure)` if the underlying
    ///   database query execution fails.
    fn get_single_optional_string_from_database(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<Option<String>, SqlInterfaceError> {
        // Get a single string value from the database
        let single_string_array = Self::get_string_array_from_database(conn, sql_query_string)?;

        // return an empty response if there is no response from the database
        if single_string_array.is_empty() {
            return Ok(Option::None);
        }

        // check the error case if there is more than one response
        if single_string_array.len() > 1 {
            return Err(SqlInterfaceError::SingleStringRequestNoSingleResponse(
                "SQL database responded with multiple responses to request to query:".to_string()
                    + sql_query_string,
            ));
        }
        Ok(Some(single_string_array[0].single_string.clone()))
    }

    /// Retrieves the stored hash for a specific software version from the database.
    ///
    /// This function constructs a query using the provided `VersionInformation`
    /// (major, minor, build numbers) to fetch the corresponding hash.
    /// It strictly expects exactly one hash to be returned.
    ///
    /// # Arguments
    /// * `version_information` - A reference to a `VersionInformation` struct
    ///   containing the major, minor, and build numbers of the software version.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(String)` containing the hash associated with the specified version.
    /// - `Err(SqlInterfaceError::SingleVersionRequestFailure)` if the database query fails.
    /// - `Err(SqlInterfaceError::SingleVersionRequestNotListed)` if no hash is found
    ///   for the given version.
    /// - `Err(SqlInterfaceError::SingleVersionRequestMultipleResults)` if more than one
    ///   hash is found for the given version (indicating a potential data inconsistency).
    pub fn get_hash_from_database(
        &mut self,
        version_information: &VersionInformation,
    ) -> Result<String, SqlInterfaceError> {
        // Get a single string value from the database
        let sql_query_string_major_replaced = str::replace(
            sql_query_strings::SQL_QUERY_VERSION_INFORMATION,
            "#major",
            &version_information.major.to_string(),
        );
        let sql_query_string_minor_replaced = str::replace(
            &sql_query_string_major_replaced,
            "#minor",
            &version_information.minor.to_string(),
        );
        let sql_query_string = str::replace(
            &sql_query_string_minor_replaced,
            "#build",
            &version_information.build.to_string(),
        );

        let single_string_array = match self
            .conn
            .query_map(sql_query_string.clone(), |single_string| SingleString {
                single_string,
            }) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::SingleVersionRequestFailure(
                    "Could not get single string from the database using query:".to_string()
                        + &sql_query_string,
                ));
            }
        };

        // check the error case if there is no response
        if single_string_array.is_empty() {
            return Err(SqlInterfaceError::SingleVersionRequestNotListed(
                "SQL database responded with empty response to query: ".to_owned()
                    + &sql_query_string,
            ));
        }

        // check the error case if there is more than one response
        if single_string_array.len() > 1 {
            error!(
                target: Self::LOG_TARGET_SQL_INTERFACE,
                "SQL database responded with multiple responses to request to query ({})",
                sql_query_string
            );
            Err(SqlInterfaceError::SingleVersionRequestMultipleResults)
        } else {
            Ok(single_string_array[0].single_string.clone())
        }
    }

    /// Executes an SQL query and collects multiple string results into a vector.
    ///
    /// This function is specifically designed for queries expected to return **more than one**
    /// string value. It will return an error if the query yields zero or only one result.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    /// * `sql_query_string` - The SQL query intended to retrieve multiple string values.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(Vec<String>)` containing all the string values retrieved from the database.
    /// - `Err(SqlInterfaceError::SingleStringRequestNoSingleResponse)` if the query
    ///   returns zero rows or only one row (as it expects *multiple*).
    /// - `Err(SqlInterfaceError::SingleStringRequestFailure)` if the underlying
    ///   database query execution fails.
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
                Err(_) => {
                    return Err(SqlInterfaceError::SingleStringRequestFailure(
                        "Could not get multiple strings from the database using query:".to_string()
                            + sql_query_string,
                    ));
                }
            };

        // check if there is no response or only one row
        if single_string_array.len() <= 1 {
            return Err(SqlInterfaceError::SingleStringRequestNoSingleResponse(
                "SQL database responded with empty or single row response to query: ".to_string()
                    + sql_query_string,
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
    /// A `Result` which is:
    /// - `Ok(i64)`: The unique integer value retrieved from the database.
    /// - `Err(SqlInterfaceError::SingleIntegerRequestNoSingleResponse)`: If the query
    ///   returns zero rows or more than one row.
    /// - `Err(SqlInterfaceError::SingleIntegerRequestFailure)`: If the underlying
    ///   database query execution fails.
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
                    "Could not get single integer from the database using query:".to_string()
                        + sql_query_string,
                ));
            }
        };

        // check the error case if there is no response
        if single_integer_array.is_empty() {
            return Err(SqlInterfaceError::SingleIntegerRequestNoSingleResponse(
                "SQL database responded with empty response to query: ".to_string()
                    + sql_query_string,
            ));
        }

        // check the error case if there is more than one response
        if single_integer_array.len() > 1 {
            return Err(SqlInterfaceError::SingleIntegerRequestNoSingleResponse(
                "SQL database responded with multiple responses to request to query:".to_string()
                    + sql_query_string,
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
    /// A `Result` which is:
    /// - `Ok(f64)`: The unique floating-point value retrieved from the database.
    /// - `Err(SqlInterfaceError::SingleFloatRequestNoSingleResponse)`: If the query
    ///   returns no rows or more than one row.
    /// - `Err(SqlInterfaceError::SingleFloatRequestFailure)`: If the underlying
    ///   database query execution encounters an error.
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
                    "Could not get single float value from the database using query:".to_string()
                        + sql_query_string,
                ));
            }
        };

        // check the error case if there is no response
        if single_float_array.is_empty() {
            return Err(SqlInterfaceError::SingleFloatRequestNoSingleResponse(
                "SQL database responded with empty response to query: ".to_string()
                    + sql_query_string,
            ));
        }

        // check the error case if there is more than one response
        if single_float_array.len() > 1 {
            return Err(SqlInterfaceError::SingleFloatRequestNoSingleResponse(
                "SQL database responded with multiple responses to request to query:".to_string()
                    + sql_query_string,
            ));
        }
        Ok(single_float_array[0].single_float)
    }

    /// Retrieves the name of the currently connected SQL database.
    ///
    /// This function executes an internal SQL query to fetch the database name.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(String)`: The name of the database as a string.
    /// - `Err(SqlInterfaceError)`: If there's any issue retrieving the database name,
    ///   such as a communication error or an unexpected response from the database.
    pub fn get_database(&mut self) -> Result<String, SqlInterfaceError> {
        // trigger SQL call
        match Self::get_single_string_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_DATABASE,
        ) {
            Ok(c) => Ok(c),
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_SQL_INTERFACE,
                    "Could not retrieve database name ({e:?})"
                );
                Err(e)
            }
        }
    }

    /// Executes an SQL query to retrieve a single timestamp string and converts it to `NaiveDateTime`.
    ///
    /// This function fetches a single string from the database (expected to be in
    /// "YYYY-MM-DD HH:MM:SS" format) and then attempts to parse it into a `NaiveDateTime` object.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    /// * `sql_query_string` - The SQL query to retrieve the timestamp as a string.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(NaiveDateTime)`: The successfully parsed timestamp.
    /// - `Err(SqlInterfaceError::SingleStringRequestNoSingleResponse)`: If the SQL query
    ///   returns no result or more than one result.
    /// - `Err(SqlInterfaceError::SingleStringRequestFailure)`: If the SQL query itself fails.
    /// - `Err(SqlInterfaceError::TimestampConversionFailure)`: If the retrieved string
    ///   cannot be parsed into a valid `NaiveDateTime`.
    pub fn get_timestamp(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<NaiveDateTime, SqlInterfaceError> {
        // trigger SQL call
        let timestamp_string = match Self::get_single_string_from_database(conn, sql_query_string) {
            Ok(c) => c,
            Err(e) => {
                if !cfg!(test) {
                    error!(
                        target: Self::LOG_TARGET_SQL_INTERFACE,
                        "could not retrieve timestamp from the database ({e:?})"
                    );
                }
                return Err(e);
            }
        };

        // convert data type
        match NaiveDateTime::parse_from_str(timestamp_string.as_str(), "%Y-%m-%d %H:%M:%S") {
            Ok(c) => Ok(c),
            Err(_) => Err(SqlInterfaceError::TimestampConversionFailure(
                "Error when converting timestamp: ".to_string() + &timestamp_string,
            )),
        }
    }

    /// Executes an SQL query to retrieve an optional timestamp string and converts it to `NaiveDateTime`.
    ///
    /// This function is designed for queries that might return a timestamp, but an empty
    /// result set is also considered a valid scenario (returning `Option::None`).
    /// This is the case when searching for Balling dosing events.
    /// It fetches a single string (if present, expected in "YYYY-MM-DD HH:MM:SS" format)
    /// and attempts to parse it into a `NaiveDateTime`.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    /// * `sql_query_string` - The SQL query to retrieve the optional timestamp as a string.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(Some(NaiveDateTime))`: If the query returns exactly one timestamp string that can be parsed.
    /// - `Ok(Option::None)`: If the SQL query returns no rows.
    /// - `Err(SqlInterfaceError::SingleStringRequestNoSingleResponse)`: If the SQL query
    ///   returns more than one row (indicating a data inconsistency).
    /// - `Err(SqlInterfaceError::SingleStringRequestFailure)`: If the SQL query itself fails.
    /// - `Err(SqlInterfaceError::TimestampConversionFailure)`: If the retrieved string
    ///   cannot be parsed into a valid `NaiveDateTime`.
    pub fn get_optional_timestamp(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<Option<NaiveDateTime>, SqlInterfaceError> {
        // trigger SQL call
        let timestamp_string_opt =
            match Self::get_single_optional_string_from_database(conn, sql_query_string) {
                Ok(c) => c,
                Err(e) => {
                    if !cfg!(test) {
                        error!(
                            target: Self::LOG_TARGET_SQL_INTERFACE,
                            "could not retrieve optional timestamp from the database ({e:?})"
                        );
                    }
                    return Err(e);
                }
            };

        let result_opt = match timestamp_string_opt {
            Some(timestamp_string) => {
                // convert data type
                match NaiveDateTime::parse_from_str(timestamp_string.as_str(), "%Y-%m-%d %H:%M:%S")
                {
                    Ok(c) => Some(c),
                    Err(_) => {
                        return Err(SqlInterfaceError::TimestampConversionFailure(
                            "Error when converting timestamp: ".to_string() + &timestamp_string,
                        ));
                    }
                }
            }
            Option::None => Option::None,
        };

        Ok(result_opt)
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
    /// A `Result` which is:
    /// - `Ok(NaiveDateTime)`: The current timestamp from the database.
    /// - `Err(SqlInterfaceError)`: If there's an issue retrieving or parsing the timestamp,
    ///   such as a database communication error, an unexpected response, or a conversion failure.
    pub fn get_current_timestamp(
        conn: &mut PooledConn,
    ) -> Result<NaiveDateTime, SqlInterfaceError> {
        Self::get_timestamp(conn, sql_query_strings::SQL_QUERY_READ_CURRENT_TIMESTAMP)
    }

    #[cfg(test)]
    // Retrieves a timestamp from the database with a specified future offset.
    //
    // This function is exclusively for use in test environments. It queries the database
    // for the current timestamp and then adds a given number of seconds to it, effectively
    // simulating a future or past point in time for testing scenarios.
    //
    // # Arguments
    // * `conn` - A mutable reference to an active database connection.
    // * `offset_seconds` - The number of seconds to add to the current database timestamp.
    //   A positive value shifts the timestamp into the future.
    //   A negative value shifts the timestamp into the past.
    //
    // # Returns
    // A `Result` which is:
    // - `Ok(NaiveDateTime)`: The calculated timestamp, offset by the given seconds.
    // - `Err(SqlInterfaceError)`: If there's an issue retrieving or parsing the base timestamp
    //   from the database.
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

    /// Executes an SQL statement given as a parameter to get a single String value
    /// and tries to convert it to timestamp of type NaiveDate.
    /// Depending on the result of the query and the conversion to NaiveDate,
    /// the function provides wrapped in a result type either
    /// - the timestamp as type NaiveDate
    /// - or an error code from the enum SqlInterfaceError
    /// # Arguments
    /// * `sql_query_string` - SQL query for retrieving timestamp as String
    fn get_date(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<NaiveDate, SqlInterfaceError> {
        // trigger SQL call
        let timestamp_string = match Self::get_single_string_from_database(conn, sql_query_string) {
            Ok(c) => c,
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_SQL_INTERFACE,
                    "could not retrieve date from the database ({e:?})"
                );
                return Err(e);
            }
        };
        
        // convert data type
        match NaiveDate::parse_from_str(timestamp_string.as_str(), "%Y-%m-%d") {
            Ok(c) => Ok(c),
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_SQL_INTERFACE,
                    "Error converting timestamp_string={} to date ({e:?})",
                    timestamp_string
                );
                Err(SqlInterfaceError::TimestampConversionFailure(
                    "Error when converting timestamp: ".to_string() + &timestamp_string,
                ))
            }
        }
    }

    /// Fetches the current calendar date directly from the database.
    ///
    /// This function queries the database server for its current date and
    /// returns it as a `NaiveDate`.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(NaiveDate)`: The current date from the database.
    /// - `Err(SqlInterfaceError)`: If there's an issue retrieving or parsing the date,
    ///   such as a database communication error, an unexpected response, or a conversion failure.
    pub fn get_current_date(conn: &mut PooledConn) -> Result<NaiveDate, SqlInterfaceError> {
        Self::get_date(conn, sql_query_strings::SQL_QUERY_READ_CURRENT_DATE)
    }

    /// Fetches the future calendar date of the following day directly from the database.
    ///
    /// This function queries the database server for tomorrow's date and
    /// returns it as a `NaiveDate`.
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to an active database connection.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(NaiveDate)`: Tomorrow's date from the database.
    /// - `Err(SqlInterfaceError)`: If there's an issue retrieving or parsing the date,
    ///   such as a database communication error, an unexpected response, or a conversion failure.
    pub fn get_tomorrow_date(conn: &mut PooledConn) -> Result<NaiveDate, SqlInterfaceError> {
        Self::get_date(conn, sql_query_strings::SQL_QUERY_READ_TOMORROW_DATE)
    }

    /// Fetches the names of all tables present in the connected SQL database.
    ///
    /// This function executes a standard SQL query to list all tables.
    /// It expects to retrieve multiple table names.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(Vec<String>)`: A vector containing the names of all tables found in the database.
    /// - `Err(SqlInterfaceError::ShowTablesWithoutProperResult)`: If the database
    ///   query for table names fails or returns an unexpected number of results (e.g., zero or one).
    /// - `Err(SqlInterfaceError::SingleStringRequestFailure)`: If an underlying issue
    ///   occurs during the fetching of string results.
    pub fn get_tables(&mut self) -> Result<Vec<String>, SqlInterfaceError> {
        match self.get_multiple_strings_from_database(sql_query_strings::SQL_QUERY_READ_TABLES) {
            Ok(c) => Ok(c),
            Err(_) => Err(SqlInterfaceError::ShowTablesWithoutProperResult(
                "Error when reading tables from the database, expected more than one result."
                    .to_string(),
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
    /// A `Result` which is:
    /// - `Ok(())`: If all tables listed in the configuration are found in the database.
    /// - `Err(SqlInterfaceError::RequiredTablesNotExisting)`: If one or more
    ///   required tables are missing from the database.
    /// - `Err(SqlInterfaceError)`: For any other error that occurs during
    ///   the process of fetching table names (e.g., database connection issues).
    pub fn check_required_tables_existing(&mut self) -> Result<(), SqlInterfaceError> {
        let existing_tables = self.get_tables()?;

        if !Self::compare_required_tables_existing_tables(
            self.config.db_tables.clone(),
            existing_tables.clone(),
        ) {
            #[cfg(not(test))]
            {
                let required_tables_string = &self.config.db_tables.join(",");
                let existing_tables_string = &existing_tables.join(",");
                error!(
                    target: Self::LOG_TARGET_SQL_INTERFACE,
                    "Required tables ({}) are not existing in database ({}).",
                    required_tables_string, existing_tables_string
                );
            }
            Err(SqlInterfaceError::RequiredTablesNotExisting)
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    // This helper function **empties the specified database table** using a TRUNCATE command.
    // Function is only used in test environments across different crates to ensure a clean state
    // before running tests.
    //
    // Arguments:
    // * `sql_interface`: A mutable reference to the main SQL interface.
    // * `sql_table_name`: The name of the table to be emptied.
    //
    // Returns:
    // `Ok(())` on success, or an `SqlInterfaceError` if the truncation fails.
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
                    "Error when truncating table: ".to_string() + &sql_table_name,
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
    #[cfg(test)]
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
    // `Some(i64)` representing the duration in seconds if the conversion and calculation
    // are successful. Returns `None` if the `timestamp_naive` cannot be unambiguously
    // converted to a local `DateTime`.
    pub fn get_duration_from_naive_timestamp_to_local_now(
        timestamp_naive: NaiveDateTime,
    ) -> Option<i64> {
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
        Some(
            reference_current_timestamp
                .signed_duration_since(timestamp_dt)
                .num_seconds(),
        )
    }

    /// Checks if the database's `wait_timeout` setting meets a minimum required value.
    ///
    /// This function retrieves the current `wait_timeout` from the connected database
    /// and compares it against a `min_wait_timeout` provided by the application.
    /// It is primarily used to ensure that database connections do not time out
    /// prematurely from the server's perspective, which could lead to "Broken pipe" errors.
    /// The function is called in the initialization phase of the control application.
    /// If the function panics, it will abort the initialization and the control application will
    /// terminate before starting regular operation.
    /// 
    /// # Arguments
    ///
    /// * `conn` - A mutable reference to a `PooledConn` (a database connection from a pool).
    ///            This connection is used to execute the SQL query to fetch the `wait_timeout`.
    /// * `min_wait_timeout` - The minimum acceptable `wait_timeout` value (in seconds)
    ///                        that the application requires for stable operation.
    ///
    /// # Panics
    ///
    /// This function will **panic** in the following scenarios:
    /// - If it fails to retrieve the `wait_timeout` value from the database (e.g., due to
    ///   connection issues, query execution errors).
    /// - If the retrieved `wait_timeout` value from the database is not a valid unsigned 64-bit integer
    ///   (e.g., if the database returns a value that cannot be converted to `u64`). This indicates
    ///   an unexpected data type or format from the database.
    /// - If the `current_wait_timeout` read from the database is less than `min_wait_timeout`.
    ///   In this case, the application will panic with a message indicating the discrepancy,
    ///   as it considers an insufficient `wait_timeout` a critical configuration error.
    pub fn check_wait_timeout(conn: &mut PooledConn, min_wait_timeout: u64) {
        let current_wait_timeout: u64 = match Self::get_single_integer_from_database(
            conn,
            SQL_QUERY_READ_TIMEOUT
        ) {
            Ok(c) => match c.try_into() {
                Ok(c) => c,
                Err(_) => {
                    panic!(
                        "Configuration file contains invalid value for wait_timeout: {}",
                        c
                    );
                }
            },
            Err(e) => {
                panic!("could not retrieve wait_timeout from the database ({e:?})");
            }
        };
        
        if current_wait_timeout < min_wait_timeout {
            panic!(
                "SQL database has wait_timeout configured of {} seconds. \
                Configuration of control application requires at least {} seconds.",
                current_wait_timeout,
                min_wait_timeout
            );
        }
    }
}

#[cfg(test)]
pub mod tests {
    use chrono::{DateTime, Local, SubsecRound, TimeZone};
    use mysql::params;
    use mysql::prelude::Queryable;

    use crate::database::sql_interface::{SqlInterface, SqlInterfaceError};
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};
    use crate::utilities::version_information::VersionInformation;

    #[test]
    // This test case checks
    // - if required tables are existing
    // - if current timestamp is local timestamp
    // Test case uses test database #28.
    pub fn test_sql_interface() {
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
            "* checking database name (PASS) succeeded. database_name_from_db={}",
            database_name_from_db
        );

        // *** check if required tables are existing - PASS *************************************
        assert_eq!(sql_interface.check_required_tables_existing(), Ok(()));
        println!("* checking required tables (PASS) succeeded.");
        // **************************************************************************************

        // *** check if required tables are existing - FAIL *************************************
        sql_interface.config.db_tables.push("dummy".to_string());
        assert_eq!(
            sql_interface.check_required_tables_existing(),
            Err(SqlInterfaceError::RequiredTablesNotExisting)
        );
        println!("* checking required tables (FAIL) succeeded.");
        // **************************************************************************************

        // *** check if current date is local date - PASS ***************************************
        let sql_date = match SqlInterface::get_current_date(&mut sql_interface.get_connection()) {
            Ok(c) => c,
            Err(_) => panic!("Could not get current date from the database."),
        };
        let sql_date_string = sql_date.format("%Y-%m-%d").to_string();
        let local_date_string = Local::now().trunc_subsecs(0).format("%Y-%m-%d").to_string();
        assert_eq!(sql_date_string, local_date_string);
        println!("* checking current date (PASS) succeeded.");
        // **************************************************************************************

        // *** check if current timestamp is local timestamp - PASS *****************************
        let sql_timestamp = match SqlInterface::get_current_timestamp(&mut sql_interface.conn) {
            Ok(c) => c,
            Err(_) => panic!("Could not get current timestamp from the database."),
        };
        let date_time: DateTime<Local> = Local.from_local_datetime(&sql_timestamp).unwrap();
        assert_eq!(date_time, Local::now().trunc_subsecs(0));
        println!("* checking current timestamp (PASS) succeeded.");
        // **************************************************************************************
    }

    pub const SQL_QUERY_WRITE_VERSION_INFORMATION: &str =
        "INSERT into version(major, minor, build, hash) values(:major, :minor, :build, :hash);";

    #[test]
    // This test case checks
    // - error response when querying empty table
    // - error response when version information does not match entry in database
    // - check error response when hash is incorrect
    // Test case uses test database #29.
    pub fn test_sql_interface_version() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            29,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");

        // *** check error response when querying empty table - FAIL ***********************************
        let version_information = VersionInformation::new();
        match SqlInterface::truncate_table(&mut sql_interface, "version".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        match sql_interface.get_hash_from_database(&version_information) {
            Ok(_) => {
                panic!("Found version information in truncated table.");
            }
            Err(e) => {
                assert_eq!(
                    e,
                    SqlInterfaceError::SingleVersionRequestNotListed(
                        "SQL database responded with empty response to query: \
                    SELECT hash FROM version WHERE major="
                            .to_string()
                            + &version_information.major.to_string()
                            + &" AND minor=".to_string()
                            + &version_information.minor.to_string()
                            + &" AND build=".to_string()
                            + &version_information.build.to_string()
                            + &";".to_string()
                    )
                );
            }
        }
        // *********************************************************************************************

        // *** check error response when version information does not match entry in database - FAIL ***
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
        match sql_interface.get_hash_from_database(&version_information) {
            Ok(_) => {
                panic!("Found correct version information although having inserted incorrect version information table.");
            }
            Err(e) => {
                assert_eq!(
                    e,
                    SqlInterfaceError::SingleVersionRequestNotListed(
                        "SQL database responded with empty response to query: \
                    SELECT hash FROM version WHERE major=0 AND minor=1 AND build=1;"
                            .to_string()
                    )
                );
            }
        }

        // *** check error response when the hash is incorrect - FAIL ********************************
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
    #[should_panic]
    // This test case checks if a excessively high value for the minimum wait timeout triggers a panic. 
    pub fn test_sql_interface_min_wait_timeout_check() {
        let mut config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            28,
        );
        config.sql_interface.db_min_wait_timeout = 1000000;
        println!("Testing with database {}", config.sql_interface.db_name);
        _ = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed with min wait timeout=0.");
    }
}
