/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::*;
use log::error;
use mysql::prelude::*;
use mysql::*;

use crate::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
};

/// Trait for interfacing between refill and SQL database
/// This trait is prerequisite to use mock implementation for testing
pub trait DatabaseInterfaceRefillTrait {
    fn get_duration_since_last_refill(&mut self) -> Result<u64, SqlInterfaceError>;

    fn get_positive_integer_value_from_database(
        &mut self,
        sql_query_string: &str,
    ) -> Result<u64, SqlInterfaceError>;

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
    /// Execute a ping to the database to avoid timeout.
    fn ping(&mut self);
}

/// Contains the configuration and the implementation of the SQL interface for Refill.
pub struct SqlInterfaceRefill {
    /// Connection to the database
    pub conn: PooledConn,
}

impl SqlInterfaceRefill {
    /// Creates a new `SqlInterfaceRefill` instance.
    ///
    /// This constructor initializes the refill management SQL interface with an established
    /// database connection.
    ///
    /// # Arguments
    /// * `conn` - An active database connection pool object.
    ///
    /// # Returns
    /// A new `SqlInterfaceRefill` struct.
    pub fn new(conn: PooledConn) -> SqlInterfaceRefill {
        SqlInterfaceRefill { conn }
    }

    /// Fetches the timestamp of the most recent water refill event from the database.
    ///
    /// This private helper function queries the database for the latest recorded
    /// refill event and converts its timestamp into a `NaiveDateTime` object.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(NaiveDateTime)`: The timestamp of the last refill event.
    /// - `Err(SqlInterfaceError)`: If there's an issue retrieving or parsing the timestamp,
    ///   such as the table being empty, a database communication error, or a timestamp
    ///   conversion failure.
    fn get_last_refill_timestamp(&mut self) -> Result<NaiveDateTime, SqlInterfaceError> {
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
    /// with the timestamp of the last recorded refill event. It ensures the result
    /// is a non-negative duration.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(u64)`: The number of seconds that have passed since the last refill event.
    ///   Returns 0 if the last refill was precisely at the current moment or in the future
    ///   (which would indicate a data anomaly).
    /// - `Err(SqlInterfaceError::NegativeInteger)`: If the calculated duration is negative
    ///   (meaning the "last refill" timestamp is in the future relative to the current timestamp,
    ///   suggesting data corruption).
    /// - `Err(SqlInterfaceError)`: If any error occurs while retrieving either the
    ///   current timestamp or the last refill timestamp from the database.
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
            Err(SqlInterfaceError::NegativeTimeDeltaSeconds(
                delta_time.num_seconds(),
            ))
        }
    }

    /// Executes an SQL query to retrieve a single integer value, validating it as non-negative.
    ///
    /// This private helper function is used for database queries that are expected to return
    /// a single integer. It further ensures that the retrieved integer is non-negative,
    /// converting it to `u64` on success, or returning a specific error if it's negative.
    ///
    /// # Arguments
    /// * `sql_query_string` - The SQL query string to be executed, expected to yield a single integer result.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(u64)`: The retrieved integer value, successfully converted to an unsigned 64-bit integer.
    /// - `Err(SqlInterfaceError::NegativeInteger)`: If the database query returns a negative integer.
    /// - `Err(SqlInterfaceError)`: If the underlying `get_single_integer_from_database` call fails
    ///   (e.g., query execution error, no result, or multiple results).
    fn get_positive_integer_value_from_database(
        &mut self,
        sql_query_string: &str,
    ) -> Result<u64, SqlInterfaceError> {
        match SqlInterface::get_single_integer_from_database(&mut self.conn, sql_query_string) {
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

    /// Checks whether the refill events table in the database is empty.
    ///
    /// This function executes an SQL query to determine if any entries exist
    /// in the refill log.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(0)` if the refill table contains no entries.
    /// - `Ok(1)` if the refill table contains at least one entry.
    /// - `Err(SqlInterfaceError)` if a database error occurs during the check,
    ///   or if an unexpected (e.g., negative or multiple) integer result is returned.
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
    /// A `Result` which is:
    /// - `Ok(u64)`: The number of refill events recorded in the last hour.
    /// - `Err(SqlInterfaceError)`: If a database error occurs during the count retrieval,
    ///   or if an unexpected (e.g., negative or multiple) integer result is returned.
    fn get_refill_count_of_last_hour(&mut self) -> Result<u64, SqlInterfaceError> {
        self.get_positive_integer_value_from_database(
            sql_query_strings::SQL_QUERY_READ_REFILL_COUNT_LAST_HOUR,
        )
        .map_err(|e| SqlInterfaceError::RefillPastDataRetrievalFailed {
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
    /// A `Result` which is:
    /// - `Ok(u64)`: The number of refill events recorded in the last 24 hours.
    /// - `Err(SqlInterfaceError)`: If a database error occurs during the count retrieval,
    ///   or if an unexpected (e.g., negative or multiple) integer result is returned.
    fn get_refill_count_of_last_24h(&mut self) -> Result<u64, SqlInterfaceError> {
        self.get_positive_integer_value_from_database(
            sql_query_strings::SQL_QUERY_READ_REFILL_COUNT_LAST_24H,
        )
        .map_err(|e| SqlInterfaceError::RefillPastDataRetrievalFailed {
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
    /// A `Result` which is:
    /// - `Ok(f64)`: The total volume of water refilled in the last 24 hours.
    /// - `Err(SqlInterfaceError)`: If a database error occurs during the volume retrieval,
    ///   or if an unexpected (e.g., zero or multiple) float result is returned.
    fn get_refill_volume_of_last_24h(&mut self) -> Result<f64, SqlInterfaceError> {
        SqlInterface::get_single_float_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_REFILL_VOLUME_LAST_24H,
        )
        .map_err(|e| SqlInterfaceError::RefillPastDataRetrievalFailed {
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
    /// A `Result` which is:
    /// - `Ok(f64)`: The total volume of water refilled in the last hour.
    /// - `Err(SqlInterfaceError)`: If a database error occurs during the volume retrieval,
    ///   or if an unexpected (e.g., zero or multiple) float result is returned.
    fn get_refill_volume_of_last_hour(&mut self) -> Result<f64, SqlInterfaceError> {
        SqlInterface::get_single_float_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_REFILL_VOLUME_LAST_HOUR,
        )
        .map_err(|e| SqlInterfaceError::RefillPastDataRetrievalFailed {
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
    /// * `error_code` - A numerical error code associated with the refill event (0 for success, non-zero for errors).
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(())` if the refill event record was successfully inserted.
    /// - `Err(SqlInterfaceError::InsertRefillEventFailure)` if an error occurs
    ///   during the database insertion.
    fn insert_refill_event(
        &mut self,
        timestamp: NaiveDateTime,
        duration: f64,
        volume: f64,
        error_code: i32,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_REFILL_EVENT.to_owned();
        self.conn
            .exec_drop::<_, _>(
                sql_query_string.clone(),
                params! {
                "timestamp" => timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
                "duration" => duration,
                "volume" => volume,
                "error_code" => error_code,
                },
            )
            .map_err(|e| SqlInterfaceError::InsertRefillEventFailure {
                sql_query_string,
                source: e,
            })
    }

    /// Execute a ping to the database to avoid timeout.
    fn ping(&mut self) {
        if let Err(e) = SqlInterface::ping_database(&mut self.conn) {
            error!(target: module_path!(), "Encountered error while pinging database: {e:?}");
        }
    }
}

#[cfg(test)]
pub mod tests {
    use chrono::{NaiveDateTime, SubsecRound};
    use mysql::prelude::Queryable;

    use crate::database::sql_interface_refill::{DatabaseInterfaceRefillTrait, SqlInterfaceRefill};
    use crate::database::{
        sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
    };
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};

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
    // Test case includes all checks for this table combined in one function to limit the number of additional databases.
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

        // *** check timestamp of last refill *************************************************
        match SqlInterface::truncate_table(&mut sql_interface, "refill".to_string()) {
            Ok(_) => {}
            Err(e) => {
                panic!("Could not prepare test case: {e:?}")
            }
        }
        assert!(matches!(sql_interface_refill.check_empty(), Ok(_)));
        assert!(matches!(
            sql_interface_refill.get_last_refill_timestamp(),
            Err(SqlInterfaceError::TimestampRequestFailure { source: _ })
        ));
        println!("* checking timestamp of last refill (FAIL) succeeded.");
        // **************************************************************************************

        // *** check timestamp of last refill - ****************************************************
        let refill_timestamp =
            NaiveDateTime::parse_from_str("2025-04-12 07:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        match sql_interface_refill.insert_refill_event(refill_timestamp, 3.6, 0.4, 0) {
            Ok(_) => {}
            Err(e) => panic!("Could not insert refill event into database: {e:?}"),
        };
        assert!(matches!(sql_interface_refill.check_empty(), Ok(1)));
        assert!(matches!(
            sql_interface_refill.get_last_refill_timestamp(),
            Ok(_)
        ));
        println!("* checking timestamp of last refill succeeded.");
        //**************************************************************************************

        //*** check duration since last refill - **************************************************
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

        // *** check refill count of last 24h (zero) - *********************************************
        let refill_count_of_last_24h_zero =
            match sql_interface_refill.get_refill_count_of_last_24h() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill count from last 24h (target=0): {e:?}"),
            };
        assert_eq!(refill_count_of_last_24h_zero, 0);
        println!("* checking refill count of last 24h (target zero) succeeded.");
        // **************************************************************************************

        // *** check refill count of last hour (zero) - ********************************************
        let refill_count_of_last_hour_zero =
            match sql_interface_refill.get_refill_count_of_last_hour() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill count from last hour (target=0): {e:?}"),
            };
        assert_eq!(refill_count_of_last_hour_zero, 0);
        println!("* checking refill count of last hour (target zero) succeeded.");
        // **************************************************************************************

        //*** check refill volume of last 24h (zero) - *********************************************
        let refill_volume_of_last_24h_zero =
            match sql_interface_refill.get_refill_volume_of_last_24h() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill volume from last 24h (target=0): {e:?}"),
            };
        assert_eq!(refill_volume_of_last_24h_zero, 0.0);
        println!("* checking refill volume of last 24h (target zero) succeeded.");
        // **************************************************************************************

        //*** check refill volume of last hour (zero) - *********************************************
        let refill_volume_of_last_hour_zero =
            match sql_interface_refill.get_refill_volume_of_last_hour() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill volume from last hour (target=0): {e:?}"),
            };
        assert_eq!(refill_volume_of_last_hour_zero, 0.0);
        println!("* checking refill volume of last hour (target zero) succeeded.");
        // **************************************************************************************

        // *** check refill count of last 24h (one) - *********************************************
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

        // *** check refill volume of last 24h (one) - *********************************************
        let refill_volume_of_last_24h_one =
            match sql_interface_refill.get_refill_volume_of_last_24h() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill volume from last 24h (target=0.25): {e:?}"),
            };
        assert_eq!(refill_volume_of_last_24h_one, 0.25);
        println!("* checking refill volume (target non-zero) succeeded.");
        // **************************************************************************************

        // *** check refill count of last hour (one) - *********************************************
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

        // *** check refill volume of last hour (one) - *********************************************
        let refill_volume_of_last_hour_one =
            match sql_interface_refill.get_refill_volume_of_last_hour() {
                Ok(c) => c,
                Err(e) => panic!("Could not get refill volume from last hour (target=0.25): {e:?}"),
            };
        assert_eq!(refill_volume_of_last_hour_one, 0.25);
        println!("* checking refill volume (target non-zero) succeeded.");
        // **************************************************************************************

        // *** check the content of last refill event - ********************************************
        // Read back refill event entry from the database
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
        // check if SQL response has the right structure
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
