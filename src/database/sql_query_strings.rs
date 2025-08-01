/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// Provides version information to check if the current application is listed in the table of supported versions.
/// It informs whether the version is supported and which hash is stored in the database for the version.
pub const SQL_QUERY_VERSION_INFORMATION: &str =
    "SELECT hash FROM version WHERE major=#major AND minor=#minor AND build=#build LIMIT 100;";

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
    "INSERT into feedlog (Timestamp, FeederOnTime, ProfileName, ProfileIdx) ",
    "values(:timestamp, :feeder_run_time, :profile_name, :profile_id)"
);

/// Definition of table name for Data recorder
#[cfg(test)]
pub const SQL_TABLE_DATA: &str = "data";

/// Definition of table name for Balling set values
#[cfg(test)]
pub const SQL_TABLE_BALLING_SETVALS: &str = "ballingsetvals";

/// Definition of table name for Balling dosing log
#[cfg(test)]
pub const SQL_TABLE_BALLING_DOSING_LOG: &str = "ballingdosinglog";

/// Definition of table name for schedule
#[cfg(test)]
pub const SQL_TABLE_SCHEDULE: &str = "schedule";

/// Definition of table name for heating set values
#[cfg(test)]
pub const SQL_TABLE_HEATING_SETVALS: &str = "heatingsetvals";

/// Definition of table name for heating statistics
#[cfg(test)]
pub const SQL_TABLE_HEATING_STATS: &str = "heatingstats";

/// Definition of table name for ventilation set values
#[cfg(test)]
pub const SQL_TABLE_VENTILATION_SETVALS: &str = "ventilationsetvals";

/// Definition of table name for feed log
#[cfg(test)]
pub const SQL_TABLE_FEEDLOG: &str = "feedlog";

/// Definition of table name for refill log
#[cfg(test)]
pub const SQL_TABLE_REFILL: &str = "refill";

/// Definition of table name for feed patterns
pub const SQL_TABLE_FEEDPATTERNS: &str = "feedpatterns";

/// Definition of table name for feed schedule
pub const SQL_TABLE_FEEDSCHEDULE: &str = "feedschedule";

/// Provides the header of a feed pattern from the database.
pub const SQL_QUERY_READ_FEEDPATTERN_HEADER: &str =
    "SELECT ProfileID, ProfileName FROM feedpatterns WHERE ProfileID=profile_id LIMIT 100;";

/// Provides one phase of a feed pattern from the database.
pub const SQL_QUERY_READ_FEED_PHASE: &str = concat!(
    "SELECT Pause#Duration, Pause#Skimmer, Pause#MPmp1, Pause#MPmp2, Pause#APmp1, Pause#APmp2, ",
    "Feed#Duration, Feed#Skimmer, Feed#MPmp1, Feed#MPmp2, Feed#APmp1, Feed#APmp2 ",
    "FROM feedpatterns WHERE ProfileID=profile_id LIMIT 100;"
);

/// Provides the timestamp of the last refill event from the database.
pub const SQL_QUERY_READ_LAST_REFILL_TIMESTAMP: &str =
    "SELECT Timestamp from refill as LSTTSTMP ORDER BY Timestamp DESC LIMIT 1;";

/// Checks if the refill table is empty.
pub const SQL_QUERY_CHECK_REFILL_EMPTY: &str = "SELECT EXISTS(SELECT 1 FROM refill) LIMIT 1;";

/// Provides the timestamp of the last Balling dosing event from the database.
pub const SQL_QUERY_READ_LAST_BALLING_DOSING_TIMESTAMP: &str = concat!(
    "SELECT Timestamp from ballingdosinglog as LSTTSTMP ",
    "WHERE pumpid=#pump_id ORDER BY Timestamp DESC LIMIT 1;"
);

/// Writes the last Balling dosing event to the database.    
pub const SQL_QUERY_WRITE_BALLING_EVENT: &str = concat!(
    "INSERT into ballingdosinglog(Timestamp, pumpid, dosingvolume) ",
    "values(:timestamp, :pump_id, :dosing_volume);"
);

#[cfg(test)]
/// Inserts a Balling pump configuration to the database.    
pub const SQL_QUERY_WRITE_PUMP_CONFIGURATION: &str = concat!(
    "INSERT into ballingsetvals(pumpid, dosingspeed, dosingvolume, label) ",
    "values(:pump_id, :dosing_speed, :dosing_volume, :label);"
);

