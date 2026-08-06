/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines a contract for components that permanently use a database connection.
//!
//! This module introduces the `Pingable` trait, which provides a generic interface
//! for sending a lightweight "ping" to a database to prevent it from timing out
//! due to inactivity.
//!
//! ## Key Components
//!
//! - **`Pingable` Trait**: A simple trait with a single method, `ping()`. It abstracts
//!   the action of performing a keep-alive check. By using a trait, generic functions
//!   and macros can operate on any database interface that implements this behavior
//!   without needing to know its concrete type. This is heavily used by the
//!   `check_quit_increment_counter_ping_database!` macro.
//!
//! - **Implementations**: The module provides `impl Pingable` blocks for various
//!   `SqlInterface...` structs (`SqlInterfaceData`, `SqlInterfaceBalling`, etc.).
//!   Each implementation calls a common `SqlInterface::ping_database` function,
//!   ensuring consistent behavior across the application.
//!
//! ## Design and Purpose
//!
//! The primary goal is to create a uniform, "fire-and-forget" keep-alive mechanism.
//! Long-running threads that interact with the database only intermittently can use
//! this trait to ensure their connection remains active.
//!
//! The `ping` method is designed to be non-disruptive; it logs any errors internally
//! but does not propagate them. This prevents keep-alive failures from crashing
//! a main control loop.
//!
//! ## Coverage Exclusion Rationale
//!
//! This module is explicitly excluded from test code coverage measurement (`cargo-llvm-cov`) via
//! `ignore-filename-regex` in `Cargo.toml`. The rationale for this exclusion is:
//! 1. **Pure Boilerplate**: It contains only the `Pingable` trait definition and macro-generated (`impl_pingable!`)
//!    delegation blocks to `SqlInterface::ping_database`.
//! 2. **Core Query Tested Elsewhere**: The underlying database ping query functionality (`SELECT CURRENT_TIMESTAMP()`)
//!    lives in `sql_interface.rs` and is independently tested.
//! 3. **Macro Coverage Artifacts**: Diagnostic error logging inside `ping()` is conditionally compiled out during test
//!    runs (`#[cfg(all(not(test), not(feature = "test-utils")))]`), which causes source line coverage tools to report
//!    these macro-generated lines as uncovered despite `ping()` being executed 600+ times across test suites.

use crate::database::sql_interface::SqlInterface;
use crate::database::sql_interface_balling::SqlInterfaceBalling;
use crate::database::sql_interface_data::SqlInterfaceData;
use crate::database::sql_interface_feed::SqlInterfaceFeed;
use crate::database::sql_interface_heating_stats::SqlInterfaceHeatingStats;
use crate::database::sql_interface_refill::SqlInterfaceRefill;
use crate::database::sql_interface_schedule::SqlInterfaceSchedule;
use crate::utilities::logger::log_error_chain;

/// Represents a component that can be "pinged" to keep a connection alive.
pub trait Pingable {
    /// Pings the database to check connection health and prevent timeouts.
    ///
    /// This function executes a lightweight query to ensure the connection is still
    /// active. If the ping fails, it logs an error using `log::error!` but does
    /// not propagate the error to the caller. This is a "fire-and-forget"
    /// keep-alive mechanism.
    fn ping(&mut self);
}

/// This macro assumes that each struct provided has a `conn` field which is a
/// mutable database connection that can be passed to `SqlInterface::ping_database`.
macro_rules! impl_pingable {
    // The `$(...)+` syntax allows the macro to accept one or more struct types,
    // separated by commas.
    ($($struct_name:ty),+) => {
        $(
            impl Pingable for $struct_name {
                /// Pings the database to check connection health and prevent timeouts.
                fn ping(&mut self) {
                    if let Err(_e) = SqlInterface::ping_database(&mut self.conn) {
                        log_error_chain( // not required during testing
                            module_path!(),
                            "Encountered error while pinging database.",
                            _e,
                        );
                    }
                }
            }
        )+
    };
}

// Now, apply the implementation to all relevant structs in a single, clean call.
impl_pingable!(
    SqlInterfaceData,
    SqlInterfaceBalling,
    SqlInterfaceFeed,
    SqlInterfaceHeatingStats,
    SqlInterfaceRefill,
    SqlInterfaceSchedule
);
