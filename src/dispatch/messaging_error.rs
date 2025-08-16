/* Copyright 2025 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/
use thiserror::Error;

#[derive(Debug, Error)]
/// Enum codifies the errors which the program may encounter in communication with the Linux messaging system.
pub enum MessagingError {
    /// Error occurred when trying to open POSIX message queue.
    #[cfg(target_os = "linux")]
    #[error("[{location}] Error occurred when trying to open POSIX message queue using message queue name {mq_name}.")]
    PosixMessageQueueOpeningError {
        location: String,
        mq_name: String,
        #[source]
        source: std::io::Error,
    },

    /// Error occurred when trying to access POSIX message queue attributes.
    #[cfg(target_os = "linux")]
    #[error("[{location}] Error occurred when trying to access POSIX message queue attributes.")]
    PosixMessageQueueAttributesError {
        location: String,
        #[source]
        source: std::io::Error,
    },
}
