/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the hardware actuation logic for a Controllino device via serial communication.

// feature-specific imports
cfg_if::cfg_if! {
    if #[cfg(feature = "controllino_hw")] {
        use spin_sleep::SpinSleeper;
        use std::time::Duration;

        use crate::relays::relay_manager::RelayActuationTrait;
        use crate::relays::actuate_controllino::ActuateControllinoConfig;
    }
}

use aquarium_control::utilities::channel_content::AquariumDevice::{
    AuxPump1, AuxPump2, Feeder, Heater, MainPump1, MainPump2, PeristalticPump1, PeristalticPump2,
    PeristalticPump3, PeristalticPump4, Skimmer, Ventilation,
};

use aquarium_control::relays::actuate_controllino::{
    controllino_constants, create_command_message, ActuateControllino, ActuateControllinoError,
    CONTROLLINO_MAX_RELAY_ID,
};
use aquarium_control::relays::relay_error::RelayError;
use aquarium_control::utilities::channel_content::InternalCommand;
use aquarium_control::utilities::config::{read_config_file, ConfigData};

#[test]
// Verifies that invalid relay id config high.
pub fn test_invalid_relay_id_config_high() {
    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

    config.controllino_relay.main_pump1_id = CONTROLLINO_MAX_RELAY_ID + 1;

    let test_result = ActuateControllino::new(config.controllino_relay);
    assert!(matches!(
        test_result,
        Err(ActuateControllinoError::RelayIdOutsideRange(_, _, _))
    ));
}

#[test]
// Verifies that invalid relay config duplicate.
pub fn test_invalid_relay_config_duplicate() {
    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

    config.controllino_relay.main_pump1_id = CONTROLLINO_MAX_RELAY_ID;
    config.controllino_relay.main_pump2_id = CONTROLLINO_MAX_RELAY_ID;

    let test_result = ActuateControllino::new(config.controllino_relay);
    assert!(matches!(
        test_result,
        Err(ActuateControllinoError::RelayIdDuplicate(_))
    ));
}

/// Retrieves the platform specific controllino configuration.
#[cfg(all(target_os = "macos", feature = "controllino_hw"))]
fn get_platform_specific_controllino_configuration() -> ActuateControllinoConfig {
    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.controllino_relay.active = true;
    config.controllino_relay.port_name = "/dev/cu.usbmodem112401".to_string();
    config.controllino_relay.controllino_processing_millis = 10;
    config.controllino_relay
}

/// Retrieves the platform specific controllino configuration.
#[cfg(all(target_os = "linux", feature = "controllino_hw"))]
fn get_platform_specific_controllino_configuration() -> ActuateControllinoConfig {
    let mut config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    config.controllino_relay.active = true;
    config.controllino_relay
}

#[cfg(feature = "controllino_hw")]
#[test]
// Test case actuates on real hardware.
// Execution of this test case requires serial port connection to the Controllino device.
pub fn test_controllino_switch_on_off_pulse() {
    let sleep_duration_500_millis = Duration::from_millis(500);
    let spin_sleeper = SpinSleeper::default();

    let controllino_config = get_platform_specific_controllino_configuration();

    let mut actuator = ActuateControllino::new(controllino_config).unwrap();

    // Request Controllino to switch on and off all relays sequentially
    actuator
        .actuate(&InternalCommand::SwitchOn(Skimmer))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOn(Ventilation))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOn(Heater))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOn(MainPump1))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOn(MainPump2))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOn(AuxPump1))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOn(AuxPump2))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOn(PeristalticPump1))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOn(PeristalticPump2))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOn(PeristalticPump3))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOn(PeristalticPump4))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOn(Feeder))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOff(Skimmer))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOff(Ventilation))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOff(Heater))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOff(MainPump1))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOff(MainPump2))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOff(AuxPump1))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOff(AuxPump2))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOff(PeristalticPump1))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOff(PeristalticPump2))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOff(PeristalticPump3))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOff(PeristalticPump4))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::SwitchOff(Feeder))
        .unwrap();

    spin_sleeper.sleep(sleep_duration_500_millis);

    // Request Controllino to pulse all relays for peristaltic pumps
    actuator
        .actuate(&InternalCommand::Pulse(PeristalticPump1, 500))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::Pulse(PeristalticPump2, 500))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::Pulse(PeristalticPump3, 500))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);

    actuator
        .actuate(&InternalCommand::Pulse(PeristalticPump4, 500))
        .unwrap();
}

#[test]
/// Test case tests if an invalid command is rejected when preparing the message for Controllino.
pub fn test_create_message_invalid_command() {
    let config: ConfigData =
        read_config_file("config/aquarium_control_test_simulator.toml".to_string()).unwrap();
    let test_result =
        create_command_message(InternalCommand::ResetAllErrors, &config.controllino_relay);

    assert!(matches!(
        test_result,
        Err(RelayError::IrrelevantCommand(
            _,
            InternalCommand::ResetAllErrors
        )),
    ));
}

