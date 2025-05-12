/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
#![allow(non_snake_case)]

use log::{error, warn};
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
#[cfg(all(target_os = "linux", feature = "target_hw"))]
use std::sync::mpsc::TryRecvError;
use std::sync::mpsc::{Receiver, Sender};

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::sensors::i2c_interface::I2cError;
use crate::simulator::get_resp_sim::GetResponseFromSimulatorTrait;
use crate::utilities::channel_content::{AquariumSignal, InternalCommand};
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

const LOG_TARGET_ATLAS_SCIENTIFIC: &str = "atlas_scientific";

// time interval for repeating measurement
const MEASUREMENT_INTERVAL_SECS: i32 = 1;

// max loops for receiving when the quit command has been received
#[cfg(all(target_os = "linux", feature = "target_hw"))]
const RECEIVE_COUNTER_MAX: u32 = 10;

/// Holds the configuration data for the Atlas Scientific sensor communication.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize, Debug)]
pub struct AtlasScientificConfig {
    /// flag to determine if the application shall communicate with hardware directly or via TCP with a simulator
    pub use_simulator: bool,

    /// replacement value for water temperature in case reading from I2C fails
    pub replacement_value_water_temperature: f32,

    /// replacement value for pH in case reading from I2C fails
    pub replacement_value_ph: f32,

    /// replacement value for conductivity in case reading from I2C fails
    pub replacement_value_conductivity: f32,
}

/// Struct collects the channels for the communication between AtlasScientific and other components.
pub struct AtlasScientificChannels {
    /// Sender part of the channel for communication via TCP wrapped as optional
    pub tx_atlas_scientific_to_tcp_opt: Option<Sender<InternalCommand>>,

    /// Receiver part of the channel for communication via TCP wrapped as optional
    pub rx_atlas_scientific_from_tcp_opt: Option<Receiver<f32>>,

    /// Sender part of the channel for communication to I2CInterface
    pub tx_atlas_scientific_to_i2c_interface_opt: Option<Sender<InternalCommand>>,

    /// Receiver part of the channel for communication from I2CInterface   
    pub rx_atlas_scientific_from_i2c_interface_opt:
        Option<Receiver<AtlasScientificMeasurementResult>>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_atlas_scientific_to_signal_handler: Sender<bool>,

    /// Receiver part of the channel for communication from the signal handler                
    pub rx_atlas_scientific_from_signal_handler: Receiver<InternalCommand>,
}

/// Contains the communication Atlas Scientific sensors via I2C.
/// Alternatively, TCP communication is used when configured to run with the simulator.
/// The struct holds attributes for the results and for error flags indicating unsuccessful communication.
/// Sensor data is read periodically and stored.
/// Requests from data logger, ventilation control and heating control are answered with last measured data.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// Creates the channels and sets up threads.
/// Thread communication of this component is as follows:
/// ```mermaid
/// graph LR
///     atlas_scientific[AtlasScientific] --> tcp_communication[TcpCommunication]
///     tcp_communication --> atlas_scientific
///     heating[Heating] --> atlas_scientific
///     atlas_scientific --> heating
///     ventilation[Ventilation] --> atlas_scientific
///     atlas_scientific --> ventilation
///     atlas_scientific --> signal_handler[SignalHandler]
///     signal_handler --> atlas_scientific
///     atlas_scientific --> data_logger[DataLogger]
///     data_logger --> atlas_scientific
/// ```
pub struct AtlasScientific {
    // configuration data for AtlasScientific
    config: AtlasScientificConfig,

    /// water temperature in °C
    water_temperature: f32,

    /// flag indicating that the water temperature may be inaccurate
    #[allow(dead_code)]
    water_temperature_invalid: bool,

    /// conductivity in mS
    conductivity: f32,

    /// flag indicating that the conductivity may be inaccurate
    #[allow(dead_code)]
    conductivity_invalid: bool,

    /// pH value
    pH: f32,

    /// flag indicating that the pH value may be inaccurate
    #[allow(dead_code)]
    pH_invalid: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive termination signal via the channel
    lock_error_channel_receive_termination: bool,

