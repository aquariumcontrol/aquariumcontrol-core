/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(feature = "debug_schedule_check")]
use log::debug;

#[cfg(all(not(test), target_os = "linux"))]
use log::info;
use log::warn;

#[cfg(all(target_os = "linux", not(test)))]
use nix::unistd::gettid;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use crate::database::sql_interface_schedule::{ScheduleEntry, ScheduleType, SqlInterfaceSchedule};
pub(crate) use crate::permission::schedule_check_channels::ScheduleCheckChannels;
use crate::utilities::channel_content::InternalCommand;
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

/// Holds the configuration data for the schedule checker.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct ScheduleCheckConfig {
    /// reading from the database only happens when the flag is true
    active: bool,

    /// interval for polling the SQL database
    schedule_check_interval: u32,

    /// strategy if entry could not be found in the database:
    /// - true allows actuation
    /// - false inhibits actuation
    strategy_if_schedule_not_found: bool,
}

#[cfg_attr(doc, aquamarine::aquamarine)]
/// Contains the configuration and the implementation for the schedule checker.
/// Thread communication is as follows:
/// ```mermaid
/// graph LR
///     schedule_check[Schedule check] --> ventilation[Ventilation control]
///     ventilation --> schedule_check
///     schedule_check --> heating[Heating control]
///     heating --> schedule_check
///     schedule_check --> refill[Refill control]
///     refill --> schedule_check
///     schedule_check --> balling[Balling dosing control]
///     balling --> schedule_check
///     schedule_check --> signal_handler[Signal handler]
///     signal_handler --> schedule_check
/// ```
pub struct ScheduleCheck {
    /// configuration data for schedule checker
    config: ScheduleCheckConfig,

    /// SQL database interface for Balling dosing control
    sql_interface_schedule: SqlInterfaceSchedule,

    /// inhibition flag to avoid flooding the log file with repeated messages about not finding schedule entry for Balling mineral dosing
    lock_error_find_balling_schedule: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about not finding schedule entry for refill control
    lock_error_find_refill_schedule: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about not finding schedule entry for ventilation control
    lock_error_find_ventilation_schedule: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about not finding schedule entry for heating control
    lock_error_find_heating_schedule: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive information from Balling dosing via the channel
    lock_error_channel_receive_balling: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive information from ventilation control via the channel
    lock_error_channel_receive_ventilation: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive information from refill control via the channel
    lock_error_channel_receive_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive information from heating control via the channel
    lock_error_channel_receive_heating: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send information to Balling mineral dosing via the channel
    lock_error_channel_send_balling: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send information to ventilation control via the channel
    lock_error_channel_send_ventilation: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send information to refill control via the channel
    lock_error_channel_send_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to send information to heating control via the channel
    lock_error_channel_send_heating: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command from Balling mineral dosing
    lock_warn_inapplicable_command_balling: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command from ventilation control
    lock_warn_inapplicable_command_ventilation: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command from refill control
    lock_warn_inapplicable_command_refill: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command from heating control
    lock_warn_inapplicable_command_heating: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read from the database
    lock_error_database_read: bool,

    /// recording when the last database ping happened
    pub last_ping_instant: Instant,

    /// database ping interval
    pub database_ping_interval: Duration,
}

