use chrono::*;
use mysql::prelude::*;
use mysql::*;
use serde_derive::Deserialize;

mod sql_query_strings {
    pub const SQL_QUERY_READ_CURRENT_DATE: &str = "SELECT CURRENT_DATE();";
    pub const SQL_QUERY_READ_CURRENT_TIMESTAMP: &str = "SELECT CURRENT_TIMESTAMP();";

    pub const SQL_QUERY_READ_LAST_REFILL_TIMESTAMP: &str =
        "SELECT Timestamp from aquarium.refill as LSTTSTMP ORDER BY Timestamp DESC LIMIT 1;";
    pub const SQL_QUERY_READ_LAST_BALLING_DOSING_TIMESTAMP: &str =
        "SELECT Timestamp from aquarium.ballingdosinglog as LSTTSTMP ORDER BY Timestamp DESC LIMIT 1;";
    pub const SQL_QUERY_READ_DURATION_UNTIL_MIDNIGHT: &str =
        "SELECT TIMESTAMPDIFF(SECOND,NOW(),DATE(NOW()) + INTERVAL 1 DAY);";
    pub const SQL_QUERY_READ_REFILL_VOLUME_LAST_24H: &str =
        "SELECT SUM(Volume) from %s.refill WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 DAY LIMIT 1;";
    pub const SQL_QUERY_READ_REFILL_COUNT_LAST_24H: &str =
        "SELECT COUNT(Volume) from %s.refill WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 DAY LIMIT 1;";

