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
        "Could not get a connection from the pool. The database may be down or the pool exhausted."
    )]
    ConnectionFromPool,

    /// The database wait_timeout is less than the required minimum.
    #[error("The database wait_timeout ({0}s) is less than the required minimum ({1}s).")]
    WaitTimeoutTooLow(u64, u64),

    /// Could not parse wait_timeout value from the database
    #[error("Could not parse wait_timeout value from the database: {0}")]
    WaitTimeoutInvalid(i64),

    /// Could not establish database connection pool
    #[error("Could not establish database connection pool with {0}")]
    ConnectionPoolFailure(String),

    /// Required tables are not existing in the database.
    #[error("Required tables ({0}) are not existing in the database ({1}).")]
    RequiredTablesNotExisting(String, String),

    /// Error when reading tables from the database, expected more than one result.
    #[error("Error when reading tables from the database, expected more than one result.")]
    ShowTablesWithoutProperResult,

    /// Could not get a single string from the database.
    #[error("Could not get a single string from the database using query: {0}")]
    SingleVersionRequestFailure(String),

    /// Database responded with empty response.
    #[error("database responded with empty response to query: {0}")]
    SingleVersionRequestNotListed(String),

    /// The database responded with multiple responses to request.
    #[error("database responded with multiple responses to request to query: {0}")]
    SingleVersionRequestMultipleResults(String),

    /// Could not get a string array from the database.
    #[error("Could not get a string array from the database using query: {0}")]
    SingleStringRequestFailure(String),

    /// database responded with multiple responses to request.
    #[error("database responded with multiple responses to request to query: {0}")]
    SingleStringRequestNoSingleResponse(String),

    /// database responded with empty response to request.
    #[error("database responded with empty response to request to query: {0}")]
    SingleStringRequestEmptyResponse(String),

    /// database responded with empty response.
    #[error("database responded with empty response to query: {0}")]
    MultipleStringRequestEmptyResponse(String),

    /// The database responded with single row response.
    #[error("database responded with single row response to query: {0}")]
    MultipleStringRequestSingleResponse(String),

    /// Could not get a single integer from the database.
    #[error("Could not get a single integer from the database using query: {0}")]
    SingleIntegerRequestFailure(String),

    /// The database responded with multiple responses.
    #[error("database responded with multiple responses to request to query: {0}")]
    SingleIntegerRequestNoSingleResponse(String),

    /// Could not get a single float value from the database.
    #[error("Could not get single float value from the database using query: {0}")]
    SingleFloatRequestFailure(String),

    /// database responded with empty response.
    #[error("database responded with empty response to query: {0}")]
    SingleFloatRequestEmptyResponse(String),

    /// database responded with multiple responses to request.
    #[error("database responded with multiple responses to request to query: {0}")]
    SingleFloatRequestNoSingleResponse(String),

    /// Could not get single heating stats entry from the database.
    #[error("Could not get single heating stats entry from database using query: {0}")]
    SingleHeatingStatsRequestFailure(String),

    /// database responded with empty response.
    #[error("database responded with empty response to query: {0}")]
    SingleHeatingStatsRequestEmptyResponse(String),

    /// database responded with multiple responses to request.
    #[error("database responded with multiple responses to request to query: {0}")]
    SingleHeatingStatsRequestNoSingleResponse(String),

    /// Could not get feed pattern header from the database
    #[error("Could not get feed pattern header from the database with query: {query}")]
    SingleFeedpatternRequestFailure {
        query: String,

        #[source]
        source: MySqlError,
    },

    /// database responded with empty response.
    #[error("database responded with empty response to query: {0}")]
    SingleFeedpatternRequestEmptyResponse(String),

    /// The database responded with multiple responses.
    #[error("database responded with multiple responses to request to query: {0}")]
    SingleFeedpatternRequestNoSingleResponse(String),

    /// Could not get schedule entries from the database.
    #[error("Could not get schedule entries from database using query: {query}")]
    ScheduleRequestFailure {
        query: String,
        #[source]
        source: MySqlError,
    },

    /// Could not process the results obtained from the database.
    #[error("Could not process the results obtained from database.")]
    ScheduleProcessingFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Error in conversion of the database result to schedule entry.
    #[error("Error in conversion of database result to schedule entry ({0}).")]
    ScheduleTypeConversionFailure(String),

    /// Could not convert start time string read from the database.
    #[error("Could not convert start time string read from database ({0}).")]
    ScheduleStartTimeConversionFailure(String),

    /// Could not convert stop time string read from the database
    #[error("Could not convert stop time string read from database ({0}).")]
    ScheduleStopTimeConversionFailure(String),

    /// Type-specific schedule could not be found in the database.
    #[error("Schedule of type {0} could not be found in database.")]
    ScheduleTypeNotFound(String),

    /// Error when trying to convert string to timestamp.
    #[error("Error when trying to convert string to timestamp: {timestamp_string}")]
    TimestampConversionFailure {
        timestamp_string: String,
        #[source]
        source: ParseError,
    },

    /// Error when inserting heating stats into the database.
    #[error(
        "Error when inserting heating stats into the database using query: {sql_query_string}"
    )]
    InsertHeatingStatsEntryFailure {
        sql_query_string: String,
        #[source]
        source: MySqlError,
    },

    /// Error occurred when trying to insert a feed event into the database.
    #[error("Error occurred when trying to insert a feed event into the database: {source}")]
    InsertFeedEventFailure {
        #[source]
        source: MySqlError,
    },

    /// Error occurred when trying to insert a refill event into the database.
    #[error("Error occurred when trying to insert a refill event into the database using query: {sql_query_string}.")]
    InsertRefillEventFailure {
        sql_query_string: String,
        #[source]
        source: MySqlError,
    },

    /// Error occurred when trying to insert a Balling dosing event into the database.
    #[error("Error occurred when trying to insert a Balling dosing event into database.")]
    InsertBallingEventFailure {
        #[source]
        source: MySqlError,
    },

    /// Database request for retrieving the refill history failed.
    #[error("Database request for retrieving the refill {category} of last {interval} failed.")]
    RefillPastDataRetrievalFailed {
        category: String,
        interval: String,
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not get feed schedule entry from the database.
    #[error("Could not get feed schedule entry from the database using query: {0}")]
    FeedScheduleEntryRequestFailure(String),

    /// Could not convert feedschedule entry string(s) read from the database to timestamp.
    #[error("Could not convert feedschedule entry string(s) read from the database to timestamp.")]
    FeedScheduleEntryTimeStampConversionFailure {
        #[source]
        source: ParseError,
    },

    /// Error occurred when processing time stamp results from the database.
    #[error("Error occurred when processing {number_entries} results from the database.")]
    FeedScheduleEntryTimeStampProcessingFailure {
        number_entries: usize,
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Error occurred when trying to calculate update of feed schedule entry.
    #[error("Error occurred when trying to calculate update of feed schedule entry.")]
    FeedScheduleUpdateFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Error occurred when trying to drop a feed schedule entry from the database.
    #[error("Error occurred when trying to drop a feed schedule entry from the database.")]
    FeedScheduleDropFailure {
        #[source]
        source: MySqlError,
    },

    // The database responded with one or even two negative values for feed phase/feed pause duration.
    #[error(
        "Database responded with negative value(s) for feed phase/feed pause duration: {0} {1}"
    )]
    DatabaseFeedPhaseDurationNegative(i16, i16),

    /// Could not get a single Balling set value from the database.
    #[error("Could not get a single Balling set value from the database using query: {0}")]
    SingleBallingSetValRequestFailure(String),

    /// database responded with empty response.
    #[error("database responded with empty response to query: {0}")]
    SingleBallingSetValEmptyResponse(String),

    /// The database responded with multiple responses.
    #[error("database responded with multiple responses to request to query: {0}")]
    SingleBallingSetValNoSingleResponse(String),

    /// Error occurred when inserting data frame into the database.
    #[error("Error occurred when inserting data frame into the database using query: {sql_query_string}")]
    InsertDataFrameFailure {
        sql_query_string: String,

        #[source]
        source: MySqlError,
    },

    /// The database responded with a timestamp in the future.
    #[error("Database responded with a timestamp in the future (delta of {0} seconds).")]
    NegativeTimeDeltaSeconds(i64),

    /// The database responded with a negative value ({0}) where a positive value was expected
    #[error("Database responded with a negative value ({0}) where a positive value was expected.")]
    NegativeInteger(i64),

    /// Could not get heating set values from the database.
    #[error("Could not get heating set values from the database with query: {0}")]
    HeatingSetValsRequestFailure(String),

    /// The database returned more than one heating set value entry.
    #[error("The database returned more than one heating set value entry with query: {0}")]
    HeatingSetValsNoSingleResponse(String),

    /// Updating the heating set values failed.
    #[error("Updating the heating set values failed.")]
    HeatingSetValsUpdateFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Updating the ventilation set values failed.
    #[error("Updating the ventilation set values failed.")]
    VentilationSetValsUpdateFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// The heating switch on temperature is higher than the heating switch-off temperature.
    #[error("The heating switch on temperature ({0}) is higher than the heating switch-off temperature ({1}).")]
    HeatingSetValsInvalid(f32, f32),

    /// Could not get ventilation set values from the database.
    #[error("Could not get ventilation set values from the database with query: {0}")]
    VentilationSetValsRequestFailure(String),

    /// The database returned more than one ventilation set value entry.
    #[error("The database returned more than one ventilation set value entry with query: {0}")]
    VentilationSetValsNoSingleResponse(String),

    /// The ventilation switch on temperature is lower than the ventilation switch-off temperature.
    #[error("The ventilation switch on temperature ({0}) is lower than the ventilation switch-off temperature ({1}).")]
    VentilationSetValsInvalid(f32, f32),

    /// Could not retrieve database name.
    #[error("Could not retrieve database name.")]
    DatabaseNameRequestFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not retrieve date from the database.
    #[error("Could not retrieve date from the database.")]
    DateRequestFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not retrieve timestamp from the database.
    #[error("Could not retrieve timestamp from database.")]
    TimestampRequestFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not retrieve optional timestamp from the database.
    #[error("Could not retrieve optional timestamp from database.")]
    OptionalTimestampRequestFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not ping database.
    #[error("Could not ping database.")]
    DatabasePingFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Balling set value table may be inconsistent - it contains NULL values.
    #[error("Balling set value table may be inconsistent - it contains {0} NULL values.")]
    DatabaseBallingSetValTableContainsNull(i64),

    /// Balling set value table may be inconsistent - invalid answer from database to query.
    #[error("Balling set value table may be inconsistent - database responded with negative value(s) ({0}, {1}, {2}).")]
    DatabaseBallingSetValTableNegativeValue(i64, i64, i64),

    /// Balling set value table contains too many rows.
    #[error("Balling set value table contains too many rows ({0}). Permissible maximum is {1}.")]
    DatabaseBallingSetValTableContainsTooManyRows(u64, u64),

    /// Balling dosing log table contains too many rows.
    #[error("Balling dosing log table contains too many rows ({0}). Permissible maximum is {1}.")]
    DatabaseBallingDosingLogTableContainsTooManyRows(u64, u64),

    /// Schedule table may be inconsistent - it contains NULL values.
    #[error("Schedule table may be inconsistent - it contains {0} NULL values.")]
    DatabaseScheduleTableContainsNull(i64),

    /// Schedule table may be inconsistent - invalid answer from database to query.
    #[error("Schedule table may be inconsistent - database responded with negative value(s) ({0}, {1}).")]
    DatabaseScheduleTableNegativeValue(i64, i64),

    /// The schedule table contains too many rows.
    #[error("Schedule table contains too many rows ({0}). Permissible maximum is {1}.")]
    DatabaseScheduleTableContainsTooManyRows(u64, u64),

    /// Data table may be inconsistent - invalid answer from database to query.
    #[error("Could not check data table for number of rows.")]
    DatabaseCheckDataFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Data table may be inconsistent - invalid answer from database to query.
    #[error("Data table may be inconsistent - database responded with negative value ({0}).")]
    DatabaseDataTableNegativeValue(i64),

    /// The data table contains too many rows.
    #[error("Data table contains too many rows ({0}). Permissible maximum is {1}.")]
    DatabaseDataTableContainsTooManyRows(u64, u64),

    /// Heating set value table may be inconsistent - it contains NULL values.
    #[error("Heating set value table may be inconsistent - it contains {0} NULL values.")]
    DatabaseHeatingSetValTableContainsNull(i64),

    /// Heating set value table may be inconsistent - invalid answer from database to query.
    #[error("Heating set value table may be inconsistent - database responded with negative value(s) ({0}, {1}).")]
    DatabaseHeatingSetValTableNegativeValue(i64, i64),

    /// Heating set value table contains too many rows.
    #[error("Heating set value table contains too many rows ({0}). Permissible maximum is 1.")]
    DatabaseHeatingSetValTableContainsTooManyRows(u64),

    /// Heating stats table may be inconsistent - it contains NULL values.
    #[error("Heating stats table may be inconsistent - it contains {0} NULL values.")]
    DatabaseHeatingStatsTableContainsNull(i64),

    /// Heating statistics table may be inconsistent - invalid answer from database to query.
    #[error("Heating set value table may be inconsistent - database responded with negative value(s) ({0}, {1}).")]
    DatabaseHeatingStatsTableNegativeValue(i64, i64),

    /// Heating stats table contains too many rows.
    #[error("Heating statistics table contains too many rows ({0}). Permissible maximum is {1}.")]
    DatabaseHeatingStatsTableContainsTooManyRows(u64, u64),

    /// Ventilation set value table may be inconsistent - it contains NULL values.
    #[error("Ventilation set value table may be inconsistent - it contains {0} NULL values.")]
    DatabaseVentilationSetValTableContainsNull(i64),

    /// Ventilation set value table may be inconsistent - invalid answer from database to query.
    #[error("Ventilation set value table may be inconsistent - database responded with negative value(s) ({0}, {1}).")]
    DatabaseVentilationSetValTableNegativeValue(i64, i64),

    /// Ventilation set value table contains too many rows.
    #[error("Ventilation set value table contains too many rows ({0}). Permissible maximum is 1.")]
    DatabaseVentilationSetValTableContainsTooManyRows(u64),

    /// Refill table may be inconsistent - invalid answer from database to query.
    #[error("Refill table may be inconsistent - database responded with negative value ({0}).")]
    DatabaseRefillTableNegativeValue(i64),

    /// Refill table contains too many rows.
    #[error("Refill table contains too many rows ({0}). Permissible maximum is {1}.")]
    DatabaseRefillTableContainsTooManyRows(u64, u64),

    /// Feed set value table may be inconsistent - it contains NULL values.
    #[error("Feed set value table {0} may be inconsistent - it contains {1} NULL values.")]
    DatabaseFeedSetValTableContainsNull(String, i64),

    /// Feed set value table contains too many rows.
    #[error("Feed table {0} contains too many rows ({1}). Permissible maximum is {2}.")]
    DatabaseFeedTableContainsTooManyRows(String, u64, u64),

    /// Could not check balling set value table for NULL values or number of rows.
    #[error(
        "Could not check balling set value table (existence of NULL values and number of rows)."
    )]
    DatabaseCheckBallingSetValsFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not check schedule table for NULL values or number of rows.
    #[error("Could not check schedule table for NULL values or number of rows.")]
    DatabaseCheckScheduleFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not check heating set value table for NULL values or number of rows.
    #[error("Could not check heating set value table for NULL values or number of rows.")]
    DatabaseCheckHeatingSetValsFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not check heating stats table for NULL values or number of rows.
    #[error("Could not check heating stats table for NULL values or number of rows.")]
    DatabaseCheckHeatingStatsFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not check ventilation set value table for NULL values or number of rows.
    #[error("Could not check ventilation set value table for NULL values or number of rows.")]
    DatabaseCheckVentilationSetValsFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not check refill table for number of rows.
    #[error("Could not check refill table for number of rows.")]
    DatabaseCheckRefillFailure {
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// Could not check feed set value table for NULL values or number of rows.
    #[error(
        "Could not check feed set value table {table_name} for NULL values or number of rows."
    )]
    DatabaseCheckFeedSetValsFailure {
        table_name: String,
        #[source]
        source: Box<SqlInterfaceError>,
    },

    /// A feed set value table may be inconsistent - invalid answer from database to query.
    #[error("Feed set value table {0} may be inconsistent - database responded with negative value(s) ({1}, {2}).")]
    DatabaseFeedTableNegativeValue(String, i64, i64),

    #[cfg(test)]
    #[error("Error when inserting heating set values into database using query: {0}")]
    InsertHeatingSetValuesFailure(String),

    #[cfg(test)]
    #[error("Error when inserting ventilation set values into database using query: {0}")]
    InsertVentilationSetValuesFailure(String),

    #[cfg(test)]
    #[error("Error occurred when trying to truncate table {0} of database.")]
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
