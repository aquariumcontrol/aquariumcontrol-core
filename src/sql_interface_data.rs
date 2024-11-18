/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::*;
use log::error;
use mysql::prelude::*;
use mysql::*;
use std::fmt;

use crate::sql_interface::sql_query_strings;
use crate::sql_interface::SqlInterfaceError;

#[derive(Debug, PartialEq)]
/// Contains the data to be logged wrapped into Option.
/// For each signal, where the data could be not be acquired. The None value is used.
/// The None value is represented as NULL in the SQL database.
/// The struct only contains minimal implementation for development purposes.
pub struct DataFrame {
    /// timestamp when the frame was captured
    pub timestamp: NaiveDateTime,

    /// measured water temperature in °C
    pub water_temperature: Option<f32>,

    /// filtered water temperature in °C
    pub water_temperature_filtered: Option<f32>,

    /// measured pH value of water
    pub ph: Option<f32>,

    /// filtered pH value of water
    pub ph_filtered: Option<f32>,

    /// measured conductivity of water in uS/cm
    pub conductivity: Option<f32>,

    /// filtered conductivity of water in uS/cm
    pub conductivity_filtered: Option<f32>,

    /// conductivity in uS/cm compensated for temperature deviation
    pub conductivity_compensated: Option<f32>,

    /// status bit indicating that fresh water refill is ongoing
    pub refill_in_progress: Option<bool>,

    /// measured tank level switch position (true=high, false=low)
    pub tank_level_switch_position: Option<bool>,

    /// tank level switch validity bit (true=invalid, false=valid)
    pub tank_level_switch_invalid: Option<bool>,

    /// calculated tank level switch position stabilized (true=high, false=low)
    pub tank_level_switch_position_stabilized: Option<bool>,

    /// status bit indicating that ventilation is switched on
    pub surface_ventilation_status: Option<bool>,

    /// measured ambient temoerature in °C
    pub ambient_temperature: Option<f32>,

    /// measured ambient humidity in %
    pub ambient_humidity: Option<f32>,

    /// status bit indicating that heater is switched on
    pub heater_status: Option<bool>,
}

