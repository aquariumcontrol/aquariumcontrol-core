use chrono::*;
use mysql::prelude::*;
use mysql::*;

use crate::sql_interface::sql_query_strings;
use crate::sql_interface::SqlInterface;
use crate::sql_interface::SqlInterfaceError;

/// This struct contains the configuration and the implementation of the SQL interface for Balling.
pub struct SqlInterfaceBalling {
    /// Connection to database
    pub conn: PooledConn,
}

/// This struct contains set value data for each pump
pub struct BallingSetVal {
    /// ID of the pump
    pub pump_id: i32,

    /// flow rate of the pump in ml/sec
    pub dosing_speed: f32,

    /// dosing volume in ml which the pump shall deliver each hour
    pub dosing_volume: f32,

    /// label which describes the fluid that the pump is delivering
    pub label: String,
}

impl SqlInterfaceBalling {
    /// The function creates an instance of SqlInterfaceBalling.
    pub fn new(conn: PooledConn) -> SqlInterfaceBalling {
        SqlInterfaceBalling { conn }
    }

    /// The function retrieves the timestamp of the last Balling dosing event from SQL database.
    fn get_last_balling_dosing_timestamp(&mut self) -> Result<NaiveDateTime, SqlInterfaceError> {
        SqlInterface::get_timestamp(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_LAST_BALLING_DOSING_TIMESTAMP,
        )
    }

    /// The function request current timestamp and timestamp of last balling dosing event and calculates the difference between them in seconds.
    pub fn get_duration_since_last_balling_dosing(&mut self) -> Result<i64, SqlInterfaceError> {
        let current_datetime = match SqlInterface::get_current_timestamp(&mut self.conn) {
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

    /// The function executes an SQL statement to retrieve Balling set values for a given pump id
    /// and evaluates if the response is a single data set.
    pub fn get_single_balling_setval_from_database(
        &mut self,
        pump_id: i32,
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
}

#[cfg(test)]
pub mod tests {
    use mysql::params;
    use mysql::prelude::Queryable;

    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::sql_interface::{sql_query_strings, SqlInterface, SqlInterfaceError};
    use crate::sql_interface_balling::SqlInterfaceBalling;
    use chrono::NaiveDateTime;

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
    /// all test cases for this crate/table combined in one function to avoid problems with multithreading
    pub fn test_sql_interface_balling_events() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_sql_interface.toml".to_string());
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
            sql_interface_balling.get_last_balling_dosing_timestamp(),
            Err(SqlInterfaceError::SingleStringRequestNoSingleResponse(
                "SQL database responded with empty response to query: \
            SELECT Timestamp from ballingdosinglog as LSTTSTMP ORDER BY Timestamp DESC LIMIT 1;"
                    .to_string()
            ))
        );
        println!("* checking timestamp of last Balling dosing log (FAIL) succeeded.");
        // **************************************************************************************

        // *** check timestamp of last Balling dosing - PASS ************************************
        let balling_timestamp =
            NaiveDateTime::parse_from_str("2022-04-11 11:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        match sql_interface_balling.insert_balling_event(balling_timestamp, 1, 0.5) {
            Ok(_) => {}
            Err(e) => panic!("Could not insert Balling dosing event into database: {e:?}"),
        };
        assert_eq!(
            sql_interface_balling.get_last_balling_dosing_timestamp(),
            Ok(balling_timestamp)
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
            match sql_interface_balling.get_duration_since_last_balling_dosing() {
                Ok(c) => c,
                Err(e) => panic!("Could not calculate duration since last Balling dosing: {e:?}"),
            };
        let reference_duration_since_last_balling_dosing =
            SqlInterface::get_duration_from_naive_timestamp_to_local_now(balling_timestamp)
                .unwrap();
        assert_eq!(
            duration_since_last_balling_dosing,
            reference_duration_since_last_balling_dosing
        );
        println!("* checking duration since last Balling dosing succeeded.");
        // **************************************************************************************
    }

    #[test]
    /// all test cases for this crate/table combined in one function to avoid problems with multithreading
    pub fn test_sql_interface_balling_setvals() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_sql_interface.toml".to_string());
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
