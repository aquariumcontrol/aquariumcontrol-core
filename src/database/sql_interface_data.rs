/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages all database interactions for time-series sensor and state data.
//!
//! This module provides a dedicated interface, `SqlInterfaceData`, for handling
//! all SQL operations related to the main `data` table. It encapsulates the logic
//! for writing `RecorderDataFrame` objects, which contain a snapshot of all sensor
//! readings and system states at a specific point in time.
//!
//! ## Key Components
//!
//! - **`SqlInterfaceData` Struct**: The primary struct that holds a database
//!   connection and provides methods for all data-logging database operations.
//!
//! - **`new()` Constructor**: A critical entry point that not only creates an
//!   `SqlInterfaceData` instance but also performs essential "fail-fast"
//!   validation. At initialization, it checks whether the number of rows in the
//!   `data` table exceeds the `max_rows_data` limit defined in the application
//!   configuration. This acts as a safeguard against uncontrolled table size at startup.
//!
//! - **`write_data_frame_to_database()`**: The core function of this module. It takes
//!   a `RecorderDataFrame` and constructs a single `INSERT ... ON DUPLICATE KEY UPDATE`
//!   query. It intelligently handles optional or invalid floating-point values
//!   (e.g., `None`, `NaN`, `Infinity`) by converting them to `NULL` before insertion,
//!   ensuring data integrity in the database.
//!
//! ## Design and Purpose
//!
//! The main goal of this module is to provide a robust and encapsulated interface
//! for persisting the application's high-frequency time-series data.
//!
//! - **Encapsulation**: All SQL logic specific to the `data` table is contained
//!   within this module, separating concerns and simplifying the data logger's
//!   responsibilities.
//!
//! - **Data Integrity**: The constructor's validation logic enforces the configured
//!   row limit at startup. The `write_data_frame_to_database` function ensures that
//!   invalid or missing data points are correctly stored as `NULL` in the database,
//!   preventing data corruption.

use mysql::prelude::*;
use mysql::*;

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_query_strings::SQL_QUERY_CHECK_DATA_COUNT;
use crate::database::{sql_interface_error::SqlInterfaceError, sql_query_strings};
use crate::recorder::recorder_data_frame::RecorderDataFrame;

/// Contains the configuration and the implementation of the SQL interface for time-based data.
#[derive(Debug)]
pub struct SqlInterfaceData {
    /// Connection to the database
    pub conn: PooledConn,
}

impl SqlInterfaceData {
    /// Creates a new `SqlInterfaceData` instance.
    ///
    /// This constructor initializes the time-based data SQL interface with an
    /// established database connection. It performs a pre-flight check to ensure
    /// the number of rows in the `data` table does not exceed the configured limit.
    ///
    /// # Arguments
    /// * `conn` - An active, pooled database connection.
    /// * `max_rows_data` - The maximum allowed number of rows in the `data` table.
    ///   If `0`, the check is skipped.
    ///
    /// # Returns
    /// A `Result` containing a new `SqlInterfaceData` instance on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The initial database query to get the table row count fails (`DatabaseCheckDataFailure`).
    /// - The retrieved row count is a negative number, indicating a database issue (`DatabaseDataTableNegativeValue`).
    /// - The number of rows in the `data` table exceeds the configured `max_rows_data` limit (`DatabaseDataTableContainsTooManyRows`).
    pub fn new(
        mut conn: PooledConn,
        max_rows_data: u64,
    ) -> Result<SqlInterfaceData, SqlInterfaceError> {
        let count_rows =
            SqlInterface::get_single_integer_from_database(&mut conn, SQL_QUERY_CHECK_DATA_COUNT)
                .map_err(|e| SqlInterfaceError::DatabaseCheckDataFailure {
                location: module_path!().to_string(),
                source: Box::new(e),
            })?;

        // check the query results
        if count_rows < 0 {
            return Err(SqlInterfaceError::DatabaseDataTableNegativeValue(
                module_path!().to_string(),
                count_rows,
            ));
        }

        if max_rows_data > 0 {
            // execute the check only when the limit is greater than zero
            if count_rows as u64 > max_rows_data {
                return Err(SqlInterfaceError::DatabaseDataTableContainsTooManyRows(
                    module_path!().to_string(),
                    count_rows as u64,
                    max_rows_data,
                ));
            }
        }
        Ok(SqlInterfaceData { conn })
    }

