/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages time-based scheduling for various application modules.
//!
//! This module provides a dedicated interface, `SqlInterfaceSchedule`, for reading and
//! interpreting time-based restrictions from the `schedule` table in the database.
//! Its primary purpose is to allow users to inhibit certain activities that may cause
//! noise or other disturbances (e.g., pumps for Balling or Refill) during specific
//! times, such as at night.
//!
//! ## Key Components
//!
//! - **`SqlInterfaceSchedule`**: The main struct that holds a database connection,
//!   performs validation, and provides methods to read and query schedule data.
//!
//! - **`ScheduleEntry`**: A processed, type-safe representation of a single schedule
//!   rule. It converts raw string data from the database into strongly typed
//!   `ScheduleType` and `chrono::NaiveTime` fields.
//!
//! - **`ScheduleType`**: An enum that identifies the specific application module
//!   (e.g., `Balling`, `Refill`, `Heating`) that a schedule rule applies to.
//!
//! ## Design and Purpose
//!
//! The module is designed to be a robust and centralized point for all scheduling logic.
//!
//! - **Fail-Fast Validation**: The `new()` constructor performs several "fail-fast"
//!   checks at startup. It verifies that the `schedule` table contains no `NULL`
//!   values and that the number of entries does not exceed a configured limit. This
//!   prevents the application from starting in an invalid state.
//!
//! - **Data Processing and Caching**: The `read_schedule()` method fetches all rules
//!   from the database, parses them into `ScheduleEntry` objects, and stores them in
//!   an internal cache (`Vec`). This is efficient as the database is only queried once.
//!
//! - **Robust Error Handling**: During processing, `read_schedule()` does not fail on
//!   the first malformed entry. Instead, it collects all parsing errors (e.g., for
//!   an invalid time string) and returns them in a `Vec`, allowing for comprehensive
//!   diagnostics of database integrity issues.
//!
//! - **Time Window Logic**: The `ScheduleEntry::check_if_allowed()` method provides a
//!   simple way to determine if the current system time falls within the active
//!   window of a given schedule.

use chrono::*;
use mysql::prelude::*;
use mysql::*;
use std::fmt;
use std::str::FromStr;

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_query_strings::{
    SQL_QUERY_CHECK_SCHEDULE_COUNT, SQL_QUERY_CHECK_SCHEDULE_NULL,
};
use crate::database::{sql_interface_error::SqlInterfaceError, sql_query_strings};

/// List of all modules that are subject to limitations by schedule.
/// Background: These activities cause noise - the user shall be able to inhibit these noises during nighttime.
#[derive(PartialEq, Debug)]
pub enum ScheduleType {
    Balling,
    Refill,
    Ventilation,
    Heating,
}

impl FromStr for ScheduleType {
    type Err = ();

    /// Converts a string slice into a `ScheduleType` enum variant.
    ///
    /// This function attempts to match the input string to one of the predefined
    /// `ScheduleType` variants (e.g., "Balling", "Refill"). The comparison is case-sensitive.
    ///
    /// # Arguments
    /// * `input` - The string slice to be converted.
    ///
    /// # Returns
    /// A `Result` containing the corresponding `ScheduleType` variant on a successful match.
    ///
    /// # Errors
    /// Returns an empty `Err(())` if the input string does not match any of the known
    /// `ScheduleType` variants.
    fn from_str(input: &str) -> Result<ScheduleType, Self::Err> {
        match input {
            "Balling" => Ok(ScheduleType::Balling),
            "Refill" => Ok(ScheduleType::Refill),
            "Ventilation" => Ok(ScheduleType::Ventilation),
            "Heating" => Ok(ScheduleType::Heating),
            _ => Err(()),
        }
    }
}

