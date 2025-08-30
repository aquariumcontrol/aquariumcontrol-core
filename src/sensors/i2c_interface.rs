/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

#[cfg(all(target_os = "linux", feature = "target_hw"))]
use log::info;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::{fmt, str};

use crate::sensors::i2c_error::I2cError;

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
        use log::error;
        use std::sync::mpsc::{TryRecvError};
        use rppal::i2c::I2c; // required for communication with AtlasScientific sensors

        #[cfg(feature = "debug_i2c_interface")]
        use log::debug;

        use std::time::Duration;
        use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
        use crate::sensors::i2c_error::I2cError::{InitializationFailure, SetTimeoutError};
        use crate::sensors::i2c_interface_channels::I2cInterfaceChannels;
        use nix::unistd::gettid;
    }
}

/// Holds the configuration data for the I2C interface.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
/// Struct is intentionally placed here instead of creating i2c_interface.
#[derive(Deserialize, Debug)]
pub struct I2cInterfaceConfig {
    /// maximum number of read attempts
    #[allow(unused)]
    max_read_attempts: u16,

    /// timeout for transactions in milliseconds
    #[allow(unused)]
    timeout_transaction: u32,
}

/// A constant for the maximum command length considering future applications as well.
const MAX_I2C_COMMAND_LENGTH: usize = 16;

/// A constant for the maximum response length from an I2C device.
const MAX_I2C_RESPONSE_LENGTH: usize = 64;

pub struct I2cRequest {
    #[allow(unused)]
    /// I2C address of the bus participant
    pub i2c_address: u16,

    #[allow(unused)]
    /// input sequence to be sent to the bus participant
    pub(crate) send_buf: [u8; MAX_I2C_COMMAND_LENGTH],

    #[allow(unused)]
    /// The actual length of the command in the buffer.
    pub(crate) send_len: usize,

    #[allow(unused)]
    /// sleep time in milliseconds before retrieving an answer from the bus participant
    pub(crate) sleep_time_millis: u64,

    #[allow(unused)]
    /// length of the expected response in byte
    pub(crate) expected_response_length: usize,
}

impl I2cRequest {
    /// Creates a new `I2cRequest` after validating the command length.
    ///
    /// This constructor prepares a command for sending over the I2C bus. It takes a
    /// command as a slice, copies it into a fixed-size buffer, and packages it
    /// with the necessary metadata for the I2C interface to execute the request.
    ///
    /// # Arguments
    /// * `i2c_address` - The 7-bit I2C address of the target device.
    /// * `send_slice` - A byte slice (`&[u8]`) containing the command to be sent.
    /// * `sleep_time_millis` - The duration in milliseconds to wait after sending the
    ///   command before attempting to read a response.
    /// * `expected_response_length` - The number of bytes expected in the response from the device.
    ///
    /// # Returns
    /// A `Result` containing a new `I2cRequest` struct on success.
    ///
    /// # Errors
    /// Returns an `I2cError` if the provided command is too large for the buffer:
    /// - `I2cError::CommandTooLong`: If the length of `send_slice` exceeds the
    ///   `MAX_I2C_COMMAND_LENGTH` constant.
    #[allow(unused)] // used in conditionally compiled code
    pub fn new(
        i2c_address: u16,
        send_slice: &[u8],
        sleep_time_millis: u64,
        expected_response_length: usize,
    ) -> Result<I2cRequest, I2cError> {
        // Ensure the command fits into our fixed-size buffer.
        if send_slice.len() > MAX_I2C_COMMAND_LENGTH {
            return Err(I2cError::CommandTooLong {
                location: module_path!().to_string(),
                max_len: MAX_I2C_COMMAND_LENGTH,
                actual_len: send_slice.len(),
            });
        }

        // Create a zero-initialized array and copy the command into it.
        let mut send_buf = [0u8; MAX_I2C_COMMAND_LENGTH];
        send_buf[..send_slice.len()].copy_from_slice(send_slice);

        Ok(I2cRequest {
            i2c_address,
            send_buf,
            send_len: send_slice.len(),
            sleep_time_millis,
            expected_response_length,
        })
    }
}

#[derive(Debug, Clone)]
pub struct I2cResponse {
    /// A fixed-size buffer holding the sequence received from the bus participant.
    pub read_buf: [u8; MAX_I2C_RESPONSE_LENGTH],

    #[allow(unused)]
    /// length of a message as informed by i2c::read
    pub length: usize,
}