/// Provides the Balling dosing pump configuration for one of the pumps.
pub const SQL_QUERY_READ_BALLING_SETVALS: &str =
    "SELECT pumpid, dosingspeed, dosingvolume, label FROM ballingsetvals WHERE pumpid=#pump_id LIMIT 100;";

/// Provides the refill volume of the last hour from the database.
/// It needs to use the COALESCE function because a null result is not interpreted correctly by the SQL library.
pub const SQL_QUERY_READ_REFILL_VOLUME_LAST_HOUR: &str = concat!(
    "SELECT COALESCE(SUM(Volume), 0) from refill ",
    "WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 HOUR LIMIT 1;"
);

/// Provides the number of refill events from the last hour from the database.
/// It does not need to use the COALESCE function because the query returns 0 if there is no refill event in the last 24h.    
pub const SQL_QUERY_READ_REFILL_COUNT_LAST_HOUR: &str = concat!(
    "SELECT COUNT(Volume) from refill ",
    "WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 HOUR LIMIT 1;"
);

/// Provides the refill volume of the last day from the database.
/// It needs to use the COALESCE function because a null result is not interpreted correctly by the SQL library.
pub const SQL_QUERY_READ_REFILL_VOLUME_LAST_24H: &str = concat!(
    "SELECT COALESCE(SUM(Volume), 0) from refill ",
    "WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 DAY LIMIT 1;"
);

/// Provides the number of refill events from the last day from the database.
/// It does not need to use the COALESCE function because the query returns 0 if there is no refill event in the last 24h.    
pub const SQL_QUERY_READ_REFILL_COUNT_LAST_24H: &str = concat!(
    "SELECT COUNT(Volume) from refill ",
    "WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 DAY LIMIT 1;"
);

/// Writes the last refill event to the database.    
pub const SQL_QUERY_WRITE_REFILL_EVENT: &str = concat!(
    "INSERT into refill(Timestamp, Duration, Volume, ErrorCode) ",
    "values(:timestamp, :duration, :volume, :error_code);"
);

/// Provides the duration in seconds until midnight from the database.
pub const SQL_QUERY_READ_DURATION_UNTIL_MIDNIGHT: &str =
    "SELECT TIMESTAMPDIFF(SECOND,NOW(),DATE(NOW()) + INTERVAL 1 DAY) LIMIT 1;";

/// Provides the heating statistical data of the current day from the database.
pub const SQL_QUERY_READ_HEATING_STATS: &str =
    "SELECT * FROM heatingstats WHERE Date=CURRENT_DATE() LIMIT 1;";

/// Provides the feed schedule entries which lie in the past in descending order
pub const SQL_QUERY_READ_PAST_FEED_SCHEDULE_ENTRY: &str = concat!(
    "SELECT TimeStamp, ProfileId, ProfileName, IsDaily FROM feedschedule ",
    "WHERE TimeStamp < CURRENT_TIMESTAMP() ORDER BY TimeStamp DESC LIMIT 100;"
);

#[cfg(test)]
/// Provides the feed schedule entries which lie in the future in ascending order
pub const SQL_QUERY_READ_FUTURE_FEED_SCHEDULE_ENTRY: &str = concat!(
    "SELECT TimeStamp, ProfileId, ProfileName, IsDaily FROM feedschedule ",
    "WHERE TimeStamp > CURRENT_TIMESTAMP() ORDER BY TimeStamp ASC LIMIT 100;"
);

/// Updates a feed schedule entry after having executing a feed pattern
/// which shall be repeated daily.
pub const SQL_QUERY_UPDATE_FEED_SCHEDULE_ENTRY: &str =
    "UPDATE feedschedule SET Timestamp=:timestamp_new WHERE Timestamp=:timestamp_old;";

/// Updates a feed schedule entry after having executing a feed pattern
/// which shall not be repeated daily.
pub const SQL_QUERY_DELETE_FEED_SCHEDULE_ENTRY: &str =
    "DELETE FROM feedschedule WHERE Timestamp = :timestamp;";

