/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::*;
use mysql::prelude::*;
use mysql::*;
use std::fmt;

use crate::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
};

/// Holds the statistical data of heating for one day.
/// It is filled with post-processed data from SQL query which is then updated by the control application.
#[derive(Clone)]
pub struct HeatingStatsEntry {
    /// calendar date of the entry after having postprocessed the SQL data read as String
    pub date: NaiveDate,

    /// energy used for heating on a specific date in kWh
    pub energy: f64,

    /// average ambient temperature value on a specific date in °C
    pub ambient_temperature_average_value: f64,

    /// counter how many samples were used to calculate the average value
    pub ambient_temperature_average_counter: i64,

    /// average water temperature value on a specific date in °C
    pub water_temperature_average_value: f64,

    /// counter how many samples were used to calculate the average value    
    pub water_temperature_average_counter: i64,

    /// counter how many seconds the heating control was active on a specific dates
    pub heating_control_runtime: i64,
}

impl HeatingStatsEntry {
    /// provides struct of type HeatingStatsEntry - used in case database has not entry for current date
    pub fn new() -> HeatingStatsEntry {
        let now = Local::now();
        let year = now.year();
        let month = now.month();
        let day = now.day();
        // Create NaiveDate from components - unwrap is safe in this context
        let today: NaiveDate = NaiveDate::from_ymd_opt(year, month, day).unwrap();

        Self {
            date: today,
            energy: 0.0,
            ambient_temperature_average_value: 0.0,
            ambient_temperature_average_counter: 0,
            water_temperature_average_value: 0.0,
            water_temperature_average_counter: 0,
            heating_control_runtime: 0,
        }
    }

    /// add another sample for calculating the average values
    pub fn update(
        &mut self,
        water_temperature_opt: Option<f64>,
        ambient_temperature_opt: Option<f64>,
        energy_increment: f64,
    ) {
        // add energy to sum of heating energy
        self.energy += energy_increment;

        // calculate new average water temperature value
        self.water_temperature_average_value = match water_temperature_opt {
            Some(water_temperature) => {
                self.water_temperature_average_counter += 1;
                if self.water_temperature_average_counter > 1 {
                    // multiple samples existing, so mix value proportionally
                    water_temperature / self.water_temperature_average_counter as f64
                        + self.water_temperature_average_value
                            * (1.0 - 1.0 / self.water_temperature_average_counter as f64)
                } else {
                    water_temperature
                }
            }
            None => {
                // do nothing - wait for next valid sample
                self.water_temperature_average_value
            }
        };

        // calculate new average ambient air temperature value
        self.ambient_temperature_average_value = match ambient_temperature_opt {
            Some(ambient_temperature) => {
                self.ambient_temperature_average_counter += 1;
                if self.ambient_temperature_average_counter > 1 {
                    // multiple samples existing, so mix value proportionally
                    ambient_temperature / self.ambient_temperature_average_counter as f64
                        + self.ambient_temperature_average_value
                            * (1.0 - 1.0 / self.ambient_temperature_average_counter as f64)
                } else {
                    ambient_temperature
                }
            }
            None => {
                // do nothing - wait for next valid sample
                self.ambient_temperature_average_value
            }
        };

        self.heating_control_runtime += 1;
    }
}

impl fmt::Display for HeatingStatsEntry {
    /// Provides output of the struct values for debugging purposes.
    /// # Arguments
    /// * `f` - configuration for formatting
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Heating Stats Entry for {}: \n\
                Water temperature: average={}°C, counter={} \n\
                Ambient temperature: average={}°C, counter={} \n\
                Energy: {}kWh\n\
                Control runtime: {}s",
            self.date,
            self.water_temperature_average_value,
            self.water_temperature_average_counter,
            self.ambient_temperature_average_value,
            self.ambient_temperature_average_counter,
            self.energy,
            self.heating_control_runtime
        )
    }
}

/// Holds the statistical data of heating for one day as retrieved from SQL database.
/// In next step, date is postprocessed to NaiveData type.
pub struct HeatingStatsStringEntry {
    /// calendar date of the entry before postprocessing to NaiveDate
    pub date: String,

    /// energy used for heating on a specific date in kWh    
    pub energy: f64,

    /// average ambient temperature value on a specific date in °C
    pub ambient_temperature_average_value: f64,

