/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::*;
use log::error;
use mysql::prelude::*;
use mysql::*;

use crate::sql_interface::{sql_query_strings, SqlInterface, SqlInterfaceError};

/// Trait for interfacing between refill and SQL database
/// This trait is prerequiste to use mock implementation for testing
pub trait DatabaseInterfaceRefillTrait {
    fn get_duration_since_last_refill(&mut self) -> Result<u64, SqlInterfaceError>;

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
pub struct SqlInterfaceRefill {
    /// Connection to database
    pub conn: PooledConn,
}

impl SqlInterfaceRefill {
    const LOG_TARGET_SQL_INTERFACE_REFILL: &'static str = "sql_interface_refill";

    /// Provides a struct of type SqlInterfaceRefill.
    /// # Arguments
    /// * `conn` - connection to SQL database
    pub fn new(conn: PooledConn) -> SqlInterfaceRefill {
        SqlInterfaceRefill { conn }
    }

    /// Retrieves the timestamp of the last refill event from SQL database.
    /// It provides wrapped into Result type either
    /// - timestamp of last refill event as type NaiveDateTime
    /// - or a value from enum SqlInterfaceError
    fn get_last_refill_timestamp(&mut self) -> Result<NaiveDateTime, SqlInterfaceError> {
        SqlInterface::get_timestamp(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_LAST_REFILL_TIMESTAMP,
        )
    }
}

impl DatabaseInterfaceRefillTrait for SqlInterfaceRefill {
    /// Requests current timestamp and timestamp of last refill event and calculates the difference between them in seconds.
    /// It provides wrapped into Result type either
    /// - number of seconds since last refill event
    /// - or a value from enum SqlInterfaceError    
    fn get_duration_since_last_refill(&mut self) -> Result<u64, SqlInterfaceError> {
        let current_datetime = match SqlInterface::get_current_timestamp(&mut self.conn) {
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

        let delta_time = current_datetime.signed_duration_since(last_refill_datetime);

        if delta_time.num_seconds() >= 0 {
            Ok(delta_time.num_seconds() as u64)
        } else {
            Err(SqlInterfaceError::NegativeInteger(delta_time.num_seconds()))
        }
    }

    /// Checks if the SQL table of refill events is empty.
    /// It returns wrapped into Result type either
    /// - a value of one if there is at least one entry in the table or
    /// - a value of zero if there is not entry in the table or
    /// - a value from enum SqlInterfaceError
    fn check_empty(&mut self) -> Result<u64, SqlInterfaceError> {
        match SqlInterface::get_single_integer_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_CHECK_REFILL_EMPTY,
        ) {
            Ok(c) => {
                if c >= 0 {
                    Ok(c as u64)
                } else {
                    Err(SqlInterfaceError::NegativeInteger(c))
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Retrieves the number of water refill events in the last 24 hours.
    /// It returns wrapped into Result type either
    /// - the number of refill events in the last 24 hours
    /// - a value from enum SqlInterfaceError    
    fn get_refill_count_of_last_24h(&mut self) -> Result<u64, SqlInterfaceError> {
        match SqlInterface::get_single_integer_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_REFILL_COUNT_LAST_24H,
        ) {
            Ok(c) => {
                if c >= 0 {
                    Ok(c as u64)
                } else {
                    Err(SqlInterfaceError::NegativeInteger(c))
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Retrieves the number of water refill events in the last hour.
    /// It returns wrapped into Result type either
    /// - the number of refill events in the last hour
    /// - a value from enum SqlInterfaceError
    fn get_refill_count_of_last_hour(&mut self) -> Result<u64, SqlInterfaceError> {
        match SqlInterface::get_single_integer_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_REFILL_COUNT_LAST_HOUR,
        ) {
            Ok(c) => {
                if c >= 0 {
                    Ok(c as u64)
                } else {
                    Err(SqlInterfaceError::NegativeInteger(c))
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Retrieves the water volume refilled in the last 24 hours
    /// It returns wrapped into Result type either
    /// - the refill volume in literes of the last 24 hours
    /// - a value from enum SqlInterfaceError
    fn get_refill_volume_of_last_24h(&mut self) -> Result<f64, SqlInterfaceError> {
        match SqlInterface::get_single_float_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_REFILL_VOLUME_LAST_24H,
        ) {
            Ok(c) => Ok(c),
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_SQL_INTERFACE_REFILL,
                    "could not retrieve refill volume of last 24h from database ({e:?})"
                );
                Err(e)
            }
        }
    }

    /// Retrieves the water volume refilled in the last hour
    /// It returns wrapped into Result type either
    /// - the refill volume in literes of the last hour
    /// - a value from enum SqlInterfaceError
    fn get_refill_volume_of_last_hour(&mut self) -> Result<f64, SqlInterfaceError> {
        match SqlInterface::get_single_float_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_REFILL_VOLUME_LAST_HOUR,
        ) {
            Ok(c) => Ok(c),
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_SQL_INTERFACE_REFILL,
                    "could not retrieve refill volume of last hour from database ({e:?})"
                );
                Err(e)
            }
        }
    }

    /// Inserts one dataset of refill event into the database.
    /// It provides wrapped into Result type either
    /// - an empty Ok value
    /// - or a value of enum SqlInterfaceError
    /// # Arguments
    /// * `timestamp` - timestamp of refill event to be inserted as type NaiveDateTime
    /// * `duration` - duration of refill event to be inserted in seconds
    /// * `volume` - volume of refill event to be inserted in literser
    /// * `error_code` - numerical error code of refill event to be inserted in liters    
    fn insert_refill_event(
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
                error!(
                    target: Self::LOG_TARGET_SQL_INTERFACE_REFILL,
                    "could not insert refill event into database ({e:?})"
                );
                Err(SqlInterfaceError::InsertRefillEventFailure(
                    "Error when inserting refill event into database.".to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use mysql::prelude::Queryable;

    use crate::config::{read_config_file_with_test_database, ConfigData};
    use crate::sql_interface::{sql_query_strings, SqlInterface, SqlInterfaceError};
    use crate::sql_interface_refill::{DatabaseInterfaceRefillTrait, SqlInterfaceRefill};
    use chrono::{NaiveDateTime, SubsecRound};

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
    // Test case includes all checks for this table combined in one function to limit amount of additional databases.
    // Test case uses test database #37.
    pub fn test_sql_interface_refill() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            37,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_refill = SqlInterfaceRefill::new(sql_interface.get_connection());

        // *** check timestamp of last refill - FAIL ********************************************
        match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }
        assert_eq!(sql_interface_refill.check_empty(), Ok(0));
        assert_eq!(
            sql_interface_refill.get_last_refill_timestamp(),
            Err(SqlInterfaceError::SingleStringRequestNoSingleResponse(
                "SQL database responded with empty response to query: \
            SELECT Timestamp from refill as LSTTSTMP ORDER BY Timestamp DESC LIMIT 1;"
                    .to_string()
            ))
        );
        println!("* checking timestamp of last refill (FAIL) succeeded.");
        // **************************************************************************************

        // *** check timestamp of last refill - PASS ********************************************
        let refill_timestamp =
            NaiveDateTime::parse_from_str("2024-11-20 07:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        match sql_interface_refill.insert_refill_event(refill_timestamp, 3.6, 0.4, 0) {
            Ok(_) => {}
            Err(e) => panic!("Could not insert refill event into database: {e:?}"),
        };
        assert_eq!(sql_interface_refill.check_empty(), Ok(1));
        assert_eq!(
            sql_interface_refill.get_last_refill_timestamp(),
            Ok(refill_timestamp)
        );
        println!("* checking timestamp of last refill (PASS) succeeded.");
        //**************************************************************************************

        //*** check duration since last refill - PASS ******************************************
        let duration_since_last_refill = match sql_interface_refill.get_duration_since_last_refill()
        {
            Ok(c) => c,
            Err(e) => panic!("Could not calculate duration since last refill: {e:?}"),
        };
        let reference_duration_since_last_refill: u64 =
            SqlInterface::get_duration_from_naive_timestamp_to_local_now(refill_timestamp).unwrap()
                as u64;
        assert_eq!(
            duration_since_last_refill,
            reference_duration_since_last_refill
        );
        println!("* checking duration since last refill succeeded.");
        //**************************************************************************************

        // *** check refill count of last 24h (zero) - PASS *************************************
        let refill_count_of_last_24h_zero =
            match sql_interface_refill.get_refill_count_of_last_24h() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill count from last 24h (target=0): {e:?}"),
            };
        assert_eq!(refill_count_of_last_24h_zero, 0);
        println!("* checking refill count of last 24h (target zero) succeeded.");
        // **************************************************************************************

        // *** check refill count of last hour (zero) - PASS ************************************
        let refill_count_of_last_hour_zero =
            match sql_interface_refill.get_refill_count_of_last_hour() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill count from last hour (target=0): {e:?}"),
            };
        assert_eq!(refill_count_of_last_hour_zero, 0);
        println!("* checking refill count of last hour (target zero) succeeded.");
        // **************************************************************************************

        //*** check refill volume of last 24h (zero) - PASS *************************************
        let refill_volume_of_last_24h_zero =
            match sql_interface_refill.get_refill_volume_of_last_24h() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill volume from last 24h (target=0): {e:?}"),
            };
        assert_eq!(refill_volume_of_last_24h_zero, 0.0);
        println!("* checking refill volume of last 24h (target zero) succeeded.");
        // **************************************************************************************

        //*** check refill volume of last hour (zero) - PASS *************************************
        let refill_volume_of_last_hour_zero =
            match sql_interface_refill.get_refill_volume_of_last_hour() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill volume from last hour (target=0): {e:?}"),
            };
        assert_eq!(refill_volume_of_last_hour_zero, 0.0);
        println!("* checking refill volume of last hour (target zero) succeeded.");
        // **************************************************************************************

        // *** check refill count of last 24h (one) - PASS *************************************
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

        // *** check refill volume of last 24h (one) - PASS *************************************
        let refill_volume_of_last_24h_one =
            match sql_interface_refill.get_refill_volume_of_last_24h() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill volue from last 24h (target=0.25): {e:?}"),
            };
        assert_eq!(refill_volume_of_last_24h_one, 0.25);
        println!("* checking refill volume (target non-zero) succeeded.");
        // **************************************************************************************

        // *** check refill count of last hour (one) - PASS *************************************
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

        // *** check refill volume of last hour (one) - PASS *************************************
        let refill_volume_of_last_hour_one =
            match sql_interface_refill.get_refill_volume_of_last_hour() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill volue from last hour (target=0.25): {e:?}"),
            };
        assert_eq!(refill_volume_of_last_hour_one, 0.25);
        println!("* checking refill volume (target non-zero) succeeded.");
        // **************************************************************************************

        // *** check content of last refill event - PASS ****************************************
        // Read back refill event entry from data base
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
        // check if SQL response has right structure
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
        // **************************************************************************************
    }
}
