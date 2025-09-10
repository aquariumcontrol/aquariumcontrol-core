/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use crate::launch::channels::AquaChannelError;
use crate::utilities::channel_content::AquariumSignal;
use std::io;
use std::num::ParseFloatError;
use thiserror::Error;

/// Contains the error definition for communication with the simulator via TCP
#[derive(Debug, Error)]
pub enum TcpCommunicationError {
    /// value indicates that an error has occurred during TCP communication
    #[allow(unused)]
    #[error("[{location}] Failed to connect to simulator at {ip_address}:{port}")]
    ConnectionFailed {
        location: String,
        ip_address: String,
        port: u16,
        #[source]
        source: io::Error,
    },

    /// value indicates that an error has occurred during writing to a TCP stream
    #[error("[{location}] Writing to stream failed.")]
    WritingToStreamFailed {
        location: String,
        #[source]
        source: io::Error,
    },

    /// value indicates that an error has occurred during flushing a TCP stream
    #[error("[{location}] Flush failed.")]
    FlushFailed {
        location: String,

        #[source]
        source: io::Error,
    },

    /// value indicates that an error has occurred during reading from a TCP stream
    #[error("[{location}] Reading from stream failed.")]
    ReadingFromStreamFailed { location: String, source: io::Error },

    /// value indicates that calling function requested an undefined signal
    #[error("[{0}] Internal error - illegal signal request {1} to simulator")]
    IllegalSignalRequestToSimulator(String, AquariumSignal),

    /// value indicates that conversion of response from String to numeric failed
    #[error("[{location}] Failed to convert response ({last_word}) to f32")]
    ResponseConversionError {
        location: String,
        last_word: String,

        #[source]
        source: ParseFloatError,
    },

    /// value indicates that the last word of response is empty
    #[error("[{0}] Last word of response is empty.")]
    LastWordOfResponseEmpty(String),

    /// value indicates that the response does not contain any words
    #[error("[{0}] Response does not contain any words.")]
    ResponseContainsNoWords(String),

    /// value indicates that sending response value via the channel failed
    #[error("[{0}] sending response value via channel failed.")]
    SendingResponseViaChannelFailure(String),

    /// value indicates that channel disconnected unexpectedly
    #[error("[{0}] Error in trying to receive command from data logger")]
    ChannelDisconnected(String),

    /// value indicates that receiving from TCP thread failed
    #[error("{location}: Error occurred when trying to receive response from TCP thread for {requester}.")]
    ReceivingFromTCPThreadFailed {
        location: String,
        requester: String,

        #[source]
        source: AquaChannelError,
    },

    /// value indicates that communication to TCP thread failed
    #[error(
        "{location}: Error occurred when trying to send request to TCP thread for {requester}."
    )]
    SendingToTCPThreadFailed {
        location: String,
        requester: String,

        #[source]
        source: AquaChannelError,
    },

    /// The TCP stream could not be cloned, which is necessary for creating separate
    /// read and write handles.
    #[error("[{location}] Failed to clone the TCP stream.")]
    StreamCloneFailed {
        location: String,
        #[source]
        source: io::Error,
    },
}
