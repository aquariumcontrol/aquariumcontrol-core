/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError, sql_query_strings,
    sql_interface_midnight::SqlInterfaceMidnightCalculatorTrait
};

use chrono::*;
use mysql::prelude::*;
use mysql::*;
use std::fmt;
use log::warn;

const LOG_TARGET_SQL_INTERFACE_HEATING: &str = "sql_interface_heating";

/// Holds the statistical data of heating for one day.
/// It is filled with post-processed data from a database query which is then updated by the control application.
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

    /// counter how many seconds the heating control was active on a specific date
    pub heating_control_runtime: i64,
}

impl HeatingStatsEntry {
    /// Creates a new `HeatingStatsEntry` initialized for the current local date.
    ///
    /// This constructor is used when there's no existing heating statistics
    /// record for the current day in the database. It sets the `date` field to today's
    /// date and initializes all statistical counters and values to zero.
    ///
    /// # Returns
    /// A new `HeatingStatsEntry` struct with default values for the current date.
    pub fn new(conn: &mut PooledConn) -> HeatingStatsEntry {
        // Get the current date from the database
        let today: NaiveDate = SqlInterface::get_current_date(conn).unwrap_or_else(|_| {
            warn!(target: LOG_TARGET_SQL_INTERFACE_HEATING, "Could not get current date from database.");
            // fall back: calculation using machine-local time
            let now = Local::now();
            let year = now.year();
            let month = now.month();
            let day = now.day();
            // Create NaiveDate from components - unwrap is safe in this context
            NaiveDate::from_ymd_opt(year, month, day).unwrap()
        });

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

    /// Updates the heating statistics entry with new sensor readings and energy consumption data.
    ///
    /// This method incorporates new samples to incrementally calculate average water and ambient
    /// temperatures, sum up energy consumption, and track the heating control's active runtime.
    ///
    /// # Arguments
    /// * `water_temperature_opt` - An `Option<f64>` representing the latest water temperature reading.
    ///   If `Some`, it contributes to the average; if `None`, the water temperature average remains unchanged.
    /// * `ambient_temperature_opt` - An `Option<f64>` representing the latest ambient air temperature reading.
    ///   If `Some`, it contributes to the average; if `None`, the ambient temperature average remains unchanged.
    /// * `energy_increment` - The amount of energy (in kWh) consumed by the heater since the last update.
    pub fn update(
        &mut self,
        water_temperature_opt: Option<f64>,
        ambient_temperature_opt: Option<f64>,
        energy_increment: f64,
    ) {
        // add energy to the sum of heating energy
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
                // do nothing - wait for the next valid sample
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
                // do nothing - wait for the next valid sample
                self.ambient_temperature_average_value
            }
        };

        self.heating_control_runtime += 1;
    }
}

impl fmt::Display for HeatingStatsEntry {
    /// Formats the `HeatingStatsEntry` struct into a multi-line, human-readable string.
    ///
    /// This implementation is primarily intended for debugging and logging, providing
    /// a clear overview of the heating statistics for a given date, including
    /// average temperatures, energy consumption, and control runtime.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to the formatter, as required by the `fmt::Display` trait.
    ///
    /// # Returns
    /// A `fmt::Result` indicating whether the formatting operation was successful.
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

/// Holds the statistical data of heating for one day as retrieved from the database.
/// In the next step, date is postprocessed to NaiveData type.
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

    /// counter how many seconds the heating control was active on a specific date
    pub heating_control_runtime: i64,
}

/// Contains the configuration and the implementation of the SQL interface for heating.
pub struct SqlInterfaceHeating {
    /// Connection to the database
    pub conn: PooledConn,

    /// Struct with implementation of trait for calculation of the time interval until midnight in seconds
    pub sql_interface_heating_midnight_calculator:
        Box<dyn SqlInterfaceMidnightCalculatorTrait + Sync + Send>,
}

