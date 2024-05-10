use chrono::LocalResult::{Ambiguous, None, Single};
use chrono::*;
use mysql::prelude::*;
use mysql::*;
use serde_derive::Deserialize;

use crate::version_information::VersionInformation;

/// This module contains constants of all SQL queries.
pub mod sql_query_strings {
    /// This query provides version retrieves checks if the current application is listed in the table of supported versions.
    /// It informs wether the version is supported and which hash is stored in the database for the version.
    pub const SQL_QUERY_VERSION_INFORMATION: &str =
        "SELECT hash FROM version WHERE major=#major AND minor=#minor AND build=#build;";

    /// This query provides the current data base name.
    pub const SQL_QUERY_DATABASE: &str = "select database();";

    /// This query is used to read all tables from the database.
    pub const SQL_QUERY_READ_TABLES: &str = "SHOW TABLES;";

    /// This query is used to read the current date (without time) from the database.
    pub const SQL_QUERY_READ_CURRENT_DATE: &str = "SELECT CURRENT_DATE();";

    /// This query is used to read the current timestamp from the database.
    pub const SQL_QUERY_READ_CURRENT_TIMESTAMP: &str = "SELECT CURRENT_TIMESTAMP();";

    /// This query is used to insert data of one feed event into the database.
    pub const SQL_QUERY_WRITE_FEED_EVENT: &str =
        "INSERT into feedlog (Timestamp, FeederOnTime, ProfileName, ProfileIdx) values(:timestamp, :feeder_run_time, :profile_name, :profile_id)";

    /// This query is used to read the header of a feed pattern from the database.
    pub const SQL_QUERY_READ_FEEDPATTERN_HEADER: &str =
        "SELECT ProfileID, ProfileName FROM feedpatterns WHERE ProfileID=profile_id;";

    /// This query is used to read one phase of a feed pattern from the database.
    pub const SQL_QUERY_READ_FEED_PHASE: &str = "SELECT Pause#Duration, Pause#Skimmer, Pause#MPmp1, Pause#MPmp2, Pause#APmp1, Pause#APmp2, \
                Feed#Duration, Feed#Skimmer, Feed#MPmp1, Feed#MPmp2, Feed#APmp1, Feed#APmp2 FROM feedpatterns WHERE ProfileID=profile_id;";

    /// This query is used to read the timestamp of the last refill event from the database.
    pub const SQL_QUERY_READ_LAST_REFILL_TIMESTAMP: &str =
        "SELECT Timestamp from refill as LSTTSTMP ORDER BY Timestamp DESC LIMIT 1;";

    /// This query is used to check if the refill table is empty.
    pub const SQL_QUERY_CHECK_REFILL_EMPTY: &str = "SELECT EXISTS(SELECT 1 FROM REFILL);";

    /// This query is used to read the timestamp of the last Balling dosing event from the database.
    pub const SQL_QUERY_READ_LAST_BALLING_DOSING_TIMESTAMP: &str =
        "SELECT Timestamp from ballingdosinglog as LSTTSTMP ORDER BY Timestamp DESC LIMIT 1;";

    /// This query is used to write the last Balling dosing event to the database.    
    pub const SQL_QUERY_WRITE_BALLING_EVENT: &str =
        "INSERT into ballingdosinglog(Timestamp, pumpid, dosingvolume) values(:timestamp, :pump_id, :dosing_volume);";

    pub const SQL_QUERY_READ_BALLING_SETVALS: &str =
        "SELECT pumpid, dosingspeed, dosingvolume, label FROM ballingsetvals WHERE pumpid=#pump_id;";

    /// This query is used to read the refill volume of the last hour from the data base.
    /// It needs to use the COALESCE function because a null result is not interpreted correctly by the SQL library.
    pub const SQL_QUERY_READ_REFILL_VOLUME_LAST_HOUR: &str =
        "SELECT COALESCE(SUM(Volume), 0) from refill WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 HOUR LIMIT 1;";

    /// This query is used to read the number of refill events of the last hour from the data base.
    /// It does not need to use the COALESCE function because the query returns 0 if there is no refill event in the last 24h.    
    pub const SQL_QUERY_READ_REFILL_COUNT_LAST_HOUR: &str =
        "SELECT COUNT(Volume) from refill WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 HOUR LIMIT 1;";

