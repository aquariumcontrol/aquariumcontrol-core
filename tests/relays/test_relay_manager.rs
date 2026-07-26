/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the central hardware actuation dispatcher thread.

use aquarium_control::launch::channels::Channels;
use aquarium_control::launch::execution_config::ExecutionConfig;
use aquarium_control::relays::relay_manager::{RelayActuationTrait, RelayError, RelayManager};
use aquarium_control::relays::relay_manager_channels::RelayManagerChannels;
use aquarium_control::utilities::channel_content::{AquariumDevice, InternalCommand};
use aquarium_control::utilities::config::{read_config_file, ConfigData};
use spin_sleep::SpinSleeper;
use std::thread::scope;
use std::time::Duration;

/// Helper function to execute the relay manager task inside a test Tokio runtime.
///
/// # Arguments
/// * `relay_manager` - A mutable reference to the `RelayManager` instance.
/// * `relay_manager_channels` - A mutable reference to the `RelayManagerChannels` instance.
/// * `actuator` - A mutable reference to the relay actuator instance.
fn execute_blocking(
    relay_manager: &mut RelayManager,
    relay_manager_channels: &mut RelayManagerChannels,
    actuator: &mut impl RelayActuationTrait,
) {
    tokio::runtime::Builder::new_multi_thread().enable_all()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            // blocking execution used for testing
            relay_manager
                .execute(relay_manager_channels, actuator)
                .await;
        });
}

// Contains mock trait implementation for unit testing purposes.
pub struct ActuateVoid {
    // execution counter for testing purposes
    execution_counter: u64,

    // heartbeat counter for testing purposes
    heartbeat_counter: u64,

    // flush counter for testing purposes
    flush_counter: u64,

    // injection of error: if not None, this value is used one time by actuate and then set to None
    onetime_error_result: Option<RelayError>,

    // injection of error: if not None, this value is used one time by actuate and then set to None
    permanent_error_result: Option<RelayError>,
}

impl RelayActuationTrait for ActuateVoid {
    // Mock function for testing
    /// Performs the `actuate` operation.
    ///
    /// # Arguments
    /// * `internal_command` - A parameter representing `internal_command`.
    fn actuate(&mut self, internal_command: &InternalCommand) -> Result<(), RelayError> {
        println!(
            "Received {} - increasing execution counter (currently at {})",
            internal_command, self.execution_counter
        );
        self.execution_counter += 1;
        if self.onetime_error_result.is_some() {
            // call will replace error value with "None" - inducing Ok-result for next call
            // of actuate
            Err(self.onetime_error_result.take().unwrap())
        } else {
            if self.permanent_error_result.is_some() {
                // Workaround: RelayError does not implement the Clone trait.
                // WriteError is the only error variant used by the test cases.
                Err(RelayError::WriteError(module_path!().to_string()))
            } else {
                Ok(())
            }
        }
    }

    /// Retrieves the heartbeat interval seconds.
    fn get_heartbeat_interval_seconds(&self) -> Option<u64> {
        Some(1)
    }

    /// Performs the `heartbeat` operation.
    fn heartbeat(&mut self) -> Result<(), RelayError> {
        self.heartbeat_counter += 1;
        Ok(())
    }

    /// Performs the `flush_buffer` operation.
    fn flush_buffer(&mut self) -> Result<(), RelayError> {
        self.flush_counter += 1;
        Ok(())
    }
}

impl ActuateVoid {
    // provides the number of executions to the test case for assertion
    /// Retrieves the execution count.
    pub fn get_execution_count(&self) -> u64 {
        self.execution_counter
    }
}

