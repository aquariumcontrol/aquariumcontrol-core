/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// Provides version information to check if the current application is listed in the table of supported versions.
/// It informs whether the version is supported and which hash is stored in the database for the version.
pub const SQL_QUERY_VERSION_INFORMATION: &str =
    "SELECT hash FROM version WHERE major=#major AND minor=#minor AND build=#build;";

/// Provides the current database name.
pub const SQL_QUERY_DATABASE: &str = "select database();";

/// Provides all tables from the database.
pub const SQL_QUERY_READ_TABLES: &str = "SHOW TABLES;";

/// Provides the current date (without time) from the database.
pub const SQL_QUERY_READ_CURRENT_DATE: &str = "SELECT CURRENT_DATE();";

/// Provides tomorrow's date (without time) from the database.
pub const SQL_QUERY_READ_TOMORROW_DATE: &str = "SELECT CURRENT_DATE() + INTERVAL 1 DAY;";

/// Provides the current timestamp from the database.
pub const SQL_QUERY_READ_CURRENT_TIMESTAMP: &str = "SELECT CURRENT_TIMESTAMP();";

/// Inserts data of one feed event into the database.
pub const SQL_QUERY_WRITE_FEED_EVENT: &str =
        "INSERT into feedlog (Timestamp, FeederOnTime, ProfileName, ProfileIdx) values(:timestamp, :feeder_run_time, :profile_name, :profile_id)";

/// Provides the header of a feed pattern from the database.
pub const SQL_QUERY_READ_FEEDPATTERN_HEADER: &str =
    "SELECT ProfileID, ProfileName FROM feedpatterns WHERE ProfileID=profile_id;";

/// Provides one phase of a feed pattern from the database.
pub const SQL_QUERY_READ_FEED_PHASE: &str = "SELECT Pause#Duration, Pause#Skimmer, Pause#MPmp1, Pause#MPmp2, Pause#APmp1, Pause#APmp2, \
                Feed#Duration, Feed#Skimmer, Feed#MPmp1, Feed#MPmp2, Feed#APmp1, Feed#APmp2 FROM feedpatterns WHERE ProfileID=profile_id;";

/// Provides the timestamp of the last refill event from the database.
pub const SQL_QUERY_READ_LAST_REFILL_TIMESTAMP: &str =
    "SELECT Timestamp from refill as LSTTSTMP ORDER BY Timestamp DESC LIMIT 1;";

/// Checks if the refill table is empty.
pub const SQL_QUERY_CHECK_REFILL_EMPTY: &str = "SELECT EXISTS(SELECT 1 FROM refill);";

/// Provides the timestamp of the last Balling dosing event from the database.
pub const SQL_QUERY_READ_LAST_BALLING_DOSING_TIMESTAMP: &str =
        "SELECT Timestamp from ballingdosinglog as LSTTSTMP WHERE pumpid=#pump_id ORDER BY Timestamp DESC LIMIT 1;";

/// Writes the last Balling dosing event to the database.    
pub const SQL_QUERY_WRITE_BALLING_EVENT: &str =
        "INSERT into ballingdosinglog(Timestamp, pumpid, dosingvolume) values(:timestamp, :pump_id, :dosing_volume);";

#[cfg(test)]
/// Inserts a Balling pump configuration to the database.    
pub const SQL_QUERY_WRITE_PUMP_CONFIGURATION: &str =
        "INSERT into ballingsetvals(pumpid, dosingspeed, dosingvolume, label) values(:pump_id, :dosing_speed, :dosing_volume, :label);";

/// Provides the Balling dosing pump configuration for one of the pumps.
pub const SQL_QUERY_READ_BALLING_SETVALS: &str =
    "SELECT pumpid, dosingspeed, dosingvolume, label FROM ballingsetvals WHERE pumpid=#pump_id;";

/// Provides the refill volume of the last hour from the database.
/// It needs to use the COALESCE function because a null result is not interpreted correctly by the SQL library.
pub const SQL_QUERY_READ_REFILL_VOLUME_LAST_HOUR: &str =
        "SELECT COALESCE(SUM(Volume), 0) from refill WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 HOUR LIMIT 1;";

/// Provides the number of refill events from the last hour from the database.
/// It does not need to use the COALESCE function because the query returns 0 if there is no refill event in the last 24h.    
pub const SQL_QUERY_READ_REFILL_COUNT_LAST_HOUR: &str =
        "SELECT COUNT(Volume) from refill WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 HOUR LIMIT 1;";