impl fmt::Display for ScheduleType {
    /// Formats the `ScheduleType` enum into a human-readable string representation.
    ///
    /// This implementation allows `ScheduleType` variants to be displayed directly
    /// using `println!` or `format!`, returning the string name of the variant.
    ///
    /// # Arguments
    /// * `f` - A mutable reference to the formatter.
    ///
    /// # Returns
    /// A `fmt::Result` indicating whether the formatting was successful.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ScheduleType::Balling => write!(f, "Balling"),
            ScheduleType::Refill => write!(f, "Refill"),
            ScheduleType::Ventilation => write!(f, "Ventilation"),
            ScheduleType::Heating => write!(f, "Heating"),
        }
    }
}

/// Contains one schedule data set as read from the database
pub struct SqlScheduleEntry {
    /// Schedule type (either Balling, Refill, Ventilation or Heating)
    schedule_type: String,

    /// Time of day from which on the control shall be active
    start_time: NaiveTime,

    /// Time of day from which on the control shall not be active
    stop_time: NaiveTime,

    /// Flag describing if the schedule entry is active
    is_active: u32,
}

/// Contains one schedule data set after post-processing
#[derive(Debug)]
pub struct ScheduleEntry {
    /// Schedule type (either Balling, Refill, Ventilation or Heating)
    schedule_type: ScheduleType,

    /// Time of day from which on the control shall be active
    start_time: NaiveTime,

    /// Time of day from which on the control shall not be active
    stop_time: NaiveTime,

    /// Flag describing if the schedule entry is active
    #[allow(unused)]
    is_active: bool,
}

impl ScheduleEntry {
    /// Creates a new `ScheduleEntry` by converting raw data from an `SqlScheduleEntry`.
    ///
    /// This constructor takes a `SqlScheduleEntry` (typically read from the database)
    /// and performs necessary type conversions, such as transforming string representations
    /// of the schedule type and times into their respective Rust types.
    ///
    /// # Arguments
    /// * `sql_schedule_entry` - A struct containing the raw schedule data from the SQL database.
    ///
    /// # Returns
    /// A `Result` containing a new, fully typed `ScheduleEntry` on success.
    ///
    /// # Errors
    /// This function will return an `Err` variant of `SqlInterfaceError` if:
    /// - The `schedule_type` string cannot be parsed into a valid `ScheduleType` enum
    ///   (`ScheduleTypeConversionFailure`).
    pub fn new(sql_schedule_entry: SqlScheduleEntry) -> Result<ScheduleEntry, SqlInterfaceError> {
        let schedule_type =
            ScheduleType::from_str(&sql_schedule_entry.schedule_type).map_err(|_| {
                SqlInterfaceError::ScheduleTypeConversionFailure(
                    module_path!().to_string(),
                    sql_schedule_entry.schedule_type,
                )
            })?;

        let is_active = sql_schedule_entry.is_active > 0;

        Ok(Self {
            schedule_type,
            start_time: sql_schedule_entry.start_time,
            stop_time: sql_schedule_entry.stop_time,
            is_active,
        })
    }

    /// Determines if the current local time falls within the allowed time window of this schedule entry.
    ///
    /// This function compares the system's current local time (`Local::now().time()`)
    /// against the `start_time` and `stop_time` defined in this `ScheduleEntry`.
    ///
    /// # Returns
    /// - `true`: If the current local time is on or after `start_time` AND on or before `stop_time`.
    /// - `false`: Given an active schedule limitation: If the current local time is outside this inclusive window.
    pub fn check_if_allowed(&self) -> bool {
        let current_time = Local::now().time();
        if self.is_active {
            // allow operation only if time is within the permissible interval
            (current_time >= self.start_time) && (current_time <= self.stop_time)
        } else {
            // allow operation if schedule limitation is not active
            true
        }
    }

    /// Performs the `new_for_test` operation.
    ///
    /// # Arguments
    /// * `schedule_type` - A parameter representing `schedule_type`.
    /// * `start_time` - A parameter representing `start_time`.
    /// * `stop_time` - A parameter representing `stop_time`.
    /// * `is_active` - A parameter representing `is_active`.
    #[cfg(test)]
    pub fn new_for_test(
        schedule_type: ScheduleType,
        start_time: NaiveTime,
        stop_time: NaiveTime,
        is_active: bool,
    ) -> ScheduleEntry {
        Self {
            schedule_type,
            start_time,
            stop_time,
            is_active,
        }
    }

