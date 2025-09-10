/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::launch::channels::{AquaChannelError, AquaReceiver, AquaSender};
use crate::utilities::channel_content::InternalCommand;
use crate::water::refill_monitor_view::RefillMonitorView;
use std::fmt;

/// Struct collects the channels for the communication between the refill control and the other components.
/// Additionally, the struct contains counters for debug purposes.
/// The implementation of the struct contains functions for those channels which are frequently used.
/// The functions increment the usage counters.
pub struct RefillChannels {
    /// Sender part of the channel for communication to relay manager
    pub tx_refill_to_relay_manager: AquaSender<InternalCommand>,

    /// Receiver part of the channel for communication from the relay manager
    pub rx_refill_from_relay_manager: AquaReceiver<bool>,

    /// Sender part of the channel for communication to the signal handler
    pub tx_refill_to_signal_handler: AquaSender<bool>,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_refill_from_signal_handler: AquaReceiver<InternalCommand>,

    /// Sender part of the channel for communication to the schedule checker
    pub tx_refill_to_schedule_check: AquaSender<InternalCommand>,

    /// Receiver part of the channel for communication from the schedule checker
    pub rx_refill_from_schedule_check: AquaReceiver<bool>,

    /// Receiver part of the channel for communication from the messaging
    pub rx_refill_from_messaging_opt: Option<AquaReceiver<InternalCommand>>,

    /// Sender part of the channel for communication to the monitors
    pub tx_refill_to_monitors: AquaSender<RefillMonitorView>,
}

impl RefillChannels {
    pub fn send_to_relay_manager(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_refill_to_relay_manager.send(command)
    }

    pub fn receive_from_relay_manager(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_refill_from_relay_manager.recv()
    }

    pub fn send_to_schedule_check(
        &mut self,
        command: InternalCommand,
    ) -> Result<(), AquaChannelError> {
        self.tx_refill_to_schedule_check.send(command)
    }

    pub fn receive_from_schedule_check(&mut self) -> Result<bool, AquaChannelError> {
        self.rx_refill_from_schedule_check.recv()
    }

    pub fn send_to_monitor(&mut self, view: RefillMonitorView) -> Result<(), AquaChannelError> {
        self.tx_refill_to_monitors.send(view)
    }

    pub fn send_to_signal_handler(&mut self, status: bool) -> Result<(), AquaChannelError> {
        self.tx_refill_to_signal_handler.send(status)
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for RefillChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "=== RefillChannels ===")?;
        writeln!(
            f,
            "tx_refill_to_relay_manager: {}",
            self.tx_refill_to_relay_manager.count
        )?;
        writeln!(
            f,
            "rx_refill_from_relay_manager: {}",
            self.rx_refill_from_relay_manager.count
        )?;
        writeln!(
            f,
            "tx_refill_to_signal_handler: {}",
            self.tx_refill_to_signal_handler.count
        )?;
        writeln!(
            f,
            "rx_refill_from_signal_handler: {}",
            self.rx_refill_from_signal_handler.count
        )?;
        writeln!(
            f,
            "tx_refill_to_schedule_check: {}",
            self.tx_refill_to_schedule_check.count
        )?;
        writeln!(
            f,
            "rx_refill_from_schedule_check: {}",
            self.rx_refill_from_schedule_check.count
        )?;
        writeln!(
            f,
            "rx_refill_from_messaging_opt: {}",
            self.rx_refill_from_messaging_opt.as_ref().unwrap().count
        )?;
        write!(
            f,
            "tx_refill_to_monitors: {}",
            self.tx_refill_to_monitors.count
        )
    }
}

#[cfg(not(feature = "debug_channels"))]
impl fmt::Display for RefillChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Channel counters are not active. Use --features \"debug_channels\" to enable them."
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::channels::Channels;

    /// A helper function to create a `RefillChannels` instance with the mock channel ends for testing.
    ///
    /// This function sets up all the necessary channels and returns the `RefillChannels`
    /// struct along with the "other ends" of the channels that the test needs to interact with
    /// or assert against.
    fn setup_channels() -> (
        RefillChannels,
        AquaReceiver<InternalCommand>, // Test receiver for schedule_check
        AquaSender<bool>,              // Test sender for schedule_check response
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
    fn test_send_to_schedule_check_sends_command_and_increments_counter() {
        // Arrange
        let (mut channels, mut rx_schedule_test, _, _) = setup_channels();
        let command = InternalCommand::Quit; // Use any variant for the test

        // Act
        let result = channels.send_to_schedule_check(command.clone());

        // Assert
        assert!(result.is_ok(), "Sending should not fail");
        let received_command = rx_schedule_test.recv().unwrap();
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
    fn test_receive_from_schedule_check_receives_response_and_increments_counter() {
        // Arrange
        let (mut channels, _, mut tx_schedule_resp_test, _) = setup_channels();
        let expected_response = true;
        tx_schedule_resp_test.send(expected_response).unwrap();

        // Act
        let result = channels.receive_from_schedule_check();

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
    fn test_send_to_monitor_sends_view_and_increments_counter() {
        // Arrange
        let (mut channels, _, _, mut rx_monitor_test) = setup_channels();
        let view = RefillMonitorView::default();

        // Act
        let result = channels.send_to_monitor(view);

        // Assert
        assert!(result.is_ok(), "Sending should not fail");
        let received_view = rx_monitor_test.recv().unwrap();
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
}
