/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Tests the driver for Atlas Scientific EZO sensors.

use aquarium_control::launch::channels::{channel, AquaReceiver, AquaSender, Channels};
use aquarium_control::sensors::atlas_scientific::{
    atlas_scientific_constants, AtlasScientificResultData,
};
use aquarium_control::sensors::atlas_scientific_channels::AtlasScientificChannels;
use aquarium_control::sensors::atlas_scientific_driver::AtlasScientificDriver;
use aquarium_control::sensors::atlas_scientific_error::AtlasScientificError;
use aquarium_control::sensors::atlas_scientific_ezo::{
    atlas_scientific_ezo_constants, AtlasScientificEzo,
};
use aquarium_control::sensors::atlas_scientific_monitor_view::AtlasScientificMonitorView;
use aquarium_control::sensors::i2c_interface::{I2cRequest, I2cResponse, I2cResult};
use aquarium_control::utilities::channel_content::{AquariumSignal, InternalCommand};
use aquarium_control::utilities::config::{read_config_file, ConfigData};
use assert_float_eq::assert_float_absolute_eq;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

cfg_if::cfg_if! {
    if #[cfg(feature = "test-utils")] {
        use aquarium_control::sensors::i2c_error::I2cError;
        use std::thread;
        use std::time::Duration;
        use spin_sleep::SpinSleeper;
    }
}

/// Helper function to run a successful receive-and-parse test for a given signal.
///
/// # Arguments
/// * `value` - The value to set.
fn run_receive_success_test(signal: AquariumSignal, value: f32) {
    let (
        mut drivers,
        mut atlas_scientific_channels,
        mut tx_i2c_interface_to_atlas_scientific,
        mutex_temp,
        mutex_ph,
        mutex_cond,
    ) = setup_receive_test();

    // Create a valid I2C response buffer.
    let mut value_str = value.to_string();
    if value_str.len() > 5 {
        value_str.truncate(5);
    }
    while value_str.len() < 6 {
        value_str.push('0');
    }

    let mut buffer = [0u8; 8];
    buffer[0] = atlas_scientific_ezo_constants::MESSAGE_START_MAGIC;
    buffer[1..7].copy_from_slice(value_str.as_bytes());
    buffer[7] = atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC;

    let i2c_response = I2cResponse::new(&buffer);

    // Send the valid response
    tx_i2c_interface_to_atlas_scientific
        .send(Ok(i2c_response))
        .unwrap();

    let driver = drivers.get_mut(&signal).unwrap();

    // Execute the function under test
    let result = driver.receive_response_from_i2c_interface(
        &signal,
        &mut atlas_scientific_channels,
        &mutex_temp,
        &mutex_ph,
        &mutex_cond,
    );

    // Assert the outcome
    assert!(
        result.is_ok(),
        "Test failed for signal {:?} with value {}. Error: {:?}",
        signal,
        value,
        result.err()
    );
    assert_float_absolute_eq!(result.unwrap().value, value, 0.001);

    // Check that the correct mutex was updated and others were not
    let (target_mutex, other1, other2) = match signal {
        AquariumSignal::WaterTemperature => (&mutex_temp, &mutex_ph, &mutex_cond),
        AquariumSignal::pH => (&mutex_ph, &mutex_temp, &mutex_cond),
        AquariumSignal::Conductivity => (&mutex_cond, &mutex_temp, &mutex_ph),
        _ => panic!("Invalid signal for this test"),
    };

    let lock = target_mutex.lock().unwrap();
    assert_float_absolute_eq!(lock.value, value, 0.001);

    let lock1 = other1.lock().unwrap();
    assert_float_absolute_eq!(lock1.value, 0.0, 0.001);
    let lock2 = other2.lock().unwrap();
    assert_float_absolute_eq!(lock2.value, 0.0, 0.001);
}

