/* Copyright 2024 Ashley Kupke
   Copyright 2025-2026 Uwe Martin
Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use std::{
    fmt::Debug,
    fs::File,
    io::{self, Write},
    sync::{
        mpsc::{SendError, Sender},
        Mutex,
    },
};

#[derive(Debug)]
pub enum SenderLoggerErr<T> {
    Io(io::Error),
    Send(SendError<T>),
}

#[derive(Clone)]
pub struct SenderLogger<'a, T: Debug>(&'a Mutex<File>, &'a Sender<T>);

impl<'a, T: Debug> SenderLogger<'a, T> {
    /// (You shouldn't use this function manually; use `generate_sender_logger_group!`)
    pub fn new(file: &'a Mutex<File>, sender: &'a Sender<T>) -> Self {
        Self(file, sender)
    }

    pub fn send_and_log(&self, t: T) -> Result<(), SenderLoggerErr<T>> {
        let mut guard = self.0.lock().unwrap();
        guard
            .write_all(format!("{t:?}\n").as_bytes())
            .map_err(|e| SenderLoggerErr::Io(e))?;
        drop(guard); //avoid deadlock
        self.1.send(t).map_err(|e| SenderLoggerErr::Send(e))?;
        Ok(())
    }
}

/// Generate a struct that groups multiple SenderLoggers to one file. The first parameter is the name of the generated struct, and all remaining parameters are generics, which can be named arbitrarily.
/// # Examples
/// ```
/// use aquarium_control::generate_sender_logger_group;
/// use std::sync::mpsc;
///
/// // Generates logger of two Senders
/// generate_sender_logger_group!(SenderLoggerTuple, A, B);
///
/// // Makes a logger for three Senders
/// generate_sender_logger_group!(SenderLoggerTriple, A, B, C);
/// // And so on; you can make a logger for as many Senders as you could possibly need
///
/// // You can also use just one sender, if desired
/// generate_sender_logger_group!(SenderLoggerSingle, A);
///
/// // Let us use these loggers:
///
/// // Generate a tuple
/// let (tx0, _rx) = mpsc::channel();
/// let (tx1, _rx) = mpsc::channel();
///
/// let tuple = SenderLoggerTuple::new("/tmp/aquarium_test_server__doctest__generate_sender_logger_group__tuple.log", tx0, tx1).unwrap();
/// let (sender0, sender1) = tuple.sender_loggers();
///
/// // Logs the supplied string to the log file, as well as sending it across the corresponding channel
/// sender0.send_and_log("This string will be logged to a file").unwrap();
/// // Logs "56" to the log file, as well as sending the value across the corresponding channel
/// sender1.send_and_log(56).unwrap();
///
/// // Makes a triple
/// let (tx0, _rx) = mpsc::channel::<i32>();
/// let (tx1, _rx) = mpsc::channel::<i32>();
/// let (tx2, _rx) = mpsc::channel::<i32>();
///
/// let triple = SenderLoggerTriple::new("/tmp/aquarium_test_server__doctest__generate_sender_logger_group__triple.log", tx0, tx1, tx2).unwrap();
/// let (_sender0, _sender1, _sender2) = triple.sender_loggers();
/// // ... and so forth
///
/// // Makes a single SenderLogger.
/// let (tx0, _rx) = mpsc::channel::<i32>();
/// let single = SenderLoggerSingle::new("/tmp/aquarium_test_server__doctest__generate_sender_logger_group__single.log", tx0).unwrap();
/// let _sender0 = single.sender_loggers();
///
/// ```
#[macro_export]
macro_rules! generate_sender_logger_group {
    ($name:ident, $($t:ident),+) => {
        #[allow(non_snake_case)]
        pub struct $name <$($t:std::fmt::Debug),+> {
            file: std::sync::Mutex<std::fs::File>,
            $($t: std::sync::mpsc::Sender<$t>),+
        }

        impl <$($t:std::fmt::Debug),+> $name <$($t),+> {
            #[allow(non_snake_case)]
            pub fn new(
                path: impl AsRef<std::path::Path>,
                $($t: std::sync::mpsc::Sender<$t>),+,
            ) -> Result<Self, std::io::Error> {
                let file = std::sync::Mutex::new(std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(path)?);
                Ok(Self {
                    file,
                    $($t),+,
                })
            }

            #[allow(unused_parens)]
            pub fn sender_loggers<'a>(&'a self) -> ($($crate::test_server::logger::SenderLogger<'a, $t>),+) {
                ($($crate::test_server::logger::SenderLogger::new(&self.file, &self.$t)),+)
            }
        }
    };
}
