/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::*;
use log::error;
use mysql::prelude::*;
use mysql::*;

use crate::database::sql_query_strings::SQL_QUERY_CHECK_BALLING_SETVALS_NULL;
use crate::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
};

/// Contains the configuration and the implementation of the SQL interface for Balling.
pub struct SqlInterfaceBalling {
    /// Connection to the database
    conn: PooledConn,
}

/// Contains set value data for each pump.
pub struct BallingSetVal {
    /// ID of the pump
    /// Attribute is public because access from within test cases.
    /// Data is not used in the implementation. Therefore, it is tagged as unused.
    #[allow(unused)]
    pub pump_id: i32,

    /// flow rate of the pump in ml/sec
    pub dosing_speed: f32,

    /// dosing volume in ml which the pump shall deliver each hour
    pub dosing_volume: f32,

    /// A label which describes the fluid that the pump is delivering
    /// Attribute is public because access from within test cases.
    /// Data is not used in the implementation. Therefore, it is tagged as unused.
    #[allow(unused)]
    pub label: String,
}

impl SqlInterfaceBalling {
    /// Creates a new `SqlInterfaceBalling` instance.
    ///
    /// This constructor initializes the Balling SQL interface with an established
    /// database connection. Before, it checks whether the Balling set value table
    /// contains any NULL values. If so, it panics, as this indicates a data consistency
    /// issue.
    ///
    /// # Arguments
    /// * `conn` - An active database connection pool object.
    ///
    /// # Returns
    /// * A new `SqlInterfaceBalling` struct in case of success.
    /// * An error `SqlInterfaceError` in case of failure.
    pub fn new(mut conn: PooledConn) -> Result<Self, SqlInterfaceError> {
        let count_null_values = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_BALLING_SETVALS_NULL,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckBallingSetValsFailure {
            source: Box::new(e),
        })?;

        if count_null_values > 0 {
            return Err(SqlInterfaceError::DatabaseBallingSetValTableContainsNull(
                count_null_values,
            ));
        }

