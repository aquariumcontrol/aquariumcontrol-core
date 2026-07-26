/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Executes the fresh water filling operation.

use chrono::Local;
use log::error;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::database::sql_interface_refill::{
    DatabaseInterfaceRefillTrait, DatabaseRefillErrorCode,
};
use crate::sensors::tank_level_switch::TankLevelSwitchSignals;
use crate::utilities::channel_content::{AquariumDevice, InternalCommand};
use crate::utilities::check_mutex_access_duration::CheckMutexAccessDurationTrait;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::water::refill_channels::RefillChannels;
use crate::water::refill_config::RefillConfig;
use crate::water::refill_error_states::RefillErrorStates;

/// allow max. 10 milliseconds for mutex to be blocked by any other thread
const MAX_MUTEX_ACCESS_DURATION_MILLIS: u64 = 10;

/// Trait for the execution of the refill once control has detected that the water level is low.
/// This trait allows running the main control with a mock implementation for testing.
///
/// # Arguments
/// * `refill_channels` - A mutable reference to the `RefillChannels` communication interface.
/// * `refill_errors` - A parameter representing `refill_errors`.
/// * `sql_interface_refill` - Database interface connection for refill operations.
/// * `mutex_tank_level_switch` - Shared mutex for the tank level switch signals.
/// * `mutex_device_scheduler_refill` - Mutex coordinating access to the device scheduler for refill.
pub trait WaterInjectionTrait {
    #[allow(async_fn_in_trait)]
    /// [impl->swreq~water_hysteresis~1]
    async fn inject_water(
        &mut self,
        refill_channels: &mut RefillChannels,
        refill_errors: &mut RefillErrorStates,
        sql_interface_refill: &mut Box<dyn DatabaseInterfaceRefillTrait + Sync + Send>,
        mutex_tank_level_switch: &Arc<Mutex<TankLevelSwitchSignals>>,
        mutex_device_scheduler_refill: &Arc<tokio::sync::Mutex<i32>>,
    ) -> bool;
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Implementation of the WaterInjectionTrait which contains the code for executing the refill once
/// the main control has detected that the water level is low.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     water_injection[Water Injection] --> relay_manager[Relay Manager]
///     relay_manager --> water_injection
///     tank_level_switch[TankLevelSwitch] -.-> water_injection
///     signal_handler[Signal Handler] --> water_injection
/// ```
pub struct WaterInjection {
    /// The volume of water (in Liters) injected during a single control loop cycle.
    /// This value is calculated at initialization from the pump's flow rate and the loop's sleep interval.
    refill_increment: f64,

    /// The extra volume of water to add after the tank level switch first reports `high`.
    /// This acts as a hysteresis to prevent the pump from cycling too frequently.
    additional_refill_volume: f64,

    /// The maximum total volume of water (in Liters) allowed for a single refill operation.
    /// This serves as a safety cutoff to prevent overfilling if the level switch fails.
    max_refill_volume: f64,

    /// The `Duration` for which the control loop sleeps on each iteration (e.g., 100 ms).
    sleep_duration_hundred_milli_sec: Duration,

    /// The sleep interval of the control loop, expressed in seconds as a floating-point number
    /// for use in volume calculations.
    sleep_interval_seconds: f64,

    /// An inhibition flag to prevent flooding the log file with repeated warnings
    /// about excessive mutex access times.
    pub lock_warn_max_mutex_access_duration: bool,

    /// A flag used for testing purposes to record if the mutex access time was ever exceeded.
    /// Unlike the lock flag, this is not reset.
    #[cfg(any(test, feature = "test-utils"))]
    pub mutex_access_duration_exceeded: bool,

