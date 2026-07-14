/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::NaiveDateTime;
use mysql::PooledConn;
use crate::database::sql_interface::{SqlInterface, SqlInterfaceError};
use crate::database::sql_test_query_strings;

/// Retrieves a timestamp from the database with a specified future or past offset.
///
/// This function is exclusively for use in test environments. It queries the database
/// for its current timestamp and then adds a given number of seconds to it, effectively
/// simulating a future or past point in time for testing scenarios.
///
/// # Arguments
/// * `conn` - A mutable reference to an active database connection.
/// * `offset_seconds` - The number of seconds to add to the current database timestamp.
///   A positive value shifts the timestamp into the future, while a negative value
///   shifts it into the past.
///
/// # Returns
/// A `Result` containing the calculated `NaiveDateTime` (offset by the given seconds) on success.
///
/// # Errors
/// This function will return an `Err` variant of `SqlInterfaceError` if:
/// - The underlying database query to fetch the timestamp string fails. This can be due to
///   a connection issue, an invalid query, or if the query does not return exactly one row.
///   The original error is wrapped in `SqlInterfaceError::TimestampRequestFailure`.
/// - The timestamp string returned by the database cannot be parsed into a `NaiveDateTime`.
///   This indicates a data format mismatch and returns `SqlInterfaceError::TimestampConversionFailure`.
impl SqlInterface {
    pub fn get_current_timestamp_offset_seconds(
        conn: &mut PooledConn,
        offset_seconds: i32,
    ) -> mysql::Result<NaiveDateTime, SqlInterfaceError> {
        let offset_seconds_string = offset_seconds.to_string();
        let sql_query_with_offset_resolved = str::replace(
            sql_test_query_strings::SQL_QUERY_CURRENT_TIMESTAMP_OFFSET_SECONDS,
            "#offset",
            &offset_seconds_string,
        );
        Self::get_timestamp(conn, &sql_query_with_offset_resolved)
    }
}
