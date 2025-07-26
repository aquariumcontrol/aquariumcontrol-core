/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use chrono::ParseError;
use mysql::Error as MySqlError;
use thiserror::Error;

#[derive(Debug, Error)]
/// Enum codifies the errors which the program may encounter in communication with the DB
pub enum SqlInterfaceError {
    #[error(
        "Could not get a connection from the pool. The database may be down or the pool exhausted."
    )]
    ConnectionFromPool,

    #[error("The database wait_timeout ({0}s) is less than the required minimum ({1}s).")]
    WaitTimeoutTooLow(u64, u64),

    #[error("Could not parse wait_timeout value from database: {0}")]
    WaitTimeoutInvalid(i64),

    #[error("Could not establish connection pool with {0}")]
    ConnectionPoolFailure(String),

    #[error("Required tables ({0}) are not existing in database ({1}).")]
    RequiredTablesNotExisting(String, String),

    #[error("Error when reading tables from the database, expected more than one result.")]
    ShowTablesWithoutProperResult,

    #[error("Could not get single string from the database using query: {0}")]
    SingleVersionRequestFailure(String),

    #[error("SQL database responded with empty response to query: {0}")]
    SingleVersionRequestNotListed(String),

    #[error("{0}")]
    SingleVersionRequestMultipleResults(String),

    #[error("Could not get string array from the database using query: {0}")]
    SingleStringRequestFailure(String),

    #[error("SQL database responded with multiple responses to request to query: {0}")]
    SingleStringRequestNoSingleResponse(String),

    #[error("SQL database responded with empty response to request to query: {0}")]
    SingleStringRequestEmptyResponse(String),

    #[error("SQL database responded with empty response to query: {0}")]
    MultipleStringRequestEmptyResponse(String),

    #[error("SQL database responded with single row response to query: {0}")]
    MultipleStringRequestSingleResponse(String),

    #[error("Could not get single integer from the database using query: {0}")]
    SingleIntegerRequestFailure(String),

    #[error("SQL database responded with multiple responses to request to query: {0}")]
    SingleIntegerRequestNoSingleResponse(String),

    #[error("Could not get single float value from the database using query: {0}")]
    SingleFloatRequestFailure(String),

    #[error("SQL database responded with empty response to query: {0}")]
    SingleFloatRequestEmptyResponse(String),

    #[error("SQL database responded with multiple responses to request to query: {0}")]
    SingleFloatRequestNoSingleResponse(String),

    #[error("Could not get single heating stats entry from SQL database using query: {0}")]
    SingleHeatingStatsRequestFailure(String),

    #[error("SQL database responded with empty response to query: {0}")]
    SingleHeatingStatsRequestEmptyResponse(String),

    #[error("SQL database responded with multiple responses to request to query: {0}")]
    SingleHeatingStatsRequestNoSingleResponse(String),

    #[error("Could not get feed pattern header from the database with query: {query}")]
    SingleFeedpatternRequestFailure {
        query: String,

        #[source]
        source: MySqlError,
    },

    #[error("SQL database responded with empty response to query: {0}")]
    SingleFeedpatternRequestEmptyResponse(String),

    #[error("SQL database responded with multiple responses to request to query: {0}")]
    SingleFeedpatternRequestNoSingleResponse(String),

    #[error("Could not get schedule entries from SQL database using query: {query}")]
    ScheduleRequestFailure{
        query: String,
        #[source]
        source: MySqlError
    },

    #[error("Could not process the results obtained from SQL database.")]
    ScheduleProcessingFailure{
        #[source]
        source: Box<SqlInterfaceError>
    },

    #[error("ScheduleEntry: Error in conversion of SQL database result to enum ({0}).")]
    ScheduleTypeConversionFailure(String),

    #[error("Could not convert start time string read from SQL database ({0}).")]
    ScheduleStartTimeConversionFailure(String),

    #[error("Could not convert stop time string read from SQL database ({0}).")]
    ScheduleStopTimeConversionFailure(String),

    #[error("Schedule of type {0} could not be found in database.")]
    ScheduleTypeNotFound(String),

    #[error("Error when trying to convert string to timestamp: {timestamp_string}")]
    TimestampConversionFailure {
        timestamp_string: String,
        #[source]
        source: ParseError,
    },

    #[error("Error when inserting heating stats into database using query: {sql_query_string}")]
    InsertHeatingStatsEntryFailure{
        sql_query_string: String,
        #[source]
        source: MySqlError,
    },

    #[error("Error occurred when trying to insert a feed event into database: {source}")]
    InsertFeedEventFailure {
        #[source]
        source: MySqlError
    },

    #[error("Error occurred when trying to insert a refill event into database using query: {sql_query_string}.")]
    InsertRefillEventFailure {
        sql_query_string: String,
        #[source]
        source: MySqlError
    },

    #[error("Error occurred when trying to insert a Balling dosing event into database.")]
    InsertBallingEventFailure {
        #[source]
        source: MySqlError,
    },

    #[error("Database request for retrieving the refill {category} of last {interval} failed.")]
    RefillPastDataRetrievalFailed {
        category: String,
        interval: String,
        #[source]
        source: Box<SqlInterfaceError>,
    },

    #[error("Could not get feed schedule entry from the database using query: {0}")]
    FeedScheduleEntryRequestFailure(String),

    #[error("Could not convert feedschedule entry string(s) read from the database to timestamp")]
    FeedScheduleEntryTimeStampConversionFailure,

    #[error("Error occurred when trying to calculate update of feed schedule entry.")]
    FeedScheduleUpdateFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    #[error("Error occurred when trying to drop a feed schedule entry from database.")]
    FeedScheduleDropFailure {
        #[source]
        source: MySqlError
    },

    #[error("Could not get single Balling set value from the database using query: {0}")]
    SingleBallingSetValRequestFailure(String),

    #[error("SQL database responded with empty response to query: {0}")]
    SingleBallingSetValEmptyResponse(String),

    #[error("SQL database responded with multiple responses to request to query: {0}")]
    SingleBallingSetValNoSingleResponse(String),

    #[error("Error when inserting data frame into database using query: {sql_query_string}")]
    InsertDataFrameFailure {
        sql_query_string: String,

        #[source]
        source: MySqlError,
    },

    #[error("Database responded with a timestamp in the future (delta of {0} seconds).")]
    NegativeTimeDeltaSeconds(i64),

    #[error("Database responded with a negative value ({0}) where a positive value was expected.")]
    NegativeInteger(i64),

    #[error("Could not get heating set values from the database with query: {0}")]
    HeatingSetValsRequestFailure(String),

    #[error("The database returned more than one heating set value entry with query: {0}")]
    HeatingSetValsNoSingleResponse(String),

    #[error("Updating the heating set values failed.")]
    HeatingSetValsUpdateFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    #[error("Updating the ventilation set values failed.")]
    VentilationSetValsUpdateFailure {
        #[source]
        source: Box<SqlInterfaceError>
    },

    #[error("The heating switch on temperature ({0}) is higher than the heating switch off temperature ({1}).")]
    HeatingSetValsInvalid(f32, f32),

    #[error("Could not get ventilation set values from the database with query: {0}")]
    VentilationSetValsRequestFailure(String),

    #[error("The database returned more than one ventilation set value entry with query: {0}")]
    VentilationSetValsNoSingleResponse(String),

    #[error("The ventilation switch on temperature ({0}) is lower than the ventilation switch off temperature ({1}).")]
    VentilationSetValsInvalid(f32, f32),

    #[error("Could not retrieve database name.")]
    DatabaseNameRequestFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    #[error("Could not retrieve date from database.")]
    DateRequestFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    #[error("Could not retrieve timestamp from database.")]
    TimestampRequestFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    #[error("Could not retrieve optional timestamp from database.")]
    OptionalTimestampRequestFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    #[error("Could not ping database.")]
    DatabasePingFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    #[error("Balling set value table may be inconsistent - it contains {0} NULL values.")]
    DatabaseBallingSetValTableContainsNull(i64),

    #[error("Schedule table may be inconsistent - it contains {0} NULL values.")]
    DatabaseScheduleTableContainsNull(i64),

    #[error("Heating set value table may be inconsistent - it contains {0} NULL values.")]
    DatabaseHeatingSetValTableContainsNull(i64),

    #[error("Heating stats table may be inconsistent - it contains {0} NULL values.")]
    DatabaseHeatingStatsTableContainsNull(i64),

    #[error("Ventilation set value table may be inconsistent - it contains {0} NULL values.")]
    DatabaseVentilationSetValTableContainsNull(i64),

    #[error("Feed set value table {0} may be inconsistent - it contains {1} NULL values.")]
    DatabaseFeedSetValTableContainsNull(String, i64),

    #[error("Could not check balling set value table for NULL values.")]
    DatabaseCheckBallingSetValsFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    #[error("Could not check schedule table for NULL values.")]
    DatabaseCheckScheduleNullFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    #[error("Could not check heating set value table for NULL values.")]
    DatabaseCheckHeatingSetValsFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    #[error("Could not check heating stats table for NULL values.")]
    DatabaseCheckHeatingStatsFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    #[error("Could not check ventilation set value table for NULL values.")]
    DatabaseCheckVentilationSetValsFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    #[error("Could not check feed set value table {table_name} for NULL values.")]
    DatabaseCheckFeedSetValsFailure {
        table_name: String,
        #[source]
        source: Box<SqlInterfaceError>,
    },

    #[cfg(test)]
    #[error("Error when inserting heating set values into database using query: {0}")]
    InsertHeatingSetValuesFailure(String),

    #[cfg(test)]
    #[error("Error when inserting ventilation set values into database using query: {0}")]
    InsertVentilationSetValuesFailure(String),

    #[cfg(test)]
    #[error("Error occurred when trying to truncate table {0} of SQL database.")]
    TruncateTableFailure(String),

    #[cfg(test)]
    #[error("This is a mock error generated for testing purposes in {0}.")]
    MockError(String),

    #[cfg(test)]
    #[error("Error when inserting Balling pump configuration into database using query: {0}")]
    InsertBallingPumpConfigurationFailure(String),

    #[cfg(test)]
    #[error("Database responded with empty response to queries: {0} {1}")]
    SingleDataFrameRequestEmptyResponse(String, String),

    #[cfg(test)]
    #[error("Database responded with multiple responses to queries: {0} {1}")]
    SingleDataFrameRequestNoSingleResponse(String, String),
}
