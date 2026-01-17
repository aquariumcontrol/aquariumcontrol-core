/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Defines the message structure used for Inter-Process Communication (IPC).
//!
//! This module contains the `Message` struct which represents the data exchanged
//! between the external control client (`aquarium_client`) and the main application
//! via POSIX message queues.

/// Stores the content of the message and provides functionality for serialization and deserialization.
#[derive(Debug, Default, Copy, Clone)]
pub struct Message {
    /// The domain (subsystem) the message is addressed to.
    pub domain: i32,
    /// The command to be executed.
    pub command: i32,
    /// Optional first parameter for the command.
    pub command_param1: i32,
    /// Optional second parameter for the command.
    pub command_param2: i32,
}

impl Message {
    /// Creates a new `Message` instance with all its fields initialized to zero.
    pub fn new() -> Message {
        Message {
            domain: 0,
            command: 0,
            command_param1: 0,
            command_param2: 0,
        }
    }

    /// Serializes the message into a byte vector for transmission.
    ///
    /// The format is a 20-byte buffer where bytes 4-19 contain the little-endian
    /// representation of the four `i32` fields. Bytes 0-3 are currently unused (padding).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = vec![0; 20];
        let domain_buf = self.domain.to_le_bytes();
        let command_buf = self.command.to_le_bytes();
        let command_param1_buf = self.command_param1.to_le_bytes();
        let command_param2_buf = self.command_param2.to_le_bytes();

        // transfer partial arrays to array for a complete message
        // Bytes 0-3 are padding/unused in the current protocol
        buf[4] = domain_buf[0];
        buf[5] = domain_buf[1];
        buf[6] = domain_buf[2];
        buf[7] = domain_buf[3];
        buf[8] = command_buf[0];
        buf[9] = command_buf[1];
        buf[10] = command_buf[2];
        buf[11] = command_buf[3];
        buf[12] = command_param1_buf[0];
        buf[13] = command_param1_buf[1];
        buf[14] = command_param1_buf[2];
        buf[15] = command_param1_buf[3];
        buf[16] = command_param2_buf[0];
        buf[17] = command_param2_buf[1];
        buf[18] = command_param2_buf[2];
        buf[19] = command_param2_buf[3];

        buf
    }

    /// Deserializes a byte buffer into a `Message`.
    ///
    /// Expects a buffer of at least 20 bytes.
    ///
    /// # Arguments
    /// * `msgbuf` - The byte buffer containing the message data.
    ///
    /// # Returns
    /// * `Ok(Message)` if parsing is successful.
    /// * `Err(&str)` if the buffer is too short.
    pub fn from_bytes(msgbuf: &[u8]) -> Result<Message, &'static str> {
        if msgbuf.len() < 20 {
            return Err("Message buffer too short");
        }

        let mut msg = Message::new();

        let mut domain_buf: [u8; 4] = [0; 4];
        domain_buf.copy_from_slice(&msgbuf[4..8]);
        msg.domain = i32::from_le_bytes(domain_buf);

        let mut command_buf: [u8; 4] = [0; 4];
        command_buf.copy_from_slice(&msgbuf[8..12]);
        msg.command = i32::from_le_bytes(command_buf);

        let mut command_param1_buf: [u8; 4] = [0; 4];
        command_param1_buf.copy_from_slice(&msgbuf[12..16]);
        msg.command_param1 = i32::from_le_bytes(command_param1_buf);

        let mut command_param2_buf: [u8; 4] = [0; 4];
        command_param2_buf.copy_from_slice(&msgbuf[16..20]);
        msg.command_param2 = i32::from_le_bytes(command_param2_buf);

        Ok(msg)
    }
}
