/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines a generic trait for updating thermal set points from a data source.
//!
//! This module provides the `ThermalSetValueUpdaterTrait`, an abstraction that decouples
//! the core thermal control logic (e.g., for heating or ventilation) from the specific
//! implementation of how set point values are retrieved.
//!
//! ## Key Components
//!
//! - **`ThermalSetValueUpdaterTrait`**: A trait that defines a single method, `update_set_value`.
//!   Any struct implementing this trait is responsible for fetching the latest "on" and "off"
//!   temperature thresholds and updating the provided variables.
//!
//! ## Design and Purpose
//!
//! The primary purpose of this trait is to enable **Dependency Inversion** and improve
//! the system's **testability** and **modularity**.
//!
//! - **Decoupling**: High-level components, like a `ThermalController`, do not need to know
//!   *how* or *from where* the set points are fetched. They only need to interact with this
//!   trait. This means the data could come from a SQL database, a configuration file, or
//!   even a remote API without changing the controller's code.
//!
//! - **Testability**: This abstraction is critical for unit testing. In a test environment,
//!   a mock implementation of `ThermalSetValueUpdaterTrait` can be created. This mock can
//!   simulate various scenarios—such as providing fixed values, returning updated values,
//!   or simulating database errors—all without requiring a live database connection. This
//!   makes tests fast, reliable, and self-contained.

use crate::database::sql_interface_error::SqlInterfaceError;

/// Trait for updating the set values of either ventilation or heating with values from the database.
/// This trait allows running the main control with a mock implementation for testing.
///
/// # Arguments
/// * `switch_off_temperature` - A parameter representing `switch_off_temperature`.
/// * `switch_on_temperature` - A parameter representing `switch_on_temperature`.
pub trait ThermalSetValueUpdaterTrait {
    fn update_set_value(
        &mut self,
        switch_off_temperature: &mut f32,
        switch_on_temperature: &mut f32,
    ) -> Result<(), SqlInterfaceError>;
}