    /// Performs the `deactivate` operation.
    #[cfg(test)]
    pub fn deactivate(&mut self) {
        self.is_active = false;
    }
}

/// Contains the configuration and the implementation of the SQL interface for Schedule.
#[derive(Debug)]
pub struct SqlInterfaceSchedule {
    /// Connection to the database
    pub conn: PooledConn,

    /// Result of the post-processed SQL query    
    schedules: Vec<ScheduleEntry>,
}

impl SqlInterfaceSchedule {
    /// Creates a new `SqlInterfaceSchedule` instance.
    ///
    /// This constructor initializes the interface with a database connection. It performs
    /// several pre-flight checks to ensure data integrity, such as verifying that the
    /// `schedule` table contains no NULL values and that its row count is within the
    /// configured limit.
    ///
    /// # Arguments
    /// * `conn` - An active, pooled database connection.
    /// * `max_rows_schedule` - The maximum allowed number of rows in the `schedule` table.
    ///
    /// # Returns
    /// A `Result` containing a new `SqlInterfaceSchedule` instance on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - Any of the initial database queries to get table counts fail (`DatabaseCheckScheduleFailure`).
    /// - Any of the retrieved counts are negative, indicating a database issue (`DatabaseScheduleTableNegativeValue`).
    /// - The `schedule` table contains entries with `NULL` values (`DatabaseScheduleTableContainsNull`).
    /// - The number of rows in the `schedule` table exceeds `max_rows_schedule` (`DatabaseScheduleTableContainsTooManyRows`).
    pub fn new(
        mut conn: PooledConn,
        max_rows_schedule: u64,
    ) -> Result<SqlInterfaceSchedule, SqlInterfaceError> {
        let count_null_values = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_SCHEDULE_NULL,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckScheduleFailure {
            location: module_path!().to_string(),
            source: Box::new(e),
        })?;
        let count_rows = SqlInterface::get_single_integer_from_database(
            &mut conn,
            SQL_QUERY_CHECK_SCHEDULE_COUNT,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckScheduleFailure {
            location: module_path!().to_string(),
            source: Box::new(e),
        })?;

        // check the query results
        if count_null_values < 0 || count_rows < 0 {
            return Err(SqlInterfaceError::DatabaseScheduleTableNegativeValue(
                module_path!().to_string(),
                count_null_values,
                count_rows,
            ));
        }

        if count_null_values > 0 {
            return Err(SqlInterfaceError::DatabaseScheduleTableContainsNull(
                module_path!().to_string(),
                count_null_values,
            ));
        }

        if max_rows_schedule > 0 {
            // execute the check only when the limit is greater than zero
            if count_rows > max_rows_schedule.cast_signed() {
                return Err(SqlInterfaceError::DatabaseScheduleTableContainsTooManyRows(
                    module_path!().to_string(),
                    count_rows.cast_unsigned(),
                    max_rows_schedule,
                ));
            }
        }

