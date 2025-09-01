/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(feature = "debug_tank_level_switch")]
use log::debug;
use std::sync::{Arc, Mutex};

#[cfg(all(target_os = "linux", not(test)))]
use nix::unistd::gettid;

use log::error;
#[cfg(all(target_os = "linux", not(test)))]
use log::info;

#[cfg(not(test))]
use log::warn;

use crate::sensors::tank_level_switch_channels::TankLevelSwitchChannels;
use crate::sensors::tank_level_switch_config::TankLevelSwitchConfig;
use crate::utilities::check_mutex_access_duration::CheckMutexAccessDurationTrait;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};
use thiserror::Error;

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
            use rppal::gpio::{Gpio, InputPin};
    }
}

use crate::simulator::get_resp_sim::GetResponseFromSimulatorTrait;
use crate::simulator::tcp_communication_error::TcpCommunicationError;
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

/// Contains error definitions for TankLevelSwitch
#[derive(Error, Debug)]
pub enum TankLevelSwitchError {
    /// Application shall use real HW, but GPIO lib interface was not provided.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{0}] Application shall use real HW, but GPIO lib interface was not provided.")]
    GPIOInterfaceNotProvided(String),

    /// Could not get pin from GPIO interface for tank level monitoring.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    #[error("[{location}] Could not get pin {pin_number} from GPIO interface for tank level monitoring.")]
    GpioPinRetrievalError {
        location: String,
        pin_number: u8,

        #[source]
        source: rppal::gpio::Error,
    },

    /// Application shall use simulator, but the channel to TCP thread was not provided.
    #[error("[{0}] Configured to run with simulator, but no channel to TCP thread provided.")]
    SimulatorChannelNotProvided(String),

    /// Input pin has not been provided. This is a configuration error.
    #[allow(unused)] // variant is constructed in platform-specific code and evaluated in generic code.
    #[error("[{0}] Input pin has not been provided. This is a configuration error.")]
    InputPinNotProvided(String),

