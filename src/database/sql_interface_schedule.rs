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
use std::str::FromStr;

use crate::database::{sql_interface_error::SqlInterfaceError, sql_query_strings};

/// List of all modules that are subject to limitations by schedule.
/// Background: These activities cause noise - user shall be able to inhibit these noises during nighttime.
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
    /// `ScheduleType` variants (e.g., "Balling", "Refill", "Ventilation", "Heating").
    /// The comparison is case-sensitive.
    ///
    /// # Arguments
    /// * `input` - The string slice to be converted.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(ScheduleType)` if the input string matches a valid `ScheduleType` variant.
    /// - `Err(())` if the input string does not match any known `ScheduleType` variant.
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
    /// Creates a new `ScheduleEntry` struct by converting raw data from `SqlScheduleEntry`.
    ///
    /// This constructor takes a `SqlScheduleEntry` (typically read directly from the database)
    /// and performs necessary type conversions, such as transforming string representations
    /// of schedule type, start time and stop time into their respective Rust enum and
    /// `NaiveTime` types. It also converts a numeric `is_active` flag to a boolean.
    ///
    /// # Arguments
    /// * `sql_schedule_entry` - A `SqlScheduleEntry` struct containing the raw schedule data from the SQL database.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(ScheduleEntry)`: A new `ScheduleEntry` struct with all data successfully converted.
    /// - `Err(SqlInterfaceError::ScheduleTypeConversionFailure)`: If the `schedule_type` string
    ///   cannot be parsed into a valid `ScheduleType` enum variant.
    /// - `Err(SqlInterfaceError::ScheduleStartTimeConversionFailure)`: If the `start_time` string
    ///   cannot be parsed into a valid `NaiveTime`.
    /// - `Err(SqlInterfaceError::ScheduleStopTimeConversionFailure)`: If the `stop_time` string
    ///   cannot be parsed into a valid `NaiveTime`.
    pub fn new(sql_schedule_entry: SqlScheduleEntry) -> Result<ScheduleEntry, SqlInterfaceError> {
        let schedule_type = match ScheduleType::from_str(&sql_schedule_entry.schedule_type) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::ScheduleTypeConversionFailure(
                    "ScheduleEntry: Error in conversion of SQL database result to enum."
                        .to_string()
                        + &sql_schedule_entry.schedule_type,
                ));
            }
        };

        let start_time =
            match NaiveTime::parse_from_str(sql_schedule_entry.start_time.as_str(), "%H:%M:%S") {
                Ok(c) => c,
                Err(_) => {
                    return Err(SqlInterfaceError::ScheduleStartTimeConversionFailure(
                        "Could not convert start time string read from SQL database: ".to_string()
                            + &sql_schedule_entry.start_time,
                    ));
                }
            };

        let stop_time =
            match NaiveTime::parse_from_str(sql_schedule_entry.stop_time.as_str(), "%H:%M:%S") {
                Ok(c) => c,
                Err(_) => {
                    return Err(SqlInterfaceError::ScheduleStopTimeConversionFailure(
                        "Could not convert stop time string read from SQL database.".to_string()
                            + &sql_schedule_entry.stop_time,
                    ));
                }
            };

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
    /// `true` if the current local time is greater than or equal to `start_time`
    /// AND less than or equal to `stop_time`; otherwise, returns `false`.
    pub fn check_if_allowed(&self) -> bool {
        let current_time = Local::now().time();
        (current_time >= self.start_time) & (current_time <= self.stop_time)
    }
}

/// Contains the configuration and the implementation of the SQL interface for Schedule.
pub struct SqlInterfaceSchedule {
    /// Connection to the database
    conn: PooledConn,

    /// Result of the post-processed SQL query    
    schedules: Vec<ScheduleEntry>,
}

impl SqlInterfaceSchedule {
    const LOG_TARGET_SQL_INTERFACE_SCHEDULE: &'static str = "sql_interface_schedule";

    /// Provides a new `SqlInterfaceSchedule` instance.
    ///
    /// This constructor initializes the interface with a database connection
    /// and an empty internal list of schedules.
    ///
    /// # Arguments
    /// * `conn` - The established database connection pool object.
    ///
    /// # Returns
    /// A new `SqlInterfaceSchedule` struct.
    pub fn new(conn: PooledConn) -> SqlInterfaceSchedule {
        Self {
            conn,
            schedules: Vec::new(),
        }
    }

