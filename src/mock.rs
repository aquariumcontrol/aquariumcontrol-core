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
    use crate::refill::RefillErrors;
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

    pub fn mock_controllino(
        rx_controllino_from_test_object_or_test_environment: &Receiver<InternalCommand>,
        tx_controllino_to_test_object: &Sender<bool>,
    ) -> Vec<ActuationEvent> {
        let mut result: Vec<ActuationEvent> = Vec::new();

        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            // receive command from test object
            match rx_controllino_from_test_object_or_test_environment.recv() {
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
        tx_refill_to_tank_level_switch: &Sender<InternalCommand>,
        rx_refill_from_tank_level_switch: &Receiver<bool>,
        rx_refill_from_signal_handler: &Receiver<InternalCommand>,
    ) -> Vec<bool> {
        let mut result: Vec<bool> = Vec::new();

        let sleep_duration = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        let mut sending_blocked: bool = false;
        loop {
            if !sending_blocked {
                match tx_refill_to_tank_level_switch.send(InternalCommand::RequestSignal(
                    AquariumSignal::TankLevelSwitchPositionStabilized,
                )) {
                    Ok(_) => {
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
                    Err(e) => {
                        panic!(
                            "Mock Refill: Could not send signal request to TankLevelSwitch ({e:?})"
                        )
                    }
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
            spin_sleeper.sleep(sleep_duration);
        }
        result
    }

    pub fn mock_data_logger(
        rx_data_logger_from_heating: &Receiver<bool>,
        rx_data_logger_from_signal_handler: &Receiver<InternalCommand>,
    ) {
        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            // receive information for data logger
            match rx_data_logger_from_heating.try_recv() {
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
        tx_schedule_check_to_heating: &Sender<bool>,
        rx_schedule_check_from_heating_or_test_environment: &Receiver<InternalCommand>,
        value: bool,
    ) {
        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            match rx_schedule_check_from_heating_or_test_environment.try_recv() {
                Ok(c) => {
                    match c {
                        InternalCommand::ScheduleCheck => {
                            // allow control to actuate
                            let _ = tx_schedule_check_to_heating.send(value);
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
            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
    }

    pub fn mock_tank_level_switch(
        tx_tank_level_switch_to_test_object: &Sender<bool>,
        rx_tank_level_switch_from_test_object_or_test_environment: &Receiver<InternalCommand>,
        rx_tank_level_switch_from_test_environment: &Receiver<TestCommand>,
        initial_value_position: bool,
        initial_value_invalid: bool,
        initial_value_position_stabilized: bool,
    ) {
        let mut mock_value_position: bool = initial_value_position;
        let mut mock_value_invalid: bool = initial_value_invalid;
        let mut mock_value_position_stabilized: bool = initial_value_position_stabilized;

        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            match rx_tank_level_switch_from_test_object_or_test_environment.try_recv() {
                Ok(c) => {
                    let response_value = match c {
                        InternalCommand::RequestSignal(d) => match d {
                            AquariumSignal::TankLevelSwitchPosition => mock_value_position,
                            AquariumSignal::TankLevelSwitchInvalid => mock_value_invalid,
                            AquariumSignal::TankLevelSwitchPositionStabilized => {
                                mock_value_position_stabilized
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
            match rx_tank_level_switch_from_test_environment.try_recv() {
                Ok(c) => match c {
                    TestCommand::UpdateSignal(d) => {
                        println!("Mock TankLevelSwitch: received {}", d);
                        match d {
                            MockSignal::TankLevelSwitchPosition(c) => mock_value_position = c,
                            MockSignal::TankLevelSwitchInvalid(c) => mock_value_invalid = c,
                            MockSignal::TankLevelSwitchPositionStabilized(c) => {
                                mock_value_position_stabilized = c
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
            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
    }

    pub fn mock_atlas_scientific(
        tx_atlas_scientific_to_test_object: &Sender<f32>,
        rx_atlas_scientific_from_test_object_or_test_environment: &Receiver<InternalCommand>,
        mock_value_water_temperature: f32,
        mock_value_conductivity: f32,
        mock_value_ph: f32,
    ) {
        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            match rx_atlas_scientific_from_test_object_or_test_environment.try_recv() {
                Ok(c) => {
                    let response_value = match c {
                        InternalCommand::RequestSignal(d) => match d {
                            AquariumSignal::WaterTemperature => mock_value_water_temperature,
                            AquariumSignal::Conductivity => mock_value_conductivity,
                            AquariumSignal::pH => mock_value_ph,
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
            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
    }

    pub fn mock_ambient(
        tx_ambient_to_test_object: &Sender<f32>,
        rx_ambient_from_test_object_or_test_environment: &Receiver<InternalCommand>,
        mock_value_ambient_temperature: f32,
        mock_value_ambient_humidity: f32,
    ) {
        let sleep_duration_one_millisecond = Duration::from_millis(1);
        let spin_sleeper = SpinSleeper::default();
        loop {
            match rx_ambient_from_test_object_or_test_environment.try_recv() {
                Ok(c) => {
                    let response_value = match c {
                        InternalCommand::RequestSignal(d) => match d {
                            AquariumSignal::AmbientTemperature => mock_value_ambient_temperature,
                            AquariumSignal::Conductivity => mock_value_ambient_humidity,
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
            spin_sleeper.sleep(sleep_duration_one_millisecond);
        }
    }

    pub fn mock_tcp(
        tx_tcp_to_test_object: &Sender<f32>,
        rx_tcp_from_test_object_or_test_environment: &Receiver<InternalCommand>,
        initial_mock_value_position: bool,
        initial_mock_value_invalid: bool,
        rx_tcp_from_test_environment: &Receiver<TestCommand>,
    ) {
        let sleep_duration = Duration::from_millis(10);
        let spin_sleeper = SpinSleeper::default();
        let mut mock_value_position = initial_mock_value_position;
        let mut mock_value_invalid = initial_mock_value_invalid;
        loop {
            match rx_tcp_from_test_object_or_test_environment.try_recv() {
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
            match rx_tcp_from_test_environment.try_recv() {
                Ok(c) => {
                    match c {
                        TestCommand::UpdateSignal(d) => {
                            println!("Mock TCP: received {}", d);
                            match d {
                                MockSignal::TankLevelSwitchPosition(c) => mock_value_position = c,
                                MockSignal::TankLevelSwitchInvalid(c) => mock_value_invalid = c,
                                MockSignal::TankLevelSwitchPositionStabilized(_) => {
                                    panic!("mock_tcp: received invalid test command {}", d);
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
            };
            spin_sleeper.sleep(sleep_duration);
        }
    }

    pub fn mock_heating(
        tx_heating_to_test_object: &Sender<InternalCommand>,
        rx_heating_from_test_object: &Receiver<bool>,
        rx_heating_from_signal_handler: &Receiver<InternalCommand>,
    ) {
        let sleep_duration = Duration::from_millis(10);
        let spin_sleeper = SpinSleeper::default();
        let mut sending_blocked: bool = false;
        loop {
            spin_sleeper.sleep(sleep_duration);
            if !sending_blocked {
                match tx_heating_to_test_object.send(InternalCommand::RequestSignal(
                    AquariumSignal::TankLevelSwitchPosition,
                )) {
                    Ok(_) => {
                        match rx_heating_from_test_object.try_recv() {
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
                    Err(e) => {
                        panic!("Mock Heating: Could not send signal request to test object: {e:?}");
                    }
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
            spin_sleeper.sleep(sleep_duration);
        }
    }
}