    /// Communication with the simulator failed.
    #[error("[{location}] Communication with simulator for {signal} failed")]
    SimulatorCommunicationError {
        location: String,
        signal: AquariumSignal,

        #[source]
        source: TcpCommunicationError,
    },
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
    #[cfg(test)]
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
            #[cfg(test)]
            mutex_access_duration_exceeded: false,
            lock_error_input_pin_none: false,
            lock_error_simulator_communication: false,
            lock_error_channel_not_provided: false,
            max_mutex_access_duration: Duration::from_millis(MAX_MUTEX_ACCESS_DURATION_MILLIS),
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
            #[cfg(test)]
            mutex_access_duration_exceeded: false,
            lock_error_input_pin_none: false,
            input_pin_opt,
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
    pub fn update_tank_level_switch_signals(
        &mut self,
        tx_tank_level_switch_to_tcp_opt: &Option<Sender<InternalCommand>>,
        rx_tank_level_switch_from_tcp_opt: &Option<Receiver<Result<f32, TcpCommunicationError>>>,
    ) -> Result<(), TankLevelSwitchError> {
        if self.config.use_simulator {
            // Ensure channels are present before proceeding.
            let tx_tank_level_switch_to_tcp =
                tx_tank_level_switch_to_tcp_opt.as_ref().ok_or_else(|| {
                    TankLevelSwitchError::SimulatorChannelNotProvided(module_path!().to_string())
                })?;
            let rx_tank_level_switch_from_tcp =
                rx_tank_level_switch_from_tcp_opt.as_ref().ok_or_else(|| {
                    TankLevelSwitchError::SimulatorChannelNotProvided(module_path!().to_string())
                })?;

            // Fetch the position signal.
            let position_signal = AquariumSignal::TankLevelSwitchPosition;
            let position_val = Self::get_response_from_simulator(
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
            let invalid_val = Self::get_response_from_simulator(
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
                // Use `if let` for more idiomatic Option handling.
                if let Some(pin) = self.input_pin_opt.as_mut() {
                    self.tank_level_switch_position = pin.is_high();
                    self.lock_error_input_pin_none = false;
                } else {
                    // This case should ideally not be reached if the constructor logic is correct.
                    return Err(TankLevelSwitchError::InputPinNotProvided(
                        module_path!().to_string(),
                    ));
                }
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
    /// * `tank_level_switch_signals` - A mutable reference to the struct containing the channels.
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
    pub fn execute(
        &mut self,
        tank_level_switch_channels: &mut TankLevelSwitchChannels,
        mutex_tank_level_switch_signals: Arc<Mutex<TankLevelSwitchSignals>>,
    ) -> Result<(), TankLevelSwitchError> {
        #[cfg(all(target_os = "linux", not(test)))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let spin_sleeper = SpinSleeper::default();
        let mut lock_warn_cycle_time_exceeded: bool = false;
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
                &tank_level_switch_channels.rx_tank_level_switch_from_signal_handler,
                None,
            );
            if quit_command_received {
                break;
            }

            if self.config.active {
                // either communicate with real HW or with the simulator to get updated signal information
                if let Err(e) = self.update_tank_level_switch_signals(
                    &tank_level_switch_channels.tx_tank_level_switch_to_tcp_opt,
                    &tank_level_switch_channels.rx_tank_level_switch_from_tcp_opt,
                ) {
                    if matches!(e, TankLevelSwitchError::InputPinNotProvided(_)) {
                        if !self.lock_error_input_pin_none {
                            log_error_chain(
                                module_path!(),
                                "Failed to update tank level switch signals.",
                                e,
                            );
                            self.lock_error_input_pin_none = true;
                        }
                    } else if matches!(e, TankLevelSwitchError::SimulatorCommunicationError { .. })
                    {
                        if !self.lock_error_simulator_communication {
                            log_error_chain(
                                module_path!(),
                                "Failed to update tank level switch signals.",
                                e,
                            );
                            self.lock_error_simulator_communication = true;
                        }
                    } else if matches!(e, TankLevelSwitchError::SimulatorChannelNotProvided(_)) {
                        if !self.lock_error_channel_not_provided {
                            log_error_chain(
                                module_path!(),
                                "Failed to update tank level switch signals.",
                                e,
                            );
                            self.lock_error_channel_not_provided = true;
                        }
                    } else {
                        // Protection for future error variants
                        log_error_chain(
                            module_path!(),
                            "Failed to update tank level switch signals.",
                            e,
                        );
                    }
                } else {
                    self.lock_error_input_pin_none = false;
                }

                self.calc_tank_level_switch_position_stabilized();

                let instant_before_locking_mutex = Instant::now();
                let mut instant_after_locking_mutex = Instant::now(); // initialization is overwritten

                // Write calculated signals to mutex
                {
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
                            if !self.lock_error_mutex_poisoned {
                                error!(target: module_path!(), "Could not lock mutex ({e:?}).");
                            }
                            self.lock_error_mutex_poisoned = true;
                        }
                    }
                }

                // check if access to mutex took too long
                self.check_mutex_access_duration(
                    instant_after_locking_mutex,
                    instant_before_locking_mutex,
                );
            }

            let stop_time = Instant::now();
            let execution_duration = stop_time.duration_since(start_time);
            if execution_duration > cycle_time_duration {
                let delta_duration = execution_duration - cycle_time_duration;
                if delta_duration.as_millis()
                    > PERMISSIBLE_CYCLE_TIME_DEVIATION_TANK_LEVEL_SWITCH_MILLIS
                    && !lock_warn_cycle_time_exceeded
                {
                    #[cfg(not(test))]
                    warn!(target: module_path!(),
                        "execution duration of {} milliseconds exceeds cycle time of {} (delta duration={})",
                        execution_duration.as_millis(),
                        CYCLE_TIME_TANK_LEVEL_SWITCH_MILLIS,
                        delta_duration.as_millis(),
                    );
                    lock_warn_cycle_time_exceeded = true;
                }
            } else {
                let remaining_sleep_time = cycle_time_duration - execution_duration;
                lock_warn_cycle_time_exceeded = false;
                // sleep time does not need to be split up because cycle time is 100 milliseconds only
                spin_sleeper.sleep(remaining_sleep_time);
            }

            start_time = Instant::now();
        }

        match tank_level_switch_channels
            .tx_tank_level_switch_to_signal_handler
            .send(true)
        {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: module_path!(),
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // This thread has channel connections to underlying threads (GpioHandler).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        let sleep_duration_one_millis = Duration::from_millis(1);
        self.wait_for_termination(
            &tank_level_switch_channels.rx_tank_level_switch_from_signal_handler,
            sleep_duration_one_millis,
            module_path!(),
        );
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use spin_sleep::SpinSleeper;
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;
    use std::thread::JoinHandle;
    use std::time::Duration;

    use crate::mocks::mock_signal::tests::MockSignal;
    use crate::mocks::mock_tcp::tests::mock_tcp_for_tank_level_switch;
    use crate::mocks::test_command::tests::TestCommand;
    use crate::sensors::tank_level_switch::{TankLevelSwitch, TankLevelSwitchSignals};
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::config::{read_config_file, ConfigData};

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    use crate::sensors::gpio_handler::GpioHandler;

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    use crate::relays::actuate_gpio;
    use crate::sensors::tank_level_switch_channels::TankLevelSwitchChannels;
    use crate::simulator::tcp_communication_error::TcpCommunicationError;

    #[test]
    pub fn test_refill_calc_tank_level_switch_position_stabilized_false_when_invalid() {
        // test if position stabilized signal is updating when the invalid signal is true
        let (tx_tank_level_switch_to_tcp, rx_tcp_from_tank_level_switch) = mpsc::channel();
        let (tx_tcp_to_tank_level_switch, rx_tank_level_switch_from_tcp) = mpsc::channel();
        let (tx_tank_level_switch_to_signal_handler, rx_signal_handler_from_tank_level_switch) =
            mpsc::channel();
        let (tx_signal_handler_to_tank_level_switch, rx_tank_level_switch_from_signal_handler) =
            mpsc::channel();
        let (_tx_test_environment_to_tcp, rx_tcp_from_test_environment) = mpsc::channel();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let duration_test_case_secs = config
            .tank_level_switch
            .tank_level_switch_position_low_stabilization_count
            + 1;

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        let mut tank_level_switch = TankLevelSwitch::new(
            config.tank_level_switch,
            true,
            None,
            config.gpio_handler.tank_level_switch,
        );
        #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
        let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true).unwrap();

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(false, false, true)));

