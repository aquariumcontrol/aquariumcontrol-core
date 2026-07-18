/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#![allow(unused)] // functions in this crate are used by outside test functions

use mysql::params;
use mysql::prelude::Queryable;
use aquarium_control::database::sql_interface::SqlInterface;
use aquarium_control::database::sql_query_strings::{SQL_TABLE_FEED_PROFILES, SQL_TABLE_FEED_SCHEDULE};
use aquarium_control::database::sql_test_query_strings;
use aquarium_control::database::sql_test_query_strings::{SQL_QUERY_WRITE_VERSION_INFORMATION, SQL_TABLE_BALLING_DOSING_LOG, SQL_TABLE_BALLING_SET_VALS, SQL_TABLE_DATA, SQL_TABLE_FEED_LOG, SQL_TABLE_HEATING_SET_VALS, SQL_TABLE_HEATING_STATS, SQL_TABLE_REFILL, SQL_TABLE_SCHEDULE, SQL_TABLE_VENTILATION_SET_VALS};
use aquarium_control::utilities::version_information::VersionInformation;

// Helper function to create mock dosing profile
pub fn add_balling_profile(
    conn: &mut mysql::PooledConn,
    pump_id: i32,
    dosing_speed: f32,
    dosing_volume: f32,
    label: String,
) {
    conn.exec_drop(
        sql_test_query_strings::SQL_QUERY_WRITE_BALLING_SET_VALS,
        params! {
            "pump_id" => pump_id,
            "dosing_speed" => dosing_speed,
            "dosing_volume" => dosing_volume,
            "label" => label
        },
    )
        .unwrap();
}

// Helper function to insert mock version information into database
pub fn insert_mock_version_information(sql_interface: &mut SqlInterface) {
    let unknown_version_information = VersionInformation {
        major: -1,
        minor: -1,
        build: -1,
        hash: "void".to_string(),
    };
    let sql_query_string = SQL_QUERY_WRITE_VERSION_INFORMATION.to_string();
    match sql_interface.conn.exec_drop::<_, _>(
        sql_query_string,
        params! {
        "major" => unknown_version_information.major,
        "minor" => unknown_version_information.minor,
        "build" => unknown_version_information.build,
        "hash" => unknown_version_information.hash.clone(),
        },
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Error when inserting version information into database: {e:?}");
        }
    };
}

// Helper function to truncate all tables defined in sql_query_strings
pub fn truncate_all_tables(sql_interface: &mut SqlInterface) {
    SqlInterface::truncate_table(sql_interface, SQL_TABLE_FEED_PROFILES.to_string()).unwrap();
    SqlInterface::truncate_table(sql_interface, SQL_TABLE_FEED_SCHEDULE.to_string()).unwrap();
    SqlInterface::truncate_table(sql_interface, SQL_TABLE_FEED_LOG.to_string()).unwrap();
    SqlInterface::truncate_table(sql_interface, SQL_TABLE_BALLING_DOSING_LOG.to_string()).unwrap();
    SqlInterface::truncate_table(sql_interface, SQL_TABLE_BALLING_SET_VALS.to_string()).unwrap();
    SqlInterface::truncate_table(sql_interface, SQL_TABLE_DATA.to_string()).unwrap();
    SqlInterface::truncate_table(sql_interface, SQL_TABLE_HEATING_SET_VALS.to_string()).unwrap();
    SqlInterface::truncate_table(sql_interface, SQL_TABLE_HEATING_STATS.to_string()).unwrap();
    SqlInterface::truncate_table(sql_interface, SQL_TABLE_VENTILATION_SET_VALS.to_string()).unwrap();
    SqlInterface::truncate_table(sql_interface, SQL_TABLE_REFILL.to_string()).unwrap();
    SqlInterface::truncate_table(sql_interface, SQL_TABLE_SCHEDULE.to_string()).unwrap();
}

// Helper function to insert current version information into database
pub fn insert_current_version_information(sql_interface: &mut SqlInterface) {
    let version_information = VersionInformation::new().unwrap();
    let sql_query_string = SQL_QUERY_WRITE_VERSION_INFORMATION.to_string();
    match sql_interface.conn.exec_drop::<_, _>(
        sql_query_string,
        params! {
            "major" => version_information.major,
            "minor" => version_information.minor,
            "build" => version_information.build,
            "hash" => version_information.hash.clone(),
        },
    ) {
        Ok(_) => {}
        Err(e) => {
            panic!("Error when inserting current version information into database: {e:?}");
        }
    };
}