    /// The maximum permissible duration for a mutex lock to be held before a warning is issued.
    pub max_mutex_access_duration: Duration,
}

impl WaterInjection {
    /// Creates a new `WaterInjection` instance.
    ///
    /// This constructor initializes the water injection module, setting up parameters
    /// crucial for controlling the refill pump during a water injection operation.
    /// It calculates the `refill_increment` based on pump flow, copies refill limits
    /// from the configuration, and sets up internal timing mechanisms.
    /// All internal "lock" flags (used to prevent log flooding) are initialized to `false`.
    ///
    /// # Arguments
    /// * `config` - A reference to the **`RefillConfig`** struct, which contains
    ///   essential parameters such as `refill_pump_flow`, `additional_refill_volume`,
    ///   and `max_refill_volume`.
    ///
    /// # Returns
    /// A new **`WaterInjection` struct**, ready to perform water injection operations.
    /// [impl->swreq~water_calibration_parameters~1]
    pub fn new(config: &RefillConfig) -> WaterInjection {
        let sleep_interval_millis = 100;
        let sleep_interval_seconds: f64 = (sleep_interval_millis as f64) / (1000f64);

        WaterInjection {
            refill_increment: sleep_interval_seconds * config.refill_pump_flow,
            additional_refill_volume: config.additional_refill_volume,
            max_refill_volume: config.max_refill_volume,
            sleep_duration_hundred_milli_sec: Duration::from_millis(sleep_interval_millis),
            sleep_interval_seconds,
            lock_warn_max_mutex_access_duration: false,
            #[cfg(any(test, feature = "test-utils"))]
            mutex_access_duration_exceeded: false,
            max_mutex_access_duration: Duration::from_millis(MAX_MUTEX_ACCESS_DURATION_MILLIS),
        }
    }
}

impl WaterInjectionTrait for WaterInjection {
    /// Executes the freshwater refill operation, controlling the refill pump and monitoring tank level.
    ///
    /// This function orchestrates the physical water injection. It first switches on the refill pump,
    /// then enters a loop to continuously monitor the tank level switch and check for various
    /// termination conditions:
    /// - Water level reaching `high` (plus `additional_refill_volume`).
    /// - Tank level switch signal becoming `invalid`.
    /// - Total refilled `volume` exceeding `max_refill_volume` (safety timeout).
    /// - A `Quit` command being received from the signal handler.
    ///
    /// During the refill, it periodically communicates the pump status to the `DataLogger`
    /// and logs the final refill event details (timestamp, duration, volume, error code) to the database.
    ///
    /// # Arguments
    /// * `refill_channels` - A mutable reference to a struct containing all necessary communication channels with
    ///   `RelayManager`, `DataLogger`, and `SignalHandler`.
    /// * `refill_errors` - A mutable reference to a `RefillErrors` struct, where specific error
    ///   flags (e.g., `error_switch_stuck_low`, `error_sql_update_failed`) will be set if issues occur during injection.
    /// * `sql_interface_refill` - A mutable reference to the SQL database interface for refill control,
    ///   used to log the refill event.
    /// * `mutex_tank_level_switch_signals` - A mutable reference to the `Arc<Mutex<TankLevelSwitchSignals>>`
    ///   for reading the current tank level switch position and validity.
    ///
    /// # Returns
    /// `true` if the refill operation was **aborted due to a `Quit` command** received from the signal handler;
    /// `false` otherwise (meaning it completed normally or aborted due to other conditions like level reached or errors).
    /// * `mutex_device_scheduler_refill` - Mutex coordinating access to the device scheduler for refill.
    async fn inject_water(
        &mut self,
        refill_channels: &mut RefillChannels,
        refill_errors: &mut RefillErrorStates,
        sql_interface_refill: &mut Box<dyn DatabaseInterfaceRefillTrait + Sync + Send>,
        mutex_tank_level_switch_signals: &Arc<Mutex<TankLevelSwitchSignals>>,
        mutex_device_scheduler_refill: &Arc<tokio::sync::Mutex<i32>>,
    ) -> bool {
        let mut mutex_device_scheduler = mutex_device_scheduler_refill.lock().await;
        let mut refill_volume = 0.0;
        let mut refill_time_seconds = 0.0;
        let mut refill_error_code = DatabaseRefillErrorCode::Success;
        let mut additional_refill_volume_executed = 0.0;
        let mut tank_level_switch_position = false;
        let mut tank_level_switch_invalid = false;
        let mut quit_command_received = false;

        // command start of refill pump
        match refill_channels
            .send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::RefillPump))
        {
            Ok(_) => {
                // now listen for the response
                match refill_channels.receive_from_relay_manager().await {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            target: module_path!(),
                            "receiving answer from relay manager when switching on refill pump failed ({e:?})"
                        );
                    }
                };
            }
            Err(e) => {
                error!(
                    target: module_path!(),
                    "channel communication to relay manager when switching on refill pump failed ({e:?})"
                );
            }
        }

        //*** wait for condition to stop refilling ***
        loop {
            tokio::time::sleep(self.sleep_duration_hundred_milli_sec).await;
            refill_time_seconds += self.sleep_interval_seconds;

            let instant_before_locking_mutex = Instant::now();
            let mut instant_after_locking_mutex = Instant::now(); // initialization is overwritten

            // access mutex for reading tank level switch signals
            {
                match mutex_tank_level_switch_signals.lock() {
                    Ok(c) => {
                        tank_level_switch_position = c.tank_level_switch_position;
                        tank_level_switch_invalid = c.tank_level_switch_invalid;
                        instant_after_locking_mutex = Instant::now();
                    }
                    Err(_) => {
                        // Do nothing, keep values as they are.
                    }
                }
            }

            // check if access to mutex took too long
            self.check_mutex_access_duration(
                None,
                instant_after_locking_mutex,
                instant_before_locking_mutex,
            );

            if tank_level_switch_position
                && additional_refill_volume_executed > self.additional_refill_volume
            {
                // the water level changed to high
                break;
            }
            if tank_level_switch_invalid {
                // switch signal is invalid
                refill_error_code = DatabaseRefillErrorCode::InvalidSwitch;
                break;
            }

            // calculate volume refilled in this operation
            refill_volume += self.refill_increment;
            if tank_level_switch_position {
                additional_refill_volume_executed += self.refill_increment;
            }
            (quit_command_received, _, _) = self
                .process_external_request(&mut refill_channels.rx_refill_from_signal_handler, None);
            if quit_command_received {
                break;
            }
            if refill_volume > self.max_refill_volume {
                refill_errors.error_switch_stuck_low = true;
                refill_error_code = DatabaseRefillErrorCode::StuckLowTimeout;
                break;
            }
        }

        // command stop of refill pump
        match refill_channels
            .send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::RefillPump))
        {
            Ok(_) => {
                // now listen for the response
                match refill_channels.receive_from_relay_manager().await {
                    Ok(_) => {}
                    Err(e) => {
                        error!(
                            target: module_path!(),
                            "receiving answer from relay manager when switching off refill pump failed ({e:?})"
                        );
                    }
                };
            }
            Err(e) => {
                error!(
                    target: module_path!(),
                    "channel communication to relay manager when switching off refill pump failed ({e:?})"
                );
            }
        }

        *mutex_device_scheduler = mutex_device_scheduler.saturating_add(1);

        // write the result to the database
        match sql_interface_refill.insert_refill_event(
            Local::now().naive_local(),
            refill_time_seconds,
            refill_volume,
            refill_error_code,
        ) {
            Ok(_) => {}
            Err(e) => {
                error!(
                    target: module_path!(),
                    "writing refill operation data to SQL database failed ({e:?})"
                );
                refill_errors.error_sql_update_failed = true;
            }
        }

        quit_command_received // return information if refill was aborted due to request from signal handler
    }
}
