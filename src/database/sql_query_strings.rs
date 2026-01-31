/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A centralized repository for all static SQL query strings used throughout the application.
//!
//! This module acts as a single source of truth for database queries, decoupling the raw SQL
//! from the application's business logic. This design offers several key advantages:
//!
//! - **Centralization**: All queries are in one place, making them easy to find, review,
//!   and manage.
//! - **Maintainability**: If a table or column name changes, it only needs to be updated
//!   here, rather than in multiple places across the codebase.
//! - **Security Auditing**: Having all queries in one file simplifies the process of
//!   auditing for potential security vulnerabilities like SQL injection.
//! - **Decoupling**: Rust code that interacts with the database can focus on logic
//!   without being cluttered with long, embedded SQL strings.
//!
//! ## Usage
//!
//! Queries are defined as `pub const &str`. They use named parameters (e.g., `:my_param`)
//! which are compatible with the `mysql` crate's `params!` macro for safe data binding.

/// Provides version information to check if the current application is listed in the table of supported versions.
/// It informs whether the version is supported and which hash is stored in the database for the version.
pub const SQL_QUERY_VERSION_INFORMATION: &str =
    "SELECT hash FROM version WHERE major=:major AND minor=:minor AND build=:build LIMIT 100;";

/// Provides the current database name.
pub const SQL_QUERY_DATABASE: &str = "select database() LIMIT 1;";

/// Provides all tables from the database. This query cannot be limited.
pub const SQL_QUERY_READ_TABLES: &str = "SHOW TABLES";

/// Provides the current date (without time) from the database.
pub const SQL_QUERY_READ_CURRENT_DATE: &str = "SELECT CURRENT_DATE() LIMIT 1;";

/// Provides tomorrow's date (without time) from the database.
pub const SQL_QUERY_READ_TOMORROW_DATE: &str = "SELECT CURRENT_DATE() + INTERVAL 1 DAY LIMIT 1;";

/// Provides the current timestamp from the database.
pub const SQL_QUERY_READ_CURRENT_TIMESTAMP: &str = "SELECT CURRENT_TIMESTAMP() LIMIT 1;";

/// Inserts data of one feed event into the database.
pub const SQL_QUERY_WRITE_FEED_EVENT: &str = concat!(
    "INSERT into feed_log (timestamp, feeder_on_time, profile_id) ",
    "values(:timestamp, :feeder_run_time, :profile_id)"
);

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

/// Definition of table name for feed profiles
pub const SQL_TABLE_FEED_PROFILES: &str = "feed_profiles";

/// Definition of table name for feed schedule
pub const SQL_TABLE_FEED_SCHEDULE: &str = "feed_schedule";

/// Provides the header of a feed profile from the database.
pub const SQL_QUERY_READ_FEED_PROFILE_HEADER: &str =
    "SELECT profile_id, profile_name FROM feed_profiles WHERE profile_id=:profile_id LIMIT 100;";

/// Provides one phase of a feed profile from the database.
pub const SQL_QUERY_READ_FEED_PHASE: &str = concat!(
    "SELECT pause_#_duration, pause_#_skimmer, ",
    "pause_#_main_pump1, pause_#_main_pump2, pause_#_aux_pump1, pause_#_aux_pump2, ",
    "feed_#_duration, ",
    "feed_#_skimmer, feed_#_main_pump1, feed_#_main_pump2, feed_#_aux_pump1, feed_#_aux_pump2 ",
    "FROM feed_profiles WHERE profile_id=:profile_id LIMIT 100;"
);

/// Provides the timestamp of the last refill event from the database.
pub const SQL_QUERY_READ_LAST_REFILL_TIMESTAMP: &str =
    "SELECT timestamp from refill_log as LSTTSTMP ORDER BY timestamp DESC LIMIT 1;";

/// Checks if the refill table is empty.
pub const SQL_QUERY_CHECK_REFILL_EMPTY: &str = "SELECT EXISTS(SELECT 1 FROM refill_log) LIMIT 1;";

