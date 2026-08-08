/* Copyright 2024 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! A dedicated thread for monitoring and stabilizing a tank water level switch.
//!
//! This module implements the `TankLevelSwitch`, a long-running thread responsible
//! for reading the state of a physical float switch. Its primary purpose is not
//! just to read the raw signal, but to apply a stabilization algorithm to prevent
//! rapid state changes ("chatter") caused by water movement.
//!
//! The module operates in two distinct modes:
//! 1.  **Hardware Mode**: Reads the state of a GPIO pin on a Raspberry Pi.
//! 2.  **Simulator Mode**: Communicates via TCP with a simulator to get mock data.
//!
//! The final, stabilized data is written to a shared `Arc<Mutex<...>>` for other
//! threads (like `RefillControl`) to consume safely.
//!
//! ## Key Components
//!
//! - **`TankLevelSwitch` Struct**: The core state machine for the thread. It holds the
//!   configuration, the raw and stabilized switch positions, and state for the
//!   stabilization counter.
//!
//! - **`execute()` Method**: The main thread loop. It runs at a fixed cycle time,
//!   periodically calling `update_tank_level_switch_signals` to get the raw
//!   input and `calc_tank_level_switch_position_stabilized` to apply the logic.
//!
//! - **`calc_tank_level_switch_position_stabilized()`**: The heart of the module's logic.
//!   It implements an asymmetric delay:
//!   - A transition to **high water** (`true`) is immediate.
//!   - A transition to **low water** (`false`) is delayed, requiring the raw signal
//!     to remain low for a configured number of cycles. This prevents the refill
//!     pump from rapidly turning on and off due to waves.
//!
//! - **`TankLevelSwitchSignals` Struct**: The output data structure. After processing,
//!   the `TankLevelSwitch` writes the raw position, stabilized position, and an
//!   invalid flag into this struct, which is shared via a mutex.
//!
//! ## Design and Architecture
//!
//! - **Real-Time Monitoring and Stabilization**: By running in its own high-frequency
//!   loop, this thread can provide responsive and reliable water level data. The
//!   stabilization logic is critical for robust automation, especially for controlling pumps.
//!
//! - **Decoupling via Mutex**: The module acts as a data *producer*. Consumer threads
//!   (like `RefillControl` or `DataLogger`) are completely decoupled; they simply
//!   read the latest state from the shared `TankLevelSwitchSignals` mutex without
//!   needing to know how the data was obtained or processed.
//!
//! - **Dual-Mode Operation**: The use of `#[cfg]` attributes and the `use_simulator`
//!   configuration flag allows the entire application to be developed and tested on a
//!   machine without GPIO hardware, significantly improving the development workflow.
//!
//! ### Thread Communication
//!
//! The `TankLevelSwitch` communicates with the `SignalHandler` for graceful shutdown
//! and with a `TcpCommunication` thread in simulator mode. It provides data to
//! consumers via a shared mutex.

#[cfg(feature = "debug_tank_level_switch")]
use log::debug;
use std::sync::{Arc, Mutex};

#[cfg(all(target_os = "linux", not(test), not(feature = "test-utils")))]
// only required in real-life execution
use nix::unistd::gettid;

use log::error;
#[cfg(all(target_os = "linux", not(test), not(feature = "test-utils")))]
// only required in real-life execution
use log::info;

#[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
use log::warn;

use crate::launch::channels::{AquaReceiver, AquaSender};
use crate::sensors::tank_level_switch_channels::TankLevelSwitchChannels;
use crate::sensors::tank_level_switch_config::TankLevelSwitchConfig;
pub use crate::sensors::tank_level_switch_error::TankLevelSwitchError;
use crate::utilities::check_mutex_access_duration::CheckMutexAccessDurationTrait;
use std::time::{Duration, Instant};

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
            use rppal::gpio::{Gpio, InputPin};
    }
}

use crate::simulator::get_resp_sim::GetResponseFromSimulatorTrait;
use crate::simulator::tcp_communication_error::TcpCommunicationError;
use crate::utilities::acknowledge_signal_handler::AcknowledgeSignalHandlerTrait;
use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
use crate::utilities::logger::log_error_chain;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
use crate::utilities::wait_for_termination::WaitForTerminationTrait;

/// application is monitoring if execution duration can maintain this limit
/// sleep time is adjusted accordingly to run at fixed execution time
const CYCLE_TIME_TANK_LEVEL_SWITCH_MILLIS: u64 = 100;

/// application is adapting the sleep time to achieve constant execution rate of 100 milliseconds.
/// this is the threshold from which on the cycle time monitoring alerts.
const PERMISSIBLE_CYCLE_TIME_DEVIATION_TANK_LEVEL_SWITCH_MILLIS: u128 = 5;

/// application is adapting the sleep time to achieve constant execution rate of 100 milliseconds.
/// this is the threshold from which on the cycle time monitoring alerts - when testing with simulator.
const PERMISSIBLE_CYCLE_TIME_DEVIATION_TANK_LEVEL_SWITCH_MILLIS_SIMULATOR: u128 = 500;

/// allow max. 10 milliseconds for mutex to be blocked by any other thread
const MAX_MUTEX_ACCESS_DURATION_MILLIS: u64 = 10;

/// Contains all signal information from the tank level switch relevant for other threads:
/// - measured tank level switch position
/// - stabilized tank level switch position
/// - flag indicating if the tank level switch position is invalid
pub struct TankLevelSwitchSignals {
    /// measured pin state
    pub tank_level_switch_position: bool,

    /// calculated stabilized pin state
    pub tank_level_switch_position_stabilized: bool,

    /// flag indicating if the tank level switch position is invalid
    pub tank_level_switch_invalid: bool,
}

impl TankLevelSwitchSignals {
    /// Creates a new `TankLevelSwitchSignals` struct with initial status values.
    ///
    /// This constructor sets the initially measured position, the calculated stabilized position,
    /// and the validity flag of the tank level switch. It's a general-purpose constructor
    /// for this data-holding struct.
    ///
    /// # Arguments
    /// * `tank_level_switch_position` - The initial measured pin state (`true` for high water, `false` for low).
    /// * `tank_level_switch_position_stabilized` - The initial calculated stabilized pin state.
    /// * `tank_level_switch_invalid` - The initial flag indicating if the switch position is considered invalid.
    ///
    /// # Returns
    /// A new `TankLevelSwitchSignals` struct.
    pub fn new(
        tank_level_switch_position: bool,
        tank_level_switch_position_stabilized: bool,
        tank_level_switch_invalid: bool,
    ) -> TankLevelSwitchSignals {
        TankLevelSwitchSignals {
            tank_level_switch_position,
            tank_level_switch_position_stabilized,
            tank_level_switch_invalid,
        }
    }
}

/// Contains the measurement of tank level position and calculation of stabilized tank level position signal.
/// Tank level position is measured by accessing the state of a dedicated GPIO pin.
/// Alternatively, TCP communication is used when configured to run with the simulator.
/// The struct holds attributes for the results and for error flags indicating unsuccessful communication.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     signal_handler[SignalHandler] --> tank_level_switch
///     tank_level_switch --> signal_handler
///     tank_level_switch --> tcp_communication[TcpCommunication]
///     tcp_communication --> tank_level_switch
/// ```
/// Communication with the simulator (TcpCommunication) and signal handler uses
/// channels. Refill control, Heating control and the data logger read from a mutex to which
/// TankLevelSwitch writes to.
pub struct TankLevelSwitch {
    /// configuration of tank level switch calculation
    config: TankLevelSwitchConfig,

    /// flag indicating if the last measurement of tank level switch indicates high (true) or low (false)
    tank_level_switch_position: bool,

    /// flag indicating if the stabilized tank level switch indicates high (true) or low (false)
    tank_level_switch_position_stabilized: bool,

    /// flag indicating that the tank level switch position may be inaccurate
    tank_level_switch_invalid: bool,

    /// time counter to determine if the position is stabilized
    tank_level_switch_position_low_stabilization_counter: u32,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command from the signal handler
    pub lock_warn_inapplicable_command_signal_handler: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    pub lock_error_channel_receive_termination: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive access time to mutex
    pub lock_warn_max_mutex_access_duration: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about mutex not being accessible.
    lock_error_mutex_poisoned: bool,

    // for testing purposes: record when mutex access time is exceeded without resetting it
    #[cfg(any(test, feature = "test-utils"))]
    pub mutex_access_duration_exceeded: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about not having received an input pin
    lock_error_input_pin_none: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about simulator communication error
    lock_error_simulator_communication: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about channel(s) not being provided
    lock_error_channel_not_provided: bool,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    // interface to GPIO pin wrapped as optional
    input_pin_opt: Option<InputPin>,

    /// Maximum permissible access duration for Mutex
    pub max_mutex_access_duration: Duration,

    /// inhibition flag to avoid flooding the log file with repeated messages about excessive cycle time
    pub lock_warn_cycle_time_exceeded: bool,
}

impl ProcessExternalRequestTrait for TankLevelSwitch {}

impl GetResponseFromSimulatorTrait for TankLevelSwitch {}

impl TankLevelSwitch {
    #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
    /// Creates a new **`TankLevelSwitch`** instance for **development or unsupported platforms**.
    ///
    /// This constructor is activated when the application is compiled for a non-Linux
    /// operating system or when the `target_hw` feature is not enabled. In this mode,
    /// it acts as a **mock or placeholder**, allowing basic testing and compilation
    /// without requiring actual GPIO hardware. It initializes the sensor position to
    /// `true` (high water) and the invalid flag to `false` by default, but sets the
    /// stabilized position based on the provided `initial_position_stabilized` argument.
    ///
    /// # Arguments
    /// * `config` - **Configuration data** for the tank level switch, primarily dictating
    ///   the `use_simulator` flag and stabilization logic.
    /// * `initial_position_stabilized` - The **assumed initially stabilized position** of the
    ///   tank level (`true` for high water, `false` for low) before any actual measurements.
    ///
    /// # Returns
    /// A new **`TankLevelSwitch` struct**, configured for simulated or non-hardware operation, wrapped in Ok().
    ///
    /// # Errors
    /// This specific implementation for non-hardware platforms does not produce any errors
    /// and always returns `Ok`. The `Result` type is used to maintain signature
    /// compatibility with the hardware-specific version of this function.
    pub fn new(
        config: TankLevelSwitchConfig,
        initial_position_stabilized: bool,
    ) -> Result<TankLevelSwitch, TankLevelSwitchError> {
        Ok(TankLevelSwitch {
            config,
            tank_level_switch_position: true, // initialization value assuming no error
            tank_level_switch_invalid: false, // initialization value assuming no error
            tank_level_switch_position_stabilized: initial_position_stabilized,
            tank_level_switch_position_low_stabilization_counter: 0,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
            lock_warn_max_mutex_access_duration: false,
            lock_error_mutex_poisoned: false,
            #[cfg(any(test, feature = "test-utils"))]
            mutex_access_duration_exceeded: false,
            lock_error_input_pin_none: false,
            lock_error_simulator_communication: false,
            lock_error_channel_not_provided: false,
            max_mutex_access_duration: Duration::from_millis(MAX_MUTEX_ACCESS_DURATION_MILLIS),
            lock_warn_cycle_time_exceeded: false,
        })
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Creates a new `TankLevelSwitch` instance for target hardware platforms.
    ///
    /// This constructor initializes the tank level switch monitoring module. If not in
    /// simulator mode, it establishes access to the physical GPIO pin, configuring it
    /// as an input with a pull-up resistor.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the tank level switch.
    /// * `initial_position_stabilized` - The assumed initially stabilized position of the
    ///   tank level (`true` for high water, `false` for low).
    /// * `gpio_lib_handle_opt` - An `Option<Gpio>` providing the interface to the GPIO library.
    ///   This must be `Some(Gpio)` if `config.use_simulator` is `false`.
    /// * `gpio_tank_level` - The GPIO pin (`u8`) connected to the tank level switch.
    ///
    /// # Returns
    /// A `Result` containing a new `TankLevelSwitch` instance on success.
    ///
    /// # Errors
    /// Returns a `TankLevelSwitchError` if initialization fails:
    /// - `TankLevelSwitchError::GPIOInterfaceNotProvided`: If `use_simulator` is `false`
    ///   but no `Gpio` handle is provided.
    /// - `TankLevelSwitchError::GpioPinRetrievalError`: If the specified GPIO pin cannot be
    ///   retrieved from the GPIO interface (e.g., invalid pin, permissions issue).
    pub fn new(
        config: TankLevelSwitchConfig,
        initial_position_stabilized: bool,
        gpio_lib_handle_opt: Option<Gpio>,
        gpio_tank_level: u8,
    ) -> Result<TankLevelSwitch, TankLevelSwitchError> {
        let input_pin_opt = if config.use_simulator {
            // In simulator mode, we don't need a real pin.
            None
        } else {
            // In hardware mode, the Gpio handle is required.
            let gpio_lib_handle = gpio_lib_handle_opt.ok_or_else(|| {
                TankLevelSwitchError::GPIOInterfaceNotProvided(module_path!().to_string())
            })?;

            // Get the specific pin for tank level monitoring.
            let tank_level_pin = gpio_lib_handle.get(gpio_tank_level).map_err(|e| {
                TankLevelSwitchError::GpioPinRetrievalError {
                    location: module_path!().to_string(),
                    pin_number: gpio_tank_level,
                    source: e,
                }
            })?;
            Some(tank_level_pin.into_input_pullup())
        };

        Ok(TankLevelSwitch {
            config,
            tank_level_switch_position: true, // initialization value assuming no error
            tank_level_switch_invalid: false, // initialization value assuming no error
            tank_level_switch_position_stabilized: initial_position_stabilized,
            tank_level_switch_position_low_stabilization_counter: 0,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
            lock_warn_max_mutex_access_duration: false,
            lock_error_mutex_poisoned: false,
            #[cfg(any(test, feature = "test-utils"))]
            mutex_access_duration_exceeded: false,
            lock_error_input_pin_none: false,
            input_pin_opt,
            lock_error_channel_not_provided: false,
            lock_error_simulator_communication: false,
            max_mutex_access_duration: Duration::from_millis(MAX_MUTEX_ACCESS_DURATION_MILLIS),
        })
    }

    /// Updates the measured tank level switch signals from either a GPIO pin or a simulator.
    ///
    /// This function determines the source of the tank level switch position and invalid status
    /// based on the `use_simulator` configuration.
    ///
    /// # Arguments
    /// * `tx_tank_level_switch_to_tcp_opt` - An `Option` containing the sender channel to the TCP simulator.
    /// * `rx_tank_level_switch_from_tcp_opt` - An `Option` containing the receiver channel from the TCP simulator.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) on success.
    ///
    /// # Errors
    /// Returns a `TankLevelSwitchError` if:
    /// - `TankLevelSwitchError::SimulatorChannelNotProvided`: If in simulator mode, and the
    ///   required channels are missing.
    /// - `TankLevelSwitchError::SimulatorCommunicationError`: If communication with the
    ///   simulator fails for either signal.
    /// - `TankLevelSwitchError::InputPinNotProvided`: If in hardware mode but the GPIO
    ///   input pin is not available.
    ///
    /// [impl->swreq~sensors_tank_level_switch_preprocessing~1]
    pub fn update_tank_level_switch_signals(
        &mut self,
        tx_tank_level_switch_to_tcp_opt: &mut Option<AquaSender<InternalCommand>>,
        rx_tank_level_switch_from_tcp_opt: &mut Option<
            AquaReceiver<Result<f32, TcpCommunicationError>>,
        >,
    ) -> Result<(), TankLevelSwitchError> {
        if self.config.use_simulator {
            // Ensure channels are present before proceeding.
            let tx_tank_level_switch_to_tcp =
                tx_tank_level_switch_to_tcp_opt.as_mut().ok_or_else(|| {
                    TankLevelSwitchError::SimulatorChannelNotProvided(module_path!().to_string())
                })?;
            let rx_tank_level_switch_from_tcp =
                rx_tank_level_switch_from_tcp_opt.as_mut().ok_or_else(|| {
                    TankLevelSwitchError::SimulatorChannelNotProvided(module_path!().to_string())
                })?;

            // Fetch the position signal.
            let position_signal = AquariumSignal::TankLevelSwitchPosition;
            let position_val = Self::get_response_from_simulator_blocking(
                module_path!().to_string(),
                tx_tank_level_switch_to_tcp,
                rx_tank_level_switch_from_tcp,
                InternalCommand::RequestSignal(position_signal.clone()),
            )
            .map_err(|e| TankLevelSwitchError::SimulatorCommunicationError {
                location: module_path!().to_string(),
                signal: position_signal,
                source: e,
            })?;
            self.tank_level_switch_position = position_val > 0.0;

            // Fetch the invalid signal.
            let invalid_signal = AquariumSignal::TankLevelSwitchInvalid;
            let invalid_val = Self::get_response_from_simulator_blocking(
                module_path!().to_string(),
                tx_tank_level_switch_to_tcp,
                rx_tank_level_switch_from_tcp,
                InternalCommand::RequestSignal(invalid_signal.clone()),
            )
            .map_err(|e| TankLevelSwitchError::SimulatorCommunicationError {
                location: module_path!().to_string(),
                signal: invalid_signal,
                source: e,
            })?;
            self.tank_level_switch_invalid = invalid_val > 0.0;
        } else {
            #[cfg(all(target_os = "linux", feature = "target_hw"))]
            {
                if let Some(pin) = self.input_pin_opt.as_mut() {
                    if self.config.reverse_gpio_logic {
                        self.tank_level_switch_position = pin.is_low();
                    } else {
                        self.tank_level_switch_position = pin.is_high();
                    }
                    self.lock_error_input_pin_none = false;
                } else {
                    // This case should ideally not be reached if the constructor logic is correct.
                    return Err(TankLevelSwitchError::InputPinNotProvided(
                        module_path!().to_string(),
                    ));
                }
            }
            #[cfg(all(not(feature = "target_hw"), any(test, feature = "test-utils")))]
            {
                // testing async runtime: do nothing
            }
        }
        Ok(())
    }

    /// Calculates a stabilized tank level position, applying asymmetric delay and handling invalid inputs.
    ///
    /// This private helper updates the `tank_level_switch_position_stabilized` flag.
    /// It implements the following logic:
    ///
    /// - **Invalid Signal**: If `tank_level_switch_invalid` is `true`, the stabilized position is
    ///   immediately forced to `true` (a safe, high-water state).
    /// - **High Water Level**: If the raw position is `true`, the stabilized signal is set to `true`
    ///   immediately, and the low-level stabilization counter is reset.
    /// - **Low Water Level (with Delay)**: If the raw position is `false`, a counter increments.
    ///   The stabilized signal only changes to `false` after this counter exceeds the configured
    ///   threshold, preventing flickering from momentary low readings.
    ///
    /// [impl->swreq~sensors_tank_level_switch_preprocessing~1]
    /// [impl->swreq~sensors_tank_level_switch_invalid~1]
    fn calc_tank_level_switch_position_stabilized(&mut self) {
        if self.tank_level_switch_invalid {
            // go to safe position if the input signal is invalid
            self.tank_level_switch_position_stabilized = true;
            // in case the sensor recovers, the counter shall start from zero again
            self.tank_level_switch_position_low_stabilization_counter = 0;
        } else {
            // consider asymmetric delay for calculation
            self.tank_level_switch_position_stabilized = match self.tank_level_switch_position {
                true => {
                    self.tank_level_switch_position_low_stabilization_counter = 0;
                    true // set the stabilized signal to true (high) without delay
                }
                false => {
                    if self.tank_level_switch_position_low_stabilization_counter
                        < self
                            .config
                            .tank_level_switch_position_low_stabilization_count
                            * 10
                    {
                        // do not change the signal if the counter is not exceeded
                        self.tank_level_switch_position_low_stabilization_counter = self
                            .tank_level_switch_position_low_stabilization_counter
                            .saturating_add(1);

                        #[cfg(feature = "debug_tank_level_switch")]
                        debug!(
                            "calc_tank_level_switch_position_stabilized: tank_level_switch_position_low_stabilization_counter = {}, tank_level_switch_position_stabilized={}",
                             self.tank_level_switch_position_low_stabilization_counter,
                             self.tank_level_switch_position_stabilized
                        );
                        self.tank_level_switch_position_stabilized
                    } else {
                        false
                    }
                }
            }
        }
    }

    /// Executes the main control loop for the tank level switch module.
    ///
    /// This function runs continuously, managing the measurement, stabilization,
    /// and provision of the tank water level. It periodically updates its state by
    /// reading from a physical GPIO pin or a simulator and writes the result to a
    /// shared mutex for access by other threads.
    ///
    /// The loop maintains a fixed cycle time and responds to `Quit` commands
    /// from the signal handler for graceful shutdown.
    ///
    /// # Arguments
    /// * `tank_level_switch_channels` - A mutable reference to the struct containing the channels.
    /// * `mutex_tank_level_switch_signals` - The shared `Mutex` where the latest sensor
    ///   signals are written for other modules to consume.
    ///
    /// # Returns
    /// An empty `Result` (`Ok(())`) on successful shutdown.
    ///
    /// # Errors
    /// Returns a `TankLevelSwitchError::SimulatorChannelNotProvided` if the module is
    /// configured to use the simulator, but the required TCP communication channels
    /// are not provided at startup.
    pub async fn execute(
        &mut self,
        tank_level_switch_channels: &mut TankLevelSwitchChannels,
        mutex_tank_level_switch_signals: Arc<Mutex<TankLevelSwitchSignals>>,
    ) -> Result<(), TankLevelSwitchError> {
        #[cfg(all(target_os = "linux", not(test), not(feature = "test-utils")))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let cycle_time_duration = Duration::from_millis(CYCLE_TIME_TANK_LEVEL_SWITCH_MILLIS);

        if (self.config.use_simulator)
            && (tank_level_switch_channels
                .tx_tank_level_switch_to_tcp_opt
                .is_none()
                | tank_level_switch_channels
                    .rx_tank_level_switch_from_tcp_opt
                    .is_none())
        {
            return Err(TankLevelSwitchError::SimulatorChannelNotProvided(
                module_path!().to_string(),
            ));
        }

        let mut start_time = Instant::now();

        loop {
            let (
                quit_command_received, // a request to end the application has been received
                _,
                _,
            ) = self.process_external_request(
                &mut tank_level_switch_channels.rx_tank_level_switch_from_signal_handler,
                None,
            );
            if quit_command_received {
                break;
            }

            if self.config.active {
                // either communicate with real HW or with the simulator to get updated signal information
                let update_result = tokio::task::block_in_place(|| {
                    self.update_tank_level_switch_signals(
                        &mut tank_level_switch_channels.tx_tank_level_switch_to_tcp_opt,
                        &mut tank_level_switch_channels.rx_tank_level_switch_from_tcp_opt,
                    )
                });
                self.handle_update_result_log_flood(&update_result);
                self.calc_tank_level_switch_position_stabilized();

                let instant_before_locking_mutex = Instant::now();
                let instant_after_locking_mutex =
                    self.write_signals_to_mutex(&mutex_tank_level_switch_signals);

                // check if access to mutex took too long
                self.check_mutex_access_duration(
                    None,
                    instant_after_locking_mutex,
                    instant_before_locking_mutex,
                );
            }

            let stop_time = Instant::now();
            let execution_duration = stop_time.duration_since(start_time);
            println!(
                "TankLevelSwitch: execution duration = {:#?}, cycle_time_duration = {:#?}",
                execution_duration, cycle_time_duration
            );
            if execution_duration > cycle_time_duration {
                let delta_duration = execution_duration - cycle_time_duration;
                let permissible_cycle_time_deviation_tank_level_switch_millis =
                    if self.config.use_simulator {
                        PERMISSIBLE_CYCLE_TIME_DEVIATION_TANK_LEVEL_SWITCH_MILLIS_SIMULATOR
                    } else {
                        PERMISSIBLE_CYCLE_TIME_DEVIATION_TANK_LEVEL_SWITCH_MILLIS
                    };
                if delta_duration.as_millis()
                    > permissible_cycle_time_deviation_tank_level_switch_millis
                    && !self.lock_warn_cycle_time_exceeded
                {
                    #[cfg(all(not(test), not(feature = "test-utils")))] // only required in real-life execution
                    #[cfg(not(tarpaulin_include))]
                    warn!(target: module_path!(),
                        "execution duration of {} milliseconds exceeds cycle time of {} (delta duration={})",
                        execution_duration.as_millis(),
                        CYCLE_TIME_TANK_LEVEL_SWITCH_MILLIS,
                        delta_duration.as_millis(),
                    );
                    self.lock_warn_cycle_time_exceeded = true;
                }
            } else {
                let remaining_sleep_time = cycle_time_duration - execution_duration;
                self.lock_warn_cycle_time_exceeded = false;
                // sleep time does not need to be split up because cycle time is 100 milliseconds only
                tokio::time::sleep(remaining_sleep_time).await;
            }

            start_time = Instant::now();
        }

        tank_level_switch_channels.acknowledge_signal_handler();

        // This thread has channel connections to underlying threads (GpioHandler).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        // This thread has channel connections to underlying threads (GpioHandler).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        let sleep_duration_one_millis = Duration::from_millis(1);
        self.wait_for_termination_async(
            &mut tank_level_switch_channels.rx_tank_level_switch_from_signal_handler,
            sleep_duration_one_millis,
            module_path!(),
        )
        .await;
        Ok(())
    }

    /// Handles log flood protection logic for update result errors.
    ///
    /// [impl->swreq~sensors_log_flood_protection~1]
    pub(crate) fn handle_update_result_log_flood(
        &mut self,
        update_result: &Result<(), TankLevelSwitchError>,
    ) {
        if let Err(e) = update_result {
            if matches!(e, TankLevelSwitchError::InputPinNotProvided(_)) {
                // [impl->swreq~sensors_log_flood_protection~1]
                if !self.lock_error_input_pin_none {
                    log_error_chain(
                        // not required during testing
                        module_path!(),
                        "Failed to update tank level switch signals.",
                        e,
                    );
                    self.lock_error_input_pin_none = true;
                }
            } else if matches!(e, TankLevelSwitchError::SimulatorCommunicationError { .. }) {
                // [impl->swreq~sensors_log_flood_protection~1]
                if !self.lock_error_simulator_communication {
                    log_error_chain(
                        // not required during testing
                        module_path!(),
                        "Failed to update tank level switch signals.",
                        e,
                    );
                    self.lock_error_simulator_communication = true;
                }
            } else if matches!(e, TankLevelSwitchError::SimulatorChannelNotProvided(_)) {
                // [impl->swreq~sensors_log_flood_protection~1]
                if !self.lock_error_channel_not_provided {
                    log_error_chain(
                        // not required during testing
                        module_path!(),
                        "Failed to update tank level switch signals.",
                        e,
                    );
                    self.lock_error_channel_not_provided = true;
                }
            } else {
                log_error_chain(
                    // not required during testing
                    module_path!(),
                    "Failed to update tank level switch signals - unknown error.",
                    e,
                );
            }
        } else {
            self.lock_error_input_pin_none = false;
            self.lock_error_simulator_communication = false;
            self.lock_error_channel_not_provided = false;
        }
    }

    /// Writes calculated signals to mutex while managing log flood protection.
    ///
    /// [impl->swreq~sensors_log_flood_protection~1]
    pub(crate) fn write_signals_to_mutex(
        &mut self,
        mutex_tank_level_switch_signals: &Arc<Mutex<TankLevelSwitchSignals>>,
    ) -> Instant {
        let mut instant_after_locking_mutex = Instant::now();
        match mutex_tank_level_switch_signals.lock() {
            Ok(mut c) => {
                instant_after_locking_mutex = Instant::now();
                c.tank_level_switch_position_stabilized =
                    self.tank_level_switch_position_stabilized;
                c.tank_level_switch_invalid = self.tank_level_switch_invalid;
                c.tank_level_switch_position = self.tank_level_switch_position;
                self.lock_error_mutex_poisoned = false;
            }
            Err(e) => {
                // [impl->swreq~sensors_log_flood_protection~1]
                if !self.lock_error_mutex_poisoned {
                    error!(target: module_path!(), "Could not lock mutex ({e:?}).");
                }
                self.lock_error_mutex_poisoned = true;
            }
        };
        instant_after_locking_mutex
    }
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub mod tests {
    // unit tests shall not appear in code coverage measurement
    use super::*;
    use crate::launch::channels::channel;
    use crate::mocks::mock_simulator::tests::mock_simulator;
    use crate::sensors::tank_level_switch_config::TankLevelSwitchConfig;
    use std::sync::{Arc, Mutex};

    #[test]
    // Verifies that TankLevelSwitch initializes lock flags to false and handles log flood protection.
    // [utest->swreq~sensors_log_flood_protection~1]
    fn test_tank_level_switch_lock_flags_log_flood_protection() {
        let config = TankLevelSwitchConfig {
            active: true,
            execute: true,
            use_simulator: false,
            tank_level_switch_position_low_stabilization_count: 3,
        };

        let mut tls = TankLevelSwitch::new(config, true).unwrap();

        // 1. Verify initial lock flags
        assert!(!tls.lock_error_mutex_poisoned);
        assert!(!tls.lock_error_input_pin_none);
        assert!(!tls.lock_error_simulator_communication);
        assert!(!tls.lock_error_channel_not_provided);

        // 2. Test InputPinNotProvided log flood protection
        let err_pin = TankLevelSwitchError::InputPinNotProvided("test_loc".to_string());
        tls.handle_update_result_log_flood(&Err(err_pin));
        assert!(
            tls.lock_error_input_pin_none,
            "lock_error_input_pin_none should be true"
        );

        let err_pin_2 = TankLevelSwitchError::InputPinNotProvided("test_loc".to_string());
        tls.handle_update_result_log_flood(&Err(err_pin_2));
        assert!(
            tls.lock_error_input_pin_none,
            "lock_error_input_pin_none should remain true"
        );

        // 3. Test SimulatorCommunicationError log flood protection
        let err_sim = TankLevelSwitchError::SimulatorCommunicationError {
            location: "test_loc".to_string(),
            signal: AquariumSignal::TankLevelSwitchPosition,
            source: TcpCommunicationError::MockError {
                location: module_path!().to_string(),
            },
        };
        tls.handle_update_result_log_flood(&Err(err_sim));
        assert!(
            tls.lock_error_simulator_communication,
            "lock_error_simulator_communication should be true"
        );

        // 4. Test SimulatorChannelNotProvided log flood protection
        let err_sim = TankLevelSwitchError::SimulatorChannelNotProvided("test_loc".to_string());
        tls.handle_update_result_log_flood(&Err(err_sim));
        assert!(
            tls.lock_error_channel_not_provided,
            "lock_error_channel_not_provided should be true"
        );

        // Reset via Ok
        tls.handle_update_result_log_flood(&Ok(()));
        assert!(!tls.lock_error_input_pin_none);
        assert!(!tls.lock_error_channel_not_provided);

        // 4. Test mutex poisoning log flood protection
        let mutex_signals = Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));
        let m_clone = mutex_signals.clone();
        let _ = std::thread::spawn(move || {
            let _g = m_clone.lock().unwrap();
            panic!("poison mutex");
        })
        .join();

        // First attempt to write to poisoned mutex: lock_error_mutex_poisoned becomes true
        tls.write_signals_to_mutex(&mutex_signals);
        assert!(
            tls.lock_error_mutex_poisoned,
            "lock_error_mutex_poisoned should be true when mutex lock fails"
        );

        // Second attempt: lock_error_mutex_poisoned remains true
        tls.write_signals_to_mutex(&mutex_signals);
        assert!(
            tls.lock_error_mutex_poisoned,
            "lock_error_mutex_poisoned should remain true"
        );

        // Unpoisoned mutex: resets lock_error_mutex_poisoned to false
        let valid_mutex = Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));
        tls.write_signals_to_mutex(&valid_mutex);
        assert!(
            !tls.lock_error_mutex_poisoned,
            "lock_error_mutex_poisoned should reset to false on success"
        );
    }

    #[test]
// [utest->swreq~sensors_tank_level_switch_preprocessing~1]
    // Verifies calc_tank_level_switch_position_stabilized applies immediate high-water transitions, asymmetric low-water delay counters, and invalid signal overrides.
    fn test_calc_tank_level_switch_position_stabilized_asymmetric_delay() {
        let config = TankLevelSwitchConfig {
            active: true,
            execute: true,
            use_simulator: false,
            tank_level_switch_position_low_stabilization_count: 1, // threshold = 1 * 10 = 10
        };

        let mut tls = TankLevelSwitch::new(config, true).unwrap();

        // 1. High water position -> immediate true, counter = 0
        tls.tank_level_switch_position = true;
        tls.calc_tank_level_switch_position_stabilized();
        assert!(tls.tank_level_switch_position_stabilized);
        assert_eq!(tls.tank_level_switch_position_low_stabilization_counter, 0);

        // 2. Transition to low water position -> counter increments up to threshold 10
        tls.tank_level_switch_position = false;
        for i in 1..=10 {
            tls.calc_tank_level_switch_position_stabilized();
            assert!(
                tls.tank_level_switch_position_stabilized,
                "should remain true before reaching threshold"
            );
            assert_eq!(tls.tank_level_switch_position_low_stabilization_counter, i);
        }

        // 11th call reaches threshold
        tls.calc_tank_level_switch_position_stabilized();
        assert!(
            !tls.tank_level_switch_position_stabilized,
            "should become false after reaching threshold 10"
        );

        // 3. Invalid signal -> forces true and resets counter
        tls.tank_level_switch_invalid = true;
        tls.calc_tank_level_switch_position_stabilized();
        assert!(
            tls.tank_level_switch_position_stabilized,
            "should force true on invalid signal"
        );
        assert_eq!(tls.tank_level_switch_position_low_stabilization_counter, 0);
    }

    #[test]
// [utest->swreq~sensors_tank_level_switch_preprocessing~1]
    // Verifies update_tank_level_switch_signals returns SimulatorChannelNotProvided when simulator channels are missing.
    fn test_update_tank_level_switch_signals_simulator_channel_not_provided() {
        let config = TankLevelSwitchConfig {
            active: true,
            execute: true,
            use_simulator: true,
            tank_level_switch_position_low_stabilization_count: 3,
        };

        let mut tls = TankLevelSwitch::new(config, true).unwrap();
        let res = tls.update_tank_level_switch_signals(&mut None, &mut None);

        assert!(matches!(
            res,
            Err(TankLevelSwitchError::SimulatorChannelNotProvided(_))
        ));

        let (tx_sig, _rx_sig) = channel::<InternalCommand>(10, false);

        let res = tls.update_tank_level_switch_signals(&mut Some(tx_sig), &mut None);

        assert!(matches!(
            res,
            Err(TankLevelSwitchError::SimulatorChannelNotProvided(_))
        ));
    }

    #[tokio::test]
// [utest->swreq~sensors_tank_level_switch_preprocessing~1]
    // Verifies update_tank_level_switch_signals returns error when simulator responds with error.
    async fn test_update_tank_level_switch_signals_simulator_responds_with_error() {
        let config = TankLevelSwitchConfig {
            active: true,
            execute: true,
            use_simulator: true,
            tank_level_switch_position_low_stabilization_count: 3,
        };

        let mut tls1 = TankLevelSwitch::new(config.clone(), true).unwrap();
        let mut tls2 = TankLevelSwitch::new(config, true).unwrap();

        // test the error path of the first communication
        let (tx_req, mut rx_req) = channel::<InternalCommand>(10, false);
        let (mut tx_resp, rx_resp) = channel::<Result<f32, TcpCommunicationError>>(10, false);

        let mock_simulator_task = tokio::spawn(async move {
            mock_simulator(&mut tx_resp, &mut rx_req, None).await;
        });

        let test_object_task = tokio::task::spawn_blocking(move || {
            tls1.update_tank_level_switch_signals(&mut Some(tx_req), &mut Some(rx_resp))
        });

        let (simulator_res, res) = tokio::join!(mock_simulator_task, test_object_task);
        simulator_res.unwrap();
        let res = res.unwrap();

        assert!(matches!(
            res,
            Err(TankLevelSwitchError::SimulatorCommunicationError {
                location: _,
                signal: AquariumSignal::TankLevelSwitchPosition,
                source: _
            })
        ));

        // test the error path of the second communication
        let (tx_req, mut rx_req) = channel::<InternalCommand>(10, false);
        let (mut tx_resp, rx_resp) = channel::<Result<f32, TcpCommunicationError>>(10, false);

        let mock_simulator_task = tokio::spawn(async move {
            mock_simulator(&mut tx_resp, &mut rx_req, Some(42.0)).await;
        });

        let test_object_task = tokio::task::spawn_blocking(move || {
            tls2.update_tank_level_switch_signals(&mut Some(tx_req), &mut Some(rx_resp))
        });
        let (simulator_res, res) = tokio::join!(mock_simulator_task, test_object_task);
        simulator_res.unwrap();
        let res = res.unwrap();

        assert!(matches!(
            res,
            Err(TankLevelSwitchError::SimulatorCommunicationError {
                location: _,
                signal: AquariumSignal::TankLevelSwitchInvalid,
                source: _
            })
        ));
    }

    #[tokio::test]
// [utest->swreq~sensors_tank_level_switch_preprocessing~1]
    // Verifies execute returns SimulatorChannelNotProvided when configured for simulator mode without TCP channels.
    async fn test_execute_simulator_channel_not_provided() {
        use crate::launch::channels::channel;

        let config = TankLevelSwitchConfig {
            active: true,
            execute: true,
            use_simulator: true,
            tank_level_switch_position_low_stabilization_count: 3,
        };

        let mut tls = TankLevelSwitch::new(config, true).unwrap();

        let (_tx_sig, rx_sig) = channel::<InternalCommand>(10, false);
        let (tx_ack, _rx_ack) = channel::<bool>(10, false);
        let mut channels = TankLevelSwitchChannels {
            tx_tank_level_switch_to_tcp_opt: None,
            rx_tank_level_switch_from_tcp_opt: None,
            tx_tank_level_switch_to_signal_handler: tx_ack,
            rx_tank_level_switch_from_signal_handler: rx_sig,
        };
        let mutex_signals = Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        let res = tls.execute(&mut channels, mutex_signals).await;
        assert!(matches!(
            res,
            Err(TankLevelSwitchError::SimulatorChannelNotProvided(_))
        ));
    }
}
