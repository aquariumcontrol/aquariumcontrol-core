/* Copyright u2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

//! Provides a centralized, thread-safe interface for I2C communication.
//!
//! This module implements the `I2cInterface` struct, which runs as a dedicated, long-running
//! thread acting as a "hardware service". Its sole purpose is to manage all interactions
//! with the I2C bus, abstracting away the low-level details from the rest of the application.
//!
//! Client modules (like `AtlasScientific`) do not perform I/O operations directly. Instead,
//! they construct an `I2cRequest`, send it to this thread via a channel, and wait for an
//! `I2cResult` to be sent back. This design prevents bus contention and encapsulates all
//! timing-sensitive hardware logic in one place.
//!
//! The entire module is heavily reliant on conditional compilation (`#[cfg]`) to allow the
//! application to be compiled and tested on platforms without I2C hardware (e.g., a
//! developer's machine), where it can run in a simulated mode.
//!
//! ## Key Components
//!
//! - **`I2cInterface` Struct**: The core of the service thread. On target hardware, it holds
//!   the `rppal::i2c::I2c` handle, which represents the physical I2C bus.
//!
//! - **`execute()` Method**: The main thread loop. It continuously listens for incoming
//!   `I2cRequest`s, executes them using `get_response_from_i2c`, and sends the
//!   `I2cResult` back to the original caller. It also handles graceful shutdown signals.
//!
//! - **`get_response_from_i2c()` Method**: The low-level, hardware-facing function that
//!   orchestrates a complete write/read transaction: setting the slave address, writing a
//!   command, pausing, and reading the response.
//!
//! - **`I2cRequest` & `I2cResponse` Structs**: These are the data transfer objects (DTOs)
//!   for the service.
//!   - `I2cRequest`: A message containing all information needed to perform a transaction
//!     (address, command, sleep time, expected response length).
//!   - `I2cResponse`: A message containing the raw data read from the I2C device.
//!
//! ## Design and Architecture
//!
//! - **Centralized I2C Service**: By funneling all I2C traffic through this single thread,
//!   the design eliminates the risk of multiple threads trying to access the bus
//!   simultaneously, which would lead to data corruption or hardware errors.
//!
//! - **Asynchronous Request/Response Model**: The use of `mpsc` channels creates a clean,
//!   asynchronous API. Clients are completely decoupled from the hardware implementation;
//!   they only need to know how to construct a request and process a response.
//!
//! - **Platform Abstraction via Conditional Compilation**: The use of `#[cfg]` is a core
//!   design principle. It allows the application to compile and run on any OS (like a
//!   developer's macOS) for logic testing, as all hardware-specific
//!   code from the `rppal` crate is excluded from the build on non-target platforms.
//!
//! - **Performance and Buffer Management**: The `execute()` loop allocates its read buffer
//!   once on the stack at startup. This buffer is then reused for every transaction,
//!   avoiding repeated, potentially costly memory allocations within the main loop.

use log::error;
#[cfg(all(not(test), not(feature = "test-utils"), target_os = "linux"))]
use log::info;
use std::fmt;
use std::time::Duration;

use crate::launch::channels::AquaChannelError;
use crate::sensors::i2c_error::I2cError;
use crate::sensors::i2c_interface_channels::I2cInterfaceChannels;
use crate::sensors::i2c_interface_config::I2cInterfaceConfig;
use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;

#[cfg(all(not(test), not(feature = "test-utils"), target_os = "linux"))]
// only required in real-life execution
use nix::unistd::gettid;

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
        use rppal::i2c::I2c; // required for communication with AtlasScientific sensors

        #[cfg(feature = "debug_i2c_interface")]
        use log::debug;

        use crate::sensors::i2c_error::I2cError::{InitializationFailure, SetTimeoutError};
    }
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
    pub send_buf: [u8; MAX_I2C_COMMAND_LENGTH],

    #[allow(unused)]
    /// The actual length of the command in the buffer.
    pub send_len: usize,

    #[allow(unused)]
    /// sleep time in milliseconds before retrieving an answer from the bus participant
    pub sleep_time_millis: u64,

    #[allow(unused)]
    /// length of the expected response in byte
    pub expected_response_length: usize,
}

impl I2cRequest {
    /// Creates a new `I2cRequest` after validating the command length.
    ///
    /// This constructor prepares a command for sending over the I2C bus. It takes a
    /// command as a slice, copies it into a fixed-size buffer, and packages it
    /// with the necessary metadata for the I2C interface to execute the request.
    ///
    /// # Arguments
    /// * `_i2c_address` - The 7-bit I2C address of the target device.
    /// * `send_slice` - A byte slice (`&[u8]`) containing the command to be sent.
    /// * `_sleep_time_millis` - The duration in milliseconds to wait after sending the
    ///   command before attempting to read a response.
    /// * `_expected_response_length` - The number of bytes expected in the response from the device.
    ///
    /// # Returns
    /// A `Result` containing a new `I2cRequest` struct on success.
    ///
    /// # Errors
    /// Returns an `I2cError` if the provided command is too large for the buffer:
    /// - `I2cError::CommandTooLong`: If the length of `send_slice` exceeds the
    ///   `MAX_I2C_COMMAND_LENGTH` constant.
    /// * `i2c_address` - A parameter representing `i2c_address`.
    /// * `sleep_time_millis` - A parameter representing `sleep_time_millis`.
    /// * `expected_response_length` - A parameter representing `expected_response_length`.
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
    /// Formats the value using the given formatter.
    ///
    /// # Arguments
    /// * `f` - The formatter to write the formatted string to.
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

    /// Performs the `empty` operation.
    pub fn empty() -> I2cResponse {
        let read_buf = [0u8; MAX_I2C_RESPONSE_LENGTH];
        I2cResponse {
            read_buf,
            length: 0,
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

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Read buffer
    read_buffer: [u8; MAX_I2C_RESPONSE_LENGTH],
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

        let read_buffer = [0u8; MAX_I2C_RESPONSE_LENGTH];

        Ok(I2cInterface {
            i2c,
            config,
            read_buffer,
        })
    }

    #[cfg(not(all(target_os = "linux", feature = "target_hw")))]
    /// Stub function for testing async runtime
    pub fn new(config: I2cInterfaceConfig) -> Result<I2cInterface, I2cError> {
        Ok(I2cInterface { config })
    }

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
    pub async fn get_response_from_i2c(
        &mut self,
        #[allow(unused)] // not used for async runtime testing
        i2c_request: I2cRequest,
    ) -> I2cResult {
        #[cfg(all(target_os = "linux", feature = "target_hw"))]
        {
            let sleep_duration = Duration::from_millis(i2c_request.sleep_time_millis);

            // Check the buffer's capacity against the request's needs.
            if self.read_buffer.len() < i2c_request.expected_response_length {
                return Err(I2cError::ProvidedBufferTooSmall {
                    location: module_path!().to_string(),
                    buffer_size: self.read_buffer.len(),
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

            let realized_write_len = tokio::task::block_in_place(|| {
                self.i2c
                    .write(&i2c_request.send_buf[..i2c_request.send_len])
            })
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
            tokio::time::sleep(sleep_duration).await;

            if i2c_request.expected_response_length > 0 {
                // Use a sub-slice of the provided buffer for the read operation.
                let read_slice = &mut self.read_buffer[..i2c_request.expected_response_length];

                let received_message_length = tokio::task::block_in_place(|| {
                    self.i2c.read(read_slice)
                })
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
            } else {
                Ok(I2cResponse::empty())
            }
        }
        #[cfg(not(all(target_os = "linux", feature = "target_hw")))]
        {
            // dummy result for testing async runtime
            Ok(I2cResponse::empty())
        }
    }

    /// Executes the main loop for the I2C interface thread, acting as a request/response service.
    ///
    /// This function runs indefinitely, processing I2C requests received from other modules.
    /// It listens to the `rx_i2c_interface_from_atlas_scientific` channel for an `I2cRequest`. Upon
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
    pub async fn execute(&mut self, i2c_interface_channels: &mut I2cInterfaceChannels) {
        #[cfg(all(not(test), not(feature = "test-utils"), target_os = "linux"))] // only required in real-life execution
        #[cfg(not(tarpaulin_include))]
        info!(target: module_path!(), "Thread started with TID: {}", gettid());

        let mut lock_error_receive_atlas_scientific = false;
        let mut lock_error_send_atlas_scientific = false;
        let sleep_time = Duration::from_millis(100);

        loop {
            let (quit_command_received, _, _) = self.process_external_request(
                &mut i2c_interface_channels.rx_i2c_interface_from_signal_handler,
                None,
            );
            if quit_command_received {
                break;
            }

            match i2c_interface_channels
                .rx_i2c_interface_from_atlas_scientific
                .try_recv()
            {
                Ok(request) => {
                    lock_error_receive_atlas_scientific = false;
                    let response = self.get_response_from_i2c(request).await;
                    match i2c_interface_channels
                        .tx_i2c_interface_to_atlas_scientific
                        .send(response)
                    {
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
                        #[cfg(feature = "debug_channels")]
                        AquaChannelError::Full => { /* Not applicable. Do nothing */ }
                        AquaChannelError::Empty => { /* empty buffer - no action required */ }
                        AquaChannelError::Disconnected => {
                            if !lock_error_receive_atlas_scientific {
                                error!(target: module_path!(), "Error occurred when trying to receive from atlas_scientific {:?}", e);
                                lock_error_receive_atlas_scientific = true;
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(sleep_time).await;
        }
    }
}

