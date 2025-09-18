/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages the aggregation and persistence of daily heating statistics.
//!
//! This module provides a dedicated interface, `SqlInterfaceHeatingStats`, for handling
//! all SQL operations related to the `heatingstats` table. It is responsible for
//! tracking daily energy consumption, average temperatures, and heater runtime.
//!
//! ## Key Components
//!
//! - **`SqlInterfaceHeatingStats` Struct**: The primary interface that holds a database
//!   connection and provides methods for all heating statistics operations.
//!
//! - **`HeatingStatsEntry` Struct**: A data structure representing the aggregated
//!   statistics for a single day. It includes an `update` method for incrementally
//!   calculating running averages.
//!
//! - **`new()` Constructor**: A critical entry point that performs "fail-fast"
//!   validation at initialization. It checks the database to ensure:
//!   - The `heatingstats` table contains no `NULL` values.
//!   - The table's row count does not exceed the configured maximum.
//!
//! - **`get_single_heating_stats_from_database()`**: Retrieves the statistics record
//!   for the current day.
//!
//! - **`insert_heating_stats_entry()`**: Writes a `HeatingStatsEntry` to the database
//!   using an `INSERT ... ON DUPLICATE KEY UPDATE` query to either create a new
//!   daily record or update an existing one.
//!
//! ## Design and Purpose
//!
//! The main goal of this module is to provide a robust and encapsulated interface
//! for managing the application's historical heating data.
//!
//! - **Encapsulation**: All SQL logic specific to the `heatingstats` table is
//!   contained within this module, separating concerns.
//!
//! - **Data Integrity**: The constructor's validation logic enforces data integrity
//!   rules at startup, preventing runtime errors caused by an invalid database state.
//!
//! - **Time-Based Logic**: It relies on a `SqlInterfaceMidnightCalculatorTrait` to
//!   determine when a day has ended, which is crucial for finalizing one day's
//!   statistics and starting a new record.

use crate::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError,
    sql_interface_midnight::SqlInterfaceMidnightCalculatorTrait, sql_query_strings,
};