impl fmt::Display for I2cResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // descriptive prefix
        write!(f, "I2cResponse [")?;

        // Use self.length to iterate over only the valid part of the buffer.
        let valid_slice = &self.read_buf[..self.length];

        // Format the read_buf content
        // Iterate over the bytes and format each as two-digit hexadecimal
        for (i, &byte) in valid_slice.iter().enumerate() {
            write!(f, "{byte:02x}")?; // :02x ensures two hexadecimal digits, padding with leading zero if necessary
            if i < valid_slice.len() - 1 {
                write!(f, " ")?; // Add a space between bytes for readability
            }
        }

        // Conditionally include the length field if it's compiled in
        #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
        {
            // Only show length if it's different from the actual buffer length,
            // or if it's explicitly useful. In many cases, length == read_buf.len().
            write!(f, "] (length: {})", self.length)?;
        }

        #[cfg(not(any(test, all(target_os = "linux", feature = "target_hw"))))]
        {
            write!(f, "]")?; // Close the bracket if length field is not present
        }

        Ok(())
    }
}

#[allow(dead_code)]
impl I2cResponse {
    /// Creates a new `I2cResponse` by copying data from a slice.
    ///
    /// This constructor is used to package the raw data received from an I2C device
    /// into a structured response. It copies the contents of the provided slice
    /// into a fixed-size internal buffer.
    ///
    /// # Arguments
    /// * `read_data` - A byte slice (`&[u8]`) containing the data read from the I2C bus.
    ///   The length of this slice determines the `length` field of the new struct.
    ///
    /// # Returns
    /// A new `I2cResponse` struct containing the sensor data.
    pub fn new(read_data: &[u8]) -> I2cResponse {
        let mut read_buf = [0u8; MAX_I2C_RESPONSE_LENGTH];

        // The length of read_data is the number of bytes we want to copy.
        // We assume `len` will not be greater than `MAX_I2C_RESPONSE_LENGTH` because
        // the calling function `get_response_from_i2c` uses a slice of the correct size.
        let len = read_data.len();
        read_buf[..len].copy_from_slice(read_data);

        I2cResponse {
            read_buf,
            length: len,
        }
    }
}

pub type I2cResult = Result<I2cResponse, I2cError>;

#[allow(dead_code)]
/// Contains the configuration and the implementation for communication via I2C **.
/// Sensor data is read periodically upon request via the channel and communicated back to the caller.
pub struct I2cInterface {
    /// I2C interface of rppal library
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    i2c: I2c,

    /// configuration for I2C interface
    config: I2cInterfaceConfig,

    /// spin sleeper for waiting response of sensor unit
    spin_sleeper: SpinSleeper,
}

