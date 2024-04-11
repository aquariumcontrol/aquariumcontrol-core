use chrono::*;
use mysql::prelude::*;
use mysql::*;
use serde_derive::Deserialize;

use crate::feedpattern::FeedPhase;
use crate::feedpattern::Feedpattern;

/// This module contains constants of all SQL queries.
mod sql_query_strings {
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
        "SELECT Timestamp from aquarium.refill as LSTTSTMP ORDER BY Timestamp DESC LIMIT 1;";

    /// This query is used to read the timestamp of the last Balling dosing event from the database.
    pub const SQL_QUERY_READ_LAST_BALLING_DOSING_TIMESTAMP: &str =
        "SELECT Timestamp from aquarium.ballingdosinglog as LSTTSTMP ORDER BY Timestamp DESC LIMIT 1;";

    /// This query is used to write the last Balling dosing event to the database.    
    pub const SQL_QUERY_WRITE_BALLING_EVENT: &str =
        "INSERT into ballingdosinglog(Timestamp, pumpid, dosingvolume) values(:timestamp, :pump_id, :dosing_volume);";

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

    /// array of required tables
    pub db_tables: Vec<String>,
}

/// This struct holds the statistical data of heating for one day.
/// It is filled with post-processed data from SQL query which is then updated by the control application.
pub struct HeatingStatsEntry {
    pub date: NaiveDate,
    pub energy: f64,
    pub ambient_temperature_average_value: f64,
    pub ambient_temperature_average_counter: i64,
    pub water_temperature_average_value: f64,
    pub water_temperature_average_counter: i64,
    pub heating_control_runtime: i64,
}

/// This struct holds the statistical data of heating for one day as retrieved from SQL database.
/// In next step, date is postprocessed to NaiveData type.
pub struct HeatingStatsStringEntry {
    pub date: String,
    pub energy: f64,
    pub ambient_temperature_average_value: f64,
    pub ambient_temperature_average_counter: i64,
    pub water_temperature_average_value: f64,
    pub water_temperature_average_counter: i64,
    pub heating_control_runtime: i64,
}

#[derive(Clone)]
/// This struct holds the header information of a feed pattern.
pub struct SqlFeedpatternHeader {
    pub profile_id: i32,
    pub profile_name: String,
}

