use chrono::*;
use mysql::prelude::*;
use mysql::*;

use crate::sql_interface::sql_query_strings;
use crate::sql_interface::SqlInterfaceError;

#[derive(Debug, PartialEq)]
pub struct DataFrame {
    pub timestamp: NaiveDateTime,
    pub water_temperature: f32,
    pub water_temperature_filtered: f32,
    pub ph: f32,
    pub ph_filtered: f32,
    pub conductivity: f32,
    pub conductivity_filtered: f32,
    pub conductivity_compensated: f32,
    pub refill_in_progress: bool,
    pub tank_level_switch_position: bool,
    pub tank_level_switch_invalid: bool,
    pub surface_ventilation_status: bool,
    pub ambient_temperature: f32,
    pub ambient_humidity: f32,
    pub heater_status: bool,
}

/// This struct contains the configuration and the implementation of the SQL interface for time-based data.
pub struct SqlInterfaceData {
    /// Connection to database
    pub conn: PooledConn,
}

impl SqlInterfaceData {
    /// The function creates an instance of SqlInterfaceData.
    pub fn new(conn: PooledConn) -> SqlInterfaceData {
        SqlInterfaceData { conn }
    }

    /// The function writes all data inside the data frame to the SQL data base.
    pub fn write_data_frame_to_database(
        &mut self,
        data: &DataFrame,
    ) -> Result<(), SqlInterfaceError> {
        let timestamp_string = data.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
        // replace signal placeholders with values
        let sql_query_water_temperature_replaced = str::replace(
            sql_query_strings::SQL_QUERY_WRITE_DATA,
            "#TemperatureUnfiltered",
            &data.water_temperature.to_string(),
        );
        let sql_query_water_temperature_filtered_replaced = str::replace(
            &sql_query_water_temperature_replaced,
            "#TemperatureFiltered",
            &data.water_temperature_filtered.to_string(),
        );
        let sql_query_ph_replaced = str::replace(
            &sql_query_water_temperature_filtered_replaced,
            "#pHUnfiltered",
            &data.ph.to_string(),
        );
        let sql_query_ph_filtered_replaced = str::replace(
            &sql_query_ph_replaced,
            "#pHFiltered",
            &data.ph_filtered.to_string(),
        );
        let sql_query_conductivity_replaced = str::replace(
            &sql_query_ph_filtered_replaced,
            "#ConductivityUnfiltered",
            &data.conductivity.to_string(),
        );
        let sql_query_conductivity_filtered_replaced = str::replace(
            &sql_query_conductivity_replaced,
            "#ConductivityFiltered",
            &data.conductivity_filtered.to_string(),
        );
        let sql_query_string_conductivity_compensated_replaced = str::replace(
            &sql_query_conductivity_filtered_replaced,
            "#ConductivityCompensated",
            &data.conductivity_compensated.to_string(),
        );
        let sql_query_refill_in_progress_replaced = str::replace(
            &sql_query_string_conductivity_compensated_replaced,
            "#RefillInProgress",
            &data.refill_in_progress.to_string(),
        );
        let sql_query_main_tank_level_switch_position_replaced = str::replace(
            &sql_query_refill_in_progress_replaced,
            "#TankLevelSwitchPosition",
            &data.tank_level_switch_position.to_string(),
        );
        let sql_query_main_tank_level_switch_invalid_replaced = str::replace(
            &sql_query_main_tank_level_switch_position_replaced,
            "#TankLevelSwitchInvalid",
            &data.tank_level_switch_invalid.to_string(),
        );
        let sql_query_surface_ventilation_status_replaced = str::replace(
            &sql_query_main_tank_level_switch_invalid_replaced,
            "#SurfaceVentilation",
            &data.surface_ventilation_status.to_string(),
        );
        let sql_query_ambient_temperature_replaced = str::replace(
            &sql_query_surface_ventilation_status_replaced,
            "#AmbTemp",
            &data.ambient_temperature.to_string(),
        );
        let sql_query_ambient_humidity_replaced = str::replace(
            &sql_query_ambient_temperature_replaced,
            "#AmbHum",
            &data.ambient_humidity.to_string(),
        );
        let sql_query_string = str::replace(
            &sql_query_ambient_humidity_replaced,
            "#Heater",
            &data.heater_status.to_string(),
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
                println!("{e:?}");
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
    use mysql::{prelude::Queryable, PooledConn};

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
    pub struct SqlDataFrame2 {
        pub refill_in_progress: bool,
        pub tank_level_switch_position: bool,
        pub tank_level_switch_invalid: bool,
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

    /// The function executes an SQL statement given as parameter and evaluates if the response is a single heating stats entry.
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
                surface_ventilation_status,
                ambient_temperature,
                ambient_humidity,
                heater_status,
            )| SqlDataFrame2 {
                refill_in_progress,
                tank_level_switch_position,
                tank_level_switch_invalid,
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
            water_temperature: data_frame_1.water_temperature,
            water_temperature_filtered: data_frame_1.water_temperature_filtered,
            ph: data_frame_1.ph,
            ph_filtered: data_frame_1.ph_filtered,
            conductivity: data_frame_1.conductivity,
            conductivity_filtered: data_frame_1.conductivity_filtered,
            conductivity_compensated: data_frame_1.conductivity_compensated,
            refill_in_progress: data_frame_2.refill_in_progress,
            tank_level_switch_position: data_frame_2.tank_level_switch_position,
            tank_level_switch_invalid: data_frame_2.tank_level_switch_invalid,
            surface_ventilation_status: data_frame_2.surface_ventilation_status,
            ambient_temperature: data_frame_2.ambient_temperature,
            ambient_humidity: data_frame_2.ambient_humidity,
            heater_status: data_frame_2.heater_status,
        })
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
            water_temperature: 25.5,
            water_temperature_filtered: 25.0,
            ph: 7.0,
            ph_filtered: 7.5,
            conductivity: 50000.0,
            conductivity_filtered: 51000.0,
            conductivity_compensated: 52000.0,
            refill_in_progress: false,
            tank_level_switch_position: false,
            tank_level_switch_invalid: false,
            surface_ventilation_status: true,
            ambient_temperature: 22.0,
            ambient_humidity: 60.0,
            heater_status: true,
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

        // *** insert and load the same DataFrame again - PASS (check ON DUPLICATE funcationality **
        let dataframe_reference_2 = DataFrame {
            timestamp: dataframe_reference_1.timestamp,
            water_temperature: 26.5,
            water_temperature_filtered: 26.0,
            ph: 8.0,
            ph_filtered: 8.5,
            conductivity: 50001.0,
            conductivity_filtered: 51001.0,
            conductivity_compensated: 52001.0,
            refill_in_progress: true,
            tank_level_switch_position: true,
            tank_level_switch_invalid: true,
            surface_ventilation_status: false,
            ambient_temperature: 23.0,
            ambient_humidity: 61.0,
            heater_status: false,
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
    }
}
