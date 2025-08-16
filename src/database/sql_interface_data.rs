/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use log::error;
use mysql::prelude::*;
use mysql::*;

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_query_strings::SQL_QUERY_CHECK_DATA_COUNT;
use crate::database::{sql_interface_error::SqlInterfaceError, sql_query_strings};
use crate::recorder::recorder_data_frame::RecorderDataFrame;

/// Contains the configuration and the implementation of the SQL interface for time-based data.
pub struct SqlInterfaceData {
    /// Connection to the database
    pub conn: PooledConn,
}

impl SqlInterfaceData {
    /// Creates a new `SqlInterfaceData` instance.
    ///
    /// This constructor initializes the time-based data SQL interface
    /// with an established database connection.
    ///
    /// # Arguments
    /// * `conn` - An active database connection pool object.
    /// * `max_rows_data` - Maximum allowed number of rows in the Balling set value table.
    /// # Returns
    /// A new `SqlInterfaceData` struct.
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
            if count_rows > max_rows_data.cast_signed() {
                return Err(SqlInterfaceError::DatabaseDataTableContainsTooManyRows(
                    module_path!().to_string(),
                    count_rows.cast_unsigned(),
                    max_rows_data,
                ));
            }
        }
        Ok(SqlInterfaceData { conn })
    }

    /// Converts an optional `f32` value into its `String` representation,
    /// specifically handling `None`, `NaN`, and infinite values by returning "NULL".
    ///
    /// This helper function ensures that floating-point numbers are correctly formatted
    /// for database insertion, where `NULL` is used for missing or invalid numerical data.
    ///
    /// # Arguments
    /// * `value` - An `Option<f32>` containing the floating-point value to convert.
    ///   If `None`, `NaN`, or `Infinity`, it will be represented as "NULL".
    ///
    /// # Returns
    /// A `String` representing the float value, or "NULL" if the input is `None`,
    /// Not-a-Number (`NaN`), positive infinity (`f32::INFINITY`), or negative infinity (`f32::NEG_INFINITY`).
    pub fn get_string_from_value(value: Option<f32>) -> String {
        let null_string = "NULL".to_string();

        match value {
            Some(c) => match c.is_nan() | c.is_infinite() {
                true => null_string,
                false => c.to_string(),
            },
            None => null_string,
        }
    }

    /// Writes a complete `RecorderDataFrame` to the SQL database.
    ///
    /// This function processes all data points within the provided `RecorderDataFrame`.
    /// It intelligently handles optional (`Option<f32>`) and floating-point values
    /// (`f32`) by converting `None`, `NaN` (Not-a-Number), or infinite values
    /// (positive and negative infinity) into `NULL` for database insertion. Boolean
    /// options are similarly converted to `NULL` if `None`.
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
    /// A `Result` which is:
    /// - `Ok(())` if the data frame was successfully inserted or updated in the database.
    /// - `Err(SqlInterfaceError::InsertDataFrameFailure)` if any error occurs during
    ///   the database operation.    
    pub fn write_data_frame_to_database(
        &mut self,
        data: &RecorderDataFrame,
    ) -> Result<(), SqlInterfaceError> {
        let null_string = "NULL".to_string();

        let timestamp_string = data.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();

        // replace signal placeholders with values
        let water_temperature_string = Self::get_string_from_value(data.water_temperature);
        let sql_query_water_temperature_replaced = str::replace(
            sql_query_strings::SQL_QUERY_WRITE_DATA,
            "#TemperatureUnfiltered",
            &water_temperature_string,
        );

        let water_temperature_filtered_string =
            Self::get_string_from_value(data.water_temperature_filtered);
        let sql_query_water_temperature_filtered_replaced = str::replace(
            &sql_query_water_temperature_replaced,
            "#TemperatureFiltered",
            &water_temperature_filtered_string,
        );

        let ph_string = Self::get_string_from_value(data.ph);
        let sql_query_ph_replaced = str::replace(
            &sql_query_water_temperature_filtered_replaced,
            "#pHUnfiltered",
            &ph_string,
        );

        let ph_filtered_string = Self::get_string_from_value(data.ph_filtered);
        let sql_query_ph_filtered_replaced =
            str::replace(&sql_query_ph_replaced, "#pHFiltered", &ph_filtered_string);

        let conductivity_string = Self::get_string_from_value(data.conductivity);
        let sql_query_conductivity_replaced = str::replace(
            &sql_query_ph_filtered_replaced,
            "#ConductivityUnfiltered",
            &conductivity_string,
        );

        let conductivity_filtered_string = Self::get_string_from_value(data.conductivity_filtered);
        let sql_query_conductivity_filtered_replaced = str::replace(
            &sql_query_conductivity_replaced,
            "#ConductivityFiltered",
            &conductivity_filtered_string,
        );

        let conductivity_compensated_string =
            Self::get_string_from_value(data.conductivity_compensated);
        let sql_query_string_conductivity_compensated_replaced = str::replace(
            &sql_query_conductivity_filtered_replaced,
            "#ConductivityCompensated",
            &conductivity_compensated_string,
        );

        let refill_in_progress_string: String;
        let sql_query_refill_in_progress_replaced = str::replace(
            &sql_query_string_conductivity_compensated_replaced,
            "#RefillInProgress",
            match data.refill_in_progress {
                Some(c) => {
                    refill_in_progress_string = c.to_string();
                    &refill_in_progress_string
                }
                None => &null_string,
            },
        );

        let tank_level_switch_position_stabilized_string: String;
        let sql_query_main_tank_level_switch_position_stabilized_replaced = str::replace(
            &sql_query_refill_in_progress_replaced,
            "#TankLevelSwitchPositionStabilized",
            match data.tank_level_switch_position_stabilized {
                Some(c) => {
                    tank_level_switch_position_stabilized_string = c.to_string();
                    &tank_level_switch_position_stabilized_string
                }
                None => &null_string,
            },
        );

        let tank_level_switch_invalid_string: String;
        let sql_query_main_tank_level_switch_invalid_replaced = str::replace(
            &sql_query_main_tank_level_switch_position_stabilized_replaced,
            "#TankLevelSwitchInvalid",
            match data.tank_level_switch_invalid {
                Some(c) => {
                    tank_level_switch_invalid_string = c.to_string();
                    &tank_level_switch_invalid_string
                }
                None => &null_string,
            },
        );

        let tank_level_switch_position_string: String;
        let sql_query_main_tank_level_switch_position_replaced = str::replace(
            &sql_query_main_tank_level_switch_invalid_replaced,
            "#TankLevelSwitchPosition",
            match data.tank_level_switch_position {
                Some(c) => {
                    tank_level_switch_position_string = c.to_string();
                    &tank_level_switch_position_string
                }
                None => &null_string,
            },
        );

        let surface_ventilation_string: String;
        let sql_query_surface_ventilation_status_replaced = str::replace(
            &sql_query_main_tank_level_switch_position_replaced,
            "#SurfaceVentilation",
            match data.surface_ventilation_status {
                Some(c) => {
                    surface_ventilation_string = c.to_string();
                    &surface_ventilation_string
                }
                None => &null_string,
            },
        );

        let ambient_temperature_string = Self::get_string_from_value(data.ambient_temperature);
        let sql_query_ambient_temperature_replaced = str::replace(
            &sql_query_surface_ventilation_status_replaced,
            "#AmbTemp",
            &ambient_temperature_string,
        );

        let ambient_humidity_string = Self::get_string_from_value(data.ambient_humidity);
        let sql_query_ambient_humidity_replaced = str::replace(
            &sql_query_ambient_temperature_replaced,
            "#AmbHum",
            &ambient_humidity_string,
        );

        let heater_string: String;
        let sql_query_string = str::replace(
            &sql_query_ambient_humidity_replaced,
            "#Heater",
            match data.heater_status {
                Some(c) => {
                    heater_string = c.to_string();
                    &heater_string
                }
                None => &null_string,
            },
        );

        // execute the database query and evaluate the result
        match self.conn.exec_drop::<_, _>(
            sql_query_string.clone(),
            params! {
            "timestamp" => timestamp_string.clone(),
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => Err(SqlInterfaceError::InsertDataFrameFailure {
                location: module_path!().to_string(),
                sql_query_string,
                source: e,
            }),
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
    use crate::database::sql_interface_data::SqlInterfaceData;
    use crate::database::sql_query_strings::SQL_TABLE_DATA;
    use crate::database::{sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError};
    use crate::recorder::recorder_data_frame::RecorderDataFrame;
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};
    use chrono::NaiveDateTime;
    use mysql::{prelude::Queryable, PooledConn, Row, Value};
    use spin_sleep::SpinSleeper;
    use std::time::Duration;

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

    // Query for reading the first part from data for test cases operating on a valid database entry
    pub const SQL_QUERY_READ_DATA_FRAME_1: &str = "SELECT Timestamp, \
        Temperature, TemperatureFiltered, \
        pH, pHFiltered, \
        Conductivity, ConductivityFiltered, ConductivityCompensated \
        FROM data WHERE Timestamp='#timestamp';";

    // Query for reading the second part from data for test cases operating on a valid database entry
    pub const SQL_QUERY_READ_DATA_FRAME_2: &str = "SELECT \
        RefillInProgress, TankLevelSwitchPosition, TankLevelSwitchInvalid, \
        TankLevelSwitchPositionStabilized, SurfaceVentilation, \
        AmbTemp, AmbHum, Heater FROM data \
        WHERE Timestamp = '#timestamp';";

    // Query for reading a complete data set for test cases with operating on an invalid database entry
    pub const SQL_QUERY_READ_NULL_DATA_FRAME: &str = "SELECT \
        Temperature, TemperatureFiltered, \
        pH, pHFiltered, \
        Conductivity, ConductivityFiltered, ConductivityCompensated \
        RefillInProgress, TankLevelSwitchPosition, TankLevelSwitchInvalid, \
        TankLevelSwitchPositionStabilized, SurfaceVentilation, \
        AmbTemp, AmbHum, Heater FROM data \
        WHERE Timestamp = '#timestamp';";

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
    // Returns:
    // A `Result` which is:
    // - `Ok(RecorderDataFrame)`: The reconstructed data frame if both queries return
    //   exactly one row with non-NULL data.
    // - `Err(SqlInterfaceError::SingleDataFrameRequestNoSingleResponse)`: If either
    //   query returns zero rows or more than one row.
    // - `Err(SqlInterfaceError)`: If any other database error occurs during fetching,
    //   or if the timestamp string from the database cannot be parsed.
    pub fn get_single_dataframe_from_database(
        conn: &mut PooledConn,
        timestamp: NaiveDateTime,
    ) -> Result<RecorderDataFrame, SqlInterfaceError> {
        let timestamp_string = timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
        // Get data frame part 1 from the database
        let sql_query_string_1 =
            str::replace(SQL_QUERY_READ_DATA_FRAME_1, "#timestamp", &timestamp_string);
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
        let sql_query_string_2 =
            str::replace(SQL_QUERY_READ_DATA_FRAME_2, "#timestamp", &timestamp_string);
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
            SQL_QUERY_READ_NULL_DATA_FRAME,
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
    // Test case includes all checks for this table combined in one function to limit the number of additional databases.
    // Test case uses test database #32.
    pub fn test_sql_interface_data() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            32,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_data = config.sql_interface.max_rows_data;
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_data =
            SqlInterfaceData::new(sql_interface.get_connection(), max_rows_data).unwrap();

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

        let test_result = SqlInterfaceData::new(sql_interface.get_connection(), 1);
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