/// Provides the refill volume of the last day from the database.
/// It needs to use the COALESCE function because a null result is not interpreted correctly by the SQL library.
pub const SQL_QUERY_READ_REFILL_VOLUME_LAST_24H: &str =
        "SELECT COALESCE(SUM(Volume), 0) from refill WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 DAY LIMIT 1;";

/// Provides the number of refill events from the last day from the database.
/// It does not need to use the COALESCE function because the query returns 0 if there is no refill event in the last 24h.    
pub const SQL_QUERY_READ_REFILL_COUNT_LAST_24H: &str =
        "SELECT COUNT(Volume) from refill WHERE Timestamp > CURRENT_TIMESTAMP() - INTERVAL 1 DAY LIMIT 1;";

/// Writes the last refill event to the database.    
pub const SQL_QUERY_WRITE_REFILL_EVENT: &str =
        "INSERT into refill(Timestamp, Duration, Volume, ErrorCode) values(:timestamp, :duration, :volume, :error_code);";

/// Provides the duration in seconds until midnight from the database.
pub const SQL_QUERY_READ_DURATION_UNTIL_MIDNIGHT: &str =
    "SELECT TIMESTAMPDIFF(SECOND,NOW(),DATE(NOW()) + INTERVAL 1 DAY);";

/// Provides the heating statistical data of the current day from the database.
pub const SQL_QUERY_READ_HEATING_STATS: &str =
    "SELECT * FROM heatingstats WHERE Date=CURRENT_DATE();";

/// Provides the feed schedule entries which lie in the past in descending order
pub const SQL_QUERY_READ_PAST_FEED_SCHEDULE_ENTRY: &str =
    "SELECT TimeStamp, ProfileId, ProfileName, IsDaily \
        FROM feedschedule WHERE TimeStamp < CURRENT_TIMESTAMP() ORDER BY TimeStamp DESC;";

#[cfg(test)]
/// Provides the feed schedule entries which lie in the future in ascending order
pub const SQL_QUERY_READ_FUTURE_FEED_SCHEDULE_ENTRY: &str =
    "SELECT TimeStamp, ProfileId, ProfileName, IsDaily \
        FROM feedschedule WHERE TimeStamp > CURRENT_TIMESTAMP() ORDER BY TimeStamp ASC;";

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
pub const SQL_QUERY_WRITE_HEATING_STATS: &str = "insert into heatingstats \
        (Date, Energy, AmbientTempAverage, AmbientTempCounter, WaterTempAverage, WaterTempCounter, HeatingControlRuntime) \
        values (:date, :energy, \
        :ambient_temperature_average, :ambient_temperature_counter, \
        :water_temperature_average, :water_temperature_counter, \
        :heating_control_runtime) \
        ON DUPLICATE KEY Update \
        Energy=#Energy, \
        AmbientTempAverage=#AmbientTempAverage, \
        AmbientTempCounter=#AmbientTempCounter, \
        WaterTempAverage=#WaterTempAverage, \
        WaterTempCounter=#WaterTempCounter, \
        HeatingControlRuntime=#HeatingControlRuntime;";

/// Writes the time-based data into the database.
pub const SQL_QUERY_WRITE_DATA: &str = "insert into data \
        (Timestamp, \
        Temperature, TemperatureFiltered, \
        pH, pHFiltered, \
        Conductivity, ConductivityFiltered, ConductivityCompensated, \
        RefillInProgress, \
        TankLevelSwitchPosition, TankLevelSwitchInvalid, TankLevelSwitchPositionStabilized, \
        SurfaceVentilation, \
        AmbTemp, AmbHum, Heater, TempWaterDs18B20, TempAmbDs18B20) \
        values (:timestamp, \
        #TemperatureUnfiltered, #TemperatureFiltered, \
        #pHUnfiltered, #pHFiltered, \
        #ConductivityUnfiltered, #ConductivityFiltered, #ConductivityCompensated, \
        #RefillInProgress, \
        #TankLevelSwitchPosition, #TankLevelSwitchInvalid, #TankLevelSwitchPositionStabilized, \
        #SurfaceVentilation, \
        #AmbTemp, #AmbHum, \
        #Heater, \
        #TempWaterDs18B20, #TempAmbDs18B20) \
        ON DUPLICATE KEY UPDATE \
        Temperature=#TemperatureUnfiltered, TemperatureFiltered=#TemperatureFiltered, \
        pH=#pHUnfiltered, pHFiltered=#pHFiltered, \
        Conductivity=#ConductivityUnfiltered, ConductivityFiltered=#ConductivityFiltered, \
        ConductivityCompensated=#ConductivityCompensated, \
        RefillInProgress=#RefillInProgress, \
        TankLevelSwitchPosition=#TankLevelSwitchPosition, \
        TankLevelSwitchInvalid=#TankLevelSwitchInvalid, \
        TankLevelSwitchPositionStabilized=#TankLevelSwitchPositionStabilized, \
        SurfaceVentilation=#SurfaceVentilation, \
        AmbTemp=#AmbTemp, AmbHum=#AmbHum, \
        Heater=#Heater, \
        TempWaterDs18B20=#TempWaterDs18B20, TempAmbDs18B20=#TempAmbDs18B20;";