/// Writes the heating statistical data into the database.
/// If there is already data for the given date, the data will not be inserted. Instead, the existing entry will be updated.
pub const SQL_QUERY_WRITE_HEATING_STATS: &str = concat!(
    "insert into heatingstats ",
    "(Date, Energy, AmbientTempAverage, AmbientTempCounter, ",
    "WaterTempAverage, WaterTempCounter, HeatingControlRuntime) ",
    "values (:date, :energy, ",
    ":ambient_temperature_average, :ambient_temperature_counter, ",
    ":water_temperature_average, :water_temperature_counter, ",
    ":heating_control_runtime) ",
    "ON DUPLICATE KEY Update ",
    "Energy=#Energy, ",
    "AmbientTempAverage=#AmbientTempAverage, ",
    "AmbientTempCounter=#AmbientTempCounter, ",
    "WaterTempAverage=#WaterTempAverage, ",
    "WaterTempCounter=#WaterTempCounter, ",
    "HeatingControlRuntime=#HeatingControlRuntime;"
);

/// Writes the time-based data into the database.
pub const SQL_QUERY_WRITE_DATA: &str = concat!(
    "insert into data ",
    "(Timestamp, ",
    "Temperature, TemperatureFiltered, ",
    "pH, pHFiltered, ",
    "Conductivity, ConductivityFiltered, ConductivityCompensated, ",
    "RefillInProgress, ",
    "TankLevelSwitchPosition, TankLevelSwitchInvalid, TankLevelSwitchPositionStabilized, ",
    "SurfaceVentilation, ",
    "AmbTemp, AmbHum, Heater) ",
    "values (:timestamp, ",
    "#TemperatureUnfiltered, #TemperatureFiltered, ",
    "#pHUnfiltered, #pHFiltered, ",
    "#ConductivityUnfiltered, #ConductivityFiltered, #ConductivityCompensated, ",
    "#RefillInProgress, ",
    "#TankLevelSwitchPosition, #TankLevelSwitchInvalid, #TankLevelSwitchPositionStabilized, ",
    "#SurfaceVentilation, ",
    "#AmbTemp, #AmbHum, ",
    "#Heater) ",
    "ON DUPLICATE KEY UPDATE ",
    "Temperature=#TemperatureUnfiltered, TemperatureFiltered=#TemperatureFiltered, ",
    "pH=#pHUnfiltered, pHFiltered=#pHFiltered, ",
    "Conductivity=#ConductivityUnfiltered, ConductivityFiltered=#ConductivityFiltered, ",
    "ConductivityCompensated=#ConductivityCompensated, ",
    "RefillInProgress=#RefillInProgress, ",
    "TankLevelSwitchPosition=#TankLevelSwitchPosition, ",
    "TankLevelSwitchInvalid=#TankLevelSwitchInvalid, ",
    "TankLevelSwitchPositionStabilized=#TankLevelSwitchPositionStabilized, ",
    "SurfaceVentilation=#SurfaceVentilation, ",
    "AmbTemp=#AmbTemp, AmbHum=#AmbHum, ",
    "Heater=#Heater;"
);

/// Checks how many entries there are in the data table.
pub const SQL_QUERY_CHECK_DATA_COUNT: &str = concat!("SELECT COUNT(*) FROM data LIMIT 1;");

/// Provides the schedule entry for one of the controls (Refill, Ventilation, Heating, Balling)
pub const SQL_QUERY_READ_SCHEDULE: &str = "SELECT * FROM schedule LIMIT 100;";

/// Checks if the balling set value table contains any NULL values.
pub const SQL_QUERY_CHECK_BALLING_SETVALS_NULL: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM ballingsetvals ",
    "WHERE dosingspeed IS NULL OR dosingvolume IS NULL OR label IS NULL OR pumpid IS NULL) LIMIT 100;"
);

/// Checks how many entries are in the balling set value table
pub const SQL_QUERY_CHECK_BALLING_SETVALS_COUNT: &str =
    concat!("SELECT COUNT(*) FROM ballingsetvals LIMIT 1;");