/// Provides the timestamp of the last Balling dosing event from the database.
pub const SQL_QUERY_READ_LAST_BALLING_DOSING_TIMESTAMP: &str = concat!(
    "SELECT timestamp from balling_log as LSTTSTMP ",
    "WHERE pump_id=#pump_id ORDER BY timestamp DESC LIMIT 1;"
);

/// Writes the last Balling dosing event to the database.    
pub const SQL_QUERY_WRITE_BALLING_EVENT: &str = concat!(
    "INSERT into balling_log(timestamp, pump_id, dosing_volume) ",
    "values(:timestamp, :pump_id, :dosing_volume);"
);

#[cfg(test)]
/// Inserts a Balling pump configuration to the database.    
pub const SQL_QUERY_WRITE_PUMP_CONFIGURATION: &str = concat!(
    "INSERT into balling_set_vals(pump_id, dosing_speed, dosing_volume, label) ",
    "values(:pump_id, :dosing_speed, :dosing_volume, :label);"
);

/// Provides the Balling dosing pump configuration for one of the pumps.
pub const SQL_QUERY_READ_BALLING_SET_VALS: &str = concat!(
    "SELECT pump_id, dosing_speed, dosing_volume, label FROM balling_set_vals ",
    "WHERE pump_id=:pump_id LIMIT 100;"
);

/// Provides the refill volume of the last hour from the database.
/// It needs to use the COALESCE function because a null result is not interpreted correctly by the
/// SQL library.
pub const SQL_QUERY_READ_REFILL_VOLUME_LAST_HOUR: &str = concat!(
    "SELECT COALESCE(SUM(volume), 0) from refill_log ",
    "WHERE timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 HOUR LIMIT 1;"
);

/// Provides the number of refill events from the last hour from the database.
/// It does not need to use the COALESCE function because the query returns 0 if there is no refill
/// event in the last 24h.
pub const SQL_QUERY_READ_REFILL_COUNT_LAST_HOUR: &str = concat!(
    "SELECT COUNT(volume) from refill_log ",
    "WHERE timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 HOUR LIMIT 1;"
);

/// Provides the refill volume of the last day from the database.
/// It needs to use the COALESCE function because a null result is not interpreted correctly by the SQL library.
pub const SQL_QUERY_READ_REFILL_VOLUME_LAST_24H: &str = concat!(
    "SELECT COALESCE(SUM(Volume), 0) from refill_log ",
    "WHERE timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 DAY LIMIT 1;"
);

/// Provides the number of refill events from the last day from the database.
pub const SQL_QUERY_READ_REFILL_COUNT_LAST_24H: &str = concat!(
    "SELECT COUNT(volume) from refill_log ",
    "WHERE timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 DAY LIMIT 1;"
);

/// Writes the last refill event to the database.    
pub const SQL_QUERY_WRITE_REFILL_EVENT: &str = concat!(
    "INSERT into refill_log(timestamp, duration, volume, error_code) ",
    "values(:timestamp, :duration, :volume, :error_code);"
);

/// Provides the duration in seconds until midnight from the database.
pub const SQL_QUERY_READ_DURATION_UNTIL_MIDNIGHT: &str =
    "SELECT TIMESTAMPDIFF(SECOND,NOW(),DATE(NOW()) + INTERVAL 1 DAY) LIMIT 1;";

/// Provides the heating statistical data of the current day from the database.
pub const SQL_QUERY_READ_HEATING_STATS: &str =
    "SELECT * FROM heating_stats WHERE date=CURRENT_DATE() LIMIT 1;";

/// Provides the feed schedule entries which lie in the past in descending order
pub const SQL_QUERY_READ_PAST_FEED_SCHEDULE_ENTRY: &str = concat!(
    "SELECT timestamp, profile_id, is_daily FROM feed_schedule ",
    "WHERE timestamp < CURRENT_TIMESTAMP() ORDER BY timestamp DESC LIMIT 100;"
);

#[cfg(test)]
/// Provides the feed schedule entries which lie in the future in ascending order
pub const SQL_QUERY_READ_FUTURE_FEED_SCHEDULE_ENTRY: &str = concat!(
    "SELECT timestamp, profile_id, is_daily FROM feed_schedule ",
    "WHERE timestamp > CURRENT_TIMESTAMP() ORDER BY timestamp ASC LIMIT 100;"
);

