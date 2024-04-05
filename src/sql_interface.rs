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

struct TimestampString {
    timestamp: String,
}

#[derive(Debug)]
/// Enum codifies the errors which the program may encounter in communication with the DB
pub enum SqlInterfaceError {
    ConnectionPoolFailure(String),
    TimestampRequestFailed(String),
    TimestampNoSingleResponse(String),
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

    /// The function executes an SQL statement given as parameter and evaluates if the response is a single value.
    /// If this is the case, the response string is converted into NaiveDateTime type.
    fn get_timestamp(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<NaiveDateTime, SqlInterfaceError> {
        // Get unique timestamp
        let unique_timestamp_string_array =
            match conn.query_map(sql_query_string, |timestamp| TimestampString { timestamp }) {
                Ok(c) => c,
                Err(_) => {
                    return Err(SqlInterfaceError::TimestampRequestFailed(
                        "Could not get unique timestamp from SQL database using query:".to_string()
                            + sql_query_string,
                    ));
                }
            };

        if unique_timestamp_string_array.len() == 0 {
            return Err(SqlInterfaceError::TimestampNoSingleResponse(
                "SQL database responded with empty response to query: ".to_string()
                    + sql_query_string,
            ));
        }
        if unique_timestamp_string_array.len() > 1 {
            return Err(SqlInterfaceError::TimestampNoSingleResponse(
                "SQL database responded with multiple responses to request to query:".to_string()
                    + sql_query_string,
            ));
        }
        let unique_timestamp_string = &unique_timestamp_string_array[0].timestamp;

        match NaiveDateTime::parse_from_str(&unique_timestamp_string, "%Y-%m-%d %H:%M:%S") {
            Ok(c) => Ok(c),
            Err(_) => Err(SqlInterfaceError::TimestampConversionFailure(
                "Error when converting unique timestamp: ".to_string() + unique_timestamp_string,
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
}