    /// counter how many samples were used to calculate the average value
    pub ambient_temperature_average_counter: i64,

    /// average water temperature value on a specific date in °C
    pub water_temperature_average_value: f64,

    /// counter how many samples were used to calculate the average value    
    pub water_temperature_average_counter: i64,

    /// counter how many seconds the heating control was active on a specific dates
    pub heating_control_runtime: i64,
}

/// Trait for calculation the time interval until midnight in seconds.
/// This trait allows running the main control with a mock implementation for testing.
pub trait SqlInterfaceHeatingMidnightCalculatorTrait {
    fn get_duration_until_midnight(&mut self) -> Result<i64, SqlInterfaceError>;
}

pub struct SqlInterfaceHeatingMidnightCalculator {
    /// Connection to database
    pub conn: PooledConn,
}

impl SqlInterfaceHeatingMidnightCalculatorTrait for SqlInterfaceHeatingMidnightCalculator {
    /// Requests the duration until midnight in seconds.
    /// It provides wrapped into Result type either
    /// - the number of seconds until midnight
    /// - or a value from enum SqlInterfaceError
    fn get_duration_until_midnight(&mut self) -> Result<i64, SqlInterfaceError> {
        match SqlInterface::get_single_integer_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_DURATION_UNTIL_MIDNIGHT,
        ) {
            Ok(c) => Ok(c),
            Err(e) => Err(e),
        }
    }
}

impl SqlInterfaceHeatingMidnightCalculator {
    /// Provides a struct of type SqlInterfaceHeatingMidnigtCalculator
    /// # Arguments
    /// * `conn` - connection to SQL database
    pub fn new(conn: PooledConn) -> SqlInterfaceHeatingMidnightCalculator {
        SqlInterfaceHeatingMidnightCalculator { conn }
    }
}

/// Contains the configuration and the implementation of the SQL interface for heating.
pub struct SqlInterfaceHeating {
    /// Connection to database
    pub conn: PooledConn,

    /// Struct with implementation of trait for calculation of time interval until midnight in seconds
    pub sql_interface_heating_midnight_calculator:
        Box<dyn SqlInterfaceHeatingMidnightCalculatorTrait + Sync + Send>,
}

impl SqlInterfaceHeating {
    /// Provides a struct of type SqlInterfaceHeating.
    /// # Arguments
    /// * `conn` - connection to SQL database
    pub fn new(
        conn: PooledConn,
        sql_interface_heating_midnight_calculator: Box<
            dyn SqlInterfaceHeatingMidnightCalculatorTrait + Sync + Send,
        >,
    ) -> SqlInterfaceHeating {
        SqlInterfaceHeating {
            conn,
            sql_interface_heating_midnight_calculator,
        }
    }

    /// Retrieves a single heating stats entry from SQL database.
    /// It provides wrapped into Result type either
    /// - struct of type HeatingStatsEntry in case of success
    /// - or a value from enum SqlInterfaceError
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

    /// Inserts one dataset of heating statistics into the SQL database.
    /// It provides wrapped into Result type either
    /// - an empty Ok value
    /// - or a value from enum SqlInterfaceError
    /// # Arguments
    /// * `heating_stats_entry` - reference to struct HeatingStatsEntry providing the data which shall be inserted into the database.
    pub fn insert_heating_stats_entry(
        &mut self,
        heating_stats_entry: &HeatingStatsEntry,
    ) -> Result<(), SqlInterfaceError> {
        // replace signals inside the ON DUPLICATE clause
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
            &heating_stats_entry.heating_control_runtime.to_string(),
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

    /// Requests the duration until midnight in seconds from implementation of trait
    pub fn get_duration_until_midnight(&mut self) -> Result<i64, SqlInterfaceError> {
        self.sql_interface_heating_midnight_calculator
            .get_duration_until_midnight()
    }
}

#[cfg(test)]
pub mod tests {
    use chrono::{Datelike, Local, NaiveDate, NaiveDateTime};

    use crate::database::sql_interface_heating::{
        HeatingStatsEntry, SqlInterfaceHeating, SqlInterfaceHeatingMidnightCalculator,
    };
    use crate::database::{sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError};
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};