        // thread for mock tcp
        let join_handle_mock_tcp = thread::Builder::new()
            .name("mock_tcp".to_string())
            .spawn(move || {
                mock_tcp_for_tank_level_switch(
                    &tx_tcp_to_tank_level_switch,
                    &rx_tcp_from_tank_level_switch,
                    Some(&rx_tcp_from_test_environment),
                    false,
                    true,
                );
            })
            .unwrap();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration = Duration::from_secs(duration_test_case_secs.into());
                let sleep_duration_thread_processing = Duration::from_millis(50);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration);
                spin_sleeper.sleep(sleep_duration_thread_processing);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                rx_signal_handler_from_tank_level_switch.recv().unwrap();
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of the test object
                let tx_tank_level_switch_to_tcp_for_test_case_finish =
                    tx_tank_level_switch_to_tcp.clone();
                let tx_tank_level_switch_to_tcp_opt = Some(tx_tank_level_switch_to_tcp);
                let rx_tank_level_switch_from_tcp_opt = Some(rx_tank_level_switch_from_tcp);

                let mut tank_level_switch_channels = TankLevelSwitchChannels {
                    tx_tank_level_switch_to_tcp_opt,
                    #[cfg(feature = "debug_channels")]
                    cnt_tx_tank_level_switch_to_tcp_opt: 0,

                    rx_tank_level_switch_from_tcp_opt,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_tank_level_switch_from_tcp_opt: 0,

                    tx_tank_level_switch_to_signal_handler,
                    #[cfg(feature = "debug_channels")]
                    cnt_tx_tank_level_switch_to_signal_handler: 0,

                    rx_tank_level_switch_from_signal_handler,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_tank_level_switch_from_signal_handler: 0,
                };

                let mutex_tank_level_switch_signals_for_assert =
                    mutex_tank_level_switch_signals.clone();
                let _ = tank_level_switch.execute(
                    &mut tank_level_switch_channels,
                    mutex_tank_level_switch_signals,
                );

                // Get the final state after execution.
                let signals_after_test_run =
                    mutex_tank_level_switch_signals_for_assert.lock().unwrap();
                // Assert that the stabilized position is forced to true (safe state) due to invalidity.
                assert!(signals_after_test_run.tank_level_switch_position_stabilized);
                // Confirm invalid-flag remained true
                assert!(signals_after_test_run.tank_level_switch_invalid);

                // send Quit signal to mock threads because the test object has terminated
                let _ =
                    tx_tank_level_switch_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
            })
            .unwrap();

        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_mock_tcp
            .join()
            .expect("Mock TCP thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");

        println!("* [TankLevelSwitch] testing if stabilized tank level position is high when invalid bit is true succeeded.");
    }

    // helper function for creating the test object
    fn create_test_object(
        tx_tank_level_switch_to_tcp: Sender<InternalCommand>,
        rx_tank_level_switch_from_tcp: Receiver<Result<f32, TcpCommunicationError>>,
        mut tank_level_switch: TankLevelSwitch,
        tx_tank_level_switch_to_signal_handler: Sender<bool>,
        rx_tank_level_switch_from_signal_handler: Receiver<InternalCommand>,
        mutex_tank_level_switch_signals: Arc<Mutex<TankLevelSwitchSignals>>,
    ) -> JoinHandle<()> {
        thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of the test object
                let tx_tank_level_switch_to_tcp_opt = Some(tx_tank_level_switch_to_tcp);
                let rx_tank_level_switch_from_tcp_opt = Some(rx_tank_level_switch_from_tcp);

                let mut tank_level_switch_channels = TankLevelSwitchChannels {
                    tx_tank_level_switch_to_tcp_opt,
                    #[cfg(feature = "debug_channels")]
                    cnt_tx_tank_level_switch_to_tcp_opt: 0,

                    rx_tank_level_switch_from_tcp_opt,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_tank_level_switch_from_tcp_opt: 0,

                    tx_tank_level_switch_to_signal_handler,
                    #[cfg(feature = "debug_channels")]
                    cnt_tx_tank_level_switch_to_signal_handler: 0,

                    rx_tank_level_switch_from_signal_handler,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_tank_level_switch_from_signal_handler: 0,
                };

                let _ = tank_level_switch.execute(
                    &mut tank_level_switch_channels,
                    mutex_tank_level_switch_signals,
                );
            })
            .unwrap()
    }

    #[test]
    pub fn test_refill_calc_tank_level_switch_position_stabilized_false_true() {
        // test if the position signal is updating without delay when the invalid signal is false
        let (tx_tank_level_switch_to_tcp, rx_tcp_from_tank_level_switch) = mpsc::channel();
        let (tx_tcp_to_tank_level_switch, rx_tank_level_switch_from_tcp) = mpsc::channel();
        let (tx_tank_level_switch_to_signal_handler, rx_signal_handler_from_tank_level_switch) =
            mpsc::channel();
        let (tx_signal_handler_to_tank_level_switch, rx_tank_level_switch_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_refill, _rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_signal_handler_to_heating, _rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_test_environment_to_tcp, rx_tcp_from_test_environment) = mpsc::channel();

        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.refill.initial_wait_interval = 2;
        config.refill.check_interval = 60;

        let duration_test_case_part_1: u64 = 3; // three seconds with level switch in low position
        let duration_test_case_part_2: u64 = 2; // two seconds with level switch in high position

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        let mut tank_level_switch = TankLevelSwitch::new(
            config.tank_level_switch,
            false,
            None,
            config.gpio_handler.tank_level_switch,
        );
        #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
        let tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, false).unwrap();
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock tcp
        let join_handle_mock_tcp = thread::Builder::new()
            .name("mock_tcp".to_string())
            .spawn(move || {
                mock_tcp_for_tank_level_switch(
                    &tx_tcp_to_tank_level_switch,
                    &rx_tcp_from_tank_level_switch,
                    Some(&rx_tcp_from_test_environment),
                    false,
                    false,
                );
            })
            .unwrap();

        let tx_test_environment_to_tcp_for_test_case_finish = tx_tank_level_switch_to_tcp.clone();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_part_1 = Duration::from_secs(duration_test_case_part_1.into());
                let sleep_duration_part_2 = Duration::from_secs(duration_test_case_part_2.into());
                let sleep_duration_thread_processing = Duration::from_millis(50);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_part_1);
                let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                    MockSignal::TankLevelSwitchPosition(true),
                ));
                spin_sleeper.sleep(sleep_duration_part_2);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                spin_sleeper.sleep(sleep_duration_thread_processing);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                rx_signal_handler_from_tank_level_switch.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);
                let _ = tx_test_environment_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for the test object
        let join_handle_test_object = create_test_object(
            tx_tank_level_switch_to_tcp,
            rx_tank_level_switch_from_tcp,
            tank_level_switch,
            tx_tank_level_switch_to_signal_handler,
            rx_tank_level_switch_from_signal_handler,
            mutex_tank_level_switch_signals,
        );

        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_mock_tcp
            .join()
            .expect("Mock TCP thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");

        println!("* [TankLevelSwitch] testing immediate change false -> true for stabilized tank level position when invalid bit is false succeeded");
    }

    #[test]
    pub fn test_refill_calc_tank_level_switch_position_stabilized_true_false() {
        // test if the position signal is updating with delay when the invalid signal is false
        let (tx_tank_level_switch_to_tcp, rx_tcp_from_tank_level_switch) = mpsc::channel();
        let (tx_tcp_to_tank_level_switch, rx_tank_level_switch_from_tcp) = mpsc::channel();
        let (tx_tank_level_switch_to_signal_handler, rx_signal_handler_from_tank_level_switch) =
            mpsc::channel();
        let (tx_signal_handler_to_tank_level_switch, rx_tank_level_switch_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_refill, _rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_signal_handler_to_heating, _rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_test_environment_to_tcp, rx_tcp_from_test_environment) = mpsc::channel();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let duration_test_case_part_1: u64 = 3; // three seconds with level switch in low position
        let duration_test_case_part_2: u64 = 8; // two seconds with level switch in high position
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        let mut tank_level_switch = TankLevelSwitch::new(
            config.tank_level_switch,
            true,
            None,
            config.gpio_handler.tank_level_switch,
        );
        #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
        let tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true).unwrap();
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock tcp
        let join_handle_mock_tcp = thread::Builder::new()
            .name("mock_tcp".to_string())
            .spawn(move || {
                mock_tcp_for_tank_level_switch(
                    &tx_tcp_to_tank_level_switch,
                    &rx_tcp_from_tank_level_switch,
                    Some(&rx_tcp_from_test_environment),
                    true,
                    false,
                );
            })
            .unwrap();

        let tx_test_environment_to_tcp_for_test_case_finish = tx_tank_level_switch_to_tcp.clone();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_part_1 = Duration::from_secs(duration_test_case_part_1.into());
                let sleep_duration_part_2 = Duration::from_secs(duration_test_case_part_2.into());
                let sleep_duration_thread_processing = Duration::from_millis(50);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_part_1);
                let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                    MockSignal::TankLevelSwitchPosition(false),
                ));
                spin_sleeper.sleep(sleep_duration_part_2);
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                spin_sleeper.sleep(sleep_duration_thread_processing);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                rx_signal_handler_from_tank_level_switch.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);
                let _ = tx_test_environment_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for the test object
        let join_handle_test_object = create_test_object(
            tx_tank_level_switch_to_tcp,
            rx_tank_level_switch_from_tcp,
            tank_level_switch,
            tx_tank_level_switch_to_signal_handler,
            rx_tank_level_switch_from_signal_handler,
            mutex_tank_level_switch_signals,
        );

        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_mock_tcp
            .join()
            .expect("Mock TCP thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");

        println!("* [TankLevelSwitch] checking delayed change true -> false for stabilized tank level position when invalid bit is false succeeded.");
    }

    #[test]
    pub fn test_refill_calc_tank_level_switch_position_stabilized_is_true_with_toggling_input() {
        // test if position stabilized signal is not updating when the raw input signal is toggling
        let (tx_tank_level_switch_to_tcp, rx_tcp_from_tank_level_switch) = mpsc::channel();
        let (tx_tcp_to_tank_level_switch, rx_tank_level_switch_from_tcp) = mpsc::channel();
        let (tx_tank_level_switch_to_signal_handler, rx_signal_handler_from_tank_level_switch) =
            mpsc::channel();
        let (tx_signal_handler_to_tank_level_switch, rx_tank_level_switch_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_refill, _rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_signal_handler_to_heating, _rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_test_environment_to_tcp, rx_tcp_from_test_environment) = mpsc::channel();

        let config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        let duration_test_case_part: u64 = 2; // two seconds with the level switch before toggling

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        let mut tank_level_switch = TankLevelSwitch::new(
            config.tank_level_switch,
            true,
            None,
            config.gpio_handler.tank_level_switch,
        );
        #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
        let tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true).unwrap();
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock tcp
        let join_handle_mock_tcp = thread::Builder::new()
            .name("mock_tcp".to_string())
            .spawn(move || {
                mock_tcp_for_tank_level_switch(
                    &tx_tcp_to_tank_level_switch,
                    &rx_tcp_from_tank_level_switch,
                    Some(&rx_tcp_from_test_environment),
                    true,
                    false,
                );
            })
            .unwrap();

        let tx_test_environment_to_tcp_for_test_case_finish = tx_tank_level_switch_to_tcp.clone();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_part = Duration::from_secs(duration_test_case_part.into());
                let sleep_duration_thread_processing = Duration::from_millis(50);
                let spin_sleeper = SpinSleeper::default();
                for _ in 0..2 {
                    spin_sleeper.sleep(sleep_duration_part);
                    let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                        MockSignal::TankLevelSwitchPosition(false),
                    ));
                    spin_sleeper.sleep(sleep_duration_part);
                    let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                        MockSignal::TankLevelSwitchPosition(true),
                    ));
                }
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                spin_sleeper.sleep(sleep_duration_thread_processing);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                rx_signal_handler_from_tank_level_switch.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);
                let _ = tx_test_environment_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for the test object
        let join_handle_test_object = create_test_object(
            tx_tank_level_switch_to_tcp,
            rx_tank_level_switch_from_tcp,
            tank_level_switch,
            tx_tank_level_switch_to_signal_handler,
            rx_tank_level_switch_from_signal_handler,
            mutex_tank_level_switch_signals,
        );

        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_mock_tcp
            .join()
            .expect("Mock TCP thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");

        println!("* [TankLevelSwitch] checking delayed change true -> false for stabilized tank level position when invalid bit is false succeeded.");
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    fn test_transition_stabilized_true_false(
        spin_sleeper: &SpinSleeper,
        sleep_duration_part: Duration,
        mutex_tank_level_switch_signals: &mut Arc<Mutex<TankLevelSwitchSignals>>,
    ) {
        for i in 0..20 {
            // loop for the first 10 seconds
            spin_sleeper.sleep(sleep_duration_part);
            {
                let tank_level_switch_signals = mutex_tank_level_switch_signals.lock().unwrap();
                println!(
                    "Tank level switch signals at iteration {}: Position={} Invalid={} Stabilized={}",
                    i,
                    tank_level_switch_signals.tank_level_switch_position,
                    tank_level_switch_signals.tank_level_switch_invalid,
                    tank_level_switch_signals.tank_level_switch_position_stabilized
                );
                if i < 10 {
                    assert_eq!(tank_level_switch_signals.tank_level_switch_position, false);
                    assert_eq!(
                        tank_level_switch_signals.tank_level_switch_position_stabilized,
                        true
                    );
                } else {
                    assert_eq!(tank_level_switch_signals.tank_level_switch_position, false);
                    assert_eq!(
                        tank_level_switch_signals.tank_level_switch_position_stabilized,
                        false
                    );
                }
            }
        }
    }

    #[test]
    #[ignore]
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    // test functionality on real hardware
    pub fn test_tank_level_switch_position_on_hardware() {
        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.gpio_handler.use_simulator = false;
        config.tank_level_switch.use_simulator = false;

        let (tx_tank_level_switch_to_signal_handler, rx_signal_handler_from_tank_level_switch) =
            mpsc::channel();
        let (tx_signal_handler_to_tank_level_switch, rx_tank_level_switch_from_signal_handler) =
            mpsc::channel();

        let tank_level_switch_pin = config.gpio_handler.tank_level_switch;

        let gpio_handler = GpioHandler::new(config.gpio_handler).unwrap();

        let gpio_lib_handle = gpio_handler.gpio_lib_handle_opt.clone().unwrap();

        let mut stimuli_pin =
            actuate_gpio::get_output_pin(&gpio_lib_handle, 21, "Tank level switch stimuli");

        let mut tank_level_switch = TankLevelSwitch::new(
            config.tank_level_switch,
            true,
            Some(gpio_handler.gpio_lib_handle_opt.unwrap()),
            tank_level_switch_pin,
        );

        let mut mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));
        let mutex_tank_level_switch_signals_clone_for_test_object =
            mutex_tank_level_switch_signals.clone();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_part = Duration::from_millis(500);
                let spin_sleeper = SpinSleeper::default();

                test_transition_stabilized_true_false(
                    &spin_sleeper,
                    sleep_duration_part,
                    &mut mutex_tank_level_switch_signals
                );

                stimuli_pin.set_high();

                for i in 0..20 {
                    // loop for the second 10 seconds
                    spin_sleeper.sleep(sleep_duration_part);
                    {
                        let tank_level_switch_signals =
                            mutex_tank_level_switch_signals.lock().unwrap();
                        println!(
                            "Tank level switch signals at iteration {}: Position={} Invalid={} Stabilized={}",
                            i,
                            tank_level_switch_signals.tank_level_switch_position,
                            tank_level_switch_signals.tank_level_switch_invalid,
                            tank_level_switch_signals.tank_level_switch_position_stabilized
                        );
                        assert_eq!(tank_level_switch_signals.tank_level_switch_position, true);
                        assert_eq!(tank_level_switch_signals.tank_level_switch_position_stabilized, true);
                    }
                }

                stimuli_pin.set_low();

                test_transition_stabilized_true_false(
                    &spin_sleeper,
                    sleep_duration_part,
                    &mut mutex_tank_level_switch_signals
                );

                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                rx_signal_handler_from_tank_level_switch.recv().unwrap();
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Terminate);
                spin_sleeper.sleep(sleep_duration_part);
            })
            .unwrap();

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                tank_level_switch.execute(
                    &None,
                    &None,
                    &tx_tank_level_switch_to_signal_handler,
                    &rx_tank_level_switch_from_signal_handler,
                    mutex_tank_level_switch_signals_clone_for_test_object,
                );
            })
            .unwrap();

        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");
    }
    #[test]
    pub fn test_refill_calc_tank_level_switch_position_stabilized_false_true_with_mutex_blocked_too_long(
    ) {
        // test if mutex access duration is measured and monitored
        let (tx_tank_level_switch_to_tcp, rx_tcp_from_tank_level_switch) = mpsc::channel();
        let (tx_tcp_to_tank_level_switch, rx_tank_level_switch_from_tcp) = mpsc::channel();
        let (tx_tank_level_switch_to_signal_handler, rx_signal_handler_from_tank_level_switch) =
            mpsc::channel();
        let (tx_signal_handler_to_tank_level_switch, rx_tank_level_switch_from_signal_handler) =
            mpsc::channel();
        let (tx_signal_handler_to_refill, _rx_refill_from_signal_handler) = mpsc::channel();
        let (tx_signal_handler_to_heating, _rx_heating_from_signal_handler) = mpsc::channel();
        let (tx_test_environment_to_tcp, rx_tcp_from_test_environment) = mpsc::channel();

        let mut config: ConfigData =
            read_config_file("/config/aquarium_control_test_generic.toml".to_string()).unwrap();
        config.refill.initial_wait_interval = 2;
        config.refill.check_interval = 60;

        let duration_test_case_part_1: u64 = 3; // three seconds with level switch in low position
        let duration_test_case_part_2: u64 = 2; // two seconds with level switch in high position

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        let mut tank_level_switch = TankLevelSwitch::new(
            config.tank_level_switch,
            false,
            None,
            config.gpio_handler.tank_level_switch,
        );
        #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
        let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, false).unwrap();
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));
        let mutex_tank_level_switch_signals_clone_for_test_environment =
            mutex_tank_level_switch_signals.clone();

        // thread for mock tcp
        let join_handle_mock_tcp = thread::Builder::new()
            .name("mock_tcp".to_string())
            .spawn(move || {
                mock_tcp_for_tank_level_switch(
                    &tx_tcp_to_tank_level_switch,
                    &rx_tcp_from_tank_level_switch,
                    Some(&rx_tcp_from_test_environment),
                    false,
                    false,
                );
            })
            .unwrap();

        let tx_test_environment_to_tcp_for_test_case_finish = tx_tank_level_switch_to_tcp.clone();

        // thread for controlling duration of test run
        let join_handle_test_environment = thread::Builder::new()
            .name("test_environment".to_string())
            .spawn(move || {
                let sleep_duration_part_1 = Duration::from_secs(duration_test_case_part_1.into());
                let sleep_duration_part_2 = Duration::from_secs(duration_test_case_part_2.into());
                let sleep_duration_thread_processing = Duration::from_millis(50);
                let spin_sleeper = SpinSleeper::default();
                spin_sleeper.sleep(sleep_duration_part_1);
                let _ = tx_test_environment_to_tcp.send(TestCommand::UpdateSignal(
                    MockSignal::TankLevelSwitchPosition(true),
                ));
                {
                    // block the mutex on purpose
                    let _unused = mutex_tank_level_switch_signals_clone_for_test_environment.lock();
                    spin_sleeper.sleep(sleep_duration_part_2);
                }
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Quit);
                spin_sleeper.sleep(sleep_duration_thread_processing);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Quit);
                rx_signal_handler_from_tank_level_switch.recv().unwrap();
                let _ = tx_signal_handler_to_heating.send(InternalCommand::Terminate);
                let _ = tx_signal_handler_to_refill.send(InternalCommand::Terminate);
                let _ = tx_test_environment_to_tcp_for_test_case_finish.send(InternalCommand::Quit);
                let _ = tx_signal_handler_to_tank_level_switch.send(InternalCommand::Terminate);
            })
            .unwrap();

        // thread for the test object
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of the test object
                let tx_tank_level_switch_to_tcp_opt = Some(tx_tank_level_switch_to_tcp);
                let rx_tank_level_switch_from_tcp_opt = Some(rx_tank_level_switch_from_tcp);

                let mut tank_level_switch_channels = TankLevelSwitchChannels {
                    tx_tank_level_switch_to_tcp_opt,
                    #[cfg(feature = "debug_channels")]
                    cnt_tx_tank_level_switch_to_tcp_opt: 0,

                    rx_tank_level_switch_from_tcp_opt,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_tank_level_switch_from_tcp_opt: 0,

                    tx_tank_level_switch_to_signal_handler,
                    #[cfg(feature = "debug_channels")]
                    cnt_tx_tank_level_switch_to_signal_handler: 0,

                    rx_tank_level_switch_from_signal_handler,
                    #[cfg(feature = "debug_channels")]
                    cnt_rx_tank_level_switch_from_signal_handler: 0,
                };

                let _ = tank_level_switch.execute(
                    &mut tank_level_switch_channels,
                    mutex_tank_level_switch_signals,
                );

                // check if monitoring recorded excessive mutex blocking time
                assert_eq!(tank_level_switch.mutex_access_duration_exceeded, true);
            })
            .unwrap();

        join_handle_test_object
            .join()
            .expect("Test object thread did not finish.");
        join_handle_mock_tcp
            .join()
            .expect("Mock TCP thread did not finish.");
        join_handle_test_environment
            .join()
            .expect("Test environment thread did not finish.");

        println!("* [TankLevelSwitch] testing immediate change false -> true for stabilized tank level position when invalid bit is false succeeded while having mutex blocked");
    }
}
