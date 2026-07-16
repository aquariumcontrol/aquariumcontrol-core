/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Manages all database interactions for time-series sensor and state data.
//!
//! This module provides a dedicated interface, `SqlInterfaceData`, for handling
//! all SQL operations related to the main `data` table. It encapsulates the logic
//! for writing `RecorderDataFrame` objects, which contain a snapshot of all sensor
//! readings and system states at a specific point in time.
//!
//! ## Key Components
//!
//! - **`SqlInterfaceData` Struct**: The primary struct that holds a database
//!   connection and provides methods for all data-logging database operations.
//!
//! - **`new()` Constructor**: A critical entry point that not only creates an
//!   `SqlInterfaceData` instance but also performs essential "fail-fast"
//!   validation. At initialization, it checks whether the number of rows in the
//!   `data` table exceeds the `max_rows_data` limit defined in the application
//!   configuration. This acts as a safeguard against uncontrolled table size at startup.
//!
//! - **`write_data_frame_to_database()`**: The core function of this module. It takes
//!   a `RecorderDataFrame` and constructs a single `INSERT ... ON DUPLICATE KEY UPDATE`
//!   query. It intelligently handles optional or invalid floating-point values
//!   (e.g., `None`, `NaN`, `Infinity`) by converting them to `NULL` before insertion,
//!   ensuring data integrity in the database.
//!
//! ## Design and Purpose
//!
//! The main goal of this module is to provide a robust and encapsulated interface
//! for persisting the application's high-frequency time-series data.
//!
//! - **Encapsulation**: All SQL logic specific to the `data` table is contained
//!   within this module, separating concerns and simplifying the data logger's
//!   responsibilities.
//!
//! - **Data Integrity**: The constructor's validation logic enforces the configured
//!   row limit at startup. The `write_data_frame_to_database` function ensures that
//!   invalid or missing data points are correctly stored as `NULL` in the database,
//!   preventing data corruption.

use mysql::prelude::*;
use mysql::*;

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_query_strings::SQL_QUERY_CHECK_DATA_COUNT;
use crate::database::{sql_interface_error::SqlInterfaceError, sql_query_strings};
use crate::recorder::recorder_data_frame::RecorderDataFrame;

/// Contains the configuration and the implementation of the SQL interface for time-based data.
#[derive(Debug)]
pub struct SqlInterfaceData {
    /// Connection to the database for retrieving timestamps
    pub conn_for_timestamp: PooledConn,

    /// Connection to the database for writing data
    pub conn: PooledConn,
}

impl SqlInterfaceData {
    /// Creates a new `SqlInterfaceData` instance.
    ///
    /// This constructor initializes the time-based data SQL interface with an
    /// established database connection. It performs a pre-flight check to ensure
    /// the number of rows in the `data` table does not exceed the configured limit.
    ///
    /// # Arguments
    /// * `conn_for_timestamp` - An active, pooled database connection for retrieving timestamps.
    /// * `conn_for_data` - An active, pooled database connection for writing data.
    /// * `max_rows_data` - The maximum allowed number of rows in the `data` table.
    ///   If `0`, the check is skipped.
    ///
    /// # Returns
    /// A `Result` containing a new `SqlInterfaceData` instance on success.
    ///
    /// # Errors
    /// This function will return an error if:
    /// - The initial database query to get the table row count fails (`DatabaseCheckDataFailure`).
    /// - The retrieved row count is a negative number, indicating a database issue (`DatabaseDataTableNegativeValue`).
    /// - The number of rows in the `data` table exceeds the configured `max_rows_data` limit (`DatabaseDataTableContainsTooManyRows`).
    pub fn new(
        conn_for_timestamp: PooledConn,
        mut conn_for_data: PooledConn,
        max_rows_data: u64,
    ) -> Result<SqlInterfaceData, SqlInterfaceError> {
        let count_rows = SqlInterface::get_single_integer_from_database(
            &mut conn_for_data,
            SQL_QUERY_CHECK_DATA_COUNT,
        )
        .map_err(|e| SqlInterfaceError::DatabaseCheckDataFailure {
            location: module_path!().to_string(),
            source: Box::new(e),
        })?;

        // check the query results
        if count_rows < 0 {
            return Err(SqlInterfaceError::DatabaseDataTableNegativeValue(
                module_path!().to_string(),
                count_rows,
            ));
        }

        if max_rows_data > 0 {
            // execute the check only when the limit is greater than zero
            if count_rows as u64 > max_rows_data {
                return Err(SqlInterfaceError::DatabaseDataTableContainsTooManyRows(
                    module_path!().to_string(),
                    count_rows as u64,
                    max_rows_data,
                ));
            }
        }
        Ok(SqlInterfaceData {
            conn_for_timestamp,
            conn: conn_for_data,
        })
    }

