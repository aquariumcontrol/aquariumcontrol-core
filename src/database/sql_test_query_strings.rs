/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/


//! A centralized repository for all static SQL query strings used throughout the tests of the application.

/// Write heating set values to the database.
pub const SQL_QUERY_WRITE_HEATING_SET_VALS: &str = concat!(
    "INSERT into heating_set_vals(heating_switch_off_temp, heating_switch_on_temp) ",
    "values(:heating_switch_off_temp, :heating_switch_on_temp);"
);

/// Write ventilation set values to the database.
pub const SQL_QUERY_WRITE_VENTILATION_SET_VALS: &str = concat!(
    "INSERT into ventilation_set_vals(ventilation_switch_off_temp, ventilation_switch_on_temp) ",
    "values(:ventilation_switch_off_temp, :ventilation_switch_on_temp);"
);

/// Deletes the content of a specific table.
/// This query is used for testing by multiple modules. Therefore, it resides here and not in the test module.
pub const SQL_QUERY_TRUNCATE_TABLE: &str = "truncate table #table;";

/// Provides feed event from the database - used for testing only.
pub const SQL_QUERY_READ_LAST_FEED_EVENT: &str =
    "SELECT timestamp, feeder_on_time, profile_id from feed_log LIMIT 100;";

/// Provide Balling dosing event from the database - used for testing only.
pub const SQL_QUERY_READ_BALLING_EVENT: &str =
    "SELECT timestamp, pump_id, dosing_volume from balling_log LIMIT 100;";

/// Provides refill event from the database - used for testing only.
pub const SQL_QUERY_READ_REFILL_EVENT: &str =
    "SELECT timestamp, duration, volume, error_code from refill_log ORDER BY Timestamp DESC LIMIT 1;";

/// Checks how many entries there are in the refill table.
pub const SQL_QUERY_CHECK_REFILL_COUNT: &str = "SELECT COUNT(*) FROM refill_log LIMIT 1;";

/// Calculates a timestamp based on current timestamp and offset in seconds.
pub const SQL_QUERY_CURRENT_TIMESTAMP_OFFSET_SECONDS: &str =
    "select date_add(current_timestamp(), interval #offset second) LIMIT 100;";

/// Insert test data into the schedule table;
pub const SQL_QUERY_WRITE_SCHEDULE_TEST_DATA: &str =
    "INSERT INTO schedule VALUES(:schedule_name, :start_time, :stop_time, 1);";

/// Inserts data of one feed schedule entry into the database.
pub const SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY: &str = concat!(
    "INSERT into feed_schedule (timestamp, profile_id, is_weekly, is_daily) ",
    "values(:timestamp, :profile_id, :is_weekly, :is_daily)"
);

/// This query is used for testing: It creates a first feed profile.
pub const SQL_QUERY_WRITE_FEED_PROFILE_TEST: &str = concat!(
    "INSERT INTO feed_profiles VALUES ",
    "(:profile_id,:profile_name,",
    "0,1,0,0,0,0,",
    "1,0,1,0,0,0,",
    "2,0,0,1,0,0,",
    "3,0,0,0,1,0,",
    "4,0,0,0,0,1,",
    "5,1,1,0,0,0,",
    "6,1,0,1,0,0,",
    "7,1,0,0,1,0,",
    "8,1,0,0,0,1,",
    "9,1,1,1,0,0,",
    "10,1,1,0,1,0,",
    "11,1,1,0,0,1,",
    "12,1,1,1,1,0,",
    "13,1,1,1,0,1,",
    "14,1,1,1,1,1,",
    "15,1,1,1,1,0,",
    "16,1,1,1,0,0,",
    "17,1,1,0,0,0,",
    "18,1,0,0,0,0,",
    "19,0,0,0,0,0);"
);

/// This query is used for testing: It creates a second feed profile.
pub const SQL_QUERY_WRITE_FEED_PROFILE_TEST2: &str = concat!(
    "INSERT INTO feed_profiles VALUES ",
    "(:profile_id,'second',",
    "1,1,1,1,1,1,",
    "1,1,1,1,1,1,",
    "2,2,2,2,2,2,",
    "2,2,2,2,2,2,",
    "3,3,3,3,3,3,",
    "3,3,3,3,3,3,",
    "4,4,4,4,4,4,",
    "4,4,4,4,4,4,",
    "5,5,5,5,5,5,",
    "5,5,5,5,5,5,",
    "6,6,6,6,6,6,",
    "6,6,6,6,6,6,",
    "7,7,7,7,7,7,",
    "7,7,7,7,7,7,",
    "8,8,8,8,8,8,",
    "8,8,8,8,8,8,",
    "9,9,9,9,9,9,",
    "9,9,9,9,9,9,",
    "1,1,1,1,1,1,",
    "1,1,1,1,1,1);"
);