#[test]
// Test case tests the channel communication for the .execute function of relay manager.
// It also checks the heartbeat communication.
/// [itest->swreq~relay_control_actuators~1]
pub fn test_relay_manager_channel_communication() {
    let config: ConfigData =
        read_config_file("config/aquarium_control_test_simulator.toml".to_string()).unwrap();

    let spin_sleeper = SpinSleeper::default();
    let sleep_duration = Duration::from_secs(2);

    let mut relay_manager = RelayManager::new(config.relay_manager, ExecutionConfig::default());

    let mut channels = Channels::new_for_test();

    scope(|scope| {
        // thread for test environment
        scope.spawn(move || {
            let _ = channels.refill.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::RefillPump));
            match channels.refill.rx_refill_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for refill thread: {e:?}");
                }
            }
            let _ = channels.heating.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::Heater));
            match channels.heating.rx_heating_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for heating thread: {e:?}");
                }
            }
            let _ = channels.ventilation.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::Ventilation));
            match channels.ventilation.rx_ventilation_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for ventilation thread: {e:?}");
                }
            }
            let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump1));
            match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                }
            }
            let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump2));
            match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                }
            }
            let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump3));
            match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                }
            }
            let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::PeristalticPump4));
            match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                }
            }
            let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::Feeder));
            match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                }
            }
            let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::MainPump1));
            match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                }
            }
            let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::MainPump2));
            match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                }
            }
            let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::AuxPump1));
            match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                }
            }
            let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::AuxPump2));
            match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                }
            }
            let _ = channels.refill.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::RefillPump));
            match channels.refill.rx_refill_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for refill thread: {e:?}");
                }
            }
            let _ = channels.heating.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::Heater));
            match channels.heating.rx_heating_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for heating thread: {e:?}");
                }
            }
            let _ = channels.ventilation.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::Ventilation));
            match channels.ventilation.rx_ventilation_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for ventilation thread: {e:?}");
                }
            }
            let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump1));
            match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                }
            }
            let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump2));
            match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                }
            }
            let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump3));
            match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                }
            }
            let _ = channels.balling.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::PeristalticPump4));
            match channels.balling.rx_balling_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Balling dosing thread: {e:?}");
                }
            }
            let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::Feeder));
            match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                }
            }
            let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::MainPump1));
            match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                }
            }
            let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::MainPump2));
            match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                }
            }
            let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::AuxPump1));
            match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                }
            }
            let _ = channels.feed.send_to_relay_manager(InternalCommand::SwitchOff(AquariumDevice::AuxPump2));
            match channels.feed.rx_feed_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for Feed thread: {e:?}");
                }
            }

            spin_sleeper.sleep(sleep_duration); // wait for two seconds to test the heartbeat

            let _ = channels.signal_handler.tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
            channels.signal_handler.rx_signal_handler_from_relay_manager.blocking_recv().unwrap();
        });

        // thread for the test object
        scope.spawn(move || {
            let mut mock_actuator = ActuateVoid {
                execution_counter: 0,
                heartbeat_counter: 0,
                flush_counter: 0,
                onetime_error_result: None,
                permanent_error_result: None,
            };

            execute_blocking(
                &mut relay_manager,
                &mut channels.relay_manager,
                &mut mock_actuator,
            );

            // Test object thread has ended.
            assert_eq!(mock_actuator.get_execution_count(), 24); //Assertion checks the number of calls to the actuator.
            assert_eq!(mock_actuator.heartbeat_counter, 2); // Assertion checks the number of heartbeats.
        });
    });
}
#[test]
// Test case tests if the relay manager is calling the flush functionality when the
// actuator returns the IncorrectChecksum error.
pub fn test_relay_manager_flush() {
    let config: ConfigData =
        read_config_file("config/aquarium_control_test_simulator.toml".to_string()).unwrap();

    let mut relay_manager = RelayManager::new(config.relay_manager, ExecutionConfig::default());

    let mut channels = Channels::new_for_test();

    scope(|scope| {
        // thread for test environment
        scope.spawn(move || {
            let _ = channels.refill.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::RefillPump));
            match channels.refill.rx_refill_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for refill thread: {e:?}");
                }
            }
            let _ = channels.heating.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::Heater));
            match channels.heating.rx_heating_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for heating thread: {e:?}");
                }
            }

            let _ = channels.signal_handler.tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
            channels.signal_handler.rx_signal_handler_from_relay_manager.blocking_recv().unwrap();
        });

        // thread for the test object
        scope.spawn(move || {
            let mut mock_actuator = ActuateVoid {
                execution_counter: 0,
                heartbeat_counter: 0,
                flush_counter: 0,
                onetime_error_result: Some(RelayError::IncorrectChecksum(
                    module_path!().to_string(),
                )),
                permanent_error_result: None,
            };

            execute_blocking(
                &mut relay_manager,
                &mut channels.relay_manager,
                &mut mock_actuator,
            );

            // Test object thread has ended.
            assert_eq!(mock_actuator.get_execution_count(), 3); //Assertion checks the number of calls to the actuator.
            assert_eq!(mock_actuator.flush_counter, 1); // Assertion checks the number of buffer flush operations.
        });
    });
}

#[test]
// Test case tests if the relay manager is calling the actuate function multiple times
// in case actuator returns an error.
pub fn test_relay_manager_repeat() {
    let config: ConfigData =
        read_config_file("config/aquarium_control_test_simulator.toml".to_string()).unwrap();

    let target_repeat_times = config.relay_manager.actuation_retries;

    let mut relay_manager = RelayManager::new(config.relay_manager, ExecutionConfig::default());

    let mut channels = Channels::new_for_test();

    scope(|scope| {
        // thread for test environment
        scope.spawn(move || {
            let _ = channels.refill.send_to_relay_manager(InternalCommand::SwitchOn(AquariumDevice::RefillPump));
            match channels.refill.rx_refill_from_relay_manager.blocking_recv() {
                Ok(c) => {
                    assert!(c);
                },
                Err(e) => {
                    panic!("test_relay_manager_channel_communication: error when receiving answer for refill thread: {e:?}");
                }
            }

            let _ = channels.signal_handler.tx_signal_handler_to_relay_manager.send(InternalCommand::Quit);
            channels.signal_handler.rx_signal_handler_from_relay_manager.blocking_recv().unwrap();
        });

        // thread for the test object
        scope.spawn(move || {
            let mut mock_actuator = ActuateVoid {
                execution_counter: 0,
                heartbeat_counter: 0,
                flush_counter: 0,
                onetime_error_result: None,
                permanent_error_result: Some(RelayError::WriteError(module_path!().to_string())),
            };

            execute_blocking(
                &mut relay_manager,
                &mut channels.relay_manager,
                &mut mock_actuator,
            );

            // Test object thread has ended.

            // Assertion checks the number of calls to the actuator.
            // Relay manager shall repeat in case of failure as configured by user.
            assert_eq!(mock_actuator.get_execution_count(), target_repeat_times + 1);
        });
    });
}