#[test]
// Test function receive_response_from_i2c_interface
// [itest->swreq~sensors_read_data~1]
pub fn test_receive_response_from_i2c_interface() {
    let mut stimuli_array_temperature = [0u8; 64];

    let message_temperature: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
        atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
        b'2',
        b'3',
        atlas_scientific_ezo_constants::DECIMAL_POINT,
        b'1',
        b'0',
        b'0',
        atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
    ];
    stimuli_array_temperature[..message_temperature.len()].copy_from_slice(&message_temperature);
    let stimuli_i2c_response_temperature = I2cResponse {
        read_buf: stimuli_array_temperature,
        length: atlas_scientific_ezo_constants::MESSAGE_SIZE,
    };

    let mut stimuli_array_conductivity = [0u8; 64];
    let message_conductivity: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
        atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
        b'4',
        b'8',
        b'0',
        b'0',
        b'0',
        b'0',
        atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
    ];
    // Copy the 8-byte message into the beginning of the 64-byte buffer.
    // The rest of the buffer will remain filled with zeros.
    stimuli_array_conductivity[..message_conductivity.len()].copy_from_slice(&message_conductivity);
    let stimuli_i2c_response_conductivity = I2cResponse {
        read_buf: stimuli_array_conductivity,
        length: atlas_scientific_ezo_constants::MESSAGE_SIZE,
    };

    let mut stimuli_array_ph = [0u8; 64];
    let message_ph: [u8; atlas_scientific_ezo_constants::MESSAGE_SIZE] = [
        atlas_scientific_ezo_constants::MESSAGE_START_MAGIC,
        b'7',
        atlas_scientific_ezo_constants::DECIMAL_POINT,
        b'2',
        b'0',
        b'0',
        b'0',
        atlas_scientific_ezo_constants::MESSAGE_FINISH_MAGIC,
    ];
    stimuli_array_ph[..message_ph.len()].copy_from_slice(&message_ph);
    let stimuli_i2c_response_ph = I2cResponse {
        read_buf: stimuli_array_ph,
        length: atlas_scientific_ezo_constants::MESSAGE_SIZE,
    };

    let config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();

    let mut atlas_scientific_ezo_temperature = AtlasScientificEzo::new(
        config.atlas_scientific.address_atlas_scientific_temperature,
        config.atlas_scientific.sleep_time_millis_temperature,
        atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
    );
    let mut atlas_scientific_ezo_ph = AtlasScientificEzo::new(
        config.atlas_scientific.address_atlas_scientific_ph,
        config.atlas_scientific.sleep_time_millis_ph,
        atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
    );
    let mut atlas_scientific_ezo_conductivity = AtlasScientificEzo::new(
        config
            .atlas_scientific
            .address_atlas_scientific_conductivity,
        config.atlas_scientific.sleep_time_millis_conductivity,
        atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
    );

    let mut channels = Channels::new_for_test();

    // Mutexes for Atlas Scientific signals
    let mutex_atlas_scientific_temperature = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_water_temperature,
    )));

    let mutex_atlas_scientific_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_ph,
    )));

    let mutex_atlas_scientific_conductivity = Arc::new(Mutex::new(AtlasScientificResultData::new(
        config.sensor_manager.replacement_value_conductivity,
    )));

    // Sending stimuli for water temperature before triggering try_recv from the test object
    _ = channels
        .i2c_interface
        .tx_i2c_interface_to_atlas_scientific
        .send(Ok(stimuli_i2c_response_temperature));

    _ = atlas_scientific_ezo_temperature.receive_response_from_i2c_interface(
        &AquariumSignal::WaterTemperature,
        &mut channels.atlas_scientific,
        &mutex_atlas_scientific_temperature,
        &mutex_atlas_scientific_ph,
        &mutex_atlas_scientific_conductivity,
    );

    match mutex_atlas_scientific_temperature.lock() {
        Ok(result) => {
            let result_data = result.clone();
            assert_eq!(result_data.value, 23.1);
        }
        Err(e) => {
            panic!("mutex_atlas_scientific_temperature lock poisoned: {:?}", e);
        }
    };

    // Sending stimuli for pH before triggering try_recv from the test object
    _ = channels
        .i2c_interface
        .tx_i2c_interface_to_atlas_scientific
        .send(Ok(stimuli_i2c_response_ph));

    _ = atlas_scientific_ezo_ph.receive_response_from_i2c_interface(
        &AquariumSignal::pH,
        &mut channels.atlas_scientific,
        &mutex_atlas_scientific_temperature,
        &mutex_atlas_scientific_ph,
        &mutex_atlas_scientific_conductivity,
    );

    match mutex_atlas_scientific_ph.lock() {
        Ok(result) => {
            let result_data = result.clone();
            assert_eq!(result_data.value, 7.2);
        }
        Err(e) => {
            panic!("mutex_atlas_scientific_ph lock poisoned: {:?}", e);
        }
    };

    // Sending stimuli for conductivity before triggering try_recv from the test object
    _ = channels
        .i2c_interface
        .tx_i2c_interface_to_atlas_scientific
        .send(Ok(stimuli_i2c_response_conductivity));

    _ = atlas_scientific_ezo_conductivity.receive_response_from_i2c_interface(
        &AquariumSignal::Conductivity,
        &mut channels.atlas_scientific,
        &mutex_atlas_scientific_temperature,
        &mutex_atlas_scientific_ph,
        &mutex_atlas_scientific_conductivity,
    );

    match mutex_atlas_scientific_conductivity.lock() {
        Ok(result) => {
            let result_data = result.clone();
            assert_eq!(result_data.value, 48000.0);
        }
        Err(e) => {
            panic!("mutex_atlas_scientific_conductivity lock poisoned: {:?}", e);
        }
    };
}