/// Checks if the feed patterns table contains any NULL values.
pub const SQL_QUERY_CHECK_FEEDPATTERNS_NULL: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM feedpatterns ",
    "WHERE ProfileID IS NULL OR ProfileName IS NULL OR ",
    "Pause01Duration IS NULL OR ",
    "Pause01Skimmer IS NULL OR ",
    "Pause01MPmp1 IS NULL OR ",
    "Pause01MPmp2 IS NULL OR ",
    "Pause01APmp1 IS NULL OR ",
    "Pause01APmp2 IS NULL OR ",
    "Feed01Duration IS NULL OR ",
    "Feed01Skimmer IS NULL OR ",
    "Feed01MPmp1 IS NULL OR ",
    "Feed01MPmp2 IS NULL OR ",
    "Feed01APmp1 IS NULL OR ",
    "Feed01APmp2 IS NULL OR ",
    "Pause02Duration IS NULL OR ",
    "Pause02Skimmer IS NULL OR ",
    "Pause02MPmp1 IS NULL OR ",
    "Pause02MPmp2 IS NULL OR ",
    "Pause02APmp1 IS NULL OR ",
    "Pause02APmp2 IS NULL OR ",
    "Feed02Duration IS NULL OR ",
    "Feed02Skimmer IS NULL OR ",
    "Feed02MPmp1 IS NULL OR ",
    "Feed02MPmp2 IS NULL OR ",
    "Feed02APmp1 IS NULL OR ",
    "Feed02APmp2 IS NULL OR ",
    "Pause03Duration IS NULL OR ",
    "Pause03Skimmer IS NULL OR ",
    "Pause03MPmp1 IS NULL OR ",
    "Pause03MPmp2 IS NULL OR ",
    "Pause03APmp1 IS NULL OR ",
    "Pause03APmp2 IS NULL OR ",
    "Feed03Duration IS NULL OR ",
    "Feed03Skimmer IS NULL OR ",
    "Feed03MPmp1 IS NULL OR ",
    "Feed03MPmp2 IS NULL OR ",
    "Feed03APmp1 IS NULL OR ",
    "Feed03APmp2 IS NULL OR ",
    "Pause04Duration IS NULL OR ",
    "Pause04Skimmer IS NULL OR ",
    "Pause04MPmp1 IS NULL OR ",
    "Pause04MPmp2 IS NULL OR ",
    "Pause04APmp1 IS NULL OR ",
    "Pause04APmp2 IS NULL OR ",
    "Feed04Duration IS NULL OR ",
    "Feed04Skimmer IS NULL OR ",
    "Feed04MPmp1 IS NULL OR ",
    "Feed04MPmp2 IS NULL OR ",
    "Feed04APmp1 IS NULL OR ",
    "Feed04APmp2 IS NULL OR ",
    "Pause05Duration IS NULL OR ",
    "Pause05Skimmer IS NULL OR ",
    "Pause05MPmp1 IS NULL OR ",
    "Pause05MPmp2 IS NULL OR ",
    "Pause05APmp1 IS NULL OR ",
    "Pause05APmp2 IS NULL OR ",
    "Feed05Duration IS NULL OR ",
    "Feed05Skimmer IS NULL OR ",
    "Feed05MPmp1 IS NULL OR ",
    "Feed05MPmp2 IS NULL OR ",
    "Feed05APmp1 IS NULL OR ",
    "Feed05APmp2 IS NULL OR ",
    "Pause06Duration IS NULL OR ",
    "Pause06Skimmer IS NULL OR ",
    "Pause06MPmp1 IS NULL OR ",
    "Pause06APmp1 IS NULL OR ",
    "Pause06MPmp2 IS NULL OR ",
    "Pause06APmp2 IS NULL OR ",
    "Feed06Duration IS NULL OR ",
    "Feed06Skimmer IS NULL OR ",
    "Feed06MPmp1 IS NULL OR ",
    "Feed06MPmp2 IS NULL OR ",
    "Feed06APmp1 IS NULL OR ",
    "Feed06APmp2 IS NULL OR ",
    "Pause07Duration IS NULL OR ",
    "Pause07Skimmer IS NULL OR ",
    "Pause07MPmp1 IS NULL OR ",
    "Pause07MPmp2 IS NULL OR ",
    "Pause07APmp1 IS NULL OR ",
    "Pause07APmp2 IS NULL OR ",
    "Feed07Duration IS NULL OR ",
    "Feed07Skimmer IS NULL OR ",
    "Feed07MPmp1 IS NULL OR ",
    "Feed07MPmp2 IS NULL OR ",
    "Feed07APmp1 IS NULL OR ",
    "Feed07APmp2 IS NULL OR ",
    "Pause08Duration IS NULL OR ",
    "Pause08Skimmer IS NULL OR ",
    "Pause08MPmp1 IS NULL OR ",
    "Pause08MPmp2 IS NULL OR ",
    "Pause08APmp1 IS NULL OR ",
    "Pause08APmp2 IS NULL OR ",
    "Feed08Duration IS NULL OR ",
    "Feed08Skimmer IS NULL OR ",
    "Feed08MPmp1 IS NULL OR ",
    "Feed08MPmp2 IS NULL OR ",
    "Feed08APmp1 IS NULL OR ",
    "Feed08APmp2 IS NULL OR ",
    "Pause09Duration IS NULL OR ",
    "Pause09Skimmer IS NULL OR ",
    "Pause09MPmp1 IS NULL OR ",
    "Pause09MPmp2 IS NULL OR ",
    "Pause09APmp1 IS NULL OR ",
    "Pause09APmp2 IS NULL OR ",
    "Feed09Duration IS NULL OR ",
    "Feed09Skimmer IS NULL OR ",
    "Feed09MPmp1 IS NULL OR ",
    "Feed09MPmp2 IS NULL OR ",
    "Feed09APmp1 IS NULL OR ",
    "Feed09APmp2 IS NULL OR ",
    "Pause10Duration IS NULL OR ",
    "Pause10Skimmer IS NULL OR ",
    "Pause10MPmp1 IS NULL OR ",
    "Pause10MPmp2 IS NULL OR ",
    "Pause10APmp1 IS NULL OR ",
    "Pause10APmp2 IS NULL OR ",
    "Feed10Duration IS NULL OR ",
    "Feed10Skimmer IS NULL OR ",
    "Feed10MPmp1 IS NULL OR ",
    "Feed10MPmp2 IS NULL OR ",
    "Feed10APmp1 IS NULL OR ",
    "Feed10APmp2 IS NULL) LIMIT 100;"
);

