/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests database interactions for the heating system's user-configurable set points.
use aquarium_control::database::sql_interface_heating_set_vals::{
    HeatingSetVals, SqlInterfaceHeatingSetVals,
};
use aquarium_control::database::sql_test_query_strings::SQL_TABLE_HEATING_SET_VALS;
use aquarium_control::database::{
    sql_interface::SqlInterface, sql_interface_error::SqlInterfaceError,
};
use aquarium_control::utilities::config::{read_config_file_with_test_database, ConfigData};

#[test]
// This test case verifies the validation logic within the `SqlInterfaceHeatingSetVals::new()` constructor.
// It covers the following scenarios:
// 1. Happy Path: Initialization succeeds when the table is empty or has one valid row.
// 2. Failure on Row Limit: Fails when `heatingsetvals` contains more than one row.
// Test case uses test database #60.
fn test_sql_interface_heating_set_vals_new() {
    // --- Common Setup ---
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        60,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface.clone())
        .expect("Initialization of SQL interface for test failed.");

    // --- Test Case 1: Happy Path (empty table) ---
    println!("* Testing new() with an empty table (Happy Path)...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_SET_VALS.to_string())
        .expect("Test setup failed: Could not truncate table.");

    let result =
        SqlInterfaceHeatingSetVals::new(sql_interface.get_connection().unwrap(), &config.heating);
    assert!(
        result.is_ok(),
        "Expected new() to succeed with an empty table, but it failed: {:?}",
        result.err()
    );
    println!("* Succeeded: Happy path initialization is successful on empty table.");

    // --- Test Case 2: Failure on too many rows ---
    println!("* Testing new() with more than one row in the table...");
    SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_SET_VALS.to_string())
        .expect("Test setup failed: Could not truncate table.");
    let conn = sql_interface.get_connection().unwrap();
    let valid_vals = HeatingSetVals {
        heating_switch_off_temperature: 25.0,
        heating_switch_on_temperature: 24.0,
    };
    let mut temp_interface = SqlInterfaceHeatingSetVals {
        conn,
        update_time: None,
        update_duration: Default::default(),
    };
    temp_interface
        .insert_heating_set_values(&valid_vals)
        .expect("Test setup failed: Could not insert first row.");
    temp_interface
        .insert_heating_set_values(&valid_vals)
        .expect("Test setup failed: Could not insert second row.");

    let result =
        SqlInterfaceHeatingSetVals::new(sql_interface.get_connection().unwrap(), &config.heating);
    assert!(
        matches!(
            result,
            Err(SqlInterfaceError::DatabaseHeatingSetValTableContainsTooManyRows(_, _))
        ),
        "Expected row limit error, but got {:?}",
        result
    );
    println!("* Succeeded: Initialization fails if table exceeds row limit.");
}

#[test]
// Test case includes all checks for this table combined in one function to limit the number of additional databases.
// Test case uses the test database #36 - focussing on heatingsetvals table only.
pub fn test_sql_interface_heating_set_vals() {
    let config: ConfigData = read_config_file_with_test_database(
        "config/aquarium_control_test_generic.toml".to_string(),
        36,
    );
    println!("Testing with database {}", config.sql_interface.db_name);
    // *** test reading heating set values **************************************************
    let mut sql_interface: SqlInterface = SqlInterface::new(config.sql_interface.clone())
        .expect("Initialization of SQL interface for test failed.");
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_SET_VALS.to_string()) {
        Ok(_) => {}
        Err(e) => panic!("Could not prepare test case: {e:?}"),
    }

    // instantiation of the test object
    let mut sql_interface_heating_setvals =
        SqlInterfaceHeatingSetVals::new(sql_interface.get_connection().unwrap(), &config.heating)
            .unwrap();

    // check if reading an empty table returns None
    match sql_interface_heating_setvals.get_heating_set_values() {
        Ok(c) => {
            assert!(c.is_none());
        }
        Err(_) => {
            panic!("Call to get_heating_set_values failed.");
        }
    }
    println!("* reading empty heating set values succeeded.");

    // define valid value combination
    let valid_heating_set_vals = HeatingSetVals {
        heating_switch_on_temperature: 20.0,
        heating_switch_off_temperature: 25.0,
    };

    // insert valid values
    assert!(matches!(
        sql_interface_heating_setvals.insert_heating_set_values(&valid_heating_set_vals),
        Ok(())
    ));
    println!("* inserting valid heating set values as precondition succeeded.");

    match sql_interface_heating_setvals.get_heating_set_values() {
        Ok(c) => {
            assert!(c.is_some());
            let heating_set_vals = c.unwrap();
            assert_eq!(
                heating_set_vals.heating_switch_off_temperature,
                valid_heating_set_vals.heating_switch_off_temperature
            );
            assert_eq!(
                heating_set_vals.heating_switch_on_temperature,
                valid_heating_set_vals.heating_switch_on_temperature
            );
        }
        Err(_) => {
            panic!("Call to get_heating_set_values failed.");
        }
    }
    println!("* reading valid heating set values succeeded.");

    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_SET_VALS.to_string()) {
        Ok(_) => {}
        Err(e) => panic!("Could not prepare test case: {e:?}"),
    }

    // define invalid value combination
    let invalid_heating_set_vals = HeatingSetVals {
        heating_switch_on_temperature: 26.0,
        heating_switch_off_temperature: 21.0,
    };

    // insert invalid values
    assert!(matches!(
        sql_interface_heating_setvals.insert_heating_set_values(&invalid_heating_set_vals),
        Ok(())
    ));
    println!("* inserting invalid heating set values as precondition succeeded.");

    // check if reading an invalid value combination returns an error
    assert!(matches!(
        sql_interface_heating_setvals.get_heating_set_values(),
        Err(SqlInterfaceError::HeatingSetValsInvalid(_, _, _))
    ));
    println!("* reading invalid heating set values succeeded.");

    // test if too many rows are detected
    match SqlInterface::truncate_table(&mut sql_interface, SQL_TABLE_HEATING_SET_VALS.to_string()) {
        Ok(_) => {}
        Err(e) => panic!("Could not prepare test case: {e:?}"),
    }
    assert!(matches!(
        sql_interface_heating_setvals.insert_heating_set_values(&valid_heating_set_vals),
        Ok(())
    ));
    assert!(matches!(
        sql_interface_heating_setvals.insert_heating_set_values(&valid_heating_set_vals),
        Ok(())
    ));
    let test_result =
        SqlInterfaceHeatingSetVals::new(sql_interface.get_connection().unwrap(), &config.heating);
    assert!(matches!(
        test_result,
        Err(SqlInterfaceError::DatabaseHeatingSetValTableContainsTooManyRows(_, _))
    ));
    println!("* detecting too many rows succeeded.");

    //***************************************************************************************
}
