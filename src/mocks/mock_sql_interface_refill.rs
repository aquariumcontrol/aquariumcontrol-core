/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use crate::database::pingable::Pingable;
    use crate::database::sql_interface_error::SqlInterfaceError;
    use crate::database::sql_interface_refill::{DatabaseInterfaceRefillTrait, DatabaseRefillErrorCode};
    use chrono::NaiveDateTime;

    pub struct MockSqlInterfaceRefill {}

    impl Pingable for MockSqlInterfaceRefill {
        fn ping(&mut self) {
            // do nothing
        }
    }

    impl DatabaseInterfaceRefillTrait for MockSqlInterfaceRefill {
        fn get_duration_since_last_refill(&mut self) -> Result<u64, SqlInterfaceError> {
            Err(SqlInterfaceError::MockError(module_path!().to_string()))
        }

        fn get_positive_integer_value_from_database(
            &mut self,
            _sql_query_string: &str,
        ) -> Result<u64, SqlInterfaceError> {
            Err(SqlInterfaceError::MockError(module_path!().to_string()))
        }

        fn check_empty(&mut self) -> Result<u64, SqlInterfaceError> {
            Err(SqlInterfaceError::MockError(module_path!().to_string()))
        }

        fn get_refill_count_of_last_hour(&mut self) -> Result<u64, SqlInterfaceError> {
            Err(SqlInterfaceError::MockError(module_path!().to_string()))
        }

        fn get_refill_count_of_last_24h(&mut self) -> Result<u64, SqlInterfaceError> {
            Err(SqlInterfaceError::MockError(module_path!().to_string()))
        }

        fn get_refill_volume_of_last_24h(&mut self) -> Result<f64, SqlInterfaceError> {
            Err(SqlInterfaceError::MockError(module_path!().to_string()))
        }

        fn get_refill_volume_of_last_hour(&mut self) -> Result<f64, SqlInterfaceError> {
            Err(SqlInterfaceError::MockError(module_path!().to_string()))
        }

        fn insert_refill_event(
            &mut self,
            _timestamp: NaiveDateTime,
            _duration: f64,
            _volume: f64,
            _error_code: DatabaseRefillErrorCode,
        ) -> Result<(), SqlInterfaceError> {
            Err(SqlInterfaceError::MockError(module_path!().to_string()))
        }
    }
}
