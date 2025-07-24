/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use thiserror::Error;

#[derive(PartialEq, Debug, Error)]
/// Enum codifies the errors which the program may encounter in communication with the DB
pub enum SqlInterfaceError {
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

    #[error("Could not get feed pattern header from the database with query: {0}")]
    SingleFeedpatternRequestFailure(String),

    #[error("SQL database responded with empty response to query: {0}")]
    SingleFeedpatternRequestEmptyResponse(String),

    #[error("SQL database responded with multiple responses to request to query: {0}")]
    SingleFeedpatternRequestNoSingleResponse(String),

    #[error("Could not get schedule entries from SQL database using query: {0}")]
    ScheduleRequestFailure(String),

    #[error("ScheduleEntry: Error in conversion of SQL database result to enum ({0}).")]
    ScheduleTypeConversionFailure(String),

    #[error("Could not convert start time string read from SQL database ({0}).")]
    ScheduleStartTimeConversionFailure(String),

    #[error("Could not convert stop time string read from SQL database ({0}).")]
    ScheduleStopTimeConversionFailure(String),

    #[error("Schedule of type {0} could not be found in database.")]
    ScheduleTypeNotFound(String),

    #[error("Error when trying to convert string to timestamp: {0}")]
    TimestampConversionFailure(String),

    #[error("Error when inserting heating stats into database using query: {0}")]
    InsertHeatingStatsEntryFailure(String),

    #[error("Error occurred when trying to insert a feed event into database.")]
    InsertFeedEventFailure,

    #[error("Error occurred when trying to insert a feed event into database.")]
    InsertRefillEventFailure,

    #[error("Error occurred when trying to insert a Balling dosing event into database.")]
    InsertBallingEventFailure,

    #[error("Could not get feed schedule entry from the database using query: {0}")]
    FeedScheduleEntryRequestFailure(String),

    #[error("Could not convert feedschedule entry string(s) read from the database to timestamp")]
    FeedScheduleEntryTimeStampConversionFailure,

    #[error("Error occurred when trying to calculate update of feed schedule entry.")]
    FeedScheduleUpdateFailure,

    #[error("Error occurred when trying to drop a feed schedule entry from database.")]
    FeedScheduleDropFailure,

    #[error("Could not get single Balling set value from the database using query: {0}")]
    SingleBallingSetValRequestFailure(String),

    #[error("SQL database responded with empty response to query: {0}")]
    SingleBallingSetValEmptyResponse(String),

    #[error("SQL database responded with multiple responses to request to query: {0}")]
    SingleBallingSetValNoSingleResponse(String),

    #[error("Error when inserting data frame into database using query: {0}")]
    InsertDataFrameFailure(String),

    #[error("Database responded with a timestamp in the future (delta of {0} seconds).")]
    NegativeTimeDeltaSeconds(i64),

    #[error("Database responded with a negative value ({0}) where a positive value was expected.")]
    NegativeInteger(i64),

    #[error("Could not get heating set values from the database with query: {0}")]
    HeatingSetValsRequestFailure(String),

    #[error("The database returned more than one heating set value entry with query: {0}")]
    HeatingSetValsNoSingleResponse(String),

    #[error("The heating switch on temperature ({0}) is higher than the heating switch off temperature ({1}).")]
    HeatingSetValsInvalid(f32, f32),

    #[error("Could not get ventilation set values from the database with query: {0}")]
    VentilationSetValsRequestFailure(String),

    #[error("The database returned more than one ventilation set value entry with query: {0}")]
    VentilationSetValsNoSingleResponse(String),

    #[error("The ventilation switch on temperature ({0}) is lower than the ventilation switch off temperature ({1}).")]
    VentilationSetValsInvalid(f32, f32),

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
    SingleDataFrameRequestNoSingleResponse(String, String)
}
