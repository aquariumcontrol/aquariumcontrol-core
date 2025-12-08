/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
//! Defines the configuration structure for the application's SQL database interface.
//!
//! This module contains the `SqlInterfaceConfig` struct, which is a direct representation
//! of the `[sql_interface]` section in the application's TOML configuration file. It serves
//! as a data container for all settings required to establish, manage, and validate the
//! database connection.
//!
//! ## Key Configuration Areas
//!
//! The struct is organized into several logical groups of parameters:
//!
//! - **Connection Details**: Fields like `db_user`, `db_password`, `db_host`, `db_port`,
//!   and `db_name` provide the essential credentials for connecting to the MySQL server.
//!
//! - **Startup Validation**:
//!   - `db_tables`: A list of table names that the application expects to exist. The
//!     `SqlInterface` will verify their presence at startup.
//!   - `db_min_wait_timeout`: Specifies the minimum required `wait_timeout` on the
//!     MySQL server to prevent premature connection drops. A value of `0` deactivates this check.
//!   - `max_rows_*`: A series of fields (`max_rows_balling_set_values`, `max_rows_data`, etc.)
//!     that define the maximum acceptable number of rows for various log and configuration
//!     tables. This acts as a safeguard against uncontrolled table growth. A value of `0` deactivates the check for a specific table.
//!
//! - **Operational Parameters**:
//!   - `db_ping_interval`: The interval, in seconds, at which the application should
//!     ping the database to keep the connection alive and prevent server-side timeouts.
//!
//! ## Usage
//!
//! An instance of `SqlInterfaceConfig` is created by deserializing the
//! `[sql_interface]` section of a `.toml` file. This instance is then passed to
//! `SqlInterface::new()` to initialize the main database connection pool for the
//! entire application.

use serde_derive::Deserialize;

/// This struct holds the configuration data for the SQL interface.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Clone)]
pub struct SqlInterfaceConfig {
    /// database user
    pub db_user: String,

    /// database password
    pub db_password: String,

    /// hostname of the database
    pub db_host: String,

    /// port name of the database
    pub db_port: u16,

    /// database name
    pub db_name: String,

    /// array of required tables
    pub db_tables: Vec<String>,

    /// minimum wait timout - application checks server variable before execution
    pub db_min_wait_timeout: u64,

    /// the database-ping interval in seconds
    pub db_ping_interval: u64,

    /// maximum allowed rows for balling set value table
    pub max_rows_balling_set_values: u64,

    /// maximum allowed rows for feed pattern table
    pub max_rows_feed_profile: u64,

    /// maximum allowed rows for feed schedule table
    pub max_rows_feed_schedule: u64,

    /// maximum allowed rows for heating stats table
    pub max_rows_heating_stats: u64,

    /// maximum allowed rows for schedule table
    pub max_rows_schedule: u64,

    /// maximum allowed rows for data table
    pub max_rows_data: u64,

    /// maximum allowed rows for refill log
    pub max_rows_refill: u64,

    /// maximum allowed rows for Balling dosing log
    pub max_rows_balling_dosing_log: u64,

    /// maximum allowed rows for Feed log
    pub max_rows_feed_log: u64,
}