#[cfg(test)]
#[cfg(not(tarpaulin_include))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub mod tests {
    use crate::launch::channels::channel;
    use crate::sensors::i2c_error::I2cError;
    use crate::sensors::i2c_interface::{
        I2cInterface, I2cRequest, I2cResponse, MAX_I2C_COMMAND_LENGTH,
    };
    use crate::sensors::i2c_interface_channels::I2cInterfaceChannels;
    use crate::sensors::i2c_interface_config::I2cInterfaceConfig;
    use crate::utilities::channel_content::InternalCommand;

    #[test]
    // Verifies that I2cRequest::new returns CommandTooLong when the input slice exceeds MAX_I2C_COMMAND_LENGTH.
    fn test_i2c_request_new_command_too_long() {
        let long_slice = [0u8; MAX_I2C_COMMAND_LENGTH + 1];
        let res = I2cRequest::new(0x66, &long_slice, 10, 4);
        assert!(matches!(res, Err(I2cError::CommandTooLong { .. })));
    }

    #[test]
    // Verifies I2cResponse Display formatting for non-empty and empty buffers.
    fn test_i2c_response_fmt() {
        let data = [0x12, 0x34, 0xab];
        let resp = I2cResponse::new(&data);
        let formatted = format!("{resp}");
        assert!(formatted.contains("12 34 ab"));

        let empty_resp = I2cResponse::empty();
        let formatted_empty = format!("{empty_resp}");
        assert!(formatted_empty.contains("I2cResponse"));
    }

    #[tokio::test]
    // Verifies execute loop handles disconnected channels gracefully.
    async fn test_execute_channel_disconnections() {
        use crate::sensors::i2c_interface::I2cRequest;

        let config = I2cInterfaceConfig {
            execute: true,
            max_read_attempts: 3,
            timeout_transaction: 100,
        };
        let mut interface = I2cInterface::new(config).unwrap();

        let (mut tx_sig, rx_sig) = channel::<InternalCommand>(10, false);
        let (_tx_atlas, rx_atlas) = channel::<I2cRequest>(10, false);
        let (tx_res, _rx_res) = channel(10, false);

        let mut channels = I2cInterfaceChannels {
            tx_i2c_interface_to_atlas_scientific: tx_res,
            rx_i2c_interface_from_atlas_scientific: rx_atlas,
            rx_i2c_interface_from_signal_handler: rx_sig,
        };

        // Send quit command to cleanly exit execute loop
        tx_sig.send(InternalCommand::Quit).unwrap();
        interface.execute(&mut channels).await;
    }

    #[tokio::test]
    // Verifies get_response_from_i2c executes transaction and returns I2cResult.
    async fn test_get_response_from_i2c_direct() {
        let config = I2cInterfaceConfig {
            execute: true,
            max_read_attempts: 3,
            timeout_transaction: 100,
        };
        let mut interface = I2cInterface::new(config).unwrap();
        let request = I2cRequest::new(0x63, b"R", 10, 5).unwrap();

        let res = interface.get_response_from_i2c(request).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    // Verifies execute loop processes incoming I2cRequest and sends I2cResult response to channel.
    async fn test_execute_processes_i2c_request_success() {
        let config = I2cInterfaceConfig {
            execute: true,
            max_read_attempts: 3,
            timeout_transaction: 100,
        };
        let mut interface = I2cInterface::new(config).unwrap();

        let (mut tx_sig, rx_sig) = channel::<InternalCommand>(10, false);
        let (mut tx_req, rx_req) = channel::<I2cRequest>(10, false);
        let (tx_resp, mut rx_resp) = channel(10, false);

        let mut channels = I2cInterfaceChannels {
            tx_i2c_interface_to_atlas_scientific: tx_resp,
            rx_i2c_interface_from_atlas_scientific: rx_req,
            rx_i2c_interface_from_signal_handler: rx_sig,
        };

        let request = I2cRequest::new(0x63, b"R", 10, 5).unwrap();
        tx_req.send(request).unwrap();

        let handle = tokio::spawn(async move {
            interface.execute(&mut channels).await;
        });

        let response = rx_resp.recv().await;
        assert!(response.is_ok());

        tx_sig.send(InternalCommand::Quit).unwrap();
        let _ = handle.await;
    }

    #[tokio::test]
    // Verifies execute loop handles disconnected response channel and disconnected request channel.
    async fn test_execute_handles_send_failure_and_recv_disconnected() {
        let config = I2cInterfaceConfig {
            execute: true,
            max_read_attempts: 3,
            timeout_transaction: 100,
        };
        let mut interface = I2cInterface::new(config).unwrap();

        let (mut tx_sig, rx_sig) = channel::<InternalCommand>(10, false);
        let (mut tx_req, rx_req) = channel::<I2cRequest>(10, false);
        let (tx_resp, rx_resp) = channel(10, false);

        drop(rx_resp);

        let mut channels = I2cInterfaceChannels {
            tx_i2c_interface_to_atlas_scientific: tx_resp,
            rx_i2c_interface_from_atlas_scientific: rx_req,
            rx_i2c_interface_from_signal_handler: rx_sig,
        };

        let request = I2cRequest::new(0x63, b"R", 10, 5).unwrap();
        tx_req.send(request).unwrap();
        drop(tx_req);
        tx_sig.send(InternalCommand::Quit).unwrap();

        interface.execute(&mut channels).await;
    }
}

