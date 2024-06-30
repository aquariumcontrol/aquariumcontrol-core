/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use chrono::{Duration, NaiveDateTime};
use mysql::prelude::*;
use mysql::*;

use crate::feedpattern::{FeedPhase, Feedpattern};
use crate::sql_interface::{sql_query_strings, SqlInterfaceError};

#[derive(Clone)]
/// Holds the header information of a feed pattern.
pub struct SqlFeedpatternHeader {
    pub profile_id: i32,
    pub profile_name: String,
}

#[derive(Clone)]
/// Holds the data of one phase of a feed pattern as retrieved from SQL data base.
/// The numeric data is post-processed to boolean in a subsequent step.
pub struct SqlFeedPhase {
    /// duration of the pause before switching on the feeder motor in milliseconds
    pub pause_duration: i16,

    /// flag indicating if the protein skimmer shall remain switched on during the pause
    pub pause_skimmer: i16,

    /// flag indicating if the main pump #1 shall remain switched on during the pause
    pub pause_main_pump_1: i16,

    /// flag indicating if the main pump #2 shall remain switched on during the pause
    pub pause_main_pump_2: i16,

    /// flag indicating if the auxiliary pump #1 shall remain switched on during the pause
    pub pause_aux_pump_1: i16,

    /// flag indicating if the auxiliary pump #2 shall remain switched on during the pause
    pub pause_aux_pump_2: i16,

    /// duration of the feed after switching on the feeder motor in milliseconds
    pub feed_duration: i16,

    /// flag indicating if the protein skimmer shall remain switched on during the feed phase
    pub feed_skimmer: i16,

    /// flag indicating if the main pump #1 shall remain switched on during the feed phase
    pub feed_main_pump_1: i16,

    /// flag indicating if the main pump #2 shall remain switched on during the feed phase
    pub feed_main_pump_2: i16,

    /// flag indicating if the auxiliary pump #1 shall remain switched on during the feed phase
    pub feed_aux_pump_1: i16,

    /// flag indicating if the auxiliary pump #2 shall remain switched on during the feed phase
    pub feed_aux_pump_2: i16,
}

/// Holds the data of a feed schedule entry as retrieved from SQL data base.
/// It is post-processed in a subsequent step.
pub struct SqlFeedScheduleEntry {
    pub timestamp: String,
    pub profile_id: i32,
    pub profile_name: String,
    pub repeat_daily: i16,
}

/// Holds the data of a feed schedule entry derived from data read from SQL data base.
#[derive(Debug, PartialEq)]
pub struct FeedScheduleEntry {
    pub timestamp: NaiveDateTime,
    pub profile_id: i32,
    pub profile_name: String,
    pub repeat_daily: bool,
}

/// Contains the configuration and the implementation of the SQL interface for Feed.
pub struct SqlInterfaceFeed {
    /// Connection to database
    pub conn: PooledConn,
}

impl SqlInterfaceFeed {
    /// Provides a struct of type SqlInterfaceFeed.
    /// # Arguments
    /// * `conn` - connection to SQL database
    pub fn new(conn: PooledConn) -> SqlInterfaceFeed {
        SqlInterfaceFeed { conn }
    }

    /// Retrieves the header of a feed pattern from SQL database.
    /// It provides wrapped into Result type either
    /// - the result of type SqlFeedpatternHeader
    /// - or a value of enum SqlInterfaceError
    /// # Arguments
    /// * `profile_id` - ID of the feed profile    
    pub fn get_feedpattern_header_from_database(
        &mut self,
        profile_id: i32,
    ) -> Result<SqlFeedpatternHeader, SqlInterfaceError> {
        // prepare query for header
        let sql_query = str::replace(
            sql_query_strings::SQL_QUERY_READ_FEEDPATTERN_HEADER,
            "profile_id",
            &profile_id.to_string(),
        );
        let single_feedpattern_entry_array =
            match self
                .conn
                .query_map(sql_query.clone(), |(profile_id, profile_name)| {
                    SqlFeedpatternHeader {
                        profile_id,
                        profile_name,
                    }
                }) {
                Ok(c) => c,
                Err(_) => {
                    return Err(SqlInterfaceError::SingleFeedpatternRequestFailure(
                        "Could not get feed pattern header from SQL database with query:"
                            .to_string()
                            + sql_query.as_str(),
                    ));
                }
            };

        // check error case if there is no response
        if single_feedpattern_entry_array.len() == 0 {
            return Err(SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse(
                "SQL database responded with empty response to query: ".to_string()
                    + sql_query.as_str(),
            ));
        }
        // check error case if there is more than one response
        if single_feedpattern_entry_array.len() > 1 {
            return Err(SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse(
                "SQL database responded with multiple responses to request to query:".to_string()
                    + sql_query.as_str(),
            ));
        }
        let feedpattern_header = &single_feedpattern_entry_array[0];

        Ok(feedpattern_header.clone())
    }

