/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[derive(PartialEq, Debug)]
/// Enum codifies the errors which the program may encounter in communication with the DB
pub enum SqlInterfaceError {
    ConnectionPoolFailure(String),
    RequiredTablesNotExisting,
    ShowTablesWithoutProperResult(String),
    SingleVersionRequestFailure(String),
    SingleVersionRequestNotListed(String),
    SingleVersionRequestMultipleResults,
    SingleStringRequestFailure(String),
    SingleStringRequestNoSingleResponse(String),
    SingleIntegerRequestFailure(String),
    SingleIntegerRequestNoSingleResponse(String),
    SingleFloatRequestFailure(String),
    SingleFloatRequestNoSingleResponse(String),
    SingleHeatingStatsRequestFailure(String),
    SingleHeatingStatsRequestNoSingleResponse(String),
    SingleFeedpatternRequestFailure(String),
    SingleFeedpatternRequestNoSingleResponse(String),
    ScheduleRequestFailure(String),
    ScheduleTypeConversionFailure(String),
    ScheduleStartTimeConversionFailure(String),
    ScheduleStopTimeConversionFailure(String),
    ScheduleTypeNotFound(String),
    TimestampConversionFailure(String),
    InsertHeatingStatsEntryFailure(String),
    InsertFeedEventFailure(String),
    InsertRefillEventFailure(String),
    InsertBallingEventFailure(String),
    FeedScheduleEntryRequestFailure(String),
    FeedScheduleEntryTimeStampConversionFailure(String),
    FeedScheduleUpdateFailure(String),
    SingleBallingSetValRequestFailure(String),
    InsertDataFrameFailure(String),
    NegativeInteger(i64),
    HeatingSetValsReadError(String),
    #[cfg(test)]
    InsertHeatingSetValuesFailure(String),
    #[cfg(test)]
    SingleDataFrameRequestNoSingleResponse(String),
    #[cfg(test)]
    TruncateTableFailure(String),
    #[cfg(test)]
    MockError,
    #[cfg(test)]
    InsertBallingPumpConfigurationFailure(String),
}