/// Provides the schedule entry for one of the controls (Refill, Ventilation, Heating, Balling)
pub const SQL_QUERY_READ_SCHEDULE: &str = "SELECT * FROM schedule;";

/// Provides the system variable that controls how long the server waits for activity on an idle connection before closing it.
pub const SQL_QUERY_READ_TIMEOUT: &str = "SELECT VARIABLE_VALUE FROM INFORMATION_SCHEMA.GLOBAL_VARIABLES WHERE VARIABLE_NAME = 'wait_timeout';";

// Deletes the content of a specific table.
// This query is used for testing by multiple modules. Therefore, it resides here and not in the test module.
#[cfg(test)]
pub const SQL_QUERY_TRUNCATE_TABLE: &str = "truncate table #table;";

// Provides feed event from the database - used for testing only.
#[cfg(test)]
pub const SQL_QUERY_READ_LAST_FEED_EVENT: &str =
    "SELECT Timestamp, FeederOnTime, ProfileName, ProfileIdx from feedlog;";

// Provide Balling dosing event from the database - used for testing only.
#[cfg(test)]
pub const SQL_QUERY_READ_BALLING_EVENT: &str =
    "SELECT Timestamp, pumpid, dosingvolume from ballingdosinglog;";

// Provides refill event from the database - used for testing only.
#[cfg(test)]
pub const SQL_QUERY_READ_REFILL_EVENT: &str =
    "SELECT Timestamp, Duration, Volume, ErrorCode from refill ORDER BY Timestamp DESC LIMIT 1;";

/// Inserts data of one feed schedule entry into the database.
#[cfg(test)]
pub const SQL_QUERY_CURRENT_TIMESTAMP_OFFSET_SECONDS: &str =
    "select date_add(current_timestamp(), interval #offset second);";

/// Inserts data of one feed schedule entry into the database.
#[cfg(test)]
pub const SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY: &str =
    "INSERT into feedschedule (Timestamp, ProfileId, ProfileName, IsWeekly, IsDaily) \
         values(:timestamp, :profile_id, :profile_name, :is_weekly, :is_daily)";

/// This query is used for testing: It creates a first feed pattern.
#[cfg(test)]
pub const SQL_QUERY_WRITE_FEEDPATTERN_TEST1: &str = "INSERT INTO feedpatterns VALUES \
                (:profile_id,'test',\
                0,1,0,0,0,0,\
                1,0,1,0,0,0,\
                2,0,0,1,0,0,\
                3,0,0,0,1,0,\
                4,0,0,0,0,1,\
                5,1,1,0,0,0,\
                6,1,0,1,0,0,\
                7,1,0,0,1,0,\
                8,1,0,0,0,1,\
                9,1,1,1,0,0,\
                10,1,1,0,1,0,\
                11,1,1,0,0,1,\
                12,1,1,1,1,0,\
                13,1,1,1,0,1,\
                14,1,1,1,1,1,\
                15,1,1,1,1,0,\
                16,1,1,1,0,0,\
                17,1,1,0,0,0,\
                18,1,0,0,0,0,\
                19,0,0,0,0,0);";

/// This query is used for testing: It creates a second feed pattern.
#[cfg(test)]
pub const SQL_QUERY_WRITE_FEEDPATTERN_TEST2: &str = "INSERT INTO feedpatterns VALUES \
                (:profile_id,'second',\
                1,1,1,1,1,1,\
                1,1,1,1,1,1,\
                2,2,2,2,2,2,\
                2,2,2,2,2,2,\
                3,3,3,3,3,3,\
                3,3,3,3,3,3,\
                4,4,4,4,4,4,\
                4,4,4,4,4,4,\
                5,5,5,5,5,5,\
                5,5,5,5,5,5,\
                6,6,6,6,6,6,\
                6,6,6,6,6,6,\
                7,7,7,7,7,7,\
                7,7,7,7,7,7,\
                8,8,8,8,8,8,\
                8,8,8,8,8,8,\
                9,9,9,9,9,9,\
                9,9,9,9,9,9,\
                1,1,1,1,1,1,\
                1,1,1,1,1,1);";