/// Provides output of the enum values for debugging purposes.
/// # Arguments
/// * `f` - configuration for formatting
impl fmt::Display for DataFrame {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let none_string = "None".to_string();
        let water_temperature_string = match self.water_temperature {
            Some(water_temperature) => water_temperature.to_string(),
            None => none_string.clone(),
        };
        let water_temperature_filtered_string = match self.water_temperature_filtered {
            Some(water_temperature_filtered) => water_temperature_filtered.to_string(),
            None => none_string.clone(),
        };
        let ph_string = match self.ph {
            Some(ph) => ph.to_string(),
            None => none_string.clone(),
        };
        let ph_filtered_string = match self.ph_filtered {
            Some(ph_filtered) => ph_filtered.to_string(),
            None => none_string.clone(),
        };
        let conductivity_string = match self.conductivity {
            Some(conductivity) => conductivity.to_string(),
            None => none_string.clone(),
        };
        let conductivity_filtered_string = match self.conductivity_filtered {
            Some(conductivity_filtered) => conductivity_filtered.to_string(),
            None => none_string.clone(),
        };
        let conductivity_compensated_string = match self.conductivity_compensated {
            Some(conductivity_compensated) => conductivity_compensated.to_string(),
            None => none_string.clone(),
        };
        let refill_in_progress_string = match self.refill_in_progress {
            Some(refill_in_progress) => match refill_in_progress {
                true => "yes".to_string(),
                false => "no".to_string(),
            },
            None => none_string.clone(),
        };
        let tank_level_switch_position_string = match self.tank_level_switch_position {
            Some(tank_level_switch_position) => match tank_level_switch_position {
                true => "high".to_string(),
                false => "low".to_string(),
            },
            None => none_string.clone(),
        };
        let tank_level_switch_invalid_string = match self.tank_level_switch_invalid {
            Some(tank_level_switch_invalid) => match tank_level_switch_invalid {
                true => "invalid".to_string(),
                false => "valid".to_string(),
            },
            None => none_string.clone(),
        };
        let tank_level_switch_position_stabilized_string =
            match self.tank_level_switch_position_stabilized {
                Some(tank_level_switch_position_stabilized) => {
                    match tank_level_switch_position_stabilized {
                        true => "high".to_string(),
                        false => "low".to_string(),
                    }
                }
                None => none_string.clone(),
            };
        let surface_ventilation_status_string = match self.surface_ventilation_status {
            Some(surface_ventilation_status) => match surface_ventilation_status {
                true => "on".to_string(),
                false => "off".to_string(),
            },
            None => none_string.clone(),
        };
        let ambient_temperature_string = match self.ambient_temperature {
            Some(ambient_temperature) => ambient_temperature.to_string(),
            None => none_string.clone(),
        };
        let ambient_humidity_string = match self.ambient_humidity {
            Some(ambient_humidity) => ambient_humidity.to_string(),
            None => none_string.clone(),
        };
        let heater_status_string = match self.heater_status {
            Some(heater_status) => match heater_status {
                true => "on".to_string(),
                false => "off".to_string(),
            },
            None => none_string.clone(),
        };
        write!(
            f,
            "\n**************************************\ntimestamp: {}\
            \nwater_temperature: {},\nwater_temperature_filtered: {}, \
            \nph: {},\nph_filtered: {}, \
            \nconductivity: {},\nconductivity_filtered: {},\nconductivity_compensated: {}, \
            \nrefill_in_progress: {}, \
            \ntank_level_switch_position: {},\ntank_level_switch_invalid: {},\ntank_level_switch_position_stabilized: {}, \
            \nsurface_ventilation_status: {},\
            \nambient_temperature: {},\nambient_humidity: {}, \
            \nheater_status: {}\n**************************************\n",
            self.timestamp,
            water_temperature_string,
            water_temperature_filtered_string,
            ph_string,
            ph_filtered_string,
            conductivity_string,
            conductivity_filtered_string,
            conductivity_compensated_string,
            refill_in_progress_string,
            tank_level_switch_position_string,
            tank_level_switch_invalid_string,
            tank_level_switch_position_stabilized_string,
            surface_ventilation_status_string,
            ambient_temperature_string,
            ambient_humidity_string,
            heater_status_string
        )
    }
}

#[cfg(test)]
impl DataFrame {
    pub fn assert(
        &self,
        target_water_temperature: Option<f32>,
        target_water_temperature_filtered: Option<f32>,
        target_water_ph: Option<f32>,
        target_water_ph_filtered: Option<f32>,
        target_water_conductivity: Option<f32>,
        target_water_conductivity_filtered: Option<f32>,
        target_water_conductivity_compensated: Option<f32>,
        target_refill_in_progress: Option<bool>,
        target_tank_level_switch_position: Option<bool>,
        target_tank_level_switch_invalid: Option<bool>,
        target_tank_level_switch_position_stabilized: Option<bool>,
        target_surface_ventilation_status: Option<bool>,
        target_ambient_temperature: Option<f32>,
        target_ambient_humidity: Option<f32>,
        target_heater_status: Option<bool>,
    ) {
        assert_eq!(self.water_temperature, target_water_temperature);
        assert_eq!(
            self.water_temperature_filtered,
            target_water_temperature_filtered,
        );
        assert_eq!(self.ph, target_water_ph);
        assert_eq!(self.ph_filtered, target_water_ph_filtered);
        assert_eq!(self.conductivity, target_water_conductivity);
        assert_eq!(
            self.conductivity_filtered,
            target_water_conductivity_filtered
        );
        assert_eq!(
            self.conductivity_compensated,
            target_water_conductivity_compensated
        );
        assert_eq!(self.refill_in_progress, target_refill_in_progress);
        assert_eq!(
            self.tank_level_switch_position,
            target_tank_level_switch_position
        );
        assert_eq!(
            self.tank_level_switch_invalid,
            target_tank_level_switch_invalid
        );
        assert_eq!(
            self.tank_level_switch_position_stabilized,
            target_tank_level_switch_position_stabilized
        );
        assert_eq!(
            self.surface_ventilation_status,
            target_surface_ventilation_status
        );
        assert_eq!(self.ambient_temperature, target_ambient_temperature);
        assert_eq!(self.ambient_humidity, target_ambient_humidity);
        assert_eq!(self.heater_status, target_heater_status);
    }
}

