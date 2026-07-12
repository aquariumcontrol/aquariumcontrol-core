/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Provides the mechanism for injecting collected data frames into the database.
//!
//! This module defines the `DataInjectionTrait` and its concrete implementation,
//! `DataInjection`. Its sole responsibility is to act as the final link in the
//! data logging pipeline, taking a complete `RecorderDataFrame` and passing it to
//! the `SqlInterfaceData` for persistence.
//!
//! ## Key Components
//!
//! - **`DataInjectionTrait`**: An abstraction for the data injection process. This is a
//!   critical design element that decouples the main `DataLogger` from the concrete
//!   database implementation, enabling the use of mock injectors for unit testing.
//!
//! - **`DataInjection` Struct**: The primary, stateless implementation of the trait. Its
//!   `inject_data` method simply calls the database interface and logs any errors
//!   that occur.
//!
//! ## Design and Architecture
//!
//! The module is designed for testability and clear separation of concerns.
//!
//! - **Decoupling via Trait**: The `DataLogger` module depends on the `DataInjectionTrait`,
//!   not the concrete `DataInjection` struct.
//!
//! - **Testability**: Because of the trait-based design, the `DataLogger` can be tested
//!   without needing a real database connection. A mock implementation of the
//!   `DataInjectionTrait` is used to verify that `inject_data` was called
//!   with the expected data frame, as demonstrated in the `DataLogger`'s tests.
//!
//! - **Simplicity**: The module has a single, clear responsibility: take a frame of
//!   data and hand it off to the database interface.

use crate::database::sql_interface_data::SqlInterfaceData;
use crate::recorder::recorder_data_frame::RecorderDataFrame;
use crate::utilities::logger::log_error_chain;
use mysql::PooledConn;

/// Trait for the execution of data logging to SQL database
/// This trait allows running the main control with a mock implementation for testing.
pub trait DataInjectionTrait {
    /// Transfers the data provided as parameter to SQL database
    /// # Arguments
    /// * `data_frame` - Data to be transferred to SQL database
    /// * `conn` - Database connection
    fn inject_data(&mut self, data_frame: RecorderDataFrame, conn: PooledConn) -> PooledConn;
}

/// Implements the `DataInjectionTrait` for concrete data logging to an SQL database.
///
/// This struct serves as the practical component for transferring `RecorderDataFrame`
/// instances to the configured SQL database.
pub struct DataInjection;

impl DataInjectionTrait for DataInjection {
    /// Transfers a `RecorderDataFrame` to the SQL database for logging.
    ///
    /// This method serves as the concrete implementation for injecting collected
    /// data frames into the database. It attempts to write the provided `data_frame`
    /// using the `sql_interface_data` and logs any errors that occur during the process.
    ///
    /// # Arguments
    /// * `data_frame` - The `RecorderDataFrame` containing the data to be persisted in the SQL database.
    ///   This is passed by value, meaning ownership is transferred to this function.
    /// * `sql_interface_data` - A mutable reference to the `SqlInterfaceData` instance,
    ///   which handles the actual database write operation.
    /// * `conn` - An active pooled database connection.
    fn inject_data(&mut self, data_frame: RecorderDataFrame, mut conn: PooledConn) -> PooledConn {
        match SqlInterfaceData::write_data_frame_to_database(&mut conn, &data_frame) {
            Ok(()) => { /* do nothing */ }
            Err(e) => {
                log_error_chain(
                    module_path!(),
                    "Error occurred when injecting data into database.",
                    e,
                );
            }
        };
        conn
    }
}