    /// inhibition flag to avoid flooding the log file with repeated messages about having received inapplicable command via the channel from the signal handler thread
    lock_warn_inapplicable_command_signal_handler: bool,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// inhibition flag to avoid flooding the log file with repeated messages about to send request via the channel to I2cInterface thread
    lock_error_channel_send_i2c_interface: bool,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// inhibition flag to avoid flooding the log file with repeated messages about failure to receive via the channel from I2cInterface thread
    lock_error_channel_receive_i2c_interface: bool,

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// inhibition flag to avoid flooding the log file with repeated messages about failure to read from Atlas Scientific sensors
    lock_error_i2c_interface: bool,
}
impl ProcessExternalRequestTrait for AtlasScientific {}

impl GetResponseFromSimulatorTrait for AtlasScientific {}
type AtlasScientificMeasurementResult = Result<(f32, f32, f32), I2cError>;

impl AtlasScientific {
    /// provides a struct of type TankLevelSwitchSignals
    /// # Arguments
    /// * `config` - configuration data for AtlasScientific
    pub fn new(config: AtlasScientificConfig) -> AtlasScientific {
        AtlasScientific {
            config,
            water_temperature: 0.0,
            water_temperature_invalid: true,
            conductivity: 0.0,
            conductivity_invalid: true,
            pH: 0.0,
            pH_invalid: true,
            lock_error_channel_receive_termination: false,
            lock_warn_inapplicable_command_signal_handler: false,

            #[cfg(all(target_os = "linux", feature = "target_hw"))]
            lock_error_channel_send_i2c_interface: false,
            #[cfg(all(target_os = "linux", feature = "target_hw"))]
            lock_error_channel_receive_i2c_interface: false,
            #[cfg(all(target_os = "linux", feature = "target_hw"))]
            lock_error_i2c_interface: false,
        }
    }

    /// communication with simulator thread via channel
    /// # Arguments
    /// * `tx_atlas_scientific_to_tcp_opt` - reference to sender part of the channel wrapped in Option for sending command to TcpCommunication
    /// * `rx_atlas_scientific_from_tcp_opt` - reference to receiver part of the channel wrapped in Option for receiving data from TcpCommunication
    fn communicate_with_simulator(
        &mut self,
        tx_atlas_scientific_to_tcp_opt: &Option<Sender<InternalCommand>>,
        rx_atlas_scientific_from_tcp_opt: &Option<Receiver<f32>>,
    ) {
        // unwrap is safe because tested above
        let tx_atlas_scientific_to_simulator = tx_atlas_scientific_to_tcp_opt.as_ref().unwrap();
        let rx_atlas_scientific_from_simulator = rx_atlas_scientific_from_tcp_opt.as_ref().unwrap();

        self.water_temperature = Self::get_response_from_simulator(
            tx_atlas_scientific_to_simulator,
            rx_atlas_scientific_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::WaterTemperature),
        );