/// Contains the configuration and the implementation of the SQL interface for time-based data.
pub struct SqlInterfaceData {
    /// Connection to database
    pub conn: PooledConn,
}

impl SqlInterfaceData {
    const LOG_TARGET_SQL_INTERFACE_DATA: &'static str = "sql_interface_data";

    /// Provides a struct of type SqlInterfaceData
    /// # Arguments
    /// * `conn` - connection to SQL database
    pub fn new(conn: PooledConn) -> SqlInterfaceData {
        SqlInterfaceData { conn }
    }

    /// Writes all data inside the data frame to the SQL data base.
    /// None values or float variables with NaN or infinite (both positive and negative) are inserted as NULL into the database.
    /// The function either returns an empty Ok value or a value of type SqlInterfaceError.
    /// # Arguments
    /// * `data` - data to be written to database as type DataFrame
    pub fn write_data_frame_to_database(
        &mut self,
        data: &DataFrame,
    ) -> Result<(), SqlInterfaceError> {
        let null_string = "NULL".to_string();

        let timestamp_string = data.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();

        // replace signal placeholders with values
        let water_temperature_string: String;
        let sql_query_water_temperature_replaced = str::replace(
            sql_query_strings::SQL_QUERY_WRITE_DATA,
            "#TemperatureUnfiltered",
            match data.water_temperature {
                Some(c) => {
                    water_temperature_string = match c.is_nan() | c.is_infinite() {
                        true => null_string.clone(),
                        false => c.to_string(),
                    };
                    &water_temperature_string
                }
                None => &null_string,
            },
        );

        let water_temperature_filtered_string: String;
        let sql_query_water_temperature_filtered_replaced = str::replace(
            &sql_query_water_temperature_replaced,
            "#TemperatureFiltered",
            match data.water_temperature_filtered {
                Some(c) => {
                    water_temperature_filtered_string = match c.is_nan() | c.is_infinite() {
                        true => null_string.clone(),
                        false => c.to_string(),
                    };
                    &water_temperature_filtered_string
                }
                None => &null_string,
            },
        );

        let ph_string: String;
        let sql_query_ph_replaced = str::replace(
            &sql_query_water_temperature_filtered_replaced,
            "#pHUnfiltered",
            match data.ph {
                Some(c) => {
                    ph_string = match c.is_nan() | c.is_infinite() {
                        true => null_string.clone(),
                        false => c.to_string(),
                    };
                    &ph_string
                }
                None => &null_string,
            },
        );

        let ph_filtered_string: String;
        let sql_query_ph_filtered_replaced = str::replace(
            &sql_query_ph_replaced,
            "#pHFiltered",
            match data.ph_filtered {
                Some(c) => {
                    ph_filtered_string = match c.is_nan() | c.is_infinite() {
                        true => null_string.clone(),
                        false => c.to_string(),
                    };
                    &ph_filtered_string
                }
                None => &null_string,
            },
        );

        let conductivity_string: String;
        let sql_query_conductivity_replaced = str::replace(
            &sql_query_ph_filtered_replaced,
            "#ConductivityUnfiltered",
            match data.conductivity {
                Some(c) => {
                    conductivity_string = match c.is_nan() | c.is_infinite() {
                        true => null_string.clone(),
                        false => c.to_string(),
                    };
                    &conductivity_string
                }
                None => &null_string,
            },
        );

        let conductivity_filtered_string: String;
        let sql_query_conductivity_filtered_replaced = str::replace(
            &sql_query_conductivity_replaced,
            "#ConductivityFiltered",
            match data.conductivity_filtered {
                Some(c) => {
                    conductivity_filtered_string = match c.is_nan() | c.is_infinite() {
                        true => null_string.clone(),
                        false => c.to_string(),
                    };
                    &conductivity_filtered_string
                }
                None => &null_string,
            },
        );

        let conductivity_compensated_string: String;
        let sql_query_string_conductivity_compensated_replaced = str::replace(
            &sql_query_conductivity_filtered_replaced,
            "#ConductivityCompensated",
            match data.conductivity_compensated {
                Some(c) => {
                    conductivity_compensated_string = match c.is_nan() | c.is_infinite() {
                        true => null_string.clone(),
                        false => c.to_string(),
                    };
                    &conductivity_compensated_string
                }
                None => &null_string,
            },
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

        let tank_level_switch_position_string: String;
        let sql_query_main_tank_level_switch_position_replaced = str::replace(
            &sql_query_refill_in_progress_replaced,
            "#TankLevelSwitchPosition",
            match data.tank_level_switch_position {
                Some(c) => {
                    tank_level_switch_position_string = c.to_string();
                    &tank_level_switch_position_string
                }
                None => &null_string,
            },
        );

        let tank_level_switch_invalid_string: String;
        let sql_query_main_tank_level_switch_invalid_replaced = str::replace(
            &sql_query_main_tank_level_switch_position_replaced,
            "#TankLevelSwitchInvalid",
            match data.tank_level_switch_invalid {
                Some(c) => {
                    tank_level_switch_invalid_string = c.to_string();
                    &tank_level_switch_invalid_string
                }
                None => &null_string,
            },
        );

        let surface_ventilation_string: String;
        let sql_query_surface_ventilation_status_replaced = str::replace(
            &sql_query_main_tank_level_switch_invalid_replaced,
            "#SurfaceVentilation",
            match data.surface_ventilation_status {
                Some(c) => {
                    surface_ventilation_string = c.to_string();
                    &surface_ventilation_string
                }
                None => &null_string,
            },
        );

        let ambient_temperature_string: String;
        let sql_query_ambient_temperature_replaced = str::replace(
            &sql_query_surface_ventilation_status_replaced,
            "#AmbTemp",
            match data.ambient_temperature {
                Some(c) => {
                    ambient_temperature_string = match c.is_nan() | c.is_infinite() {
                        true => null_string.clone(),
                        false => c.to_string(),
                    };
                    &ambient_temperature_string
                }
                None => &null_string,
            },
        );

        let ambient_humidity_string: String;
        let sql_query_ambient_humidity_replaced = str::replace(
            &sql_query_ambient_temperature_replaced,
            "#AmbHum",
            match data.ambient_humidity {
                Some(c) => {
                    ambient_humidity_string = match c.is_nan() | c.is_infinite() {
                        true => null_string.clone(),
                        false => c.to_string(),
                    };
                    &ambient_humidity_string
                }
                None => &null_string,
            },
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

        // execute query
        match self.conn.exec_drop::<_, _>(
            sql_query_string.clone(),
            params! {
            "timestamp" => timestamp_string.clone(),
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                error!(
                    target: Self::LOG_TARGET_SQL_INTERFACE_DATA,
                    "could not insert data frame into database ({e:?})"
                );
                return Err(SqlInterfaceError::InsertDataFrameFailure(
                    "Error when inserting data frame into database using query: ".to_string()
                        + sql_query_string.as_str(),
                ));
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::config::{read_config_file, ConfigData};
    use crate::sql_interface::{SqlInterface, SqlInterfaceError};
    use crate::sql_interface_data::{DataFrame, SqlInterfaceData};
    use chrono::NaiveDateTime;
    use mysql::{prelude::Queryable, PooledConn, Row, Value};

    /// This struct contains the first part of one row from table 'data'
    /// Due to limitations of the library, this struct can only contain up to 12 elements.
    /// The library has no means for handling NULL values.
    /// So the data types of this struct are no optionals.
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

    /// This struct contains the second part of one row from table 'data'
    /// Due to limitations of the library, this struct can only contain up to 12 elements.
    /// The library has no means for handling NULL values.
    /// So the data types of this struct are no optionals.
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

    pub const SQL_QUERY_READ_DATA_FRAME_1: &str = "SELECT Timestamp, \
        Temperature, TemperatureFiltered, \
        pH, pHFiltered, \
        Conductivity, ConductivityFiltered, ConductivityCompensated \
        FROM data WHERE Timestamp='#timestamp';";

    pub const SQL_QUERY_READ_DATA_FRAME_2: &str = "SELECT \
        RefillInProgress, TankLevelSwitchPosition, TankLevelSwitchInvalid, SurfaceVentilation, \
        AmbTemp, AmbHum, Heater FROM data \
        WHERE Timestamp = '#timestamp';";

    pub const SQL_QUERY_READ_NULL_DATA_FRAME: &str = "SELECT \
        Temperature, TemperatureFiltered, \
        pH, pHFiltered, \
        Conductivity, ConductivityFiltered, ConductivityCompensated \
        RefillInProgress, TankLevelSwitchPosition, TankLevelSwitchInvalid, SurfaceVentilation, \
        AmbTemp, AmbHum, Heater FROM data \
        WHERE Timestamp = '#timestamp';";

    /// The function executes two SQL statements to retrieve one row from table 'data'
    /// In Ok case, the function returns one struct of type DataFrame wrapped in Result.
    /// Note: This function cannot handle entries with NULL data due to limitations of the SQL library.
    pub fn get_single_dataframe_from_database(
        conn: &mut PooledConn,
        timestamp: NaiveDateTime,
    ) -> Result<DataFrame, SqlInterfaceError> {
        let timestamp_string = timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
        // Get data frame part 1 from database
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
                timestamp: timestamp,
                water_temperature: water_temp,
                water_temperature_filtered: water_temp_filt,
                ph: ph,
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

        // check error case if there is no response
        if (data_frame_1_array.len() == 0) || (data_frame_2_array.len() == 0) {
            return Err(SqlInterfaceError::SingleDataFrameRequestNoSingleResponse(
                "SQL database responded with empty response to query.".to_string(),
            ));
        }
        // check error case if there is more than one response
        if (data_frame_2_array.len() > 1) || (data_frame_2_array.len() > 1) {
            return Err(SqlInterfaceError::SingleDataFrameRequestNoSingleResponse(
                "SQL database responded with multiple responses to request to query: ".to_string(),
            ));
        }
        let data_frame_1 = &data_frame_1_array[0];
        let data_frame_2 = &data_frame_2_array[0];

        Ok(DataFrame {
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

    /// The function executes two SQL statements to retrieve one row from table 'data'
    /// In Ok case, the function returns one struct of type DataFrame wrapped in Result.
    /// Note: This function handles exclusively entries with NULL data.
    pub fn check_database_row_if_all_null(conn: &mut PooledConn, timestamp: NaiveDateTime) {
        let timestamp_string = timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
        // Get series of NULL values from database
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
    /// all test cases for this crate combined in one function to avoid problems with multithreading
    pub fn test_sql_interface_data() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_sql_interface.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_data = SqlInterfaceData::new(sql_interface.get_connection());

        // *** insert and load DataFrame - PASS ****************************************************
        match SqlInterface::truncate_table(&mut sql_interface, "data".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        let current_timestamp = match SqlInterface::get_current_timestamp(&mut sql_interface.conn) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get current timestamp from database: {e:?}");
            }
        };
        let dataframe_reference_1 = DataFrame {
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

        // *** insert and load the same DataFrame again - PASS (check ON DUPLICATE functionality) **
        let dataframe_reference_2 = DataFrame {
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

        // *** insert and load DataFrame with NULL data - PASS *************************************
        match SqlInterface::truncate_table(&mut sql_interface, "data".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        let current_timestamp = match SqlInterface::get_current_timestamp(&mut sql_interface.conn) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not get current timestamp from database: {e:?}");
            }
        };
        let dataframe_reference_3 = DataFrame {
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
        // *****************************************************************************************
    }
}