#[test]
// Test case checks if send_request_to_i2c_interface correctly sends a request for pH.
// [itest->swreq~sensors_read_data~1]
fn test_send_request_to_i2c_interface_ph_success() {
    let config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let sleep_time_millis_ph = config.atlas_scientific.sleep_time_millis_ph;
    let address_atlas_scientific_ph = config.atlas_scientific.address_atlas_scientific_ph;
    let atlas_scientific_ezo_ph = AtlasScientificEzo::new(
        config.atlas_scientific.address_atlas_scientific_ph,
        config.atlas_scientific.sleep_time_millis_ph,
        atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
    );

    let (tx_atlas_scientific_to_i2c_interface, mut rx_i2c_interface_from_atlas_scientific): (
        AquaSender<I2cRequest>,
        AquaReceiver<I2cRequest>,
    ) = channel(1, false);
    let (_tx_i2c_interface_to_atlas_scientific, rx_atlas_scientific_from_i2c_interface): (
        AquaSender<I2cResult>,
        AquaReceiver<I2cResult>,
    ) = channel(1, false);
    let (tx_atlas_scientific_to_signal_handler, _rx_signal_handler_from_atlas_scientific): (
        AquaSender<bool>,
        AquaReceiver<bool>,
    ) = channel(1, false);
    let (_tx_signal_handler_to_atlas_scientific, rx_atlas_scientific_from_signal_handler): (
        AquaSender<InternalCommand>,
        AquaReceiver<InternalCommand>,
    ) = channel(1, false);
    let (tx_atlas_scientific_to_monitors, _rx_monitors_from_atlas_scientific): (
        AquaSender<AtlasScientificMonitorView>,
        AquaReceiver<AtlasScientificMonitorView>,
    ) = channel(1, false);
    let mut atlas_scientific_channels = AtlasScientificChannels {
        tx_atlas_scientific_to_i2c_interface,
        #[cfg(feature = "debug_channels")]
        cnt_tx_atlas_scientific_to_i2c_interface: 0,

        rx_atlas_scientific_from_i2c_interface,
        #[cfg(feature = "debug_channels")]
        cnt_rx_atlas_scientific_from_i2c_interface: 0,

        tx_atlas_scientific_to_signal_handler,
        #[cfg(feature = "debug_channels")]
        cnt_tx_atlas_scientific_to_signal_handler: 0,

        rx_atlas_scientific_from_signal_handler,
        #[cfg(feature = "debug_channels")]
        cnt_rx_atlas_scientific_from_signal_handler: 0,

        tx_atlas_scientific_to_monitors,
    };

    let result =
        atlas_scientific_ezo_ph.send_request_to_i2c_interface(&mut atlas_scientific_channels);

    // Assert that the sending was successful
    assert!(result.is_ok());

    // Assert that a message was received and it's the correct one
    let received_request = rx_i2c_interface_from_atlas_scientific.try_recv().unwrap();
    assert_eq!(received_request.i2c_address, address_atlas_scientific_ph);
    assert_eq!(received_request.sleep_time_millis, sleep_time_millis_ph);
    assert_eq!(
        received_request.send_buf[0],
        [atlas_scientific_ezo_constants::COMMAND_READ][0]
    );
    assert_eq!(received_request.send_len, 1);
    assert_eq!(
        received_request.expected_response_length,
        atlas_scientific_ezo_constants::MESSAGE_SIZE
    );
}

