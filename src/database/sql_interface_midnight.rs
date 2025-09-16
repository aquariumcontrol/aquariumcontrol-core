/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Provides a trait and implementation for calculating the time until midnight using the database clock.
//!
//! This module defines an abstraction (`SqlInterfaceMidnightCalculatorTrait`) and a concrete
//! implementation (`SqlInterfaceMidnightCalculator`) for determining the number of seconds
//! remaining until the next midnight. The primary purpose of this module is to decouple
//! time-sensitive components (like daily statistics aggregators) from the specific details
//! of the database, adhering to the Dependency Inversion Principle.
//!
//! ## Key Components
//!
//! - **`SqlInterfaceMidnightCalculatorTrait`**: An abstraction that defines the contract for
//!   any component capable of calculating the duration until midnight. This allows for
//!   dependency injection, making it possible to use a mock implementation for testing.
//!
//! - **`SqlInterfaceMidnightCalculator`**: The concrete implementation of the trait. It holds
//!   a database connection and executes an SQL query to get a precise time from the
//!   database server's clock.
//!
//! ## Design and Purpose
//!
//! By relying on the database server for the current time, the application ensures that
//! all time-based calculations are consistent, even if the application is running on a
//! machine with a desynchronized clock.
//!
//! The use of a trait is a crucial architectural choice. It allows higher-level components,
//! such as `SqlInterfaceHeatingStats`, to depend on the `SqlInterfaceMidnightCalculatorTrait`
//! abstraction rather than the concrete `SqlInterfaceMidnightCalculator` struct. This
//! makes the system more modular and significantly easier to test, as a mock calculator
//! can be provided during unit tests without needing a live database.

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_error::SqlInterfaceError;
use crate::database::sql_query_strings;
use mysql::PooledConn;

/// Trait for calculation the time interval until midnight in seconds.
/// This trait allows running the main control with a mock implementation for testing.
pub trait SqlInterfaceMidnightCalculatorTrait {
    fn get_duration_until_midnight(&mut self) -> Result<i64, SqlInterfaceError>;
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
    /// This function executes a standard SQL query to retrieve a precise count of seconds
    /// from the current database time until 00:00:00 of the upcoming day.
    ///
    /// # Returns
    /// A `Result` containing the number of seconds (as an `i64`) remaining until midnight on success.
    ///
    /// # Errors
    /// This function will return an `Err` variant of `SqlInterfaceError` if the underlying
    /// call to `get_single_integer_from_database` fails. This can happen if:
    /// - The database connection is lost or invalid.
    /// - The SQL query fails to execute for other reasons (e.g., syntax error, permissions).
    /// - The query does not return exactly one row and one column containing an integer.
    fn get_duration_until_midnight(&mut self) -> Result<i64, SqlInterfaceError> {
        SqlInterface::get_single_integer_from_database(
            &mut self.conn,
            sql_query_strings::SQL_QUERY_READ_DURATION_UNTIL_MIDNIGHT,
        )
    }
}

impl SqlInterfaceMidnightCalculator {
    /// Creates a new `SqlInterfaceMidnightCalculator` instance.
    ///
    /// This constructor initializes the calculator with an established database connection,
    /// which it uses to perform time-related queries, such as determining time until midnight.
    ///
    /// # Arguments
    /// * `conn` - An active database connection pool object.
    ///
    /// # Returns
    /// A new `SqlInterfaceMidnightCalculator` struct.
    pub fn new(conn: PooledConn) -> SqlInterfaceMidnightCalculator {
        SqlInterfaceMidnightCalculator { conn }
    }
}
