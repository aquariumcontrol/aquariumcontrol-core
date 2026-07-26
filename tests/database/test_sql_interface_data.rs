/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests all database interactions for time-series sensor and state data.

use aquarium_control::database::sql_interface_data::SqlInterfaceData;
use aquarium_control::database::sql_test_query_strings::SQL_TABLE_DATA;
use aquarium_control::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_test_query_strings,
};
use aquarium_control::recorder::recorder_data_frame::RecorderDataFrame;
use aquarium_control::utilities::config::{read_config_file_with_test_database, ConfigData};
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
// [itest->swreq~recorder_connect_database~1]
fn test_sql_interface_data_new() {
    // --- Common Setup ---
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
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
        conn_for_timestamp: sql_interface.get_connection().unwrap(),
        conn: sql_interface.get_connection().unwrap(),
    };
    let data_frame = RecorderDataFrame {
        timestamp: chrono::Local::now().naive_local(),
        ..Default::default()
    };
    SqlInterfaceData::write_data_frame_to_database(&mut data_db.conn, &data_frame).unwrap();

    // Check: new() should succeed with a generous limit of 10
    let result = SqlInterfaceData::new(
        sql_interface.get_connection().unwrap(),
        sql_interface.get_connection().unwrap(),
        10,
    );
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
        conn_for_timestamp: sql_interface.get_connection().unwrap(),
        conn: sql_interface.get_connection().unwrap(),
    };
    let data_frame_2 = RecorderDataFrame {
        timestamp: (chrono::Local::now() + chrono::Duration::seconds(1)).naive_local(),
        ..Default::default()
    };
    SqlInterfaceData::write_data_frame_to_database(&mut data_db.conn, &data_frame_2).unwrap();

    // Check: new() should fail with a strict limit of 1
    let result = SqlInterfaceData::new(
        sql_interface.get_connection().unwrap(),
        sql_interface.get_connection().unwrap(),
        1,
    );
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
    let result = SqlInterfaceData::new(
        sql_interface.get_connection().unwrap(),
        sql_interface.get_connection().unwrap(),
        0, // Set the limit to 0
    );
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