#[test]
// Test case checks if send_request_to_i2c_interface correctly sends a request for conductivity.
// [itest->swreq~sensors_read_data~1]
fn test_send_request_to_i2c_interface_conductivity_success() {
    let config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let sleep_time_millis_conductivity = config.atlas_scientific.sleep_time_millis_conductivity;
    let address_atlas_scientific_conductivity = config
        .atlas_scientific
        .address_atlas_scientific_conductivity;
    let atlas_scientific_ezo_conductivity = AtlasScientificEzo::new(
        config
            .atlas_scientific
            .address_atlas_scientific_conductivity,
        config.atlas_scientific.sleep_time_millis_conductivity,
        atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
    );

    let mut channels = Channels::new_for_test();

    let result = atlas_scientific_ezo_conductivity
        .send_request_to_i2c_interface(&mut channels.atlas_scientific);

    // Assert that the sending was successful
    assert!(result.is_ok());

    // Assert that a message was received and it's the correct one
    let received_request = channels
        .i2c_interface
        .rx_i2c_interface_from_atlas_scientific
        .try_recv()
        .unwrap();
    assert_eq!(
        received_request.i2c_address,
        address_atlas_scientific_conductivity
    );
    assert_eq!(
        received_request.sleep_time_millis,
        sleep_time_millis_conductivity
    );
    assert_eq!(
        received_request.send_buf[0],
        [atlas_scientific_ezo_constants::COMMAND_READ][0]
    );
    assert_eq!(
        received_request.send_len, 1
    );
    assert_eq!(
        received_request.expected_response_length,
        atlas_scientific_ezo_constants::MESSAGE_SIZE
    );
}

#[test]
// Test case checks if send_request_to_i2c_interface correctly sends a request for water temperature.
// [itest->swreq~sensors_read_data~1]
fn test_send_request_to_i2c_interface_temperature_success() {
    let config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let sleep_time_millis_temperature = config.atlas_scientific.sleep_time_millis_temperature;
    let address_atlas_scientific_temperature =
        config.atlas_scientific.address_atlas_scientific_temperature;
    let atlas_scientific_ezo_temperature = AtlasScientificEzo::new(
        config.atlas_scientific.address_atlas_scientific_temperature,
        config.atlas_scientific.sleep_time_millis_temperature,
        atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
    );

    let mut channels = Channels::new_for_test();

    let result = atlas_scientific_ezo_temperature
        .send_request_to_i2c_interface(&mut channels.atlas_scientific);

    // Assert that the sending was successful
    assert!(result.is_ok());

    // Assert that a message was received and it's the correct one
    let received_request = channels
        .i2c_interface
        .rx_i2c_interface_from_atlas_scientific
        .try_recv()
        .unwrap();
    assert_eq!(
        received_request.i2c_address,
        address_atlas_scientific_temperature
    );
    assert_eq!(
        received_request.sleep_time_millis,
        sleep_time_millis_temperature
    );
    assert_eq!(
        received_request.send_buf[0],
        [atlas_scientific_ezo_constants::COMMAND_READ][0]
    );
    assert_eq!(
        received_request.expected_response_length,
        atlas_scientific_ezo_constants::MESSAGE_SIZE
    );
}