    /// Writes a complete `RecorderDataFrame` to the SQL database.
    ///
    /// This function processes all data points within the provided `RecorderDataFrame`.
    /// It intelligently handles optional (`Option<f32>`) and floating-point values
    /// (`f32`) by converting `None`, `NaN` (Not-a-Number), or infinite values
    /// into `NULL` for database insertion. Boolean options are similarly converted
    /// to `NULL` if `None`.
    ///
    /// The data is inserted into the database. If a record with the same timestamp
    /// already exists, the existing record will be updated (this behavior depends
    /// on the underlying SQL query `SQL_QUERY_WRITE_DATA` using an
    /// `ON DUPLICATE KEY UPDATE` clause).
    ///
    /// # Arguments
    /// * `conn` - A mutable reference to the database connection.
    /// * `data` - A reference to the `RecorderDataFrame` containing the data to be written.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) if the data frame was successfully inserted or updated in the database.
    ///
    /// # Errors
    /// Returns `SqlInterfaceError::InsertDataFrameFailure` if the database operation fails.
    /// This can be caused by a lost connection, constraint violations (e.g., foreign keys),
    /// incorrect data types, or a malformed SQL query. The original `mysql::Error` is
    /// included as the source for detailed debugging.
    pub fn write_data_frame_to_database(
        conn: &mut PooledConn,
        data: &RecorderDataFrame,
    ) -> Result<(), SqlInterfaceError> {
        let params = params! {
            "timestamp" => data.timestamp,
            "water_temperature" => data.water_temperature,
            "water_temperature_filtered" => data.water_temperature_filtered,
            "ph_value" => data.ph,
            "ph_value_filtered" => data.ph_filtered,
            "conductivity" => data.conductivity,
            "conductivity_filtered" => data.conductivity_filtered,
            "refill_in_progress" => data.refill_in_progress,
            "tank_level_switch_position" => data.tank_level_switch_position,
            "tank_level_switch_invalid" => data.tank_level_switch_invalid,
            "tank_level_switch_position_stabilized" => data.tank_level_switch_position_stabilized,
            "surface_ventilation_status" => data.surface_ventilation_status,
            "ambient_temperature" => data.ambient_temperature,
            "ambient_humidity" => data.ambient_humidity,
            "heater_status" => data.heater_status,
        };

        // execute the database query and evaluate the result
        conn.exec_drop::<_, _>(sql_query_strings::SQL_QUERY_WRITE_DATA, params)
            .map_err(|e| SqlInterfaceError::InsertDataFrameFailure {
                location: module_path!().to_string(),
                query: sql_query_strings::SQL_QUERY_WRITE_DATA.to_string(),
                source: e,
            })
    }

    #[cfg(test)]
    /// Retrieves all water temperature readings from the last 24 hours.
    ///
    /// This function queries the `data` table for all records within the last
    /// 24 hours that have a non-NULL `water_temperature`. It returns a vector
    /// of tuples, where each tuple contains the Unix timestamp (as `f64`) and
    /// the water temperature (as `f64`).
    ///
    /// # Returns
    /// A `Result` containing a `Vec<(f64, f64, u64, 64)>` on success where the tuple consists of
    /// timestamp, temperature, ventilation status, heater status.
    ///
    /// # Errors
    /// Returns a `SqlInterfaceError::SelectDataFailure` if the database query fails.
    pub fn get_water_temperature_last_24h(
        &mut self,
    ) -> Result<Vec<(f64, f64, u64, u64)>, SqlInterfaceError> {
        self.conn
            .query_map(
                sql_query_strings::SQL_QUERY_GET_WATER_TEMPERATURE_LAST_24H,
                |(timestamp, water_temperature, surface_ventilation_status, heater_status): (
                    i64,
                    f32,
                    u64,
                    u64,
                )| {
                    (
                        timestamp as f64,
                        water_temperature as f64,
                        surface_ventilation_status,
                        heater_status,
                    )
                },
            )
            .map_err(|e| SqlInterfaceError::SelectDataFailure {
                location: module_path!().to_string(),
                query: sql_query_strings::SQL_QUERY_GET_WATER_TEMPERATURE_LAST_24H.to_string(),
                source: e,
            })
    }
}