/// Checks how many entries there are in the feed patterns table.
pub const SQL_QUERY_CHECK_FEEDPATTERN_COUNT: &str =
    concat!("SELECT COUNT(*) FROM feedpatterns LIMIT 1;");

/// Checks if the feed schedule table contains any NULL values.
pub const SQL_QUERY_CHECK_FEEDSCHEDULE_NULL: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM feedschedule ",
    "WHERE Timestamp IS NULL OR ",
    "ProfileId IS NULL OR ",
    "ProfileName IS NULL OR ",
    "IsWeekly IS NULL OR ",
    "IsDaily IS NULL) LIMIT 100;"
);

/// Checks how many entries there are in the feed schedule table.
pub const SQL_QUERY_CHECK_FEEDSCHEDULE_COUNT: &str =
    concat!("SELECT COUNT(*) FROM feedschedule LIMIT 1;");

/// Checks if the heating set value table contains any NULL values.
pub const SQL_QUERY_CHECK_HEATING_SETVALS_NULL: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM heatingsetvals ",
    "WHERE heatingSwitchOffTemp IS NULL OR heatingSwitchOnTemp IS NULL) LIMIT 100;"
);

/// Checks how many entries there are in the heating set value table.
pub const SQL_QUERY_CHECK_HEATING_SETVALS_COUNT: &str =
    concat!("SELECT COUNT(*) FROM heatingsetvals LIMIT 1;");

/// Checks if the heating stats table contains any NULL values.
pub const SQL_QUERY_CHECK_HEATING_STATS_NULL: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM heatingstats ",
    "WHERE Date IS NULL OR ",
    "Energy IS NULL OR ",
    "AmbientTempAverage IS NULL OR ",
    "AmbientTempCounter IS NULL OR ",
    "WaterTempAverage IS NULL OR ",
    "WaterTempCounter IS NULL OR ",
    "HeatingControlRuntime IS NULL) LIMIT 100;"
);

/// Checks how many entries there are in the heating set value table.
pub const SQL_QUERY_CHECK_HEATING_STATS_COUNT: &str =
    concat!("SELECT COUNT(*) FROM heatingstats LIMIT 1;");

/// Reads the heating set values from the database.
pub const SQL_QUERY_READ_HEATING_SETVALS: &str =
    "SELECT heatingSwitchOffTemp, heatingSwitchOnTemp FROM heatingsetvals LIMIT 100;";

/// Reads the ventilation set values from the database.
pub const SQL_QUERY_READ_VENTILATION_SETVALS: &str =
    "SELECT ventilationSwitchOffTemp, ventilationSwitchOnTemp FROM ventilationsetvals LIMIT 100;";

#[cfg(test)]
/// Write heating set values to the database.
pub const SQL_QUERY_WRITE_HEATING_SETVALS: &str = concat!(
    "INSERT into heatingsetvals(heatingSwitchOffTemp, heatingSwitchOnTemp) ",
    "values(:heating_switch_off_temp, :heating_switch_on_temp);"
);

