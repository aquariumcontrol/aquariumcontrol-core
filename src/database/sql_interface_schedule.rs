/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

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
#[derive(PartialEq)]
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
    start_time: String,

    /// Time of day from which on the control shall not be active
    stop_time: String,

    /// Flag describing if the schedule entry is active
    is_active: u32,
}

/// Contains one schedule data set after post-processing
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
    /// - The `start_time` string cannot be parsed into a valid `NaiveTime`
    ///   (`ScheduleStartTimeConversionFailure`).
    /// - The `stop_time` string cannot be parsed into a valid `NaiveTime`
    ///   (`ScheduleStopTimeConversionFailure`).
    pub fn new(sql_schedule_entry: SqlScheduleEntry) -> Result<ScheduleEntry, SqlInterfaceError> {
        let schedule_type =
            ScheduleType::from_str(&sql_schedule_entry.schedule_type).map_err(|_| {
                SqlInterfaceError::ScheduleTypeConversionFailure(
                    module_path!().to_string(),
                    sql_schedule_entry.schedule_type,
                )
            })?;

        let start_time =
            NaiveTime::parse_from_str(sql_schedule_entry.start_time.as_str(), "%H:%M:%S").map_err(
                |_| {
                    SqlInterfaceError::ScheduleStartTimeConversionFailure(
                        module_path!().to_string(),
                        sql_schedule_entry.start_time,
                    )
                },
            )?;

        let stop_time =
            NaiveTime::parse_from_str(sql_schedule_entry.stop_time.as_str(), "%H:%M:%S").map_err(
                |_| {
                    SqlInterfaceError::ScheduleStopTimeConversionFailure(
                        module_path!().to_string(),
                        sql_schedule_entry.stop_time,
                    )
                },
            )?;

        let is_active = sql_schedule_entry.is_active > 0;

        Ok(Self {
            schedule_type,
            start_time,
            stop_time,
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
    /// - `false`: If the current local time is outside of this inclusive window.
    pub fn check_if_allowed(&self) -> bool {
        let current_time = Local::now().time();
        (current_time >= self.start_time) && (current_time <= self.stop_time)
    }
}

/// Contains the configuration and the implementation of the SQL interface for Schedule.
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
        let schedule_string_array = match self.conn.query_map(
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
        for schedule_string in schedule_string_array {
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
                    start_time: start.to_string(),
                    stop_time: stop.to_string(),
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

    #[test]
    // Test case verifies that the correct error is returned for a malformed start time.
    // Test case uses test database #54.
    pub fn test_sql_interface_schedule_start_time_failure() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            54, // Use database #54
        );
        println!(
            "Testing ScheduleStartTimeConversionFailure with database {}",
            config.sql_interface.db_name
        );
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface.clone())
            .expect("Initialization of SQL interface for test failed.");
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string()).unwrap();

        // Insert a schedule with a malformed start_time
        let malformed_start_time = "99:00:00";
        sql_interface
            .conn
            .exec_drop(
                sql_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
                params! {
                    "schedule_name" => "Heating",
                    "start_time" => malformed_start_time,
                    "stop_time" => "23:59:00",
                },
            )
            .unwrap();

        let mut sql_interface_schedule = SqlInterfaceSchedule::new(
            sql_interface.get_connection().unwrap(),
            config.sql_interface.max_rows_schedule,
        )
        .unwrap();

        // read_schedule should return an error
        let read_result = sql_interface_schedule.read_schedule();

        // Assert that we get an error
        assert!(read_result.is_err());

        if let Err(errors) = read_result {
            assert_eq!(errors.len(), 1);
            let error = &errors[0];
            assert!(matches!(
                error,
                SqlInterfaceError::ScheduleProcessingFailure { .. }
            ));

            // Check the nested error source
            if let SqlInterfaceError::ScheduleProcessingFailure { source, .. } = error {
                assert!(matches!(
                    **source, // Dereference the Box<SqlInterfaceError>
                    SqlInterfaceError::ScheduleStartTimeConversionFailure(_, _)
                ));
                if let SqlInterfaceError::ScheduleStartTimeConversionFailure(_, time_str) =
                    &**source
                {
                    assert_eq!(time_str, malformed_start_time);
                    println!(
                        "Correctly received ScheduleStartTimeConversionFailure for time '{}'",
                        time_str
                    );
                }
            }
        }
    }

    #[test]
    // Test case verifies that the correct error is returned for a malformed stop time.
    // Test case uses test database #55.
    pub fn test_sql_interface_schedule_stop_time_failure() {
        let config: ConfigData = read_config_file_with_test_database(
            "/config/aquarium_control_test_generic.toml".to_string(),
            55, // Use database #55
        );
        println!(
            "Testing ScheduleStopTimeConversionFailure with database {}",
            config.sql_interface.db_name
        );
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface.clone())
            .expect("Initialization of SQL interface for test failed.");
        SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_SCHEDULE.to_string()).unwrap();

        // Insert a schedule with a malformed stop_time
        let malformed_stop_time = "25:00:00";
        sql_interface
            .conn
            .exec_drop(
                sql_query_strings::SQL_QUERY_WRITE_SCHEDULE_TEST_DATA,
                params! {
                    "schedule_name" => "Heating",
                    "start_time" => "00:00:00",
                    "stop_time" => malformed_stop_time,
                },
            )
            .unwrap();

        let mut sql_interface_schedule = SqlInterfaceSchedule::new(
            sql_interface.get_connection().unwrap(),
            config.sql_interface.max_rows_schedule,
        )
        .unwrap();

        // read_schedule should return an error
        let read_result = sql_interface_schedule.read_schedule();

        // Assert that we get an error
        assert!(read_result.is_err());

        if let Err(errors) = read_result {
            assert_eq!(errors.len(), 1);
            let error = &errors[0];
            assert!(matches!(
                error,
                SqlInterfaceError::ScheduleProcessingFailure { .. }
            ));

            // Check the nested error source
            if let SqlInterfaceError::ScheduleProcessingFailure { source, .. } = error {
                assert!(matches!(
                    **source, // Dereference the Box<SqlInterfaceError>
                    SqlInterfaceError::ScheduleStopTimeConversionFailure(_, _)
                ));
                if let SqlInterfaceError::ScheduleStopTimeConversionFailure(_, time_str) = &**source
                {
                    assert_eq!(time_str, malformed_stop_time);
                    println!(
                        "Correctly received ScheduleStopTimeConversionFailure for time '{}'",
                        time_str
                    );
                }
            }
        }
    }
}
