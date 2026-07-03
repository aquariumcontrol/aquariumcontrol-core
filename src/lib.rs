/// This container module features all functionality related to RGB LED.
pub mod beacon;
/// This is the container module for the database communication.
/// Communicating with the SQL database includes preparing SQL queries, executing the queries and processing of query results.
pub mod database;
/// Communication with the user via client application or webserver using POSIX message queues.
pub mod dispatch;
/// This container module features all the functionality to execute the automatic feeding.
pub mod food;
/// This container module features all the functionality to execute the Balling mineral dosing.
pub mod mineral;
#[cfg(test)]
/// This container module features a set of mock functionality used by test cases.
pub mod mocks;
/// The container module contains the schedule checker.
pub mod permission;
/// This container module contains all functionality to periodically store signals either in one table of the SQL database or in specific text files stored in the RAM disk
pub mod recorder;
/// This container module features all functionality to actuate the relays which control the actuators.
pub mod relays;
/// This container module features all functionality to read and postprocess sensor data.
pub mod sensors;
/// This container module features all functionality to communicate with the test server via TCP.
pub mod simulator;
/// This container module features all functionality to either cool the water using ventilation heat the water using a heater.
pub mod thermal;
/// This container module features all functionality that could not be assigned to any other container module.
pub mod utilities;
/// This container module features all functionality to monitor the control system.
pub mod watchmen;
/// This container module features all functionality to replace the evaporated water with fresh water.
pub mod water;

/// This container module features all supporting functionality for the main module.
pub mod launch;

/// Functionality for test server
pub mod test_server;
