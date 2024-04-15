use chrono::*;
use mysql::prelude::*;
use mysql::*;

use crate::sql_interface::sql_query_strings;
use crate::sql_interface::SqlInterface;
use crate::sql_interface::SqlInterfaceError;

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
    pub main_tank_level_switch_status: bool,
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
        data: DataFrame,
    ) -> Result<(), SqlInterfaceError> {
        // replace signals inside the ON DUPLICATE clause - query #1
        let sql_query_water_temperature_replaced = str::replace(
            sql_query_strings::SQL_QUERY_WRITE_DATA_1,
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
        let sql_query_string_1 = str::replace(
            &sql_query_conductivity_filtered_replaced,
            "#ConductivityCompensated",
            &data.conductivity_compensated.to_string(),
        );

        // replace signals inside the ON DUPLICATE clause - query #2
        let sql_query_refill_in_progress_replaced = str::replace(
            sql_query_strings::SQL_QUERY_WRITE_DATA_2,
            "#RefillInProgress",
            &data.refill_in_progress.to_string(),
        );
        let sql_query_main_tank_level_switch_status_replaced = str::replace(
            &sql_query_refill_in_progress_replaced,
            "#MainTankLevelSwitchStatus",
            &data.main_tank_level_switch_status.to_string(),
        );
        let sql_query_surface_ventilation_status_replaced = str::replace(
            &sql_query_main_tank_level_switch_status_replaced,
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
        let sql_query_string_2 = str::replace(
            &sql_query_ambient_humidity_replaced,
            "#Heater",
            &data.heater_status.to_string(),
        );

        let timestamp_string = data.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
        // execute query 1
        let result_part_1 = match self.conn.exec_drop::<_, _>(
            sql_query_string_1.clone(),
            params! {
            "timestamp" => timestamp_string.clone(),
            "water_temperature" => data.water_temperature,
            "water_temperature_filtered" => data.water_temperature_filtered,
            "ph" => data.ph,
            "ph_filtered" => data.ph_filtered,
            "conductivity" => data.conductivity,
            "conductivity_filtered" => data.conductivity_filtered,
            "conductivity_compensated" => data.conductivity_compensated,
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("{e:?}");
                return Err(SqlInterfaceError::InsertDataFrameFailure(
                    "Error when inserting data frame part 1 into database using query: "
                        .to_string()
                        + sql_query_string_1.as_str(),
                ));
            }
        };
        if !result_part_1.is_ok() {
            return result_part_1;
        }

        // execute query 2
        match self.conn.exec_drop::<_, _>(
            sql_query_string_2.clone(),
            params! {
            "timestamp" => timestamp_string,
            "refill_in_progress" => data.refill_in_progress,
            "main_tank_level_switch_status" => data.main_tank_level_switch_status,
            "surface_ventilation_status" => data.surface_ventilation_status,
            "ambient_temperature" => data.ambient_temperature,
            "ambient_humidity" => data.ambient_humidity,
            "heater_status" => data.heater_status,
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("{e:?}");
                Err(SqlInterfaceError::InsertDataFrameFailure(
                    "Error when inserting data frame part 2 into database using query: "
                        .to_string()
                        + sql_query_string_2.as_str(),
                ))
            }
        }
    }
}
