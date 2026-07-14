/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

// Contains functions for execution of basic queries used by the other modules
pub mod sql_interface;

// Contains functions for execution of all queries relevant for Balling mineral dosing
pub mod sql_interface_balling;

// Contains functions for execution of all queries relevant for time-data recording (recorder)
pub mod sql_interface_data;

// Contains functions for execution of all queries relevant for feed (food)
pub mod sql_interface_feed;

// Contains functions for execution of all queries relevant for heating statistics (thermal)
pub mod sql_interface_heating_stats;

// Contains functions for execution of all queries relevant for refill (water)
pub mod sql_interface_refill;

// Contains functions for execution of all queries relevant for schedule (permission)
pub mod sql_interface_schedule;

// Contains enum with error definitions
pub mod sql_interface_error;

// Contains functions for execution of all queries relevant for calculating the time until midnight
pub mod sql_interface_midnight;

// Contains all SQL queries used in implementation as constants with placeholders
pub mod sql_query_strings;

// Contains functions for execution of all queries relevant for heating set values (thermal)
pub mod sql_interface_heating_set_vals;

// Contains functions for execution of all queries relevant for ventilation set values (thermal)
pub mod sql_interface_ventilation_set_vals;

// Contains data structure for transfer of heating statistics to the database
pub mod sql_interface_heating_stats_data_transfer;

// Configuration of SqlInterface
pub mod sql_interface_config;

// Represents a component that can be "pinged" to keep a connection alive.
pub mod pingable;

// Trait and implementation for database interface of Feed allowing mocking
pub mod database_interface_feed_trait;

// Trait for updating of set values for either heating or ventilation control
pub mod thermal_set_value_updater_trait;

// Contains helper functions for testing
pub mod sql_interface_test_utils;

// Contains all SQL queries used in testing as constants with placeholders
pub mod sql_test_query_strings;