/// Updates a feed schedule entry after having executing a feed profile
/// which shall be repeated daily.
pub const SQL_QUERY_UPDATE_FEED_SCHEDULE_ENTRY: &str =
    "UPDATE feed_schedule SET timestamp=:timestamp_new WHERE timestamp=:timestamp_old;";

/// Updates a feed schedule entry after having executing a feed profile
/// which shall not be repeated daily.
pub const SQL_QUERY_DELETE_FEED_SCHEDULE_ENTRY: &str =
    "DELETE FROM feed_schedule WHERE timestamp = :timestamp;";

/// Writes the heating statistical data into the database.
/// If there is already data for the given date, the data will not be inserted. Instead, the
/// existing entry will be updated.
pub const SQL_QUERY_WRITE_HEATING_STATS: &str = concat!(
    "INSERT INTO heating_stats (date, energy, ambient_temp_average, ambient_temp_counter, ",
    "water_temp_average, water_temp_counter, heating_control_runtime) ",
    "VALUES (:date, :energy, :ambient_temperature_average, :ambient_temperature_counter, ",
    ":water_temperature_average, :water_temperature_counter, :heating_control_runtime) ",
    "ON DUPLICATE KEY UPDATE ",
    "energy = VALUES(energy), ",
    "ambient_temp_average = VALUES(ambient_temp_average), ",
    "ambient_temp_counter = VALUES(ambient_temp_counter), ",
    "water_temp_average = VALUES(water_temp_average), ",
    "water_temp_counter = VALUES(water_temp_counter), ",
    "heating_control_runtime = VALUES(heating_control_runtime);"
);

/// Writes the time-based data into the database.
pub const SQL_QUERY_WRITE_DATA: &str = concat!(
    "INSERT INTO data (",
    "timestamp, ",
    "water_temperature, water_temperature_filtered, ",
    "ph_value, ph_value_filtered, ",
    "conductivity, conductivity_filtered, ",
    "refill_in_progress, ",
    "tank_level_switch_position, tank_level_switch_invalid, tank_level_switch_position_stabilized, ",
    "surface_ventilation_status, ambient_temperature, ambient_humidity, heater_status",
    ") VALUES (",
    ":timestamp, :water_temperature, :water_temperature_filtered, :ph_value, :ph_value_filtered, ",
    ":conductivity, :conductivity_filtered, :refill_in_progress, ",
    ":tank_level_switch_position, :tank_level_switch_invalid, :tank_level_switch_position_stabilized, ",
    ":surface_ventilation_status, :ambient_temperature, :ambient_humidity, :heater_status",
    ") ON DUPLICATE KEY UPDATE ",
    "water_temperature=VALUES(water_temperature), ",
    "water_temperature_filtered=VALUES(water_temperature_filtered), ",
    "ph_value=VALUES(ph_value), ph_value_filtered=VALUES(ph_value_filtered), ",
    "conductivity=VALUES(conductivity), ",
    "conductivity_filtered=VALUES(conductivity_filtered), ",
    "refill_in_progress=VALUES(refill_in_progress), ",
    "tank_level_switch_position=VALUES(tank_level_switch_position), ",
    "tank_level_switch_invalid=VALUES(tank_level_switch_invalid), ",
    "tank_level_switch_position_stabilized=VALUES(tank_level_switch_position_stabilized), ",
    "surface_ventilation_status=VALUES(surface_ventilation_status), ",
    "ambient_temperature=VALUES(ambient_temperature), ambient_humidity=VALUES(ambient_humidity), ",
    "heater_status=VALUES(heater_status);"
);

pub const SQL_QUERY_GET_WATER_TEMPERATURE_LAST_24H: &str = concat!(
    "SELECT UNIX_TIMESTAMP(timestamp), water_temperature FROM data ",
    "WHERE timestamp >= NOW() - INTERVAL 1 DAY AND water_temperature IS NOT NULL ",
    "ORDER BY timestamp ASC"
);

