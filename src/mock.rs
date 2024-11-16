/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use chrono::{DateTime, Utc};
    use spin_sleep::SpinSleeper;
    use std::fmt;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::time::{Duration, SystemTime};

    use crate::channel_content::AquariumSignal;
    use crate::channel_content::InternalCommand;
    use crate::data_injection::DataInjectionTrait;
    use crate::refill::RefillErrors;
    use crate::sql_interface_data::DataFrame;
    use crate::sql_interface_data::SqlInterfaceData;
    use crate::sql_interface_refill::SqlInterfaceRefill;
    use crate::water_injection::WaterInjectionTrait;

    /// Enum is used for internal thread messaging between test environment and mock threads
    #[derive(Clone, Debug, PartialEq)]
    pub enum MockSignal {
        /// position of tank level switch
        TankLevelSwitchPosition(bool),

        #[allow(dead_code)]
        /// status bit of tank level switch signal
        TankLevelSwitchInvalid(bool),

        /// position of tank level switch stabilized
        TankLevelSwitchPositionStabilized(bool),

        /// water temperature in °C
        WaterTemperature(f32),

        #[allow(non_camel_case_types)]
        /// pH Value
        pH(f32),

        /// conductivity im uS/cm
        Conductivity(f32),

        /// ambient (air) temperature in °C
        AmbientTemperature(f32),

        /// ambient humidity in %
        AmbientHumidity(f32),

        /// heating control status
        HeatingControlStatus(bool),

        /// status of ventilation control
        VentilationControlStatus(bool),

        /// refill pump status
        RefillPumpStatus(bool),

        /// schedule alllows operation
        ScheduleAllows(bool),
    }

    impl fmt::Display for MockSignal {
        /// Function provides output of the enum values for debugging purposes.
        /// # Arguments
        /// * `f` - configuration for formatting    
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match self {
                MockSignal::TankLevelSwitchPosition(d) => write!(
                    f,
                    "TankLevelSwitchPosition({})",
                    match d {
                        true => "high",
                        false => "low",
                    }
                ),
                MockSignal::TankLevelSwitchInvalid(d) => write!(
                    f,
                    "TankLevelSwitchInvalid({})",
                    match d {
                        true => "invalid",
                        false => "valid",
                    }
                ),
                MockSignal::TankLevelSwitchPositionStabilized(d) => write!(
                    f,
                    "TankLevelSwitchPositionStabilized({})",
                    match d {
                        true => "high",
                        false => "low",
                    }
                ),
                MockSignal::WaterTemperature(d) => write!(f, "WaterTemperature({})", d),
                MockSignal::pH(d) => write!(f, "pH({})", d),
                MockSignal::Conductivity(d) => write!(f, "Conductivity({})", d),
                MockSignal::AmbientTemperature(d) => write!(f, "AmbientTemperature({})", d),
                MockSignal::AmbientHumidity(d) => write!(f, "AmbientHumidity({})", d),
                MockSignal::HeatingControlStatus(d) => write!(
                    f,
                    "VentilationControlStatus({})",
                    match d {
                        true => "on",
                        false => "off",
                    }
                ),
                MockSignal::VentilationControlStatus(d) => write!(
                    f,
                    "VentilationControlStatus({})",
                    match d {
                        true => "on",
                        false => "off",
                    }
                ),
                MockSignal::RefillPumpStatus(d) => write!(
                    f,
                    "RefillPumpStatus({})",
                    match d {
                        true => "on",
                        false => "off",
                    }
                ),
                MockSignal::ScheduleAllows(d) => write!(
                    f,
                    "ScheduleAllows({})",
                    match d {
                        true => "allowed",
                        false => "not allowed",
                    }
                ),
            }
        }
    }

    /// Enum is used for internal thread messaging.
    /// Note: Relay state and device state do not necessarily correlate.
    #[derive(Clone, Debug, PartialEq)]
    pub enum TestCommand {
        /// Request to update a signal in to be used by mock thread
        UpdateSignal(MockSignal),

        /// give feedback to test environment that water injection has been triggered
        InformWaterInjectionEvent,
    }

    impl fmt::Display for TestCommand {
        /// Function provides output of the enum values for debugging purposes.
        /// # Arguments
        /// * `f` - configuration for formatting    
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            match self {
                TestCommand::UpdateSignal(d) => write!(f, "TestCommand::UpdateSignal({})", d),
                TestCommand::InformWaterInjectionEvent => {
                    write!(f, "TestCommand::InformWaterInjectionEvent",)
                }
            }
        }
    }

    #[derive(Clone)]
    pub struct ActuationEvent {
        pub time: SystemTime,
        pub command: InternalCommand,
    }

    impl ActuationEvent {
        pub fn new(time: SystemTime, command: InternalCommand) -> ActuationEvent {
            ActuationEvent {
                time: time,
                command: command,
            }
        }
    }

    impl fmt::Display for ActuationEvent {
        /// Function provides output of ActuationEvent for development purposes
        /// # Arguments
        /// * `f` - configuration for formatting
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let datetime: DateTime<Utc> = self.time.into();
            write!(f, "{}: {}", datetime.format("%d/%m/%Y %T"), self.command)
        }
    }

    pub struct MockWaterInjection {
        tx_water_injection_to_test_environment_opt: Option<Sender<TestCommand>>,

        pub injection_recorder: Vec<SystemTime>,
    }

    impl MockWaterInjection {
        pub fn new(
            tx_water_injection_to_test_environment_opt: Option<Sender<TestCommand>>,
        ) -> MockWaterInjection {
            MockWaterInjection {
                tx_water_injection_to_test_environment_opt:
                    tx_water_injection_to_test_environment_opt,
                injection_recorder: Vec::new(),
            }
        }
    }

    impl WaterInjectionTrait for MockWaterInjection {
        fn inject_water(
            &mut self,
            _tx_to_controllino: &Sender<InternalCommand>,
            _rx_from_controllino: &Receiver<bool>,
            _tx_to_tank_level_switch: &Sender<InternalCommand>,
            _rx_from_tank_level_switch: &Receiver<bool>,
            _rx_from_signal_handler: &Receiver<InternalCommand>,
            _tx_to_data_logger: &Sender<bool>,
            _rx_from_data_logger: &Receiver<InternalCommand>,
            _refill_errors: &mut RefillErrors,
            _sql_interface_refill: &mut SqlInterfaceRefill,
        ) -> bool {
            if self.tx_water_injection_to_test_environment_opt.is_some() {
                let tx_water_injection_to_test_environment = self
                    .tx_water_injection_to_test_environment_opt
                    .clone()
                    .unwrap();
                let _ = tx_water_injection_to_test_environment
                    .send(TestCommand::InformWaterInjectionEvent);
            }
            self.injection_recorder.push(SystemTime::now());
            false // simulate that no Quit command has been received
        }
    }

    pub struct MockDataInjection {
        pub data_frames: Vec<DataFrame>,
    }

    impl MockDataInjection {
        pub fn new() -> MockDataInjection {
            MockDataInjection {
                data_frames: Vec::new(),
            }
        }

        pub fn output(&self) {
            if self.data_frames.is_empty() {
                println!("MockDataInjection has not recorded any data frames.");
            }
            for (position, data_frame) in self.data_frames.iter().enumerate() {
                println!("Dataframe #{}: {}", position, data_frame);
            }
        }
    }

    impl DataInjectionTrait for MockDataInjection {
        fn inject_data(&mut self, data_frame: DataFrame, _: &mut SqlInterfaceData) {
            println!(
                "MockDataInjection.inject_data: recording data frame: {}",
                data_frame
            );

            // add data frame to vector
            self.data_frames.push(data_frame);
        }
    }

    pub fn mock_controllino(
        tx_controllino_to_test_object: &Sender<bool>,
        rx_controllino_from_test_object: &Receiver<InternalCommand>,
    ) -> Vec<ActuationEvent> {
        let mut result: Vec<ActuationEvent> = Vec::new();

        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            // receive command from test object
            match rx_controllino_from_test_object.recv() {
                Ok(command) => {
                    match command {
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            // println!("mock_controllino: Received command {}", command);
                            // store tuple of SystemTime and command in vector
                            result.push(ActuationEvent::new(SystemTime::now(), command));
                        }
                    }
                }
                Err(e) => {
                    panic!("Mock Controllino: Error when receiving actuation command from test object: {e:?}");
                }
            }

            // send confirmation back
            match tx_controllino_to_test_object.send(true) {
                Ok(_) => {}
                Err(e) => {
                    panic!("Mock Controllino: Error when sending back actuation command confirmation to test object: {e:?}");
                }
            }

            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
        result
    }

    pub fn mock_refill(
        tx_refill_to_tank_level_switch_opt: Option<&Sender<InternalCommand>>,
        rx_refill_from_tank_level_switch_opt: Option<&Receiver<bool>>,
        rx_refill_from_signal_handler: &Receiver<InternalCommand>,
        tx_refill_to_data_logger_opt: Option<&Sender<bool>>,
        rx_refill_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        initial_stimuli: bool,
    ) -> Vec<bool> {
        let mut result: Vec<bool> = Vec::new();

        let sleep_duration = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        let mut sending_blocked: bool = false;
        let mut loop_counter = 0;
        let mut stimuli = initial_stimuli;

        loop {
            if !sending_blocked {
                match tx_refill_to_tank_level_switch_opt {
                    Some(tx_refill_to_tank_level_switch) => {
                        match tx_refill_to_tank_level_switch.send(InternalCommand::RequestSignal(
                            AquariumSignal::TankLevelSwitchPositionStabilized,
                        )) {
                            Ok(_) => {
                                match rx_refill_from_tank_level_switch_opt {
                                    Some(rx_refill_from_tank_level_switch) => {
                                        match rx_refill_from_tank_level_switch.try_recv() {
                                            Ok(d) => {
                                                result.push(d);
                                            }
                                            Err(e) => {
                                                match e {
                                                    TryRecvError::Empty => { /* empty buffer - no action required */
                                                    }
                                                    TryRecvError::Disconnected => {
                                                        panic!(
                                                            "Mock Refill: Error in trying to receive command from TankLevelSwitch: {e:?}"
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    None => {
                                        panic!(
                                            "Mock Refill: invoked with valid sender but without valid receiver for channel communication with tank level switch"
                                        )
                                    }
                                }
                            }
                            Err(e) => {
                                panic!(
                                    "Mock Refill: Could not send signal request to TankLevelSwitch ({e:?})"
                                )
                            }
                        }
                    }
                    None => { /* do nothing */ }
                };
            }

            match rx_refill_from_signal_handler.try_recv() {
                Ok(c) => {
                    match c {
                        InternalCommand::Quit => {
                            sending_blocked = true;
                            println!("Mock refill: Received Quit signal - blocking sending");
                        }
                        InternalCommand::Terminate => {
                            break;
                        }
                        _ => {
                            panic!("Mock Refill: Received invalid command.");
                        }
                    };
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!(
                                "Mock Refill: Error in trying to receive command from signal handler: {e:?}"
                            );
                        }
                    }
                }
            };
            match rx_refill_from_test_environment_opt {
                Some(rx_refill_from_test_environment) => {
                    match rx_refill_from_test_environment.try_recv() {
                        Ok(c) => {
                            let test_command = c.clone();
                            match c {
                                TestCommand::UpdateSignal(d) => {
                                    let test_signal = d.clone();
                                    match d {
                                        MockSignal::RefillPumpStatus(c) => stimuli = c,
                                        _ => {
                                            panic!("Mock Refill: received test command for invalid signal {}", test_signal);
                                        }
                                    }
                                }
                                _ => {
                                    panic!(
                                        "Mock Refill: received invalid command {}.",
                                        test_command
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!(
                                "Mock Refill: Error in trying to receive command from test environment: {e:?}"
                            );
                                }
                            }
                        }
                    }
                }
                None => { /* do nothing */ }
            };
            if loop_counter % 1000 == 0 {
                // send data to data logger only once a second
                if !sending_blocked {
                    match tx_refill_to_data_logger_opt {
                        Some(tx_refill_to_data_logger) => {
                            match tx_refill_to_data_logger.send(stimuli) {
                                Ok(_) => { /* do nothing */ }
                                Err(e) => {
                                    panic!(
                                        "Mock Refill: Could not send signal request to DataLogger ({e:?})"
                                    )
                                }
                            };
                        }
                        None => { /* do nothing */ }
                    };
                }
            }
            spin_sleeper.sleep(sleep_duration);
            loop_counter += 1;
        }
        result
    }

    pub fn mock_data_logger(
        _tx_data_logger_to_test_object: &Sender<InternalCommand>,
        rx_data_logger_from_test_object: &Receiver<bool>,
        rx_data_logger_from_signal_handler: &Receiver<InternalCommand>,
    ) {
        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            // receive information for data logger
            match rx_data_logger_from_test_object.try_recv() {
                Ok(_) => { /* do nothing */ }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            // for this mock thread, this case occurrs - test case execution has concluded
                            break;
                        }
                    }
                }
            }
            // receive quit signal
            match rx_data_logger_from_signal_handler.try_recv() {
                Ok(c) => {
                    match c {
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            panic!("Mock DataLogger: Received invalid command.");
                        }
                    };
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            // for this mock thread, this case occurrs - test case execution has concluded
                            break;
                        }
                    }
                }
            }
            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
    }

    // send via channel a value to the test object to indicate if actuation is permitted.
    pub fn mock_schedule_check(
        tx_schedule_check_to_test_object: &Sender<bool>,
        rx_schedule_check_from_test_object: &Receiver<InternalCommand>,
        rx_schedule_check_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        initial_stimuli: bool,
    ) {
        let mut stimuli = initial_stimuli;

        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            match rx_schedule_check_from_test_object.try_recv() {
                Ok(c) => {
                    match c {
                        InternalCommand::ScheduleCheck => {
                            // allow control to actuate
                            let _ = tx_schedule_check_to_test_object.send(stimuli);
                        }
                        InternalCommand::Quit => {
                            // test case has finished
                            break;
                        }
                        _ => {
                            panic!("Mock ScheduleCheck: test object sent invalid command.");
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!(
                                    "Mock ScheduleCheck: Error in trying to receive command from test object: {e:?}"
                                );
                        }
                    }
                }
            }
            match rx_schedule_check_from_test_environment_opt {
                Some(rx_schedule_check_from_test_environment) => {
                    match rx_schedule_check_from_test_environment.try_recv() {
                        Ok(c) => {
                            let test_command = c.clone();
                            match c {
                                TestCommand::UpdateSignal(d) => {
                                    let test_signal = d.clone();
                                    match d {
                                        MockSignal::ScheduleAllows(c) => stimuli = c,
                                        _ => {
                                            panic!("Mock Schedule Check: received test command for invalid signal {}", test_signal);
                                        }
                                    }
                                }
                                _ => {
                                    panic!(
                                        "Mock Schedule Check: received invalid command {}.",
                                        test_command
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!(
                                "Mock Schedule Check: Error in trying to receive command from test environment: {e:?}"
                            );
                                }
                            }
                        }
                    }
                }
                None => { /* do nothing */ }
            };
            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
    }

    pub fn mock_tank_level_switch(
        tx_tank_level_switch_to_test_object: &Sender<bool>,
        rx_tank_level_switch_from_test_object_or_test_environment: &Receiver<InternalCommand>,
        rx_tank_level_switch_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        tx_tank_level_switch_to_data_logger_opt: Option<&Sender<bool>>,
        rx_tank_level_switch_from_data_logger_opt: Option<&Receiver<InternalCommand>>,
        initial_stimuli_position: bool,
        initial_stimuli_invalid: bool,
        initial_stimuli_position_stabilized: bool,
    ) {
        let mut stimuli_position: bool = initial_stimuli_position;
        let mut stimuli_invalid: bool = initial_stimuli_invalid;
        let mut stimuli_position_stabilized: bool = initial_stimuli_position_stabilized;

        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            match rx_tank_level_switch_from_test_object_or_test_environment.try_recv() {
                Ok(c) => {
                    let response_value = match c {
                        InternalCommand::RequestSignal(d) => match d {
                            AquariumSignal::TankLevelSwitchPosition => stimuli_position,
                            AquariumSignal::TankLevelSwitchInvalid => stimuli_invalid,
                            AquariumSignal::TankLevelSwitchPositionStabilized => {
                                stimuli_position_stabilized
                            }
                            _ => {
                                panic!("Mock TankLevelSwitch: inapplicable signal request {}", d);
                            }
                        },
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            panic!("Mock TankLevelSwitch: Received invalid command.");
                        }
                    };
                    match tx_tank_level_switch_to_test_object.send(response_value) {
                        Ok(()) => {}
                        Err(e) => {
                            panic!(
                                "Mock TankLevelSwitch: Error in trying to send response back data logger: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!(
                                "Mock TankLevelSwitch: Error in trying to receive command from test object or test environment: {e:?}"
                            );
                        }
                    }
                }
            }
            
            // communicate with data logger
            match rx_tank_level_switch_from_data_logger_opt {
                Some(rx_tank_level_switch_from_data_logger) => {
                    match rx_tank_level_switch_from_data_logger.try_recv() {
                        Ok(data_logger_request) => {
                            match data_logger_request {
                                InternalCommand::RequestSignal(data_logger_signal_request) => {
                                    let data_logger_signal_response = match data_logger_signal_request {
                                        AquariumSignal::TankLevelSwitchPosition => stimuli_position,
                                        AquariumSignal::TankLevelSwitchInvalid =>  stimuli_invalid,
                                        AquariumSignal::TankLevelSwitchPositionStabilized => stimuli_position_stabilized,
                                        _ => {
                                            panic!("Mock Tank Level Switch: Invalid signal {} requested from DataLogger", data_logger_signal_request);
                                        }
                                    };
                                    match tx_tank_level_switch_to_data_logger_opt {
                                        Some(tx_tank_level_switch_to_data_logger) => {
                                            match tx_tank_level_switch_to_data_logger.send(data_logger_signal_response) {
                                                Ok(_) => { /* do nothing */ }
                                                Err(e) => {
                                                    panic!(
                                                        "Mock Tank Level Switch: Could not send signal request to DataLogger ({e:?})"
                                                    );
                                                }
                                            };
                                        }
                                        None => {
                                            panic!("Mock Tank Level switch invoked without Sender for channel communication to data logger");
                                        }    
                                    };
                                },
                                _ => {
                                    panic!("Mock Tank Level Switch: Received invalid command {} from DataLogger", data_logger_request);
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!(
                                        "Mock Tank Level Switch: Error in trying to receive command from TankLevelSwitch: {e:?}"
                                    );
                                }
                            }
                        }
                    }
                }
                None => {
                    panic!(
                        "Mock Tank Level Switch: invoked with valid sender but without valid receiver for channel communication with tank level switch"
                    );
                }
            }            
            
            match rx_tank_level_switch_from_test_environment_opt {
                Some(rx_tank_level_switch_from_test_environment) => {
                    match rx_tank_level_switch_from_test_environment.try_recv() {
                        Ok(c) => match c {
                            TestCommand::UpdateSignal(d) => {
                                println!("Mock TankLevelSwitch: received {}", d);
                                match d {
                                    MockSignal::TankLevelSwitchPosition(c) => {
                                        stimuli_position = c
                                    }
                                    MockSignal::TankLevelSwitchInvalid(c) => stimuli_invalid = c,
                                    MockSignal::TankLevelSwitchPositionStabilized(c) => {
                                        stimuli_position_stabilized = c
                                    }
                                    _ => {
                                        panic!(
                                    "Mock TankLevelSwitch: Invalid test command {} received.",
                                    d
                                );
                                    }
                                }
                            }
                            _ => {
                                panic!("Mock TankLevelSwitch: received invalid command {}", c);
                            }
                        },
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!(
                                "Mock TankLevelSwitch: Error in trying to receive command from test environment: {e:?}"
                            );
                                }
                            }
                        }
                    }
                }
                None => { /* do nothing */ }
            }
            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
    }

    pub fn mock_atlas_scientific(
        tx_atlas_scientific_to_test_object: &Sender<f32>,
        rx_atlas_scientific_from_test_object: &Receiver<InternalCommand>,
        rx_atlas_scientific_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        initial_stimuli_water_temperature: f32,
        initial_stimuli_conductivity: f32,
        initial_stimuli_ph: f32,
    ) {
        let mut stimuli_conductivity = initial_stimuli_conductivity;
        let mut stimuli_water_temperature = initial_stimuli_water_temperature;
        let mut stimuli_ph = initial_stimuli_ph;

        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            match rx_atlas_scientific_from_test_object.try_recv() {
                Ok(c) => {
                    let response_value = match c {
                        InternalCommand::RequestSignal(d) => match d {
                            AquariumSignal::WaterTemperature => stimuli_water_temperature,
                            AquariumSignal::Conductivity => stimuli_conductivity,
                            AquariumSignal::pH => stimuli_ph,
                            _ => {
                                panic!("Mock AtlasScientific: inapplicable signal request {}", d);
                            }
                        },
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            panic!("Mock AtlasScientific: Received invalid command.");
                        }
                    };
                    match tx_atlas_scientific_to_test_object.send(response_value) {
                        Ok(()) => {}
                        Err(e) => {
                            panic!(
                                "Mock AtlasScientific: Error in trying to send response back data logger: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!(
                                "Mock AtlasScientific: Error in trying to receive command from data logger: {e:?}"
                            );
                        }
                    }
                }
            }
            match rx_atlas_scientific_from_test_environment_opt {
                Some(rx_atlas_scientific_from_test_environment) => {
                    match rx_atlas_scientific_from_test_environment.try_recv() {
                        Ok(c) => {
                            let test_command = c.clone();
                            match c {
                                TestCommand::UpdateSignal(d) => {
                                    let test_signal = d.clone();
                                    match d {
                                        MockSignal::WaterTemperature(c) => {
                                            stimuli_water_temperature = c
                                        }
                                        MockSignal::Conductivity(c) => stimuli_conductivity = c,
                                        MockSignal::pH(c) => stimuli_ph = c,
                                        _ => {
                                            panic!("Mock Atlas Scientific: received test command for invalid signal {}", test_signal);
                                        }
                                    }
                                }
                                _ => {
                                    panic!(
                                        "Mock Atlas Scientific: received invalid command {}.",
                                        test_command
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!(
                                "Mock Atlas Scientific: Error in trying to receive command from test environment: {e:?}"
                            );
                                }
                            }
                        }
                    }
                }
                None => { /* do nothing */ }
            };
            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
    }

    pub fn mock_ambient(
        tx_ambient_to_test_object: &Sender<f32>,
        rx_ambient_from_test_object: &Receiver<InternalCommand>,
        rx_ambient_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        initial_stimuli_ambient_temperature: f32,
        initial_stimuli_ambient_humidity: f32,
    ) {
        let mut stimuli_ambient_temperature = initial_stimuli_ambient_temperature;
        let mut stimuli_ambient_humidity = initial_stimuli_ambient_humidity;

        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            match rx_ambient_from_test_object.try_recv() {
                Ok(c) => {
                    let response_value = match c {
                        InternalCommand::RequestSignal(d) => match d {
                            AquariumSignal::AmbientTemperature => stimuli_ambient_temperature,
                            AquariumSignal::AmbientHumidity => stimuli_ambient_humidity,
                            _ => {
                                panic!("Mock Ambient: inapplicable signal request {}", d);
                            }
                        },
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            panic!("Mock Ambient: Received invalid command.");
                        }
                    };
                    match tx_ambient_to_test_object.send(response_value) {
                        Ok(()) => {}
                        Err(e) => {
                            panic!(
                                "Mock Ambient: Error in trying to send response back data logger: {e:?}"
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!(
                                "Mock Ambient: Error in trying to receive command from data logger: {e:?}"
                            );
                        }
                    }
                }
            }
            match rx_ambient_from_test_environment_opt {
                Some(rx_ambient_from_test_environment) => {
                    match rx_ambient_from_test_environment.try_recv() {
                        Ok(c) => {
                            let test_command = c.clone();
                            match c {
                                TestCommand::UpdateSignal(d) => {
                                    let test_signal = d.clone();
                                    match d {
                                        MockSignal::AmbientTemperature(c) => {
                                            stimuli_ambient_temperature = c
                                        }
                                        MockSignal::AmbientHumidity(c) => {
                                            stimuli_ambient_humidity = c
                                        }
                                        _ => {
                                            panic!("Mock ambient: received test command for invalid signal {}", test_signal);
                                        }
                                    }
                                }
                                _ => {
                                    panic!(
                                        "Mock Ambient: received invalid command {}.",
                                        test_command
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!(
                                "Mock Ambient: Error in trying to receive command from test environment: {e:?}"
                            );
                                }
                            }
                        }
                    }
                }
                None => { /* do nothing */ }
            };
            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
    }

    pub fn mock_tcp(
        tx_tcp_to_test_object: &Sender<f32>,
        rx_tcp_from_test_object: &Receiver<InternalCommand>,
        rx_tcp_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        initial_mock_value_position: bool,
        initial_mock_value_invalid: bool,
    ) {
        let sleep_duration = Duration::from_millis(10);
        let spin_sleeper = SpinSleeper::default();
        let mut mock_value_position = initial_mock_value_position;
        let mut mock_value_invalid = initial_mock_value_invalid;
        loop {
            match rx_tcp_from_test_object.try_recv() {
                Ok(c) => {
                    let response_value: f32 = match c {
                        InternalCommand::RequestSignal(d) => match d {
                            AquariumSignal::TankLevelSwitchPosition => mock_value_position.into(),
                            AquariumSignal::TankLevelSwitchInvalid => mock_value_invalid.into(),
                            _ => {
                                panic!("unknown signal request to mock TCP");
                            }
                        },
                        InternalCommand::Quit => {
                            break;
                        }
                        _ => {
                            panic!("invalid command to mock TCP handler");
                        }
                    };
                    match tx_tcp_to_test_object.send(response_value) {
                        Ok(()) => {}
                        Err(e) => {
                            panic!(
                                "mock_tcp: sending mock data from TCP to test object failed ({e:?})",
                            );
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!("mock_tcp: receiving command from test object/test environment failed ({e:?})",);
                        }
                    }
                }
            };
            match rx_tcp_from_test_environment_opt {
                Some(rx_tcp_from_test_environment) => match rx_tcp_from_test_environment.try_recv()
                {
                    Ok(c) => {
                        match c {
                            TestCommand::UpdateSignal(d) => {
                                println!("Mock TCP: received {}", d);
                                match d {
                                    MockSignal::TankLevelSwitchPosition(c) => {
                                        mock_value_position = c
                                    }
                                    MockSignal::TankLevelSwitchInvalid(c) => mock_value_invalid = c,
                                    MockSignal::TankLevelSwitchPositionStabilized(_) => {
                                        panic!("mock_tcp: received invalid test command {}", d);
                                    }
                                    _ => {
                                        panic!("mock_tcp: Invalid test command {} received.", d);
                                    }
                                }
                            }
                            _ => {
                                panic!("mock_tcp: received invalid command {}", c);
                            }
                        };
                    }
                    Err(e) => {
                        match e {
                            TryRecvError::Empty => { /* empty buffer - no action required */ }
                            TryRecvError::Disconnected => {
                                panic!(
                                "mock_tcp: receiving command from test environment failed ({e:?})",
                            );
                            }
                        }
                    }
                },
                None => { /* do nothing */ }
            }
            spin_sleeper.sleep(sleep_duration);
        }
    }

    pub fn mock_heating(
        tx_heating_to_tank_level_switch_opt: Option<&Sender<InternalCommand>>,
        rx_heating_from_tank_level_switch_opt: Option<&Receiver<bool>>,
        rx_heating_from_signal_handler: &Receiver<InternalCommand>,
        tx_heating_to_data_logger_opt: Option<&Sender<bool>>,
        rx_heating_from_data_logger_opt: Option<&Receiver<InternalCommand>>,
        rx_heating_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        initial_stimuli: bool,
    ) {
        let sleep_duration = Duration::from_millis(10);
        let spin_sleeper = SpinSleeper::default();
        let mut sending_blocked: bool = false;
        let mut stimuli = initial_stimuli;

        loop {
            spin_sleeper.sleep(sleep_duration);
            if !sending_blocked {
                match tx_heating_to_tank_level_switch_opt {
                    Some(tx_heating_to_tank_level_switch) => {
                        match tx_heating_to_tank_level_switch.send(InternalCommand::RequestSignal(
                            AquariumSignal::TankLevelSwitchPosition,
                        )) {
                            Ok(_) => {
                                match rx_heating_from_tank_level_switch_opt {
                                    Some(rx_heating_from_tank_level_switch) => {
                                        match rx_heating_from_tank_level_switch.try_recv() {
                                            Ok(_) => {
                                                // do nothing
                                            }
                                            Err(e) => {
                                                match e {
                                                    TryRecvError::Empty => { /* empty buffer - no action required */
                                                    }
                                                    TryRecvError::Disconnected => {
                                                        panic!(
                                                            "Mock Heating: Error in trying to receive signal from test object: {e:?}"
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    None => {
                                        panic!("Mock Heating: invoked with valid sender but with empty receiver part for channel communication to tank level switch.");
                                    }
                                }
                            }
                            Err(e) => {
                                panic!("Mock Heating: Could not send signal request to test object: {e:?}");
                            }
                        }
                    }
                    None => { /* do nothing */ }
                };
            }
            match rx_heating_from_signal_handler.try_recv() {
                Ok(c) => {
                    match c {
                        InternalCommand::Quit => {
                            sending_blocked = true;
                            println!("Mock Heating: Received Quit signal - blocking sending");
                        }
                        InternalCommand::Terminate => {
                            break;
                        }
                        _ => {
                            panic!("Mock Heating: Received invalid command.");
                        }
                    };
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!(
                                "Mock Heating: Error in trying to receive command from signal handler: {e:?}"
                            );
                        }
                    }
                }
            };

            // communicate with data logger
            match rx_heating_from_data_logger_opt {
                Some(rx_heating_from_data_logger) => {
                    match rx_heating_from_data_logger.try_recv() {
                        Ok(data_logger_request) => {
                            match data_logger_request {
                                InternalCommand::RequestSignal(data_logger_signal_request) => {
                                    match data_logger_signal_request {
                                        AquariumSignal::HeatingControlStatus => {
                                            match tx_heating_to_data_logger_opt {
                                                Some(tx_heating_to_data_logger) => {
                                                    match tx_heating_to_data_logger.send(stimuli) {
                                                        Ok(_) => { /* do nothing */ }
                                                        Err(e) => {
                                                            panic!(
                                                                "Mock Heating: Could not send signal request to DataLogger ({e:?})"
                                                            );
                                                        }
                                                    };
                                                }
                                                None => {
                                                    panic!("Mock Heating invoked without Sender for channel communication to data logger");                                                    
                                                }
                                            };
                                        }
                                        _ => {
                                            panic!("Mock Heating: Invalid signal {} requested from DataLogger", data_logger_signal_request);
                                        }
                                    }
                                },
                                _ => {
                                    panic!("Mock Heating: Received invalid command {} from DataLogger", data_logger_request);
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!(
                                        "Mock Heating: Error in trying to receive command from TankLevelSwitch: {e:?}"
                                    );
                                }
                            }
                        }
                    }
                }
                None => {
                    panic!(
                        "Mock Heating: invoked with valid sender but without valid receiver for channel communication with tank level switch"
                    );
                }
            }

            match rx_heating_from_test_environment_opt {
                Some(rx_heating_from_test_environment) => {
                    match rx_heating_from_test_environment.try_recv() {
                        Ok(c) => {
                            let test_command = c.clone();
                            match c {
                                TestCommand::UpdateSignal(d) => {
                                    let test_signal = d.clone();
                                    match d {
                                        MockSignal::HeatingControlStatus(c) => stimuli = c,
                                        _ => {
                                            panic!("Mock heating: received test command for invalid signal {}", test_signal);
                                        }
                                    }
                                }
                                _ => {
                                    panic!(
                                        "Mock Heating: received invalid command {}.",
                                        test_command
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!(
                                "Mock Heating: Error in trying to receive command from test environment: {e:?}"
                            );
                                }
                            }
                        }
                    }
                }
                None => { /* do nothing */ }
            };

            spin_sleeper.sleep(sleep_duration);
        }
    }

    pub fn mock_ventilation(
        tx_ventilation_to_data_logger: &Sender<bool>,
        rx_ventilation_from_signal_handler: &Receiver<InternalCommand>,
        rx_ventilation_from_test_environment_opt: Option<&Receiver<TestCommand>>,
        initial_stimuli: bool,
    ) {
        let sleep_duration = Duration::from_secs(1);
        let spin_sleeper = SpinSleeper::default();
        let mut sending_blocked: bool = false;
        let mut stimuli = initial_stimuli;

        loop {
            if !sending_blocked {
                match tx_ventilation_to_data_logger.send(stimuli) {
                    Ok(_) => { /* do nothing */ }
                    Err(e) => {
                        panic!(
                            "Mock Ventilation: Could not send signal request to test object: {e:?}"
                        );
                    }
                };
            }
            match rx_ventilation_from_signal_handler.try_recv() {
                Ok(c) => {
                    match c {
                        InternalCommand::Quit => {
                            sending_blocked = true;
                            println!("Mock Ventilation: Received Quit signal - blocking sending");
                        }
                        InternalCommand::Terminate => {
                            break;
                        }
                        _ => {
                            panic!("Mock Ventilation: Received invalid command.");
                        }
                    };
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            panic!(
                                "Mock Ventilation: Error in trying to receive command from signal handler: {e:?}"
                            );
                        }
                    }
                }
            };

            match rx_ventilation_from_test_environment_opt {
                Some(rx_ventilation_from_test_environment) => {
                    match rx_ventilation_from_test_environment.try_recv() {
                        Ok(c) => {
                            let test_command = c.clone();
                            match c {
                                TestCommand::UpdateSignal(d) => {
                                    let test_signal = d.clone();
                                    match d {
                                        MockSignal::VentilationControlStatus(c) => stimuli = c,
                                        _ => {
                                            panic!("Mock ventilation: received test command for invalid signal {}", test_signal);
                                        }
                                    }
                                }
                                _ => {
                                    panic!(
                                        "Mock ventilation: received invalid command {}.",
                                        test_command
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
                                TryRecvError::Disconnected => {
                                    panic!(
                                "Mock ventilation: Error in trying to receive command from test environment: {e:?}"
                            );
                                }
                            }
                        }
                    }
                }
                None => { /* do nothing */ }
            };

            spin_sleeper.sleep(sleep_duration);
        }
    }
}
