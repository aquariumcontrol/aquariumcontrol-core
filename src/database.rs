/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// Contains functions for execution of basic queries used by the other modules
pub mod sql_interface;

/// Contains functions for execution of all queries relevant for Balling mineral dosing
pub mod sql_interface_balling;

/// Contains functions for execution of all queries relevant for time-data recording (recorder)
pub mod sql_interface_data;

/// Contains functions for execution of all queries relevant for feed (food)
pub mod sql_interface_feed;

/// Contains functions for execution of all queries relevant for heating (thermal)
pub mod sql_interface_heating;

/// Contains functions for execution of all queries relevant for refill (water)
pub mod sql_interface_refill;

/// Contains functions for execution of all queries relevant for schedule (permission)
pub mod sql_interface_schedule;

/// Contains enum with error definitions
pub mod sql_interface_error;

pub mod sql_interface_midnight;
/// Contains all SQL queries as constants with placeholders
pub mod sql_query_strings;
