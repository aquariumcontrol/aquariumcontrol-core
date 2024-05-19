/* Copyright 2024 Uwe Martin

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

use serde_derive::Deserialize;
use spin_sleep::SpinSleeper;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::channel_content::ActuatorState;
use crate::channel_content::AquariumDevice;
use crate::channel_content::AquariumSignal;
use crate::channel_content::InternalCommand;
use crate::sql_interface_feed::SqlInterfaceFeed;

/// This struct holds the configuration data for the feed control.
/// The configuration is loaded from the .toml configuration file.
/// This struct does not contain any implementation.
#[derive(Deserialize)]
pub struct FeedConfig {
    active: bool,
}

pub struct Feed {
    config: FeedConfig,
    sql_interface_feed: SqlInterfaceFeed,
}

impl Feed {
    pub fn new(config: FeedConfig, sql_interface_feed: SqlInterfaceFeed) -> Feed {
        Self {
            config: config,
            sql_interface_feed: sql_interface_feed,
        }
    }

    pub fn execute(
        &mut self,
        mutex_device_scheduler_feed: Arc<Mutex<i32>>,
        tx_feed_to_controllino: Sender<InternalCommand>,
        rx_feed_from_controllino: Receiver<bool>,
        tx_feed_to_signal_handler: Sender<bool>,
        rx_feed_from_signal_handler: Receiver<InternalCommand>,
    ) {
    }
}