/// Checks how many entries there are in the data table.
pub const SQL_QUERY_CHECK_DATA_COUNT: &str = "SELECT COUNT(*) FROM data LIMIT 1;";

/// Provides the schedule entry for one of the controls (Refill, Ventilation, Heating, Balling)
pub const SQL_QUERY_READ_SCHEDULE: &str = "SELECT * FROM schedule LIMIT 100;";

/// Checks if the balling set value table contains any NULL values.
pub const SQL_QUERY_CHECK_BALLING_SET_VALS_NULL: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM balling_set_vals ",
    "WHERE dosing_speed IS NULL OR dosing_volume IS NULL OR label IS NULL OR pump_id IS NULL) LIMIT 100;"
);

/// Checks how many entries are in the balling set value table
pub const SQL_QUERY_CHECK_BALLING_SET_VALS_COUNT: &str =
    "SELECT COUNT(*) FROM balling_set_vals LIMIT 1;";

/// Checks how many entries are in the balling dosing log
pub const SQL_QUERY_CHECK_BALLING_LOG_COUNT: &str = "SELECT COUNT(*) FROM balling_log LIMIT 1;";

/// Checks if the feed profiles table contains any NULL values.
pub const SQL_QUERY_CHECK_FEED_PROFILES_NULL: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM feed_profiles ",
    "WHERE profile_id IS NULL OR profile_name IS NULL OR ",
    "pause_01_duration IS NULL OR ",
    "pause_01_skimmer IS NULL OR ",
    "pause_01_main_pump1 IS NULL OR ",
    "pause_01_main_pump2 IS NULL OR ",
    "pause_01_aux_pump1 IS NULL OR ",
    "pause_01_aux_pump2 IS NULL OR ",
    "feed_01_duration IS NULL OR ",
    "feed_01_skimmer IS NULL OR ",
    "feed_01_main_pump1 IS NULL OR ",
    "feed_01_main_pump2 IS NULL OR ",
    "feed_01_aux_pump1 IS NULL OR ",
    "feed_01_aux_pump2 IS NULL OR ",
    "pause_02_duration IS NULL OR ",
    "pause_02_skimmer IS NULL OR ",
    "pause_02_main_pump1 IS NULL OR ",
    "pause_02_main_pump2 IS NULL OR ",
    "pause_02_aux_pump1 IS NULL OR ",
    "pause_02_aux_pump2 IS NULL OR ",
    "feed_02_duration IS NULL OR ",
    "feed_02_skimmer IS NULL OR ",
    "feed_02_main_pump1 IS NULL OR ",
    "feed_02_main_pump2 IS NULL OR ",
    "feed_02_aux_pump1 IS NULL OR ",
    "feed_02_aux_pump2 IS NULL OR ",
    "pause_03_duration IS NULL OR ",
    "pause_03_skimmer IS NULL OR ",
    "pause_03_main_pump1 IS NULL OR ",
    "pause_03_main_pump2 IS NULL OR ",
    "pause_03_aux_pump1 IS NULL OR ",
    "pause_03_aux_pump2 IS NULL OR ",
    "feed_03_duration IS NULL OR ",
    "feed_03_skimmer IS NULL OR ",
    "feed_03_main_pump1 IS NULL OR ",
    "feed_03_main_pump2 IS NULL OR ",
    "feed_03_aux_pump1 IS NULL OR ",
    "feed_03_aux_pump2 IS NULL OR ",
    "pause_04_duration IS NULL OR ",
    "pause_04_skimmer IS NULL OR ",
    "pause_04_main_pump1 IS NULL OR ",
    "pause_04_main_pump2 IS NULL OR ",
    "pause_04_aux_pump1 IS NULL OR ",
    "pause_04_aux_pump2 IS NULL OR ",
    "feed_04_duration IS NULL OR ",
    "feed_04_skimmer IS NULL OR ",
    "feed_04_main_pump1 IS NULL OR ",
    "feed_04_main_pump2 IS NULL OR ",
    "feed_04_aux_pump1 IS NULL OR ",
    "feed_04_aux_pump2 IS NULL OR ",
    "pause_05_duration IS NULL OR ",
    "pause_05_skimmer IS NULL OR ",
    "pause_05_main_pump1 IS NULL OR ",
    "pause_05_main_pump2 IS NULL OR ",
    "pause_05_aux_pump1 IS NULL OR ",
    "pause_05_aux_pump2 IS NULL OR ",
    "feed_05_duration IS NULL OR ",
    "feed_05_skimmer IS NULL OR ",
    "feed_05_main_pump1 IS NULL OR ",
    "feed_05_main_pump2 IS NULL OR ",
    "feed_05_aux_pump1 IS NULL OR ",
    "feed_05_aux_pump2 IS NULL OR ",
    "pause_06_duration IS NULL OR ",
    "pause_06_skimmer IS NULL OR ",
    "pause_06_main_pump1 IS NULL OR ",
    "pause_06_aux_pump1 IS NULL OR ",
    "pause_06_main_pump2 IS NULL OR ",
    "pause_06_aux_pump2 IS NULL OR ",
    "feed_06_duration IS NULL OR ",
    "feed_06_skimmer IS NULL OR ",
    "feed_06_main_pump1 IS NULL OR ",
    "feed_06_main_pump2 IS NULL OR ",
    "feed_06_aux_pump1 IS NULL OR ",
    "feed_06_aux_pump2 IS NULL OR ",
    "pause_07_duration IS NULL OR ",
    "pause_07_skimmer IS NULL OR ",
    "pause_07_main_pump1 IS NULL OR ",
    "pause_07_main_pump2 IS NULL OR ",
    "pause_07_aux_pump1 IS NULL OR ",
    "pause_07_aux_pump2 IS NULL OR ",
    "feed_07_duration IS NULL OR ",
    "feed_07_skimmer IS NULL OR ",
    "feed_07_main_pump1 IS NULL OR ",
    "feed_07_main_pump2 IS NULL OR ",
    "feed_07_aux_pump1 IS NULL OR ",
    "feed_07_aux_pump2 IS NULL OR ",
    "pause_08_duration IS NULL OR ",
    "pause_08_skimmer IS NULL OR ",
    "pause_08_main_pump1 IS NULL OR ",
    "pause_08_main_pump2 IS NULL OR ",
    "pause_08_aux_pump1 IS NULL OR ",
    "pause_08_aux_pump2 IS NULL OR ",
    "feed_08_duration IS NULL OR ",
    "feed_08_skimmer IS NULL OR ",
    "feed_08_main_pump1 IS NULL OR ",
    "feed_08_main_pump2 IS NULL OR ",
    "feed_08_aux_pump1 IS NULL OR ",
    "feed_08_aux_pump2 IS NULL OR ",
    "pause_09_duration IS NULL OR ",
    "pause_09_skimmer IS NULL OR ",
    "pause_09_main_pump1 IS NULL OR ",
    "pause_09_main_pump2 IS NULL OR ",
    "pause_09_aux_pump1 IS NULL OR ",
    "pause_09_aux_pump2 IS NULL OR ",
    "feed_09_duration IS NULL OR ",
    "feed_09_skimmer IS NULL OR ",
    "feed_09_main_pump1 IS NULL OR ",
    "feed_09_main_pump2 IS NULL OR ",
    "feed_09_aux_pump1 IS NULL OR ",
    "feed_09_aux_pump2 IS NULL OR ",
    "pause_10_duration IS NULL OR ",
    "pause_10_skimmer IS NULL OR ",
    "pause_10_main_pump1 IS NULL OR ",
    "pause_10_main_pump2 IS NULL OR ",
    "pause_10_aux_pump1 IS NULL OR ",
    "pause_10_aux_pump2 IS NULL OR ",
    "feed_10_duration IS NULL OR ",
    "feed_10_skimmer IS NULL OR ",
    "feed_10_main_pump1 IS NULL OR ",
    "feed_10_main_pump2 IS NULL OR ",
    "feed_10_aux_pump1 IS NULL OR ",
    "feed_10_aux_pump2 IS NULL) LIMIT 100;"
);