        Ok(SqlInterfaceBalling { conn })
    }

    /// Retrieves the timestamp of the last Balling dosing event for a specific pump.
    ///
    /// This function queries the database for the most recent dosing timestamp associated
    /// with the given `pump_id`. Since a pump might not have any previous dosing events,
    /// the result is wrapped in an `Option`.
    ///
    /// # Arguments
    /// * `pump_id` - The unique identifier of the pump for which to retrieve the last dosing timestamp.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(Some(NaiveDateTime))` if a last dosing timestamp is found for the pump.
    /// - `Ok(None)` if no previous dosing events are found for the specified pump.
    /// - `Err(SqlInterfaceError)` if there's any issue during the database query or timestamp parsing.
    fn get_last_balling_dosing_timestamp(
        &mut self,
        pump_id: i64,
    ) -> Result<Option<NaiveDateTime>, SqlInterfaceError> {
        let sql_query = str::replace(
            sql_query_strings::SQL_QUERY_READ_LAST_BALLING_DOSING_TIMESTAMP,
            "#pump_id",
            &pump_id.to_string(),
        );
        SqlInterface::get_optional_timestamp(&mut self.conn, &sql_query)
    }

    /// Calculates the duration in seconds since the last Balling dosing event for a given pump.
    ///
    /// This function retrieves both the current database timestamp and the last dosing
    /// timestamp for the specified `pump_id`. It then computes the time difference
    /// between these two points in seconds.
    ///
    /// # Arguments
    /// * `pump_id` - The unique identifier of the pump.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(Some(i64))` containing the number of seconds that have passed since the
    ///   last Balling dosing event. A positive value indicates time elapsed.
    /// - `Ok(None)` if there has been no previous Balling dosing event recorded for this pump.
    /// - `Err(SqlInterfaceError)` if there's an issue retrieving either the current
    ///   timestamp or the last dosing timestamp from the database.
    pub fn get_duration_since_last_balling_dosing(
        &mut self,
        pump_id: i64,
    ) -> Result<Option<i64>, SqlInterfaceError> {
        let current_datetime = match SqlInterface::get_current_timestamp(&mut self.conn) {
            Ok(c) => c,
            Err(e) => {
                return Err(e);
            }
        };

        let last_balling_dosing_datetime_opt = match self.get_last_balling_dosing_timestamp(pump_id)
        {
            Ok(c) => c,
            Err(e) => {
                return Err(e);
            }
        };

        let result_opt = match last_balling_dosing_datetime_opt {
            Some(last_balling_dosing_datetime) => {
                let delta_time = current_datetime - last_balling_dosing_datetime;
                Some(delta_time.num_seconds())
            }
            None => None,
        };

        Ok(result_opt)
    }

    /// Inserts a new Balling dosing event record into the SQL database.
    ///
    /// This function logs details of a Balling dosing operation, including the
    /// exact time, the pump used, and the volume of fluid dispensed.
    ///
    /// # Arguments
    /// * `timestamp` - The `NaiveDateTime` when the dosing event occurred.
    /// * `pump_id` - The identifier of the pump that performed the dosing.
    /// * `dosing_volume` - The volume (in milliliters) of fluid dispensed during this event.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(())` if the Balling event record was successfully inserted.
    /// - `Err(SqlInterfaceError::InsertBallingEventFailure)` if an error occurs
    ///   during the database insertion.
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
            Err(e) => Err(SqlInterfaceError::InsertBallingEventFailure { source: e }),
        }
    }

    /// Retrieves a single set of Balling dosing parameters for a specified pump from the database.
    ///
    /// This function queries the database for the Balling set values (e.g., dosing speed, volume, label)
    /// associated with a given `pump_id`. It strictly expects to find exactly one matching dataset.
    ///
    /// # Arguments
    /// * `pump_id` - The unique identifier of the pump for which to retrieve the set values.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(BallingSetVal)` containing the retrieved dosing parameters if exactly one
    ///   matching record is found.
    /// - `Err(SqlInterfaceError::SingleBallingSetValRequestFailure)` if the database query
    ///   fails, or if it returns no records, or if it returns more than one record
    ///   (indicating a data consistency issue).
    pub fn get_single_balling_setval_from_database(
        &mut self,
        pump_id: i64,
    ) -> Result<BallingSetVal, SqlInterfaceError> {
        let sql_query_string = str::replace(
            sql_query_strings::SQL_QUERY_READ_BALLING_SETVALS,
            "#pump_id",
            &pump_id.to_string(),
        );
        // Get a single data set from the database
        let mut single_balling_setval_array = match self.conn.query_map(
            sql_query_string.clone(),
            |(pump_id, dosing_speed, dosing_volume, label)| BallingSetVal {
                pump_id,
                dosing_speed,
                dosing_volume,
                label,
            },
        ) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::SingleBallingSetValRequestFailure(
                    sql_query_string,
                ));
            }
        };

        // check the error case if there is no response
        if single_balling_setval_array.is_empty() {
            return Err(SqlInterfaceError::SingleBallingSetValEmptyResponse(
                sql_query_string,
            ));
        }

        // check the error case if there is more than one response
        if single_balling_setval_array.len() > 1 {
            return Err(SqlInterfaceError::SingleBallingSetValNoSingleResponse(
                sql_query_string,
            ));
        }
        Ok(single_balling_setval_array.remove(0))
    }

    #[cfg(test)]
    // Inserts a pump configuration dataset into the database.
    //
    // This helper function is used exclusively in test environments to
    // pre-populate the database with specific pump configuration data,
    // allowing for consistent and isolated testing of related functionalities.
    //
    // # Arguments
    // * `pump_id` - The unique identifier for the pump.
    // * `dosing_speed` - The flow rate of the pump in milliliters per second.
    // * `dosing_volume` - The volume (in milliliters) intended to be dosed in a single event.
    // * `label` - A descriptive label for the fluid being dispensed by the pump.
    //
    // # Returns
    // A `Result` which is:
    // - `Ok(())` if the pump configuration was successfully inserted.
    // - `Err(SqlInterfaceError::InsertBallingPumpConfigurationFailure)` if an error occurs
    //   during the database insertion.
    pub fn insert_pump_configuration(
        &mut self,
        pump_id: i64,
        dosing_speed: f32,
        dosing_volume: f32,
        label: String,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_PUMP_CONFIGURATION.to_owned();
        let sql_query_string_for_error = sql_query_string.clone();
        match self.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "pump_id" => pump_id,
            "dosing_speed" => dosing_speed,
            "dosing_volume" => dosing_volume,
            "label" => label,
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                error!( // logging for testing purposes only
                    target: module_path!(),
                    "could not insert Balling pump configuration into database ({e:?})"
                );
                Err(SqlInterfaceError::InsertBallingPumpConfigurationFailure(
                    sql_query_string_for_error,
                ))
            }
        }
    }

    /// Execute a ping to the database to avoid timeout.
    pub fn ping(&mut self) {
        if let Err(e) = SqlInterface::ping_database(&mut self.conn) {
            error!(target: module_path!(), "Encountered error while pinging database: {e:?}");
        }
    }
}

#[cfg(test)]
pub mod tests {
    use chrono::NaiveDateTime;
    use mysql::{params, prelude::Queryable};

