/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

pub(crate) use crate::sensors::i2c_error::I2cError;
use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::{fmt, str};

cfg_if::cfg_if! {
    if #[cfg(all(target_os = "linux", feature = "target_hw"))] {
        use log::error;
        use std::sync::mpsc::{Sender, Receiver, TryRecvError};
        use rppal::i2c::I2c; // required for communication with AtlasScientific sensors

        #[cfg(feature = "debug_i2c_interface")]
        use log::debug;

        use std::time::Duration;
        use crate::utilities::channel_content::InternalCommand;
        use crate::utilities::proc_ext_req::ProcessExternalRequestTrait;
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

pub struct I2cRequest {
    #[allow(unused)]
    /// I2C address of the bus participant
    pub i2c_address: u16,

    #[allow(unused)]
    /// input sequence to be sent to the bus participant
    pub(crate) send_buf: Vec<u8>,

    #[allow(unused)]
    /// sleep time in milliseconds before retrieving an answer from the bus participant
    pub(crate) sleep_time_millis: u64,

    #[allow(unused)]
    /// length of the expected response in byte
    pub(crate) expected_response_length: usize,
}

impl I2cRequest {
    pub fn new(
        i2c_address: u16,
        send_buf: Vec<u8>,
        sleep_time_millis: u64,
        expected_response_length: usize,
    ) -> I2cRequest {
        I2cRequest {
            i2c_address,
            send_buf,
            sleep_time_millis,
            expected_response_length,
        }
    }
}

#[derive(Debug, Clone)]
pub struct I2cResponse {
    /// sequence received from the bus participant
    pub read_buf: Vec<u8>,

    #[cfg(any(test, all(target_os = "linux", feature = "target_hw")))]
    #[allow(unused)]
    /// length of a message as informed by i2c::read
    pub length: usize,
}

impl fmt::Display for I2cResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // descriptive prefix
        write!(f, "I2cResponse [")?;

        // Format the read_buf content
        // Iterate over the bytes and format each as two-digit hexadecimal
        for (i, &byte) in self.read_buf.iter().enumerate() {
            write!(f, "{byte:02x}")?; // :02x ensures two hexadecimal digits, padding with leading zero if necessary
            if i < self.read_buf.len() - 1 {
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

#[cfg(all(target_os = "linux", feature = "target_hw"))]
impl I2cResponse {
    pub fn new(read_buf: Vec<u8>, length: usize) -> I2cResponse {
        I2cResponse { read_buf, length }
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
    /// Creates a new **`I2cInterface`** instance, establishing and configuring the I2C bus for sensor communication.
    ///
    /// This constructor initializes the I2C interface on target hardware. It opens the I2C bus,
    /// sets a global transaction timeout.
    ///
    /// # Arguments
    /// * `config` - **Configuration data** for the I2C interface
    ///
    /// # Returns
    /// A new **`I2cInterface` struct**, ready to communicate with the I2C bus participants.
    ///
    /// # Panics
    /// This function will **panic** if:
    /// - It **fails to open the I2C bus** (e.g., I2C is not enabled, permissions issues).
    /// - It **fails to set the global transaction timeout** for the I2C bus.
    pub fn new(config: I2cInterfaceConfig) -> I2cInterface {
        I2cInterface {
            i2c: {
                let i2c = match I2c::new() {
                    Ok(c) => c,
                    Err(e) => {
                        panic!(
                            "{}: initialization of I2C interface failed ({e:?})",
                            module_path!()
                        );
                    }
                };
                match i2c.set_timeout(config.timeout_transaction) {
                    Ok(_) => i2c,
                    Err(_) => {
                        panic!(
                            "{}: could not set timeout for transactions.",
                            module_path!()
                        );
                    }
                }
            },

            spin_sleeper: SpinSleeper::default(),

            config,
        }
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Communicates with I2C bus participants via request and response
    ///
    /// This function performs a complete I2C communication cycle to retrieve a sensor reading:
    /// 1.  It sets the address of the bus participant which shall receive the request.
    /// 2.  It sends the request buffer to the bus participant.
    /// 3.  It pauses for a specific duration to allow the sensor to process the command.
    /// 4.  It reads the response from the sensor.
    /// 5.  It checks if the response length matches the expected length from the request.
    ///
    /// Errors encountered at any stage (e.g., I2C communication issues or malformed sensor responses)
    /// are logged and returned as `I2cError` variants.
    ///
    /// # Arguments
    /// * `i2c_request` - A struct of type `I2cRequest` containing all details of the request.
    ///
    /// # Returns
    /// A `Result<I2cResponse, I2cError>`:
    /// - `Ok(I2cResponse)`: The successfully read sensor data
    /// - `Err(I2cError::SetAddressError)`: If setting the I2C slave address fails.
    /// - `Err(I2cError::WriteFailure)`: If the command cannot be written to the sensor.
    /// - `Err(I2cError::InsufficientWrite)`: If the command write is incomplete.
    /// - `Err(I2cError::InsufficientRead)`: If the full response cannot be read from the sensor.
    /// - `Err(I2cError::IncorrectResponseLength)`: If the sensor's response message does not have the correct length.
    pub fn get_response_from_i2c(&mut self, i2c_request: I2cRequest) -> I2cResult {
        let sleep_duration = Duration::from_millis(i2c_request.sleep_time_millis);

        match self.i2c.set_slave_address(i2c_request.i2c_address) {
            Ok(_) => { /* do nothing */ }
            Err(e) => {
                error!(
                    target: module_path!(),
                    "could not set {} as address of device ({e:?})",
                    i2c_request.i2c_address
                );
                return Err(I2cError::SetAddressError(i2c_request.i2c_address));
            }
        }

        match self.i2c.write(&i2c_request.send_buf) {
            Ok(realized_write_len) => {
                if realized_write_len != i2c_request.send_buf.len() {
                    error!(
                        target: module_path!(),
                        "could not write complete request to I2C (target: {}, realized: {})",
                        i2c_request.send_buf.len(),
                        realized_write_len
                    );
                    return Err(I2cError::InsufficientWrite);
                }
            }
            Err(e) => {
                error!(
                    target: module_path!(),
                    "writing to I2C device failed {e:?}",
                );
                return Err(I2cError::WriteFailure);
            }
        }

        // give the I2C bus participant time to process the request
        self.spin_sleeper.sleep(sleep_duration);

        let mut read_buffer = vec![0u8; i2c_request.expected_response_length];

        match self.i2c.read(&mut read_buffer) {
            Ok(received_message_length) => {
                if received_message_length != i2c_request.expected_response_length {
                    error!(
                        target: module_path!(),
                        "did not receive full response from I2C bus participant. Expected {}, got {}",
                        i2c_request.expected_response_length,
                        received_message_length
                    );
                    Err(I2cError::IncorrectResponseLength(received_message_length))
                } else {
                    Ok(I2cResponse::new(read_buffer, received_message_length))
                }
            }
            Err(_) => {
                error!(
                    target: module_path!(),
                    "did not receive response from I2C bus participant",
                );
                Err(I2cError::InsufficientRead)
            }
        }
    }

    #[cfg(all(target_os = "linux", feature = "target_hw"))]
    /// Executes the main loop for the I2C interface thread, processing incoming requests and sending sensor data responses.
    ///
    /// This function runs indefinitely, acting as a dedicated service for communicating with
    /// Atlas Scientific I2C sensors. It continuously waits for `InternalCommand` messages
    /// from other modules (like `AtlasScientific` or the `SignalHandler`).
    ///
    /// When a `RequestSignal` command for water temperature, pH, or conductivity is received,
    /// it triggers a full sensor measurement cycle using `self.get_all_data()`. The `Result`
    /// of this measurement (containing all three sensor values or an error) is then sent back
    /// to the requesting module.
    ///
    /// The loop also handles `Quit` commands for graceful shutdown and logs errors or warnings
    /// for invalid commands or channel communication issues, using internal lock flags to prevent log flooding.
    ///
    /// # Arguments
    /// * `tx_to_atlas_scientific` - The sender channel for sending `Result<(f32, f32, f32), I2cError>`
    ///   measurement data back to the `AtlasScientific` module.
    /// * `rx_from_atlas_scientific_or_signal_handler` - The receiver channel for getting `InternalCommand`s
    ///   (e.g., `RequestSignal`, `Quit`) from the `AtlasScientific` module or the `SignalHandler`.
    pub fn execute(
        &mut self,
        tx_to_atlas_scientific: Sender<I2cResult>,
        rx_from_atlas_scientific: Receiver<I2cRequest>,
        rx_from_signal_handler: Receiver<InternalCommand>,
    ) {
        let mut lock_error_receive_atlas_scientific = false;
        let mut lock_error_send_atlas_scientific = false;
        let sleep_time = Duration::from_millis(100);
        let spin_sleeper = SpinSleeper::default();

        loop {
            let (quit_command_received, _, _) =
                self.process_external_request(&rx_from_signal_handler, None);
            if quit_command_received {
                break;
            }

            match rx_from_atlas_scientific.try_recv() {
                Ok(request) => {
                    lock_error_receive_atlas_scientific = false;
                    let response = self.get_response_from_i2c(request);
                    match tx_to_atlas_scientific.send(response) {
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