/// Checks how many entries there are in the feed profiles table.
pub const SQL_QUERY_CHECK_FEED_PROFILES_COUNT: &str = "SELECT COUNT(*) FROM feed_profiles LIMIT 1;";

/// Checks how many entries there are in the feed log table.
pub const SQL_QUERY_CHECK_FEED_LOG_COUNT: &str = "SELECT COUNT(*) FROM feed_log LIMIT 1;";

/// Checks if the feed schedule table contains any NULL values.
pub const SQL_QUERY_CHECK_FEED_SCHEDULE_NULL: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM feed_schedule ",
    "WHERE timestamp IS NULL OR ",
    "profile_id IS NULL OR ",
    "is_weekly IS NULL OR ",
    "is_daily IS NULL) LIMIT 100;"
);

/// Checks how many entries there are in the feed schedule table.
pub const SQL_QUERY_CHECK_FEED_SCHEDULE_COUNT: &str = "SELECT COUNT(*) FROM feed_schedule LIMIT 1;";

/// Checks if the heating set value table contains any NULL values.
pub const SQL_QUERY_CHECK_HEATING_SET_VALS_NULL: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM heating_set_vals ",
    "WHERE heating_switch_off_temp IS NULL OR heating_switch_on_temp IS NULL) LIMIT 100;"
);

