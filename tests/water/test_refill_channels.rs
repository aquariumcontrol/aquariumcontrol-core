/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Test the channel aggregation struct for the `Refill` thread.

use aquarium_control::launch::channels::{AquaReceiver, AquaSender, Channels};
use aquarium_control::utilities::channel_content::InternalCommand;
use aquarium_control::water::refill_channels::RefillChannels;
use aquarium_control::water::refill_monitor_view::RefillMonitorView;

/// A helper function to create a `RefillChannels` instance with the mock channel ends for testing.
///
/// This function sets up all the necessary channels and returns the `RefillChannels`
/// struct along with the "other ends" of the channels that the test needs to interact with
/// or assert against.
///
/// [utest->swreq~water_channel_communication~1]
fn setup_channels() -> (
    RefillChannels,
    AquaReceiver<InternalCommand>,   // Test receiver for schedule_check
    AquaSender<bool>,                // Test sender for schedule_check response
    AquaReceiver<RefillMonitorView>, // Test receiver for monitors
) {
    let channels = Channels::new_for_test();
    (
        channels.refill,
        channels.schedule_check.rx_schedule_check_from_refill,
        channels.schedule_check.tx_schedule_check_to_refill,
        channels.monitors.rx_monitors_from_refill,
    )
}

#[test]
// Verifies that send to schedule check sends command and increments counter.
fn test_send_to_schedule_check_sends_command_and_increments_counter() {
    // Arrange
    let (mut channels, mut rx_schedule_test, _, _) = setup_channels();
    let command = InternalCommand::Quit; // Use any variant for the test

    // Act
    let result = channels.send_to_schedule_check(command.clone());

    // Assert
    assert!(result.is_ok(), "Sending should not fail");
    let received_command = rx_schedule_test.blocking_recv().unwrap();
    assert_eq!(
        received_command, command,
        "The correct command should be received"
    );

    #[cfg(feature = "debug_channels")]
    {
        assert_eq!(
            channels.tx_refill_to_schedule_check, 1,
            "Counter should be incremented"
        );
    }
}

#[test]
// Verifies that receive from schedule check receives response and increments counter.
fn test_receive_from_schedule_check_receives_response_and_increments_counter() {
    // Arrange
    let (mut channels, _, mut tx_schedule_resp_test, _) = setup_channels();
    let expected_response = true;
    tx_schedule_resp_test.send(expected_response).unwrap();

    // Act
    let result = channels.rx_refill_from_schedule_check.blocking_recv();

    // Assert
    assert!(result.is_ok(), "Receiving should not fail");
    assert_eq!(
        result.unwrap(),
        expected_response,
        "The correct response should be received"
    );

    #[cfg(feature = "debug_channels")]
    {
        assert_eq!(
            channels.rx_refill_from_schedule_check.count, 1,
            "Counter should be incremented"
        );
    }
}

#[test]
// Verifies that send to monitor sends view and increments counter.
fn test_send_to_monitor_sends_view_and_increments_counter() {
    // Arrange
    let (mut channels, _, _, mut rx_monitor_test) = setup_channels();
    let view = RefillMonitorView::default();

    // Act
    let result = channels.send_to_monitor(view);

    // Assert
    assert!(result.is_ok(), "Sending should not fail");
    let received_view = rx_monitor_test.blocking_recv().unwrap();
    assert_eq!(
        received_view,
        RefillMonitorView::default(),
        "The correct monitor view should be received"
    );

    #[cfg(feature = "debug_channels")]
    {
        assert_eq!(
            channels.tx_refill_to_monitors.count, 1,
            "Counter should be incremented"
        );
    }
}
