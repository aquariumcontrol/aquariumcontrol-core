/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(test)]
pub mod tests {
    use chrono::Local;
    use chrono::{DateTime, Utc};
    use spin_sleep::SpinSleeper;
    use std::fmt;
    use std::sync::mpsc;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError};
    use std::time::{Duration, SystemTime};

    use crate::channel_content::AquariumDevice;
    use crate::channel_content::AquariumSignal;
    use crate::channel_content::InternalCommand;

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
        loop {
            spin_sleeper.sleep(sleep_duration);
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
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
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
                    panic!("Mock Refill: Could not send signal request to TankLevelSwitch ({e:?})")
                }
            };
            match rx_refill_from_signal_handler.try_recv() {
                Ok(c) => {
                    match c {
                        InternalCommand::Quit => {
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

    pub fn mock_data_logger(rx_data_logger_from_heating: &Receiver<bool>) {
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
        mock_value_position: bool,
        mock_value_invalid: bool,
        mock_value_position_stabilized: bool,
    ) {
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
                                "Mock TankLevelSwitch: Error in trying to receive command from data logger: {e:?}"
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
        mock_value_position: f32,
        mock_value_invalid: f32,
    ) {
        let sleep_duration = Duration::from_millis(10);
        let spin_sleeper = SpinSleeper::default();
        loop {
            match rx_tcp_from_test_object_or_test_environment.try_recv() {
                Ok(c) => {
                    let response_value: f32 = match c {
                        InternalCommand::RequestSignal(d) => match d {
                            AquariumSignal::TankLevelSwitchPosition => mock_value_position.into(),
                            AquariumSignal::TankLevelSwitchInvalid => true.into(),
                            _ => {
                                panic!("unknown signal request to mock GPIO");
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
                            panic!("mock_tcp: receiving command from test object failed ({e:?})",);
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
        loop {
            spin_sleeper.sleep(sleep_duration);
            match tx_heating_to_test_object.send(InternalCommand::RequestSignal(
                AquariumSignal::TankLevelSwitchPosition,
            )) {
                Ok(c) => {
                    match rx_heating_from_test_object.try_recv() {
                        Ok(d) => {
                            // do nothing
                        }
                        Err(e) => {
                            match e {
                                TryRecvError::Empty => { /* empty buffer - no action required */ }
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
                    panic!("Mock Heating: Could not send signal request to test object");
                }
            };
            match rx_heating_from_signal_handler.try_recv() {
                Ok(c) => {
                    let response_value = match c {
                        InternalCommand::Quit => {
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