impl ScheduleCheck {
    /// Creates a new `ScheduleCheck` instance.
    ///
    /// This constructor initializes the schedule checking module with its specific
    /// configuration and a connection to the SQL database interface responsible
    /// for schedule data. It also sets up all internal lock flags to `false`,
    /// which are used to prevent log flooding during operation.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the schedule checker, dictating
    ///   behavior like check intervals and strategies for missing schedules.
    /// * `sql_interface_schedule` - A `SqlInterfaceSchedule` instance providing
    ///   the interface to the SQL database for retrieving schedule entries.
    /// * `database_ping_interval` - A `Duration` instance, providing the interval to ping the database.
    ///
    /// # Returns
    /// A new `ScheduleCheck` struct, ready to monitor and evaluate schedules.
    pub fn new(
        config: ScheduleCheckConfig,
        sql_interface_schedule: SqlInterfaceSchedule,
        database_ping_interval: Duration,
    ) -> ScheduleCheck {
        ScheduleCheck {
            config,
            sql_interface_schedule,
            lock_error_find_balling_schedule: false,
            lock_error_find_refill_schedule: false,
            lock_error_find_ventilation_schedule: false,
            lock_error_find_heating_schedule: false,
            lock_error_channel_receive_balling: false,
            lock_error_channel_receive_ventilation: false,
            lock_error_channel_receive_refill: false,
            lock_error_channel_receive_heating: false,
            lock_error_channel_send_balling: false,
            lock_error_channel_send_ventilation: false,
            lock_error_channel_send_refill: false,
            lock_error_channel_send_heating: false,
            lock_warn_inapplicable_command_balling: false,
            lock_warn_inapplicable_command_ventilation: false,
            lock_warn_inapplicable_command_refill: false,
            lock_warn_inapplicable_command_heating: false,
            lock_error_database_read: false,
            last_ping_instant: Instant::now(),
            database_ping_interval,
        }
    }

    /// Checks for and processes a pending `ScheduleCheck` request from another module.
    ///
    /// This private helper function is called repeatedly for each control module
    /// (Balling, Refill, Ventilation, Heating). It attempts to receive
    /// a `ScheduleCheck` command without blocking. If received, it evaluates whether actuation is
    /// currently allowed based on the provided `schedule_entry` and the configured
    /// `strategy_if_schedule_not_found`. The result (a boolean) is then returned
    /// to the calling module.
    ///
    /// The function uses several "lock" flags to prevent repetitive logging of the same error/warning.
    ///
    /// # Arguments
    /// * `config` - A reference to the `ScheduleCheckConfig` for general strategy settings.
    /// * `tx` - The sender channel to send the `bool` result (actuation allowed/disallowed) back to the requesting module.
    /// * `rx` - The receiver channel to receive `InternalCommand` (specifically `ScheduleCheck`) from the requesting module.
    /// * `schedule_entry` - An `Option` containing the `ScheduleEntry` relevant to the requesting module,
    ///   or `None` if no schedule was found for that module.
    /// * `lock_error_channel_receive` - A mutable reference to a boolean flag that, when `true`,
    ///   inhibits repeated error logging for channel receive failures.
    /// * `lock_error_channel_send` - A mutable reference to a boolean flag that, when `true`,
    ///   inhibits repeated error logging for channel send failures.
    /// * `lock_warn_inapplicable_command` - A mutable reference to a boolean flag that, when `true`,
    ///   inhibits repeated warning logging for inapplicable commands received.
    ///
    /// # Returns
    /// * A tuple (bool, bool) indicating a successful send operation and a successful receive-operation.
    fn check_pending_schedule_request(
        config: &ScheduleCheckConfig,
        tx: &Sender<bool>,
        rx: &Receiver<InternalCommand>,
        schedule_entry: Option<&ScheduleEntry>,
        lock_error_channel_receive: &mut bool,
        lock_error_channel_send: &mut bool,
        lock_warn_inapplicable_command: &mut bool,
    ) -> (bool, bool) {
        let mut send_success = false;
        let mut receive_success = false;
        match tx.send(match rx.try_recv() {
            Ok(c) => match c {
                InternalCommand::ScheduleCheck => {
                    *lock_warn_inapplicable_command = false;
                    match schedule_entry {
                        Some(c) => {
                            receive_success = true;
                            c.check_if_allowed()
                        },
                        None => config.strategy_if_schedule_not_found,
                    }
                }
                _ => {
                    if !*lock_warn_inapplicable_command {
                        warn!(
                            target: module_path!(),
                            "ignoring not applicable command ({c:?}) from signal handler"
                        );
                        *lock_warn_inapplicable_command = true;
                    }
                    return (send_success, receive_success);
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => {
                        /* empty buffer - no action required */
                        *lock_error_channel_receive = false;
                    }
                    TryRecvError::Disconnected => {
                        if !*lock_error_channel_receive {
                            log_error_chain(
                                module_path!(),
                                "receiving command from signal handler failed",
                                e,
                            );
                            *lock_error_channel_receive = true;
                        }
                    }
                }
                return (send_success, receive_success);
            }
        }) {
            Ok(_) => {
                send_success = true;
                *lock_error_channel_send = false;
            }
            Err(e) => {
                if !*lock_error_channel_send {
                    log_error_chain(module_path!(), "sending back message failed", e);
                    *lock_error_channel_send = true;
                }
            }
        };
        (send_success, receive_success)
    }