#[cfg(test)]
/// Write ventilation set values to the database.
pub const SQL_QUERY_WRITE_VENTILATION_SETVALS: &str = concat!(
    "INSERT into ventilationsetvals(ventilationSwitchOffTemp, ventilationSwitchOnTemp) ",
    "values(:ventilation_switch_off_temp, :ventilation_switch_on_temp);"
);

// Checks if the ventilation set value table contains any NULL values.
pub const SQL_QUERY_CHECK_VENTILATION_SETVALS_NULL: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM ventilationsetvals ",
    "WHERE ventilationSwitchOffTemp IS NULL OR ventilationSwitchOnTemp IS NULL) LIMIT 100;"
);

/// Checks how many entries there are in the heating set value table.
pub const SQL_QUERY_CHECK_VENTILATION_SETVALS_COUNT: &str =
    concat!("SELECT COUNT(*) FROM ventilationsetvals LIMIT 1;");

/// Checks if the schedule table contains any NULL values.
pub const SQL_QUERY_CHECK_SCHEDULE_NULL: &str = concat!(
    "SELECT EXISTS(SELECT 1 FROM schedule ",
    "WHERE scheduleType IS NULL OR startTime IS NULL OR stopTime IS NULL OR isActive IS NULL) LIMIT 100;"
);

/// Checks how many entries there are in the schedule table.
pub const SQL_QUERY_CHECK_SCHEDULE_COUNT: &str = concat!("SELECT COUNT(*) FROM schedule LIMIT 1;");

/// Provides the system variable that controls how long the server waits for activity on an idle connection before closing it.
pub const SQL_QUERY_READ_TIMEOUT: &str = concat!(
    "SELECT VARIABLE_VALUE FROM INFORMATION_SCHEMA.GLOBAL_VARIABLES ",
    "WHERE VARIABLE_NAME = 'wait_timeout' LIMIT 100;"
);

/// Minimal request to the database to avoid timeouts
pub const SQL_QUERY_PING: &str = "SELECT 1 LIMIT 1;";

// Deletes the content of a specific table.
// This query is used for testing by multiple modules. Therefore, it resides here and not in the test module.
#[cfg(test)]
pub const SQL_QUERY_TRUNCATE_TABLE: &str = "truncate table #table;";

// Provides feed event from the database - used for testing only.
#[cfg(test)]
pub const SQL_QUERY_READ_LAST_FEED_EVENT: &str =
    "SELECT Timestamp, FeederOnTime, ProfileName, ProfileIdx from feedlog LIMIT 100;";

// Provide Balling dosing event from the database - used for testing only.
#[cfg(test)]
pub const SQL_QUERY_READ_BALLING_EVENT: &str =
    "SELECT Timestamp, pumpid, dosingvolume from ballingdosinglog LIMIT 100;";

// Provides refill event from the database - used for testing only.
#[cfg(test)]
pub const SQL_QUERY_READ_REFILL_EVENT: &str =
    "SELECT Timestamp, Duration, Volume, ErrorCode from refill ORDER BY Timestamp DESC LIMIT 1;";

/// Checks how many entries there are in the refill table.
pub const SQL_QUERY_CHECK_REFILL_COUNT: &str = concat!("SELECT COUNT(*) FROM refill LIMIT 1;");

/// Calculates a timestamp based on current timestamp and offset in seconds.
#[cfg(test)]
pub const SQL_QUERY_CURRENT_TIMESTAMP_OFFSET_SECONDS: &str =
    "select date_add(current_timestamp(), interval #offset second) LIMIT 100;";

/// Insert test data into the schedule table;
#[cfg(test)]
pub const SQL_QUERY_WRITE_SCHEDULE_TEST_DATA: &str =
    "INSERT INTO schedule VALUES(:schedule_name, :start_time, :stop_time, 1);";

/// Inserts data of one feed schedule entry into the database.
#[cfg(test)]
pub const SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY: &str = concat!(
    "INSERT into feedschedule (Timestamp, ProfileId, ProfileName, IsWeekly, IsDaily) ",
    "values(:timestamp, :profile_id, :profile_name, :is_weekly, :is_daily)"
);

/// This query is used for testing: It creates a first feed pattern.
#[cfg(test)]
pub const SQL_QUERY_WRITE_FEEDPATTERN_TEST: &str = concat!(
    "INSERT INTO feedpatterns VALUES ",
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

/// This query is used for testing: It creates a second feed pattern.
#[cfg(test)]
pub const SQL_QUERY_WRITE_FEEDPATTERN_TEST2: &str = concat!(
    "INSERT INTO feedpatterns VALUES ",
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
