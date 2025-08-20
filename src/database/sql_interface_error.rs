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
    /// Could not get a connection from the pool. The database may be down or the pool exhausted.
    #[error(
        "[{location}] Could not get a connection from the pool. The database may be down or the pool exhausted."
    )]
    ConnectionFromPool {
        location: String,

        #[source]
        source: mysql::Error
    },

    /// The database wait_timeout is less than the required minimum.
    #[error("[{0}] The database wait_timeout ({1}s) is less than the required minimum ({2}s).")]
    WaitTimeoutTooLow(String, u64, u64),

    /// Could not parse wait_timeout value from the database
    #[error("[{0}] Could not parse wait_timeout value from the database: {0}")]
    WaitTimeoutInvalid(String, i64),

    /// Could not establish database connection pool
    #[error("[{location}] Could not establish database connection pool with {url}")]
    ConnectionPoolFailure {
        location: String,
        url: String,

        #[source]
        source: mysql::Error
    },

    /// Required tables are not existing in the database.
    #[error("[{0}] Required tables ({1}) are not existing in the database ({2}).")]
    RequiredTablesNotExisting(String, String, String),

    /// Error when reading tables from the database, expected more than one result.
    #[error("[{0}] Error when reading tables from the database, expected more than one result.")]
    ShowTablesWithoutProperResult(String),

    /// Could not get a single string from the database.
    #[error("[{0}] Could not get a single string from the database using query: {1}")]
    SingleVersionRequestFailure(String, String),

    /// Database responded with empty response.
    #[error("[{0}] Database responded with empty response to query: {1}")]
    SingleVersionRequestNotListed(String, String),

    /// The database responded with multiple responses to request.
    #[error("[{0}] Database responded with multiple responses to request to query: {1}")]
    SingleVersionRequestMultipleResults(String, String),

    /// Could not get a string array from the database.
    #[error("[{0}] Could not get a string array from the database using query: {1}")]
    SingleStringRequestFailure(String, String),

    /// database responded with multiple responses to request.
    #[error("[{0}] database responded with multiple responses to request to query: {1}")]
    SingleStringRequestNoSingleResponse(String, String),

    /// database responded with empty response to request.
    #[error("[{0}] database responded with empty response to request to query: {1}")]
    SingleStringRequestEmptyResponse(String, String),

    /// database responded with empty response.
    #[error("[{0}] Database responded with empty response to query: {1}")]
    MultipleStringRequestEmptyResponse(String, String),

    /// The database responded with single row response.
    #[error("[{0}] Database responded with single row response to query: {1}")]
    MultipleStringRequestSingleResponse(String, String),

    /// Could not get a single integer from the database.
    #[error("[{0}] Could not get a single integer from the database using query: {1}")]
    SingleIntegerRequestFailure(String, String),

    /// The database responded with multiple responses.
    #[error("[{0}] Database responded with multiple responses to request to query: {1}")]
    SingleIntegerRequestNoSingleResponse(String, String),

    /// Could not get a single float value from the database.
    #[error("[{0}] Could not get single float value from the database using query: {1}")]
    SingleFloatRequestFailure(String, String),

    /// database responded with empty response.
    #[error("[{0}] Database responded with empty response to query: {1}")]
    SingleFloatRequestEmptyResponse(String, String),

    /// database responded with multiple responses to request.
    #[error("[{0}] Database responded with multiple responses to request to query: {1}")]
    SingleFloatRequestNoSingleResponse(String, String),

    /// Could not get single heating stats entry from the database.
    #[error("[{0}] Could not get single heating stats entry from database using query: {1}")]
    SingleHeatingStatsRequestFailure(String, String),

    /// database responded with empty response.
    #[error("[{0}] Database responded with empty response to query: {1}")]
    SingleHeatingStatsRequestEmptyResponse(String, String),

    /// database responded with multiple responses to request.
    #[error("[{0}] Database responded with multiple responses to request to query: {1}")]
    SingleHeatingStatsRequestNoSingleResponse(String, String),

    /// Could not get feed pattern header from the database
    #[error(
        "[{location}] Could not get feed pattern header from the database with query: {query}"
    )]
    SingleFeedpatternRequestFailure {
        location: String,
        query: String,

        #[source]
        source: MySqlError,
    },

    /// database responded with empty response.
    #[error("[{0}] Database responded with empty response to query: {1}")]
    SingleFeedpatternRequestEmptyResponse(String, String),

    /// The database responded with multiple responses.
    #[error("[{0}] Database responded with multiple responses to request to query: {1}")]
    SingleFeedpatternRequestNoSingleResponse(String, String),

    /// Could not get schedule entries from the database.
    #[error("[{location}] Could not get schedule entries from database using query: {query}")]
    ScheduleRequestFailure {
        location: String,
        query: String,
        #[source]
        source: MySqlError,
    },

    /// Could not process the results obtained from the database.
    #[error("[{location}] Could not process the results obtained from database.")]
    ScheduleProcessingFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Error in conversion of the database result to schedule entry.
    #[error("[{0}] Error in conversion of database result to schedule entry ({1}).")]
    ScheduleTypeConversionFailure(String, String),

    /// Could not convert start time string read from the database.
    #[error("[{0}] Could not convert start time string read from database ({1}).")]
    ScheduleStartTimeConversionFailure(String, String),

    /// Could not convert stop time string read from the database
    #[error("[{0}] Could not convert stop time string read from database ({1}).")]
    ScheduleStopTimeConversionFailure(String, String),

    /// Type-specific schedule could not be found in the database.
    #[error("[{0}] Schedule of type {1} could not be found in database.")]
    ScheduleTypeNotFound(String, String),

    /// Error when trying to convert string to timestamp.
    #[error("[{location}] Error when trying to convert string to timestamp: {timestamp_string}")]
    TimestampConversionFailure {
        location: String,
        timestamp_string: String,
        #[source]
        source: ParseError,
    },

    /// Error when inserting heating stats into the database.
    #[error(
        "[{location}] Error when inserting heating stats into the database using query: {sql_query_string}"
    )]
    InsertHeatingStatsEntryFailure {
        location: String,
        sql_query_string: String,
        #[source]
        source: MySqlError,
    },

    /// Error occurred when trying to insert a feed event into the database.
    #[error("[{location}] Error occurred when trying to insert a feed event into the database: {source}")]
    InsertFeedEventFailure {
        location: String,

        #[source]
        source: MySqlError,
    },

    /// Error occurred when trying to insert a refill event into the database.
    #[error("[{location}] Error occurred when trying to insert a refill event into the database using query: {sql_query_string}.")]
    InsertRefillEventFailure {
        location: String,
        sql_query_string: String,
        #[source]
        source: MySqlError,
    },

    /// Error occurred when trying to insert a Balling dosing event into the database.
    #[error(
        "[{location}] Error occurred when trying to insert a Balling dosing event into database."
    )]
    InsertBallingEventFailure {
        location: String,
        #[source]
        source: MySqlError,
    },

    /// Database request for retrieving the refill history failed.
    #[error("[{location}] Database request for retrieving the refill {category} of last {interval} failed.")]
    RefillPastDataRetrievalFailed {
        location: String,
        category: String,
        interval: String,
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not get feed schedule entry from the database.
    #[error("[{location}] Could not get feed schedule entry from the database using query: {query}")]
    FeedScheduleEntryRequestFailure {
        location: String,
        query: String,

        #[source]
        source: MySqlError
    },

    /// Could not convert feedschedule entry string(s) read from the database to timestamp.
    #[error("[{location}] Could not convert feedschedule entry string(s) read from the database to timestamp.")]
    FeedScheduleEntryTimeStampConversionFailure {
        location: String,
        #[source]
        source: ParseError,
    },

    /// Error occurred when processing time stamp results from the database.
    #[error(
        "[{location}] Error occurred when processing {number_entries} results from the database."
    )]
    FeedScheduleEntryTimeStampProcessingFailure {
        location: String,
        number_entries: usize,
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Error occurred when trying to calculate update of feed schedule entry.
    #[error("[{location}] Error occurred when trying to calculate update of feed schedule entry.")]
    FeedScheduleUpdateFailure {
        location: String,
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Error occurred when trying to drop a feed schedule entry from the database.
    #[error(
        "[{location}] Error occurred when trying to drop a feed schedule entry from the database."
    )]
    FeedScheduleDropFailure {
        location: String,
        #[source]
        source: MySqlError,
    },

    // The database responded with one or even two negative values for feed phase/feed pause duration.
    #[error(
        "[{0}] Database responded with negative value(s) for feed phase/feed pause duration: {1} {2}"
    )]
    DatabaseFeedPhaseDurationNegative(String, i16, i16),

    /// Could not get a single Balling set value from the database.
    #[error("[{location}] Could not get a single Balling set value from the database using query: {query}")]
    SingleBallingSetValRequestFailure {
        location: String,
        query: String,

        #[source]
        source: MySqlError
    },

    /// database responded with empty response.
    #[error("[{0}] Database responded with empty response to query: {1}")]
    SingleBallingSetValEmptyResponse(String, String),

    /// The database responded with multiple responses.
    #[error("[{0}] Database responded with multiple responses to request to query: {1}")]
    SingleBallingSetValNoSingleResponse(String, String),

    /// Error occurred when inserting data frame into the database.
    #[error("[{location}] Error occurred when inserting data frame into the database using query: {sql_query_string}")]
    InsertDataFrameFailure {
        location: String,
        sql_query_string: String,

        #[source]
        source: MySqlError,
    },

    /// The database responded with a timestamp in the future.
    #[error("[{0}] Database responded with a timestamp in the future (delta of {1} seconds).")]
    NegativeTimeDeltaSeconds(String, i64),

    /// The database responded with a negative value ({0}) where a positive value was expected
    #[error(
        "[{0}] Database responded with a negative value ({1}) where a positive value was expected."
    )]
    NegativeInteger(String, i64),

    /// Could not get heating set values from the database.
    #[error("[{location}] Could not get heating set values from the database with query: {query}")]
    HeatingSetValsRequestFailure {
        location: String,
        query: String,

        #[source]
        source: MySqlError,
    },

    /// The database returned more than one heating set value entry.
    #[error("[{0}] The database returned more than one heating set value entry with query: {1}")]
    HeatingSetValsNoSingleResponse(String, String),

    /// Updating the heating set values failed.
    #[error("[{location}] Updating the heating set values failed.")]
    HeatingSetValsUpdateFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Updating the ventilation set values failed.
    #[error("[{location}] Updating the ventilation set values failed.")]
    VentilationSetValsUpdateFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// The heating switch on temperature is higher than the heating switch-off temperature.
    #[error("[{0}] The heating switch on temperature ({1}) is higher than the heating switch-off temperature ({2}).")]
    HeatingSetValsInvalid(String, f32, f32),

    /// Could not get ventilation set values from the database.
    #[error("[{0}] Could not get ventilation set values from the database with query: {1}")]
    VentilationSetValsRequestFailure(String, String),

    /// The database returned more than one ventilation set value entry.
    #[error(
        "[{0}] The database returned more than one ventilation set value entry with query: {1}"
    )]
    VentilationSetValsNoSingleResponse(String, String),

    /// The ventilation switch on temperature is lower than the ventilation switch-off temperature.
    #[error("[{0}] The ventilation switch on temperature ({1}) is lower than the ventilation switch-off temperature ({2}).")]
    VentilationSetValsInvalid(String, f32, f32),

    /// Could not retrieve database name.
    #[error("[{location}] Could not retrieve database name.")]
    DatabaseNameRequestFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not retrieve date from the database.
    #[error("[{location}] Could not retrieve date from the database.")]
    DateRequestFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not retrieve timestamp from the database.
    #[error("[{location}] Could not retrieve timestamp from database.")]
    TimestampRequestFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not retrieve optional timestamp from the database.
    #[error("[{location}] Could not retrieve optional timestamp from database.")]
    OptionalTimestampRequestFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not ping database.
    #[error("[{location}] Could not ping database.")]
    DatabasePingFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Balling set value table may be inconsistent - it contains NULL values.
    #[error("[{0}] Balling set value table may be inconsistent - it contains {1} NULL values.")]
    DatabaseBallingSetValTableContainsNull(String, i64),

    /// Balling set value table may be inconsistent - invalid answer from database to query.
    #[error("[{0}] Balling set value table may be inconsistent - database responded with negative value(s) ({1}, {2}, {3}).")]
    DatabaseBallingSetValTableNegativeValue(String, i64, i64, i64),

    /// Balling set value table contains too many rows.
    #[error(
        "[{0}] Balling set value table contains too many rows ({1}). Permissible maximum is {2}."
    )]
    DatabaseBallingSetValTableContainsTooManyRows(String, u64, u64),

    /// Balling dosing log table contains too many rows.
    #[error(
        "[{0}] Balling dosing log table contains too many rows ({1}). Permissible maximum is {2}."
    )]
    DatabaseBallingDosingLogTableContainsTooManyRows(String, u64, u64),

    /// Schedule table may be inconsistent - it contains NULL values.
    #[error("[{0}] Schedule table may be inconsistent - it contains {1} NULL values.")]
    DatabaseScheduleTableContainsNull(String, i64),

    /// Schedule table may be inconsistent - invalid answer from database to query.
    #[error("[{0}] Schedule table may be inconsistent - database responded with negative value(s) ({1}, {2}).")]
    DatabaseScheduleTableNegativeValue(String, i64, i64),

    /// The schedule table contains too many rows.
    #[error("[{0}] Schedule table contains too many rows ({1}). Permissible maximum is {2}.")]
    DatabaseScheduleTableContainsTooManyRows(String, u64, u64),

    /// Data table may be inconsistent - invalid answer from database to query.
    #[error("[{location}] Could not check data table for number of rows.")]
    DatabaseCheckDataFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Data table may be inconsistent - invalid answer from database to query.
    #[error(
        "[{0}] Data table may be inconsistent - database responded with negative value ({1})."
    )]
    DatabaseDataTableNegativeValue(String, i64),

    /// The data table contains too many rows.
    #[error("[{0}] Data table contains too many rows ({1}). Permissible maximum is {2}.")]
    DatabaseDataTableContainsTooManyRows(String, u64, u64),

    /// Heating set value table may be inconsistent - it contains NULL values.
    #[error("[{0}] Heating set value table may be inconsistent - it contains {1} NULL values.")]
    DatabaseHeatingSetValTableContainsNull(String, i64),

    /// Heating set value table may be inconsistent - invalid answer from database to query.
    #[error("[{0}] Heating set value table may be inconsistent - database responded with negative value(s) ({1}, {2}).")]
    DatabaseHeatingSetValTableNegativeValue(String, i64, i64),

    /// Heating set value table contains too many rows.
    #[error(
        "[{0}] Heating set value table contains too many rows ({1}). Permissible maximum is 1."
    )]
    DatabaseHeatingSetValTableContainsTooManyRows(String, u64),

    /// Heating stats table may be inconsistent - it contains NULL values.
    #[error("[{0}] Heating stats table may be inconsistent - it contains {1} NULL values.")]
    DatabaseHeatingStatsTableContainsNull(String, i64),

    /// Heating statistics table may be inconsistent - invalid answer from database to query.
    #[error("[{0}] Heating set value table may be inconsistent - database responded with negative value(s) ({1}, {2}).")]
    DatabaseHeatingStatsTableNegativeValue(String, i64, i64),

    /// Heating stats table contains too many rows.
    #[error(
        "[{0}] Heating statistics table contains too many rows ({1}). Permissible maximum is {2}."
    )]
    DatabaseHeatingStatsTableContainsTooManyRows(String, u64, u64),

    /// Ventilation set value table may be inconsistent - it contains NULL values.
    #[error(
        "[{0}] Ventilation set value table may be inconsistent - it contains {1} NULL values."
    )]
    DatabaseVentilationSetValTableContainsNull(String, i64),

    /// Ventilation set value table may be inconsistent - invalid answer from database to query.
    #[error("[{0}] Ventilation set value table may be inconsistent - database responded with negative value(s) ({1}, {2}).")]
    DatabaseVentilationSetValTableNegativeValue(String, i64, i64),

    /// Ventilation set value table contains too many rows.
    #[error(
        "[{0}] Ventilation set value table contains too many rows ({1}). Permissible maximum is 1."
    )]
    DatabaseVentilationSetValTableContainsTooManyRows(String, u64),

    /// Refill table may be inconsistent - invalid answer from database to query.
    #[error(
        "[{0}] Refill table may be inconsistent - database responded with negative value ({1})."
    )]
    DatabaseRefillTableNegativeValue(String, i64),

    /// Refill table contains too many rows.
    #[error("[{0}] Refill table contains too many rows ({1}). Permissible maximum is {2}.")]
    DatabaseRefillTableContainsTooManyRows(String, u64, u64),

    /// Feed set value table may be inconsistent - it contains NULL values.
    #[error("[{0}] Feed set value table {1} may be inconsistent - it contains {2} NULL values.")]
    DatabaseFeedSetValTableContainsNull(String, String, i64),

    /// Feed set value table contains too many rows.
    #[error("[{0}] Feed table {1} contains too many rows ({2}). Permissible maximum is {3}.")]
    DatabaseFeedTableContainsTooManyRows(String, String, u64, u64),

    /// Could not check balling set value table for NULL values or number of rows.
    #[error(
        "[{location}] Could not check balling set value table (existence of NULL values and number of rows)."
    )]
    DatabaseCheckBallingSetValsFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not check schedule table for NULL values or number of rows.
    #[error("[{location}] Could not check schedule table for NULL values or number of rows.")]
    DatabaseCheckScheduleFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not check heating set value table for NULL values or number of rows.
    #[error(
        "[{location}] Could not check heating set value table for NULL values or number of rows."
    )]
    DatabaseCheckHeatingSetValsFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not check heating stats table for NULL values or number of rows.
    #[error("[{location}] Could not check heating stats table for NULL values or number of rows.")]
    DatabaseCheckHeatingStatsFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not check ventilation set value table for NULL values or number of rows.
    #[error("[{location}] Could not check ventilation set value table for NULL values or number of rows.")]
    DatabaseCheckVentilationSetValsFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not check refill table for number of rows.
    #[error("[{location}] Could not check refill table for number of rows.")]
    DatabaseCheckRefillFailure {
        location: String,

        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not check feed set value table for NULL values or number of rows.
    #[error(
        "[{location}] Could not check feed set value table {table_name} for NULL values or number of rows."
    )]
    DatabaseCheckFeedSetValsFailure {
        location: String,
        table_name: String,
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// A feed set value table may be inconsistent - invalid answer from database to query.
    #[error("[{0}] Feed set value table {1} may be inconsistent - database responded with negative value(s) ({2}, {3}).")]
    DatabaseFeedTableNegativeValue(String, String, i64, i64),

    #[cfg(test)]
    #[error("[{location}] Error when inserting heating set values into database using query: {query}")]
    InsertHeatingSetValuesFailure {
        location: String,
        query: String,

        #[source]
        source: MySqlError,
    },

    #[cfg(test)]
    #[error("[{0}] Error when inserting ventilation set values into database using query: {1}")]
    InsertVentilationSetValuesFailure(String, String),

    #[cfg(test)]
    #[error("[{0}] Error occurred when trying to truncate table {1} of database.")]
    TruncateTableFailure(String, String),

    #[cfg(test)]
    #[error("[{0}] This is a mock error generated for testing purposes.")]
    MockError(String),

    #[cfg(test)]
    #[error(
        "[{0}] Error when inserting Balling pump configuration into database using query: {1}"
    )]
    InsertBallingPumpConfigurationFailure(String, String),

    #[cfg(test)]
    #[error("[{0}] Database responded with empty response to queries: {1} {2}")]
    SingleDataFrameRequestEmptyResponse(String, String, String),

    #[cfg(test)]
    #[error("[{0}] Database responded with multiple responses to queries: {1} {2}")]
    SingleDataFrameRequestNoSingleResponse(String, String, String),
}