/// Retrieves a single `RecorderDataFrame` from the 'data' table in the database.
///
/// This function executes two separate SQL queries to fetch all components of a
/// `RecorderDataFrame` for a specific timestamp. It strictly expects exactly one
/// matching row for each part.
///
/// Note: Due to limitations of the underlying SQL library, this function **cannot**
/// properly handle database entries where values are `NULL`. It expects all fields
/// to contain concrete data.
///
/// Arguments:
/// * `conn`: A mutable reference to an active database connection.
/// * `timestamp`: The `NaiveDateTime` to identify the specific data frame to retrieve.
///
/// # Returns
/// A `Result` containing the reconstructed `RecorderDataFrame` if both queries succeed
/// and return exactly one row with non-NULL data.
///
/// # Errors
/// This function will return an error if:
/// - Either of the underlying database queries fail.
/// - Either query returns zero rows (`SingleDataFrameRequestEmptyResponse`).
/// - Either query returns more than one row (`SingleDataFrameRequestNoSingleResponse`).
/// - The timestamp string retrieved from the database cannot be parsed into a `NaiveDateTime`.
#[allow(clippy::result_large_err)]
pub fn get_single_dataframe_from_database(
    conn: &mut PooledConn,
    timestamp: NaiveDateTime,
) -> Result<RecorderDataFrame, SqlInterfaceError> {
    let timestamp_string = timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
    // Get data frame part 1 from the database
    let sql_query_string_1 = str::replace(
        sql_test_query_strings::SQL_QUERY_READ_DATA_FRAME_1,
        "#timestamp",
        &timestamp_string,
    );
    let data_frame_1_array: Vec<SqlDataFrame1> = match conn.query_map(
        sql_query_string_1.clone(),
        |(timestamp, water_temp, water_temp_filt, ph, ph_filt, conduc, conduc_filt)| {
            SqlDataFrame1 {
                timestamp,
                water_temperature: water_temp,
                water_temperature_filtered: water_temp_filt,
                ph,
                ph_filtered: ph_filt,
                conductivity: conduc,
                conductivity_filtered: conduc_filt,
            }
        },
    ) {
        Ok(c) => c,
        Err(e) => {
            panic!(
                "Could not get data frame part 1 from SQL database: {e:?} using query string {}",
                sql_query_string_1
            );
        }
    };
    let sql_query_string_2 = str::replace(
        sql_test_query_strings::SQL_QUERY_READ_DATA_FRAME_2,
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
            panic!(
                "Could not get data frame part 2 from SQL database: {e:?} using query string {}",
                sql_query_string_2
            );
        }
    };

    // check the error case if there is no response
    if (data_frame_1_array.is_empty()) || (data_frame_2_array.is_empty()) {
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
/// Checks the database row if all null.
///
/// # Arguments
/// * `conn` - An active pooled database connection.
/// * `timestamp` - A parameter representing `timestamp`.
pub fn check_database_row_if_all_null(conn: &mut PooledConn, timestamp: NaiveDateTime) {
    let timestamp_string = timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
    // Get a series of NULL values from the database
    let sql_query_string = str::replace(
        sql_test_query_strings::SQL_QUERY_READ_NULL_DATA_FRAME,
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
        "config/aquarium_control_test_generic.toml".to_string(),
        32,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let max_rows_data = config.sql_interface.max_rows_data;
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
        .expect("Initialization of SQL interface for test failed.");
    let mut sql_interface_data = SqlInterfaceData::new(
        sql_interface.get_connection().unwrap(),
        sql_interface.get_connection().unwrap(),
        max_rows_data,
    )
    .unwrap();
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
        refill_in_progress: Some(false),
        tank_level_switch_position: Some(false),
        tank_level_switch_invalid: Some(false),
        tank_level_switch_position_stabilized: Some(false),
        surface_ventilation_status: Some(true),
        ambient_temperature: Some(22.0),
        ambient_humidity: Some(60.0),
        heater_status: Some(true),
    };
    match SqlInterfaceData::write_data_frame_to_database(
        &mut sql_interface_data.conn,
        &dataframe_reference_1,
    ) {
        Ok(()) => {}
        Err(e) => {
            panic!("Could not write dataframe to database: {e:?}");
        }
    }
    let dataframe_test = match get_single_dataframe_from_database(
        &mut sql_interface_data.conn_for_timestamp,
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
        refill_in_progress: Some(true),
        tank_level_switch_position: Some(true),
        tank_level_switch_invalid: Some(true),
        tank_level_switch_position_stabilized: Some(true),
        surface_ventilation_status: Some(false),
        ambient_temperature: Some(23.0),
        ambient_humidity: Some(61.0),
        heater_status: Some(false),
    };
    match SqlInterfaceData::write_data_frame_to_database(
        &mut sql_interface_data.conn,
        &dataframe_reference_2,
    ) {
        Ok(()) => {}
        Err(e) => {
            panic!("Could not write dataframe to database: {e:?}");
        }
    }
    let dataframe_test = match get_single_dataframe_from_database(
        &mut sql_interface_data.conn_for_timestamp,
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
        refill_in_progress: None,
        tank_level_switch_position: None,
        tank_level_switch_invalid: None,
        tank_level_switch_position_stabilized: None,
        surface_ventilation_status: None,
        ambient_temperature: None,
        ambient_humidity: None,
        heater_status: None,
    };
    match SqlInterfaceData::write_data_frame_to_database(
        &mut sql_interface_data.conn,
        &dataframe_reference_3,
    ) {
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
    match SqlInterfaceData::write_data_frame_to_database(
        &mut sql_interface_data.conn,
        &dataframe_reference_4,
    ) {
        Ok(()) => {}
        Err(e) => {
            panic!("Could not write dataframe to database: {e:?}");
        }
    }

    let test_result = SqlInterfaceData::new(
        sql_interface.get_connection().unwrap(),
        sql_interface.get_connection().unwrap(),
        1,
    );
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