    #[test]
    // Test case includes all checks for this table combined in one function to limit amount of additional databases.
    // Test case uses test database #36.
    pub fn test_sql_interface_heating() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            36,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let sql_interface_heating_midnight_calculator = Box::new(
            SqlInterfaceHeatingMidnightCalculator::new(sql_interface.get_connection()),
        );
        let mut sql_interface_heating = SqlInterfaceHeating::new(
            sql_interface.get_connection(),
            sql_interface_heating_midnight_calculator,
        );

        // *** retrieve heating stats - FAIL ****************************************************
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
        // **************************************************************************************

        // *** insert heating stats - PASS ******************************************************
        // prepare data for inserting into database
        let today = match SqlInterface::get_current_date(&mut sql_interface.get_connection()) {
            Ok(c) => c,
            Err(_) => panic!("Could not get current date."),
        };
        let mut heating_stats_entry = HeatingStatsEntry {
            date: today.into(),
            energy: 0.9,
            ambient_temperature_average_value: 22.0,
            ambient_temperature_average_counter: 72000,
            water_temperature_average_value: 25.0,
            water_temperature_average_counter: 78000,
            heating_control_runtime: 64000,
        };
        assert_eq!(
            sql_interface_heating.insert_heating_stats_entry(&heating_stats_entry),
            Ok(())
        );
        println!("* checking inserting heating stats (PASS) succeeded.");
        // **************************************************************************************

        // *** retrieve stats - PASS ************************************************************
        match sql_interface_heating.get_single_heating_stats_from_database() {
            Ok(c) => {
                assert_eq!(c.date, today.into());
                assert_eq!(c.energy, 0.9);
                assert_eq!(c.ambient_temperature_average_value, 22.0);
                assert_eq!(c.ambient_temperature_average_counter, 72000);
                assert_eq!(c.water_temperature_average_value, 25.0);
                assert_eq!(c.water_temperature_average_counter, 78000);
                assert_eq!(c.heating_control_runtime, 64000);
            }
            Err(_) => {
                panic!("Call to get_single_heating_stats_from_database has failed.");
            }
        }
        println!("* checking retrieving heating stats (PASS) succeeded.");

        // *** retrieve stats - PASS ************************************************************
        // update data and trigger SQL statement (ON DUPLICATE UPDATE)
        heating_stats_entry.energy += 0.1;
        heating_stats_entry.ambient_temperature_average_value += 0.1;
        heating_stats_entry.ambient_temperature_average_counter += 1;
        heating_stats_entry.water_temperature_average_value += 0.1;
        heating_stats_entry.water_temperature_average_counter += 1;
        heating_stats_entry.heating_control_runtime += 1;
        assert_eq!(
            sql_interface_heating.insert_heating_stats_entry(&heating_stats_entry),
            Ok(())
        );
        match sql_interface_heating.get_single_heating_stats_from_database() {
            Ok(c) => {
                assert_eq!(c.date, today.into());
                assert_eq!(c.energy, 1.0);
                assert_eq!(c.ambient_temperature_average_value, 22.1);
                assert_eq!(c.ambient_temperature_average_counter, 72001);
                assert_eq!(c.water_temperature_average_value, 25.1);
                assert_eq!(c.water_temperature_average_counter, 78001);
                assert_eq!(c.heating_control_runtime, 64001);
            }
            Err(_) => {
                panic!("Call to get_single_heating_stats_from_database has failed.");
            }
        }
        println!("* checking retrieving updated heating stats (PASS) succeeded.");
        // **************************************************************************************

        // *** calculate duration until midnight ************************************************
        // reference calculation in Rust
        // Get current date components
        let now = Local::now();
        let year = now.year();
        let month = now.month();
        let day = now.day();

        // Create NaiveDate from components
        let today: NaiveDate = NaiveDate::from_ymd_opt(year, month, day).unwrap();

        // Create a NaiveDateTime for midnight
        let midnight = NaiveDateTime::new(today, chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        // Calculate the difference
        let duration_until_midnight = 24 * 3600
            - SqlInterface::get_duration_from_naive_timestamp_to_local_now(midnight).unwrap();
        match sql_interface_heating.get_duration_until_midnight() {
            Ok(c) => {
                assert_eq!(c, duration_until_midnight);
            }
            Err(_) => {
                panic!("Call to get_duration_until_midnight failed.");
            }
        }
        println!("* checking duration until midnight in seconds (PASS) succeeded.");
        // **************************************************************************************
    }
}
