use chrono::*;
use mysql::prelude::*;
use mysql::*;

use crate::sql_interface::sql_query_strings;
use crate::sql_interface::SqlInterfaceError;

/// This struct holds the statistical data of heating for one day.
/// It is filled with post-processed data from SQL query which is then updated by the control application.
pub struct HeatingStatsEntry {
    pub date: NaiveDate,
    pub energy: f64,
    pub ambient_temperature_average_value: f64,
    pub ambient_temperature_average_counter: i64,
    pub water_temperature_average_value: f64,
    pub water_temperature_average_counter: i64,
    pub heating_control_runtime: i64,
}

/// This struct holds the statistical data of heating for one day as retrieved from SQL database.
/// In next step, date is postprocessed to NaiveData type.
pub struct HeatingStatsStringEntry {
    pub date: String,
    pub energy: f64,
    pub ambient_temperature_average_value: f64,
    pub ambient_temperature_average_counter: i64,
    pub water_temperature_average_value: f64,
    pub water_temperature_average_counter: i64,
    pub heating_control_runtime: i64,
}

/// This struct contains the configuration and the implementation of the SQL interface for heating.
pub struct SqlInterfaceHeating {
    /// Connection to database
    pub conn: PooledConn,
}

impl SqlInterfaceHeating {
    /// The function creates an instance of SqlInterfaceBalling.
    pub fn new(conn: PooledConn) -> SqlInterfaceHeating {
        SqlInterfaceHeating { conn }
    }

