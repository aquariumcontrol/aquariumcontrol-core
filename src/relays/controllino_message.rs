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