#[test]
// Test case tests the implementation of the relay logic.
// During the creation of the message to Controllino, the application considers
// if relay needs to bet set or unset to switch on/off a device.
pub fn test_controllino_create_message_switch_on_with_set_relay_switch_off_with_unset_relay() {
    let config: ConfigData =
        read_config_file("config/aquarium_control_test_simulator.toml".to_string()).unwrap();

    // switching on all devices
    let mut message = create_command_message(
        InternalCommand::SwitchOn(Skimmer),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_SET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.skimmer_id);

    message = create_command_message(
        InternalCommand::SwitchOn(Ventilation),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_SET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.ventilation_id);

    message = create_command_message(InternalCommand::SwitchOn(Heater), &config.controllino_relay)
        .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_SET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.heater_id);

    message = create_command_message(
        InternalCommand::SwitchOn(MainPump1),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_SET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.main_pump1_id);

    message = create_command_message(
        InternalCommand::SwitchOn(MainPump2),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_SET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.main_pump2_id);

    message = create_command_message(
        InternalCommand::SwitchOn(AuxPump1),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_SET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.aux_pump1_id);

    message = create_command_message(
        InternalCommand::SwitchOn(AuxPump2),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_SET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.aux_pump2_id);

    message = create_command_message(
        InternalCommand::SwitchOn(PeristalticPump1),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_SET_RELAY as u8
    );
    assert_eq!(
        message.data[1],
        config.controllino_relay.peristaltic_pump1_id
    );

    message = create_command_message(
        InternalCommand::SwitchOn(PeristalticPump2),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_SET_RELAY as u8
    );
    assert_eq!(
        message.data[1],
        config.controllino_relay.peristaltic_pump2_id
    );

    message = create_command_message(
        InternalCommand::SwitchOn(PeristalticPump3),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_SET_RELAY as u8
    );
    assert_eq!(
        message.data[1],
        config.controllino_relay.peristaltic_pump3_id
    );

    message = create_command_message(
        InternalCommand::SwitchOn(PeristalticPump4),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_SET_RELAY as u8
    );
    assert_eq!(
        message.data[1],
        config.controllino_relay.peristaltic_pump4_id
    );

    message = create_command_message(InternalCommand::SwitchOn(Feeder), &config.controllino_relay)
        .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_SET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.feeder_id);

    // switching off all devices
    let message = create_command_message(
        InternalCommand::SwitchOff(Skimmer),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_UNSET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.skimmer_id);

    let mut message = create_command_message(
        InternalCommand::SwitchOff(Ventilation),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_UNSET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.ventilation_id);

    message = create_command_message(
        InternalCommand::SwitchOff(Heater),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_UNSET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.heater_id);

    message = create_command_message(
        InternalCommand::SwitchOff(MainPump1),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_UNSET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.main_pump1_id);

    message = create_command_message(
        InternalCommand::SwitchOff(MainPump2),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_UNSET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.main_pump2_id);

    message = create_command_message(
        InternalCommand::SwitchOff(AuxPump1),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_UNSET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.aux_pump1_id);

    message = create_command_message(
        InternalCommand::SwitchOff(AuxPump2),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_UNSET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.aux_pump2_id);

    message = create_command_message(
        InternalCommand::SwitchOff(PeristalticPump1),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_UNSET_RELAY as u8
    );
    assert_eq!(
        message.data[1],
        config.controllino_relay.peristaltic_pump1_id
    );

    message = create_command_message(
        InternalCommand::SwitchOff(PeristalticPump2),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_UNSET_RELAY as u8
    );
    assert_eq!(
        message.data[1],
        config.controllino_relay.peristaltic_pump2_id
    );

    message = create_command_message(
        InternalCommand::SwitchOff(PeristalticPump3),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_UNSET_RELAY as u8
    );
    assert_eq!(
        message.data[1],
        config.controllino_relay.peristaltic_pump3_id
    );

    message = create_command_message(
        InternalCommand::SwitchOff(PeristalticPump4),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_UNSET_RELAY as u8
    );
    assert_eq!(
        message.data[1],
        config.controllino_relay.peristaltic_pump4_id
    );

    message = create_command_message(
        InternalCommand::SwitchOff(Feeder),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_UNSET_RELAY as u8
    );
    assert_eq!(message.data[1], config.controllino_relay.feeder_id);
}