    /// This query is used to read the refill volume of the last day from the data base.
    /// It needs to use the COALESCE function because a null result is not interpreted correctly by the SQL library.
    pub const SQL_QUERY_READ_REFILL_VOLUME_LAST_24H: &str =
        "SELECT COALESCE(SUM(Volume), 0) from refill WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 DAY LIMIT 1;";

    /// This query is used to read the number of refill events of the last day from the data base.
    /// It does not need to use the COALESCE function because the query returns 0 if there is no refill event in the last 24h.    
    pub const SQL_QUERY_READ_REFILL_COUNT_LAST_24H: &str =
        "SELECT COUNT(Volume) from refill WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 DAY LIMIT 1;";

    /// This query is used to write the last refill event to the database.    
    pub const SQL_QUERY_WRITE_REFILL_EVENT: &str =
        "INSERT into refill(Timestamp, Duration, Volume, ErrorCode) values(:timestamp, :duration, :volume, :error_code);";

    /// This query is used to read the duration in seconds until midnight from the database.
    pub const SQL_QUERY_READ_DURATION_UNTIL_MIDNIGHT: &str =
        "SELECT TIMESTAMPDIFF(SECOND,NOW(),DATE(NOW()) + INTERVAL 1 DAY);";

    /// This query is used to read the heating statistical data of the current day from the database.
    pub const SQL_QUERY_READ_HEATING_STATS: &str =
        "SELECT * FROM heatingstats WHERE Date=CURRENT_DATE();";

    /// This query is used to read oldest feed schedule entry which lies in the past.
    pub const SQL_QUERY_READ_FEEDSCHEDULE_ENTRY: &str = "SELECT TimeStamp, ProfileName, IsDaily \
        FROM feedschedule WHERE TimeStamp < CURRENT_TIMESTAMP() ORDER BY TimeStamp ASC;";

    /// This query is used to update a feed schedule entry after having executing a feed pattern
    /// which shall be repeated daily.
    pub const SQL_QUERY_UPDATE_FEEDSCHEDULE_ENTRY: &str =
        "UPDATE feedschedule SET Timestamp=:timestamp_new WHERE Timestamp=:timestamp_old;";

    /// This query is used to update a feed schedule entry after having executing a feed pattern
    /// which shall not be repeated daily.
    pub const SQL_QUERY_DELETE_FEEDSCHEDULE_ENTRY: &str =
        "DELETE FROM feedschedule WHERE Timestamp = :timestamp;";

    /// This query is used to write the heating statistical data into the database.
    /// If there is already data for the given date, the data will not be insert. Instead, the existing entry will be updated.
    pub const SQL_QUERY_WRITE_HEATING_STATS: &str = "insert into heatingstats \
        (Date, Energy, AmbientTempAverage, AmbientTempCounter, WaterTempAverage, WaterTempCounter, HeatingControlRuntime) \
        values (:date, :energy, \
        :ambient_temperature_average, :ambient_temperature_counter, \
        :water_temperature_average, :water_temperature_counter, \
        :heating_control_runtime) \
        ON DUPLICATE KEY Update \
        Energy=#Energy, \
        AmbientTempAverage=#AmbientTempAverage, \
        AmbientTempCounter=#AmbientTempCounter, \
        WaterTempAverage=#WaterTempAverage, \
        WaterTempCounter=#WaterTempCounter, \
        HeatingControlRuntime=#HeatingControlRuntime;";