        Ok(Self {
            conn,
            schedules: Vec::new(),
        })
    }

    /// Reads all schedule entries from the SQL database, processing and storing them internally.
    ///
    /// This function queries the database for all available schedule configurations.
    /// It then attempts to convert each raw database entry into a `ScheduleEntry` struct.
    /// The internal `schedules` vector is cleared and then populated with only the
    /// successfully processed entries.
    ///
    /// # Returns
    /// - `Ok(())`: If the database query was successful and all retrieved entries were processed without errors.
    ///
    /// # Errors
    /// Returns a `Vec<SqlInterfaceError>` which will contain one or more errors if:
    /// - The initial database query fails (`ScheduleRequestFailure`).
    /// - One or more raw schedule entries fail to be converted into a `ScheduleEntry`
    ///   (`ScheduleProcessingFailure`). This can happen due to invalid data formats
    ///   (e.g., a malformed time string) in the database.
    pub fn read_schedule(&mut self) -> Result<(), Vec<SqlInterfaceError>> {
        // Get all schedule entries from the database
        let schedule_entry_array = match self.conn.query_map(
            sql_query_strings::SQL_QUERY_READ_SCHEDULE,
            |(schedule_type, start_time, stop_time, is_active)| SqlScheduleEntry {
                schedule_type,
                start_time,
                stop_time,
                is_active,
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                return Err(vec![SqlInterfaceError::ScheduleRequestFailure {
                    location: module_path!().to_string(),
                    query: sql_query_strings::SQL_QUERY_READ_SCHEDULE.to_string(),
                    source: e,
                }]);
            }
        };

        self.schedules.clear();

        let mut errors = vec![]; // for collecting the errors
        for schedule_string in schedule_entry_array {
            match ScheduleEntry::new(schedule_string) {
                Ok(schedule_entry) => {
                    self.schedules.push(schedule_entry);
                }
                Err(e) => {
                    errors.push(
                        // collect the error
                        SqlInterfaceError::ScheduleProcessingFailure {
                            location: module_path!().to_string(),
                            source: Box::new(e),
                        },
                    );
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Finds a specific schedule entry by its type from the internal cache.
    ///
    /// This function searches the `schedules` vector, which must be populated by
    /// a prior call to `read_schedule`, for an entry matching the given `schedule_type`.
    ///
    /// # Arguments
    /// * `schedule_type` - The `ScheduleType` to search for.
    ///
    /// # Returns
    /// A `Result` containing a reference to the found `ScheduleEntry` on success.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::ScheduleTypeNotFound` if no schedule entry
    /// matching the provided `schedule_type` exists in the internal cache.
    pub fn find_schedule(
        &self,
        schedule_type: ScheduleType,
    ) -> Result<&ScheduleEntry, SqlInterfaceError> {
        self.schedules
            .iter()
            .find(|&schedule| schedule.schedule_type == schedule_type)
            .ok_or_else(|| {
                // Generate a comma-separated string of all available schedule types.
                let available_types = self
                    .schedules
                    .iter()
                    .map(|s| s.schedule_type.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");

                SqlInterfaceError::ScheduleTypeNotFound(
                    module_path!().to_string(),
                    schedule_type.to_string(),
                    available_types,
                )
            })
    }
}

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface::SqlInterface;
    use crate::database::sql_interface_error::SqlInterfaceError;
    use crate::database::sql_interface_schedule::{
        ScheduleEntry, ScheduleType, SqlInterfaceSchedule, SqlScheduleEntry,
    };
    use crate::database::sql_query_strings;
    use crate::database::sql_query_strings::SQL_TABLE_SCHEDULE;
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};
    use mysql::params;
    use mysql::prelude::Queryable;
    use std::collections::HashMap;

    #[test]
    // This test case verifies the validation and data processing logic for SqlInterfaceSchedule.
    // It covers the following scenarios:
    // 1. `new()`: Happy path, failure on NULL, and failure on row limits.
    // 2. `read_schedule()`: Failure on invalid data (bad schedule type) and partial success.
    // Test case uses test database #63.
    fn test_sql_interface_schedule_new_and_read() {
        // --- Common Setup ---
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            63, // Using database #63 as requested
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface.clone())
            .expect("Initialization of SQL interface for test failed.");

        // --- Test `new()`: Happy Path (empty table) ---
        println!("* Testing new() with an empty table (Happy Path)...");
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string())
            .expect("Test setup failed: Could not truncate table.");

        let result = SqlInterfaceSchedule::new(sql_interface.get_connection().unwrap(), 10);
        assert!(
            result.is_ok(),
            "Expected new() to succeed with an empty table, but it failed: {:?}",
            result.err()
        );
        println!("* Succeeded: Happy path initialization is successful on an empty table.");

        // --- Test `new()`: Failure on too many rows ---
        println!("* Testing new() with more rows than the limit...");
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string())
            .expect("Test setup failed: Could not truncate table.");
        let mut conn = sql_interface.get_connection().unwrap();
        conn.exec_drop(
            sql_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
            params! { "schedule_name" => "Balling", "start_time" => "01:00:00", "stop_time" => "02:00:00" },
        )
            .unwrap();
        conn.exec_drop(
            sql_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
            params! { "schedule_name" => "Refill", "start_time" => "03:00:00", "stop_time" => "04:00:00" },
        )
            .unwrap();

        // Set the limit to 1
        let result = SqlInterfaceSchedule::new(conn, 1);
        assert!(
            matches!(
                result,
                Err(SqlInterfaceError::DatabaseScheduleTableContainsTooManyRows(
                    _,
                    _,
                    _
                ))
            ),
            "Expected row limit error, but got {:?}",
            result
        );
        println!("* Succeeded: Initialization fails if table exceeds row limit.");

        // --- Test `read_schedule()`: Failure on invalid data ---
        println!("* Testing read_schedule() with an invalid schedule type...");
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string())
            .expect("Test setup failed: Could not truncate table.");
        let mut conn = sql_interface.get_connection().unwrap();
        conn.exec_drop(
            sql_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
            params! { "schedule_name" => "InvalidType", "start_time" => "01:00:00", "stop_time" => "02:00:00" },
        )
            .unwrap();

        let mut schedule_db = SqlInterfaceSchedule::new(conn, 10).unwrap();
        let read_result = schedule_db.read_schedule();

        assert!(
            matches!(read_result, Err(ref errors) if !errors.is_empty()),
            "Expected read_schedule to return a vector of errors"
        );

        if let Err(errors) = read_result {
            assert!(matches!(
                errors[0],
                SqlInterfaceError::ScheduleProcessingFailure { .. }
            ));
            println!("* Succeeded: read_schedule correctly fails on invalid data.");
        }

        // --- Test `read_schedule()`: Partial success ---
        println!("* Testing read_schedule() with mixed valid and invalid data...");
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string())
            .expect("Test setup failed: Could not truncate table.");
        let mut conn = sql_interface.get_connection().unwrap();
        // Insert one valid row
        conn.exec_drop(
            sql_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
            params! { "schedule_name" => "Balling", "start_time" => "01:00:00", "stop_time" => "02:00:00" },
        )
            .unwrap();
        // Insert one invalid row
        conn.exec_drop(
            sql_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
            params! { "schedule_name" => "AnotherInvalidType", "start_time" => "03:00:00", "stop_time" => "04:00:00" },
        )
            .unwrap();

        let mut schedule_db = SqlInterfaceSchedule::new(conn, 10).unwrap();
        let read_result = schedule_db.read_schedule();

        // Expect an error, but also check that the valid data was processed.
        assert!(
            read_result.is_err(),
            "Expected an error due to the invalid entry"
        );
        assert_eq!(
            schedule_db.schedules.len(),
            1,
            "Expected exactly one valid schedule to be processed"
        );
        assert_eq!(
            schedule_db.schedules[0].schedule_type,
            ScheduleType::Balling
        );
        println!("* Succeeded: read_schedule correctly processes valid data while reporting errors for invalid data.");
    }

    #[test]
    // Test case reads entries from the SQL database.
    // Since the application does not modify schedule data in the SQL database,
    // results are compared to fixed references.
    // Test case uses test database #38.
    pub fn test_sql_interface_schedule() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            38,
        );
        println!("Testing with database {}", config.sql_interface.db_name);
        let max_rows_schedule = config.sql_interface.max_rows_schedule;
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string()).unwrap();

        let test_data = [
            ("Balling", "01:00:00", "23:00:00"),
            ("Heating", "00:00:00", "23:59:00"),
            ("Ventilation", "03:00:00", "21:00:00"),
            ("Refill", "02:00:00", "22:00:00"),
        ];

        for (name, start, stop) in &test_data {
            sql_interface
                .conn
                .exec_drop(
                    sql_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
                    params! {
                        "schedule_name" => *name,
                        "start_time" => *start,
                        "stop_time" => *stop,
                    },
                )
                .unwrap();
        }

        let mut sql_interface_schedule =
            SqlInterfaceSchedule::new(sql_interface.get_connection().unwrap(), max_rows_schedule)
                .unwrap();

        match sql_interface_schedule.read_schedule() {
            Ok(_) => {
                println!("Reading schedule succeeded checking content now.");
            }
            Err(e) => {
                panic!("Reading schedule from SQL database failed: {e:?}");
            }
        }

        let references: HashMap<String, _> = test_data
            .iter()
            .map(|(name, start, stop)| {
                let entry = ScheduleEntry::new(SqlScheduleEntry {
                    schedule_type: name.to_string(),
                    start_time: start.to_string().parse().unwrap(),
                    stop_time: stop.to_string().parse().unwrap(),
                    is_active: 1,
                })
                .unwrap();
                (name.to_string(), entry)
            })
            .collect();

        assert_eq!(sql_interface_schedule.schedules.len(), 4);

        for schedule_entry in sql_interface_schedule.schedules {
            let reference = references
                .get(&schedule_entry.schedule_type.to_string())
                .unwrap();
            assert_eq!(reference.start_time, schedule_entry.start_time);
            assert_eq!(reference.stop_time, schedule_entry.stop_time);
            assert_eq!(reference.is_active, schedule_entry.is_active);
        }
        // *** check if row limitation is active ***
        let test_result = SqlInterfaceSchedule::new(sql_interface.conn, 1);
        assert!(matches!(
            test_result,
            Err(SqlInterfaceError::DatabaseScheduleTableContainsTooManyRows(
                _,
                _,
                _
            ))
        ));
    }

    #[test]
    // Test case verifies that the correct error is returned when a schedule is not found.
    // Test case uses test database #53.
    pub fn test_sql_interface_schedule_not_found() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            53,
        );
        println!(
            "Testing ScheduleTypeNotFound with database {}",
            config.sql_interface.db_name
        );
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface.clone())
            .expect("Initialization of SQL interface for test failed.");
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string()).unwrap();

        // Insert a SUBSET of schedules to test the "not found" case
        let test_data = [
            ("Heating", "00:00:00", "23:59:00"),
            ("Ventilation", "03:00:00", "21:00:00"),
        ];

        for (name, start, stop) in &test_data {
            sql_interface
                .conn
                .exec_drop(
                    sql_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
                    params! {
                        "schedule_name" => *name,
                        "start_time" => *start,
                        "stop_time" => *stop,
                    },
                )
                .unwrap();
        }

        let mut sql_interface_schedule = SqlInterfaceSchedule::new(
            sql_interface.get_connection().unwrap(),
            config.sql_interface.max_rows_schedule,
        )
        .unwrap();

        sql_interface_schedule
            .read_schedule()
            .expect("Reading schedule from SQL database failed");

        // Now, try to find a schedule that was NOT inserted (e.g., Balling)
        println!("Checking for non-existent schedule entry (Balling).");
        let find_result = sql_interface_schedule.find_schedule(ScheduleType::Balling);

        // Assert that we get the correct error variant
        assert!(matches!(
            find_result,
            Err(SqlInterfaceError::ScheduleTypeNotFound(_, _, _))
        ));

        // For more robust testing, also verify the content of the error message.
        if let Err(SqlInterfaceError::ScheduleTypeNotFound(_, requested, available)) = find_result {
            assert_eq!(requested, "Balling");
            assert!(available.contains("Heating") && available.contains("Ventilation"));
            println!(
                "Correctly received ScheduleTypeNotFound for '{}'. Available types: '{}'",
                requested, available
            );
        }
    }
}
