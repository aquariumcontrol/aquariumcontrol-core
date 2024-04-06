use chrono::*;
use mysql::prelude::*;
use mysql::*;
use serde_derive::Deserialize;

mod sql_query_strings {
    pub const SQL_QUERY_LAST_REFILL_TIMESTAMP: &str =
        "SELECT Timestamp from aquarium.refill as LSTTSTMP ORDER BY Timestamp DESC LIMIT 1;";
    pub const SQL_QUERY_LAST_BALLING_DOSING_TIMESTAMP: &str =
        "SELECT Timestamp from aquarium.ballingdosinglog as LSTTSTMP ORDER BY Timestamp DESC LIMIT 1;";
    pub const SQL_QUERY_CURRENT_TIMESTAMP: &str = "SELECT CURRENT_TIMESTAMP();";
    pub const SQL_QUERY_DURATION_UNTIL_MIDNIGHT: &str =
        "SELECT TIMESTAMPDIFF(SECOND,NOW(),DATE(NOW()) + INTERVAL 1 DAY);";
    pub const SQL_QUERY_REFILL_VOLUME_LAST_24H: &str =
        "SELECT SUM(Volume) from %s.refill WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 DAY LIMIT 1;";
    pub const SQL_QUERY_REFILL_COUNT_LAST_24H: &str =
        "SELECT COUNT(Volume) from %s.refill WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 DAY LIMIT 1;";
}
/// This struct holds the configuration data for the SQL interface.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct SqlInterfaceConfig {
    /// database user
    pub db_user: String,

    /// database password
    pub db_password: String,

    /// hostname of database
    pub db_host: String,

    /// port name of database
    pub db_port: u16,

    /// database name
    pub db_name: String,
}

/*
#[derive(Debug, PartialEq, Eq)]
struct Car {
    id: i32,
    name: String,
    price: i32,
}
*/

struct SingleInteger {
    single_integer: i64,
}

struct SingleString {
    single_string: String,
}

struct SingleFloat {
    single_float: f64,
}

#[derive(Debug)]
/// Enum codifies the errors which the program may encounter in communication with the DB
pub enum SqlInterfaceError {
    ConnectionPoolFailure(String),
    SingleStringRequestFailed(String),
    SingleStringRequestNoSingleResponse(String),
    SingleIntegerRequestFailed(String),
    SingleIntegerRequestNoSingleResponse(String),
    SingleFloatRequestFailed(String),
    SingleFloatRequestNoSingleResponse(String),
    TimestampConversionFailure(String),
}

/// This struct contains the configuration and the implementation of the SQL interface.
pub struct SqlInterface {
    /// The configuration data read from the .toml file.
    pub config: SqlInterfaceConfig,

    /// Connection pool
    pub pool: Pool,

    /// Connection to database
    pub conn: PooledConn,
}

impl SqlInterface {
    /// The function creates an instance of SqlInterface.
    pub fn new(config: SqlInterfaceConfig) -> Result<SqlInterface, SqlInterfaceError> {
        // Create connection pool
        let url = "mysql://aquarium:4w4umx7AzdAwCXi7@127.0.0.1:3306/aquarium";
        let pool = match Pool::new(url) {
            Ok(c) => c,
            Err(e) => {
                return Err(SqlInterfaceError::ConnectionPoolFailure(
                    "Could not establish connection pool with ".to_owned()
                        + url
                        + ": "
                        + &e.to_string(),
                ));
            }
        };

        // Get connection
        let conn = match pool.get_conn() {
            Ok(c) => c,
            Err(_) => {
                panic!("Could not open SQL data base connection.");
            }
        };

        Ok(SqlInterface { config, pool, conn })
    }