#[test]
// Test case checks that send_request_to_i2c_interface handles a disconnected channel.
// [itest->swreq~sensors_read_data~1]
fn test_send_request_to_i2c_interface_channel_disconnected() {
    let config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let atlas_scientific_ezo_ph = AtlasScientificEzo::new(
        config.atlas_scientific.address_atlas_scientific_ph,
        config.atlas_scientific.sleep_time_millis_ph,
        atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
    );

    let mut channels = Channels::new_for_test();

    // Drop the receiver to simulate a disconnected channel
    drop(
        channels
            .i2c_interface
            .rx_i2c_interface_from_atlas_scientific,
    );

    // Attempt to send a request
    let result =
        atlas_scientific_ezo_ph.send_request_to_i2c_interface(&mut channels.atlas_scientific);

    // Assert that the function returns false, indicating failure
    assert!(result.is_err());
}

/// Helper function to set up a standard test environment for receive tests.
#[allow(clippy::type_complexity)]
fn setup_receive_test() -> (
    HashMap<AquariumSignal, AtlasScientificEzo>,
    AtlasScientificChannels,
    AquaSender<I2cResult>,
    Arc<Mutex<AtlasScientificResultData>>,
    Arc<Mutex<AtlasScientificResultData>>,
    Arc<Mutex<AtlasScientificResultData>>,
) {
    let config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let mut drivers = HashMap::new();
    drivers.insert(
        AquariumSignal::WaterTemperature,
        AtlasScientificEzo::new(
            config.atlas_scientific.address_atlas_scientific_temperature,
            config.atlas_scientific.sleep_time_millis_temperature,
            atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
        ),
    );
    drivers.insert(
        AquariumSignal::pH,
        AtlasScientificEzo::new(
            config.atlas_scientific.address_atlas_scientific_ph,
            config.atlas_scientific.sleep_time_millis_ph,
            atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
        ),
    );
    drivers.insert(
        AquariumSignal::Conductivity,
        AtlasScientificEzo::new(
            config
                .atlas_scientific
                .address_atlas_scientific_conductivity,
            config.atlas_scientific.sleep_time_millis_conductivity,
            atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS,
        ),
    );

    let channels = Channels::new_for_test();

    // Initialize mutexes with a known default value (0.0)
    let mutex_temp = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
    let mutex_ph = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));
    let mutex_cond = Arc::new(Mutex::new(AtlasScientificResultData::new(0.0)));

    (
        drivers,
        channels.atlas_scientific,
        channels.i2c_interface.tx_i2c_interface_to_atlas_scientific,
        mutex_temp,
        mutex_ph,
        mutex_cond,
    )
}

#[test]
/// Tests the happy path for all three Atlas Scientific signals.
fn test_receive_response_success_all_signals() {
    run_receive_success_test(AquariumSignal::WaterTemperature, 25.5);
    run_receive_success_test(AquariumSignal::pH, 8.2);
    run_receive_success_test(AquariumSignal::Conductivity, 51000.0);
}

#[cfg(feature = "test-utils")]
#[test]
/// Tests that the function correctly handles an I2C communication failure from the channel.
fn test_receive_response_i2c_failure() {
    let (
        mut drivers,
        mut atlas_scientific_channels,
        mut tx_i2c_interface_to_atlas_scientific,
        mutex_temp,
        mutex_ph,
        mutex_cond,
    ) = setup_receive_test();

    // Send an I2C error over the channel.
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    let simulated_io_error = io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Simulated hardware permission error",
    );

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    let mock_rppal_error = rppal::i2c::Error::Io(simulated_io_error);

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    let mock_error = I2cError::WriteFailure {
        source: mock_rppal_error,
        location: module_path!().to_string(),
    };

    #[cfg(not(all(target_os = "linux", feature = "target_hw")))]
    let mock_error = I2cError::WriteFailure {
        location: module_path!().to_string(),
    };

    tx_i2c_interface_to_atlas_scientific
        .send(Err(mock_error))
        .unwrap();

    let driver_ph = drivers.get_mut(&AquariumSignal::pH).unwrap();

    let result = driver_ph.receive_response_from_i2c_interface(
        &AquariumSignal::pH,
        &mut atlas_scientific_channels,
        &mutex_temp,
        &mutex_ph,
        &mutex_cond,
    );

    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(AtlasScientificError::I2cCommunicationFailure { .. })
    ));
}