        self.conductivity = Self::get_response_from_simulator(
            tx_atlas_scientific_to_simulator,
            rx_atlas_scientific_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::Conductivity),
        );

        self.pH = Self::get_response_from_simulator(
            tx_atlas_scientific_to_simulator,
            rx_atlas_scientific_from_simulator,
            InternalCommand::RequestSignal(AquariumSignal::pH),
        );
    }

    /// request execution of measurement by I2cInterface
    /// # Arguments
    /// * `tx_atlas_scientific_to_i2c_interface` - reference to sender part of the channel for sending command to I2cInterface
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    fn send_request_to_i2c_interface(
        &mut self,
        tx_atlas_scientific_to_i2c_interface: &Sender<InternalCommand>,
    ) -> bool {
        match tx_atlas_scientific_to_i2c_interface.send(InternalCommand::RequestSignal(
            AquariumSignal::WaterTemperature,
        )) {
            Ok(()) => {
                self.lock_error_channel_send_i2c_interface = false;
                true
            }
            Err(e) => {
                if !self.lock_error_channel_send_i2c_interface {
                    error!(
                        target: LOG_TARGET_ATLAS_SCIENTIFIC,
                        "sending request to I2cInterface thread failed. ({e:?})"
                    );
                    self.lock_error_channel_send_i2c_interface = true;
                }
                false
            }
        }
    }

    /// receive and process response from I2cInterface
    /// # Arguments
    /// * `rx_atlas_scientific_from_i2c_interface` - reference to the receiver part of the channel for receiving data from I2cInterface
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    fn receive_response_from_i2c_interface(
        &mut self,
        rx_atlas_scientific_from_i2c_interface: &Receiver<Result<(f32, f32, f32), I2cError>>,
    ) -> bool {
        match rx_atlas_scientific_from_i2c_interface.try_recv() {
            Ok(response) => match response {
                Ok((water_temperature, pH, conductivity)) => {
                    self.water_temperature = water_temperature;
                    self.pH = pH;
                    self.conductivity = conductivity;
                    self.water_temperature_invalid = false;
                    self.pH_invalid = false;
                    self.conductivity_invalid = false;
                    self.lock_error_i2c_interface = false;
                    false // allow for the next request to be sent
                }
                #[cfg(not(test))]
                Err(e) => {
                    self.water_temperature_invalid = true;
                    self.pH_invalid = true;
                    self.conductivity_invalid = true;
                    #[cfg(not(test))]
                    if !self.lock_error_i2c_interface {
                        error!(
                            target: LOG_TARGET_ATLAS_SCIENTIFIC,
                            "failure occurred when reading from I2cInterface ({e:?})"
                        );
                        self.lock_error_i2c_interface = true;
                    }
                    false // allow for the next request to be sent
                }
                #[cfg(test)]
                Err(_) => {
                    self.water_temperature_invalid = true;
                    self.pH_invalid = true;
                    self.conductivity_invalid = true;
                    true // do not allow for the next request to be sent
                }
            },
            Err(e) => {
                match e {
                    TryRecvError::Empty => {
                        /* empty buffer - no action required */
                        self.lock_error_channel_receive_i2c_interface = false;
                    }
                    TryRecvError::Disconnected => {
                        if !self.lock_error_channel_receive_i2c_interface {
                            error!(
                                target: LOG_TARGET_ATLAS_SCIENTIFIC,
                                "error occurred when trying to receive response from I2cInterface ({e:?})"
                            );
                            self.lock_error_channel_receive_i2c_interface = true;
                        }
                    }
                }
                true // repeat receiving
            }
        }
    }

    /// Executes the main control in a loop until the signal handler commands quitting and termination.
    /// # Arguments
    /// * `atlas_scientific_channels` - struct holding the channels for communication between AtlasScientific and other components
    /// * `mutex_atlas_scientific_water_temperature` - mutex for the water temperature
    /// * `mutex_atlas_scientific_ph` - mutex for the pH
    /// * `mutex_atlas_scientific_conductivity` - mutex for the conductivity
    pub fn execute(
        &mut self,
        atlas_scientific_channels: AtlasScientificChannels,
        mutex_atlas_scientific_water_temperature: Arc<Mutex<f32>>,
        mutex_atlas_scientific_ph: Arc<Mutex<f32>>,
        mutex_atlas_scientific_conductivity: Arc<Mutex<f32>>,
    ) {
        let sleep_duration_ten_millis = Duration::from_millis(10);
        let sleep_duration_one_millis = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();

        cfg_if::cfg_if! {
            if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
                let mut state_send_receive: bool = false;
                let mut receive_counter: u32 = 0;
            }
        }

        #[cfg(any(not(target_os = "linux"), not(feature = "target_hw")))]
        if atlas_scientific_channels
            .tx_atlas_scientific_to_i2c_interface_opt
            .is_some()
            || atlas_scientific_channels
                .rx_atlas_scientific_from_i2c_interface_opt
                .is_some()
        {
            warn!(target: LOG_TARGET_ATLAS_SCIENTIFIC,
                "Sender/Receiver to I2cInterface are not 'None'");
        }

        // initialize signals with replacement values
        self.water_temperature = self.config.replacement_value_water_temperature;
        self.pH = self.config.replacement_value_ph;
        self.conductivity = self.config.replacement_value_conductivity;

        // initialize countdown
        let mut measurement_countdown: i32 = MEASUREMENT_INTERVAL_SECS * 100;

        if (self.config.use_simulator)
            && (atlas_scientific_channels
                .tx_atlas_scientific_to_tcp_opt
                .is_none()
                | atlas_scientific_channels
                    .rx_atlas_scientific_from_tcp_opt
                    .is_none())
        {
            panic!(
                "{}: configured to run with simulator, but no channel to TCP thread provided.",
                LOG_TARGET_ATLAS_SCIENTIFIC
            );
        }

        loop {
            let (
                quit_command_received, // the request to end the application has been received
                _,
                _,
            ) = self.process_external_request(
                &atlas_scientific_channels.rx_atlas_scientific_from_signal_handler,
                None,
            );

            cfg_if::cfg_if! {
                if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
                    if quit_command_received && (!state_send_receive || receive_counter > RECEIVE_COUNTER_MAX) {
                        // quit command has been received, and there is no pending measurement
                        // no further waiting for Dht is required
                        break;
                    }
                }
                else {
                    if quit_command_received {
                        break;
                    }
                }
            }

            if measurement_countdown > 0 {
                measurement_countdown = measurement_countdown.saturating_sub(1);
            } else if !self.config.use_simulator {
                #[cfg(all(target_os = "linux", feature = "target_hw"))]
                {
                    if atlas_scientific_channels.tx_atlas_scientific_to_i2c_interface_opt.is_some()
                        && atlas_scientific_channels.rx_atlas_scientific_from_i2c_interface_opt.is_some()
                    {
                        if !state_send_receive {
                            let tx_atlas_scientific_to_i2c_interface =
                                atlas_scientific_channels.tx_atlas_scientific_to_i2c_interface_opt.as_ref().unwrap();

                            state_send_receive = self.send_request_to_i2c_interface(
                                &tx_atlas_scientific_to_i2c_interface,
                            );
                        } else {
                            // read from the channel if the measurement result is available from I2cInterface thread
                            let rx_atlas_scientific_from_i2c_interface =
                                atlas_scientific_channels.rx_atlas_scientific_from_i2c_interface_opt.as_ref().unwrap();

                            state_send_receive = self.receive_response_from_i2c_interface(
                                &rx_atlas_scientific_from_i2c_interface,
                            );

                            if !state_send_receive {
                                // data could be received successfully
                                measurement_countdown = MEASUREMENT_INTERVAL_SECS * 10;
                                receive_counter = 0;
                                {
                                    let mut mutex_water_temperature =
                                        mutex_atlas_scientific_water_temperature.lock().unwrap();
                                    let mut mutex_ph = mutex_atlas_scientific_ph.lock().unwrap();
                                    let mut mutex_atlas_scientific =
                                        mutex_atlas_scientific_conductivity.lock().unwrap();
                                    *mutex_water_temperature = self.water_temperature;
                                    *mutex_ph = self.pH;
                                    *mutex_atlas_scientific = self.conductivity;
                                }
                            } else {
                                receive_counter = receive_counter.saturating_add(1);
                            }
                        }
                    }
                }
            } else {
                self.communicate_with_simulator(
                    &atlas_scientific_channels.tx_atlas_scientific_to_tcp_opt,
                    &atlas_scientific_channels.rx_atlas_scientific_from_tcp_opt,
                );
                {
                    let mut mutex_water_temperature =
                        mutex_atlas_scientific_water_temperature.lock().unwrap();
                    let mut mutex_ph = mutex_atlas_scientific_ph.lock().unwrap();
                    let mut mutex_atlas_scientific =
                        mutex_atlas_scientific_conductivity.lock().unwrap();
                    *mutex_water_temperature = self.water_temperature;
                    *mutex_ph = self.pH;
                    *mutex_atlas_scientific = self.conductivity;
                }
                measurement_countdown = MEASUREMENT_INTERVAL_SECS * 10;
            }

            spin_sleeper.sleep(sleep_duration_ten_millis);
        }

        match atlas_scientific_channels
            .tx_atlas_scientific_to_signal_handler
            .send(true)
        {
            Ok(()) => {}
            Err(e) => {
                error!(
                    target: LOG_TARGET_ATLAS_SCIENTIFIC,
                    "sending quit confirmation back to signal handler failed ({e:?})"
                );
            }
        };

        // This thread has channel connections to underlying threads (GpioHandler).
        // Those threads have to stop receiving commands from this thread.
        // The shutdown sequence is handled by the signal_handler module.
        let mut termination_command_received = false;
        while !termination_command_received {
            termination_command_received = match atlas_scientific_channels
                .rx_atlas_scientific_from_signal_handler
                .recv()
            {
                Ok(c) => match c {
                    InternalCommand::Terminate => {
                        self.lock_warn_inapplicable_command_signal_handler = false;
                        true
                    }
                    _ => {
                        if !self.lock_warn_inapplicable_command_signal_handler {
                            warn!(
                                target: LOG_TARGET_ATLAS_SCIENTIFIC,
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
                            target: LOG_TARGET_ATLAS_SCIENTIFIC,
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