    /// This query is used to write time-based data into the database.
    pub const SQL_QUERY_WRITE_DATA: &str = "insert into data \
        (Timestamp, \
        Temperature, TemperatureFiltered, \
        pH, pHFiltered, \
        Conductivity, ConductivityFiltered, ConductivityCompensated,
        RefillInProgress, TankLevelSwitchPosition, TankLevelSwitchInvalid, SurfaceVentilation,
        AmbTemp, AmbHum, Heater) \
        values (:timestamp, \
        #TemperatureUnfiltered, #TemperatureFiltered, \
        #pHUnfiltered, #pHFiltered, \
        #ConductivityUnfiltered, #ConductivityFiltered, #ConductivityCompensated, \
        #RefillInProgress, #TankLevelSwitchPosition, #TankLevelSwitchInvalid, #SurfaceVentilation,
        #AmbTemp, #AmbHum, #Heater) \
        ON DUPLICATE KEY UPDATE \
        Temperature=#TemperatureUnfiltered, TemperatureFiltered=#TemperatureFiltered, \
        pH=#pHUnfiltered, pHFiltered=#pHFiltered, \
        Conductivity=#ConductivityUnfiltered, ConductivityFiltered=#ConductivityFiltered, \
        ConductivityCompensated=#ConductivityCompensated, \
        RefillInProgress=#RefillInProgress, \
        TankLevelSwitchPosition=#TankLevelSwitchPosition, TankLevelSwitchInvalid=#TankLevelSwitchInvalid, \
        SurfaceVentilation=#SurfaceVentilation, \
        AmbTemp=#AmbTemp, AmbHum=#AmbHum, Heater=#Heater;";

    // This query is used for testing by multiple crates, therefore it resides here and not in the test module.
    #[allow(dead_code)]
    pub const SQL_QUERY_TRUNCATE_TABLE: &str = "truncate table #table;";

    /// This query is used to read feed event from the database - used for testing only.
    #[allow(dead_code)]
    pub const SQL_QUERY_READ_LAST_FEED_EVENT: &str =
        "SELECT Timestamp, FeederOnTime, ProfileName, ProfileIdx from feedlog;";

    /// This query is used to read Balling dosing event from the database - used for testing only.
    #[allow(dead_code)]
    pub const SQL_QUERY_READ_BALLING_EVENT: &str =
        "SELECT Timestamp, pumpid, dosingvolume from ballingdosinglog;";

    /// This query is used to read refill event from the database - used for testing only.
    #[allow(dead_code)]
    pub const SQL_QUERY_READ_REFILL_EVENT: &str =
        "SELECT Timestamp, Duration, Volume, ErrorCode from refill ORDER BY Timestamp DESC LIMIT 1;";
}

/// This struct holds the configuration data for the SQL interface.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Clone)]
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

    /// array of required tables
    pub db_tables: Vec<String>,
}

/// Struct is used to retrieve a single integer value from SQL database.
struct SingleInteger {
    single_integer: i64,
}

/// Struct is used to retrieve a single string value from SQL database.
struct SingleString {
    single_string: String,
}

/// Struct is used to retrieve a single float value from SQL database.
struct SingleFloat {
    single_float: f64,
}

#[derive(PartialEq, Debug)]
/// Enum codifies the errors which the program may encounter in communication with the DB
pub enum SqlInterfaceError {
    ConnectionPoolFailure(String),
    RequiredTablesNotExisting,
    ShowTablesWithoutProperResult(String),
    SingleVersionRequestFailure(String),
    SingleVersionRequestNotListed(String),
    SingleVersionRequestMultipleResults,
    SingleStringRequestFailure(String),
    SingleStringRequestNoSingleResponse(String),
    SingleIntegerRequestFailure(String),
    SingleIntegerRequestNoSingleResponse(String),
    SingleFloatRequestFailure(String),
    SingleFloatRequestNoSingleResponse(String),
    SingleHeatingStatsRequestFailure(String),
    SingleHeatingStatsRequestNoSingleResponse(String),
    SingleFeedpatternRequestFailure(String),
    SingleFeedpatternRequestNoSingleResponse(String),
    TimestampConversionFailure(String),
    InsertHeatingStatsEntryFailure(String),
    InsertFeedEventFailure(String),
    InsertRefillEventFailure(String),
    InsertBallingEventFailure(String),
    TruncateTableFailure(String),
    FeedScheduleEntryRequestFailure(String),
    FeedScheduleEntryTimeStampConversionFailure(String),
    SqlInterfaceErrorFeedScheduleUpdateFailure(String),
    SingleBallingSetValRequestFailure(String),
    InsertDataFrameFailure(String),
    #[allow(dead_code)]
    SingleDataFrameRequestNoSingleResponse(String),
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