    /// Executes the main control loop for the schedule checker.
    ///
    /// This function runs continuously, managing the validation of various device schedules
    /// against the current time. It periodically reads the latest schedule entries from
    /// the SQL database and, for each relevant module (Ventilation, Heating, Refill, Balling),
    /// it checks if that module is currently allowed to actuate its devices based on its schedule.
    /// The result of this check is communicated back to the respective module via channels.
    ///
    /// The loop continues until a `Quit` command is received from the signal handler.
    /// Upon receiving `Quit`, it sends a confirmation back to the signal handler and
    /// then waits for a `Terminate` command to complete its shutdown.
    ///
    /// # Arguments
    /// * `schedule_check_channels` - A `ScheduleCheckChannels` struct containing all
    ///   necessary `mpsc` sender and receiver channels for communication with
    ///   other application modules and the signal handler.
    pub fn execute(&mut self, schedule_check_channels: &mut ScheduleCheckChannels) {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let sleep_duration_hundred_millis = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();
        let mut loop_counter = 0;

        loop {
            if self.config.active && loop_counter % self.config.schedule_check_interval * 10 == 0 {
                if let Err(errors) = self.sql_interface_schedule.read_schedule() {
                    if !self.lock_error_database_read {
                        for e in errors {
                            log_error_chain(module_path!(), "Reading schedule failed", e);
                        }
                        self.lock_error_database_read = true;
                    }
                } else {
                    self.lock_error_database_read = false;
                }
            }

            let (quit_command_received, _, _) = self.process_external_request(
                &schedule_check_channels.rx_schedule_check_from_signal_handler,
                None,
            );
            if quit_command_received {
                #[cfg(feature = "debug_schedule_check")]
                debug!(
                    target: module_path!(),
                    "received QUIT command"
                );

                break;
            }

            //*********** ventilation ***********
            let schedule_entry_ventilation_opt = match self
                .sql_interface_schedule
                .find_schedule(ScheduleType::Ventilation)
            {
                Ok(c) => {
                    self.lock_error_find_ventilation_schedule = false;
                    Some(c)
                }
                Err(e) => {
                    if !self.lock_error_find_ventilation_schedule {
                        log_error_chain(
                            module_path!(),
                            "could not find schedule for ventilation",
                            e,
                        );
                        self.lock_error_find_ventilation_schedule = true;
                    }
                    None
                }
            };
            let (_send_success, _receive_success) = Self::check_pending_schedule_request(
                &self.config,
                &schedule_check_channels.tx_schedule_check_to_ventilation,
                &schedule_check_channels.rx_schedule_check_from_ventilation,
                schedule_entry_ventilation_opt,
                &mut self.lock_error_channel_receive_ventilation,
                &mut self.lock_error_channel_send_ventilation,
                &mut self.lock_warn_inapplicable_command_ventilation,
            );
            #[cfg(feature = "debug_channels")]
            {
                if _send_success {
                    schedule_check_channels.cnt_rx_schedule_check_from_ventilation += 1;
                }
                if _receive_success {
                    schedule_check_channels.cnt_tx_schedule_check_to_ventilation += 1;
                }
            }
            //***************************************

            //*********** heating ***********
            let schedule_entry_heating_opt = match self
                .sql_interface_schedule
                .find_schedule(ScheduleType::Heating)
            {
                Ok(c) => {
                    self.lock_error_find_heating_schedule = false;
                    Some(c)
                }
                Err(e) => {
                    if !self.lock_error_find_heating_schedule {
                        log_error_chain(module_path!(), "could not find schedule for heating", e);
                        self.lock_error_find_heating_schedule = true;
                    }
                    None
                }
            };
            let (_send_success, _receive_success) = Self::check_pending_schedule_request(
                &self.config,
                &schedule_check_channels.tx_schedule_check_to_heating,
                &schedule_check_channels.rx_schedule_check_from_heating,
                schedule_entry_heating_opt,
                &mut self.lock_error_channel_receive_heating,
                &mut self.lock_error_channel_send_heating,
                &mut self.lock_warn_inapplicable_command_heating,
            );
            #[cfg(feature = "debug_channels")]
            {
                if _send_success {
                    schedule_check_channels.cnt_rx_schedule_check_from_heating += 1;
                }
                if _receive_success {
                    schedule_check_channels.cnt_tx_schedule_check_to_heating += 1;
                }
            }
            //***************************************

            //*********** refill ***********
            let schedule_entry_refill_opt = match self
                .sql_interface_schedule
                .find_schedule(ScheduleType::Refill)
            {
                Ok(c) => {
                    self.lock_error_find_refill_schedule = false;
                    Some(c)
                }
                Err(e) => {
                    if !self.lock_error_find_refill_schedule {
                        log_error_chain(module_path!(), "could not find schedule for refill", e);
                        self.lock_error_find_refill_schedule = true;
                    }
                    None
                }
            };
            let (_send_success, _receive_success) = Self::check_pending_schedule_request(
                &self.config,
                &schedule_check_channels.tx_schedule_check_to_refill,
                &schedule_check_channels.rx_schedule_check_from_refill,
                schedule_entry_refill_opt,
                &mut self.lock_error_channel_receive_refill,
                &mut self.lock_error_channel_send_refill,
                &mut self.lock_warn_inapplicable_command_refill,
            );
            #[cfg(feature = "debug_channels")]
            {
                if _send_success {
                    schedule_check_channels.cnt_rx_schedule_check_from_refill += 1;
                }
                if _receive_success {
                    schedule_check_channels.cnt_tx_schedule_check_to_refill += 1;
                }
            }
            //***************************************

            //*********** Balling ***********
            let schedule_entry_balling_opt = match self
                .sql_interface_schedule
                .find_schedule(ScheduleType::Balling)
            {
                Ok(c) => {
                    self.lock_error_find_balling_schedule = false;
                    Some(c)
                }
                Err(e) => {
                    if !self.lock_error_find_balling_schedule {
                        log_error_chain(module_path!(), "could not find schedule for balling", e);
                        self.lock_error_find_balling_schedule = true;
                    }
                    None
                }
            };
            let (_send_success, _receive_success) = Self::check_pending_schedule_request(
                &self.config,
                &schedule_check_channels.tx_schedule_check_to_balling,
                &schedule_check_channels.rx_schedule_check_from_balling,
                schedule_entry_balling_opt,
                &mut self.lock_error_channel_receive_balling,
                &mut self.lock_error_channel_send_balling,
                &mut self.lock_warn_inapplicable_command_balling,
            );
            #[cfg(feature = "debug_channels")]
            {
                if _send_success {
                    schedule_check_channels.cnt_rx_schedule_check_from_balling += 1;
                }
                if _receive_success {
                    schedule_check_channels.cnt_tx_schedule_check_to_balling += 1;
                }
            }
            //***************************************

            spin_sleeper.sleep(sleep_duration_hundred_millis);

            loop_counter += 1;

            // avoid overflow
            if loop_counter == 1_000_000 {
                loop_counter = 0;
            }

            // Check for how long ago the last database ping happened.
            if Instant::now().duration_since(self.last_ping_instant) > self.database_ping_interval {
                self.sql_interface_schedule.ping();
                self.last_ping_instant = Instant::now();
            }
        }

        // The application received the request to terminate. That is why the loop was left.
        // Answer to the thread which sent the request for termination, so that shutdown can proceed further.
        match schedule_check_channels.send_to_signal_handler(true)
        {
            Ok(()) => {
                #[cfg(feature = "debug_schedule_check")]
                debug!(
                    target: module_path!(),
                    "sent back confirmation to schedule checker"
                );
            }
            Err(e) => {
                log_error_chain(
                    module_path!(),
                    "sending quit confirmation back to signal handler failed",
                    e,
                );
            }
        };
    }
}