    /// Reads all schedule entries from the SQL database, processes them and updates the internal state.
    ///
    /// This function queries the database for all available schedule configurations.
    /// Each raw database entry is then converted into a `ScheduleEntry` struct,
    /// performing necessary type conversions and validations. The internal `schedules`
    /// vector of this struct is cleared and then populated with the newly processed entries.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(())` if all schedule entries were successfully read and processed.
    /// - `Err(SqlInterfaceError::ScheduleRequestFailure)` if the initial database query fails.
    /// - `Err(SqlInterfaceError)`: If any error occurs during the conversion of individual
    ///   raw schedule entries into `ScheduleEntry` structs (e.g., due to invalid data formats).
    pub fn read_schedule(&mut self) -> Result<(), SqlInterfaceError> {
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
            Err(_) => {
                return Err(SqlInterfaceError::ScheduleRequestFailure(
                    "Could not get schedule entries from SQL database using query:".to_string()
                        + sql_query_strings::SQL_QUERY_READ_SCHEDULE,
                ));
            }
        };

        self.schedules.clear();

        for schedule_string in schedule_string_array {
            match ScheduleEntry::new(schedule_string) {
                Ok(schedule_entry) => {
                    self.schedules.push(schedule_entry);
                }
                Err(e) => {
                    error!(
                        target: Self::LOG_TARGET_SQL_INTERFACE_SCHEDULE,
                        "reading schedule failed ({e:?})"
                    );
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// Retrieves the schedule for a given schedule type.
    /// # Arguments
    /// * `schedule_type` - information which type of schedule is requested
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(&ScheduleEntry)` in case the schedule could be retrieved successfully.
    /// - `Err(SqlInterfaceError)` if the schedule could not be retrieved.
    pub fn find_schedule(
        &self,
        schedule_type: ScheduleType,
    ) -> Result<&ScheduleEntry, SqlInterfaceError> {
        for schedule in &self.schedules {
            if schedule.schedule_type == schedule_type {
                return Ok(schedule);
            }
        }
        Err(SqlInterfaceError::ScheduleTypeNotFound(
            schedule_type.to_string(),
        ))
    }
}

#[cfg(test)]
pub mod tests {
    use crate::database::sql_interface::SqlInterface;
    use crate::database::sql_interface_schedule::{
        ScheduleEntry, ScheduleType, SqlInterfaceSchedule, SqlScheduleEntry,
    };
    use crate::utilities::config::{read_config_file_with_test_database, ConfigData};

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
        let sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_schedule = SqlInterfaceSchedule::new(sql_interface.get_connection());

        match sql_interface_schedule.read_schedule() {
            Ok(_) => {
                println!("Reading schedule succeeded checking content now.");
            }
            Err(e) => {
                panic!("Reading schedule from SQL database failed: {e:?}");
            }
        }

        let schedule_entry_reference_balling = ScheduleEntry::new(SqlScheduleEntry {
            schedule_type: "Balling".to_string(),
            start_time: "01:00:00".to_string(),
            stop_time: "23:00:00".to_string(),
            is_active: 1,
        })
        .unwrap();

        let schedule_entry_reference_refill = ScheduleEntry::new(SqlScheduleEntry {
            schedule_type: "Refill".to_string(),
            start_time: "02:00:00".to_string(),
            stop_time: "22:00:00".to_string(),
            is_active: 1,
        })
        .unwrap();

        let schedule_entry_reference_ventilation = ScheduleEntry::new(SqlScheduleEntry {
            schedule_type: "Ventilation".to_string(),
            start_time: "03:00:00".to_string(),
            stop_time: "21:00:00".to_string(),
            is_active: 1,
        })
        .unwrap();

        let schedule_entry_reference_heating = ScheduleEntry::new(SqlScheduleEntry {
            schedule_type: "Heating".to_string(),
            start_time: "00:00:00".to_string(),
            stop_time: "23:59:00".to_string(),
            is_active: 1,
        })
        .unwrap();

        for schedule_entry in sql_interface_schedule.schedules {
            let reference = match schedule_entry.schedule_type {
                ScheduleType::Balling => &schedule_entry_reference_balling,
                ScheduleType::Refill => &schedule_entry_reference_refill,
                ScheduleType::Ventilation => &schedule_entry_reference_ventilation,
                ScheduleType::Heating => &schedule_entry_reference_heating,
            };
            assert_eq!(reference.start_time, schedule_entry.start_time);
            assert_eq!(reference.stop_time, schedule_entry.stop_time);
            assert_eq!(reference.is_active, schedule_entry.is_active);
        }
    }
}