        // Get connection for requests of main thread
        let conn = match pool.get_conn() {
            Ok(c) => c,
            Err(_) => {
                panic!("Could not open SQL data base connection.");
            }
        };

        Ok(SqlInterface { config, pool, conn })
    }

    pub fn get_connection(&self) -> PooledConn {
        match self.pool.get_conn() {
            Ok(c) => c,
            Err(_) => {
                panic!("Could not open SQL data base connection.");
            }
        }
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

    /// The function executes an SQL statement and evaluates if the response is empty, a single string value or contains multiple results.
    pub fn get_hash_from_database(
        &mut self,
        version_information: &VersionInformation,
    ) -> Result<String, SqlInterfaceError> {
        // Get single string value from database
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
                    "Could not get single string from SQL database using query:".to_string()
                        + &sql_query_string,
                ));
            }
        };

        // check error case if there is no response
        if single_string_array.len() == 0 {
            return Err(SqlInterfaceError::SingleVersionRequestNotListed(
                "SQL database responded with empty response to query: ".to_owned()
                    + &sql_query_string,
            ));
        }

        // check error case if there is more than one response
        if single_string_array.len() > 1 {
            println!(
                "SQL database responded with multiple responses to request to query: {}",
                sql_query_string
            );
            Err(SqlInterfaceError::SingleVersionRequestMultipleResults)
        } else {
            Ok(single_string_array[0].single_string.clone())
        }
    }

    /// The function executes an SQL statement given as parameter and evaluates if the response consists of multiple (> 1) rows.
    fn get_multiple_strings_from_database(
        &mut self,
        sql_query_string: &str,
    ) -> Result<Vec<String>, SqlInterfaceError> {
        // Get multiple string values from database
        let single_string_array =
            match self
                .conn
                .query_map(sql_query_string, |single_string| SingleString {
                    single_string,
                }) {
                Ok(c) => c,
                Err(_) => {
                    return Err(SqlInterfaceError::SingleStringRequestFailure(
                        "Could not get multiple strings from SQL database using query:".to_string()
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

    /// The function executes an SQL statement given as parameter and evaluates if the response is a single integer value.
    pub fn get_single_integer_from_database(
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
    pub fn get_single_float_from_database(
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

    /// The function triggers SQL call for a single value for retrieving the current database name.
    pub fn get_database(&mut self) -> Result<String, SqlInterfaceError> {
        // trigger SQL call
        match Self::get_single_string_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_DATABASE,
        ) {
            Ok(c) => Ok(c),
            Err(e) => {
                println!("{e:?}");
                Err(e)
            }
        }
    }

    /// The function triggers SQL call for a single value and the response string is converted into NaiveDateTime type.
    pub fn get_timestamp(
        conn: &mut PooledConn,
        sql_query_string: &str,
    ) -> Result<NaiveDateTime, SqlInterfaceError> {
        // trigger SQL call
        let timestamp_string = match Self::get_single_string_from_database(conn, sql_query_string) {
            Ok(c) => c,
            Err(e) => {
                if !cfg!(test) {
                    println!("{e:?}");
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

    /// The function retrieves the current timestamp from SQL database.
    pub fn get_current_timestamp(
        conn: &mut PooledConn,
    ) -> Result<NaiveDateTime, SqlInterfaceError> {
        Self::get_timestamp(conn, sql_query_strings::SQL_QUERY_READ_CURRENT_TIMESTAMP)
    }

    /// The function triggers SQL call for a single value and the response string is converted into NaiveDateTime type with modified formatting string.
    fn get_date(&mut self, sql_query_string: &str) -> Result<NaiveDate, SqlInterfaceError> {
        // trigger SQL call
        let timestamp_string =
            match Self::get_single_string_from_database(&mut self.conn, sql_query_string) {
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
    pub fn get_current_date(&mut self) -> Result<NaiveDate, SqlInterfaceError> {
        self.get_date(sql_query_strings::SQL_QUERY_READ_CURRENT_DATE)
    }

    /// Get all tables from database
    pub fn get_tables(&mut self) -> Result<Vec<String>, SqlInterfaceError> {
        match self.get_multiple_strings_from_database(sql_query_strings::SQL_QUERY_READ_TABLES) {
            Ok(c) => Ok(c),
            Err(_) => Err(SqlInterfaceError::ShowTablesWithoutProperResult(
                "Error when reading tables from database, expected more than one result."
                    .to_string(),
            )),
        }
    }

    /// Compare two vectors of strings and check if all of the strings of the first vector are contained within the second vector.
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

    /// Read all tables from the data base and check if all required tables are there.
    pub fn check_required_tables_existing(&mut self) -> Result<(), SqlInterfaceError> {
        let existing_tables = match self.get_tables() {
            Ok(c) => c,
            Err(e) => return Err(e),
        };
        if !Self::compare_required_tables_existing_tables(
            self.config.db_tables.clone(),
            existing_tables.clone(),
        ) {
            let required_tables_string = &self.config.db_tables.join(",");
            let existing_tables_string = &existing_tables.join(",");
            println!(
                "Required tables ({}) are not existing in database ({}).",
                required_tables_string, existing_tables_string
            );

            Err(SqlInterfaceError::RequiredTablesNotExisting)
        } else {
            Ok(())
        }
    }

    /// the function empties the table
    /// This function is used for testing purposes only across multiple crates.
    #[allow(dead_code)]
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

    /// calculate a timestamp of type NaiveDateTime which lies a defined period into the past
    /// This function is used for testing purposes only across multiple crates.
    #[allow(dead_code)]
    pub fn get_naive_timestamp_from_past(
        hours_in_past: i64,
        seconds_in_past: i64,
    ) -> NaiveDateTime {
        let timestamp_from_past = chrono::Local::now()
            - Duration::hours(hours_in_past)
            - Duration::seconds(seconds_in_past);
        timestamp_from_past.naive_local()
    }

    /// calculates the time difference between a given timestamp of type NaiveDateTime
    /// to the current local time in seconds
    #[allow(dead_code)]
    pub fn get_duration_from_naive_timestamp_to_local_now(
        timestamp_naive: NaiveDateTime,
    ) -> Option<i64> {
        let reference_current_timestamp = chrono::Local::now();
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
}

#[cfg(test)]
pub mod tests {
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::sql_interface::{SqlInterface, SqlInterfaceError};
    use crate::version_information::VersionInformation;
    use chrono::{DateTime, Local, SubsecRound, TimeZone};
    use mysql::params;
    use mysql::prelude::Queryable;

    #[test]
    /// all test cases for this crate/table combined in one function to avoid problems with multithreading
    pub fn test_sql_interface() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_sql_interface.toml".to_string());
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
        let sql_date = match SqlInterface::get_current_date(&mut sql_interface) {
            Ok(c) => c,
            Err(_) => panic!("Could not get current date from SQL database."),
        };
        let sql_date_string = sql_date.format("%Y-%m-%d").to_string();
        let local_date_string = chrono::offset::Local::now()
            .trunc_subsecs(0)
            .format("%Y-%m-%d")
            .to_string();
        assert_eq!(sql_date_string, local_date_string);
        println!("* checking current date (PASS) succeeded.");
        // **************************************************************************************

        // *** check if current timestamp is local timestamp - PASS *****************************
        let sql_timestamp = match SqlInterface::get_current_timestamp(&mut sql_interface.conn) {
            Ok(c) => c,
            Err(_) => panic!("Could not get current timestamp from SQL database."),
        };
        let date_time: DateTime<Local> = Local.from_local_datetime(&sql_timestamp).unwrap();
        assert_eq!(date_time, chrono::offset::Local::now().trunc_subsecs(0));
        println!("* checking current timestamp (PASS) succeeded.");
        // **************************************************************************************
    }

    pub const SQL_QUERY_WRITE_VERSION_INFORMATION: &str =
        "INSERT into version(major, minor, build, hash) values(:major, :minor, :build, :hash);";

    #[test]
    /// all test cases for this crate/table combined in one function to avoid problems with multithreading
    pub fn test_sql_interface_version() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_sql_interface.toml".to_string());
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

        // *** check error response when hash is incorrect - FAIL ********************************
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
                panic!("Error retrieving version information from database: {e:?}");
            }
        }
        // ***************************************************************************************
    }
}