/// Query for reading the first part from data for test cases operating on a valid database entry
pub const SQL_QUERY_READ_DATA_FRAME_1: &str = concat!(
    "SELECT timestamp, ",
    "water_temperature, water_temperature_filtered, ",
    "ph_value, ph_value_filtered, ",
    "conductivity, conductivity_filtered ",
    "FROM data WHERE timestamp='#timestamp';"
);

/// Query for reading the second part from data for test cases operating on a valid database entry
pub const SQL_QUERY_READ_DATA_FRAME_2: &str = concat!(
    "SELECT refill_in_progress, ",
    "tank_level_switch_position, tank_level_switch_invalid, tank_level_switch_position_stabilized, ",
    "surface_ventilation_status, ",
    "ambient_temperature, ambient_humidity, heater_status FROM data ",
    "WHERE timestamp = '#timestamp';"
);

/// Query for reading a complete data set for test cases with operating on an invalid database entry
pub const SQL_QUERY_READ_NULL_DATA_FRAME: &str = concat!(
    "SELECT water_temperature, water_temperature_filtered, ",
    "ph_value, ph_value_filtered, ",
    "conductivity, conductivity_filtered ",
    "refill_in_progress, ",
    "tank_level_switch_position, tank_level_switch_invalid, tank_level_switch_position_stabilized, ",
    "surface_ventilation_status, ",
    "ambient_temperature, ambient_humidity, heater_status FROM data ",
    "WHERE timestamp = '#timestamp';"
);

/// Query for writing a set of balling set values for one pump to the database
pub const SQL_QUERY_WRITE_BALLING_SET_VALS: &str = concat!(
    "INSERT into balling_set_vals(pump_id, dosing_speed, dosing_volume, label) ",
    "values(:pump_id, :dosing_speed, :dosing_volume, :label);"
);

/// Query for writing the version information of the application into the version table
pub const SQL_QUERY_WRITE_VERSION_INFORMATION: &str =
    "INSERT into version(major, minor, build, hash) values(:major, :minor, :build, :hash);";

#[cfg(test)]
/// Definition of table name for Data recorder
pub const SQL_TABLE_DATA: &str = "data";

#[cfg(test)]
/// Definition of table name for Balling set values
pub const SQL_TABLE_BALLING_SET_VALS: &str = "balling_set_vals";

#[cfg(test)]
/// Definition of table name for Balling dosing log
pub const SQL_TABLE_BALLING_DOSING_LOG: &str = "balling_log";

#[cfg(test)]
/// Definition of table name for schedule
pub const SQL_TABLE_SCHEDULE: &str = "schedule";

#[cfg(test)]
/// Definition of table name for heating set values
pub const SQL_TABLE_HEATING_SET_VALS: &str = "heating_set_vals";

#[cfg(test)]
/// Definition of table name for heating statistics
pub const SQL_TABLE_HEATING_STATS: &str = "heating_stats";

#[cfg(test)]
/// Definition of table name for ventilation set values
pub const SQL_TABLE_VENTILATION_SET_VALS: &str = "ventilation_set_vals";

/// Definition of table name for feed log
pub const SQL_TABLE_FEED_LOG: &str = "feed_log";

#[cfg(test)]
/// Definition of table name for refill log
pub const SQL_TABLE_REFILL: &str = "refill_log";

// Writes a Balling dosing event to the database using given timestamp
#[cfg(test)]
pub const SQL_QUERY_WRITE_BALLING_EVENT_WITH_TIMESTAMP: &str = concat!(
    "INSERT into balling_log(timestamp, pump_id, dosing_volume) ",
    "values(:timestamp, :pump_id, :dosing_volume);"
);

#[cfg(test)]
/// Inserts a Balling pump configuration to the database.
pub const SQL_QUERY_WRITE_PUMP_CONFIGURATION: &str = concat!(
    "INSERT into balling_set_vals(pump_id, dosing_speed, dosing_volume, label) ",
    "values(:pump_id, :dosing_speed, :dosing_volume, :label);"
);

#[cfg(test)]
/// Provides the feed schedule entries which lie in the future in ascending order
pub const SQL_QUERY_READ_FUTURE_FEED_SCHEDULE_ENTRY: &str = concat!(
    "SELECT timestamp, profile_id, is_daily FROM feed_schedule ",
    "WHERE timestamp > CURRENT_TIMESTAMP() ORDER BY timestamp ASC LIMIT 100;"
);
