/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use chrono::NaiveDateTime;

    use crate::database::database_interface_feed_trait::DatabaseInterfaceFeedTrait;
    use crate::database::pingable::Pingable;
    use crate::database::sql_interface_error::SqlInterfaceError;
    use crate::food::feed_profile::FeedProfile;
    use crate::food::feed_schedule_entry::FeedScheduleEntry;

    /// Contains the configuration and the implementation of the mock SQL interface for Feed.
    /// This mock version only returns errors instead of any data.
    pub struct MockSqlInterfaceFeed {}

    impl MockSqlInterfaceFeed {
        /// Creates a new instance.
        pub fn new() -> MockSqlInterfaceFeed {
            MockSqlInterfaceFeed {}
        }
    }

    /// implementation of super trait
    impl Pingable for MockSqlInterfaceFeed {
        fn ping(&mut self) {
            // do nothing
        }
    }

    impl DatabaseInterfaceFeedTrait for MockSqlInterfaceFeed {
        /// Retrieves the current timestamp.
        fn get_current_timestamp(&mut self) -> Result<NaiveDateTime, SqlInterfaceError> {
            Err(SqlInterfaceError::MockError(module_path!().to_string()))
        }

        /// Retrieves the past feed schedule entries from database.
        fn get_past_feed_schedule_entries_from_database(
            &mut self,
        ) -> Result<Option<Vec<FeedScheduleEntry>>, SqlInterfaceError> {
            Err(SqlInterfaceError::MockError(module_path!().to_string()))
        }

        /// Performs the `insert_feed_event` operation.
        ///
        /// # Arguments
        /// * `timestamp` - A parameter representing `timestamp`.
        /// * `feeder_run_time` - A parameter representing `feeder_run_time`.
        /// * `profile_id` - A parameter representing `profile_id`.
        fn insert_feed_event(
            &mut self,
            _timestamp: NaiveDateTime,
            _feeder_run_time: f64,
            _profile_id: i32,
        ) -> Result<(), SqlInterfaceError> {
            Err(SqlInterfaceError::MockError(module_path!().to_string()))
        }

        /// Retrieves the single feed profile from database.
        ///
        /// # Arguments
        /// * `profile_id` - A parameter representing `profile_id`.
        fn get_single_feed_profile_from_database(
            &mut self,
            _profile_id: i32,
        ) -> Result<FeedProfile, SqlInterfaceError> {
            Err(SqlInterfaceError::MockError(module_path!().to_string()))
        }

        /// Updates the feed schedule entries in database.
        ///
        /// # Arguments
        /// * `feedschedule_entry` - A parameter representing `feedschedule_entry`.
        fn update_feed_schedule_entries_in_database(
            &mut self,
            _feedschedule_entry: &mut Vec<FeedScheduleEntry>,
        ) -> Result<(), SqlInterfaceError> {
            Err(SqlInterfaceError::MockError(module_path!().to_string()))
        }

        /// Performs the `insert_feed_schedule_entry` operation.
        ///
        /// # Arguments
        /// * `timestamp` - A parameter representing `timestamp`.
        /// * `profile_id` - A parameter representing `profile_id`.
        /// * `is_weekly` - A parameter representing `is_weekly`.
        /// * `is_daily` - A parameter representing `is_daily`.
        #[cfg(test)]
        fn insert_feed_schedule_entry(
            &mut self,
            _timestamp: NaiveDateTime,
            _profile_id: i32,
            _is_weekly: bool,
            _is_daily: bool,
        ) -> Result<(), SqlInterfaceError> {
            Err(SqlInterfaceError::MockError(module_path!().to_string()))
        }
    }
}
