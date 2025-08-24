/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use crate::utilities::channel_content::InternalCommand;
use crate::water::refill_monitor_view::RefillMonitorView;
use std::fmt;
use std::sync::mpsc::{Receiver, RecvError, Sender};

/// Struct collects the channels for the communication between the refill control and the other components.
/// Additionally, the struct contains counters for debug purposes.
/// The implementation of the struct contains functions for those channels which are frequently used.
/// The functions increment the usage counters.
pub struct RefillChannels {
    /// Sender part of the channel for communication to relay manager
    pub tx_refill_to_relay_manager: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_refill_to_relay_manager: u64,

    /// Receiver part of the channel for communication from the relay manager
    pub rx_refill_from_relay_manager: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_refill_from_relay_manager: u64,

    /// Sender part of the channel for communication to the signal handler
    pub tx_refill_to_signal_handler: Sender<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_refill_to_signal_handler: u64,

    /// Receiver part of the channel for communication from the signal handler
    pub rx_refill_from_signal_handler: Receiver<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_refill_from_signal_handler: u64,

    /// Sender part of the channel for communication to the schedule checker
    pub tx_refill_to_schedule_check: Sender<InternalCommand>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_refill_to_schedule_check: u64,

    /// Receiver part of the channel for communication from the schedule checker
    pub rx_refill_from_schedule_check: Receiver<bool>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_refill_from_schedule_check: u64,

    /// Receiver part of the channel for communication from the messaging
    pub rx_refill_from_messaging_opt: Option<Receiver<InternalCommand>>,
    #[cfg(feature = "debug_channels")]
    pub cnt_rx_refill_from_messaging_opt: u64,

    /// Sender part of the channel for communication to the monitors
    pub tx_refill_to_monitors: Sender<RefillMonitorView>,
    #[cfg(feature = "debug_channels")]
    pub cnt_tx_refill_to_monitors: u64,
}

impl RefillChannels {
    pub fn send_to_relay_manager(&mut self, command: InternalCommand) -> Result<(), std::sync::mpsc::SendError<InternalCommand>> {
        let result = self.tx_refill_to_relay_manager.send(command);
        #[cfg(feature = "debug_channels")]
        {
            self.cnt_tx_refill_to_relay_manager += 1;
        }
        result
    }

    pub fn receive_from_relay_manager(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_refill_from_relay_manager.recv();
        #[cfg(feature = "debug_channels")]
        {
            self.cnt_rx_refill_from_relay_manager += 1;
        }
        result
    }

    pub fn send_to_schedule_check(&mut self, command: InternalCommand) -> Result<(), std::sync::mpsc::SendError<InternalCommand>> {
        let result = self.tx_refill_to_schedule_check.send(command);
        #[cfg(feature = "debug_channels")]
        {
            self.cnt_tx_refill_to_schedule_check += 1;
        }
        result
    }

    pub fn receive_from_schedule_check(&mut self) -> Result<bool, RecvError> {
        let result = self.rx_refill_from_schedule_check.recv();
        #[cfg(feature = "debug_channels")]
        {
            self.cnt_rx_refill_from_schedule_check += 1;
        }
        result
    }

    pub fn send_to_monitor(&mut self, view: RefillMonitorView) -> Result<(), std::sync::mpsc::SendError<RefillMonitorView>> {
        let result = self.tx_refill_to_monitors.send(view);
        #[cfg(feature = "debug_channels")]
        {
            self.cnt_tx_refill_to_monitors += 1;
        }
        result
    }
}