#[test]
/// Tests that the function returns the correct error when the channel is empty.
fn test_receive_response_channel_empty() {
    let (
        mut drivers,
        mut atlas_scientific_channels,
        _tx_i2c_interface_to_atlas_scientific,
        mutex_temp,
        mutex_ph,
        mutex_cond,
    ) = setup_receive_test();

    let driver_ph = drivers.get_mut(&AquariumSignal::pH).unwrap();

    let result = driver_ph.receive_response_from_i2c_interface(
        &AquariumSignal::pH,
        &mut atlas_scientific_channels,
        &mutex_temp,
        &mutex_ph,
        &mutex_cond,
    );

    assert!(matches!(
        result,
        Err(AtlasScientificError::ChannelIsEmpty(_))
    ));
}

#[test]
/// Tests that the function returns the correct error when the channel is disconnected.
fn test_receive_response_channel_disconnected() {
    let (
        mut drivers,
        mut atlas_scientific_channels,
        tx_i2c_interface_to_atlas_scientific,
        mutex_temp,
        mutex_ph,
        mutex_cond,
    ) = setup_receive_test();

    drop(tx_i2c_interface_to_atlas_scientific); // Disconnect the channel

    let driver_ph = drivers.get_mut(&AquariumSignal::pH).unwrap();
    let result = driver_ph.receive_response_from_i2c_interface(
        &AquariumSignal::pH,
        &mut atlas_scientific_channels,
        &mutex_temp,
        &mutex_ph,
        &mutex_cond,
    );

    assert!(matches!(
        result,
        Err(AtlasScientificError::ChannelIsDisconnected { .. })
    ));
}

#[cfg(feature = "test-utils")]
#[test]
/// Tests that a long wait for a mutex lock triggers the internal warning flag.
fn test_receive_response_long_mutex_wait_triggers_warning() {
    let (
        mut drivers,
        mut atlas_scientific_channels,
        mut tx_i2c_interface_to_atlas_scientific,
        mutex_temp,
        mutex_ph,
        mutex_cond,
    ) = setup_receive_test();

    // Spawn a thread that will hold the lock for a while, simulating a slow consumer.
    let mutex_temp_clone = mutex_temp.clone();
    let handle = thread::spawn(move || {
        let _lock = mutex_temp_clone.lock().unwrap();
        let spin_sleeper = SpinSleeper::default();
        spin_sleeper.sleep(Duration::from_millis(
            atlas_scientific_constants::MAX_MUTEX_ACCESS_DURATION_MILLIS + 15,
        ));
        // Lock is released when the thread finishes.
    });

    // Give the spawned thread a moment to acquire the lock.
    let spin_sleeper = SpinSleeper::default();
    spin_sleeper.sleep(Duration::from_millis(5));

    // Send a valid response that will be processed.
    let mut read_buf = [0u8; 64];
    let message = [1, b'2', b'5', b'.', b'0', b'0', b'0', 0];
    read_buf[..message.len()].copy_from_slice(&message);
    let i2c_response = I2cResponse {
        read_buf,
        length: 8,
    };
    tx_i2c_interface_to_atlas_scientific
        .send(Ok(i2c_response))
        .unwrap();

    let driver_temperature = drivers.get_mut(&AquariumSignal::WaterTemperature).unwrap();

    // This call will now block until the spawned thread releases the lock.
    // The duration of this block will be longer than the threshold.
    let result = driver_temperature.receive_response_from_i2c_interface(
        &AquariumSignal::WaterTemperature,
        &mut atlas_scientific_channels,
        &mutex_temp,
        &mutex_ph,
        &mutex_cond,
    );

    // Wait for the spawned thread to complete.
    handle.join().unwrap();

    // Assert that the operation was ultimately successful.
    assert!(result.is_ok());

    // Assert that the warning flag was set because of the long wait.
    assert!(driver_temperature.mutex_temperature_access_duration_exceeded);
}