impl I2cInterface {
    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Creates a new `I2cInterface` instance, establishing and configuring the I2C bus.
    ///
    /// This constructor initializes the I2C interface on the target hardware. It opens the I2C bus
    /// provided by the `rppal` library and sets a global transaction timeout based on the
    /// provided configuration.
    ///
    /// # Arguments
    /// * `config` - Configuration data for the I2C interface, including the transaction timeout.
    ///
    /// # Returns
    /// A `Result` containing a new `I2cInterface` struct on success, ready to communicate
    /// with devices on the I2C bus.
    ///
    /// # Errors
    /// Returns an `I2cError` if the I2C bus cannot be initialized or configured:
    /// - `I2cError::InitializationFailure`: If the underlying call to `I2c::new()` fails,
    ///   which can be caused by permission issues (not running as root or not in the `i2c` group),
    ///   or if the I2C hardware is not enabled on the system.
    /// - `I2cError::SetTimeoutError`: If setting the I2C transaction timeout fails.
    pub fn new(config: I2cInterfaceConfig) -> Result<I2cInterface, I2cError> {
        let i2c = I2c::new().map_err(|e| InitializationFailure {
            location: module_path!().to_string(),
            source: e,
        })?;
        i2c.set_timeout(config.timeout_transaction)
            .map_err(|e| SetTimeoutError {
                location: module_path!().to_string(),
                source: e,
            })?;

        Ok(I2cInterface {
            i2c,
            spin_sleeper: SpinSleeper::default(),
            config,
        })
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Performs a complete I2C write/read transaction with a sensor.
    ///
    /// This function orchestrates a full I2C communication cycle:
    /// 1.  Sets the I2C slave address for the target device.
    /// 2.  Writes the command from the `i2c_request`.
    /// 3.  Pauses for the specified duration to allow the sensor to process the command.
    /// 4.  Reads the response from the sensor into the provided buffer.
    ///
    /// # Arguments
    /// * `i2c_request` - A struct containing the I2C address, command, and timing details.
    /// * `buffer` - A mutable byte slice that will be used to store the data read from the sensor.
    ///
    /// # Returns
    /// A `Result` containing an `I2cResponse` with the successfully read sensor data.
    ///
    /// # Errors
    /// Returns an `I2cError` variant if any stage of the communication fails:
    /// - `I2cError::ProvidedBufferTooSmall`: If the provided `buffer` is not large enough
    ///   to hold the `expected_response_length`.
    /// - `I2cError::SetAddressError`: If setting the I2C slave address fails.
    /// - `I2cError::WriteFailure`: If the command cannot be written to the sensor.
    /// - `I2cError::InsufficientWrite`: If not all bytes of the command could be written.
    /// - `I2cError::InsufficientRead`: If the full response could not be read from the sensor.
    /// - `I2cError::IncorrectResponseLength`: If the number of bytes read does not match
    ///   the `expected_response_length`.
    pub fn get_response_from_i2c(
        &mut self,
        i2c_request: I2cRequest,
        buffer: &mut [u8],
    ) -> I2cResult {
        let sleep_duration = Duration::from_millis(i2c_request.sleep_time_millis);

        // Check the buffer's capacity against the request's needs.
        if buffer.len() < i2c_request.expected_response_length {
            return Err(I2cError::ProvidedBufferTooSmall {
                location: module_path!().to_string(),
                buffer_size: buffer.len(),
                expected_size: i2c_request.expected_response_length,
            });
        }

        self.i2c
            .set_slave_address(i2c_request.i2c_address)
            .map_err(|e| I2cError::SetAddressError {
                location: module_path!().to_string(),
                address: i2c_request.i2c_address,
                source: e,
            })?;

        let realized_write_len = self
            .i2c
            .write(&i2c_request.send_buf[..i2c_request.send_len])
            .map_err(|e| I2cError::WriteFailure {
                location: module_path!().to_string(),
                source: e,
            })?;
        if realized_write_len != i2c_request.send_len {
            return Err(I2cError::InsufficientWrite(
                module_path!().to_string(),
                i2c_request.send_len,
                realized_write_len,
            ));
        }

        // give the I2C bus participant time to process the request
        self.spin_sleeper.sleep(sleep_duration);

        // Use a sub-slice of the provided buffer for the read operation.
        let read_slice = &mut buffer[..i2c_request.expected_response_length];

        let received_message_length =
            self.i2c
                .read(read_slice)
                .map_err(|e| I2cError::InsufficientRead {
                    location: module_path!().to_string(),
                    source: e,
                })?;
        if received_message_length != i2c_request.expected_response_length {
            Err(I2cError::IncorrectResponseLength(
                module_path!().to_string(),
                i2c_request.expected_response_length,
                received_message_length,
            ))
        } else {
            Ok(I2cResponse::new(read_slice))
        }
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Executes the main loop for the I2C interface thread, acting as a request/response service.
    ///
    /// This function runs indefinitely, processing I2C requests received from other modules.
    /// It listens on the `rx_from_atlas_scientific` channel for an `I2cRequest`. Upon
    /// receiving a request, it calls `get_response_from_i2c` to perform the hardware
    /// communication and sends the resulting `I2cResult` back to the caller via the
    /// channel.
    ///
    /// The loop also listens for a `Quit` command on the `rx_from_signal_handler` channel
    /// to enable graceful shutdown. It uses internal lock flags to prevent log flooding
    /// from repeated channel communication errors.
    ///
    /// # Arguments
    /// * `i2c_interface_channels` - A mutable reference to the struct containing the channels.
    pub fn execute(&mut self, i2c_interface_channels: &mut I2cInterfaceChannels) {
        #[cfg(not(test))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let mut lock_error_receive_atlas_scientific = false;
        let mut lock_error_send_atlas_scientific = false;
        let sleep_time = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        // Allocate the read buffer once, here on the stack, using the correct size.
        let mut read_buffer = [0u8; MAX_I2C_RESPONSE_LENGTH];

        loop {
            let (quit_command_received, _, _) = self.process_external_request(
                &i2c_interface_channels.rx_i2c_interface_from_signal_handler,
                None,
            );
            if quit_command_received {
                break;
            }

            match i2c_interface_channels.receive_from_atlas_scientific() {
                Ok(request) => {
                    lock_error_receive_atlas_scientific = false;
                    let response = self.get_response_from_i2c(request, &mut read_buffer);
                    match i2c_interface_channels.send_to_atlas_scientific(response) {
                        Ok(_) => {
                            lock_error_send_atlas_scientific = false;
                        }
                        Err(e) => {
                            if !lock_error_send_atlas_scientific {
                                error!(target: module_path!(), "Error occurred when sending to atlas_scientific {:?}", e);
                                lock_error_send_atlas_scientific = true;
                            }
                        }
                    }
                }
                Err(e) => {
                    match e {
                        TryRecvError::Empty => { /* empty buffer - no action required */ }
                        TryRecvError::Disconnected => {
                            if !lock_error_receive_atlas_scientific {
                                error!(target: module_path!(), "Error occurred when trying to receive from atlas_scientific {:?}", e);
                                lock_error_receive_atlas_scientific = true;
                            }
                        }
                    }
                }
            }
            spin_sleeper.sleep(sleep_time);
        }
    }
}