#[cfg(feature = "debug_channels")]
impl fmt::Display for RefillChannels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "")?;
        writeln!(
            f,
            "cnt_tx_refill_to_relay_manager: {}",
            self.cnt_tx_refill_to_relay_manager
        )?;
        writeln!(
            f,
            "cnt_rx_refill_from_relay_manager: {}",
            self.cnt_rx_refill_from_relay_manager
        )?;
        writeln!(
            f,
            "cnt_tx_refill_to_signal_handler: {}",
            self.cnt_tx_refill_to_signal_handler
        )?;
        writeln!(
            f,
            "cnt_rx_refill_from_signal_handler: {}",
            self.cnt_rx_refill_from_signal_handler
        )?;
        writeln!(
            f,
            "cnt_tx_refill_to_schedule_check: {}",
            self.cnt_tx_refill_to_schedule_check
        )?;
        writeln!(
            f,
            "cnt_rx_refill_from_schedule_check: {}",
            self.cnt_rx_refill_from_schedule_check
        )?;
        writeln!(
            f,
            "cnt_rx_refill_from_messaging_opt: {}",
            self.cnt_rx_refill_from_messaging_opt
        )?;
        write!(
            f,
            "cnt_tx_refill_to_monitors: {}",
            self.cnt_tx_refill_to_monitors
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
    use std::sync::mpsc;

    /// A helper function to create a `RefillChannels` instance with mock channel ends for testing.
    ///
    /// This function sets up all the necessary `mpsc` channels and returns the `RefillChannels`
    /// struct along with the "other ends" of the channels that the test needs to interact with
    /// or assert against.
    fn setup_channels() -> (
        RefillChannels,
        mpsc::Receiver<InternalCommand>, // Test receiver for schedule_check
        mpsc::Sender<bool>,               // Test sender for schedule_check response
        mpsc::Receiver<RefillMonitorView>, // Test receiver for monitors
    ) {
        let (tx_relay, _rx_relay) = mpsc::channel();
        let (_tx_relay_resp, rx_relay_resp) = mpsc::channel();
        let (tx_signal, _rx_signal) = mpsc::channel();
        let (_tx_signal_resp, rx_signal_resp) = mpsc::channel();
        let (tx_schedule, rx_schedule_test) = mpsc::channel();
        let (tx_schedule_resp_test, rx_schedule) = mpsc::channel();
        let (_tx_messaging, rx_messaging) = mpsc::channel();
        let (tx_monitor, rx_monitor_test) = mpsc::channel();

        let channels = RefillChannels {
            tx_refill_to_relay_manager: tx_relay,
            rx_refill_from_relay_manager: rx_relay_resp,
            tx_refill_to_signal_handler: tx_signal,
            rx_refill_from_signal_handler: rx_signal_resp,
            tx_refill_to_schedule_check: tx_schedule,
            rx_refill_from_schedule_check: rx_schedule,
            rx_refill_from_messaging_opt: Some(rx_messaging),
            tx_refill_to_monitors: tx_monitor,

            #[cfg(feature = "debug_channels")]
            cnt_tx_refill_to_relay_manager: 0,
            #[cfg(feature = "debug_channels")]
            cnt_rx_refill_from_relay_manager: 0,
            #[cfg(feature = "debug_channels")]
            cnt_tx_refill_to_signal_handler: 0,
            #[cfg(feature = "debug_channels")]
            cnt_rx_refill_from_signal_handler: 0,
            #[cfg(feature = "debug_channels")]
            cnt_tx_refill_to_schedule_check: 0,
            #[cfg(feature = "debug_channels")]
            cnt_rx_refill_from_schedule_check: 0,
            #[cfg(feature = "debug_channels")]
            cnt_rx_refill_from_messaging_opt: 0,
            #[cfg(feature = "debug_channels")]
            cnt_tx_refill_to_monitors: 0,
        };

        (
            channels,
            rx_schedule_test,
            tx_schedule_resp_test,
            rx_monitor_test,
        )
    }

    #[test]
    fn test_send_to_schedule_check_sends_command_and_increments_counter() {
        // Arrange
        let (mut channels, rx_schedule_test, _, _) = setup_channels();
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
                channels.cnt_tx_refill_to_schedule_check, 1,
                "Counter should be incremented"
            );
        }
    }

    #[test]
    fn test_receive_from_schedule_check_receives_response_and_increments_counter() {
        // Arrange
        let (mut channels, _, tx_schedule_resp_test, _) = setup_channels();
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
                channels.cnt_rx_refill_from_schedule_check, 1,
                "Counter should be incremented"
            );
        }
    }

    #[test]
    fn test_send_to_monitor_sends_view_and_increments_counter() {
        // Arrange
        let (mut channels, _, _, rx_monitor_test) = setup_channels();
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
                channels.cnt_tx_refill_to_monitors, 1,
                "Counter should be incremented"
            );
        }
    }
}