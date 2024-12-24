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

/// Contains the configuration and the implementation of the SQL interface for Balling.
pub struct SqlInterfaceBalling {
    /// Connection to database
    conn: PooledConn,
}

/// Contains set value data for each pump.
pub struct BallingSetVal {
    /// ID of the pump
    /// Attribute is public becaused access from within test cases.
    pub pump_id: i32,

    /// flow rate of the pump in ml/sec
    pub dosing_speed: f32,

    /// dosing volume in ml which the pump shall deliver each hour
    pub dosing_volume: f32,

    /// label which describes the fluid that the pump is delivering
    /// Attribute is public becaused access from within test cases.
    pub label: String,
}

impl SqlInterfaceBalling {
    const LOG_TARGET_SQL_INTERFACE_BALLING: &'static str = "sql_interface_balling";

    /// Provides a struct of type SqlInterfaceBalling
    /// # Arguments
    /// * `config` - configuration data for Balling dosing control
    pub fn new(conn: PooledConn) -> SqlInterfaceBalling {
        SqlInterfaceBalling { conn }
    }

    /// Retrieves the timestamp of the last Balling dosing event from SQL database and returns wrapped
    /// into a Result type either:
    /// - result as type NaiveDateTime or
    /// - value of enum SqlInterfaceError
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

    /// Requests current timestamp and timestamp of last balling dosing event and calculates the difference between them in seconds.
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
            None => Option::None,
        };

        Ok(result_opt)
    }

    /// Inserts one dataset of Balling dosing event into the SQL database.
    /// # Arguments
    /// * `timestamp` - timestamp to be inserted as NaiveDateTime
    /// * `pump_id` - id of the pump for which the Balling dosing event shall be logged
    /// * `dosing_volue` - volume in ml which has been dosed into the aquarium
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
                error!(
                    target: Self::LOG_TARGET_SQL_INTERFACE_BALLING,
                    "could not insert Balling dosing event into database ({e:?})"
                );
                Err(SqlInterfaceError::InsertBallingEventFailure(
                    "Error when inserting Balling dosing event into database.".to_string(),
                ))
            }
        }
    }

    /// Retrieves Balling set values from SQL database for a given pump id
    /// and evaluates if the response is a single data set.
    /// * `pump_id` - id of the pump for which the Balling dosing event shall be retrieved
    pub fn get_single_balling_setval_from_database(
        &mut self,
        pump_id: i64,
    ) -> Result<BallingSetVal, SqlInterfaceError> {
        let sql_query_string = str::replace(
            sql_query_strings::SQL_QUERY_READ_BALLING_SETVALS,
            "#pump_id",
            &pump_id.to_string(),
        );
        // Get single data set from database
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
                    "Could not get single Balling set value from SQL database using query:"
                        .to_string()
                        + &sql_query_string,
                ));
            }
        };

        // check error case if there is no response
        if single_balling_setval_array.len() == 0 {
            return Err(SqlInterfaceError::SingleBallingSetValRequestFailure(
                "SQL database responded with empty response to query: ".to_string()
                    + &sql_query_string,
            ));
        }

        // check error case if there is more than one response
        if single_balling_setval_array.len() > 1 {
            return Err(SqlInterfaceError::SingleBallingSetValRequestFailure(
                "SQL database responded with multiple responses to request to query:".to_string()
                    + &sql_query_string,
            ));
        }
        Ok(single_balling_setval_array.remove(0))
    }

    #[cfg(test)]
    pub fn insert_pump_configuration(
        &mut self,
        pump_id: i64,
        dosing_speed: f32,
        dosing_volume: f32,
        label: String,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_PUMP_CONFIGURATION.to_owned();
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
                error!(
                    target: Self::LOG_TARGET_SQL_INTERFACE_BALLING,
                    "could not insert Balling pump configuration into database ({e:?})"
                );
                Err(SqlInterfaceError::InsertBallingPumpConfigurationFailure(
                    "Error when inserting Balling pump configuration into database.".to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use chrono::NaiveDateTime;
    use mysql::{params, prelude::Queryable};

    use crate::config::{read_config_file_with_test_database, ConfigData};
    use crate::sql_interface::{sql_query_strings, SqlInterface, SqlInterfaceError};
    use crate::sql_interface_balling::SqlInterfaceBalling;

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
    /// Test case includes all checks for this table combined in one function to limit amount of additional databases.
    // Test case uses test database #30.
    pub fn test_sql_interface_balling_events() {
        let pump_id = 1;
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_sql_interface.toml".to_string(),
            30,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_balling = SqlInterfaceBalling::new(sql_interface.get_connection());

        // *** check timestamp of last Balling dosing - FAIL ************************************
        match SqlInterface::truncate_table(&mut sql_interface, "ballingdosinglog".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        assert_eq!(
            sql_interface_balling.get_last_balling_dosing_timestamp(pump_id),
            Ok(None)
        );
        println!("* checking timestamp of last Balling dosing log (FAIL) succeeded.");
        // **************************************************************************************

        // *** check timestamp of last Balling dosing - PASS ************************************
        let balling_timestamp =
            NaiveDateTime::parse_from_str("2024-11-20 11:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        match sql_interface_balling.insert_balling_event(balling_timestamp, pump_id, 0.5) {
            Ok(_) => {}
            Err(e) => panic!("Could not insert Balling dosing event into database: {e:?}"),
        };
        assert_eq!(
            sql_interface_balling.get_last_balling_dosing_timestamp(pump_id),
            Ok(Some(balling_timestamp))
        );
        println!("* checking timestamp of last Balling dosing (PASS) succeeded.");
        // **************************************************************************************

        // *** check content of last Balling dosing - PASS **************************************
        // Read back Balling dosing event entry from data base
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
        // check if SQL response has right structure
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
        // **************************************************************************************

        // *** check duration since last Balling dosing - PASS **********************************
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
        // **************************************************************************************
    }

    #[test]
    /// Test case includes all checks for this table combined in one function to limit amount of additional databases.
    // Test case uses test database #31.
    pub fn test_sql_interface_balling_setvals() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_sql_interface.toml".to_string(),
            31,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_balling = SqlInterfaceBalling::new(sql_interface.get_connection());

        // *** read balling set vals from empty table - FAIL ************************************
        match SqlInterface::truncate_table(&mut sql_interface, "ballingsetvals".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        match sql_interface_balling.get_single_balling_setval_from_database(1) {
            Ok(_) => {
                panic!("Reading from empty set value table returned value.");
            }
            Err(e) => {
                assert_eq!(
                    e,
                    SqlInterfaceError::SingleBallingSetValRequestFailure(
                        "SQL database responded with empty response to query: \
                        SELECT pumpid, dosingspeed, dosingvolume, label FROM ballingsetvals \
                        WHERE pumpid=1;"
                            .to_string()
                    )
                );
            }
        }
        println!("* checking reading from empty Balling dosing set value table succeeded.");
        // **************************************************************************************

        // *** read balling set vals from pre-filled table - PASS *******************************
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