    pub const SQL_QUERY_READ_HEATING_STATS: &str =
        "SELECT * FROM %s.heatingstats WHERE Date=CURRENT_DATE();";
    pub const SQL_QUERY_WRITE_HEATING_STATS: &str =
        "insert into %s.heatingstats(Date, Energy, AmbientTempAverage, AmbientTempCounter, WaterTempAverage, WaterTempCounter, HeatingControlRuntime) values (\'%s\', %lf, %lf, %d, %lf, %d, %d) ON DUPLICATE KEY Update Energy=%lf, AmbientTempAverage=%lf, AmbientTempCounter=%d, WaterTempAverage=%lf, WaterTempCounter=%d, HeatingControlRuntime=%d;";
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

pub struct HeatingStatsEntry {
    pub date: NaiveDate,
    pub energy: f64,
    pub ambient_temperature_average_value: f64,
    pub ambient_temperature_average_counter: i64,
    pub water_temperature_average_value: f64,
    pub water_temperature_average_counter: i64,
    pub heating_control_runtime: i64,
}

pub struct HeatingStatsStringEntry {
    pub date: String,
    pub energy: f64,
    pub ambient_temperature_average_value: f64,
    pub ambient_temperature_average_counter: i64,
    pub water_temperature_average_value: f64,
    pub water_temperature_average_counter: i64,
    pub heating_control_runtime: i64,
}

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
    SingleStringRequestFailure(String),
    SingleStringRequestNoSingleResponse(String),
    SingleIntegerRequestFailure(String),
    SingleIntegerRequestNoSingleResponse(String),
    SingleFloatRequestFailure(String),
    SingleFloatRequestNoSingleResponse(String),
    SingleHeatingStatsRequestFailure(String),
    SingleHeatingStatsRequestNoSingleResponse(String),
    TimestampConversionFailure(String),
    InsertHeatingStatsEntryFailure(String),
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
                return Err(SqlInterfaceError::SingleStringRequestFailure(
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
                return Err(SqlInterfaceError::SingleIntegerRequestFailure(
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
                return Err(SqlInterfaceError::SingleFloatRequestFailure(
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

    /// The function executes an SQL statement given as parameter and evaluates if the response is a single heating stats entry.
    pub fn get_single_heating_stats_from_database(
        conn: &mut PooledConn,
    ) -> Result<HeatingStatsEntry, SqlInterfaceError> {
        // Get single heating stats entry from database
        let single_heating_stats_entry_array = match conn.query_map(
            sql_query_strings::SQL_QUERY_READ_HEATING_STATS,
            |(
                date,
                energy,
                ambient_temperature_average_value,
                ambient_temperature_average_counter,
                water_temperature_average_value,
                water_temperature_average_counter,
                heating_control_runtime,
            )| HeatingStatsStringEntry {
                date,
                energy,
                ambient_temperature_average_value,
                ambient_temperature_average_counter,
                water_temperature_average_value,
                water_temperature_average_counter,
                heating_control_runtime,
            },
        ) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::SingleHeatingStatsRequestFailure(
                    "Could not get single heating stats entry from SQL database.".to_string(),
                ));
            }
        };

        // check error case if there is no response
        if single_heating_stats_entry_array.len() == 0 {
            return Err(
                SqlInterfaceError::SingleHeatingStatsRequestNoSingleResponse(
                    "SQL database responded with empty response to query: ".to_string(),
                ),
            );
        }

        // check error case if there is more than one response
        if single_heating_stats_entry_array.len() > 1 {
            return Err(
                SqlInterfaceError::SingleHeatingStatsRequestNoSingleResponse(
                    "SQL database responded with multiple responses to request to query:"
                        .to_string(),
                ),
            );
        }

        let single_heating_stats_string_entry = &single_heating_stats_entry_array[0];

        // convert String to NaiveDate
        let date = match NaiveDateTime::parse_from_str(
            single_heating_stats_string_entry.date.as_str(),
            "%Y-%m-%d",
        ) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::TimestampConversionFailure(
                    "Error when converting timestamp: ".to_string()
                        + &single_heating_stats_string_entry.date,
                ));
            }
        };

        let single_heating_stats_entry = HeatingStatsEntry {
            date: date.into(),
            energy: single_heating_stats_string_entry.energy,
            ambient_temperature_average_value: single_heating_stats_string_entry
                .ambient_temperature_average_value,
            ambient_temperature_average_counter: single_heating_stats_string_entry
                .ambient_temperature_average_counter,
            water_temperature_average_value: single_heating_stats_string_entry
                .water_temperature_average_value,
            water_temperature_average_counter: single_heating_stats_string_entry
                .water_temperature_average_counter,
            heating_control_runtime: single_heating_stats_string_entry.heating_control_runtime,
        };
        Ok(single_heating_stats_entry)
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
                "Error when converting timestamp: ".to_string() + &timestamp_string,
            )),
        }
    }

    /// The function retrieves the current timestamp from SQL database.
    fn get_current_timestamp(conn: &mut PooledConn) -> Result<NaiveDateTime, SqlInterfaceError> {
        Self::get_timestamp(conn, sql_query_strings::SQL_QUERY_READ_CURRENT_TIMESTAMP)
    }

    /// The function triggers SQL call for a single value and the response string is converted into NaiveDateTime type with modified formatting string.
    fn get_date(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<NaiveDate, SqlInterfaceError> {
        // trigger SQL call
        let timestamp_string = match Self::get_single_string_from_database(conn, sql_query_string) {
            Ok(c) => c,
            Err(e) => {
                println!("Error when retrieving single string from database: {e:?}");
                return Err(e);
            }
        };
        // convert data type
        match NaiveDate::parse_from_str(timestamp_string.as_str(), "%Y-%m-%d") {
            Ok(c) => Ok(c),
            Err(e) => {
                println!(
                    "Error converting timestamp_string={} {e:?}",
                    timestamp_string
                );
                Err(SqlInterfaceError::TimestampConversionFailure(
                    "Error when converting timestamp: ".to_string() + &timestamp_string,
                ))
            }
        }
    }

    /// The function retrieves the current date from SQL database.
    pub fn get_current_date(conn: &mut PooledConn) -> Result<NaiveDate, SqlInterfaceError> {
        Self::get_date(conn, sql_query_strings::SQL_QUERY_READ_CURRENT_DATE)
    }

    /// The function retrieves the timestamp of the last refill event from SQL database.
    fn get_last_refill_timestamp(
        conn: &mut PooledConn,
    ) -> Result<NaiveDateTime, SqlInterfaceError> {
        Self::get_timestamp(
            conn,
            sql_query_strings::SQL_QUERY_READ_LAST_REFILL_TIMESTAMP,
        )
    }

    /// The function retrieves the timestamp of the last Balling dosing event from SQL database.
    fn get_last_balling_dosing_timestamp(
        conn: &mut PooledConn,
    ) -> Result<NaiveDateTime, SqlInterfaceError> {
        Self::get_timestamp(
            conn,
            sql_query_strings::SQL_QUERY_READ_LAST_BALLING_DOSING_TIMESTAMP,
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
            sql_query_strings::SQL_QUERY_READ_DURATION_UNTIL_MIDNIGHT,
        ) {
            Ok(c) => Ok(c),
            Err(e) => Err(e),
        }
    }

    /// The function requests the number of water refill events in the last 24 hours
    pub fn get_refill_count_of_last_24h(&mut self) -> Result<i64, SqlInterfaceError> {
        match Self::get_single_integer_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_REFILL_COUNT_LAST_24H,
        ) {
            Ok(c) => Ok(c),
            Err(e) => Err(e),
        }
    }

    /// The function requests the water volume refilled in the last 24 hours
    pub fn get_refill_volume_of_last_24h(&mut self) -> Result<f64, SqlInterfaceError> {
        match Self::get_single_float_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_REFILL_VOLUME_LAST_24H,
        ) {
            Ok(c) => Ok(c),
            Err(e) => Err(e),
        }
    }

    /// The function inserts one dataset of heating statistics into the database
    // Now let's insert payments to the database
    pub fn insert_heating_stats_entry(
        &mut self,
        heating_stats_entry: HeatingStatsEntry,
    ) -> Result<(), SqlInterfaceError> {
        match self.conn.exec::<bool, _, _>(
            sql_query_strings::SQL_QUERY_WRITE_HEATING_STATS,
            params! {
            "Date" => heating_stats_entry.date.to_string(),
            "Energy" => heating_stats_entry.energy,
            "AmbientTempAverage" => heating_stats_entry.ambient_temperature_average_value,
            "AmbientTempCounter" => heating_stats_entry.ambient_temperature_average_counter,
            "WaterTempAverage" => heating_stats_entry.water_temperature_average_value,
            "WaterTempCounter" => heating_stats_entry.water_temperature_average_counter,
            "HeatingControlRuntime" => heating_stats_entry.heating_control_runtime},
        ) {
            Ok(_) => Ok(()),
            Err(_) => Err(SqlInterfaceError::InsertHeatingStatsEntryFailure(
                "Error when inserting heating stats into database.".to_string(),
            )),
        }
    }
    // todo: check of show tables
    // todo: check of database version
}

#[cfg(test)]
pub mod tests {
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::SqlInterface;
    use chrono::Utc;

    #[test]
    pub fn test_current_date() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_sql_interface.toml".to_string());
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");

        let date = match SqlInterface::get_current_date(&mut sql_interface.conn) {
            Ok(c) => c,
            Err(_) => panic!("Could not get current date from SQL database."),
        };

        assert_eq!(date, Utc::now().naive_utc().into());
    }
}
