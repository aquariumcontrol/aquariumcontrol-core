/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use mysql::PooledConn;
use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_error::SqlInterfaceError;
use crate::database::sql_query_strings;

/// Trait for calculation the time interval until midnight in seconds.
/// This trait allows running the main control with a mock implementation for testing.
pub trait SqlInterfaceMidnightCalculatorTrait {
    fn get_duration_until_midnight(&mut self) -> mysql::Result<i64, SqlInterfaceError>;
}

/// Struct holds connection data to the SQL database and provides implementation for calculating
/// the time in seconds until midnight.
pub struct SqlInterfaceMidnightCalculator {
    /// Connection to the database
    pub conn: PooledConn,
}

impl SqlInterfaceMidnightCalculatorTrait for SqlInterfaceMidnightCalculator {
    /// Queries the database to determine the number of seconds remaining until the next midnight.
    ///
    /// This function executes an SQL query to retrieve a precise count of seconds
    /// from the current database time until 00:00:00 of the upcoming day.
    ///
    /// # Returns
    /// A `Result` which is:
    /// - `Ok(i64)`: The number of seconds remaining until midnight. This value is expected to be non-negative.
    /// - `Err(SqlInterfaceError)`: If there's an issue retrieving the integer from the database,
    ///   such as a query failure, or if the database returns an unexpected number of results.
    fn get_duration_until_midnight(&mut self) -> mysql::Result<i64, SqlInterfaceError> {
        match SqlInterface::get_single_integer_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_DURATION_UNTIL_MIDNIGHT,
        ) {
            Ok(c) => Ok(c),
            Err(e) => Err(e),
        }
    }
}

impl SqlInterfaceMidnightCalculator {
    /// Creates a new `SqlInterfaceHeatingMidnightCalculator` instance.
    ///
    /// This constructor initializes the calculator with an established database connection,
    /// which it uses to perform time-related queries, such as determining time until midnight.
    ///
    /// # Arguments
    /// * `conn` - An active database connection pool object.
    ///
    /// # Returns
    /// A new `SqlInterfaceHeatingMidnightCalculator` struct.
    pub fn new(conn: PooledConn) -> SqlInterfaceMidnightCalculator {
        SqlInterfaceMidnightCalculator { conn }
    }
}