#[test]
// Tests the message creation for pulse command.
// Pulse command is only permitted for a subset of the devices (e.g., peristaltic pumps).
pub fn test_create_message_pulse() {
    let config: ConfigData =
        read_config_file("config/aquarium_control_test_simulator.toml".to_string()).unwrap();

    let mut error_message = create_command_message(
        InternalCommand::Pulse(Skimmer, 1000),
        &config.controllino_relay,
    );
    assert!(matches!(
        error_message,
        Err(RelayError::PulseNotAllowedForDevice(_, Skimmer))
    ));

    error_message = create_command_message(
        InternalCommand::Pulse(Ventilation, 1000),
        &config.controllino_relay,
    );
    assert!(matches!(
        error_message,
        Err(RelayError::PulseNotAllowedForDevice(_, Ventilation))
    ));

    error_message = create_command_message(
        InternalCommand::Pulse(Heater, 1000),
        &config.controllino_relay,
    );
    assert!(matches!(
        error_message,
        Err(RelayError::PulseNotAllowedForDevice(_, Heater))
    ));

    error_message = create_command_message(
        InternalCommand::Pulse(MainPump1, 1000),
        &config.controllino_relay,
    );
    assert!(matches!(
        error_message,
        Err(RelayError::PulseNotAllowedForDevice(_, MainPump1))
    ));

    error_message = create_command_message(
        InternalCommand::Pulse(MainPump2, 1000),
        &config.controllino_relay,
    );
    assert!(matches!(
        error_message,
        Err(RelayError::PulseNotAllowedForDevice(_, MainPump2))
    ));

    error_message = create_command_message(
        InternalCommand::Pulse(AuxPump1, 1000),
        &config.controllino_relay,
    );
    assert!(matches!(
        error_message,
        Err(RelayError::PulseNotAllowedForDevice(_, AuxPump1))
    ));

    error_message = create_command_message(
        InternalCommand::Pulse(AuxPump2, 1000),
        &config.controllino_relay,
    );
    assert!(matches!(
        error_message,
        Err(RelayError::PulseNotAllowedForDevice(_, AuxPump2))
    ));

    let mut message = create_command_message(
        InternalCommand::Pulse(PeristalticPump1, 1000),
        &config.controllino_relay,
    )
    .unwrap();

    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_PULSE_RELAY as u8
    );
    assert_eq!(
        message.data[1],
        config.controllino_relay.peristaltic_pump1_id
    );

    message = create_command_message(
        InternalCommand::Pulse(PeristalticPump2, 1000),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_PULSE_RELAY as u8
    );
    assert_eq!(
        message.data[1],
        config.controllino_relay.peristaltic_pump2_id
    );

    message = create_command_message(
        InternalCommand::Pulse(PeristalticPump3, 1000),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_PULSE_RELAY as u8
    );
    assert_eq!(
        message.data[1],
        config.controllino_relay.peristaltic_pump3_id
    );

    message = create_command_message(
        InternalCommand::Pulse(PeristalticPump4, 1000),
        &config.controllino_relay,
    )
    .unwrap();
    assert_eq!(
        message.data[0],
        controllino_constants::COMMAND_PULSE_RELAY as u8
    );
    assert_eq!(
        message.data[1],
        config.controllino_relay.peristaltic_pump4_id
    );

    error_message = create_command_message(
        InternalCommand::Pulse(Feeder, 1000),
        &config.controllino_relay,
    );
    assert!(matches!(
        error_message,
        Err(RelayError::PulseNotAllowedForDevice(_, Feeder))
    ));
}

#[cfg(feature = "controllino_hw")]
#[test]
// Test case actuates on real hardware.
// Execution of this test case requires serial port connection to the Controllino device.
pub fn test_controllino_simulate_feed_profile() {
    let sleep_duration_500_millis = Duration::from_millis(500);
    let sleep_duration_8_secs = Duration::from_secs(8);
    let sleep_duration_6_secs = Duration::from_secs(6);
    let spin_sleeper = SpinSleeper::default();

    let controllino_config = get_platform_specific_controllino_configuration();

    let mut actuator = ActuateControllino::new(controllino_config).unwrap();

    // Request Controllino to switch on and off all relays sequentially
    actuator
        .actuate(&InternalCommand::SwitchOff(MainPump1))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);
    actuator
        .actuate(&InternalCommand::SwitchOff(MainPump2))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);
    actuator
        .actuate(&InternalCommand::SwitchOff(AuxPump1))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_8_secs);
    actuator
        .actuate(&InternalCommand::SwitchOn(Feeder))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_6_secs);
    actuator
        .actuate(&InternalCommand::SwitchOff(Feeder))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_8_secs);
    actuator
        .actuate(&InternalCommand::SwitchOn(Feeder))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_6_secs);
    actuator
        .actuate(&InternalCommand::SwitchOff(Feeder))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_8_secs);
    actuator
        .actuate(&InternalCommand::SwitchOn(MainPump1))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);
    actuator
        .actuate(&InternalCommand::SwitchOn(MainPump2))
        .unwrap();
    spin_sleeper.sleep(sleep_duration_500_millis);
    actuator
        .actuate(&InternalCommand::SwitchOn(AuxPump1))
        .unwrap();
}