    /// The function executes an SQL statement given as parameter and evaluates if the response is a single heating stats entry.
    pub fn get_single_heating_stats_from_database(
        &mut self,
    ) -> Result<HeatingStatsEntry, SqlInterfaceError> {
        // Get single heating stats entry from database
        let single_heating_stats_entry_array = match self.conn.query_map(
            sql_query_strings::SQL_QUERY_READ_HEATING_STATS,
            |(
                date,
                energy,
                ambient_temperature_average_value,
                ambient_temperature_average_counter,
                water_temperature_average_value,
                water_temperature_average_counter,
                heating_control_runtime,
            )| HeatingStatsStringEntry {
                date,
                energy,
                ambient_temperature_average_value,
                ambient_temperature_average_counter,
                water_temperature_average_value,
                water_temperature_average_counter,
                heating_control_runtime,
            },
        ) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::SingleHeatingStatsRequestFailure(
                    "Could not get single heating stats entry from SQL database.".to_string(),
                ));
            }
        };

        // check error case if there is no response
        if single_heating_stats_entry_array.len() == 0 {
            return Err(
                SqlInterfaceError::SingleHeatingStatsRequestNoSingleResponse(
                    "SQL database responded with empty response to query: ".to_string()
                        + sql_query_strings::SQL_QUERY_READ_HEATING_STATS,
                ),
            );
        }
        // check error case if there is more than one response
        if single_heating_stats_entry_array.len() > 1 {
            return Err(
                SqlInterfaceError::SingleHeatingStatsRequestNoSingleResponse(
                    "SQL database responded with multiple responses to request to query: "
                        .to_string()
                        + sql_query_strings::SQL_QUERY_READ_HEATING_STATS,
                ),
            );
        }
        let single_heating_stats_string_entry = &single_heating_stats_entry_array[0];

        // convert String to NaiveDate
        let date = match NaiveDate::parse_from_str(
            single_heating_stats_string_entry.date.as_str(),
            "%Y-%m-%d",
        ) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::TimestampConversionFailure(
                    "Error when converting timestamp: ".to_string()
                        + &single_heating_stats_string_entry.date,
                ));
            }
        };

        let single_heating_stats_entry = HeatingStatsEntry {
            date: date.into(),
            energy: single_heating_stats_string_entry.energy,
            ambient_temperature_average_value: single_heating_stats_string_entry
                .ambient_temperature_average_value,
            ambient_temperature_average_counter: single_heating_stats_string_entry
                .ambient_temperature_average_counter,
            water_temperature_average_value: single_heating_stats_string_entry
                .water_temperature_average_value,
            water_temperature_average_counter: single_heating_stats_string_entry
                .water_temperature_average_counter,
            heating_control_runtime: single_heating_stats_string_entry.heating_control_runtime,
        };
        Ok(single_heating_stats_entry)
    }

    /// The function inserts one dataset of heating statistics into the database
    pub fn insert_heating_stats_entry(
        &mut self,
        heating_stats_entry: HeatingStatsEntry,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_energy_replaced = str::replace(
            sql_query_strings::SQL_QUERY_WRITE_HEATING_STATS,
            "#Energy",
            &heating_stats_entry.energy.to_string(),
        );
        let sql_query_ambient_temperature_average_replaced = str::replace(
            &sql_query_energy_replaced,
            "#AmbientTempAverage",
            &heating_stats_entry
                .ambient_temperature_average_value
                .to_string(),
        );
        let sql_query_ambient_temperature_counter_replaced = str::replace(
            &sql_query_ambient_temperature_average_replaced,
            "#AmbientTempCounter",
            &heating_stats_entry
                .ambient_temperature_average_counter
                .to_string(),
        );
        let sql_query_water_temperature_average_replaced = str::replace(
            &sql_query_ambient_temperature_counter_replaced,
            "#WaterTempAverage",
            &heating_stats_entry
                .water_temperature_average_value
                .to_string(),
        );
        let sql_query_water_temperature_counter_replaced = str::replace(
            &sql_query_water_temperature_average_replaced,
            "#WaterTempCounter",
            &heating_stats_entry
                .water_temperature_average_counter
                .to_string(),
        );
        let sql_query_string = str::replace(
            &sql_query_water_temperature_counter_replaced,
            "#HeatingControlRuntime",
            &heating_stats_entry
                .water_temperature_average_counter
                .to_string(),
        );

        match self.conn.exec_drop::<_, _>(sql_query_string.clone(), params!{
                "date" => heating_stats_entry.date.format("%Y-%m-%d").to_string(),
                "energy" => heating_stats_entry.energy,
                "ambient_temperature_average" => heating_stats_entry.ambient_temperature_average_value,
                "ambient_temperature_counter" => heating_stats_entry.ambient_temperature_average_counter,
                "water_temperature_average" => heating_stats_entry.water_temperature_average_value,
                "water_temperature_counter" => heating_stats_entry.water_temperature_average_counter,
                "heating_control_runtime" => heating_stats_entry.heating_control_runtime,
                },
            ) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("{e:?}");
                Err(SqlInterfaceError::InsertHeatingStatsEntryFailure(
                    "Error when inserting heating stats into database using query: ".to_string() + sql_query_string.as_str(),
                ))
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::sql_interface::{SqlInterface, SqlInterfaceError};
    use crate::sql_interface_heating::SqlInterfaceHeating;

    #[test]
    /// all test cases for this crate combined in one function to avoid problems with multithreading
    pub fn test_sql_interface_heating() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_sql_interface.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_heating = SqlInterfaceHeating::new(sql_interface.get_connection());

        //*** retrieve heating stats - FAIL ****************************************************
        match SqlInterface::truncate_table(&mut sql_interface, "heatingstats".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        match sql_interface_heating.get_single_heating_stats_from_database() {
            Ok(_) => {
                panic!("Call to get_single_heating_stats_from_database should have failed.");
            }
            Err(e) => {
                assert_eq!(
                    e,
                    SqlInterfaceError::SingleHeatingStatsRequestNoSingleResponse(
                        "SQL database responded with empty response to query: \
                    SELECT * FROM heatingstats WHERE Date=CURRENT_DATE();"
                            .to_string()
                    )
                );
            }
        }
        println!("* checking retrieving heating stats (FAIL) succeeded.");
        //**************************************************************************************
    }
}
