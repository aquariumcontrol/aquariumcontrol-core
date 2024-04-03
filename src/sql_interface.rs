use chrono::*;
use mysql::prelude::*;
use mysql::*;
use serde_derive::Deserialize;

/// This struct holds the configuration data for the SQL interface.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct SqlInterfaceConfig {
    /// database user
    pub db_user: String,

    /// database password
    pub db_password: String,

    /// hostname of database
    pub db_host: String,

    /// port name of database
    pub db_port: u16,

    /// database name
    pub db_name: String,
}

/*
#[derive(Debug, PartialEq, Eq)]
struct Car {
    id: i32,
    name: String,
    price: i32,
}
*/

struct TimestampString {
    timestamp: String,
}

/// This struct contains the configuration and the implementation of the SQL interface.
pub struct SqlInterface {
    /// The configuration data read from the .toml file.
    pub config: SqlInterfaceConfig,
}

impl SqlInterface {
    pub fn new(config: SqlInterfaceConfig) -> SqlInterface {
        SqlInterface { config }
    }

    pub fn testmariadb() -> std::result::Result<std::string::String, mysql::Error> {
        let url = "mysql://aquarium:4w4umx7AzdAwCXi7@127.0.0.1:3306/aquarium";

        println!("Connecting to database...");
        let pool = match Pool::new(url) {
            Ok(c) => {
                println!("URL pool established.");
                c
            }
            Err(e) => {
                panic!("Could not establish URL pool with {}: {}", url, e);
            }
        };

        let mut conn = match pool.get_conn() {
            Ok(c) => {
                println!("Database connection opened.");
                c
            }
            // Handle the `error` case.
            Err(_) => {
                panic!("Could not open data base connection.");
            }
        };

        println!("Executing query");

        let current_timestamp_string_array = conn
            .query_map("SELECT CURRENT_TIMESTAMP();", |timestamp| TimestampString {
                timestamp,
            })?;
        // todo: error checking

        // todo: check if array has exactly one element
        let current_timestamp_string = &current_timestamp_string_array[0].timestamp;

        // todo: error checking
        let current_datetime =
            match NaiveDateTime::parse_from_str(&current_timestamp_string, "%Y-%m-%d %H:%M:%S") {
                Ok(c) => {
                    println!("Converted current timestamp successfully.");
                    c
                }
                Err(e) => {
                    panic!("Error when converting current timestamp: {}", e);
                }
            };

        println!("current_datetime = {}", current_datetime);

        let last_refill_timestamp_string_array = conn.query_map(
            "SELECT Timestamp from aquarium.refill as LSTTSTMP ORDER BY Timestamp DESC LIMIT 1;",
            |timestamp| TimestampString { timestamp },
        )?;
        // todo: error checking

        // todo: check if array has exactly one element
        let last_refill_timestamp_string = &last_refill_timestamp_string_array[0].timestamp;

        // todo: error checking
        let last_refill_datetime =
            match NaiveDateTime::parse_from_str(&last_refill_timestamp_string, "%Y-%m-%d %H:%M:%S")
            {
                Ok(c) => {
                    println!("Converted last refill timestamp successfully.");
                    c
                }
                Err(e) => {
                    panic!("Error when converting last refill timestamp: {}", e);
                }
            };

        println!("last_refill_datetime = {}", last_refill_datetime);

        let delta_time = current_datetime - last_refill_datetime;

        println!("delta_time.num_seconds() = {}", delta_time.num_seconds());

        Ok("Yay!".into())
    }
}