impl SqlInterfaceHeating {
    /// Creates a new `SqlInterfaceHeating` instance.
    ///
    /// This constructor initializes the heating SQL interface with a primary
    /// database connection and a dedicated calculator for time-until-midnight
    /// functionality, which allows for dependency injection for testing.
    ///
    /// # Arguments
    /// * `conn` - An active database connection pool object for general heating-related operations.
    /// * `sql_interface_heating_midnight_calculator` - A boxed trait object that provides
    ///   the implementation for calculating the duration until midnight. This allows
    ///   for dynamic dispatch and mock implementations in tests.
    ///
    /// # Returns
    /// A new `SqlInterfaceHeating` struct.
    pub fn new(
        mut conn: PooledConn,
        sql_interface_heating_midnight_calculator: Box<
            dyn SqlInterfaceMidnightCalculatorTrait + Sync + Send,
        >,
    ) -> SqlInterfaceHeating {
        if SqlInterface::get_single_integer_from_database(
            &mut conn, sql_query_strings::SQL_QUERY_CHECK_HEATINGSETVALS_NULL)
            .expect("Could not check heating set value table for NULL values.") > 0 {
            panic!("{}: Heating set value table may be inconsistent - it contains NULL values.",
                   LOG_TARGET_SQL_INTERFACE_HEATING);
        }
        if SqlInterface::get_single_integer_from_database(
            &mut conn, sql_query_strings::SQL_QUERY_CHECK_HEATINGSTATS_NULL)
            .expect("Could not check heating statistics table for NULL values.") > 0 {
            panic!("{}: Heating statistics table may be inconsistent - it contains NULL values.",
                   LOG_TARGET_SQL_INTERFACE_HEATING);
        }
        SqlInterfaceHeating {
            conn,
            sql_interface_heating_midnight_calculator,
        }
    }

    /// Retrieves a single heating stats entry from the database.
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

        // check the error case if there is no response
        if single_heating_stats_entry_array.is_empty() {
            return Err(
                SqlInterfaceError::SingleHeatingStatsRequestNoSingleResponse(
                    "SQL database responded with empty response to query: ".to_string()
                        + sql_query_strings::SQL_QUERY_READ_HEATING_STATS,
                ),
            );
        }
        // check the error case if there is more than one response
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
            date,
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

    /// Retrieves a single heating statistics entry from the database for the day that just passed
    /// or for the current day in case the control application is terminated.
    ///
    /// This function queries the database for heating statistics data, expecting to find
    /// exactly one record. It then post-processes the raw string-based date into a `NaiveDate`
    /// and constructs a `HeatingStatsEntry` struct.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(HeatingStatsEntry)`: The successfully retrieved and parsed heating statistics entry.
    /// - `Err(SqlInterfaceError::SingleHeatingStatsRequestFailure)`: If the database query fails.
    /// - `Err(SqlInterfaceError::SingleHeatingStatsRequestNoSingleResponse)`: If the query
    ///   returns zero records or more than one record (indicating a data inconsistency).
    /// - `Err(SqlInterfaceError::TimestampConversionFailure)`: If the date string
    ///   retrieved from the database cannot be parsed into a valid `NaiveDate`.
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

    /// Delegates the request for the duration until midnight to the configured calculator.
    ///
    /// This function calls the `get_duration_until_midnight` method on the
    /// `sql_interface_heating_midnight_calculator` trait object, providing an
    /// abstraction layer for retrieving this specific time calculation.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(i64)`: The number of seconds remaining until the next midnight,
    ///   as determined by the injected calculator implementation.
    /// - `Err(SqlInterfaceError)`: If the underlying calculator encounters an error
    ///   during its operation.
    pub fn get_duration_until_midnight(&mut self) -> Result<i64, SqlInterfaceError> {
        self.sql_interface_heating_midnight_calculator
            .get_duration_until_midnight()
    }

    /// Execute a ping to the database to avoid timeout.
    pub fn ping(&mut self) {
        SqlInterface::ping_database(&mut self.conn, LOG_TARGET_SQL_INTERFACE_HEATING);
    }}

#[cfg(test)]
pub mod tests {
    use chrono::{Datelike, Local, NaiveDate, NaiveDateTime};

    use crate::database::sql_interface_heating_stats::{HeatingStatsEntry, SqlInterfaceHeating};
    use crate::database::sql_interface_midnight::SqlInterfaceMidnightCalculator;
    use crate::database::{sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError};
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};

    #[test]
    // Test case includes all checks for this table combined in one function to limit the number of additional databases.
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
            SqlInterfaceMidnightCalculator::new(sql_interface.get_connection()),
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
        // prepare data for inserting into the database
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
