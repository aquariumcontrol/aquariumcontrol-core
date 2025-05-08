/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#[cfg(feature = "debug_tank_level_switch")]
use log::debug;
use std::sync::{Arc, Mutex};

use log::{error, warn};
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, SystemTime};

#[cfg(all(target_os = "linux", feature = "target_hw"))]
use rppal::gpio::InputPin;

use crate::simulator::get_resp_sim::GetResponseFromSimulatorTrait;
use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

/// Holds the configuration data for the tank level switch calculation.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Clone)]
pub struct TankLevelSwitchConfig {
    /// flag to determine if the application shall communicate with hardware directly or via TCP with a simulator
    pub use_simulator: bool,

    /// number of seconds which need to indicate a low level before the refill control assumes that the water level is low
    tank_level_switch_position_low_stabilization_count: u32,
}

const LOG_TARGET_TANK_LEVEL_SWITCH: &str = "tank_level_switch";

/// application is monitoring if execution duration can maintain this limit
/// sleep time is adjusted accordingly to run at fixed execution time
const CYCLE_TIME_TANK_LEVEL_SWITCH_MILLIS: u64 = 100;

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
    /// provides a struct of type TankLevelSwitchSignals for non-Linux platforms when not running on target HW
    /// # Arguments
    /// * `config` - configuration data for TankLevelSwitch
    /// * `initial_position_stabilized` - assumed initial position of tank level before the first measurement
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
///     refill[Refill] --> tank_level_switch[TankLevelSwitch]
///     tank_level_switch --> refill
///     signal_handler[SignalHandler] --> tank_level_switch
///     tank_level_switch --> signal_handler
///     tank_level_switch --> tcp_communication[TcpCommunication]
///     tcp_communication --> tank_level_switch
///     heating[Heating] --> tank_level_switch
///     tank_level_switch --> heating
///     tank_level_switch --> data_logger[DataLogger]
///     data_logger --> tank_level_switch
/// ```
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
    lock_warn_inapplicable_command_signal_handler: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    lock_error_channel_receive_termination: bool,
    
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// inhibition flag to avoid flooding the log file with repeated messages about not having received an input pin
    lock_error_input_pin_none: bool,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    // interface to GPIO pin wrapped as optional
    input_pin_opt: Option<InputPin>,
}

impl ProcessExternalRequestTrait for TankLevelSwitch {}

impl GetResponseFromSimulatorTrait for TankLevelSwitch {}