use crate::database::sql_query_strings::{
    SQL_QUERY_CHECK_HEATING_STATS_COUNT, SQL_QUERY_CHECK_HEATING_STATS_NULL,
};
use chrono::*;
use log::warn;
use mysql::prelude::*;
use mysql::*;
use std::fmt;

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
            warn!(target: module_path!(), "Could not get current date from database.");
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

    #[cfg(test)]
    pub fn default() -> Self {
        Self {
            date: Local::now().date_naive(),
            energy: 0.0,
            ambient_temperature_average_value: 0.0,
            ambient_temperature_average_counter: 0,
            water_temperature_average_value: 0.0,
            water_temperature_average_counter: 0,
            heating_control_runtime: 0,
        }
    }

    #[cfg(test)]
    pub fn increment_date(&mut self) {
        let tomorrow = self.date.checked_add_days(Days::new(1));
        self.date = tomorrow.unwrap();
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

/// Contains the configuration and the implementation of the SQL interface for heating.
pub struct SqlInterfaceHeatingStats {
    /// Connection to the database
    pub conn: PooledConn,

    /// Struct with implementation of trait for calculation of the time interval until midnight in seconds
    pub sql_interface_heating_midnight_calculator:
        Box<dyn SqlInterfaceMidnightCalculatorTrait + Sync + Send>,
}

impl SqlInterfaceHeatingStats {
    /// Creates a new `SqlInterfaceHeatingStats` instance.
    ///
    /// This constructor initializes the SQL interface for heating statistics. It performs
    /// several pre-flight checks to ensure data integrity, such as verifying that the
    /// `heatingstats` table contains no NULL values and that its row count is within
    /// the configured limit.
    ///
    /// # Arguments
    /// * `conn` - An active, pooled database connection.
    /// * `max_rows_heating_stats` - The maximum allowed number of rows in the `heatingstats` table.
    /// * `sql_interface_heating_midnight_calculator` - A boxed trait object that provides
    ///   the implementation for calculating the duration until midnight.
    ///
    /// # Returns
    /// A `Result` containing a new `SqlInterfaceHeatingStats` instance on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - Any of the initial database queries to get table counts fail (`DatabaseCheckHeatingStatsFailure`).
    /// - Any of the retrieved counts are negative, indicating a database issue (`DatabaseHeatingStatsTableNegativeValue`).
    /// - The `heatingstats` table contains entries with `NULL` values (`DatabaseHeatingStatsTableContainsNull`).
    /// - The number of rows in the `heatingstats` table exceeds `max_rows_heating_stats` (`DatabaseHeatingStatsTableContainsTooManyRows`).
    pub fn new(
        mut conn: PooledConn,
        max_rows_heating_stats: u64,
        sql_interface_heating_midnight_calculator: Box<
            dyn SqlInterfaceMidnightCalculatorTrait + Sync + Send,
        >,
    ) -> Result<SqlInterfaceHeatingStats, SqlInterfaceError> {
        let count_null_values = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_HEATING_STATS_NULL,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckHeatingStatsFailure {
            location: module_path!().to_string(),
            source: Box::new(e),
        })?;
        let count_rows = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_HEATING_STATS_COUNT,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckHeatingStatsFailure {
            location: module_path!().to_string(),
            source: Box::new(e),
        })?;

        // check the query results
        if count_null_values < 0 || count_rows < 0 {
            return Err(SqlInterfaceError::DatabaseHeatingStatsTableNegativeValue(
                module_path!().to_string(),
                count_null_values,
                count_rows,
            ));
        }

        if count_null_values > 0 {
            return Err(SqlInterfaceError::DatabaseHeatingStatsTableContainsNull(
                module_path!().to_string(),
                count_null_values,
            ));
        }

        if max_rows_heating_stats > 0 {
            // execute the check only when the limit is greater than zero
            if count_rows > max_rows_heating_stats.cast_signed() {
                return Err(
                    SqlInterfaceError::DatabaseHeatingStatsTableContainsTooManyRows(
                        module_path!().to_string(),
                        count_rows.cast_unsigned(),
                        max_rows_heating_stats,
                    ),
                );
            }
        }

        Ok(SqlInterfaceHeatingStats {
            conn,
            sql_interface_heating_midnight_calculator,
        })
    }

    /// Retrieves a single `HeatingStatsEntry` for the current date from the database.
    ///
    /// This function executes an SQL query to fetch heating statistical data that
    /// corresponds to the database's current date. It is designed to retrieve
    /// exactly one record for "today."
    ///
    /// # Returns
    /// A `Result` containing the `HeatingStatsEntry` for the current date on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The underlying database query fails (`SingleHeatingStatsRequestFailure`). This can be
    ///   due to a connection issue or an invalid query.
    /// - The query returns zero rows, indicating no statistics have been recorded for
    ///   the current day (`SingleHeatingStatsRequestEmptyResponse`).
    /// - The query returns more than one row, which signifies a data inconsistency
    ///   (`SingleHeatingStatsRequestNoSingleResponse`).
    /// - The date string retrieved from the database cannot be parsed into a `NaiveDate`
    ///   (`TimestampConversionFailure`).
    pub fn get_single_heating_stats_from_database(
        &mut self,
    ) -> Result<HeatingStatsEntry, SqlInterfaceError> {
        // Get single heating stats entry from database
        let single_heating_stats_entry_array = self
            .conn
            .query_map(
                sql_query_strings::SQL_QUERY_READ_HEATING_STATS,
                |(
                    date,
                    energy,
                    ambient_temperature_average_value,
                    ambient_temperature_average_counter,
                    water_temperature_average_value,
                    water_temperature_average_counter,
                    heating_control_runtime,
                )| HeatingStatsEntry {
                    date,
                    energy,
                    ambient_temperature_average_value,
                    ambient_temperature_average_counter,
                    water_temperature_average_value,
                    water_temperature_average_counter,
                    heating_control_runtime,
                },
            )
            .map_err(|e| SqlInterfaceError::SingleHeatingStatsRequestFailure {
                location: module_path!().to_string(),
                query: sql_query_strings::SQL_QUERY_READ_HEATING_STATS.to_string(),
                source: e,
            })?;

        // check the error case if there is no response
        if single_heating_stats_entry_array.is_empty() {
            return Err(SqlInterfaceError::SingleHeatingStatsRequestEmptyResponse(
                module_path!().to_string(),
                sql_query_strings::SQL_QUERY_READ_HEATING_STATS.to_string(),
            ));
        }
        // check the error case if there is more than one response
        if single_heating_stats_entry_array.len() > 1 {
            return Err(
                SqlInterfaceError::SingleHeatingStatsRequestNoSingleResponse(
                    module_path!().to_string(),
                    sql_query_strings::SQL_QUERY_READ_HEATING_STATS.to_string(),
                ),
            );
        }
        Ok(single_heating_stats_entry_array[0].clone())
    }

    /// Inserts new heating statistical data into the database or updates an existing entry for a specific date.
    ///
    /// This function attempts to write a `HeatingStatsEntry` into the `heatingstats` table.
    /// It's designed to manage daily statistics, where only one unique entry per date is expected.
    ///
    /// The underlying SQL query utilizes an `INSERT ... ON DUPLICATE KEY UPDATE` clause:
    /// - If no record exists in the `heatingstats` table for the `date` specified in `heating_stats_entry`,
    ///   a new row containing all provided statistical data is inserted.
    /// - There is a `PRIMARY KEY` constraint for the `Date` column in the `heatingstats` table.
    /// - If a record *already exists* for `heating_stats_entry.date`, the existing row's
    ///   `Energy`, `AmbientTempAverage`, `AmbientTempCounter`, `WaterTempAverage`, `WaterTempCounter`,
    ///   and `HeatingControlRuntime` columns are updated with the new values from `heating_stats_entry`.
    ///
    /// **Important Note on Query Construction: **
    /// The query is constructed in two parts:
    /// 1.  The `INSERT` clause uses **parameterized queries** (e.g., `:date`, `:energy`), which is the
    ///     recommended and secure way to prevent SQL injection.
    /// 2.  The `ON DUPLICATE KEY UPDATE` clause, however, is built by **directly embedding string
    ///     representations** of the data (e.g., `Energy=#Energy`).
    ///
    /// # Arguments
    ///
    /// * `heating_stats_entry` - A reference to the `HeatingStatsEntry` struct containing all the
    ///   statistical data (date, energy, temperatures, runtime) to be inserted or updated.
    ///   The `date` field is crucial as it identifies the unique daily record.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the data was successfully inserted or updated.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::InsertHeatingStatsEntryFailure` if the database
    /// operation fails. This can be caused by a lost connection, constraint violations,
    /// incorrect data types, or a malformed SQL query. The original `mysql::Error` is
    /// included as the source for detailed debugging.
    pub fn insert_heating_stats_entry(
        &mut self,
        heating_stats_entry: &HeatingStatsEntry,
    ) -> Result<(), SqlInterfaceError> {
        let params = params! {
            "date" => heating_stats_entry.date,
            "energy" => heating_stats_entry.energy,
            "ambient_temperature_average" => heating_stats_entry.ambient_temperature_average_value,
            "ambient_temperature_counter" => heating_stats_entry.ambient_temperature_average_counter,
            "water_temperature_average" => heating_stats_entry.water_temperature_average_value,
            "water_temperature_counter" => heating_stats_entry.water_temperature_average_counter,
            "heating_control_runtime" => heating_stats_entry.heating_control_runtime,
        };

        self.conn
            .exec_drop(sql_query_strings::SQL_QUERY_WRITE_HEATING_STATS, params)
            .map_err(|e| SqlInterfaceError::InsertHeatingStatsEntryFailure {
                location: module_path!().to_string(),
                query: sql_query_strings::SQL_QUERY_WRITE_HEATING_STATS.to_string(),
                source: e,
            })
    }

    /// Calculates the duration in seconds until the next midnight.
    ///
    /// This function delegates the calculation to the `sql_interface_heating_midnight_calculator`
    /// trait object, which was provided during construction. This allows for flexible
    /// and testable time calculations.
    ///
    /// # Returns
    /// A `Result` containing the number of seconds until midnight on success.
    ///
    /// # Errors
    /// This function will return an error if the underlying calculator implementation
    /// fails. This is typically a pass-through of the error from the calculator's
    /// `get_duration_until_midnight` method.
    pub fn get_duration_until_midnight(&mut self) -> Result<i64, SqlInterfaceError> {
        self.sql_interface_heating_midnight_calculator
            .get_duration_until_midnight()
    }
}

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface_heating_stats::{
        HeatingStatsEntry, SqlInterfaceHeatingStats,
    };
    use crate::database::sql_interface_midnight::SqlInterfaceMidnightCalculator;
    use crate::database::sql_query_strings::SQL_TABLE_HEATING_STATS;
    use crate::database::{sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError};
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};
    use chrono::{Datelike, Local, NaiveDate, NaiveDateTime};

    #[test]
    // This test case verifies the validation logic within the `SqlInterfaceHeatingStats::new()` constructor.
    // It covers the following scenarios:
    // 1. Happy Path: Initialization succeeds when the table has a valid number of rows.
    // 2. Failure on Row Limit: Fails when `heatingstats` contains more rows than the configured limit.
    // 3. Deactivated Check: Succeeds even with excess rows if the limit is set to 0.
    // Test case uses test database #61.
    fn test_sql_interface_heating_stats_new() {
        // --- Common Setup ---
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            61, // Using database #61 as requested
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface.clone())
            .expect("Initialization of SQL interface for test failed.");

        // --- Test Case 1: Happy Path (empty table) ---
        println!("* Testing new() with an empty table (Happy Path)...");
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_STATS.to_string())
            .expect("Test setup failed: Could not truncate table.");

        let midnight_calc = Box::new(SqlInterfaceMidnightCalculator::new(
            sql_interface.get_connection().unwrap(),
        ));
        let result_happy_case = SqlInterfaceHeatingStats::new(
            sql_interface.get_connection().unwrap(),
            10,
            midnight_calc,
        );
        assert!(
            result_happy_case.is_ok(),
            "Expected new() to succeed with an empty table, but it failed: {:?}",
            result_happy_case.err()
        );
        println!("* Succeeded: Happy path initialization is successful on an empty table.");

        // --- Test Case 2: Failure on too many rows ---
        println!("* Testing new() with more rows than the limit...");
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_STATS.to_string())
            .expect("Test setup failed: Could not truncate table.");
        let conn = sql_interface.get_connection().unwrap();

        let mut heating_stats_entry = HeatingStatsEntry::default();
        let mut sql_interface_heating_stats_for_test_setup = result_happy_case.unwrap();
        _ = sql_interface_heating_stats_for_test_setup
            .insert_heating_stats_entry(&heating_stats_entry);
        heating_stats_entry.increment_date();
        _ = sql_interface_heating_stats_for_test_setup
            .insert_heating_stats_entry(&heating_stats_entry);

        let midnight_calc = Box::new(SqlInterfaceMidnightCalculator::new(
            sql_interface.get_connection().unwrap(),
        ));
        // Set the limit to 1
        let result = SqlInterfaceHeatingStats::new(conn, 1, midnight_calc);
        assert!(matches!(
            result,
            Err(SqlInterfaceError::DatabaseHeatingStatsTableContainsTooManyRows(_, _, _))
        ));
        println!("* Succeeded: Initialization fails if table exceeds row limit.");

        // --- Test Case 3: Deactivated check (limit is 0) ---
        println!("* Testing new() with a deactivated row limit check (limit = 0)...");
        // We reuse the state from the previous test (2 rows in the table)
        let midnight_calc = Box::new(SqlInterfaceMidnightCalculator::new(
            sql_interface.get_connection().unwrap(),
        ));
        // Set the limit to 0
        let result = SqlInterfaceHeatingStats::new(
            sql_interface.get_connection().unwrap(),
            0,
            midnight_calc,
        );
        assert!(
            result.is_ok(),
            "Expected new() to succeed with deactivated check, but it failed: {:?}",
            result.err()
        );
        println!("* Succeeded: Initialization passes when row limit check is deactivated.");
    }

    #[test]
    // Test case includes all checks for this table combined in one function to limit the number of additional databases.
    // Test case uses test database #36.
    pub fn test_sql_interface_heating() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            36,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_heating_stats = config.sql_interface.max_rows_heating_stats;
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let sql_interface_heating_midnight_calculator = Box::new(
            SqlInterfaceMidnightCalculator::new(sql_interface.get_connection().unwrap()),
        );
        let mut sql_interface_heating = SqlInterfaceHeatingStats::new(
            sql_interface.get_connection().unwrap(),
            max_rows_heating_stats,
            sql_interface_heating_midnight_calculator,
        )
        .unwrap();

        // *** retrieve heating stats - ************************************************************
        match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_STATS.to_string())
        {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        match sql_interface_heating.get_single_heating_stats_from_database() {
            Ok(_) => {
                panic!("Call to get_single_heating_stats_from_database should have failed.");
            }
            Err(e) => {
                assert!(matches!(
                    e,
                    SqlInterfaceError::SingleHeatingStatsRequestEmptyResponse(_, _)
                ));
            }
        }
        println!("* checking reading from empty heating stat table succeeded.");
        // *****************************************************************************************

        // *** insert heating stats ****************************************************************
        // prepare data for inserting into the database
        let today =
            match SqlInterface::get_current_date(&mut sql_interface.get_connection().unwrap()) {
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
        assert!(matches!(
            sql_interface_heating.insert_heating_stats_entry(&heating_stats_entry),
            Ok(())
        ));
        println!("* checking inserting heating stats succeeded.");
        // *****************************************************************************************

        // *** retrieve stats **********************************************************************
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
        println!("* checking retrieving heating stats succeeded.");

        // *** retrieve stats **********************************************************************
        // update data and trigger SQL statement (ON DUPLICATE UPDATE)
        heating_stats_entry.energy += 0.1;
        heating_stats_entry.ambient_temperature_average_value += 0.1;
        heating_stats_entry.ambient_temperature_average_counter += 1;
        heating_stats_entry.water_temperature_average_value += 0.1;
        heating_stats_entry.water_temperature_average_counter += 1;
        heating_stats_entry.heating_control_runtime += 1;
        assert!(matches!(
            sql_interface_heating.insert_heating_stats_entry(&heating_stats_entry),
            Ok(())
        ));
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
        println!("* checking retrieving updated heating stats succeeded.");
        // *****************************************************************************************

        // *** calculate duration until midnight ***************************************************
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
            - SqlInterface::get_duration_from_naive_timestamp_to_local_now(midnight)
                .unwrap()
                .num_seconds();
        match sql_interface_heating.get_duration_until_midnight() {
            Ok(c) => {
                assert_eq!(c, duration_until_midnight);
            }
            Err(_) => {
                panic!("Call to get_duration_until_midnight failed.");
            }
        }
        println!("* checking duration until midnight in seconds succeeded.");

        // *** check if too many rows are detected ***
        // insert one more entry with modified date
        heating_stats_entry.date =
            SqlInterface::get_tomorrow_date(&mut sql_interface.get_connection().unwrap()).unwrap();
        assert!(matches!(
            sql_interface_heating.insert_heating_stats_entry(&heating_stats_entry),
            Ok(())
        ));
        let sql_interface_heating_midnight_calculator = Box::new(
            SqlInterfaceMidnightCalculator::new(sql_interface.get_connection().unwrap()),
        );
        let test_result = SqlInterfaceHeatingStats::new(
            sql_interface.get_connection().unwrap(),
            1,
            sql_interface_heating_midnight_calculator,
        );
        assert!(matches!(
            test_result,
            Err(SqlInterfaceError::DatabaseHeatingStatsTableContainsTooManyRows(_, _, _))
        ));
        // *****************************************************************************************
    }
}