    /// Retrieves the feed pattern data of a specific phase from SQL database.
    /// # Arguments
    /// * `profile_id` - ID of the feed profile    
    /// * `phase_nr` - phase number of the feed profile   
    pub fn get_feed_phase_from_database(
        &mut self,
        profile_id: i32,
        phase_nr: i32,
    ) -> Result<SqlFeedPhase, SqlInterfaceError> {
        // prepare query for header
        let sql_query_pre = str::replace(
            sql_query_strings::SQL_QUERY_READ_FEED_PHASE,
            "profile_id",
            &profile_id.to_string(),
        );
        let sql_query = str::replace(&sql_query_pre, "#", format!("{:02}", phase_nr).as_str());

        let single_feedpattern_entry_array = match self.conn.query_map(
            sql_query.clone(),
            |(
                pause_duration,
                pause_skimmer,
                pause_main_pump_1,
                pause_main_pump_2,
                pause_aux_pump_1,
                pause_aux_pump_2,
                feed_duration,
                feed_skimmer,
                feed_main_pump_1,
                feed_main_pump_2,
                feed_aux_pump_1,
                feed_aux_pump_2,
            )| SqlFeedPhase {
                pause_duration,
                pause_skimmer,
                pause_main_pump_1,
                pause_main_pump_2,
                pause_aux_pump_1,
                pause_aux_pump_2,
                feed_duration,
                feed_skimmer,
                feed_main_pump_1,
                feed_main_pump_2,
                feed_aux_pump_1,
                feed_aux_pump_2,
            },
        ) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::SingleFeedpatternRequestFailure(
                    "Could not get feed pattern phase from SQL database with query: ".to_string()
                        + sql_query.as_str(),
                ));
            }
        };

        // check error case if there is no response
        if single_feedpattern_entry_array.len() == 0 {
            return Err(SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse(
                "SQL database responded with empty response to query: ".to_string()
                    + sql_query.as_str(),
            ));
        }
        // check error case if there is more than one response
        if single_feedpattern_entry_array.len() > 1 {
            return Err(SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse(
                "SQL database responded with multiple responses to request to query: ".to_string()
                    + sql_query.as_str(),
            ));
        }
        let feed_phase = &single_feedpattern_entry_array[0];

        Ok(feed_phase.clone())
    }

    /// The function retries the header and all phases of a feed pattern from the SQL database.
    /// It provides wrapped into Result type either
    /// - the result of type Feedpattern
    /// - or a value of enum SqlInterfaceError
    /// # Arguments
    /// * `profile_id` - ID of the feed profile    
    pub fn get_single_feedpattern_from_database(
        &mut self,
        profile_id: i32,
    ) -> Result<Feedpattern, SqlInterfaceError> {
        let sql_feedpattern_header = match self.get_feedpattern_header_from_database(profile_id) {
            Ok(c) => c,
            Err(e) => {
                println!("{e:?}");
                return Err(e);
            }
        };
        let mut feed_phases = Vec::new();
        for nr in 1..11 {
            let sql_feed_phase = match self.get_feed_phase_from_database(profile_id, nr) {
                Ok(c) => c,
                Err(e) => {
                    return Err(e);
                }
            };
            let feed_phase = FeedPhase {
                pause_duration: sql_feed_phase.pause_duration,
                pause_skimmer: sql_feed_phase.pause_skimmer > 0,
                pause_main_pump_1: sql_feed_phase.pause_main_pump_1 > 0,
                pause_main_pump_2: sql_feed_phase.pause_main_pump_2 > 0,
                pause_aux_pump_1: sql_feed_phase.pause_aux_pump_1 > 0,
                pause_aux_pump_2: sql_feed_phase.pause_aux_pump_2 > 0,
                feed_duration: sql_feed_phase.feed_duration,
                feed_skimmer: sql_feed_phase.feed_skimmer > 0,
                feed_main_pump_1: sql_feed_phase.feed_main_pump_1 > 0,
                feed_main_pump_2: sql_feed_phase.feed_main_pump_2 > 0,
                feed_aux_pump_1: sql_feed_phase.feed_aux_pump_1 > 0,
                feed_aux_pump_2: sql_feed_phase.feed_aux_pump_2 > 0,
            };
            feed_phases.push(feed_phase);
        }
        Ok(Feedpattern {
            profile_id: sql_feedpattern_header.profile_id,
            profile_name: sql_feedpattern_header.profile_name,
            feedphases: feed_phases,
        })
    }

    /// Inserts one dataset of feed event into the database.
    /// The function either returns an empty Ok value or a value of type SqlInterfaceError.
    /// # Arguments
    /// * `timestamp` - timestamp of feed event
    /// * `feeder_run_time` - run time of feeder in seconds
    /// * `profile_name` - name of the feed profile executed
    /// * `profile_id` - ID of the feed profile executed
    pub fn insert_feed_event(
        &mut self,
        timestamp: NaiveDateTime,
        feeder_run_time: f64,
        profile_name: String,
        profile_id: i32,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_FEED_EVENT.to_owned();
        match self.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "timestamp" => timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "feeder_run_time" => feeder_run_time,
            "profile_name" => profile_name,
            "profile_id" => profile_id,
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("{e:?}");
                Err(SqlInterfaceError::InsertFeedEventFailure(
                    "Error when inserting refill event into database.".to_string(),
                ))
            }
        }
    }

    /// Retrieve from SQL database the oldest feed schedule entry which lies in the past.
    /// Function provides wrapped into Result type either
    /// - Feedschedule entry of type FeedScheduleEntry or None value wrapped in type Option
    /// - a value of enum SqlInterfaceError
    pub fn get_feedschedule_entry_from_database(
        &mut self,
    ) -> Result<Option<FeedScheduleEntry>, SqlInterfaceError> {
        let sql_query = sql_query_strings::SQL_QUERY_READ_FEED_SCHEDULE_ENTRY;

        let feedschedule_entry_array = match self.conn.query_map(
            sql_query,
            |(timestamp, profile_id, profile_name, repeat_daily)| SqlFeedScheduleEntry {
                timestamp,
                profile_id,
                profile_name,
                repeat_daily,
            },
        ) {
            Ok(c) => c,
            Err(_) => {
                return Err(SqlInterfaceError::FeedScheduleEntryRequestFailure(
                    "Could not get feed schedule entry from SQL database with query:".to_string()
                        + sql_query,
                ));
            }
        };

        // return none if query result is empty
        if feedschedule_entry_array.len() == 0 {
            return Ok(None);
        }

        let timestamp = match NaiveDateTime::parse_from_str(
            feedschedule_entry_array[0].timestamp.as_str(),
            "%Y-%m-%d %H:%M:%S",
        ) {
            Ok(c) => c,
            Err(e) => {
                println!("{}", e);
                return Err(
                    SqlInterfaceError::FeedScheduleEntryTimeStampConversionFailure(
                        "Could not convert timestamp string read from SQL database".to_string(),
                    ),
                );
            }
        };

        let feedschedule_entry = FeedScheduleEntry {
            timestamp: timestamp,
            profile_id: feedschedule_entry_array[0].profile_id,
            profile_name: feedschedule_entry_array[0].profile_name.clone(),
            repeat_daily: feedschedule_entry_array[0].repeat_daily != 0,
        };

        Ok(Some(feedschedule_entry))
    }

    /// Triggers the SQL queries to either update the existing feed schedule entry with
    /// tomorrow's date or delete the entry.
    /// # Arguments
    /// * `feedschedule_entry` - reference to a struct of type FeedScheduleEntry for which the timestamp shall be updated.
    pub fn update_feedschedule_entry_in_database(
        &mut self,
        feedschedule_entry: &mut FeedScheduleEntry,
    ) -> Result<(), SqlInterfaceError> {
        if feedschedule_entry.repeat_daily {
            // feed schedule shall be repeated daily, so update the entry with a date 24h in the future
            let sql_query_string =
                sql_query_strings::SQL_QUERY_UPDATE_FEED_SCHEDULE_ENTRY.to_owned();
            let timestamp_tomorrow = feedschedule_entry.timestamp + Duration::days(1);
            println!("Timestamp for tomorrow: {}", timestamp_tomorrow.to_string());
            match self.conn.exec_drop::<_, _>(
                sql_query_string,
                params! {
                "timestamp_new" => timestamp_tomorrow.format("%Y-%m-%d %H:%M:%S").to_string(),
                "timestamp_old" => feedschedule_entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
                },
            ) {
                Ok(_) => Ok(()),
                Err(e) => {
                    println!("{e:?}");
                    Err(SqlInterfaceError::SqlInterfaceErrorFeedScheduleUpdateFailure(
                        "Error when updating feed schedule of entry which shall be repeated.".to_string(),
                    ))
                }
            }
        } else {
            // feed schedule shall not be repeated daily, delete the entry from the database
            let sql_query_string =
                sql_query_strings::SQL_QUERY_DELETE_FEED_SCHEDULE_ENTRY.to_owned();
            match self.conn.exec_drop::<_, _>(
                sql_query_string,
                params! {
                "timestamp" => feedschedule_entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
                },
            ) {
                Ok(_) => Ok(()),
                Err(e) => {
                    println!("{e:?}");
                    Err(
                        SqlInterfaceError::SqlInterfaceErrorFeedScheduleUpdateFailure(
                            "Error when deleting refill entry which shall be not repeated."
                                .to_string(),
                        ),
                    )
                }
            }
        }
    }

    // Inserts one dataset of feed schedule entry into the database - for testing purposes.
    // The function either returns an empty Ok value or a value of type SqlInterfaceError.
    // # Arguments
    // * `timestamp` - timestamp of feed event
    // * `feeder_run_time` - run time of feeder in seconds
    // * `profile_name` - name of the feed profile executed
    // * `profile_id` - ID of the feed profile executed
    pub fn insert_feed_schedule_entry(
        &mut self,
        timestamp: NaiveDateTime,
        profile_id: i32,
        profile_name: String,
        is_weekly: bool,
        is_daily: bool,
    ) -> Result<(), SqlInterfaceError> {
        let sql_query_string = sql_query_strings::SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY.to_owned();
        match self.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "timestamp" => timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "profile_id" => profile_id,
            "profile_name" => profile_name,
            "is_weekly" => is_weekly,
            "is_daily" => is_daily,
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!("{e:?}");
                Err(SqlInterfaceError::InsertFeedEventFailure(
                    "Error when inserting refill event into database.".to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use chrono::{Duration, NaiveDateTime};
    use mysql::params;
    use mysql::prelude::Queryable;

    use crate::config::read_config_file;
    use crate::config::ConfigData;
    use crate::sql_interface::{sql_query_strings, SqlInterface, SqlInterfaceError};
    use crate::sql_interface_feed::SqlInterfaceFeed;

    pub struct SqlFeedEvent {
        timestamp: String,
        feeder_run_time: f32,
        feed_profile_name: String,
        feed_profile_id: i32,
    }

    pub struct FeedEvent {
        timestamp: NaiveDateTime,
        feeder_run_time: f32,
        feed_profile_name: String,
        feed_profile_id: i32,
    }

    /// This query is used for testing: It creates a feed pattern. It is used only by tests of this crate, therefore it resides here.
    pub const SQL_QUERY_WRITE_FEEDPATTERN: &str = "INSERT INTO feedpatterns VALUES \
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

    /// This query is used for testing: It creates a feed schedule entry. It is used only by tests of this crate, therefore it resides here.
    pub const SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY: &str = "INSERT INTO feedschedule VALUES \
                (:timestamp,:profile_id,:profile_name,0,:repeat_daily);";

    #[test]
    /// all test cases for this crate/SQL table combined in one function to avoid problems with multithreading
    pub fn test_sql_interface_feedpattern() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_sql_interface.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_feed = SqlInterfaceFeed::new(sql_interface.get_connection());

        //*** check retrieving of feed pattern from empty table - FAIL **************************
        match SqlInterface::truncate_table(&mut sql_interface, "feedpatterns".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        match sql_interface_feed.get_single_feedpattern_from_database(1) {
            Ok(_) => {
                panic!("Call to get_single_feedpattern_from_database should have failed.");
            }
            Err(e) => {
                assert_eq!(
                    e,
                    SqlInterfaceError::SingleFeedpatternRequestNoSingleResponse(
                        "SQL database responded with empty response to query: \
                        SELECT ProfileID, ProfileName FROM feedpatterns WHERE ProfileID=1;"
                            .to_string()
                    )
                );
            }
        }
        println!("* checking retrieving of Feed pattern (FAIL) succeeded.");
        // **************************************************************************************

        // *** check retrieving of existing feed pattern - FAIL *********************************
        // prepare database: insert feed profile
        let sql_query_string = SQL_QUERY_WRITE_FEEDPATTERN.to_string();
        match sql_interface_feed.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "profile_id" => 1,
            },
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Error when inserting feed pattern into database: {e:?}");
            }
        };
        match sql_interface_feed.get_single_feedpattern_from_database(1) {
            Ok(c) => {
                assert_eq!(c.profile_id, 1);
                assert_eq!(c.profile_name, "test".to_string());
                assert_eq!(c.feedphases[0].pause_duration, 0);
                assert_eq!(c.feedphases[0].pause_skimmer, true);
                assert_eq!(c.feedphases[0].pause_main_pump_1, false);
                assert_eq!(c.feedphases[0].pause_main_pump_2, false);
                assert_eq!(c.feedphases[0].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[0].pause_aux_pump_2, false);
                assert_eq!(c.feedphases[0].feed_duration, 1);
                assert_eq!(c.feedphases[0].feed_skimmer, false);
                assert_eq!(c.feedphases[0].feed_main_pump_1, true);
                assert_eq!(c.feedphases[0].feed_main_pump_2, false);
                assert_eq!(c.feedphases[0].feed_aux_pump_1, false);
                assert_eq!(c.feedphases[0].feed_aux_pump_2, false);

                assert_eq!(c.feedphases[1].pause_duration, 2);
                assert_eq!(c.feedphases[1].pause_skimmer, false);
                assert_eq!(c.feedphases[1].pause_main_pump_1, false);
                assert_eq!(c.feedphases[1].pause_main_pump_2, true);
                assert_eq!(c.feedphases[1].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[1].pause_aux_pump_2, false);
                assert_eq!(c.feedphases[1].feed_duration, 3);
                assert_eq!(c.feedphases[1].feed_skimmer, false);
                assert_eq!(c.feedphases[1].feed_main_pump_1, false);
                assert_eq!(c.feedphases[1].feed_main_pump_2, false);
                assert_eq!(c.feedphases[1].feed_aux_pump_1, true);
                assert_eq!(c.feedphases[1].feed_aux_pump_2, false);

                assert_eq!(c.feedphases[2].pause_duration, 4);
                assert_eq!(c.feedphases[2].pause_skimmer, false);
                assert_eq!(c.feedphases[2].pause_main_pump_1, false);
                assert_eq!(c.feedphases[2].pause_main_pump_2, false);
                assert_eq!(c.feedphases[2].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[2].pause_aux_pump_2, true);
                assert_eq!(c.feedphases[2].feed_duration, 5);
                assert_eq!(c.feedphases[2].feed_skimmer, true);
                assert_eq!(c.feedphases[2].feed_main_pump_1, true);
                assert_eq!(c.feedphases[2].feed_main_pump_2, false);
                assert_eq!(c.feedphases[2].feed_aux_pump_1, false);
                assert_eq!(c.feedphases[2].feed_aux_pump_2, false);

                assert_eq!(c.feedphases[3].pause_duration, 6);
                assert_eq!(c.feedphases[3].pause_skimmer, true);
                assert_eq!(c.feedphases[3].pause_main_pump_1, false);
                assert_eq!(c.feedphases[3].pause_main_pump_2, true);
                assert_eq!(c.feedphases[3].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[3].pause_aux_pump_2, false);
                assert_eq!(c.feedphases[3].feed_duration, 7);
                assert_eq!(c.feedphases[3].feed_skimmer, true);
                assert_eq!(c.feedphases[3].feed_main_pump_1, false);
                assert_eq!(c.feedphases[3].feed_main_pump_2, false);
                assert_eq!(c.feedphases[3].feed_aux_pump_1, true);
                assert_eq!(c.feedphases[3].feed_aux_pump_2, false);

                assert_eq!(c.feedphases[4].pause_duration, 8);
                assert_eq!(c.feedphases[4].pause_skimmer, true);
                assert_eq!(c.feedphases[4].pause_main_pump_1, false);
                assert_eq!(c.feedphases[4].pause_main_pump_2, false);
                assert_eq!(c.feedphases[4].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[4].pause_aux_pump_2, true);
                assert_eq!(c.feedphases[4].feed_duration, 9);
                assert_eq!(c.feedphases[4].feed_skimmer, true);
                assert_eq!(c.feedphases[4].feed_main_pump_1, true);
                assert_eq!(c.feedphases[4].feed_main_pump_2, true);
                assert_eq!(c.feedphases[4].feed_aux_pump_1, false);
                assert_eq!(c.feedphases[4].feed_aux_pump_2, false);

                assert_eq!(c.feedphases[5].pause_duration, 10);
                assert_eq!(c.feedphases[5].pause_skimmer, true);
                assert_eq!(c.feedphases[5].pause_main_pump_1, true);
                assert_eq!(c.feedphases[5].pause_main_pump_2, true);
                assert_eq!(c.feedphases[5].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[5].pause_aux_pump_2, false);
                assert_eq!(c.feedphases[5].feed_duration, 11);
                assert_eq!(c.feedphases[5].feed_skimmer, true);
                assert_eq!(c.feedphases[5].feed_main_pump_1, true);
                assert_eq!(c.feedphases[5].feed_main_pump_2, false);
                assert_eq!(c.feedphases[5].feed_aux_pump_1, false);
                assert_eq!(c.feedphases[5].feed_aux_pump_2, true);

                assert_eq!(c.feedphases[6].pause_duration, 12);
                assert_eq!(c.feedphases[6].pause_skimmer, true);
                assert_eq!(c.feedphases[6].pause_main_pump_1, true);
                assert_eq!(c.feedphases[6].pause_main_pump_2, true);
                assert_eq!(c.feedphases[6].pause_aux_pump_1, true);
                assert_eq!(c.feedphases[6].pause_aux_pump_2, false);
                assert_eq!(c.feedphases[6].feed_duration, 13);
                assert_eq!(c.feedphases[6].feed_skimmer, true);
                assert_eq!(c.feedphases[6].feed_main_pump_1, true);
                assert_eq!(c.feedphases[6].feed_main_pump_2, true);
                assert_eq!(c.feedphases[6].feed_aux_pump_1, false);
                assert_eq!(c.feedphases[6].feed_aux_pump_2, true);

                assert_eq!(c.feedphases[7].pause_duration, 14);
                assert_eq!(c.feedphases[7].pause_skimmer, true);
                assert_eq!(c.feedphases[7].pause_main_pump_1, true);
                assert_eq!(c.feedphases[7].pause_main_pump_2, true);
                assert_eq!(c.feedphases[7].pause_aux_pump_1, true);
                assert_eq!(c.feedphases[7].pause_aux_pump_2, true);
                assert_eq!(c.feedphases[7].feed_duration, 15);
                assert_eq!(c.feedphases[7].feed_skimmer, true);
                assert_eq!(c.feedphases[7].feed_main_pump_1, true);
                assert_eq!(c.feedphases[7].feed_main_pump_2, true);
                assert_eq!(c.feedphases[7].feed_aux_pump_1, true);
                assert_eq!(c.feedphases[7].feed_aux_pump_2, false);

                assert_eq!(c.feedphases[8].pause_duration, 16);
                assert_eq!(c.feedphases[8].pause_skimmer, true);
                assert_eq!(c.feedphases[8].pause_main_pump_1, true);
                assert_eq!(c.feedphases[8].pause_main_pump_2, true);
                assert_eq!(c.feedphases[8].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[8].pause_aux_pump_2, false);
                assert_eq!(c.feedphases[8].feed_duration, 17);
                assert_eq!(c.feedphases[8].feed_skimmer, true);
                assert_eq!(c.feedphases[8].feed_main_pump_1, true);
                assert_eq!(c.feedphases[8].feed_main_pump_2, false);
                assert_eq!(c.feedphases[8].feed_aux_pump_1, false);
                assert_eq!(c.feedphases[8].feed_aux_pump_2, false);

                assert_eq!(c.feedphases[9].pause_duration, 18);
                assert_eq!(c.feedphases[9].pause_skimmer, true);
                assert_eq!(c.feedphases[9].pause_main_pump_1, false);
                assert_eq!(c.feedphases[9].pause_main_pump_2, false);
                assert_eq!(c.feedphases[9].pause_aux_pump_1, false);
                assert_eq!(c.feedphases[9].pause_aux_pump_2, false);
                assert_eq!(c.feedphases[9].feed_duration, 19);
                assert_eq!(c.feedphases[9].feed_skimmer, false);
                assert_eq!(c.feedphases[9].feed_main_pump_1, false);
                assert_eq!(c.feedphases[9].feed_main_pump_2, false);
                assert_eq!(c.feedphases[9].feed_aux_pump_1, false);
                assert_eq!(c.feedphases[9].feed_aux_pump_2, false);
            }
            Err(e) => {
                panic!("Call to get_single_feedpattern_from_database has failed: {e:?}.");
            }
        }
        println!("* checking retrieving of feed pattern (PASS) succeeded.");
        // **************************************************************************************
    }

    #[test]
    /// all test cases for this crate/SQL table combined in one function to avoid problems with multithreading
    pub fn test_sql_interface_feedlog() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_sql_interface.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_feed = SqlInterfaceFeed::new(sql_interface.get_connection());
        //*** check inserting of feed event into empty table - FAIL ****************************
        match SqlInterface::truncate_table(&mut sql_interface, "feedlog".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        let feed_timestamp =
            NaiveDateTime::parse_from_str("2022-04-11 11:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        match sql_interface_feed.insert_feed_event(feed_timestamp, 5.5, "Test".to_string(), 1) {
            Ok(_) => {}
            Err(e) => panic!("Could not insert feed event into database: {e:?}"),
        }
        // Read back feed log entry from data base
        let single_feed_event_entry_array = match sql_interface_feed.conn.query_map(
            sql_query_strings::SQL_QUERY_READ_LAST_FEED_EVENT,
            |(timestamp, feeder_run_time, feed_profile_name, feed_profile_id)| SqlFeedEvent {
                timestamp,
                feeder_run_time,
                feed_profile_name,
                feed_profile_id,
            },
        ) {
            Ok(c) => c,
            Err(e) => {
                panic!("Could not read feed event from data base: {e:?}");
            }
        };
        // check if SQL response has right structure
        assert_eq!(single_feed_event_entry_array.len(), 1);
        // check if SQL response contains the correct data
        let feed_event = FeedEvent {
            timestamp: NaiveDateTime::parse_from_str(
                single_feed_event_entry_array[0].timestamp.as_str(),
                "%Y-%m-%d %H:%M:%S",
            )
            .expect("conversion of timestamp to NaiveDate failed."),
            feeder_run_time: single_feed_event_entry_array[0].feeder_run_time,
            feed_profile_name: single_feed_event_entry_array[0].feed_profile_name.clone(),
            feed_profile_id: single_feed_event_entry_array[0].feed_profile_id,
        };
        assert_eq!(feed_event.timestamp, feed_timestamp);
        assert_eq!(feed_event.feeder_run_time, 5.5);
        assert_eq!(feed_event.feed_profile_name, "Test");
        assert_eq!(feed_event.feed_profile_id, 1);
        println!("* checking inserting of feed event into log (PASS) succeeded.");
        // **************************************************************************************
    }

    #[test]
    /// all test cases for this crate combined in one function to avoid problems with multithreading
    pub fn test_sql_interface_feedschedule() {
        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_sql_interface.toml".to_string());
        println!("Testing with database {}", config.sql_interface.db_name);
        let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface)
            .expect("Initialization of SQL interface for test failed.");
        let mut sql_interface_feed = SqlInterfaceFeed::new(sql_interface.get_connection());
        // *** check trying to load feed schedule entry when table is empty *********************
        match SqlInterface::truncate_table(&mut sql_interface, "feedschedule".to_string()) {
            Ok(_) => {}
            Err(e) => panic!("Could not prepare test case: {e:?}"),
        }
        match sql_interface_feed.get_feedschedule_entry_from_database() {
            Ok(c) => {
                assert_eq!(c, None);
            }
            Err(_) => {
                panic!("Error when trying to load feed schedule entry from empty table.");
            }
        }
        println!("* reading from empty feed schedule (no result) succeeded.");
        // **************************************************************************************

        // *** check trying to load feed schedule entry *****************************************
        let timestamp_reference = "2024-04-10 14:00:00";
        let profile_id_reference = 1;
        let profile_name_reference = "test";
        let repeat_daily_reference = true;

        let sql_query_string = SQL_QUERY_WRITE_FEED_SCHEDULE_ENTRY.to_string();
        match sql_interface_feed.conn.exec_drop::<_, _>(
            sql_query_string,
            params! {
            "timestamp" => timestamp_reference,
            "profile_id" => profile_id_reference,
            "profile_name" => profile_name_reference,
            "repeat_daily" => repeat_daily_reference,
            },
        ) {
            Ok(_) => {}
            Err(e) => {
                panic!("Error when inserting feed schedule entry into database: {e:?}");
            }
        };

        let mut feedschedule_entry_before_update = match sql_interface_feed
            .get_feedschedule_entry_from_database()
        {
            Ok(c) => match c {
                Some(d) => {
                    assert_eq!(
                        d.timestamp,
                        NaiveDateTime::parse_from_str(timestamp_reference, "%Y-%m-%d %H:%M:%S")
                            .unwrap()
                    );
                    assert_eq!(d.profile_id, profile_id_reference);
                    assert_eq!(d.profile_name, profile_name_reference);
                    assert_eq!(d.repeat_daily, repeat_daily_reference);
                    d
                }
                None => {
                    panic!("SQL database returned empty result although feed schedule entry was inserted.");
                }
            },
            Err(_) => {
                panic!("Error occurred when trying to load feed schedule entry.");
            }
        };
        println!("* reading existing entry from feed schedule (PASS) succeeded.");
        // **************************************************************************************

        // *** check updating feed schedule with repetion ***************************************
        match sql_interface_feed
            .update_feedschedule_entry_in_database(&mut feedschedule_entry_before_update)
        {
            Ok(()) => {}
            Err(e) => {
                panic!("Error occurred when trying to update feedschule entry which is marked for repitition: {e:?}");
            }
        }

        let mut feedschedule_entry_after_update = match sql_interface_feed
            .get_feedschedule_entry_from_database()
        {
            Ok(c) => match c {
                Some(d) => {
                    assert_eq!(
                        d.timestamp,
                        feedschedule_entry_before_update.timestamp + Duration::days(1)
                    );
                    assert_eq!(d.profile_id, profile_id_reference);
                    assert_eq!(d.profile_name, profile_name_reference);
                    assert_eq!(d.repeat_daily, repeat_daily_reference);
                    d
                }
                None => {
                    panic!("SQL database returned empty result although feed schedule entry was inserted.");
                }
            },
            Err(_) => {
                panic!("Error occurred when trying to load feed schedule entry.");
            }
        };
        println!("* update existing entry from feed schedule (PASS) succeeded.");
        // **************************************************************************************

        // *** check updating feed schedule without repetion ************************************
        // modify existing feed schedule entry -> it shall be deleted with call to update
        feedschedule_entry_after_update.repeat_daily = false;
        match sql_interface_feed
            .update_feedschedule_entry_in_database(&mut feedschedule_entry_after_update)
        {
            Ok(()) => {}
            Err(e) => {
                panic!("Error occurred when trying to update feedschule entry which is not marked for repitition: {e:?}");
            }
        }

        match sql_interface_feed.get_feedschedule_entry_from_database() {
            Ok(c) => {
                assert_eq!(c, None);
            }
            Err(_) => {
                panic!("Error when trying to load feed schedule entry from empty table.");
            }
        };
        println!("* deleting existing entry from feed schedule (PASS) succeeded.");
        // **************************************************************************************
    }
}