/// Checks how many entries there are in the heating set value table.
pub const SQL_QUERY_CHECK_HEATING_SET_VALS_COUNT: &str =
    "SELECT COUNT(*) FROM heating_set_vals LIMIT 1;";

/// Checks if the heating stats table contains any NULL values.
pub const SQL_QUERY_CHECK_HEATING_STATS_NULL: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM heating_stats ",
    "WHERE date IS NULL OR ",
    "energy IS NULL OR ",
    "ambient_temp_average IS NULL OR ",
    "ambient_temp_counter IS NULL OR ",
    "water_temp_average IS NULL OR ",
    "water_temp_counter IS NULL OR ",
    "heating_control_runtime IS NULL) LIMIT 100;"
);

/// Checks how many entries there are in the heating set value table.
pub const SQL_QUERY_CHECK_HEATING_STATS_COUNT: &str = "SELECT COUNT(*) FROM heating_stats LIMIT 1;";

/// Reads the heating set values from the database.
pub const SQL_QUERY_READ_HEATING_SET_VALS: &str =
    "SELECT heating_switch_off_temp, heating_switch_on_temp FROM heating_set_vals LIMIT 100;";

/// Reads the ventilation set values from the database.
pub const SQL_QUERY_READ_VENTILATION_SET_VALS: &str =
    "SELECT ventilation_switch_off_temp, ventilation_switch_on_temp FROM ventilation_set_vals LIMIT 100;";

#[cfg(test)]
/// Write heating set values to the database.
pub const SQL_QUERY_WRITE_HEATING_SET_VALS: &str = concat!(
    "INSERT into heating_set_vals(heating_switch_off_temp, heating_switch_on_temp) ",
    "values(:heating_switch_off_temp, :heating_switch_on_temp);"
);

#[cfg(test)]
/// Write ventilation set values to the database.
pub const SQL_QUERY_WRITE_VENTILATION_SET_VALS: &str = concat!(
    "INSERT into ventilation_set_vals(ventilation_switch_off_temp, ventilation_switch_on_temp) ",
    "values(:ventilation_switch_off_temp, :ventilation_switch_on_temp);"
);

// Checks if the ventilation set value table contains any NULL values.
pub const SQL_QUERY_CHECK_VENTILATION_SET_VALS_NULL: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM ventilation_set_vals ",
    "WHERE ventilation_switch_off_temp IS NULL OR ventilation_switch_on_temp IS NULL) LIMIT 100;"
);

/// Checks how many entries there are in the heating set value table.
pub const SQL_QUERY_CHECK_VENTILATION_SET_VALS_COUNT: &str =
    "SELECT COUNT(*) FROM ventilation_set_vals LIMIT 1;";

/// Checks if the schedule table contains any NULL values.
pub const SQL_QUERY_CHECK_SCHEDULE_NULL: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM schedule ",
    "WHERE schedule_type IS NULL OR start_time IS NULL OR stop_time IS NULL OR is_active IS NULL) ",
    "LIMIT 100;"
);