#[derive(Clone)]
/// This struct holds the data of one phase of a feed pattern as retrieved from SQL data base.
/// The numeric data is post-processed to boolean in a subsequent step.
pub struct SqlFeedPhase {
    pub pause_duration: i16,
    pub pause_skimmer: i16,
    pub pause_main_pump_1: i16,
    pub pause_main_pump_2: i16,
    pub pause_aux_pump_1: i16,
    pub pause_aux_pump_2: i16,
    pub feed_duration: i16,
    pub feed_skimmer: i16,
    pub feed_main_pump_1: i16,
    pub feed_main_pump_2: i16,
    pub feed_aux_pump_1: i16,
    pub feed_aux_pump_2: i16,
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

#[derive(Debug)]
/// Enum codifies the errors which the program may encounter in communication with the DB
pub enum SqlInterfaceError {
    ConnectionPoolFailure(String),
    ShowTablesWithoutProperResult(String),
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
        &mut self,
        sql_query_string: &str,
    ) -> Result<String, SqlInterfaceError> {
        // Get single string value from database
        let single_string_array =
            match self
                .conn
                .query_map(sql_query_string, |single_string| SingleString {
                    single_string,
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
    fn get_single_integer_from_database(
        &mut self,
        sql_query_string: &str,
    ) -> Result<i64, SqlInterfaceError> {
        // Get single integer value from database
        let single_integer_array =
            match self
                .conn
                .query_map(sql_query_string, |single_integer| SingleInteger {
                    single_integer,
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
        &mut self,
        sql_query_string: &str,
    ) -> Result<f64, SqlInterfaceError> {
        // Get single float value from database
        let single_float_array =
            match self
                .conn
                .query_map(sql_query_string, |single_float| SingleFloat {
                    single_float,
                }) {
                Ok(c) => c,
                Err(_) => {
                    return Err(SqlInterfaceError::SingleFloatRequestFailure(
                        "Could not get single float value from SQL database using query:"
                            .to_string()
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
        &mut self,
    ) -> Result<HeatingStatsEntry, SqlInterfaceError> {
        // Get single heating stats entry from database
        let single_heating_stats_entry_array = match self.conn.query_map(
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
        let date = match NaiveDate::parse_from_str(
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

    /// The function executes an SQL statement given as parameter and evaluates if the response is a single feed pattern header
    pub fn get_feedpattern_header_from_database(
        &mut self,
        profile_id: i32,
    ) -> Result<SqlFeedpatternHeader, SqlInterfaceError> {
        // prepare query for header
        let sql_query = str::replace(
            sql_query_strings::SQL_QUERY_READ_FEEDPATTERN_HEADER,
            "profile_id",
            &profile_id.to_string(),
        );
        let single_feedpattern_entry_array =
            match self
                .conn
                .query_map(sql_query.clone(), |(profile_id, profile_name)| {
                    SqlFeedpatternHeader {
                        profile_id,
                        profile_name,
                    }
                }) {
                Ok(c) => c,
                Err(_) => {
                    return Err(SqlInterfaceError::SingleFeedpatternRequestFailure(
                        "Could not get feed pattern header from SQL database with query:"
                            .to_string()
                            + sql_query.as_str(),
                    ));
                }
            };

        // check error case if there is no response
        if single_feedpattern_entry_array.len() == 0 {
            return Err(SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse(
                "SQL database responded with empty response to query: ".to_string()
                    + sql_query.as_str(),
            ));
        }
        // check error case if there is more than one response
        if single_feedpattern_entry_array.len() > 1 {
            return Err(SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse(
                "SQL database responded with multiple responses to request to query:".to_string()
                    + sql_query.as_str(),
            ));
        }
        let feedpattern_header = &single_feedpattern_entry_array[0];

        Ok(feedpattern_header.clone())
    }

    /// The function executes an SQL statement given as parameter and evaluates if the response is a single feed pattern phase
    pub fn get_feed_phase_from_database(
        &mut self,
        profile_id: i32,
        phase_nr: i32,
    ) -> Result<SqlFeedPhase, SqlInterfaceError> {
        // prepare query for header
        let sql_query_pre = str::replace(
            sql_query_strings::SQL_QUERY_READ_FEED_PHASE,
            "profile_id",
            &profile_id.to_string(),
        );
        let sql_query = str::replace(&sql_query_pre, "#", format!("{:02}", phase_nr).as_str());

        let single_feedpattern_entry_array = match self.conn.query_map(
            sql_query.clone(),
            |(
                pause_duration,
                pause_skimmer,
                pause_main_pump_1,
                pause_main_pump_2,
                pause_aux_pump_1,
                pause_aux_pump_2,
                feed_duration,
                feed_skimmer,
                feed_main_pump_1,
                feed_main_pump_2,
                feed_aux_pump_1,
                feed_aux_pump_2,
            )| SqlFeedPhase {
                pause_duration,
                pause_skimmer,
                pause_main_pump_1,
                pause_main_pump_2,
                pause_aux_pump_1,
                pause_aux_pump_2,
                feed_duration,
                feed_skimmer,
                feed_main_pump_1,
                feed_main_pump_2,
                feed_aux_pump_1,
                feed_aux_pump_2,
            },
        ) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::SingleFeedpatternRequestFailure(
                    "Could not get feed pattern phase from SQL database with query: ".to_string()
                        + sql_query.as_str(),
                ));
            }
        };

        // check error case if there is no response
        if single_feedpattern_entry_array.len() == 0 {
            return Err(SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse(
                "SQL database responded with empty response to query: ".to_string()
                    + sql_query.as_str(),
            ));
        }
        // check error case if there is more than one response
        if single_feedpattern_entry_array.len() > 1 {
            return Err(SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse(
                "SQL database responded with multiple responses to request to query: ".to_string()
                    + sql_query.as_str(),
            ));
        }
        let feed_phase = &single_feedpattern_entry_array[0];

        Ok(feed_phase.clone())
    }

    /// The function retries the header and all phases of a feed pattern from the SQL database.
    pub fn get_single_feedpattern_from_database(
        &mut self,
        profile_id: i32,
    ) -> Result<Feedpattern, SqlInterfaceError> {
        let sql_feedpattern_header = match self.get_feedpattern_header_from_database(profile_id) {
            Ok(c) => c,
            Err(e) => {
                println!("{e:?}");
                return Err(e);
            }
        };
        let mut feed_phases = Vec::new();
        for nr in 1..10 {
            let sql_feed_phase = match self.get_feed_phase_from_database(profile_id, nr) {
                Ok(c) => c,
                Err(e) => {
                    return Err(e);
                }
            };
            let feed_phase = FeedPhase {
                pause_duration: sql_feed_phase.pause_duration,
                pause_skimmer: sql_feed_phase.pause_skimmer > 0,
                pause_main_pump_1: sql_feed_phase.pause_main_pump_1 > 0,
                pause_main_pump_2: sql_feed_phase.pause_main_pump_2 > 0,
                pause_aux_pump_1: sql_feed_phase.pause_aux_pump_1 > 0,
                pause_aux_pump_2: sql_feed_phase.pause_aux_pump_2 > 0,
                feed_duration: sql_feed_phase.feed_duration,
                feed_skimmer: sql_feed_phase.feed_skimmer > 0,
                feed_main_pump_1: sql_feed_phase.feed_main_pump_1 > 0,
                feed_main_pump_2: sql_feed_phase.feed_main_pump_2 > 0,
                feed_aux_pump_1: sql_feed_phase.feed_aux_pump_1 > 0,
                feed_aux_pump_2: sql_feed_phase.feed_aux_pump_2 > 0,
            };
            feed_phases.push(feed_phase);
        }
        Ok(Feedpattern {
            profile_id: sql_feedpattern_header.profile_id,
            profile_name: sql_feedpattern_header.profile_name,
            feedphases: feed_phases,
        })
    }

    /// The function triggers SQL call for a single value and the response string is converted into NaiveDateTime type.
    fn get_timestamp(
        &mut self,
        sql_query_string: &str,
    ) -> Result<NaiveDateTime, SqlInterfaceError> {
        // trigger SQL call
        let timestamp_string = match self.get_single_string_from_database(sql_query_string) {
            Ok(c) => c,
            Err(e) => {
                println!("{e:?}");
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
    fn get_current_timestamp(&mut self) -> Result<NaiveDateTime, SqlInterfaceError> {
        self.get_timestamp(sql_query_strings::SQL_QUERY_READ_CURRENT_TIMESTAMP)
    }

    /// The function triggers SQL call for a single value and the response string is converted into NaiveDateTime type with modified formatting string.
    fn get_date(&mut self, sql_query_string: &str) -> Result<NaiveDate, SqlInterfaceError> {
        // trigger SQL call
        let timestamp_string = match self.get_single_string_from_database(sql_query_string) {
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

    /// The function retrieves the timestamp of the last refill event from SQL database.
    fn get_last_refill_timestamp(&mut self) -> Result<NaiveDateTime, SqlInterfaceError> {
        self.get_timestamp(sql_query_strings::SQL_QUERY_READ_LAST_REFILL_TIMESTAMP)
    }

    /// The function retrieves the timestamp of the last Balling dosing event from SQL database.
    fn get_last_balling_dosing_timestamp(&mut self) -> Result<NaiveDateTime, SqlInterfaceError> {
        self.get_timestamp(sql_query_strings::SQL_QUERY_READ_LAST_BALLING_DOSING_TIMESTAMP)
    }

    /// The function request current timestamp and timestamp of last refill event and calculates the difference between them in seconds.
    pub fn get_duration_since_last_refill(&mut self) -> Result<i64, SqlInterfaceError> {
        let current_datetime = match self.get_current_timestamp() {
            Ok(c) => c,
            Err(e) => {
                return Err(e);
            }
        };

        let last_refill_datetime = match self.get_last_refill_timestamp() {
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
        let current_datetime = match self.get_current_timestamp() {
            Ok(c) => c,
            Err(e) => {
                return Err(e);
            }
        };

        let last_balling_dosing_datetime = match self.get_last_balling_dosing_timestamp() {
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
        match self.get_single_integer_from_database(
            sql_query_strings::SQL_QUERY_READ_DURATION_UNTIL_MIDNIGHT,
        ) {
            Ok(c) => Ok(c),
            Err(e) => Err(e),
        }
    }

    /// The function requests the number of water refill events in the last 24 hours
    pub fn get_refill_count_of_last_24h(&mut self) -> Result<i64, SqlInterfaceError> {
        match self.get_single_integer_from_database(
            sql_query_strings::SQL_QUERY_READ_REFILL_COUNT_LAST_24H,
        ) {
            Ok(c) => Ok(c),
            Err(e) => Err(e),
        }
    }

    /// The function requests the water volume refilled in the last 24 hours
    pub fn get_refill_volume_of_last_24h(&mut self) -> Result<f64, SqlInterfaceError> {
        match self.get_single_float_from_database(
            sql_query_strings::SQL_QUERY_READ_REFILL_VOLUME_LAST_24H,
        ) {
            Ok(c) => Ok(c),
            Err(e) => {
                println!("{e:?}");
                Err(e)
            }
        }
    }

    /// The function inserts one dataset of heating statistics into the database
    pub fn insert_heating_stats_entry(
        &mut self,
        heating_stats_entry: HeatingStatsEntry,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_energy_replaced = str::replace(
            sql_query_strings::SQL_QUERY_WRITE_HEATING_STATS,
            "#Energy",
            &heating_stats_entry.energy.to_string(),
        );
        let sql_query_ambient_temperature_average_replaced = str::replace(
            &sql_query_energy_replaced,
            "#AmbientTempAverage",
            &heating_stats_entry
                .ambient_temperature_average_value
                .to_string(),
        );
        let sql_query_ambient_temperature_counter_replaced = str::replace(
            &sql_query_ambient_temperature_average_replaced,
            "#AmbientTempCounter",
            &heating_stats_entry
                .ambient_temperature_average_counter
                .to_string(),
        );
        let sql_query_water_temperature_average_replaced = str::replace(
            &sql_query_ambient_temperature_counter_replaced,
            "#WaterTempAverage",
            &heating_stats_entry
                .water_temperature_average_value
                .to_string(),
        );
        let sql_query_water_temperature_counter_replaced = str::replace(
            &sql_query_water_temperature_average_replaced,
            "#WaterTempCounter",
            &heating_stats_entry
                .water_temperature_average_counter
                .to_string(),
        );
        let sql_query_string = str::replace(
            &sql_query_water_temperature_counter_replaced,
            "#HeatingControlRuntime",
            &heating_stats_entry
                .water_temperature_average_counter
                .to_string(),
        );

        match self.conn.exec_drop::<_, _>(sql_query_string.clone(), params!{
                "date" => heating_stats_entry.date.format("%Y-%m-%d").to_string(),
                "energy" => heating_stats_entry.energy,
                "ambient_temperature_average" => heating_stats_entry.ambient_temperature_average_value,
                "ambient_temperature_counter" => heating_stats_entry.ambient_temperature_average_counter,
                "water_temperature_average" => heating_stats_entry.water_temperature_average_value,
                "water_temperature_counter" => heating_stats_entry.water_temperature_average_counter,
                "heating_control_runtime" => heating_stats_entry.heating_control_runtime,
                },
            ) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("{e:?}");
                Err(SqlInterfaceError::InsertHeatingStatsEntryFailure(
                    "Error when inserting heating stats into database using query: ".to_string() + sql_query_string.as_str(),
                ))
            }
        }
    }

    /// The function inserts one dataset of refill event into the database
    pub fn insert_refill_event(
        &mut self,
        timestamp: NaiveDateTime,
        duration: f64,
        volume: f64,
        error_code: i32,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_REFILL_EVENT.to_owned();
        match self.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "timestamp" => timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "duration" => duration,
            "volume" => volume,
            "error_code" => error_code,
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("{e:?}");
                Err(SqlInterfaceError::InsertRefillEventFailure(
                    "Error when inserting refill event into database.".to_string(),
                ))
            }
        }
    }

    /// The function inserts one dataset of feed event into the database
    pub fn insert_feed_event(
        &mut self,
        timestamp: NaiveDateTime,
        feeder_run_time: f64,
        profile_name: String,
        profile_id: i64,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_FEED_EVENT.to_owned();
        match self.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "timestamp" => timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "feeder_run_time" => feeder_run_time,
            "profile_name" => profile_name,
            "profile_id" => profile_id,
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("{e:?}");
                Err(SqlInterfaceError::InsertFeedEventFailure(
                    "Error when inserting refill event into database.".to_string(),
                ))
            }
        }
    }

    /// The function inserts one dataset of Balling dosing event into the database
    pub fn insert_balling_event(
        &mut self,
        timestamp: NaiveDateTime,
        pump_id: i64,
        dosing_volume: f64,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_BALLING_EVENT.to_owned();
        match self.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "timestamp" => timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "pump_id" => pump_id,
            "dosing_volume" => dosing_volume,
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("{e:?}");
                Err(SqlInterfaceError::InsertBallingEventFailure(
                    "Error when inserting Balling dosing event into database.".to_string(),
                ))
            }
        }
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
    // todo: check of show tables
    // todo: check of database version
}

#[cfg(test)]
pub mod tests {
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::SqlInterface;
    use chrono::Utc;
    use mysql::prelude::Queryable;

    pub const SQL_TEST_QUERY_DROP_TABLE_IF_EXISTS: &str = "DROP TABLE IF EXISTS";

    #[test]
    pub fn test_current_date() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_sql_interface.toml".to_string());
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");

        let _ = sql_interface.conn.query_drop(
            SQL_TEST_QUERY_DROP_TABLE_IF_EXISTS.to_owned()
                + sql_interface.config.db_tables[0].as_str()
                + ";",
        );

        let date = match sql_interface.get_current_date() {
            Ok(c) => c,
            Err(_) => panic!("Could not get current date from SQL database."),
        };

        assert_eq!(date, Utc::now().naive_utc().into());
    }
}