    use crate::database::sql_interface_balling::SqlInterfaceBalling;
    use crate::database::{
        sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
    };
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};

    pub struct SqlBallingEvent {
        timestamp: String,
        pump_id: i32,
        dosing_volume: f32,
    }

    pub struct BallingEvent {
        timestamp: NaiveDateTime,
        pump_id: i32,
        dosing_volume: f32,
    }

    pub const SQL_QUERY_WRITE_BALLING_SETVALS: &str =
        "INSERT into ballingsetvals(pumpid, dosingspeed, dosingvolume, label) values(:pump_id, :dosing_speed, :dosing_volume, :label);";

    #[test]
    // Test case verifies the behavior of all functions accessing the Balling event log table.
    // They are all combined in one function to limit the number of additional databases.
    // Test case uses test database #30.
    pub fn test_sql_interface_balling_events() {
        let pump_id = 1;
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            30,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_balling =
            SqlInterfaceBalling::new(sql_interface.get_connection()).unwrap();

        // *** check the timestamp of last Balling dosing ******************************************
        match SqlInterface::truncate_table(&mut sql_interface, "ballingdosinglog".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        assert!(matches!(
            sql_interface_balling.get_last_balling_dosing_timestamp(pump_id),
            Ok(None)
        ));
        println!("* checking timestamp of last Balling dosing log succeeded.");
        // *****************************************************************************************

        // *** check timestamp of last Balling dosing **********************************************
        let balling_timestamp =
            NaiveDateTime::parse_from_str("2025-04-12 11:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        match sql_interface_balling.insert_balling_event(balling_timestamp, pump_id, 0.5) {
            Ok(_) => {}
            Err(e) => panic!("Could not insert Balling dosing event into database: {e:?}"),
        };
        assert!(matches!(
            sql_interface_balling.get_last_balling_dosing_timestamp(pump_id),
            Ok(_)
        ));
        println!("* checking timestamp of last Balling dosing succeeded.");
        // *****************************************************************************************

        // *** check the content of last Balling dosing ********************************************
        // Read back Balling dosing event entry from the database
        let single_balling_event_entry_array = match sql_interface_balling.conn.query_map(
            sql_query_strings::SQL_QUERY_READ_BALLING_EVENT,
            |(timestamp, pump_id, dosing_volume)| SqlBallingEvent {
                timestamp,
                pump_id,
                dosing_volume,
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not read Balling dosing event from data base: {e:?}");
            }
        };
        // check if SQL response has the right structure
        assert_eq!(single_balling_event_entry_array.len(), 1);
        // check if SQL response contains the correct data
        let balling_event = BallingEvent {
            timestamp: NaiveDateTime::parse_from_str(
                single_balling_event_entry_array[0].timestamp.as_str(),
                "%Y-%m-%d %H:%M:%S",
            )
            .expect("conversion of timestamp to NaiveDate failed."),
            pump_id: single_balling_event_entry_array[0].pump_id,
            dosing_volume: single_balling_event_entry_array[0].dosing_volume,
        };
        assert_eq!(balling_event.timestamp, balling_timestamp);
        assert_eq!(balling_event.pump_id, 1);
        assert_eq!(balling_event.dosing_volume, 0.5);
        // *****************************************************************************************

        // *** check duration since last Balling dosing ********************************************
        let duration_since_last_balling_dosing =
            match sql_interface_balling.get_duration_since_last_balling_dosing(1) {
                Ok(c) => c,
                Err(e) => panic!("Could not calculate duration since last Balling dosing: {e:?}"),
            };
        let reference_duration_since_last_balling_dosing =
            SqlInterface::get_duration_from_naive_timestamp_to_local_now(balling_timestamp)
                .unwrap();
        assert_eq!(
            duration_since_last_balling_dosing,
            Some(reference_duration_since_last_balling_dosing)
        );
        println!("* checking duration since last Balling dosing succeeded.");
        // *****************************************************************************************
    }

    #[test]
    // Test case verifies the behavior of all functions accessing the Balling set value table.
    // They are all combined in one function to limit the number of additional databases.
    // Test case uses test database #31.
    pub fn test_sql_interface_balling_setvals() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            31,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_balling =
            SqlInterfaceBalling::new(sql_interface.get_connection()).unwrap();

        // *** read balling set vals from empty table **********************************************
        match SqlInterface::truncate_table(&mut sql_interface, "ballingsetvals".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        match sql_interface_balling.get_single_balling_setval_from_database(1) {
            Ok(_) => {
                panic!("Reading from empty set value table returned value.");
            }
            Err(e) => {
                assert!(matches!(
                    e,
                    SqlInterfaceError::SingleBallingSetValEmptyResponse(_)
                ));
            }
        }
        println!("* checking reading from empty Balling dosing set value table succeeded.");
        // *****************************************************************************************

        // *** read balling set vals from pre-filled table *****************************************
        // prepare database: insert Balling set vals
        let sql_query_string = SQL_QUERY_WRITE_BALLING_SETVALS.to_string();
        match sql_interface_balling.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "pump_id" => 1,
            "dosing_speed" => 2.0,
            "dosing_volume" => 0.5,
            "label" => "test_pump".to_string(),
            },
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Error when inserting Balling set values into database: {e:?}");
            }
        };
        match sql_interface_balling.get_single_balling_setval_from_database(1) {
            Ok(c) => {
                assert_eq!(c.pump_id, 1);
                assert_eq!(c.dosing_speed, 2.0);
                assert_eq!(c.dosing_volume, 0.5);
                assert_eq!(c.label, "test_pump");
            }
            Err(e) => {
                panic!("Error when reading from pre-filled Balling set value table: {e:?}");
            }
        }
        println!("* checking reading existing Balling dosing set values succeeded.");
        // **************************************************************************************
    }
}