/// Checks how many entries there are in the schedule table.
pub const SQL_QUERY_CHECK_SCHEDULE_COUNT: &str = "SELECT COUNT(*) FROM schedule LIMIT 1;";

/// Provides the system variable that controls how long the server waits for activity on an idle connection before closing it.
pub const SQL_QUERY_READ_TIMEOUT: &str = concat!(
    "SELECT VARIABLE_VALUE FROM INFORMATION_SCHEMA.GLOBAL_VARIABLES ",
    "WHERE VARIABLE_NAME = 'wait_timeout' LIMIT 100;"
);

/// Minimal request to the database to avoid timeouts
pub const SQL_QUERY_PING: &str = "SELECT 1 LIMIT 1;";

#[cfg(test)]
// Deletes the content of a specific table.
// This query is used for testing by multiple modules. Therefore, it resides here and not in the test module.
pub const SQL_QUERY_TRUNCATE_TABLE: &str = "truncate table #table;";

#[cfg(test)]
// Provides feed event from the database - used for testing only.
pub const SQL_QUERY_READ_LAST_FEED_EVENT: &str =
    "SELECT timestamp, feeder_on_time, profile_id from feed_log LIMIT 100;";

#[cfg(test)]
// Provide Balling dosing event from the database - used for testing only.
pub const SQL_QUERY_READ_BALLING_EVENT: &str =
    "SELECT timestamp, pump_id, dosing_volume from balling_log LIMIT 100;";

#[cfg(test)]
// Provides refill event from the database - used for testing only.
pub const SQL_QUERY_READ_REFILL_EVENT: &str =
    "SELECT timestamp, duration, volume, error_code from refill_log ORDER BY Timestamp DESC LIMIT 1;";

/// Checks how many entries there are in the refill table.
pub const SQL_QUERY_CHECK_REFILL_COUNT: &str = "SELECT COUNT(*) FROM refill_log LIMIT 1;";

#[cfg(test)]
/// Calculates a timestamp based on current timestamp and offset in seconds.
pub const SQL_QUERY_CURRENT_TIMESTAMP_OFFSET_SECONDS: &str =
    "select date_add(current_timestamp(), interval #offset second) LIMIT 100;";

#[cfg(test)]
/// Insert test data into the schedule table;
pub const SQL_QUERY_WRITE_SCHEDULE_TEST_DATA: &str =
    "INSERT INTO schedule VALUES(:schedule_name, :start_time, :stop_time, 1);";

#[cfg(test)]
/// Inserts data of one feed schedule entry into the database.
pub const SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY: &str = concat!(
    "INSERT into feed_schedule (timestamp, profile_id, is_weekly, is_daily) ",
    "values(:timestamp, :profile_id, :is_weekly, :is_daily)"
);

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
// Query for reading the first part from data for test cases operating on a valid database entry
pub const SQL_QUERY_READ_DATA_FRAME_1: &str = concat!(
    "SELECT timestamp, ",
    "water_temperature, water_temperature_filtered, ",
    "ph_value, ph_value_filtered, ",
    "conductivity, conductivity_filtered ",
    "FROM data WHERE timestamp='#timestamp';"
);

#[cfg(test)]
// Query for reading the second part from data for test cases operating on a valid database entry
pub const SQL_QUERY_READ_DATA_FRAME_2: &str = concat!(
        "SELECT refill_in_progress, ",
        "tank_level_switch_position, tank_level_switch_invalid, tank_level_switch_position_stabilized, ",
        "surface_ventilation_status, ",
        "ambient_temperature, ambient_humidity, heater_status FROM data ",
        "WHERE timestamp = '#timestamp';"
);

#[cfg(test)]
// Query for reading a complete data set for test cases with operating on an invalid database entry
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

#[cfg(test)]
// Query for writing a set of balling set values for one pump to the database
pub const SQL_QUERY_WRITE_BALLING_SET_VALS: &str = concat!(
    "INSERT into balling_set_vals(pump_id, dosing_speed, dosing_volume, label) ",
    "values(:pump_id, :dosing_speed, :dosing_volume, :label);"
);