#[test]
#[cfg(all(target_os = "linux", feature = "target_hw"))]
#[ignore]
// read from real Atlas Scientific hardware (EZO) combined in one test case to avoid race conditions:
// - pH sensor
// - conductivity sensor
// - temperature
///

/// [itest->swreq~sensors_read_data~1]
pub fn test_atlas_scientific_ezo_read_sensors() {
    let config: ConfigData =
        read_config_file("config/aquarium_control_test_generic.toml".to_string()).unwrap();
    let mut i2c_interface = I2cInterface::new(config.i2c_interface).unwrap();

    let send_buf = [atlas_scientific_ezo_constants::COMMAND_READ];
    let request_ph = I2cRequest::new(
        config.atlas_scientific.address_atlas_scientific_ph,
        &send_buf,
        config.atlas_scientific.sleep_time_millis_ph,
        atlas_scientific_ezo_constants::MESSAGE_SIZE,
    )
    .unwrap();
    let request_temperature = I2cRequest::new(
        config.atlas_scientific.address_atlas_scientific_temperature,
        &send_buf,
        config.atlas_scientific.sleep_time_millis_temperature,
        atlas_scientific_ezo_constants::MESSAGE_SIZE,
    )
    .unwrap();
    let request_conductivity = I2cRequest::new(
        config
            .atlas_scientific
            .address_atlas_scientific_conductivity,
        &send_buf,
        config.atlas_scientific.sleep_time_millis_conductivity,
        atlas_scientific_ezo_constants::MESSAGE_SIZE,
    )
    .unwrap();

    match i2c_interface.get_response_from_i2c(request_ph) {
        Ok(response_ph) => {
            println!("Atlas Scientific response for pH: {:?}", response_ph);
            let ph = AtlasScientificEzo::check_response(
                response_ph.read_buf[..atlas_scientific_ezo_constants::MESSAGE_SIZE]
                    .try_into()
                    .unwrap(),
                true,
            )
            .unwrap();
            println!("pH={}", ph);
        }
        Err(err_response_ph) => {
            println!("Failed to read from pH sensor: {:?}", err_response_ph);
        }
    }

    match i2c_interface.get_response_from_i2c(request_temperature) {
        Ok(response_temperature) => {
            println!(
                "Atlas Scientific response for temperature: {:?}",
                response_temperature
            );
            let temperature = AtlasScientificEzo::check_response(
                response_temperature.read_buf[..atlas_scientific_ezo_constants::MESSAGE_SIZE]
                    .try_into()
                    .unwrap(),
                true,
            )
            .unwrap();
            println!("temperature={}", temperature);
        }
        Err(err_response_temperature) => {
            println!(
                "Failed to read from temperature sensor: {:?}",
                err_response_temperature
            );
        }
    }

    match i2c_interface.get_response_from_i2c(request_conductivity) {
        Ok(response_conductivity) => {
            println!(
                "Atlas Scientific response for conductivity: {:?}",
                response_conductivity
            );
            let conductivity = AtlasScientificEzo::check_response(
                response_conductivity.read_buf[..atlas_scientific_ezo_constants::MESSAGE_SIZE]
                    .try_into()
                    .unwrap(),
                true,
            )
            .unwrap();
            println!("conductivity={}", conductivity);
        }
        Err(err_response_conductivity) => {
            println!(
                "Failed to read from conductivity sensor: {:?}",
                err_response_conductivity
            );
        }
    }
}