impl TankLevelSwitch {
    #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
    /// provides a struct of type TankLevelSwitch for non-Linux platforms when not running on target HW
    /// # Arguments
    /// * `config` - configuration data for TankLevelSwitch
    /// * `initial_position_stabilized` - assumed initial position of tank level before the first measurement
    pub fn new(
        config: TankLevelSwitchConfig,
        initial_position_stabilized: bool,
    ) -> TankLevelSwitch {
        TankLevelSwitch {
            config,
            tank_level_switch_position: true, // initialization value assuming no error
            tank_level_switch_invalid: false, // initialization value assuming no error
            tank_level_switch_position_stabilized: initial_position_stabilized,
            tank_level_switch_position_low_stabilization_counter: 0,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
        }
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// provides a struct of type TankLevelSwitchSignals for target HW
    /// # Arguments
    /// * `config` - configuration data for TankLevelSwitch
    /// * `initial_position_stabilized` - assumed initial position of tank level before the first measurement
    /// * `input_pin_opt` - interface to GPIO pin wrapped as optional
    pub fn new(
        config: TankLevelSwitchConfig,
        initial_position_stabilized: bool,
        input_pin_opt: Option<InputPin>,
    ) -> TankLevelSwitch {
        if !config.use_simulator && !input_pin_opt.is_some() {
            panic!(
                "{}: application shall use real HW, but GPIO pin interface not provided.",
                LOG_TARGET_TANK_LEVEL_SWITCH
            );
        }

        TankLevelSwitch {
            config: config,
            tank_level_switch_position: true, // initialization value assuming no error
            tank_level_switch_invalid: false, // initialization value assuming no error
            tank_level_switch_position_stabilized: initial_position_stabilized,
            tank_level_switch_position_low_stabilization_counter: 0,
            lock_warn_inapplicable_command_signal_handler: false,
            lock_error_channel_receive_termination: false,
            lock_error_invalid_signal_request_refill: false,
            lock_warn_inapplicable_command_refill: false,
            lock_error_channel_receive_refill: false,
            lock_error_refill_response: false,
            lock_error_invalid_signal_request_heating: false,
            lock_warn_inapplicable_command_heating: false,
            lock_error_channel_receive_heating: false,
            lock_error_heating_response: false,
            lock_error_invalid_signal_request_data_logger: false,
            lock_warn_inapplicable_command_data_logger: false,
            lock_error_channel_receive_data_logger: false,
            lock_error_data_logger_response: false,
            lock_error_input_pin_none: false,
            input_pin_opt: input_pin_opt,
        }
    }

    /// Executes either
    /// - the measurement of GPIO pin position or
    /// - the channel communication to update the tank level switch position signal.
    /// # Arguments
    /// * `tx_to_tcp_opt` - sender part of the channel for communication via TCP wrapped as optional
    /// * `rx_from_tcp_opt` - receiver part of the channel for communication via TCP wrapped as optional
    pub fn update_tank_level_switch_signals(
        &mut self,
        tx_to_tcp_opt: &Option<Sender<InternalCommand>>,
        rx_from_tcp_opt: &Option<Receiver<f32>>,
    ) {
        if self.config.use_simulator {
            let tx_to_tcp = tx_to_tcp_opt.as_ref().unwrap();
            let rx_from_tcp = rx_from_tcp_opt.as_ref().unwrap();
            self.tank_level_switch_position = Self::get_response_from_simulator(
                &tx_to_tcp,
                &rx_from_tcp,
                InternalCommand::RequestSignal(AquariumSignal::TankLevelSwitchPosition),
            ) > 0.0;
        } else {
            #[cfg(all(target_os = "linux", feature = "target_hw"))]
            {
                // unwrap is safe because tested in constructor
                match self.input_pin_opt.as_mut() {
                    Some(pin) => {
                        self.tank_level_switch_position = pin.is_high();
                        self.lock_error_input_pin_none = false;
                    }
                    None => {
                        if !self.lock_error_input_pin_none {
                            error!(
                                target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                "Input pin not provided"
                            );
                            self.lock_error_input_pin_none = true;
                        }
                    }
                }
            }
        }
    }

    /// Calculates a stabilized tank level switch position signal considering:
    /// - if the invalid bit of signal is true, then use the current value of the stabilized position signal.
    /// - if the new signal is true (indicating high water level), then set the stabilized signal immediately to true (high).
    /// - If the new signal is false (indicating low water level), then set increase the stabilization counter.
    /// - If the stabilization counter exceeds the configured limit, then set the counter to zero and set the stabilized
    /// Signal value 'true' indicates high water level.
    fn calc_tank_level_switch_position_stabilized(&mut self) {
        if self.tank_level_switch_invalid {
            // go to safe position if the input signal is invalid
            self.tank_level_switch_position_stabilized = true;
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
                        self.tank_level_switch_position_stabilized //
                    } else {
                        false
                    }
                }
            }
        }
    }

    /// Executes the main control in a loop until the signal handler commands quitting and termination.
    /// # Arguments
    /// * `tx_to_tcp_opt` - Sender part of the channel for communication via TCP wrapped as optional
    /// * `rx_from_tcp_opt` - Receiver part of the channel for communication via TCP wrapped as optional
    /// * `tx_to_signal_handler` - Sender part of the channel for communication to signal handler
    /// * `rx_from_signal_handler` - Receiver part of the channel for communication from signal handler                
    /// * `mutex_tank_level_switch_signals` - Mutex containing tank level switch signals
    pub fn execute(
        &mut self,
        tx_to_tcp_opt: &Option<Sender<InternalCommand>>,
        rx_from_tcp_opt: &Option<Receiver<f32>>,
        tx_to_signal_handler: &Sender<bool>,
        rx_from_signal_handler: &Receiver<InternalCommand>,
        mutex_tank_level_switch_signals: Arc<Mutex<TankLevelSwitchSignals>>,
    ) {
        let sleep_duration_one_millis = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        let mut lock_warn_cycle_time_exceeded: bool = false;

        if (self.config.use_simulator) && (tx_to_tcp_opt.is_none() | rx_from_tcp_opt.is_none()) {
            panic!(
                "{}: configured to run with simulator, but no channel to TCP thread provided.",
                LOG_TARGET_TANK_LEVEL_SWITCH
            );
        }

        let mut start_time = SystemTime::now();

        loop {
            let (
                quit_command_received, // a request to end the application has been received
                _,
                _,
            ) = self.process_external_request(rx_from_signal_handler, None);
            if quit_command_received {
                break;
            }

            // either communicate with real HW or with the simulator to get updated signal information
            self.update_tank_level_switch_signals(&tx_to_tcp_opt, &rx_from_tcp_opt);

            self.calc_tank_level_switch_position_stabilized();
            // Write calculated signals to mutex
            {
                match mutex_tank_level_switch_signals.lock() {
                    Ok(mut c) => {
                        c.tank_level_switch_position_stabilized =
                            self.tank_level_switch_position_stabilized;
                        c.tank_level_switch_invalid = self.tank_level_switch_invalid;
                        c.tank_level_switch_position = self.tank_level_switch_position;
                    }
                    Err(_) => {
                        // Do nothing
                    }
                }
            }

            let stop_time = SystemTime::now();
            let execution_duration_millis =
                stop_time.duration_since(start_time).unwrap().as_millis() as u64;

            let remaining_sleep_time_millis: u64;
            if execution_duration_millis >= CYCLE_TIME_TANK_LEVEL_SWITCH_MILLIS {
                if !lock_warn_cycle_time_exceeded {
                    warn!(target: LOG_TARGET_TANK_LEVEL_SWITCH,
                        "execution duration of {} milliseconds exceeds cycle time of {}",
                        execution_duration_millis,
                        CYCLE_TIME_TANK_LEVEL_SWITCH_MILLIS);
                    lock_warn_cycle_time_exceeded = true;
                } else {
                    lock_warn_cycle_time_exceeded = false;
                }
                remaining_sleep_time_millis = 0;
            } else {
                remaining_sleep_time_millis =
                    CYCLE_TIME_TANK_LEVEL_SWITCH_MILLIS - execution_duration_millis;
            }
            // sleep time does not need to be split up because cycle time is 100 milliseconds only
            let sleep_duration = Duration::from_millis(remaining_sleep_time_millis);
            spin_sleeper.sleep(sleep_duration);

            start_time = SystemTime::now();
        }

        _ = match tx_to_signal_handler.send(true) {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_TANK_LEVEL_SWITCH,
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // This thread has channel connections to underlying threads (GpioHandler).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match rx_from_signal_handler.recv() {
                Ok(c) => match c {
                    InternalCommand::Terminate => {
                        self.lock_warn_inapplicable_command_signal_handler = false;
                        true
                    }
                    _ => {
                        if !self.lock_warn_inapplicable_command_signal_handler {
                            warn!(
                                target: LOG_TARGET_TANK_LEVEL_SWITCH,
                                "ignoring inapplicable command from signal handler ({})",
                                c
                            );
                            self.lock_warn_inapplicable_command_signal_handler = true;
                        }
                        false
                    }
                },
                Err(e) => {
                    if !self.lock_error_channel_receive_termination {
                        error!(
                            target: LOG_TARGET_TANK_LEVEL_SWITCH,
                            "receiving termination signal from signal handler failed ({e:?})"
                        );
                        self.lock_error_channel_receive_termination = true;
                    }
                    false
                }
            };
            spin_sleeper.sleep(sleep_duration_one_millis);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use spin_sleep::SpinSleeper;
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use crate::sensors::tank_level_switch::{TankLevelSwitch, TankLevelSwitchSignals};
    use crate::stubs::mock_signal::tests::MockSignal;
    use crate::stubs::mock_tcp::tests::mock_tcp;
    use crate::stubs::test_command::tests::TestCommand;
    use crate::utilities::channel_content::InternalCommand;
    use crate::utilities::config::{read_config_file, ConfigData};

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
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        let duration_test_case_secs = config
            .tank_level_switch
            .tank_level_switch_position_low_stabilization_count
            + 1;

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true, None);
        #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
        let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true);

        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock tcp
        let join_handle_mock_tcp = thread::Builder::new()
            .name("mock_tcp".to_string())
            .spawn(move || {
                mock_tcp(
                    &tx_tcp_to_tank_level_switch,
                    &rx_tcp_from_tank_level_switch,
                    Some(&rx_tcp_from_test_environment),
                    true,
                    false,
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

                tank_level_switch.execute(
                    &tx_tank_level_switch_to_tcp_opt,
                    &rx_tank_level_switch_from_tcp_opt,
                    &tx_tank_level_switch_to_signal_handler,
                    &rx_tank_level_switch_from_signal_handler,
                    mutex_tank_level_switch_signals,
                );

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
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        config.refill.initial_wait_interval = 2;
        config.refill.check_interval = 60;

        let duration_test_case_part_1: u64 = 3; // three seconds with level switch in low position
        let duration_test_case_part_2: u64 = 2; // two seconds with level switch in high position

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, false, None);
        #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
        let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, false);
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock tcp
        let join_handle_mock_tcp = thread::Builder::new()
            .name("mock_tcp".to_string())
            .spawn(move || {
                mock_tcp(
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
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of the test object
                let tx_tank_level_switch_to_tcp_opt = Some(tx_tank_level_switch_to_tcp);
                let rx_tank_level_switch_from_tcp_opt = Some(rx_tank_level_switch_from_tcp);

                tank_level_switch.execute(
                    &tx_tank_level_switch_to_tcp_opt,
                    &rx_tank_level_switch_from_tcp_opt,
                    &tx_tank_level_switch_to_signal_handler,
                    &rx_tank_level_switch_from_signal_handler,
                    mutex_tank_level_switch_signals,
                );
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
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        let duration_test_case_part_1: u64 = 3; // three seconds with level switch in low position
        let duration_test_case_part_2: u64 = 8; // two seconds with level switch in high position
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true, None);
        #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
        let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true);
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock tcp
        let join_handle_mock_tcp = thread::Builder::new()
            .name("mock_tcp".to_string())
            .spawn(move || {
                mock_tcp(
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
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of the test object
                let tx_tank_level_switch_to_tcp_opt = Some(tx_tank_level_switch_to_tcp);
                let rx_tank_level_switch_from_tcp_opt = Some(rx_tank_level_switch_from_tcp);

                tank_level_switch.execute(
                    &tx_tank_level_switch_to_tcp_opt,
                    &rx_tank_level_switch_from_tcp_opt,
                    &tx_tank_level_switch_to_signal_handler,
                    &rx_tank_level_switch_from_signal_handler,
                    mutex_tank_level_switch_signals,
                );
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
            read_config_file("/config/aquarium_control_test_generic.toml".to_string());
        let duration_test_case_part: u64 = 2; // two seconds with the level switch before toggling

        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true, None);
        #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
        let mut tank_level_switch = TankLevelSwitch::new(config.tank_level_switch, true);
        let mutex_tank_level_switch_signals =
            Arc::new(Mutex::new(TankLevelSwitchSignals::new(true, true, false)));

        // thread for mock tcp
        let join_handle_mock_tcp = thread::Builder::new()
            .name("mock_tcp".to_string())
            .spawn(move || {
                mock_tcp(
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
        let join_handle_test_object = thread::Builder::new()
            .name("test_object".to_string())
            .spawn(move || {
                // clone sender part of channels to mock threads so that we can terminate them after execution of the test object
                let tx_tank_level_switch_to_tcp_opt = Some(tx_tank_level_switch_to_tcp);
                let rx_tank_level_switch_from_tcp_opt = Some(rx_tank_level_switch_from_tcp);

                tank_level_switch.execute(
                    &tx_tank_level_switch_to_tcp_opt,
                    &rx_tank_level_switch_from_tcp_opt,
                    &tx_tank_level_switch_to_signal_handler,
                    &rx_tank_level_switch_from_signal_handler,
                    mutex_tank_level_switch_signals,
                );
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

        println!("* [TankLevelSwitch] checking delayed change true -> false for stabilized tank level position when invalid bit is false succeeded.");
    }
}
