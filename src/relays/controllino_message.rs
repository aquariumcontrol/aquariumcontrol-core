/* Copyright 2025 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the data structures for the custom 8-byte serial protocol used with the Controllino.
//!
//! This module is the definitive source for creating valid message packets to be sent
//! to the Controllino hardware. It provides a safe and structured way to build commands,
//! ensuring that all required fields and, most importantly, the checksums are correctly
//! calculated and placed before transmission.
//!
//! ## Key Components
//!
//! - **`ControllinoMessageContent`**: A high-level, developer-friendly struct for
//!   *building* a command. It represents the logical parts of a message (command, ID, payload)
//!   without concerning the user with the low-level byte layout or checksums.
//!
//! - **`ControllinoMessage`**: The low-level, 8-byte representation of the message that is
//!   ready for direct serial transmission. Its `data` field is a `[u8; 8]` array.
//!
//! - **`ControllinoMessage::new()`**: The crucial conversion function that takes a high-level
//!   `ControllinoMessageContent` and serializes it into a `ControllinoMessage`,
//!   calculating and embedding the two required checksums in the process.
//!
//! ## Serial Communication Protocol
//!
//! The module implements a fixed-size 8-byte packet protocol with two separate checksums
//! to ensure message integrity over the serial line.
//!
//! The byte layout is as follows:
//!
//! | Byte  | Name        | Description                                       |
//! | :---- | :---------- | :------------------------------------------------ |
//! | `[0]` | `Command`   | A character representing the action (e.g., 'S', 'U'). |
//! | `[1]` | `ID`        | The numeric ID of the target relay or output.     |
//! | `[2]` | `Reserved`  | Reserved for future use (always 0).               |
//! | `[3]` | `Checksum1` | XOR checksum of bytes `[0]`, `[1]`, and `[2]`.    |
//! | `[4]` | `PayloadHi` | The high byte of a 16-bit payload value.          |
//! | `[5]` | `PayloadLo` | The low byte of a 16-bit payload value.           |
//! | `[6]` | `Reserved`  | Reserved for future use (always '0').             |
//! | `[7]` | `Checksum2` | XOR checksum of bytes `[4]`, `[5]`, and `[6]`.    |

use crate::relays::actuate_controllino::controllino_constants;
use std::fmt;

/// Container for message content sent to Controllino
pub struct ControllinoMessageContent {
    /// Set by control application with command sent to Controllino
    pub command: char,

    /// Set by control application with id (e.g., for relay) of an element sent to Controllino
    pub id: u8,

    /// Unused byte
    pub reserved1: u8,

    /// Either used to communicate pulse duration in milliseconds to Controllino or
    /// set by Controllino with data to be sent back to control application
    pub payload: u16,

    /// Unused byte
    pub reserved2: char,
}

impl ControllinoMessageContent {
    /// Provides a new `ControllinoMessageContent` struct.
    ///
    /// This struct serves as a container for the data that will eventually be
    /// assembled into a full `ControllinoMessage` for serial transmission.
    /// It initializes the message content with the specified command, ID, and
    /// pulse duration, setting reserved bytes to zero.
    ///
    /// # Arguments
    /// * `command` - The character representing the command to be sent to Controllino.
    /// * `id` - The identifier for the relay or digital output relevant to the command.
    /// * `duration_millis` - The pulse duration in milliseconds, or `0` if not applicable to the command.
    pub fn new(command: char, id: u8, duration_millis: u16) -> ControllinoMessageContent {
        ControllinoMessageContent {
            command,
            id,
            reserved1: 0,
            payload: duration_millis,
            reserved2: '0',
        }
    }
}

/// Data structure for the message to be sent to Controllino
#[derive(Debug, PartialEq)]
pub struct ControllinoMessage {
    /// represents the raw message transferred via serial port
    pub data: [u8; controllino_constants::MESSAGE_SIZE],
}

impl ControllinoMessage {
    /// Creates a new `ControllinoMessage` from `ControllinoMessageContent`.
    ///
    /// This crucial function takes the structured message content, converts its fields
    /// into a raw 8-byte array (`[u8; 8]`) suitable for serial transmission, and
    /// **calculates and embeds the necessary checksums** into the data array.
    /// The resulting `ControllinoMessage` is ready to be sent directly to the Controllino hardware.
    ///
    /// # Arguments
    /// * `content` - The `ControllinoMessageContent` struct containing the command, ID,
    ///   and payload (e.g., pulse duration) to be packed into the message.
    ///
    /// # Returns
    /// A new `ControllinoMessage` struct with its `data` field populated, including checksums.
    pub fn new(content: ControllinoMessageContent) -> ControllinoMessage {
        // transfer content into data structure and calculate checksum
        let mut data: [u8; controllino_constants::MESSAGE_SIZE] =
            [0; controllino_constants::MESSAGE_SIZE];
        data[0] = content.command as u8;
        data[1] = content.id;
        data[2] = content.reserved1;
        data[3] = data[0] ^ data[1] ^ data[2];
        data[4] = ((content.payload & 0xFF00) >> 8) as u8;
        data[5] = (content.payload & 0x00FF) as u8;
        data[6] = content.reserved2 as u8;
        data[7] = data[4] ^ data[5] ^ data[6];

        ControllinoMessage { data }
    }
}

impl fmt::Display for ControllinoMessage {
    /// Provides output of the struct data for debugging purposes.
    /// # Arguments
    /// * `f` - configuration for formatting
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ControllinoMessage: [0]={} [1]={} [2]={} [3]={} [4]={} [5]={} [6]={} [7]={}",
            self.data[0],
            self.data[1],
            self.data[2],
            self.data[3],
            self.data[4],
            self.data[5],
            self.data[6],
            self.data[7]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_controllino_message_set_relay() {
        // Arrange: Create content for a simple "Set Relay" command with no duration.
        let command = 'S';
        let id = 5;
        let duration = 0;
        let content = ControllinoMessageContent::new(command, id, duration);

        // Act: Create the low-level message.
        let message = ControllinoMessage::new(content);

        // Assert: Verify each byte in the resulting data array.
        let mut expected_data = [0u8; 8];
        expected_data[0] = 'S' as u8; // 83
        expected_data[1] = 5;
        expected_data[2] = 0;
        expected_data[3] = expected_data[0] ^ expected_data[1] ^ expected_data[2]; // Checksum 1
        expected_data[4] = 0; // Payload high byte
        expected_data[5] = 0; // Payload low byte
        expected_data[6] = '0' as u8; // 48
        expected_data[7] = expected_data[4] ^ expected_data[5] ^ expected_data[6]; // Checksum 2

        assert_eq!(
            message.data, expected_data,
            "Set Relay message did not match expected format."
        );
    }

    #[test]
    fn test_new_controllino_message_pulse_relay() {
        // Arrange: Create content for a "Pulse Relay" command with a duration.
        let command = 'P';
        let id = 9;
        let duration = 1500; // Represents 0x05DC in hex
        let content = ControllinoMessageContent::new(command, id, duration);

        // Act: Create the low-level message.
        let message = ControllinoMessage::new(content);

        // Assert: Verify each byte, especially the split payload and checksums.
        let mut expected_data = [0u8; 8];
        expected_data[0] = 'P' as u8; // 80
        expected_data[1] = 9;
        expected_data[2] = 0;
        expected_data[3] = expected_data[0] ^ expected_data[1] ^ expected_data[2]; // Checksum 1
        expected_data[4] = 5; // High byte of 1500 (0x05)
        expected_data[5] = 220; // Low byte of 1500 (0xDC)
        expected_data[6] = '0' as u8; // 48
        expected_data[7] = expected_data[4] ^ expected_data[5] ^ expected_data[6]; // Checksum 2

        assert_eq!(
            message.data, expected_data,
            "Pulse Relay message did not match expected format."
        );
    }

    #[test]
    fn test_new_controllino_message_heartbeat_zero_values() {
        // Arrange: Test with zero values for ID and duration (e.g., a heartbeat).
        let command = 'H';
        let id = 0;
        let duration = 0;
        let content = ControllinoMessageContent::new(command, id, duration);

        // Act: Create the low-level message.
        let message = ControllinoMessage::new(content);

        // Assert: Verify checksums with zeroed inputs.
        let mut expected_data = [0u8; 8];
        expected_data[0] = 'H' as u8; // 72
        expected_data[1] = 0;
        expected_data[2] = 0;
        expected_data[3] = expected_data[0]; // Checksum 1 is just the command byte
        expected_data[4] = 0;
        expected_data[5] = 0;
        expected_data[6] = '0' as u8; // 48
        expected_data[7] = expected_data[6]; // Checksum 2 is just the reserved byte

        assert_eq!(
            message.data, expected_data,
            "Heartbeat message with zero values did not match."
        );
    }

    #[test]
    fn test_new_controllino_message_max_values() {
        // Arrange: Test with maximum values for ID and duration.
        let command = 'X';
        let id = u8::MAX; // 255
        let duration = u16::MAX; // 65535 (0xFFFF)
        let content = ControllinoMessageContent::new(command, id, duration);

        // Act: Create the low-level message.
        let message = ControllinoMessage::new(content);

        // Assert: Verify correct handling of max values and checksum logic.
        let mut expected_data = [0u8; 8];
        expected_data[0] = 'X' as u8; // 88
        expected_data[1] = 255;
        expected_data[2] = 0;
        expected_data[3] = expected_data[0] ^ expected_data[1] ^ expected_data[2]; // Checksum 1
        expected_data[4] = 255; // High byte of 0xFFFF
        expected_data[5] = 255; // Low byte of 0xFFFF
        expected_data[6] = '0' as u8; // 48
        expected_data[7] = expected_data[4] ^ expected_data[5] ^ expected_data[6]; // 255 ^ 255 ^ 48 = 48

        assert_eq!(
            message.data, expected_data,
            "Message with max values did not match expected format."
        );
        assert_eq!(
            expected_data[7], 48,
            "Checksum 2 with two 255s should result in the third value."
        );
    }
}