    /// The function executes an SQL statement given as parameter and evaluates if the response is a single string value.
    fn get_single_string_from_database(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<String, SqlInterfaceError> {
        // Get single string value from database
        let single_string_array = match conn.query_map(sql_query_string, |single_string| {
            SingleString { single_string }
        }) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::SingleStringRequestFailed(
                    "Could not get single string from SQL database using query:".to_string()
                        + sql_query_string,
                ));
            }
        };

        // check error case if there is no response
        if single_string_array.len() == 0 {
            return Err(SqlInterfaceError::SingleStringRequestNoSingleResponse(
                "SQL database responded with empty response to query: ".to_string()
                    + sql_query_string,
            ));
        }

        // check error case if there is more than one response
        if single_string_array.len() > 1 {
            return Err(SqlInterfaceError::SingleStringRequestNoSingleResponse(
                "SQL database responded with multiple responses to request to query:".to_string()
                    + sql_query_string,
            ));
        }
        Ok(single_string_array[0].single_string.clone())
    }

    /// The function executes an SQL statement given as parameter and evaluates if the response is a single integer value.
    fn get_single_integer_from_database(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<i64, SqlInterfaceError> {
        // Get single integer value from database
        let single_integer_array = match conn.query_map(sql_query_string, |single_integer| {
            SingleInteger { single_integer }
        }) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::SingleIntegerRequestFailed(
                    "Could not get single integer from SQL database using query:".to_string()
                        + sql_query_string,
                ));
            }
        };

        // check error case if there is no response
        if single_integer_array.len() == 0 {
            return Err(SqlInterfaceError::SingleIntegerRequestNoSingleResponse(
                "SQL database responded with empty response to query: ".to_string()
                    + sql_query_string,
            ));
        }

        // check error case if there is more than one response
        if single_integer_array.len() > 1 {
            return Err(SqlInterfaceError::SingleIntegerRequestNoSingleResponse(
                "SQL database responded with multiple responses to request to query:".to_string()
                    + sql_query_string,
            ));
        }
        Ok(single_integer_array[0].single_integer)
    }

    /// The function executes an SQL statement given as parameter and evaluates if the response is a single float value.
    fn get_single_float_from_database(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<f64, SqlInterfaceError> {
        // Get single float value from database
        let single_float_array = match conn.query_map(sql_query_string, |single_float| {
            SingleFloat { single_float }
        }) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::SingleFloatRequestFailed(
                    "Could not get single float value from SQL database using query:".to_string()
                        + sql_query_string,
                ));
            }
        };

        // check error case if there is no response
        if single_float_array.len() == 0 {
            return Err(SqlInterfaceError::SingleFloatRequestNoSingleResponse(
                "SQL database responded with empty response to query: ".to_string()
                    + sql_query_string,
            ));
        }

        // check error case if there is more than one response
        if single_float_array.len() > 1 {
            return Err(SqlInterfaceError::SingleFloatRequestNoSingleResponse(
                "SQL database responded with multiple responses to request to query:".to_string()
                    + sql_query_string,
            ));
        }
        Ok(single_float_array[0].single_float)
    }

    /// The function triggers SQL call for a single value and the response string is converted into NaiveDateTime type.
    fn get_timestamp(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<NaiveDateTime, SqlInterfaceError> {
        // trigger SQL call
        let timestamp_string = match Self::get_single_string_from_database(conn, sql_query_string) {
            Ok(c) => c,
            Err(e) => {
                return Err(e);
            }
        };

        // convert data type
        match NaiveDateTime::parse_from_str(timestamp_string.as_str(), "%Y-%m-%d %H:%M:%S") {
            Ok(c) => Ok(c),
            Err(_) => Err(SqlInterfaceError::TimestampConversionFailure(
                "Error when converting unique timestamp: ".to_string() + &timestamp_string,
            )),
        }
    }

    /// The function retrieves the current timestamp from SQL database.
    fn get_current_timestamp(conn: &mut PooledConn) -> Result<NaiveDateTime, SqlInterfaceError> {
        Self::get_timestamp(conn, sql_query_strings::SQL_QUERY_CURRENT_TIMESTAMP)
    }

    /// The function retrieves the timestamp of the last refill event from SQL database.
    fn get_last_refill_timestamp(
        conn: &mut PooledConn,
    ) -> Result<NaiveDateTime, SqlInterfaceError> {
        Self::get_timestamp(conn, sql_query_strings::SQL_QUERY_LAST_REFILL_TIMESTAMP)
    }

    /// The function retrieves the timestamp of the last Balling dosing event from SQL database.
    fn get_last_balling_dosing_timestamp(
        conn: &mut PooledConn,
    ) -> Result<NaiveDateTime, SqlInterfaceError> {
        Self::get_timestamp(
            conn,
            sql_query_strings::SQL_QUERY_LAST_BALLING_DOSING_TIMESTAMP,
        )
    }

    /// The function request current timestamp and timestamp of last refill event and calculates the difference between them in seconds.
    pub fn get_duration_since_last_refill(&mut self) -> Result<i64, SqlInterfaceError> {
        let current_datetime = match Self::get_current_timestamp(&mut self.conn) {
            Ok(c) => c,
            Err(e) => {
                return Err(e);
            }
        };

        let last_refill_datetime = match Self::get_last_refill_timestamp(&mut self.conn) {
            Ok(c) => c,
            Err(e) => {
                return Err(e);
            }
        };

        let delta_time = current_datetime - last_refill_datetime;

        Ok(delta_time.num_seconds())
    }

    /// The function request current timestamp and timestamp of last balling dosing event and calculates the difference between them in seconds.
    pub fn get_duration_since_last_balling_dosing(&mut self) -> Result<i64, SqlInterfaceError> {
        let current_datetime = match Self::get_current_timestamp(&mut self.conn) {
            Ok(c) => c,
            Err(e) => {
                return Err(e);
            }
        };

        let last_balling_dosing_datetime =
            match Self::get_last_balling_dosing_timestamp(&mut self.conn) {
                Ok(c) => c,
                Err(e) => {
                    return Err(e);
                }
            };

        let delta_time = current_datetime - last_balling_dosing_datetime;

        Ok(delta_time.num_seconds())
    }

    /// The function requests the duration until midnight in seconds.
    pub fn get_duration_until_midnight(&mut self) -> Result<i64, SqlInterfaceError> {
        match Self::get_single_integer_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_DURATION_UNTIL_MIDNIGHT,
        ) {
            Ok(c) => Ok(c),
            Err(e) => Err(e),
        }
    }

    /// The function requests the number of water refill events in the last 24 hours
    pub fn get_refill_count_of_last_24h(&mut self) -> Result<i64, SqlInterfaceError> {
        match Self::get_single_integer_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_REFILL_COUNT_LAST_24H,
        ) {
            Ok(c) => Ok(c),
            Err(e) => Err(e),
        }
    }

    /// The function requests the water volume refilled in the last 24 hours
    pub fn get_refill_volume_of_last_24h(&mut self) -> Result<f64, SqlInterfaceError> {
        match Self::get_single_float_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_REFILL_VOLUME_LAST_24H,
        ) {
            Ok(c) => Ok(c),
            Err(e) => Err(e),
        }
    }
    
    // todo: check of show tables
    // todo: check of database version
}