    /// Writes a complete `RecorderDataFrame` to the SQL database.
    ///
    /// This function processes all data points within the provided `RecorderDataFrame`.
    /// It intelligently handles optional (`Option<f32>`) and floating-point values
    /// (`f32`) by converting `None`, `NaN` (Not-a-Number), or infinite values
    /// into `NULL` for database insertion. Boolean options are similarly converted
    /// to `NULL` if `None`.
    ///
    /// The data is inserted into the database. If a record with the same timestamp
    /// already exists, the existing record will be updated (this behavior depends
    /// on the underlying SQL query `SQL_QUERY_WRITE_DATA` using an
    /// `ON DUPLICATE KEY UPDATE` clause).
    ///
    /// # Arguments
    /// * `data` - A reference to the `RecorderDataFrame` containing the data to be written.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the data frame was successfully inserted or updated in the database.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::InsertDataFrameFailure` if the database operation fails.
    /// This can be caused by a lost connection, constraint violations (e.g., foreign keys),
    /// incorrect data types, or a malformed SQL query. The original `mysql::Error` is
    /// included as the source for detailed debugging.
    pub fn write_data_frame_to_database(
        &mut self,
        data: &RecorderDataFrame,
    ) -> Result<(), SqlInterfaceError> {
        let params = params! {
            "timestamp" => data.timestamp,
            "temp" => data.water_temperature,
            "temp_f" => data.water_temperature_filtered,
            "ph" => data.ph,
            "ph_f" => data.ph_filtered,
            "cond" => data.conductivity,
            "cond_f" => data.conductivity_filtered,
            "cond_c" => data.conductivity_compensated,
            "refill" => data.refill_in_progress,
            "tank_pos" => data.tank_level_switch_position,
            "tank_inv" => data.tank_level_switch_invalid,
            "tank_stab" => data.tank_level_switch_position_stabilized,
            "vent" => data.surface_ventilation_status,
            "amb_temp" => data.ambient_temperature,
            "amb_hum" => data.ambient_humidity,
            "heater" => data.heater_status,
        };

        // execute the database query and evaluate the result
        self.conn
            .exec_drop::<_, _>(sql_query_strings::SQL_QUERY_WRITE_DATA, params)
            .map_err(|e| SqlInterfaceError::InsertDataFrameFailure {
                location: module_path!().to_string(),
                query: sql_query_strings::SQL_QUERY_WRITE_DATA.to_string(),
                source: e,
            })
    }
}

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface_data::SqlInterfaceData;
    use crate::database::sql_query_strings::SQL_TABLE_DATA;
    use crate::database::{
        sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
    };
    use crate::recorder::recorder_data_frame::RecorderDataFrame;
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};
    use chrono::NaiveDateTime;
    use mysql::{prelude::Queryable, PooledConn, Row, Value};
    use spin_sleep::SpinSleeper;
    use std::time::Duration;

    #[test]
    // This test case verifies the validation logic within the `SqlInterfaceData::new()` constructor.
    // It covers the following scenarios:
    // 1. Happy Path: Initialization succeeds when the row count is within the limit.
    // 2. Failure on Row Limit: Fails when the `data` table exceeds its max row count.
    // 3. Deactivated Check: Succeeds even with excess rows if the limit is set to 0.
    // Test case uses test database #58.
    fn test_sql_interface_data_new() {
        // --- Common Setup ---
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            58, // Use a new database number to avoid test conflicts
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");

        // --- Test Case 1: Happy Path (row count is within limit) ---
        println!("* Testing new() with valid row count (Happy Path)...");
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_DATA.to_string()).unwrap();

        // Insert one row to have some data
        let mut data_db = SqlInterfaceData {
            conn: sql_interface.get_connection().unwrap(),
        };
        let data_frame = RecorderDataFrame {
            timestamp: chrono::Local::now().naive_local(),
            ..Default::default()
        };
        data_db.write_data_frame_to_database(&data_frame).unwrap();

        // Check: new() should succeed with a generous limit of 10
        let result = SqlInterfaceData::new(sql_interface.get_connection().unwrap(), 10);
        assert!(
            result.is_ok(),
            "Expected new() to succeed on happy path, but it failed: {:?}",
            result.err()
        );
        println!("* Succeeded: Happy path initialization is successful.");

        // --- Test Case 2: Failure on row limit exceeded ---
        println!("* Testing new() with too many rows in the data table...");
        // Insert a second row
        let mut data_db = SqlInterfaceData {
            conn: sql_interface.get_connection().unwrap(),
        };
        let data_frame_2 = RecorderDataFrame {
            timestamp: (chrono::Local::now() + chrono::Duration::seconds(1)).naive_local(),
            ..Default::default()
        };
        data_db.write_data_frame_to_database(&data_frame_2).unwrap();

        // Check: new() should fail with a strict limit of 1
        let result = SqlInterfaceData::new(sql_interface.get_connection().unwrap(), 1);
        assert!(
            matches!(
                result,
                Err(SqlInterfaceError::DatabaseDataTableContainsTooManyRows(
                    _,
                    _,
                    _
                ))
            ),
            "Expected row limit error, but got {:?}",
            result
        );
        println!("* Succeeded: Initialization fails if data table exceeds row limit.");

        // --- Test Case 3: Deactivated check (limit is 0) ---
        println!("* Testing new() with deactivated row limit check (limit = 0)...");
        // We reuse the state from the previous test (2 rows in the table)
        let result = SqlInterfaceData::new(sql_interface.get_connection().unwrap(), 0); // Set the limit to 0
        assert!(
            result.is_ok(),
            "Expected new() to succeed with deactivated check, but it failed: {:?}",
            result.err()
        );
        println!("* Succeeded: Initialization passes when row limit check is deactivated.");
    }

    // This struct contains the first part of one row from table 'data'
    // Due to limitations of the library, this struct can only contain up to 12 elements.
    // The library has no means to handle NULL values.
    // So the data types of this struct are not optional.
    pub struct SqlDataFrame1 {
        pub timestamp: String,
        pub water_temperature: f32,
        pub water_temperature_filtered: f32,
        pub ph: f32,
        pub ph_filtered: f32,
        pub conductivity: f32,
        pub conductivity_filtered: f32,
        pub conductivity_compensated: f32,
    }

    // This struct contains the second part of one row from table 'data'
    // Due to limitations of the library, this struct can only contain up to 12 elements.
    // The library has no means to handle NULL values.
    // So the data types of this struct are not optional.
    pub struct SqlDataFrame2 {
        pub refill_in_progress: bool,
        pub tank_level_switch_position: bool,
        pub tank_level_switch_invalid: bool,
        pub tank_level_switch_position_stabilized: bool,
        pub surface_ventilation_status: bool,
        pub ambient_temperature: f32,
        pub ambient_humidity: f32,
        pub heater_status: bool,
    }

    // Retrieves a single `RecorderDataFrame` from the 'data' table in the database.
    //
    // This function executes two separate SQL queries to fetch all components of a
    // `RecorderDataFrame` for a specific timestamp. It strictly expects exactly one
    // matching row for each part.
    //
    // Note: Due to limitations of the underlying SQL library, this function **cannot**
    // properly handle database entries where values are `NULL`. It expects all fields
    // to contain concrete data.
    //
    // Arguments:
    // * `conn`: A mutable reference to an active database connection.
    // * `timestamp`: The `NaiveDateTime` to identify the specific data frame to retrieve.
    //
    // # Returns
    // A `Result` containing the reconstructed `RecorderDataFrame` if both queries succeed
    // and return exactly one row with non-NULL data.
    //
    // # Errors
    // This function will return an error if:
    // - Either of the underlying database queries fail.
    // - Either query returns zero rows (`SingleDataFrameRequestEmptyResponse`).
    // - Either query returns more than one row (`SingleDataFrameRequestNoSingleResponse`).
    // - The timestamp string retrieved from the database cannot be parsed into a `NaiveDateTime`.
    pub fn get_single_dataframe_from_database(
        conn: &mut PooledConn,
        timestamp: NaiveDateTime,
    ) -> Result<RecorderDataFrame, SqlInterfaceError> {
        let timestamp_string = timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
        // Get data frame part 1 from the database
        let sql_query_string_1 = str::replace(
            sql_query_strings::SQL_QUERY_READ_DATA_FRAME_1,
            "#timestamp",
            &timestamp_string,
        );
        let data_frame_1_array: Vec<SqlDataFrame1> = match conn.query_map(
            sql_query_string_1.clone(),
            |(
                timestamp,
                water_temp,
                water_temp_filt,
                ph,
                ph_filt,
                conduc,
                conduc_filt,
                conduc_comp,
            )| SqlDataFrame1 {
                timestamp,
                water_temperature: water_temp,
                water_temperature_filtered: water_temp_filt,
                ph,
                ph_filtered: ph_filt,
                conductivity: conduc,
                conductivity_filtered: conduc_filt,
                conductivity_compensated: conduc_comp,
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get data frame part 1 from SQL database: {e:?} using query string {}", sql_query_string_1);
            }
        };
        let sql_query_string_2 = str::replace(
            sql_query_strings::SQL_QUERY_READ_DATA_FRAME_2,
            "#timestamp",
            &timestamp_string,
        );
        let data_frame_2_array: Vec<SqlDataFrame2> = match conn.query_map(
            sql_query_string_2.clone(),
            |(
                refill_in_progress,
                tank_level_switch_position,
                tank_level_switch_invalid,
                tank_level_switch_position_stabilized,
                surface_ventilation_status,
                ambient_temperature,
                ambient_humidity,
                heater_status,
            )| SqlDataFrame2 {
                refill_in_progress,
                tank_level_switch_position,
                tank_level_switch_invalid,
                tank_level_switch_position_stabilized,
                surface_ventilation_status,
                ambient_temperature,
                ambient_humidity,
                heater_status,
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get data frame part 2 from SQL database: {e:?} using query string {}", sql_query_string_2);
            }
        };

        // check the error case if there is no response
        if (data_frame_1_array.len() == 0) || (data_frame_2_array.len() == 0) {
            return Err(SqlInterfaceError::SingleDataFrameRequestEmptyResponse(
                module_path!().to_string(),
                sql_query_string_1,
                sql_query_string_2,
            ));
        }
        // check the error case if there is more than one response
        if (data_frame_2_array.len() > 1) || (data_frame_2_array.len() > 1) {
            return Err(SqlInterfaceError::SingleDataFrameRequestNoSingleResponse(
                module_path!().to_string(),
                sql_query_string_1,
                sql_query_string_2,
            ));
        }
        let data_frame_1 = &data_frame_1_array[0];
        let data_frame_2 = &data_frame_2_array[0];

        Ok(RecorderDataFrame {
            timestamp: NaiveDateTime::parse_from_str(&data_frame_1.timestamp, "%Y-%m-%d %H:%M:%S")
                .expect("Failed to convert string to timestamp of type NaiveDateTime."),
            water_temperature: Some(data_frame_1.water_temperature),
            water_temperature_filtered: Some(data_frame_1.water_temperature_filtered),
            ph: Some(data_frame_1.ph),
            ph_filtered: Some(data_frame_1.ph_filtered),
            conductivity: Some(data_frame_1.conductivity),
            conductivity_filtered: Some(data_frame_1.conductivity_filtered),
            conductivity_compensated: Some(data_frame_1.conductivity_compensated),
            refill_in_progress: Some(data_frame_2.refill_in_progress),
            tank_level_switch_position: Some(data_frame_2.tank_level_switch_position),
            tank_level_switch_invalid: Some(data_frame_2.tank_level_switch_invalid),
            tank_level_switch_position_stabilized: Some(
                data_frame_2.tank_level_switch_position_stabilized,
            ),
            surface_ventilation_status: Some(data_frame_2.surface_ventilation_status),
            ambient_temperature: Some(data_frame_2.ambient_temperature),
            ambient_humidity: Some(data_frame_2.ambient_humidity),
            heater_status: Some(data_frame_2.heater_status),
        })
    }

    // The function executes two SQL statements to retrieve one row from table 'data'
    // In Ok case, the function returns one struct of type DataFrame wrapped in Result.
    // Note: This function exclusively handles entries with NULL data.
    // # Arguments
    // * `conn` - connection to SQL database
    // * `timestamp` - timestamp data to be inserted into the query
    pub fn check_database_row_if_all_null(conn: &mut PooledConn, timestamp: NaiveDateTime) {
        let timestamp_string = timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
        // Get a series of NULL values from the database
        let sql_query_string = str::replace(
            sql_query_strings::SQL_QUERY_READ_NULL_DATA_FRAME,
            "#timestamp",
            &timestamp_string,
        );
        let row: Row = conn
            .query_first(sql_query_string)
            .unwrap()
            .expect("Querying single row of NULL values failed.");

        for cell in row.columns_ref() {
            let cell_value = &row[cell.name_str().as_ref()];
            match cell_value {
                _val @ Value::NULL => {
                    println!("Identified NULL value.");
                }
                _ => {
                    panic!("Identified non-=NULL value");
                }
            }
        }
    }

    #[test]
    // Test case includes all checks for writing to the data table combined in one function to
    // limit the number of additional databases.
    // Test case uses test database #32.
    pub fn test_sql_interface_data_write_data_frame() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            32,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_data = config.sql_interface.max_rows_data;
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_data =
            SqlInterfaceData::new(sql_interface.get_connection().unwrap(), max_rows_data).unwrap();

        // *** insert and load RecorderDataFrame ***************************************************
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_DATA.to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        let current_timestamp = match SqlInterface::get_current_timestamp(&mut sql_interface.conn) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get current timestamp from database: {e:?}");
            }
        };
        let dataframe_reference_1 = RecorderDataFrame {
            timestamp: current_timestamp,
            water_temperature: Some(25.5),
            water_temperature_filtered: Some(25.0),
            ph: Some(7.0),
            ph_filtered: Some(7.5),
            conductivity: Some(50000.0),
            conductivity_filtered: Some(51000.0),
            conductivity_compensated: Some(52000.0),
            refill_in_progress: Some(false),
            tank_level_switch_position: Some(false),
            tank_level_switch_invalid: Some(false),
            tank_level_switch_position_stabilized: Some(false),
            surface_ventilation_status: Some(true),
            ambient_temperature: Some(22.0),
            ambient_humidity: Some(60.0),
            heater_status: Some(true),
        };
        match sql_interface_data.write_data_frame_to_database(&dataframe_reference_1) {
            Ok(()) => {}
            Err(e) => {
                panic!("Could not write dataframe to database: {e:?}");
            }
        }
        let dataframe_test = match get_single_dataframe_from_database(
            &mut sql_interface_data.conn,
            dataframe_reference_1.timestamp,
        ) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not retrieve data frame from database: {e:?}");
            }
        };
        assert_eq!(dataframe_test, dataframe_reference_1);
        println!("* checking inserting data frame into empty table succeeded");
        // *****************************************************************************************

        // *** insert and load the same RecorderDataFrame again (check ON DUPLICATE functionality) *
        let dataframe_reference_2 = RecorderDataFrame {
            timestamp: dataframe_reference_1.timestamp,
            water_temperature: Some(26.5),
            water_temperature_filtered: Some(26.0),
            ph: Some(8.0),
            ph_filtered: Some(8.5),
            conductivity: Some(50001.0),
            conductivity_filtered: Some(51001.0),
            conductivity_compensated: Some(52001.0),
            refill_in_progress: Some(true),
            tank_level_switch_position: Some(true),
            tank_level_switch_invalid: Some(true),
            tank_level_switch_position_stabilized: Some(true),
            surface_ventilation_status: Some(false),
            ambient_temperature: Some(23.0),
            ambient_humidity: Some(61.0),
            heater_status: Some(false),
        };
        match sql_interface_data.write_data_frame_to_database(&dataframe_reference_2) {
            Ok(()) => {}
            Err(e) => {
                panic!("Could not write dataframe to database: {e:?}");
            }
        }
        let dataframe_test = match get_single_dataframe_from_database(
            &mut sql_interface_data.conn,
            dataframe_reference_2.timestamp,
        ) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not retrieve data frame from database: {e:?}");
            }
        };
        assert_eq!(dataframe_test, dataframe_reference_2);
        println!("* checking inserting already existing data frame into empty table succeeded");
        // *****************************************************************************************

        // *** insert and load DataFrame with NULL data ********************************************
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_DATA.to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        let current_timestamp = match SqlInterface::get_current_timestamp(&mut sql_interface.conn) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get current timestamp from database: {e:?}");
            }
        };
        let dataframe_reference_3 = RecorderDataFrame {
            timestamp: current_timestamp,
            water_temperature: None,
            water_temperature_filtered: None,
            ph: None,
            ph_filtered: None,
            conductivity: None,
            conductivity_filtered: None,
            conductivity_compensated: None,
            refill_in_progress: None,
            tank_level_switch_position: None,
            tank_level_switch_invalid: None,
            tank_level_switch_position_stabilized: None,
            surface_ventilation_status: None,
            ambient_temperature: None,
            ambient_humidity: None,
            heater_status: None,
        };
        match sql_interface_data.write_data_frame_to_database(&dataframe_reference_3) {
            Ok(()) => {}
            Err(e) => {
                panic!("Could not write dataframe to database: {e:?}");
            }
        }
        check_database_row_if_all_null(
            &mut sql_interface_data.conn,
            dataframe_reference_3.timestamp,
        );
        println!("* checking inserting data frame with NULL values into empty table succeeded");

        // sleep for one second to get distinct timestamps
        let spin_sleeper = SpinSleeper::default();
        let sleep_duration_one_second = Duration::from_secs(1);
        spin_sleeper.sleep(sleep_duration_one_second);

        // create additional dataframe
        let mut dataframe_reference_4 = dataframe_reference_3.clone();
        let current_timestamp = match SqlInterface::get_current_timestamp(&mut sql_interface.conn) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get current timestamp from database: {e:?}");
            }
        };
        dataframe_reference_4.timestamp = current_timestamp;

        // insert additional dataframe
        match sql_interface_data.write_data_frame_to_database(&dataframe_reference_4) {
            Ok(()) => {}
            Err(e) => {
                panic!("Could not write dataframe to database: {e:?}");
            }
        }

        let test_result = SqlInterfaceData::new(sql_interface.get_connection().unwrap(), 1);
        assert!(matches!(
            test_result,
            Err(SqlInterfaceError::DatabaseDataTableContainsTooManyRows(
                _,
                _,
                _
            ))
        ));
        // *****************************************************************************************
    }
}
